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
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use sha2::{Digest, Sha256};
use spin::Mutex;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

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
    /// Label (256 bytes max)
    pub label: [u8; 256],
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
            sys_chunk_array_size: u32::from_le_bytes([data[160], data[161], data[162], data[163]]),
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
            label,
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

        acc = v1.rotate_left(1)
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

pub fn scrub_superblock_mirrors_with_layout(
    disk_data: &[u8],
    mirrors: &[u64],
) -> BtrfsScrubReport {
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
    if let Some(existing) = volumes.iter_mut().find(|entry| entry.mount_point == mount_point) {
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
pub struct BtrfsFilesystem {
    /// Parse edilmiş superblock
    pub superblock: BtrfsSuperblock,
    /// Subvolume listesi
    pub subvolumes: Vec<BtrfsSubvolume>,
    /// Inode cache (objectid → inode item)
    pub inode_cache: BTreeMap<u64, BtrfsInodeItem>,
    /// Chunk map (logical → physical)
    pub chunk_map: Vec<(u64, u64, BtrfsChunkItem)>, // (logical_start, logical_end, chunk)
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

    /// Snapshot oluşturur (O(1) CoW klon)
    pub fn create_snapshot(&mut self, source_id: u64, name: &str, readonly: bool) -> u64 {
        let snap_id = BTRFS_FIRST_FREE_OBJECTID + self.subvolumes.len() as u64;
        self.subvolumes.push(BtrfsSubvolume {
            id: snap_id,
            parent_id: source_id,
            name: String::from(name),
            generation: self.superblock.generation,
            readonly,
            root_generation: self.superblock.generation,
        });

        crate::serial_println!(
            "[Btrfs] Snapshot created: {} (id={}, parent={}, ro={})",
            name,
            snap_id,
            source_id,
            readonly
        );

        snap_id
    }

    /// Logical adres → physical adrese çevirir
    pub fn logical_to_physical(&self, logical: u64) -> Option<u64> {
        for (start, end, chunk) in &self.chunk_map {
            if logical >= *start && logical < *end {
                let offset = logical - start;
                if let Some(stripe) = chunk.stripes.first() {
                    return Some(stripe.offset + offset);
                }
            }
        }
        None
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
        crate::serial_println!("[Btrfs] Mount: {}", self.mount_point);
    }
}

// ============================================================================
// Global Registry
// ============================================================================

lazy_static::lazy_static! {
    static ref BTRFS_FILESYSTEMS: Mutex<Vec<BtrfsFilesystem>> = Mutex::new(Vec::new());
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
    if disk_data.len() < BTRFS_SUPER_OFFSET + BTRFS_SUPERBLOCK_SIZE {
        return Err("Disk too small for Btrfs");
    }

    let scrub = scrub_superblock_mirrors(disk_data);
    let mirror_offset = scrub.selected_mirror.ok_or("Invalid Btrfs superblock")?;
    let sb = BtrfsSuperblock::from_bytes(
        &disk_data[mirror_offset as usize..mirror_offset as usize + BTRFS_SUPERBLOCK_SIZE],
    )
    .ok_or("Invalid Btrfs superblock")?;

    let mut fs = BtrfsFilesystem::new(sb, mount_point);
    fs.add_default_subvolume();
    fs.print_info();

    BTRFS_FILESYSTEMS.lock().push(fs);
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
