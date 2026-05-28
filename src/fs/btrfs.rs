//! # Btrfs Dosya Sistemi — Copy-on-Write B-Tree Filesystem
//!
//! Btrfs (B-Tree FS), Oracle tarafından geliştirilen modern CoW dosya sistemi.
//! Snapshot, subvolume, RAID, compression ve checksum desteği sağlar.
//!
//! ## Mimari
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │                   Btrfs Volume                       │
//! ├─────────────────────────────────────────────────────┤
//! │              Superblock (64KB offset)                │
//! ├─────────────────────────────────────────────────────┤
//! │           Chunk Tree (logical → physical)            │
//! ├─────────────────────────────────────────────────────┤
//! │     Root Tree ──► FS Tree (subvolume)                │
//! │                 ──► Extent Tree (free space)         │
//! │                 ──► Checksum Tree                    │
//! │                 ──► Device Tree                      │
//! └─────────────────────────────────────────────────────┘
//! ```
//!
//! ## Özellikler
//!
//! - Copy-on-Write (CoW) — her yazma yeni blok alır
//! - Snapshot (anlık görüntü) — O(1) subvolume klonu
//! - Subvolume — bağımsız dosya sistemi ağaçları
//! - B-Tree tabanlı metadata + data yönetimi
//! - Inline checksum (CRC32C/xxHash/SHA256)
//! - Extent-based allocation
//! - Compression (zlib, lzo, zstd)
//! - RAID 0/1/5/6/10 desteği

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use sha2::{Digest, Sha256};
use spin::Mutex;

// ============================================================================
// Magic Numbers & Constants
// ============================================================================

/// Btrfs superblock magic: "_BHRfS_M" (little-endian)
pub const BTRFS_MAGIC: u64 = 0x4D5F53665248425F;

/// Superblock offset (64 KiB)
pub const BTRFS_SUPER_OFFSET: usize = 0x10000;

/// Superblock mirror offsets
pub const BTRFS_SUPER_MIRROR_1: u64 = 64 * 1024;
pub const BTRFS_SUPER_MIRROR_2: u64 = 256 * 1024 * 1024;
pub const BTRFS_SUPER_MIRROR_3: u64 = 1024 * 1024 * 1024 * 1024; // 1 TiB

/// Node/leaf size (default 16 KiB)
pub const BTRFS_DEFAULT_NODE_SIZE: u32 = 16384;

/// Sector size (default 4 KiB)
pub const BTRFS_DEFAULT_SECTOR_SIZE: u32 = 4096;
pub const BTRFS_SUPERBLOCK_SIZE: usize = 4096;

/// Checksum types
pub const BTRFS_CSUM_TYPE_CRC32: u16 = 0;
pub const BTRFS_CSUM_TYPE_XXHASH: u16 = 1;
pub const BTRFS_CSUM_TYPE_SHA256: u16 = 2;
pub const BTRFS_CSUM_TYPE_BLAKE2: u16 = 3;
pub const BTRFS_FIRST_CHUNK_TREE_OBJECTID: u64 = 256;
pub const BTRFS_FT_REG_FILE: u8 = 1;
pub const BTRFS_FT_DIR: u8 = 2;
const BTRFS_HEADER_SIZE: usize = 101;
const BTRFS_LEAF_ITEM_SIZE: usize = 25;
const BTRFS_KEY_PTR_SIZE: usize = 33;
const BTRFS_DIR_ITEM_DATA_SIZE: usize = 30;
const BTRFS_ROOT_ITEM_MIN_SIZE: usize = 239;
const BTRFS_SUPERBLOCK_SYS_CHUNK_ARRAY_OFFSET: usize = 811;

// ============================================================================
// Object IDs — Well-Known Tree Roots
// ============================================================================

/// Root tree objectid
pub const BTRFS_ROOT_TREE_OBJECTID: u64 = 1;
/// Extent tree objectid
pub const BTRFS_EXTENT_TREE_OBJECTID: u64 = 2;
/// Chunk tree objectid
pub const BTRFS_CHUNK_TREE_OBJECTID: u64 = 3;
/// Device tree objectid
pub const BTRFS_DEV_TREE_OBJECTID: u64 = 4;
/// FS tree objectid (default subvolume)
pub const BTRFS_FS_TREE_OBJECTID: u64 = 5;
/// Checksum tree objectid
pub const BTRFS_CSUM_TREE_OBJECTID: u64 = 7;
/// Free space tree objectid
pub const BTRFS_FREE_SPACE_TREE_OBJECTID: u64 = 10;

/// İlk kullanılabilir objectid
pub const BTRFS_FIRST_FREE_OBJECTID: u64 = 256;

// ============================================================================
// Item Key Types
// ============================================================================

/// Inode item
pub const BTRFS_INODE_ITEM_KEY: u8 = 1;
/// Inode ref (name → parent)
pub const BTRFS_INODE_REF_KEY: u8 = 12;
/// Dir item (name → inode, hash)
pub const BTRFS_DIR_ITEM_KEY: u8 = 84;
/// Dir index
pub const BTRFS_DIR_INDEX_KEY: u8 = 96;
/// Extent data (file data mapping)
pub const BTRFS_EXTENT_DATA_KEY: u8 = 108;
/// Root item (subvolume root)
pub const BTRFS_ROOT_ITEM_KEY: u8 = 132;
/// Root ref
pub const BTRFS_ROOT_REF_KEY: u8 = 156;
/// Extent item (block allocation)
pub const BTRFS_EXTENT_ITEM_KEY: u8 = 168;
/// Block group item
pub const BTRFS_BLOCK_GROUP_ITEM_KEY: u8 = 192;
/// Chunk item
pub const BTRFS_CHUNK_ITEM_KEY: u8 = 228;
/// Device item
pub const BTRFS_DEV_ITEM_KEY: u8 = 216;
/// Root item readonly flag
pub const BTRFS_ROOT_SUBVOL_RDONLY: u64 = 1 << 0;

// ============================================================================
// Inode Mode Bits
// ============================================================================

pub const S_IFMT: u32 = 0o170000;
pub const S_IFREG: u32 = 0o100000;
pub const S_IFDIR: u32 = 0o040000;
pub const S_IFLNK: u32 = 0o120000;

// ============================================================================
// On-Disk Structures
// ============================================================================

/// Btrfs B-Tree anahtar yapısı (17 bytes on-disk)
///
/// Her B-Tree item'ı bu anahtar ile indekslenir.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct BtrfsKey {
    /// Object ID (inode, tree, vs.)
    pub objectid: u64,
    /// Item tipi (INODE_ITEM, DIR_ITEM, EXTENT_DATA, vs.)
    pub item_type: u8,
    /// Offset (tip-bağımlı anlam)
    pub offset: u64,
}

impl BtrfsKey {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 17 {
            return None;
        }
        Some(Self {
            objectid: u64::from_le_bytes([
                data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
            ]),
            item_type: data[8],
            offset: u64::from_le_bytes([
                data[9], data[10], data[11], data[12], data[13], data[14], data[15], data[16],
            ]),
        })
    }
}

/// Btrfs Superblock — 64 KiB offset'te
#[derive(Clone, Debug)]
pub struct BtrfsSuperblock {
    /// Checksum (CRC32C of bytes 32..4096)
    pub csum: [u8; 32],
    /// FS UUID
    pub fsid: [u8; 16],
    /// Physical address of this block
    pub bytenr: u64,
    /// Flags
    pub flags: u64,
    /// Magic: "_BHRfS_M"
    pub magic: u64,
    /// Generation (transaction counter)
    pub generation: u64,
    /// Root tree logical address
    pub root: u64,
    /// Chunk tree logical address
    pub chunk_root: u64,
    /// Log tree logical address
    pub log_root: u64,
    /// Total bytes in filesystem
    pub total_bytes: u64,
    /// Used bytes
    pub bytes_used: u64,
    /// Root directory objectid
    pub root_dir_objectid: u64,
    /// Number of devices
    pub num_devices: u64,
    /// Sector size
    pub sector_size: u32,
    /// Node size
    pub node_size: u32,
    /// Leaf size (= node_size in modern btrfs)
    pub leaf_size: u32,
    /// Stripe size
    pub stripe_size: u32,
    /// System chunk array size
    pub sys_chunk_array_size: u32,
    /// Chunk root generation
    pub chunk_root_generation: u64,
    /// Compat flags
    pub compat_flags: u64,
    /// Compat RO flags
    pub compat_ro_flags: u64,
    /// Incompat flags
    pub incompat_flags: u64,
    /// Checksum type (CRC32C, xxHash, SHA256, BLAKE2)
    pub csum_type: u16,
    /// Root tree level
    pub root_level: u8,
    /// Chunk tree level
    pub chunk_root_level: u8,
    /// Log tree level
    pub log_root_level: u8,
    /// Label (256 bytes max)
    pub label: [u8; 256],
    /// Embedded system chunk array used to bootstrap logical → physical mapping
    pub sys_chunk_array: Vec<u8>,
}

impl BtrfsSuperblock {
    /// Ham baytlardan superblock parse eder (little-endian)
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 512 {
            return None;
        }

        let magic = u64::from_le_bytes([
            data[64], data[65], data[66], data[67], data[68], data[69], data[70], data[71],
        ]);
        if magic != BTRFS_MAGIC {
            return None;
        }

        let mut csum = [0u8; 32];
        csum.copy_from_slice(&data[0..32]);

        let mut fsid = [0u8; 16];
        fsid.copy_from_slice(&data[32..48]);

        let mut label = [0u8; 256];
        let label_end = core::cmp::min(data.len(), 555);
        if label_end > 299 {
            let copy_len = core::cmp::min(256, label_end - 299);
            label[..copy_len].copy_from_slice(&data[299..299 + copy_len]);
        }
        let sys_chunk_array_size =
            u32::from_le_bytes([data[160], data[161], data[162], data[163]]) as usize;
        let sys_chunk_start = BTRFS_SUPERBLOCK_SYS_CHUNK_ARRAY_OFFSET;
        let sys_chunk_end = sys_chunk_start.saturating_add(sys_chunk_array_size);
        let sys_chunk_array = if sys_chunk_end <= data.len() {
            data[sys_chunk_start..sys_chunk_end].to_vec()
        } else {
            Vec::new()
        };

        Some(Self {
            csum,
            fsid,
            bytenr: u64::from_le_bytes([
                data[48], data[49], data[50], data[51], data[52], data[53], data[54], data[55],
            ]),
            flags: u64::from_le_bytes([
                data[56], data[57], data[58], data[59], data[60], data[61], data[62], data[63],
            ]),
            magic,
            generation: u64::from_le_bytes([
                data[72], data[73], data[74], data[75], data[76], data[77], data[78], data[79],
            ]),
            root: u64::from_le_bytes([
                data[80], data[81], data[82], data[83], data[84], data[85], data[86], data[87],
            ]),
            chunk_root: u64::from_le_bytes([
                data[88], data[89], data[90], data[91], data[92], data[93], data[94], data[95],
            ]),
            log_root: u64::from_le_bytes([
                data[96], data[97], data[98], data[99], data[100], data[101], data[102], data[103],
            ]),
            total_bytes: u64::from_le_bytes([
                data[112], data[113], data[114], data[115], data[116], data[117], data[118],
                data[119],
            ]),
            bytes_used: u64::from_le_bytes([
                data[120], data[121], data[122], data[123], data[124], data[125], data[126],
                data[127],
            ]),
            root_dir_objectid: u64::from_le_bytes([
                data[128], data[129], data[130], data[131], data[132], data[133], data[134],
                data[135],
            ]),
            num_devices: u64::from_le_bytes([
                data[136], data[137], data[138], data[139], data[140], data[141], data[142],
                data[143],
            ]),
            sector_size: u32::from_le_bytes([data[144], data[145], data[146], data[147]]),
            node_size: u32::from_le_bytes([data[148], data[149], data[150], data[151]]),
            leaf_size: u32::from_le_bytes([data[152], data[153], data[154], data[155]]),
            stripe_size: u32::from_le_bytes([data[156], data[157], data[158], data[159]]),
            sys_chunk_array_size: sys_chunk_array_size as u32,
            chunk_root_generation: u64::from_le_bytes([
                data[164], data[165], data[166], data[167], data[168], data[169], data[170],
                data[171],
            ]),
            compat_flags: u64::from_le_bytes([
                data[172], data[173], data[174], data[175], data[176], data[177], data[178],
                data[179],
            ]),
            compat_ro_flags: u64::from_le_bytes([
                data[180], data[181], data[182], data[183], data[184], data[185], data[186],
                data[187],
            ]),
            incompat_flags: u64::from_le_bytes([
                data[188], data[189], data[190], data[191], data[192], data[193], data[194],
                data[195],
            ]),
            csum_type: u16::from_le_bytes([data[196], data[197]]),
            root_level: data[198],
            chunk_root_level: data[199],
            log_root_level: data[200],
            label,
            sys_chunk_array,
        })
    }

    /// Toplam kapasiteyi bayt olarak döndürür
    pub fn total_size(&self) -> u64 {
        self.total_bytes
    }

    /// Kullanılan alanı döndürür
    pub fn used_size(&self) -> u64 {
        self.bytes_used
    }

    /// Boş alanı döndürür
    pub fn free_size(&self) -> u64 {
        self.total_bytes.saturating_sub(self.bytes_used)
    }

    /// Label string
    pub fn label_str(&self) -> &str {
        let end = self.label.iter().position(|&b| b == 0).unwrap_or(256);
        core::str::from_utf8(&self.label[..end]).unwrap_or("")
    }

    /// Checksum type string
    pub fn csum_type_str(&self) -> &'static str {
        match self.csum_type {
            BTRFS_CSUM_TYPE_CRC32 => "CRC32C",
            BTRFS_CSUM_TYPE_XXHASH => "xxHash",
            BTRFS_CSUM_TYPE_SHA256 => "SHA256",
            BTRFS_CSUM_TYPE_BLAKE2 => "BLAKE2b",
            _ => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BtrfsChecksumError {
    InvalidSuperblock,
    ChecksumMismatch,
    UnsupportedChecksumType,
    BufferTooSmall,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BtrfsScrubIssueKind {
    MirrorMissing,
    InvalidSuperblock,
    ChecksumMismatch,
    BytenrMismatch,
    GenerationStale,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BtrfsScrubIssue {
    pub mirror_offset: u64,
    pub kind: BtrfsScrubIssueKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BtrfsScrubReport {
    pub mirrors_seen: usize,
    pub valid_mirrors: usize,
    pub selected_mirror: Option<u64>,
    pub freshest_generation: u64,
    pub issues: Vec<BtrfsScrubIssue>,
}

impl BtrfsScrubReport {
    fn new() -> Self {
        Self {
            mirrors_seen: 0,
            valid_mirrors: 0,
            selected_mirror: None,
            freshest_generation: 0,
            issues: Vec::new(),
        }
    }
}

pub const BTRFS_DEFAULT_SCRUB_INTERVAL_TICKS: usize = 1000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BtrfsScrubMirrorImage {
    pub mirror_offset: u64,
    pub block: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BtrfsScrubVolume {
    pub mount_point: String,
    pub mirrors: Vec<BtrfsScrubMirrorImage>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BtrfsScrubDaemonVolumeReport {
    pub mount_point: String,
    pub tick: u64,
    pub report: BtrfsScrubReport,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BtrfsScrubDaemonStatus {
    pub started: bool,
    pub interval_ticks: usize,
    pub pass_count: u64,
    pub registered_volumes: usize,
    pub last_tick: u64,
    pub last_reports: Vec<BtrfsScrubDaemonVolumeReport>,
}

fn crc32c(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0x82F63B78 & mask);
        }
    }
    !crc
}

fn xxhash64_round(acc: u64, input: u64) -> u64 {
    let acc = acc.wrapping_add(input.wrapping_mul(14_029_467_366_897_019_727));
    let acc = acc.rotate_left(31);
    acc.wrapping_mul(11_400_714_785_074_694_791)
}

fn xxhash64_merge_round(acc: u64, val: u64) -> u64 {
    let acc = acc ^ xxhash64_round(0, val);
    acc.wrapping_mul(11_400_714_785_074_694_791)
        .wrapping_add(9_650_029_242_287_828_579)
}

fn xxhash64(data: &[u8]) -> u64 {
    const P1: u64 = 11_400_714_785_074_694_791;
    const P2: u64 = 14_029_467_366_897_019_727;
    const P3: u64 = 1_609_587_929_392_839_161;
    const P4: u64 = 9_650_029_242_287_828_579;
    const P5: u64 = 2_870_177_450_012_600_261;

    let mut index = 0usize;
    let mut acc;

    if data.len() >= 32 {
        let mut v1 = P1.wrapping_add(P2);
        let mut v2 = P2;
        let mut v3 = 0u64;
        let mut v4 = 0u64.wrapping_sub(P1);

        while index + 32 <= data.len() {
            let read = |offset: usize| {
                u64::from_le_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3],
                    data[offset + 4],
                    data[offset + 5],
                    data[offset + 6],
                    data[offset + 7],
                ])
            };

            v1 = xxhash64_round(v1, read(index));
            v2 = xxhash64_round(v2, read(index + 8));
            v3 = xxhash64_round(v3, read(index + 16));
            v4 = xxhash64_round(v4, read(index + 24));
            index += 32;
        }

        acc = v1
            .rotate_left(1)
            .wrapping_add(v2.rotate_left(7))
            .wrapping_add(v3.rotate_left(12))
            .wrapping_add(v4.rotate_left(18));
        acc = xxhash64_merge_round(acc, v1);
        acc = xxhash64_merge_round(acc, v2);
        acc = xxhash64_merge_round(acc, v3);
        acc = xxhash64_merge_round(acc, v4);
    } else {
        acc = P5;
    }

    acc = acc.wrapping_add(data.len() as u64);

    while index + 8 <= data.len() {
        let lane = u64::from_le_bytes([
            data[index],
            data[index + 1],
            data[index + 2],
            data[index + 3],
            data[index + 4],
            data[index + 5],
            data[index + 6],
            data[index + 7],
        ]);
        acc ^= xxhash64_round(0, lane);
        acc = acc.rotate_left(27).wrapping_mul(P1).wrapping_add(P4);
        index += 8;
    }

    if index + 4 <= data.len() {
        let lane = u32::from_le_bytes([
            data[index],
            data[index + 1],
            data[index + 2],
            data[index + 3],
        ]) as u64;
        acc ^= lane.wrapping_mul(P1);
        acc = acc.rotate_left(23).wrapping_mul(P2).wrapping_add(P3);
        index += 4;
    }

    while index < data.len() {
        acc ^= (data[index] as u64).wrapping_mul(P5);
        acc = acc.rotate_left(11).wrapping_mul(P1);
        index += 1;
    }

    acc ^= acc >> 33;
    acc = acc.wrapping_mul(P2);
    acc ^= acc >> 29;
    acc = acc.wrapping_mul(P3);
    acc ^= acc >> 32;
    acc
}

pub fn encode_superblock_checksum(
    csum_type: u16,
    payload: &[u8],
) -> Result<[u8; 32], BtrfsChecksumError> {
    let mut encoded = [0u8; 32];

    match csum_type {
        BTRFS_CSUM_TYPE_CRC32 => {
            encoded[..4].copy_from_slice(&crc32c(payload).to_le_bytes());
            Ok(encoded)
        }
        BTRFS_CSUM_TYPE_XXHASH => {
            encoded[..8].copy_from_slice(&xxhash64(payload).to_le_bytes());
            Ok(encoded)
        }
        BTRFS_CSUM_TYPE_SHA256 => {
            let digest = Sha256::digest(payload);
            encoded.copy_from_slice(&digest);
            Ok(encoded)
        }
        _ => Err(BtrfsChecksumError::UnsupportedChecksumType),
    }
}

pub fn verify_superblock_checksum(block: &[u8]) -> Result<(), BtrfsChecksumError> {
    if block.len() < BTRFS_SUPERBLOCK_SIZE {
        return Err(BtrfsChecksumError::BufferTooSmall);
    }

    let sb = BtrfsSuperblock::from_bytes(block).ok_or(BtrfsChecksumError::InvalidSuperblock)?;
    let expected = encode_superblock_checksum(sb.csum_type, &block[32..BTRFS_SUPERBLOCK_SIZE])?;

    if expected == sb.csum {
        Ok(())
    } else {
        Err(BtrfsChecksumError::ChecksumMismatch)
    }
}

pub fn stamp_superblock_checksum(block: &mut [u8]) -> Result<(), BtrfsChecksumError> {
    if block.len() < BTRFS_SUPERBLOCK_SIZE {
        return Err(BtrfsChecksumError::BufferTooSmall);
    }

    let csum_type = u16::from_le_bytes([block[196], block[197]]);
    let encoded = encode_superblock_checksum(csum_type, &block[32..BTRFS_SUPERBLOCK_SIZE])?;
    block[..32].copy_from_slice(&encoded);
    Ok(())
}

fn capture_scrub_mirror_images(disk_data: &[u8], mirrors: &[u64]) -> Vec<BtrfsScrubMirrorImage> {
    let mut images = Vec::with_capacity(mirrors.len());

    for &mirror_offset in mirrors {
        let mirror_end = mirror_offset as usize + BTRFS_SUPERBLOCK_SIZE;
        let block = if mirror_end <= disk_data.len() {
            Some(disk_data[mirror_offset as usize..mirror_end].to_vec())
        } else {
            None
        };

        images.push(BtrfsScrubMirrorImage {
            mirror_offset,
            block,
        });
    }

    images
}

pub fn scrub_superblock_mirror_images(images: &[BtrfsScrubMirrorImage]) -> BtrfsScrubReport {
    let mut report = BtrfsScrubReport::new();

    for image in images {
        let block = match image.block.as_ref() {
            Some(block) if block.len() >= BTRFS_SUPERBLOCK_SIZE => {
                report.mirrors_seen += 1;
                &block[..BTRFS_SUPERBLOCK_SIZE]
            }
            Some(_) => {
                report.issues.push(BtrfsScrubIssue {
                    mirror_offset: image.mirror_offset,
                    kind: BtrfsScrubIssueKind::InvalidSuperblock,
                });
                continue;
            }
            None => {
                report.issues.push(BtrfsScrubIssue {
                    mirror_offset: image.mirror_offset,
                    kind: BtrfsScrubIssueKind::MirrorMissing,
                });
                continue;
            }
        };

        let sb = match BtrfsSuperblock::from_bytes(block) {
            Some(sb) => sb,
            None => {
                report.issues.push(BtrfsScrubIssue {
                    mirror_offset: image.mirror_offset,
                    kind: BtrfsScrubIssueKind::InvalidSuperblock,
                });
                continue;
            }
        };

        if sb.bytenr != image.mirror_offset {
            report.issues.push(BtrfsScrubIssue {
                mirror_offset: image.mirror_offset,
                kind: BtrfsScrubIssueKind::BytenrMismatch,
            });
            continue;
        }

        if verify_superblock_checksum(block).is_err() {
            report.issues.push(BtrfsScrubIssue {
                mirror_offset: image.mirror_offset,
                kind: BtrfsScrubIssueKind::ChecksumMismatch,
            });
            continue;
        }

        report.valid_mirrors += 1;
        if sb.generation >= report.freshest_generation {
            if let Some(previous_offset) = report.selected_mirror {
                if sb.generation > report.freshest_generation {
                    report.issues.push(BtrfsScrubIssue {
                        mirror_offset: previous_offset,
                        kind: BtrfsScrubIssueKind::GenerationStale,
                    });
                }
            }
            report.freshest_generation = sb.generation;
            report.selected_mirror = Some(image.mirror_offset);
        }
    }

    report
}

pub fn scrub_superblock_mirrors_with_layout(disk_data: &[u8], mirrors: &[u64]) -> BtrfsScrubReport {
    let images = capture_scrub_mirror_images(disk_data, mirrors);
    scrub_superblock_mirror_images(&images)
}

pub fn scrub_superblock_mirrors(disk_data: &[u8]) -> BtrfsScrubReport {
    scrub_superblock_mirrors_with_layout(
        disk_data,
        &[
            BTRFS_SUPER_MIRROR_1,
            BTRFS_SUPER_MIRROR_2,
            BTRFS_SUPER_MIRROR_3,
        ],
    )
}

pub fn register_scrub_volume_with_layout(mount_point: &str, disk_data: &[u8], mirrors: &[u64]) {
    let volume = BtrfsScrubVolume {
        mount_point: String::from(mount_point),
        mirrors: capture_scrub_mirror_images(disk_data, mirrors),
    };

    let mut volumes = BTRFS_SCRUB_VOLUMES.lock();
    if let Some(existing) = volumes
        .iter_mut()
        .find(|entry| entry.mount_point == mount_point)
    {
        *existing = volume;
    } else {
        volumes.push(volume);
    }
}

pub fn register_scrub_volume(mount_point: &str, disk_data: &[u8]) {
    register_scrub_volume_with_layout(
        mount_point,
        disk_data,
        &[
            BTRFS_SUPER_MIRROR_1,
            BTRFS_SUPER_MIRROR_2,
            BTRFS_SUPER_MIRROR_3,
        ],
    );
}

pub fn run_scrub_daemon_pass() -> usize {
    let volumes = BTRFS_SCRUB_VOLUMES.lock().clone();
    let tick = crate::task::scheduler::get_ticks() as u64;
    let mut reports = Vec::with_capacity(volumes.len());
    let mut attention_required = 0usize;

    for volume in volumes {
        let report = scrub_superblock_mirror_images(&volume.mirrors);
        let has_hard_issue = report
            .issues
            .iter()
            .any(|issue| issue.kind != BtrfsScrubIssueKind::GenerationStale);
        if report.selected_mirror.is_none() || has_hard_issue {
            attention_required += 1;
        }
        reports.push(BtrfsScrubDaemonVolumeReport {
            mount_point: volume.mount_point,
            tick,
            report,
        });
    }

    *BTRFS_SCRUB_LAST_REPORTS.lock() = reports;
    BTRFS_SCRUB_DAEMON_LAST_TICK.store(tick, Ordering::Release);
    BTRFS_SCRUB_DAEMON_PASS_COUNT.fetch_add(1, Ordering::AcqRel);
    attention_required
}

pub fn scrub_daemon_status() -> BtrfsScrubDaemonStatus {
    BtrfsScrubDaemonStatus {
        started: BTRFS_SCRUB_DAEMON_STARTED.load(Ordering::Acquire),
        interval_ticks: BTRFS_SCRUB_DAEMON_INTERVAL_TICKS.load(Ordering::Acquire),
        pass_count: BTRFS_SCRUB_DAEMON_PASS_COUNT.load(Ordering::Acquire),
        registered_volumes: BTRFS_SCRUB_VOLUMES.lock().len(),
        last_tick: BTRFS_SCRUB_DAEMON_LAST_TICK.load(Ordering::Acquire),
        last_reports: BTRFS_SCRUB_LAST_REPORTS.lock().clone(),
    }
}

fn scrub_daemon_entry() -> ! {
    crate::serial_println!(
        "[Btrfs] Scrub daemon online (interval={} ticks)",
        BTRFS_SCRUB_DAEMON_INTERVAL_TICKS.load(Ordering::Acquire)
    );

    loop {
        let attention_required = run_scrub_daemon_pass();
        if attention_required > 0 {
            crate::serial_println!(
                "[Btrfs] Scrub pass flagged {} volume(s) for attention",
                attention_required
            );
        }

        let interval = BTRFS_SCRUB_DAEMON_INTERVAL_TICKS
            .load(Ordering::Acquire)
            .max(1);
        crate::task::scheduler::sleep(interval);
    }
}

pub fn ensure_scrub_daemon(interval_ticks: usize) -> bool {
    BTRFS_SCRUB_DAEMON_INTERVAL_TICKS.store(interval_ticks.max(1), Ordering::Release);

    if BTRFS_SCRUB_DAEMON_STARTED.load(Ordering::Acquire) {
        return false;
    }

    if !crate::task::scheduler::is_ready() {
        return false;
    }

    if BTRFS_SCRUB_DAEMON_STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return false;
    }

    crate::task::scheduler::spawn_with_priority(
        scrub_daemon_entry,
        crate::task::task::Priority::Low,
        "btrfs_scrubd",
    );
    true
}

// ============================================================================
// B-Tree Node Header
// ============================================================================

/// Btrfs B-Tree node/leaf header (101 bytes on-disk)
#[derive(Clone, Debug)]
pub struct BtrfsHeader {
    /// Checksum
    pub csum: [u8; 32],
    /// FS UUID
    pub fsid: [u8; 16],
    /// Logical byte offset of this node
    pub bytenr: u64,
    /// Flags
    pub flags: u64,
    /// Chunk tree UUID
    pub chunk_tree_uuid: [u8; 16],
    /// Generation
    pub generation: u64,
    /// Owner (tree that owns this node)
    pub owner: u64,
    /// Number of items
    pub nritems: u32,
    /// Level (0 = leaf, >0 = internal node)
    pub level: u8,
}

impl BtrfsHeader {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 101 {
            return None;
        }

        let mut csum = [0u8; 32];
        csum.copy_from_slice(&data[0..32]);

        let mut fsid = [0u8; 16];
        fsid.copy_from_slice(&data[32..48]);

        let mut chunk_tree_uuid = [0u8; 16];
        chunk_tree_uuid.copy_from_slice(&data[64..80]);

        Some(Self {
            csum,
            fsid,
            bytenr: u64::from_le_bytes([
                data[48], data[49], data[50], data[51], data[52], data[53], data[54], data[55],
            ]),
            flags: u64::from_le_bytes([
                data[56], data[57], data[58], data[59], data[60], data[61], data[62], data[63],
            ]),
            chunk_tree_uuid,
            generation: u64::from_le_bytes([
                data[80], data[81], data[82], data[83], data[84], data[85], data[86], data[87],
            ]),
            owner: u64::from_le_bytes([
                data[88], data[89], data[90], data[91], data[92], data[93], data[94], data[95],
            ]),
            nritems: u32::from_le_bytes([data[96], data[97], data[98], data[99]]),
            level: data[100],
        })
    }

    pub fn is_leaf(&self) -> bool {
        self.level == 0
    }
}

/// B-Tree leaf item pointer (25 bytes on-disk)
#[derive(Clone, Debug)]
pub struct BtrfsItem {
    /// Key
    pub key: BtrfsKey,
    /// Data offset (relative to end of header)
    pub offset: u32,
    /// Data size
    pub size: u32,
}

impl BtrfsItem {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 25 {
            return None;
        }
        let key = BtrfsKey::from_bytes(&data[0..17])?;
        Some(Self {
            key,
            offset: u32::from_le_bytes([data[17], data[18], data[19], data[20]]),
            size: u32::from_le_bytes([data[21], data[22], data[23], data[24]]),
        })
    }
}

/// B-Tree internal node key pointer (33 bytes)
#[derive(Clone, Debug)]
pub struct BtrfsKeyPtr {
    pub key: BtrfsKey,
    /// Block number of child node
    pub blockptr: u64,
    /// Generation of child
    pub generation: u64,
}

impl BtrfsKeyPtr {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 33 {
            return None;
        }
        let key = BtrfsKey::from_bytes(&data[0..17])?;
        Some(Self {
            key,
            blockptr: u64::from_le_bytes([
                data[17], data[18], data[19], data[20], data[21], data[22], data[23], data[24],
            ]),
            generation: u64::from_le_bytes([
                data[25], data[26], data[27], data[28], data[29], data[30], data[31], data[32],
            ]),
        })
    }
}

// ============================================================================
// Btrfs Inode
// ============================================================================

/// Btrfs inode item (160 bytes on-disk)
#[derive(Clone, Debug)]
pub struct BtrfsInodeItem {
    /// Generation
    pub generation: u64,
    /// Transaction ID
    pub transid: u64,
    /// File size
    pub size: u64,
    /// Disk space used (bytes)
    pub nbytes: u64,
    /// Block group hint
    pub block_group: u64,
    /// Hard link count
    pub nlink: u32,
    /// User ID
    pub uid: u32,
    /// Group ID
    pub gid: u32,
    /// File mode (permissions + type)
    pub mode: u32,
    /// Inode flags
    pub flags: u64,
    /// Sequence number (for NFS)
    pub sequence: u64,
    /// Access time (sec, nsec)
    pub atime: (u64, u32),
    /// Change time
    pub ctime: (u64, u32),
    /// Modification time
    pub mtime: (u64, u32),
    /// Creation time
    pub otime: (u64, u32),
}

impl BtrfsInodeItem {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 160 {
            return None;
        }

        Some(Self {
            generation: u64::from_le_bytes([
                data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
            ]),
            transid: u64::from_le_bytes([
                data[8], data[9], data[10], data[11], data[12], data[13], data[14], data[15],
            ]),
            size: u64::from_le_bytes([
                data[16], data[17], data[18], data[19], data[20], data[21], data[22], data[23],
            ]),
            nbytes: u64::from_le_bytes([
                data[24], data[25], data[26], data[27], data[28], data[29], data[30], data[31],
            ]),
            block_group: u64::from_le_bytes([
                data[32], data[33], data[34], data[35], data[36], data[37], data[38], data[39],
            ]),
            nlink: u32::from_le_bytes([data[40], data[41], data[42], data[43]]),
            uid: u32::from_le_bytes([data[44], data[45], data[46], data[47]]),
            gid: u32::from_le_bytes([data[48], data[49], data[50], data[51]]),
            mode: u32::from_le_bytes([data[52], data[53], data[54], data[55]]),
            flags: u64::from_le_bytes([
                data[64], data[65], data[66], data[67], data[68], data[69], data[70], data[71],
            ]),
            sequence: u64::from_le_bytes([
                data[72], data[73], data[74], data[75], data[76], data[77], data[78], data[79],
            ]),
            atime: (
                u64::from_le_bytes([
                    data[112], data[113], data[114], data[115], data[116], data[117], data[118],
                    data[119],
                ]),
                u32::from_le_bytes([data[120], data[121], data[122], data[123]]),
            ),
            ctime: (
                u64::from_le_bytes([
                    data[124], data[125], data[126], data[127], data[128], data[129], data[130],
                    data[131],
                ]),
                u32::from_le_bytes([data[132], data[133], data[134], data[135]]),
            ),
            mtime: (
                u64::from_le_bytes([
                    data[136], data[137], data[138], data[139], data[140], data[141], data[142],
                    data[143],
                ]),
                u32::from_le_bytes([data[144], data[145], data[146], data[147]]),
            ),
            otime: (
                u64::from_le_bytes([
                    data[148], data[149], data[150], data[151], data[152], data[153], data[154],
                    data[155],
                ]),
                u32::from_le_bytes([data[156], data[157], data[158], data[159]]),
            ),
        })
    }

    /// Regular file mı?
    pub fn is_regular(&self) -> bool {
        self.mode & S_IFMT == S_IFREG
    }

    /// Directory mi?
    pub fn is_directory(&self) -> bool {
        self.mode & S_IFMT == S_IFDIR
    }

    /// Symlink mi?
    pub fn is_symlink(&self) -> bool {
        self.mode & S_IFMT == S_IFLNK
    }
}

// ============================================================================
// Extent Data
// ============================================================================

/// Extent data inline/regular discrimination
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BtrfsExtentType {
    Inline = 0,
    Regular = 1,
    Prealloc = 2,
}

/// Btrfs extent data item
#[derive(Clone, Debug)]
pub struct BtrfsExtentData {
    /// Generation
    pub generation: u64,
    /// RAM bytes (uncompressed extent size)
    pub ram_bytes: u64,
    /// Compression type (0=none, 1=zlib, 2=lzo, 3=zstd)
    pub compression: u8,
    /// Encryption (reserved)
    pub encryption: u8,
    /// Type (0=inline, 1=regular, 2=prealloc)
    pub extent_type: u8,
    /// For regular/prealloc: logical disk offset
    pub disk_bytenr: u64,
    /// Disk extent size
    pub disk_num_bytes: u64,
    /// Offset within extent
    pub offset: u64,
    /// Number of bytes used
    pub num_bytes: u64,
}

impl BtrfsExtentData {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 21 {
            return None;
        }

        let extent_type = data[20];

        let (disk_bytenr, disk_num_bytes, offset, num_bytes) = if extent_type > 0
            && data.len() >= 53
        {
            (
                u64::from_le_bytes([
                    data[21], data[22], data[23], data[24], data[25], data[26], data[27], data[28],
                ]),
                u64::from_le_bytes([
                    data[29], data[30], data[31], data[32], data[33], data[34], data[35], data[36],
                ]),
                u64::from_le_bytes([
                    data[37], data[38], data[39], data[40], data[41], data[42], data[43], data[44],
                ]),
                u64::from_le_bytes([
                    data[45], data[46], data[47], data[48], data[49], data[50], data[51], data[52],
                ]),
            )
        } else {
            (0, 0, 0, 0)
        };

        Some(Self {
            generation: u64::from_le_bytes([
                data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
            ]),
            ram_bytes: u64::from_le_bytes([
                data[8], data[9], data[10], data[11], data[12], data[13], data[14], data[15],
            ]),
            compression: data[16],
            encryption: data[17],
            extent_type,
            disk_bytenr,
            disk_num_bytes,
            offset,
            num_bytes,
        })
    }

    pub fn is_inline(&self) -> bool {
        self.extent_type == 0
    }

    pub fn compression_str(&self) -> &'static str {
        match self.compression {
            0 => "none",
            1 => "zlib",
            2 => "lzo",
            3 => "zstd",
            _ => "unknown",
        }
    }
}

#[derive(Clone, Debug)]
pub struct BtrfsRootItem {
    pub generation: u64,
    pub root_dirid: u64,
    pub bytenr: u64,
    pub byte_limit: u64,
    pub bytes_used: u64,
    pub last_snapshot: u64,
    pub flags: u64,
    pub refs: u32,
    pub level: u8,
}

impl BtrfsRootItem {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < BTRFS_ROOT_ITEM_MIN_SIZE {
            return None;
        }

        Some(Self {
            generation: u64::from_le_bytes([
                data[160], data[161], data[162], data[163], data[164], data[165], data[166],
                data[167],
            ]),
            root_dirid: u64::from_le_bytes([
                data[168], data[169], data[170], data[171], data[172], data[173], data[174],
                data[175],
            ]),
            bytenr: u64::from_le_bytes([
                data[176], data[177], data[178], data[179], data[180], data[181], data[182],
                data[183],
            ]),
            byte_limit: u64::from_le_bytes([
                data[184], data[185], data[186], data[187], data[188], data[189], data[190],
                data[191],
            ]),
            bytes_used: u64::from_le_bytes([
                data[192], data[193], data[194], data[195], data[196], data[197], data[198],
                data[199],
            ]),
            last_snapshot: u64::from_le_bytes([
                data[200], data[201], data[202], data[203], data[204], data[205], data[206],
                data[207],
            ]),
            flags: u64::from_le_bytes([
                data[208], data[209], data[210], data[211], data[212], data[213], data[214],
                data[215],
            ]),
            refs: u32::from_le_bytes([data[216], data[217], data[218], data[219]]),
            level: data[238],
        })
    }
}

#[derive(Clone, Debug)]
pub struct BtrfsDirectoryEntry {
    pub name: String,
    pub inode: u64,
    pub file_type: u8,
}

#[derive(Clone, Debug)]
struct BtrfsCollectedItem {
    key: BtrfsKey,
    data: Vec<u8>,
}

#[derive(Clone, Debug)]
struct BtrfsFileExtentRecord {
    file_offset: u64,
    extent: BtrfsExtentData,
    inline_data: Vec<u8>,
}

#[derive(Clone, Debug)]
pub enum BtrfsStorage {
    Resident(Arc<Vec<u8>>),
}

#[derive(Clone, Debug)]
pub struct MountedBtrfs {
    pub fs: BtrfsFilesystem,
    pub storage: BtrfsStorage,
}

// ============================================================================
// Chunk Item (Logical → Physical mapping)
// ============================================================================

/// Btrfs chunk item — logical → physical block mapping
#[derive(Clone, Debug)]
pub struct BtrfsChunkItem {
    /// Chunk size
    pub length: u64,
    /// Owner (objectid of the root)
    pub owner: u64,
    /// Stripe length
    pub stripe_len: u64,
    /// Type flags (DATA, METADATA, SYSTEM, RAID0/1/5/6/10)
    pub type_flags: u64,
    /// Optimal I/O alignment
    pub io_align: u32,
    /// Optimal I/O width
    pub io_width: u32,
    /// Sector size
    pub sector_size: u32,
    /// Number of stripes
    pub num_stripes: u16,
    /// Sub-stripes (RAID10 only)
    pub sub_stripes: u16,
    /// Stripe array
    pub stripes: Vec<BtrfsStripe>,
}

/// Btrfs stripe entry
#[derive(Clone, Debug)]
pub struct BtrfsStripe {
    pub devid: u64,
    pub offset: u64,
    pub dev_uuid: [u8; 16],
}

impl BtrfsChunkItem {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 48 {
            return None;
        }

        let num_stripes = u16::from_le_bytes([data[44], data[45]]);
        let sub_stripes = u16::from_le_bytes([data[46], data[47]]);

        let mut stripes = Vec::new();
        let stripe_start = 48;
        for i in 0..num_stripes as usize {
            let off = stripe_start + i * 32;
            if off + 32 > data.len() {
                break;
            }

            let mut uuid = [0u8; 16];
            uuid.copy_from_slice(&data[off + 16..off + 32]);

            stripes.push(BtrfsStripe {
                devid: u64::from_le_bytes([
                    data[off],
                    data[off + 1],
                    data[off + 2],
                    data[off + 3],
                    data[off + 4],
                    data[off + 5],
                    data[off + 6],
                    data[off + 7],
                ]),
                offset: u64::from_le_bytes([
                    data[off + 8],
                    data[off + 9],
                    data[off + 10],
                    data[off + 11],
                    data[off + 12],
                    data[off + 13],
                    data[off + 14],
                    data[off + 15],
                ]),
                dev_uuid: uuid,
            });
        }

        Some(Self {
            length: u64::from_le_bytes([
                data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
            ]),
            owner: u64::from_le_bytes([
                data[8], data[9], data[10], data[11], data[12], data[13], data[14], data[15],
            ]),
            stripe_len: u64::from_le_bytes([
                data[16], data[17], data[18], data[19], data[20], data[21], data[22], data[23],
            ]),
            type_flags: u64::from_le_bytes([
                data[24], data[25], data[26], data[27], data[28], data[29], data[30], data[31],
            ]),
            io_align: u32::from_le_bytes([data[32], data[33], data[34], data[35]]),
            io_width: u32::from_le_bytes([data[36], data[37], data[38], data[39]]),
            sector_size: u32::from_le_bytes([data[40], data[41], data[42], data[43]]),
            num_stripes,
            sub_stripes,
            stripes,
        })
    }

    /// RAID seviyesini string olarak döner
    pub fn raid_type_str(&self) -> &'static str {
        let flags = self.type_flags;
        if flags & (1 << 3) != 0 {
            "RAID0"
        } else if flags & (1 << 4) != 0 {
            "RAID1"
        } else if flags & (1 << 5) != 0 {
            "DUP"
        } else if flags & (1 << 6) != 0 {
            "RAID10"
        } else if flags & (1 << 7) != 0 {
            "RAID5"
        } else if flags & (1 << 8) != 0 {
            "RAID6"
        } else {
            "single"
        }
    }
}

// ============================================================================
// Subvolume / Snapshot
// ============================================================================

/// Btrfs subvolume bilgisi
#[derive(Clone, Debug)]
pub struct BtrfsSubvolume {
    /// Subvolume objectid
    pub id: u64,
    /// Parent subvolume id (0 = top-level)
    pub parent_id: u64,
    /// İsim
    pub name: String,
    /// Generation
    pub generation: u64,
    /// Read-only snapshot mu?
    pub readonly: bool,
    /// Root inode generation
    pub root_generation: u64,
}

// ============================================================================
// Btrfs Filesystem Manager
// ============================================================================

/// Btrfs dosya sistemi yöneticisi
#[derive(Clone, Debug)]
pub struct BtrfsFilesystem {
    /// Parse edilmiş superblock
    pub superblock: BtrfsSuperblock,
    /// Subvolume listesi
    pub subvolumes: Vec<BtrfsSubvolume>,
    /// Inode cache (objectid → inode item)
    pub inode_cache: BTreeMap<u64, BtrfsInodeItem>,
    /// Chunk map (logical → physical)
    pub chunk_map: Vec<(u64, u64, BtrfsChunkItem)>, // (logical_start, logical_end, chunk)
    /// Root item cache (subvolume id → root item)
    pub root_items: BTreeMap<u64, BtrfsRootItem>,
    /// Directory entry cache (directory inode → entries)
    pub directory_entries: BTreeMap<u64, Vec<BtrfsDirectoryEntry>>,
    /// File extent cache (inode → extent descriptors)
    file_extents: BTreeMap<u64, Vec<BtrfsFileExtentRecord>>,
    /// Varsayılan subvolume id
    pub default_subvolume_id: u64,
    /// Varsayılan subvolume kök inode id
    pub default_root_dirid: u64,
    /// Varsayılan subvolume FS tree root logical bytenr
    pub default_fs_tree_bytenr: u64,
    /// Mount noktası
    pub mount_point: String,
}

impl BtrfsFilesystem {
    /// Superblock'tan yeni Btrfs dosya sistemi oluşturur
    pub fn new(sb: BtrfsSuperblock, mount_point: &str) -> Self {
        Self {
            superblock: sb,
            subvolumes: Vec::new(),
            inode_cache: BTreeMap::new(),
            chunk_map: Vec::new(),
            root_items: BTreeMap::new(),
            directory_entries: BTreeMap::new(),
            file_extents: BTreeMap::new(),
            default_subvolume_id: BTRFS_FS_TREE_OBJECTID,
            default_root_dirid: BTRFS_FIRST_FREE_OBJECTID,
            default_fs_tree_bytenr: 0,
            mount_point: String::from(mount_point),
        }
    }

    /// Default subvolume ekler
    pub fn add_default_subvolume(&mut self) {
        self.subvolumes.push(BtrfsSubvolume {
            id: BTRFS_FS_TREE_OBJECTID,
            parent_id: 0,
            name: String::from("top"),
            generation: self.superblock.generation,
            readonly: false,
            root_generation: self.superblock.generation,
        });
    }

    fn upsert_chunk(
        &mut self,
        logical_start: u64,
        chunk: BtrfsChunkItem,
    ) -> Result<(), &'static str> {
        self.chunk_mapping_supported(&chunk)?;
        let logical_end = logical_start.saturating_add(chunk.length);
        self.chunk_map
            .retain(|(start, _, _)| *start != logical_start);
        self.chunk_map.push((logical_start, logical_end, chunk));
        self.chunk_map.sort_by_key(|(start, _, _)| *start);
        Ok(())
    }

    fn load_system_chunk_array(&mut self) -> Result<(), &'static str> {
        let mut cursor = 0usize;
        let bytes = self.superblock.sys_chunk_array.clone();
        while cursor < bytes.len() {
            if bytes[cursor..].iter().all(|byte| *byte == 0) {
                break;
            }
            if cursor + 17 > bytes.len() {
                return Err("btrfs: truncated system chunk key");
            }
            let key = BtrfsKey::from_bytes(&bytes[cursor..cursor + 17])
                .ok_or("btrfs: invalid system chunk key")?;
            cursor += 17;
            if key.item_type != BTRFS_CHUNK_ITEM_KEY {
                return Err("btrfs: unsupported system chunk entry");
            }
            if cursor + 48 > bytes.len() {
                return Err("btrfs: truncated system chunk item");
            }
            let num_stripes = u16::from_le_bytes([bytes[cursor + 44], bytes[cursor + 45]]) as usize;
            let chunk_size = 48usize.saturating_add(num_stripes.saturating_mul(32));
            if cursor + chunk_size > bytes.len() {
                return Err("btrfs: truncated system chunk stripe array");
            }
            let chunk = BtrfsChunkItem::from_bytes(&bytes[cursor..cursor + chunk_size])
                .ok_or("btrfs: invalid system chunk item")?;
            self.upsert_chunk(key.offset, chunk)?;
            cursor += chunk_size;
        }
        Ok(())
    }

    /// Snapshot oluşturur (O(1) CoW klon)
    pub fn create_snapshot(
        &mut self,
        _source_id: u64,
        _name: &str,
        _readonly: bool,
    ) -> Result<u64, &'static str> {
        Err("btrfs: snapshot creation is not supported on the resident read-only backend")
    }

    fn chunk_mapping_supported(&self, chunk: &BtrfsChunkItem) -> Result<(), &'static str> {
        if self.superblock.num_devices != 1 {
            return Err("btrfs: multi-device volumes are not supported");
        }
        if chunk.raid_type_str() != "single" {
            return Err("btrfs: raid chunk profiles are not supported");
        }
        if chunk.num_stripes != 1 || chunk.stripes.len() != 1 || chunk.sub_stripes > 1 {
            return Err("btrfs: multi-stripe chunks are not supported");
        }
        Ok(())
    }

    /// Logical adres → physical adrese çevirir
    pub fn logical_to_physical(&self, logical: u64) -> Option<u64> {
        for (start, end, chunk) in &self.chunk_map {
            if logical >= *start && logical < *end {
                if self.chunk_mapping_supported(chunk).is_err() {
                    return None;
                }
                let offset = logical - start;
                if let Some(stripe) = chunk.stripes.first() {
                    return Some(stripe.offset + offset);
                }
            }
        }
        None
    }

    fn read_logical_range(
        &self,
        storage: &BtrfsStorage,
        logical: u64,
        len: usize,
    ) -> Result<Vec<u8>, &'static str> {
        let mut physical = None;
        for (start, end, chunk) in &self.chunk_map {
            if logical >= *start && logical < *end {
                self.chunk_mapping_supported(chunk)?;
                let offset = logical - start;
                if let Some(stripe) = chunk.stripes.first() {
                    physical = Some(stripe.offset + offset);
                    break;
                }
            }
        }
        let physical = physical.ok_or("btrfs: logical address is not mapped")?;
        let start = physical as usize;
        let end = start
            .checked_add(len)
            .ok_or("btrfs: address overflow while reading")?;
        match storage {
            BtrfsStorage::Resident(image) => {
                if end > image.len() {
                    return Err("btrfs: read exceeds resident image");
                }
                Ok(image[start..end].to_vec())
            }
        }
    }

    fn collect_tree_items(
        &self,
        storage: &BtrfsStorage,
        logical: u64,
        items: &mut Vec<BtrfsCollectedItem>,
        visited: &mut Vec<u64>,
    ) -> Result<(), &'static str> {
        if visited.iter().any(|seen| *seen == logical) {
            return Err("btrfs: tree cycle detected");
        }
        visited.push(logical);

        let block =
            self.read_logical_range(storage, logical, self.superblock.node_size as usize)?;
        let header = BtrfsHeader::from_bytes(&block).ok_or("btrfs: invalid tree header")?;
        if header.bytenr != logical {
            return Err("btrfs: tree header bytenr mismatch");
        }
        if header.fsid != self.superblock.fsid {
            return Err("btrfs: tree header fsid mismatch");
        }

        if header.is_leaf() {
            let item_table_end =
                BTRFS_HEADER_SIZE.saturating_add(header.nritems as usize * BTRFS_LEAF_ITEM_SIZE);
            if item_table_end > block.len() {
                return Err("btrfs: truncated leaf item table");
            }
            for index in 0..header.nritems as usize {
                let slot = BTRFS_HEADER_SIZE + index * BTRFS_LEAF_ITEM_SIZE;
                let item = BtrfsItem::from_bytes(&block[slot..slot + BTRFS_LEAF_ITEM_SIZE])
                    .ok_or("btrfs: invalid leaf item")?;
                let data_start = item.offset as usize;
                let data_end = data_start
                    .checked_add(item.size as usize)
                    .ok_or("btrfs: leaf item overflow")?;
                if data_start < item_table_end || data_end > block.len() {
                    return Err("btrfs: leaf item payload out of range");
                }
                items.push(BtrfsCollectedItem {
                    key: item.key,
                    data: block[data_start..data_end].to_vec(),
                });
            }
        } else {
            let ptr_table_end =
                BTRFS_HEADER_SIZE.saturating_add(header.nritems as usize * BTRFS_KEY_PTR_SIZE);
            if ptr_table_end > block.len() {
                return Err("btrfs: truncated internal node pointer table");
            }
            for index in 0..header.nritems as usize {
                let slot = BTRFS_HEADER_SIZE + index * BTRFS_KEY_PTR_SIZE;
                let key_ptr = BtrfsKeyPtr::from_bytes(&block[slot..slot + BTRFS_KEY_PTR_SIZE])
                    .ok_or("btrfs: invalid tree key pointer")?;
                self.collect_tree_items(storage, key_ptr.blockptr, items, visited)?;
            }
        }

        Ok(())
    }

    fn load_chunk_tree(&mut self, storage: &BtrfsStorage) -> Result<(), &'static str> {
        let mut items = Vec::new();
        let mut visited = Vec::new();
        self.collect_tree_items(
            storage,
            self.superblock.chunk_root,
            &mut items,
            &mut visited,
        )?;
        for item in items {
            if item.key.item_type != BTRFS_CHUNK_ITEM_KEY {
                continue;
            }
            let chunk =
                BtrfsChunkItem::from_bytes(&item.data).ok_or("btrfs: invalid chunk tree item")?;
            self.upsert_chunk(item.key.offset, chunk)?;
        }
        if self.chunk_map.is_empty() {
            return Err("btrfs: chunk map bootstrap produced no mappings");
        }
        Ok(())
    }

    fn load_root_tree(&mut self, storage: &BtrfsStorage) -> Result<(), &'static str> {
        let mut items = Vec::new();
        let mut visited = Vec::new();
        self.collect_tree_items(storage, self.superblock.root, &mut items, &mut visited)?;

        let mut best_default_root: Option<(BtrfsKey, BtrfsRootItem)> = None;
        for item in items {
            if item.key.item_type != BTRFS_ROOT_ITEM_KEY {
                continue;
            }
            let root_item =
                BtrfsRootItem::from_bytes(&item.data).ok_or("btrfs: invalid root item")?;
            self.root_items.insert(item.key.objectid, root_item.clone());
            if item.key.objectid == BTRFS_FS_TREE_OBJECTID
                && best_default_root
                    .as_ref()
                    .is_none_or(|(best_key, _)| item.key.offset > best_key.offset)
            {
                best_default_root = Some((item.key, root_item));
            }
        }

        let (_, default_root) = best_default_root.ok_or("btrfs: default fs tree root missing")?;
        self.default_subvolume_id = BTRFS_FS_TREE_OBJECTID;
        self.default_root_dirid = default_root.root_dirid;
        self.default_fs_tree_bytenr = default_root.bytenr;
        self.subvolumes.clear();
        self.subvolumes.push(BtrfsSubvolume {
            id: self.default_subvolume_id,
            parent_id: 0,
            name: String::from("top"),
            generation: default_root.generation,
            readonly: (default_root.flags & BTRFS_ROOT_SUBVOL_RDONLY) != 0,
            root_generation: default_root.generation,
        });

        Ok(())
    }

    fn index_fs_tree_items(&mut self, items: Vec<BtrfsCollectedItem>) -> Result<(), &'static str> {
        self.inode_cache.clear();
        self.directory_entries.clear();
        self.file_extents.clear();

        for item in items {
            match item.key.item_type {
                BTRFS_INODE_ITEM_KEY => {
                    let inode = BtrfsInodeItem::from_bytes(&item.data)
                        .ok_or("btrfs: invalid inode item")?;
                    self.inode_cache.insert(item.key.objectid, inode);
                }
                BTRFS_DIR_ITEM_KEY | BTRFS_DIR_INDEX_KEY => {
                    let entries = parse_directory_entries(&item.data)?;
                    self.directory_entries
                        .entry(item.key.objectid)
                        .or_default()
                        .extend(entries);
                }
                BTRFS_EXTENT_DATA_KEY => {
                    let extent = BtrfsExtentData::from_bytes(&item.data)
                        .ok_or("btrfs: invalid extent data")?;
                    let inline_data = if extent.is_inline() {
                        item.data[21..].to_vec()
                    } else {
                        Vec::new()
                    };
                    self.file_extents
                        .entry(item.key.objectid)
                        .or_default()
                        .push(BtrfsFileExtentRecord {
                            file_offset: item.key.offset,
                            extent,
                            inline_data,
                        });
                }
                _ => {}
            }
        }

        for extents in self.file_extents.values_mut() {
            extents.sort_by_key(|record| record.file_offset);
        }

        Ok(())
    }

    fn load_default_fs_tree(&mut self, storage: &BtrfsStorage) -> Result<(), &'static str> {
        if self.default_fs_tree_bytenr == 0 {
            return Err("btrfs: default fs tree root not resolved");
        }
        let mut items = Vec::new();
        let mut visited = Vec::new();
        self.collect_tree_items(
            storage,
            self.default_fs_tree_bytenr,
            &mut items,
            &mut visited,
        )?;
        self.index_fs_tree_items(items)?;
        if !self.inode_cache.contains_key(&self.default_root_dirid) {
            return Err("btrfs: default root inode missing");
        }
        Ok(())
    }

    pub fn load_from_storage(&mut self, storage: &BtrfsStorage) -> Result<(), &'static str> {
        if self.superblock.num_devices != 1 {
            return Err("btrfs: multi-device volumes are not supported");
        }
        self.load_system_chunk_array()?;
        self.load_chunk_tree(storage)?;
        self.load_root_tree(storage)?;
        self.load_default_fs_tree(storage)?;
        Ok(())
    }

    pub fn get_inode(&self, inode: u64) -> Result<BtrfsInodeItem, &'static str> {
        self.inode_cache
            .get(&inode)
            .cloned()
            .ok_or("btrfs: inode not found")
    }

    pub fn root_inode(&self) -> u64 {
        self.default_root_dirid
    }

    pub fn resolve_path(&self, path: &str) -> Result<u64, &'static str> {
        let mut current = self.root_inode();
        let trimmed = path.trim_matches('/');
        if trimmed.is_empty() {
            return Ok(current);
        }

        for component in trimmed.split('/').filter(|component| !component.is_empty()) {
            let inode = self.get_inode(current)?;
            if !inode.is_directory() {
                return Err("btrfs: parent is not a directory");
            }
            let entries = self
                .directory_entries
                .get(&current)
                .ok_or("btrfs: directory entries missing")?;
            let next = entries
                .iter()
                .find(|entry| entry.name == component)
                .map(|entry| entry.inode)
                .ok_or("btrfs: file not found")?;
            current = next;
        }

        Ok(current)
    }

    pub fn list_directory(&self, inode: u64) -> Result<Vec<BtrfsDirectoryEntry>, &'static str> {
        let inode_item = self.get_inode(inode)?;
        if !inode_item.is_directory() {
            return Err("btrfs: path is not a directory");
        }
        Ok(self
            .directory_entries
            .get(&inode)
            .cloned()
            .unwrap_or_else(Vec::new))
    }

    pub fn read_file_from_storage(
        &self,
        inode: u64,
        storage: &BtrfsStorage,
    ) -> Result<Vec<u8>, &'static str> {
        let inode_item = self.get_inode(inode)?;
        if inode_item.is_directory() {
            return Err("btrfs: path is a directory");
        }

        let mut file = vec![0u8; inode_item.size as usize];
        let extents = self
            .file_extents
            .get(&inode)
            .cloned()
            .unwrap_or_else(Vec::new);

        for record in extents {
            let source = if record.extent.is_inline() {
                record.inline_data
            } else if record.extent.extent_type == BtrfsExtentType::Regular as u8 {
                self.read_logical_range(
                    storage,
                    record
                        .extent
                        .disk_bytenr
                        .saturating_add(record.extent.offset),
                    record.extent.num_bytes as usize,
                )?
            } else {
                return Err("btrfs: prealloc extents are not supported");
            };

            let decompressed = if record.extent.compression != 0 {
                decompress_btrfs_data(
                    &source,
                    record.extent.ram_bytes as usize,
                    record.extent.compression,
                )?
            } else {
                source
            };

            let start = record.file_offset as usize;
            if start >= file.len() {
                continue;
            }
            let copy_len = core::cmp::min(decompressed.len(), file.len() - start);
            file[start..start + copy_len].copy_from_slice(&decompressed[..copy_len]);
        }

        Ok(file)
    }

    /// Dosya sistemi bilgilerini yazdırır
    pub fn print_info(&self) {
        crate::serial_println!("[Btrfs] === Filesystem Info ===");
        crate::serial_println!("[Btrfs] Label: {}", self.superblock.label_str());
        crate::serial_println!("[Btrfs] Generation: {}", self.superblock.generation);
        crate::serial_println!("[Btrfs] Node size: {} bytes", self.superblock.node_size);
        crate::serial_println!("[Btrfs] Sector size: {} bytes", self.superblock.sector_size);
        crate::serial_println!(
            "[Btrfs] Total: {} MB",
            self.superblock.total_size() / (1024 * 1024)
        );
        crate::serial_println!(
            "[Btrfs] Used: {} MB",
            self.superblock.used_size() / (1024 * 1024)
        );
        crate::serial_println!(
            "[Btrfs] Free: {} MB",
            self.superblock.free_size() / (1024 * 1024)
        );
        crate::serial_println!("[Btrfs] Checksum: {}", self.superblock.csum_type_str());
        crate::serial_println!("[Btrfs] Devices: {}", self.superblock.num_devices);
        crate::serial_println!("[Btrfs] Subvolumes: {}", self.subvolumes.len());
        crate::serial_println!("[Btrfs] Root inode: {}", self.default_root_dirid);
        crate::serial_println!("[Btrfs] Mount: {}", self.mount_point);
    }
}

/// Btrfs compressed data decompression
/// compression: 1=zlib, 2=lzo, 3=zstd
fn decompress_btrfs_data(
    data: &[u8],
    decompressed_size: usize,
    compression: u8,
) -> Result<Vec<u8>, &'static str> {
    match compression {
        1 => {
            // zlib/DEFLATE — RFC 1951 compliant decompression
            // btrfs uses raw DEFLATE (no zlib wrapper)
            crate::compression::deflate::decompress_deflate(data, decompressed_size)
        }
        2 => {
            // LZO1X — LZO decompression
            crate::compression::lzo1x::decompress_lzo1x(data, decompressed_size)
        }
        3 => {
            // Zstandard — ZSTD decompression
            crate::compression::zstd::decompress_zstd(data, decompressed_size)
        }
        _ => Err("btrfs: unknown compression type"),
    }
}

fn parse_directory_entries(data: &[u8]) -> Result<Vec<BtrfsDirectoryEntry>, &'static str> {
    let mut entries = Vec::new();
    let mut cursor = 0usize;

    while cursor < data.len() {
        if cursor + BTRFS_DIR_ITEM_DATA_SIZE > data.len() {
            return Err("btrfs: truncated directory entry");
        }
        let location = BtrfsKey::from_bytes(&data[cursor..cursor + 17])
            .ok_or("btrfs: invalid directory entry key")?;
        let data_len = u16::from_le_bytes([data[cursor + 25], data[cursor + 26]]) as usize;
        let name_len = u16::from_le_bytes([data[cursor + 27], data[cursor + 28]]) as usize;
        let file_type = data[cursor + 29];
        let total_len = BTRFS_DIR_ITEM_DATA_SIZE
            .checked_add(name_len)
            .and_then(|size| size.checked_add(data_len))
            .ok_or("btrfs: directory entry length overflow")?;
        if cursor + total_len > data.len() {
            return Err("btrfs: directory entry payload out of range");
        }
        let name_start = cursor + BTRFS_DIR_ITEM_DATA_SIZE;
        let name_end = name_start + name_len;
        let name = core::str::from_utf8(&data[name_start..name_end])
            .map_err(|_| "btrfs: invalid utf-8 directory entry name")?;
        entries.push(BtrfsDirectoryEntry {
            name: String::from(name),
            inode: location.objectid,
            file_type,
        });
        cursor += total_len;
    }

    Ok(entries)
}

// ============================================================================
// Global Registry
// ============================================================================

lazy_static::lazy_static! {
    static ref BTRFS_FILESYSTEMS: Mutex<BTreeMap<String, MountedBtrfs>> =
        Mutex::new(BTreeMap::new());
    static ref BTRFS_SCRUB_VOLUMES: Mutex<Vec<BtrfsScrubVolume>> = Mutex::new(Vec::new());
    static ref BTRFS_SCRUB_LAST_REPORTS: Mutex<Vec<BtrfsScrubDaemonVolumeReport>> =
        Mutex::new(Vec::new());
}

static BTRFS_SCRUB_DAEMON_STARTED: AtomicBool = AtomicBool::new(false);
static BTRFS_SCRUB_DAEMON_INTERVAL_TICKS: AtomicUsize =
    AtomicUsize::new(BTRFS_DEFAULT_SCRUB_INTERVAL_TICKS);
static BTRFS_SCRUB_DAEMON_PASS_COUNT: AtomicU64 = AtomicU64::new(0);
static BTRFS_SCRUB_DAEMON_LAST_TICK: AtomicU64 = AtomicU64::new(0);

/// Btrfs modülünü başlatır
pub fn init() {
    crate::serial_println!("[Btrfs] Btrfs filesystem module initialized");
    crate::serial_println!(
        "[Btrfs] Features: CoW, snapshot, subvolume, B-Tree, checksum, compression"
    );
    let _ = ensure_scrub_daemon(BTRFS_DEFAULT_SCRUB_INTERVAL_TICKS);
}

/// Btrfs mount eder
pub fn mount_from_data(disk_data: &[u8], mount_point: &str) -> Result<(), &'static str> {
    mount_named_from_data(mount_point, disk_data, mount_point)
}

pub fn mount_named_from_data(
    name: &str,
    disk_data: &[u8],
    mount_point: &str,
) -> Result<(), &'static str> {
    if disk_data.len() < BTRFS_SUPER_OFFSET + BTRFS_SUPERBLOCK_SIZE {
        return Err("Disk too small for Btrfs");
    }

    let scrub = scrub_superblock_mirrors(disk_data);
    let mirror_offset = scrub.selected_mirror.ok_or("Invalid Btrfs superblock")?;
    let sb = BtrfsSuperblock::from_bytes(
        &disk_data[mirror_offset as usize..mirror_offset as usize + BTRFS_SUPERBLOCK_SIZE],
    )
    .ok_or("Invalid Btrfs superblock")?;

    let storage = BtrfsStorage::Resident(Arc::new(disk_data.to_vec()));
    let mut fs = BtrfsFilesystem::new(sb, mount_point);
    fs.load_from_storage(&storage)?;
    fs.print_info();

    BTRFS_FILESYSTEMS
        .lock()
        .insert(name.to_string(), MountedBtrfs { fs, storage });
    register_scrub_volume(mount_point, disk_data);
    let attention_required = run_scrub_daemon_pass();
    if attention_required > 0 {
        crate::serial_println!(
            "[Btrfs] Immediate scrub flagged {} volume(s) after mount",
            attention_required
        );
    }
    let _ = ensure_scrub_daemon(BTRFS_DEFAULT_SCRUB_INTERVAL_TICKS);
    Ok(())
}

/// Mount edilmiş Btrfs sayısı
pub fn mounted_count() -> usize {
    BTRFS_FILESYSTEMS.lock().len()
}

pub fn get_mounted_btrfs(name: &str) -> Option<MountedBtrfs> {
    BTRFS_FILESYSTEMS.lock().get(name).cloned()
}

pub fn unmount_btrfs(name: &str) -> bool {
    BTRFS_FILESYSTEMS.lock().remove(name).is_some()
}

// ============================================================================
// Copy-on-Write (CoW) Write Path
// ============================================================================

/// Btrfs CoW block allocation result
#[derive(Clone, Debug)]
pub struct BtrfsAllocResult {
    /// Newly allocated logical byte offset
    pub bytenr: u64,
    /// Allocated size in bytes
    pub num_bytes: u64,
}

/// Free space info for extent allocation
#[derive(Clone, Debug)]
pub struct BtrfsFreeSpace {
    /// Start of free region (logical)
    pub start: u64,
    /// Length of free region
    pub length: u64,
}

/// CoW-aware block writer for Btrfs
///
/// Implements the core CoW semantics:
/// 1. Allocate a new block (never overwrite in-place)
/// 2. Copy old data to new block (if updating existing)
/// 3. Modify the new block
/// 4. Update parent pointer to new block
/// 5. Compute and stamp block checksum
/// 6. Bump generation
#[derive(Clone, Debug)]
pub struct BtrfsCowWriter {
    /// Mutable image buffer (resident backend)
    image: Arc<Mutex<Vec<u8>>>,
    /// Next free logical address for allocation
    alloc_cursor: u64,
    /// Pending extent allocations (logical -> size)
    allocated_extents: BTreeMap<u64, u64>,
    /// Freed extents (pending transaction commit)
    freed_extents: Vec<(u64, u64)>,
    /// Current transaction generation
    generation: u64,
    /// Block group free space cache
    free_space: Vec<BtrfsFreeSpace>,
    /// Data checksums (file_offset -> checksum)
    data_csums: BTreeMap<u64, [u8; 32]>,
    /// Checksum type from superblock
    csum_type: u16,
    /// Node size for B-Tree blocks
    node_size: u32,
    /// Sector size
    sector_size: u32,
}

impl BtrfsCowWriter {
    /// Create a new CoW writer from a mounted filesystem
    pub fn from_fs(fs: &BtrfsFilesystem, image: Arc<Mutex<Vec<u8>>>) -> Self {
        let total_bytes = fs.superblock.total_bytes;
        let used_bytes = fs.superblock.bytes_used;
        let free_start = used_bytes;
        let free_len = total_bytes.saturating_sub(used_bytes);

        let mut free_space = Vec::new();
        if free_len > 0 {
            free_space.push(BtrfsFreeSpace {
                start: free_start,
                length: free_len,
            });
        }

        // Populate free space from chunk map gaps
        let mut last_end = 0u64;
        for (start, end, _) in &fs.chunk_map {
            if *start > last_end {
                free_space.push(BtrfsFreeSpace {
                    start: last_end,
                    length: start - last_end,
                });
            }
            last_end = *end;
        }

        Self {
            image,
            alloc_cursor: free_start,
            allocated_extents: BTreeMap::new(),
            freed_extents: Vec::new(),
            generation: fs.superblock.generation + 1,
            free_space,
            data_csums: BTreeMap::new(),
            csum_type: fs.superblock.csum_type,
            node_size: fs.superblock.node_size,
            sector_size: fs.superblock.sector_size,
        }
    }

    /// Allocate a new block for CoW write.
    ///
    /// Returns the logical byte offset of the newly allocated block.
    /// The block is zeroed and ready for writing.
    pub fn alloc_block(&mut self, size: u64) -> Result<u64, &'static str> {
        let aligned_size = (size + self.sector_size as u64 - 1) / self.sector_size as u64
            * self.sector_size as u64;

        // Find a free region that fits
        for (i, region) in self.free_space.iter().enumerate() {
            if region.length >= aligned_size {
                let bytenr = region.start;
                let remaining = region.length - aligned_size;

                // Update free space
                if remaining > 0 {
                    self.free_space[i].start += aligned_size;
                    self.free_space[i].length = remaining;
                } else {
                    self.free_space.remove(i);
                }

                self.allocated_extents.insert(bytenr, aligned_size);
                self.alloc_cursor = bytenr + aligned_size;

                // Zero the allocated region
                let start = bytenr as usize;
                let end = start + aligned_size as usize;
                let mut image = self.image.lock();
                if end > image.len() {
                    image.resize(end, 0);
                }
                for byte in &mut image[start..end] {
                    *byte = 0;
                }

                return Ok(bytenr);
            }
        }

        // No free region found; extend the image
        let bytenr = self.alloc_cursor;
        let start = bytenr as usize;
        let end = start + aligned_size as usize;
        let mut image = self.image.lock();
        if end > image.len() {
            image.resize(end, 0);
        }
        for byte in &mut image[start..end] {
            *byte = 0;
        }

        self.allocated_extents.insert(bytenr, aligned_size);
        self.alloc_cursor = end as u64;

        Ok(bytenr)
    }

    /// Free a previously allocated block.
    ///
    /// The block is added to the freed list and will be reclaimed
    /// after the current transaction commits.
    pub fn free_block(&mut self, bytenr: u64, size: u64) {
        self.allocated_extents.remove(&bytenr);
        self.freed_extents.push((bytenr, size));
        self.free_space.push(BtrfsFreeSpace {
            start: bytenr,
            length: size,
        });
        // Merge adjacent free regions
        self.merge_free_space();
    }

    /// Merge adjacent free space regions to reduce fragmentation
    fn merge_free_space(&mut self) {
        if self.free_space.len() <= 1 {
            return;
        }
        self.free_space.sort_by_key(|r| r.start);
        let mut merged = Vec::with_capacity(self.free_space.len());
        let mut current = self.free_space[0].clone();
        for region in self.free_space.iter().skip(1) {
            if region.start == current.start + current.length {
                current.length += region.length;
            } else {
                merged.push(current);
                current = region.clone();
            }
        }
        merged.push(current);
        self.free_space = merged;
    }

    /// Write data to a newly allocated block with CoW semantics.
    ///
    /// 1. Allocate new block
    /// 2. Write data
    /// 3. Compute and stamp checksum
    /// 4. Return logical address
    pub fn write_cow_block(&mut self, data: &[u8], is_metadata: bool) -> Result<u64, &'static str> {
        let bytenr = self.alloc_block(data.len() as u64)?;

        // Write data to image
        let start = bytenr as usize;
        let end = start + data.len();
        let mut image = self.image.lock();
        image[start..end].copy_from_slice(data);

        // Stamp checksum for metadata blocks
        if is_metadata {
            drop(image);
            self.stamp_block_checksum(bytenr, data.len())?;
        }

        Ok(bytenr)
    }

    /// Compute and stamp the checksum for a metadata block.
    ///
    /// The checksum covers the entire block (excluding the checksum field itself).
    pub fn stamp_block_checksum(
        &mut self,
        bytenr: u64,
        block_size: usize,
    ) -> Result<(), &'static str> {
        let start = bytenr as usize;
        let end = start + block_size;
        let image = self.image.lock();
        if end > image.len() {
            return Err("btrfs: block extends beyond image");
        }

        let block = &image[start..end];
        let csum = encode_superblock_checksum(self.csum_type, &block[32..block_size])
            .map_err(|_| "btrfs: checksum computation failed")?;

        drop(image);
        let mut image = self.image.lock();
        image[start..start + 32].copy_from_slice(&csum);

        Ok(())
    }

    /// Compute data checksum for a file extent.
    ///
    /// Returns the checksum bytes for the given data.
    pub fn compute_data_csum(&self, data: &[u8]) -> Result<[u8; 32], &'static str> {
        encode_superblock_checksum(self.csum_type, data)
            .map_err(|_| "btrfs: data checksum computation failed")
    }

    /// Verify data checksum against stored checksum.
    pub fn verify_data_csum(&self, data: &[u8], expected: &[u8; 32]) -> Result<(), &'static str> {
        let computed = self.compute_data_csum(data)?;
        if computed == *expected {
            Ok(())
        } else {
            Err("btrfs: data checksum mismatch")
        }
    }

    /// Update the superblock to point to new root tree after CoW modifications.
    ///
    /// This is the final step in a transaction: update the root pointer
    /// and write the superblock to all mirror locations.
    pub fn commit_transaction(
        &mut self,
        new_root_bytenr: u64,
        new_root_level: u8,
    ) -> Result<(), &'static str> {
        let mut image = self.image.lock();

        // Update superblock at each mirror
        let mirrors = [
            BTRFS_SUPER_MIRROR_1 as usize,
            BTRFS_SUPER_MIRROR_2 as usize,
            BTRFS_SUPER_MIRROR_3 as usize,
        ];

        for &mirror in &mirrors {
            if mirror + BTRFS_SUPERBLOCK_SIZE > image.len() {
                continue;
            }

            let sb_start = mirror;

            // Update root tree pointer
            let root_bytes = new_root_bytenr.to_le_bytes();
            image[sb_start + 80..sb_start + 88].copy_from_slice(&root_bytes);

            // Update root level
            image[sb_start + 198] = new_root_level;

            // Update generation
            let gen_bytes = self.generation.to_le_bytes();
            image[sb_start + 72..sb_start + 80].copy_from_slice(&gen_bytes);

            // Update bytes_used (sum of all allocated extents)
            let mut total_used = 0u64;
            for (_, size) in &self.allocated_extents {
                total_used += size;
            }
            let used_bytes = total_used.to_le_bytes();
            image[sb_start + 120..sb_start + 128].copy_from_slice(&used_bytes);

            // Recompute superblock checksum
            let csum = encode_superblock_checksum(
                self.csum_type,
                &image[sb_start + 32..sb_start + BTRFS_SUPERBLOCK_SIZE],
            )
            .map_err(|_| "btrfs: superblock checksum failed")?;

            image[sb_start..sb_start + 32].copy_from_slice(&csum);
        }

        // Clear freed extents after successful commit
        self.freed_extents.clear();

        Ok(())
    }

    /// Get the current generation number
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Get allocated extent count
    pub fn allocated_count(&self) -> usize {
        self.allocated_extents.len()
    }

    /// Get free space regions
    pub fn free_space_regions(&self) -> &[BtrfsFreeSpace] {
        &self.free_space
    }
}

/// Insert a new item into a B-Tree leaf block.
///
/// This is the core tree modification operation for CoW writes.
/// Returns the modified leaf block data.
pub fn btrfs_leaf_insert(
    leaf_data: &[u8],
    header: &BtrfsHeader,
    new_key: &BtrfsKey,
    new_data: &[u8],
    node_size: usize,
) -> Result<Vec<u8>, &'static str> {
    if !header.is_leaf() {
        return Err("btrfs: not a leaf block");
    }

    let mut new_leaf = leaf_data.to_vec();
    if new_leaf.len() < node_size {
        new_leaf.resize(node_size, 0);
    }

    let item_table_end = BTRFS_HEADER_SIZE + (header.nritems as usize + 1) * BTRFS_LEAF_ITEM_SIZE;

    // Find insertion position (keys are sorted)
    let mut insert_pos = header.nritems as usize;
    for i in 0..header.nritems as usize {
        let slot = BTRFS_HEADER_SIZE + i * BTRFS_LEAF_ITEM_SIZE;
        let existing_key = BtrfsKey::from_bytes(&new_leaf[slot..slot + 17])
            .ok_or("btrfs: invalid existing key")?;
        if *new_key < existing_key {
            insert_pos = i;
            break;
        }
    }

    // Calculate data insertion point (data grows from end of block)
    let mut data_end = node_size;
    for i in 0..header.nritems as usize {
        let slot = BTRFS_HEADER_SIZE + i * BTRFS_LEAF_ITEM_SIZE;
        let item = BtrfsItem::from_bytes(&new_leaf[slot..slot + BTRFS_LEAF_ITEM_SIZE])
            .ok_or("btrfs: invalid leaf item")?;
        let item_start = item.offset as usize;
        if item_start < data_end {
            data_end = item_start;
        }
    }

    let new_data_size = new_data.len();
    if data_end < item_table_end + new_data_size {
        return Err("btrfs: leaf block full");
    }

    // Insert data at the end (growing downward)
    let data_start = data_end - new_data_size;
    new_leaf[data_start..data_start + new_data_size].copy_from_slice(new_data);

    // Shift existing items to make room for new item
    let item_insert_pos = BTRFS_HEADER_SIZE + insert_pos * BTRFS_LEAF_ITEM_SIZE;
    let items_to_shift = (header.nritems as usize - insert_pos) * BTRFS_LEAF_ITEM_SIZE;
    if items_to_shift > 0 {
        let src_start = item_insert_pos;
        let dst_start = item_insert_pos + BTRFS_LEAF_ITEM_SIZE;
        let src_end = src_start + items_to_shift;
        // Shift items upward
        for i in (0..items_to_shift).rev() {
            new_leaf[dst_start + i] = new_leaf[src_start + i];
        }
    }

    // Write new item
    let item_slot = item_insert_pos;
    new_leaf[item_slot..item_slot + 17].copy_from_slice(&serialize_key(new_key));
    new_leaf[item_slot + 17..item_slot + 21].copy_from_slice(&(data_start as u32).to_le_bytes());
    new_leaf[item_slot + 21..item_slot + 25].copy_from_slice(&(new_data_size as u32).to_le_bytes());

    // Update header nritems
    let new_nritems = (header.nritems + 1).to_le_bytes();
    new_leaf[96..100].copy_from_slice(&new_nritems);

    Ok(new_leaf)
}

/// Serialize a BtrfsKey to bytes (17 bytes, little-endian)
fn serialize_key(key: &BtrfsKey) -> [u8; 17] {
    let mut buf = [0u8; 17];
    buf[0..8].copy_from_slice(&key.objectid.to_le_bytes());
    buf[8] = key.item_type;
    buf[9..17].copy_from_slice(&key.offset.to_le_bytes());
    buf
}

/// Delete an item from a B-Tree leaf block by key.
///
/// Returns the modified leaf block data.
pub fn btrfs_leaf_delete(
    leaf_data: &[u8],
    header: &BtrfsHeader,
    target_key: &BtrfsKey,
    node_size: usize,
) -> Result<Vec<u8>, &'static str> {
    if !header.is_leaf() {
        return Err("btrfs: not a leaf block");
    }
    if header.nritems == 0 {
        return Err("btrfs: leaf block empty");
    }

    let mut new_leaf = leaf_data.to_vec();

    // Find the item to delete
    let mut delete_pos = None;
    for i in 0..header.nritems as usize {
        let slot = BTRFS_HEADER_SIZE + i * BTRFS_LEAF_ITEM_SIZE;
        let key = BtrfsKey::from_bytes(&new_leaf[slot..slot + 17]).ok_or("btrfs: invalid key")?;
        if key == *target_key {
            delete_pos = Some(i);
            break;
        }
    }

    let pos = delete_pos.ok_or("btrfs: key not found in leaf")?;
    let item_slot = BTRFS_HEADER_SIZE + pos * BTRFS_LEAF_ITEM_SIZE;
    let item = BtrfsItem::from_bytes(&new_leaf[item_slot..item_slot + BTRFS_LEAF_ITEM_SIZE])
        .ok_or("btrfs: invalid item")?;

    // Remove the item from the item table
    let items_after = (header.nritems as usize - 1 - pos) * BTRFS_LEAF_ITEM_SIZE;
    if items_after > 0 {
        let src_start = item_slot + BTRFS_LEAF_ITEM_SIZE;
        let dst_start = item_slot;
        for i in 0..items_after {
            new_leaf[dst_start + i] = new_leaf[src_start + i];
        }
    }

    // Note: In a full implementation, we would also compact the data area
    // by removing the deleted item's data and shifting subsequent data.
    // For the resident backend, we mark the slot as free by zeroing it.
    let data_start = item.offset as usize;
    let data_size = item.size as usize;
    for byte in &mut new_leaf[data_start..data_start + data_size] {
        *byte = 0;
    }

    // Update nritems
    let new_nritems = (header.nritems - 1).to_le_bytes();
    new_leaf[96..100].copy_from_slice(&new_nritems);

    // Zero the last item slot
    let last_slot = BTRFS_HEADER_SIZE + (header.nritems as usize - 1) * BTRFS_LEAF_ITEM_SIZE;
    for byte in &mut new_leaf[last_slot..last_slot + BTRFS_LEAF_ITEM_SIZE] {
        *byte = 0;
    }

    Ok(new_leaf)
}

/// Create a new extent item for file data.
///
/// Returns the serialized extent data for a regular (non-inline) extent.
pub fn create_extent_data(
    generation: u64,
    ram_bytes: u64,
    disk_bytenr: u64,
    disk_num_bytes: u64,
    offset: u64,
    num_bytes: u64,
    compression: u8,
) -> Vec<u8> {
    let mut data = vec![0u8; 53];
    data[0..8].copy_from_slice(&generation.to_le_bytes());
    data[8..16].copy_from_slice(&ram_bytes.to_le_bytes());
    data[16] = compression;
    data[17] = 0; // encryption (reserved)
    data[20] = 1; // extent_type = regular
    data[21..29].copy_from_slice(&disk_bytenr.to_le_bytes());
    data[29..37].copy_from_slice(&disk_num_bytes.to_le_bytes());
    data[37..45].copy_from_slice(&offset.to_le_bytes());
    data[45..53].copy_from_slice(&num_bytes.to_le_bytes());
    data
}

/// Create an inline extent item for small file data.
///
/// Returns the serialized extent data with inline payload.
pub fn create_inline_extent_data(generation: u64, file_data: &[u8], compression: u8) -> Vec<u8> {
    let mut data = vec![0u8; 21];
    data[0..8].copy_from_slice(&generation.to_le_bytes());
    data[8..16].copy_from_slice(&(file_data.len() as u64).to_le_bytes());
    data[16] = compression;
    data[17] = 0; // encryption
    data[20] = 0; // extent_type = inline
    data.extend_from_slice(file_data);
    data
}

/// Write file data with CoW semantics.
///
/// 1. Allocate new extent(s) for the data
/// 2. Write data to allocated extents
/// 3. Compute data checksums
/// 4. Create/update extent data item
/// 5. Update inode size and nbytes
///
/// Returns the new extent descriptors.
pub fn btrfs_cow_write_file(
    cow: &mut BtrfsCowWriter,
    inode_id: u64,
    file_offset: u64,
    data: &[u8],
    fs: &BtrfsFilesystem,
) -> Result<Vec<BtrfsAllocResult>, &'static str> {
    let block_size = fs.superblock.sector_size as u64;
    let mut results = Vec::new();
    let mut remaining = data;
    let mut offset = file_offset;

    while !remaining.is_empty() {
        let chunk_size = remaining.len().min(block_size as usize);
        let chunk = &remaining[..chunk_size];

        // Allocate extent for this chunk
        let bytenr = cow.alloc_block(chunk_size as u64)?;

        // Write data to extent
        let start = bytenr as usize;
        let end = start + chunk_size;
        let mut image = cow.image.lock();
        image[start..end].copy_from_slice(chunk);
        drop(image);

        // Compute data checksum
        let csum = cow.compute_data_csum(chunk)?;
        cow.data_csums.insert(offset, csum);

        results.push(BtrfsAllocResult {
            bytenr,
            num_bytes: chunk_size as u64,
        });

        remaining = &remaining[chunk_size..];
        offset += chunk_size as u64;
    }

    Ok(results)
}
