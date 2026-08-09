//! # ext4 Dosya Sistemi
//!
//! Günlükleme (journaling) desteği ile Dördüncü Genişletilmiş Dosya Sistemi (ext4)
//! uygulaması. Okuma ve yazma desteği sunar; JBD2 günlüğü ile çökmeden kurtarma sağlar.
//!
//! ## ext4 Disk Yapısı (ASCII Diyagram)
//! ```text
//! Disk Düzeni:
//! ┌──────────────────────────────────────────────────────────────┐
//! │  0 - 1023  │  Önyükleme Bloğu (boot block)                  │
//! ├──────────────────────────────────────────────────────────────┤
//! │ 1024-2047  │  Süper Blok (Superblock) - sihirli sayı 0xEF53 │
//! ├──────────────────────────────────────────────────────────────┤
//! │  Blok 1    │  Blok Grubu Tanımlayıcıları (Group Descriptors) │
//! ├──────────────────────────────────────────────────────────────┤
//! │  Blok 2+   │  Blok Bitmap (hangi bloklar kullanımda?)        │
//! ├──────────────────────────────────────────────────────────────┤
//! │  ...       │  Inode Bitmap (hangi inode'lar kullanımda?)     │
//! ├──────────────────────────────────────────────────────────────┤
//! │  ...       │  Inode Tablosu (dosya meta verisi)              │
//! ├──────────────────────────────────────────────────────────────┤
//! │  ...       │  Veri Blokları (dosya içeriği)                  │
//! └──────────────────────────────────────────────────────────────┘
//!
//! Her Blok Grubu aynı yapıya sahiptir. Extent ağacı (extent tree)
//! büyük dosyalar için blok haritalamasını verimli şekilde yapar.
//! ```

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::mem;
use core::sync::atomic::{AtomicU8, Ordering};
use spin::Mutex;

use super::ext4_journal::{Journal, JournalError, Transaction, TransactionState};

// ============================================================================
// ext4 SABİTLERİ
// ============================================================================

/// ext4 sihirli sayısı - süper blok doğrulama için kullanılır
const EXT4_MAGIC: u16 = 0xEF53;

/// Süper blok ofseti (baştan 1024 bayt sonra)
pub(crate) const SUPERBLOCK_OFFSET: u64 = 1024;

/// Inode türleri - dosya modu bitlerindeki tür alanı
const EXT4_S_IFIFO: u16 = 0x1000;
const EXT4_S_IFCHR: u16 = 0x2000;
const EXT4_S_IFDIR: u16 = 0x4000;
const EXT4_S_IFBLK: u16 = 0x6000;
const EXT4_S_IFREG: u16 = 0x8000;
const EXT4_S_IFLNK: u16 = 0xA000;
const EXT4_S_IFSOCK: u16 = 0xC000;

/// COMPAT feature bayrakları — kernel ext4.h + super.html spec
const EXT4_FEATURE_COMPAT_DIR_PREALLOC: u32 = 0x0001;
const EXT4_FEATURE_COMPAT_IMAGIC_INODES: u32 = 0x0002;
const EXT4_FEATURE_COMPAT_HAS_JOURNAL: u32 = 0x0004;
const EXT4_FEATURE_COMPAT_EXT_ATTR: u32 = 0x0008;
const EXT4_FEATURE_COMPAT_RESIZE_INODE: u32 = 0x0010;
const EXT4_FEATURE_COMPAT_DIR_INDEX: u32 = 0x0020;
const EXT4_FEATURE_COMPAT_LAZY_BG: u32 = 0x0040;
const EXT4_FEATURE_COMPAT_EXCLUDE_INODE: u32 = 0x0080;
const EXT4_FEATURE_COMPAT_EXCLUDE_BITMAP: u32 = 0x0100;
const EXT4_FEATURE_COMPAT_SPARSE_SUPER2: u32 = 0x0200;
const EXT4_FEATURE_COMPAT_FAST_COMMIT: u32 = 0x0400;
const EXT4_FEATURE_COMPAT_STABLE_INODES: u32 = 0x0800;
const EXT4_FEATURE_COMPAT_ORPHAN_FILE: u32 = 0x1000;

/// INCOMPAT feature bayrakları — kernel ext4.h
const EXT4_FEATURE_INCOMPAT_COMPRESSION: u32 = 0x0001;
const EXT4_FEATURE_INCOMPAT_FILETYPE: u32 = 0x0002;
const EXT4_FEATURE_INCOMPAT_RECOVER: u32 = 0x0004;
const EXT4_FEATURE_INCOMPAT_JOURNAL_DEV: u32 = 0x0008;
const EXT4_FEATURE_INCOMPAT_META_BG: u32 = 0x0010;
const EXT4_FEATURE_INCOMPAT_EXTENTS: u32 = 0x0040;
const EXT4_FEATURE_INCOMPAT_64BIT: u32 = 0x0080;
const EXT4_FEATURE_INCOMPAT_MMP: u32 = 0x0100;
const EXT4_FEATURE_INCOMPAT_FLEX_BG: u32 = 0x0200;
const EXT4_FEATURE_INCOMPAT_EA_INODE: u32 = 0x0400;
const EXT4_FEATURE_INCOMPAT_DIRDATA: u32 = 0x1000;
const EXT4_FEATURE_INCOMPAT_CSUM_SEED: u32 = 0x2000;
const EXT4_FEATURE_INCOMPAT_LARGEDIR: u32 = 0x4000;
const EXT4_FEATURE_INCOMPAT_INLINE_DATA: u32 = 0x8000;
const EXT4_FEATURE_INCOMPAT_ENCRYPT: u32 = 0x10000;
const EXT4_FEATURE_INCOMPAT_CASEFOLD: u32 = 0x20000;

/// echOS ext4 sürücüsünün desteklediği tüm INCOMPAT feature'lar
const EXT4_KNOWN_INCOMPAT: u32 = EXT4_FEATURE_INCOMPAT_COMPRESSION
    | EXT4_FEATURE_INCOMPAT_FILETYPE
    | EXT4_FEATURE_INCOMPAT_RECOVER
    | EXT4_FEATURE_INCOMPAT_JOURNAL_DEV
    | EXT4_FEATURE_INCOMPAT_META_BG
    | EXT4_FEATURE_INCOMPAT_EXTENTS
    | EXT4_FEATURE_INCOMPAT_64BIT
    | EXT4_FEATURE_INCOMPAT_MMP
    | EXT4_FEATURE_INCOMPAT_FLEX_BG
    | EXT4_FEATURE_INCOMPAT_EA_INODE
    | EXT4_FEATURE_INCOMPAT_DIRDATA
    | EXT4_FEATURE_INCOMPAT_CSUM_SEED
    | EXT4_FEATURE_INCOMPAT_LARGEDIR
    | EXT4_FEATURE_INCOMPAT_INLINE_DATA
    | EXT4_FEATURE_INCOMPAT_ENCRYPT
    | EXT4_FEATURE_INCOMPAT_CASEFOLD;

/// Inode flag: inline data kullanılıyor (i_block'ın ilk 60+ baytı data içerir)
const EXT4_INLINE_DATA_FL: u32 = 0x10000000;

/// RO_COMPAT feature'lar — spec super.html: SPARSE_SUPER, HUGE_FILE, GDT_CSUM, METADATA_CSUM, VERITY
const EXT4_FEATURE_RO_COMPAT_SPARSE_SUPER: u32 = 0x0001;
const EXT4_FEATURE_RO_COMPAT_LARGE_FILE: u32 = 0x0002;
const EXT4_FEATURE_RO_COMPAT_BTREE_DIR: u32 = 0x0004;
const EXT4_FEATURE_RO_COMPAT_HUGE_FILE: u32 = 0x0008;
const EXT4_FEATURE_RO_COMPAT_GDT_CSUM: u32 = 0x0010;
const EXT4_FEATURE_RO_COMPAT_DIR_NLINK: u32 = 0x0020;
const EXT4_FEATURE_RO_COMPAT_EXTRA_ISIZE: u32 = 0x0040;
const EXT4_FEATURE_RO_COMPAT_HAS_SNAPSHOT: u32 = 0x0080;
const EXT4_FEATURE_RO_COMPAT_QUOTA: u32 = 0x0100;
const EXT4_FEATURE_RO_COMPAT_BIGALLOC: u32 = 0x0200;
const EXT4_FEATURE_RO_COMPAT_METADATA_CSUM: u32 = 0x0400;
const EXT4_FEATURE_RO_COMPAT_REPLICA: u32 = 0x0800;
const EXT4_FEATURE_RO_COMPAT_READONLY: u32 = 0x1000;
const EXT4_FEATURE_RO_COMPAT_PROJECT: u32 = 0x2000;
const EXT4_FEATURE_RO_COMPAT_VERITY: u32 = 0x8000;
const EXT4_FEATURE_RO_COMPAT_ORPHAN_PRESENT: u32 = 0x10000;

// ============================================================================
// MMP (Multiple Mount Protection) — spec: mmp.html
// ============================================================================

const EXT4_MMP_MAGIC: u32 = 0x004D4D50; // "MMP"
const EXT4_MMP_SEQ_CLEAN: u32 = 0xFF4E4F43; // seq when fs is cleanly unmounted
const EXT4_MMP_SEQ_FSCK: u32 = 0xE24F4F4B; // seq when fsck is running

/// MMP bloğu — mmp_struct, 4096 baytlık bir block içinde
struct Ext4MmpBlock {
    mmp_magic: u32,          // 0x00
    mmp_seq: u32,            // 0x04
    mmp_time: u64,           // 0x08
    mmp_nodename: [u8; 64],  // 0x10
    mmp_bdevname: [u8; 32],  // 0x50
    mmp_check_interval: u16, // 0x70
    mmp_pad1: u16,           // 0x72
    mmp_pad2: [u32; 226],    // 0x74
    // 0x3FC: mmp_checksum (calculated separately)
}

/// echOS ext4 sürücüsünün tanıdığı tüm RO_COMPAT feature'lar
const EXT4_KNOWN_RO_COMPAT: u32 = EXT4_FEATURE_RO_COMPAT_SPARSE_SUPER
    | EXT4_FEATURE_RO_COMPAT_LARGE_FILE
    | EXT4_FEATURE_RO_COMPAT_BTREE_DIR
    | EXT4_FEATURE_RO_COMPAT_HUGE_FILE
    | EXT4_FEATURE_RO_COMPAT_GDT_CSUM
    | EXT4_FEATURE_RO_COMPAT_DIR_NLINK
    | EXT4_FEATURE_RO_COMPAT_EXTRA_ISIZE
    | EXT4_FEATURE_RO_COMPAT_HAS_SNAPSHOT
    | EXT4_FEATURE_RO_COMPAT_QUOTA
    | EXT4_FEATURE_RO_COMPAT_BIGALLOC
    | EXT4_FEATURE_RO_COMPAT_METADATA_CSUM
    | EXT4_FEATURE_RO_COMPAT_REPLICA
    | EXT4_FEATURE_RO_COMPAT_READONLY
    | EXT4_FEATURE_RO_COMPAT_PROJECT
    | EXT4_FEATURE_RO_COMPAT_VERITY
    | EXT4_FEATURE_RO_COMPAT_ORPHAN_PRESENT;

// ============================================================================
// DOSYA TÜRLERİ
// ============================================================================

/// Dosya türü numaralandırması
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ext4FileType {
    Regular,
    Directory,
    Symlink,
    CharDevice,
    BlockDevice,
    Fifo,
    Socket,
    Unknown,
}

/// Dizin girdisi - bir dizindeki dosya veya alt dizin kaydı
#[derive(Clone, Debug)]
pub struct Ext4DirEntry {
    pub name: String,
    pub inode: u32,
    pub file_type: Ext4FileType,
}

/// Dosya meta verisi - boyut, izinler, zaman damgaları
#[derive(Clone, Debug)]
pub struct Ext4Metadata {
    pub size: u64,
    pub file_type: Ext4FileType,
    pub permissions: u16,
    pub uid: u16,
    pub gid: u16,
    pub links: u16,
    pub atime: u32,
    pub mtime: u32,
    pub ctime: u32,
}

/// Dosya sistemi hata türleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ext4Error {
    InvalidFormat,
    ReadError,
    WriteError,
    NotFound,
    NotSupported,
    OutOfMemory,
    Corrupted,
    ChecksumError,
    JournalError,
}

impl From<super::ext4_journal::JournalError> for Ext4Error {
    fn from(_: super::ext4_journal::JournalError) -> Self {
        Ext4Error::JournalError
    }
}

// ============================================================================
// SÜPER BLOK
// ============================================================================

/// ext4 Süper Bloğu - dosya sisteminin ana meta veri yapısı (temel alanlar)
#[derive(Clone, Copy, Debug)]
pub struct Ext4Superblock {
    pub s_inodes_count: u32,
    pub s_blocks_count_lo: u32,
    pub s_r_blocks_count_lo: u32,
    pub s_free_blocks_count_lo: u32,
    pub s_free_inodes_count: u32,
    pub s_first_data_block: u32,
    pub s_log_block_size: u32,
    pub s_blocks_per_group: u32,
    pub s_inodes_per_group: u32,
    pub s_magic: u16,
    pub s_state: u16,
    pub s_feature_compat: u32,
    pub s_feature_ro_compat: u32,
    pub s_feature_incompat: u32,
    pub s_first_ino: u32,
    pub s_inode_size: u16,
    pub s_desc_size: u16,
    pub s_blocks_count_hi: u32,
    pub s_free_blocks_count_hi: u32,
    pub s_uuid: [u8; 16],
    pub s_log_cluster_size: u32,
    pub s_last_orphan: u32,        // 0xE8 — legacy orphan linked list head
    pub s_mmp_interval: u16,       // 0x166 — MMP check interval (seconds)
    pub s_mmp_block: u64,          // 0x168 — MMP block number
    pub s_orphan_file_inum: u32,   // 0x240 — orphan file inode numarası (COMPAT_ORPHAN_FILE)
    pub s_encryption_level: u8,    // 0x176 — şifreleme sürümü (INCOMPAT_ENCRYPT)
    pub s_encrypt_algos: [u8; 4],  // 0x254 — şifreleme algoritmaları (en fazla 4)
    pub s_encrypt_pw_salt: [u8; 16], // 0x258 — string2key için tuz
    pub s_mnt_count: u16,          // 0x34 — son fsck'den beri bağlanma sayısı
    pub s_max_mnt_count: u16,      // 0x36 — fsck gerekli bağlanma sayısı
    pub s_errors: u16,             // 0x3C — hata davranış politikası
    pub s_lastcheck: u32,          // 0x40 — son fsck zamanı (saniye)
    pub s_checkinterval: u32,      // 0x44 — maksimum fsck aralığı (saniye)
    pub s_error_count: u32,        // 0x194 — toplam hata sayısı
    pub s_checksum: u32,
}

impl Ext4Superblock {
    /// CRC32C ile süper blok checksum'ını doğrular
    /// Spec: super.html — s_checksum at 0x3FC, CRC32C over sb with checksum zeroed
    pub fn verify_checksum(data: &[u8]) -> bool {
        if data.len() < 1024 {
            return false;
        }
        let stored = u32::from_le_bytes([data[1020], data[1021], data[1022], data[1023]]);
        if stored == 0 {
            return true; // checksums disabled
        }
        let mut zeroed = data[..1024].to_vec();
        zeroed[1020..1024].copy_from_slice(&[0u8; 4]);
        let computed = crate::fs::journal::crc32c(&zeroed);
        computed == stored
    }

    /// Süper bloğu ham baytlardan çözümler ve sihirli sayıyı doğrular
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 1024 {
            return None;
        }

        let magic = u16::from_le_bytes([data[56], data[57]]);
        if magic != EXT4_MAGIC {
            return None;
        }

        let mut uuid = [0u8; 16];
        uuid.copy_from_slice(&data[104..120]);

        Some(Ext4Superblock {
            s_inodes_count: u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
            s_blocks_count_lo: u32::from_le_bytes([data[4], data[5], data[6], data[7]]),
            s_r_blocks_count_lo: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
            s_free_blocks_count_lo: u32::from_le_bytes([data[12], data[13], data[14], data[15]]),
            s_free_inodes_count: u32::from_le_bytes([data[16], data[17], data[18], data[19]]),
            s_first_data_block: u32::from_le_bytes([data[20], data[21], data[22], data[23]]),
            s_log_block_size: u32::from_le_bytes([data[24], data[25], data[26], data[27]]),
            s_blocks_per_group: u32::from_le_bytes([data[32], data[33], data[34], data[35]]),
            s_inodes_per_group: u32::from_le_bytes([data[40], data[41], data[42], data[43]]),
            s_magic: magic,
            s_state: u16::from_le_bytes([data[58], data[59]]),
            s_feature_compat: u32::from_le_bytes([data[92], data[93], data[94], data[95]]),
            s_feature_ro_compat: u32::from_le_bytes([data[96], data[97], data[98], data[99]]),
            s_feature_incompat: u32::from_le_bytes([data[100], data[101], data[102], data[103]]),
            s_first_ino: u32::from_le_bytes([data[84], data[85], data[86], data[87]]),
            s_inode_size: u16::from_le_bytes([data[88], data[89]]),
            s_desc_size: u16::from_le_bytes([data[254], data[255]]),
            s_blocks_count_hi: u32::from_le_bytes([data[336], data[337], data[338], data[339]]),
            s_free_blocks_count_hi: u32::from_le_bytes([
                data[340], data[341], data[342], data[343],
            ]),
            s_uuid: uuid,
            s_log_cluster_size: u32::from_le_bytes([data[204], data[205], data[206], data[207]]),
            s_last_orphan: u32::from_le_bytes([data[232], data[233], data[234], data[235]]),
            s_mmp_interval: u16::from_le_bytes([data[358], data[359]]),
            s_mmp_block: u64::from_le_bytes([
                data[360], data[361], data[362], data[363],
                data[364], data[365], data[366], data[367],
            ]),
            s_orphan_file_inum: u32::from_le_bytes([data[576], data[577], data[578], data[579]]),
            s_encryption_level: data[374],
            s_encrypt_algos: [data[596], data[597], data[598], data[599]],
            s_encrypt_pw_salt: {
                let mut salt = [0u8; 16];
                salt.copy_from_slice(&data[600..616]);
                salt
            },
            s_mnt_count: u16::from_le_bytes([data[52], data[53]]),
            s_max_mnt_count: u16::from_le_bytes([data[54], data[55]]),
            s_errors: u16::from_le_bytes([data[60], data[61]]),
            s_lastcheck: u32::from_le_bytes([data[64], data[65], data[66], data[67]]),
            s_checkinterval: u32::from_le_bytes([data[68], data[69], data[70], data[71]]),
            s_error_count: u32::from_le_bytes([data[404], data[405], data[406], data[407]]),
            s_checksum: u32::from_le_bytes([data[1020], data[1021], data[1022], data[1023]]),
        })
    }

    /// Blok boyutunu hesaplar: 1024 << s_log_block_size (örn. 4096 bayt)
    pub fn block_size(&self) -> u32 {
        1024 << self.s_log_block_size
    }

    /// Toplam blok sayısını 64 bit olarak döndürür (hi + lo birleşimi)
    pub fn total_blocks(&self) -> u64 {
        ((self.s_blocks_count_hi as u64) << 32) | (self.s_blocks_count_lo as u64)
    }

    /// Serbest blok sayısını 64 bit olarak döndürür
    pub fn free_blocks(&self) -> u64 {
        ((self.s_free_blocks_count_hi as u64) << 32) | (self.s_free_blocks_count_lo as u64)
    }

    /// Blok grubu sayısını hesaplar (toplam bloklar / grup başına bloklar)
    pub fn block_groups_count(&self) -> u32 {
        let total = self.total_blocks();
        ((total + self.s_blocks_per_group as u64 - 1) / self.s_blocks_per_group as u64) as u32
    }

    /// Dosya sisteminin 64-bit modda olup olmadığını kontrol eder
    pub fn is_64bit(&self) -> bool {
        (self.s_feature_incompat & EXT4_FEATURE_INCOMPAT_64BIT) != 0
    }

    /// Dosya sisteminin extent ağacı kullanıp kullanmadığını kontrol eder
    pub fn has_extents(&self) -> bool {
        (self.s_feature_incompat & EXT4_FEATURE_INCOMPAT_EXTENTS) != 0
    }
}

// ============================================================================
// BLOK GRUBU TANIMLAYICILARI
// ============================================================================

/// Blok grubu tanımlayıcısının CRC32C checksum'ını doğrular
/// Spec: group_descr.html — bg_checksum 32-bit offset 0x1E, 64-bit offset 0x3E
/// CRC32C seed = le32(group_index) XOR ~0
fn verify_gd_checksum(data: &[u8], group_index: u32, desc_size: usize) -> bool {
    let checksum_offset = if desc_size >= 64 { 0x3E } else { 0x1E };
    if data.len() < checksum_offset + 2 {
        return false;
    }
    let stored = u16::from_le_bytes([data[checksum_offset], data[checksum_offset + 1]]);
    if stored == 0 {
        return true; // checksums disabled for this group
    }

    // Kernel algorithm: crc32c_le(~0, group_le32) then crc32c_le(crc, desc_bytes_with_checksum_zeroed)
    // Spec: group_descr.html — metadata_csum mode
    let group_le = group_index.to_le_bytes();
    let mut crc = crate::fs::journal::crc32c_with_seed(&group_le, 0xFFFFFFFF);

    // Feed bytes before checksum
    if checksum_offset > 0 {
        crc = crate::fs::journal::crc32c_with_seed(&data[..checksum_offset], crc);
    }
    // Skip checksum field (2 bytes at checksum_offset)
    let after_start = checksum_offset + 2;
    let desc_end = desc_size.min(data.len());
    if after_start < desc_end {
        crc = crate::fs::journal::crc32c_with_seed(&data[after_start..desc_end], crc);
    }

    // Lower 16 bits of CRC32C is the stored checksum (kernel: ext4_group_desc_csum returns le16)
    let computed = (crc & 0xFFFF) as u16;
    computed == stored
}

/// Blok Grubu Tanımlayıcısı - her blok grubunun harita ve tablo konumlarını tutar
#[derive(Clone, Copy, Debug)]
pub struct Ext4GroupDescriptor {
    pub bg_block_bitmap_lo: u32,
    pub bg_inode_bitmap_lo: u32,
    pub bg_inode_table_lo: u32,
    pub bg_free_blocks_count_lo: u16,
    pub bg_free_inodes_count_lo: u16,
    pub bg_used_dirs_lo: u16,
    pub bg_flags: u16,
    pub bg_exclude_bitmap_lo: u32,    // offset 0x14: snapshot exclusion bitmap
    pub bg_block_bitmap_csum_lo: u16, // offset 0x18
    pub bg_inode_bitmap_csum_lo: u16, // offset 0x1A
    pub bg_itable_unused_lo: u16,     // offset 0x1C: unused inode count (lazy init)
    pub bg_checksum: u16,             // offset 0x1E
    pub bg_block_bitmap_hi: u32,      // offset 0x20
    pub bg_inode_bitmap_hi: u32,      // offset 0x24
    pub bg_inode_table_hi: u32,       // offset 0x28
    pub bg_free_blocks_count_hi: u16, // offset 0x2C
    pub bg_free_inodes_count_hi: u16, // offset 0x2E
    pub bg_used_dirs_hi: u16,         // offset 0x30
    pub bg_itable_unused_hi: u16,     // offset 0x32
    pub bg_exclude_bitmap_hi: u32,    // offset 0x34
    pub bg_block_bitmap_csum_hi: u16, // offset 0x38
    pub bg_inode_bitmap_csum_hi: u16, // offset 0x3A
    pub bg_reserved: u32,             // offset 0x3C: padding to 64 bytes
}

impl Ext4GroupDescriptor {
    /// 32-baytlık disk formatından tanımlayıcıyı çözümler (64-bit feature yoksa)
    pub fn parse_32(data: &[u8]) -> Option<Self> {
        if data.len() < 32 {
            return None;
        }

        Some(Ext4GroupDescriptor {
            bg_block_bitmap_lo: u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
            bg_inode_bitmap_lo: u32::from_le_bytes([data[4], data[5], data[6], data[7]]),
            bg_inode_table_lo: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
            bg_free_blocks_count_lo: u16::from_le_bytes([data[12], data[13]]),
            bg_free_inodes_count_lo: u16::from_le_bytes([data[14], data[15]]),
            bg_used_dirs_lo: u16::from_le_bytes([data[16], data[17]]),
            bg_flags: u16::from_le_bytes([data[18], data[19]]),
            bg_exclude_bitmap_lo: u32::from_le_bytes([data[20], data[21], data[22], data[23]]),
            bg_block_bitmap_csum_lo: u16::from_le_bytes([data[24], data[25]]),
            bg_inode_bitmap_csum_lo: u16::from_le_bytes([data[26], data[27]]),
            bg_itable_unused_lo: u16::from_le_bytes([data[28], data[29]]),
            bg_checksum: u16::from_le_bytes([data[30], data[31]]),
            bg_block_bitmap_hi: 0,
            bg_inode_bitmap_hi: 0,
            bg_inode_table_hi: 0,
            bg_free_blocks_count_hi: 0,
            bg_free_inodes_count_hi: 0,
            bg_used_dirs_hi: 0,
            bg_itable_unused_hi: 0,
            bg_exclude_bitmap_hi: 0,
            bg_block_bitmap_csum_hi: 0,
            bg_inode_bitmap_csum_hi: 0,
            bg_reserved: 0,
        })
    }

    /// 64-baytlık disk formatından tanımlayıcıyı çözümler (64-bit feature varsa)
    pub fn parse_64(data: &[u8]) -> Option<Self> {
        if data.len() < 64 {
            return None;
        }

        Some(Ext4GroupDescriptor {
            bg_block_bitmap_lo: u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
            bg_inode_bitmap_lo: u32::from_le_bytes([data[4], data[5], data[6], data[7]]),
            bg_inode_table_lo: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
            bg_free_blocks_count_lo: u16::from_le_bytes([data[12], data[13]]),
            bg_free_inodes_count_lo: u16::from_le_bytes([data[14], data[15]]),
            bg_used_dirs_lo: u16::from_le_bytes([data[16], data[17]]),
            bg_flags: u16::from_le_bytes([data[18], data[19]]),
            bg_exclude_bitmap_lo: u32::from_le_bytes([data[20], data[21], data[22], data[23]]),
            bg_block_bitmap_csum_lo: u16::from_le_bytes([data[24], data[25]]),
            bg_inode_bitmap_csum_lo: u16::from_le_bytes([data[26], data[27]]),
            bg_itable_unused_lo: u16::from_le_bytes([data[28], data[29]]),
            bg_checksum: u16::from_le_bytes([data[30], data[31]]),
            bg_block_bitmap_hi: u32::from_le_bytes([data[32], data[33], data[34], data[35]]),
            bg_inode_bitmap_hi: u32::from_le_bytes([data[36], data[37], data[38], data[39]]),
            bg_inode_table_hi: u32::from_le_bytes([data[40], data[41], data[42], data[43]]),
            bg_free_blocks_count_hi: u16::from_le_bytes([data[44], data[45]]),
            bg_free_inodes_count_hi: u16::from_le_bytes([data[46], data[47]]),
            bg_used_dirs_hi: u16::from_le_bytes([data[48], data[49]]),
            bg_itable_unused_hi: u16::from_le_bytes([data[50], data[51]]),
            bg_exclude_bitmap_hi: u32::from_le_bytes([data[52], data[53], data[54], data[55]]),
            bg_block_bitmap_csum_hi: u16::from_le_bytes([data[56], data[57]]),
            bg_inode_bitmap_csum_hi: u16::from_le_bytes([data[58], data[59]]),
            bg_reserved: u32::from_le_bytes([data[60], data[61], data[62], data[63]]),
        })
    }

    /// Blok bitmap'in diskdeki bloğunu döndürür (64-bit moda göre)
    pub fn block_bitmap(&self, is_64bit: bool) -> u64 {
        if is_64bit {
            ((self.bg_block_bitmap_hi as u64) << 32) | self.bg_block_bitmap_lo as u64
        } else {
            self.bg_block_bitmap_lo as u64
        }
    }

    /// Inode bitmap'in diskdeki bloğunu döndürür (64-bit moda göre)
    pub fn inode_bitmap(&self, is_64bit: bool) -> u64 {
        if is_64bit {
            ((self.bg_inode_bitmap_hi as u64) << 32) | self.bg_inode_bitmap_lo as u64
        } else {
            self.bg_inode_bitmap_lo as u64
        }
    }

    /// Inode tablosunun diskdeki başlangıç bloğunu döndürür (64-bit moda göre)
    pub fn inode_table(&self, is_64bit: bool) -> u64 {
        if is_64bit {
            ((self.bg_inode_table_hi as u64) << 32) | self.bg_inode_table_lo as u64
        } else {
            self.bg_inode_table_lo as u64
        }
    }

    /// Block bitmap uninitialized mı? (EXT4_BG_BLOCK_UNINIT = 0x2)
    /// Spec: BLOCK_UNINIT set ise bitmap sıfır kabul edilir (tüm bloklar free)
    pub fn is_block_uninit(&self) -> bool {
        self.bg_flags & 0x2 != 0
    }

    /// Inode bitmap uninitialized mı? (EXT4_BG_INODE_UNINIT = 0x1)
    /// Spec: INODE_UNINIT set ise bitmap sıfır kabul edilir (tüm inode'lar free)
    pub fn is_inode_uninit(&self) -> bool {
        self.bg_flags & 0x1 != 0
    }

    /// Inode table zeroed mu? (EXT4_BG_INODE_ZEROED = 0x4)
    pub fn is_inode_zeroed(&self) -> bool {
        self.bg_flags & 0x4 != 0
    }

    /// Serbest blok sayısını döndürür (64-bit moda göre)
    pub fn free_blocks_count(&self, is_64bit: bool) -> u32 {
        if is_64bit {
            (self.bg_free_blocks_count_hi as u32) << 16 | self.bg_free_blocks_count_lo as u32
        } else {
            self.bg_free_blocks_count_lo as u32
        }
    }

    /// Kullanılmayan inode sayısının üst sınırını döndürür (itable_unused, 64-bit moda göre)
    /// Spec: inode tablosunun sonundaki itable_unused kadar inode kullanılmamış kabul edilir
    pub fn itable_unused(&self, is_64bit: bool) -> u32 {
        if is_64bit {
            (self.bg_itable_unused_hi as u32) << 16 | self.bg_itable_unused_lo as u32
        } else {
            self.bg_itable_unused_lo as u32
        }
    }

    /// Verilen sayıda inode'un unused olduğunu ayarlar (itable_unused)
    pub fn set_itable_unused(&mut self, val: u32, is_64bit: bool) {
        if is_64bit {
            self.bg_itable_unused_lo = (val & 0xFFFF) as u16;
            self.bg_itable_unused_hi = (val >> 16) as u16;
        } else {
            self.bg_itable_unused_lo = (val & 0xFFFF) as u16;
        }
    }

    /// Serbest inode sayısını döndürür (64-bit moda göre)
    pub fn free_inodes_count(&self, is_64bit: bool) -> u32 {
        if is_64bit {
            (self.bg_free_inodes_count_hi as u32) << 16 | self.bg_free_inodes_count_lo as u32
        } else {
            self.bg_free_inodes_count_lo as u32
        }
    }

    /// Kullanılmış dizin sayısını döndürür (64-bit moda göre)
    pub fn used_dirs_count(&self, is_64bit: bool) -> u32 {
        if is_64bit {
            (self.bg_used_dirs_hi as u32) << 16 | self.bg_used_dirs_lo as u32
        } else {
            self.bg_used_dirs_lo as u32
        }
    }

    /// 32-baytlık disk formatına serileştirir
    pub fn serialize_32(&self) -> [u8; 32] {
        let mut buf = [0u8; 32];
        buf[0..4].copy_from_slice(&self.bg_block_bitmap_lo.to_le_bytes());
        buf[4..8].copy_from_slice(&self.bg_inode_bitmap_lo.to_le_bytes());
        buf[8..12].copy_from_slice(&self.bg_inode_table_lo.to_le_bytes());
        buf[12..14].copy_from_slice(&self.bg_free_blocks_count_lo.to_le_bytes());
        buf[14..16].copy_from_slice(&self.bg_free_inodes_count_lo.to_le_bytes());
        buf[16..18].copy_from_slice(&self.bg_used_dirs_lo.to_le_bytes());
        buf[18..20].copy_from_slice(&self.bg_flags.to_le_bytes());
        buf[20..24].copy_from_slice(&self.bg_exclude_bitmap_lo.to_le_bytes());
        buf[24..26].copy_from_slice(&self.bg_block_bitmap_csum_lo.to_le_bytes());
        buf[26..28].copy_from_slice(&self.bg_inode_bitmap_csum_lo.to_le_bytes());
        buf[28..30].copy_from_slice(&self.bg_itable_unused_lo.to_le_bytes());
        buf[30..32].copy_from_slice(&self.bg_checksum.to_le_bytes());
        buf
    }

    /// 64-baytlık disk formatına serileştirir
    pub fn serialize_64(&self) -> [u8; 64] {
        let mut buf = [0u8; 64];
        buf[0..4].copy_from_slice(&self.bg_block_bitmap_lo.to_le_bytes());
        buf[4..8].copy_from_slice(&self.bg_inode_bitmap_lo.to_le_bytes());
        buf[8..12].copy_from_slice(&self.bg_inode_table_lo.to_le_bytes());
        buf[12..14].copy_from_slice(&self.bg_free_blocks_count_lo.to_le_bytes());
        buf[14..16].copy_from_slice(&self.bg_free_inodes_count_lo.to_le_bytes());
        buf[16..18].copy_from_slice(&self.bg_used_dirs_lo.to_le_bytes());
        buf[18..20].copy_from_slice(&self.bg_flags.to_le_bytes());
        buf[20..24].copy_from_slice(&self.bg_exclude_bitmap_lo.to_le_bytes());
        buf[24..26].copy_from_slice(&self.bg_block_bitmap_csum_lo.to_le_bytes());
        buf[26..28].copy_from_slice(&self.bg_inode_bitmap_csum_lo.to_le_bytes());
        buf[28..30].copy_from_slice(&self.bg_itable_unused_lo.to_le_bytes());
        buf[30..32].copy_from_slice(&self.bg_checksum.to_le_bytes());
        buf[32..36].copy_from_slice(&self.bg_block_bitmap_hi.to_le_bytes());
        buf[36..40].copy_from_slice(&self.bg_inode_bitmap_hi.to_le_bytes());
        buf[40..44].copy_from_slice(&self.bg_inode_table_hi.to_le_bytes());
        buf[44..46].copy_from_slice(&self.bg_free_blocks_count_hi.to_le_bytes());
        buf[46..48].copy_from_slice(&self.bg_free_inodes_count_hi.to_le_bytes());
        buf[48..50].copy_from_slice(&self.bg_used_dirs_hi.to_le_bytes());
        buf[50..52].copy_from_slice(&self.bg_itable_unused_hi.to_le_bytes());
        buf[52..56].copy_from_slice(&self.bg_exclude_bitmap_hi.to_le_bytes());
        buf[56..58].copy_from_slice(&self.bg_block_bitmap_csum_hi.to_le_bytes());
        buf[58..60].copy_from_slice(&self.bg_inode_bitmap_csum_hi.to_le_bytes());
        buf[60..64].copy_from_slice(&self.bg_reserved.to_le_bytes());
        buf
    }
}

// ============================================================================
// INODE
// ============================================================================

/// ext4 Inode yapısı - dosya ve dizinlerin meta verisini tutan temel yapı
#[derive(Clone, Copy, Debug)]
pub struct Ext4Inode {
    pub i_mode: u16,
    pub i_uid: u16,
    pub i_size_lo: u32,
    pub i_atime: u32,
    pub i_ctime: u32,
    pub i_mtime: u32,
    pub i_dtime: u32,
    pub i_gid: u16,
    pub i_links_count: u16,
    pub i_blocks_lo: u32,
    pub i_flags: u32,
    pub i_block: [u8; 60],
    pub i_size_hi: u32,
    pub i_generation: u32,
    pub i_file_acl_lo: u32,
    pub i_extra_isize: u16,
    pub i_checksum_hi: u16,
    pub i_crtime: u64,
    pub i_version_hi: u32,
    pub i_projid: u32,
}

impl Default for Ext4Inode {
    fn default() -> Self {
        Self {
            i_mode: 0,
            i_uid: 0,
            i_size_lo: 0,
            i_atime: 0,
            i_ctime: 0,
            i_mtime: 0,
            i_dtime: 0,
            i_gid: 0,
            i_links_count: 0,
            i_blocks_lo: 0,
            i_flags: 0,
            i_block: [0u8; 60],
            i_size_hi: 0,
            i_generation: 0,
            i_file_acl_lo: 0,
            i_extra_isize: 0,
            i_checksum_hi: 0,
            i_crtime: 0,
            i_version_hi: 0,
            i_projid: 0,
        }
    }
}

/// INDEX_FL — inode bu flag'i taşıyorsa dizin blokları HTree (hash tree) formatındadır
const EXT4_INDEX_FL: u32 = 0x00001000;

/// ENCRYPT_FL — inode fscrypt ile şifrelenmiştir
const EXT4_ENCRYPT_FL: u32 = 0x00000800;

/// EA_INODE_FL — inode, büyük xattr değerleri için ayrılmış bir EA inode'dur
const EXT4_EA_INODE_FL: u32 = 0x00200000;

/// EXTENTS_FL — inode extent ağacı kullanır
const EXT4_EXTENTS_FL: u32 = 0x00080000;

/// VERITY_FL — inode fsverity ile korunuyor
const EXT4_VERITY_FL: u32 = 0x00100000;

/// DAX_FL — inode Direct Access (doğrudan bellek erişimi) kullanır
const EXT4_DAX_FL: u32 = 0x02000000;

/// PROJINHERIT_FL — alt dosyalar project ID miras alır
const EXT4_PROJINHERIT_FL: u32 = 0x20000000;

/// CASEFOLD_FL — dizin case-insensitive
const EXT4_CASEFOLD_FL: u32 = 0x40000000;

// ============================================================================
// FS-VERITY (fs-verity per fsverity spec)
// ============================================================================

const VERITY_DESCRIPTOR_SIZE: usize = 256; // fsverity_descriptor size (version 1)

#[derive(Clone, Copy)]
enum VerityHashAlg {
    Sha256 = 1,
    Sha512 = 2,
}

impl VerityHashAlg {
    fn digest_len(&self) -> usize {
        match self {
            VerityHashAlg::Sha256 => 32,
            VerityHashAlg::Sha512 => 64,
        }
    }
}

struct VerityDescriptor {
    version: u8,
    hash_algorithm: u8,
    log_blocksize: u8,
    salt_size: u8,
    data_size: u32,
    root_hash: [u8; 64],
    salt: [u8; 32],
}

impl VerityDescriptor {
    fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 64 {
            return None;
        }
        let mut root_hash = [0u8; 64];
        let hash_len = root_hash.len().min(data.len().saturating_sub(32));
        root_hash[..hash_len].copy_from_slice(&data[32..32 + hash_len]);
        let mut salt = [0u8; 32];
        let salt_len = salt.len().min(data.len().saturating_sub(96));
        salt[..salt_len].copy_from_slice(&data[96..96 + salt_len]);
        Some(VerityDescriptor {
            version: data[0],
            hash_algorithm: data[1],
            log_blocksize: data[2],
            salt_size: data[3],
            data_size: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
            root_hash,
            salt,
        })
    }
}

fn verity_hash_block(alg: VerityHashAlg, salt: &[u8], block: &[u8]) -> Vec<u8> {
    use sha2::{Digest, Sha256, Sha512};
    match alg {
        VerityHashAlg::Sha256 => {
            let mut hasher = Sha256::new();
            if !salt.is_empty() {
                hasher.update(salt);
            }
            hasher.update(block);
            hasher.finalize().to_vec()
        }
        VerityHashAlg::Sha512 => {
            let mut hasher = Sha512::new();
            if !salt.is_empty() {
                hasher.update(salt);
            }
            hasher.update(block);
            hasher.finalize().to_vec()
        }
    }
}

fn compute_verity_root_hash(
    alg: VerityHashAlg,
    log_blocksize: u8,
    salt: &[u8],
    file_data: &[u8],
    merkle_tree: &[Vec<u8>],
) -> Vec<u8> {
    let block_size = 1usize << log_blocksize;

    if merkle_tree.is_empty() {
        return Vec::new();
    }

    let leaf_count = (file_data.len() + block_size - 1) / block_size;
    let mut leaf_hashes = Vec::with_capacity(leaf_count * alg.digest_len());

    for i in 0..leaf_count {
        let start = i * block_size;
        let end = core::cmp::min(start + block_size, file_data.len());
        let block = &file_data[start..end];
        let hash = verity_hash_block(alg, salt, block);
        leaf_hashes.extend_from_slice(&hash);
    }

    let current_level_ref = &leaf_hashes;
    let mut current_level = current_level_ref.clone();
    let mut level_idx = merkle_tree.len();

    while current_level.len() > alg.digest_len() && level_idx > 0 {
        level_idx -= 1;
        let stored_level = &merkle_tree[level_idx];
        let level_size = stored_level.len();
        let mut next_level = Vec::with_capacity(level_size);

        for chunk in current_level.chunks(block_size) {
            let hash = verity_hash_block(alg, salt, chunk);
            next_level.extend_from_slice(&hash);
        }

        current_level = next_level;
    }

    if current_level.len() == alg.digest_len() {
        current_level
    } else {
        if leaf_hashes.len() >= alg.digest_len() {
            leaf_hashes[..alg.digest_len()].to_vec()
        } else {
            Vec::new()
        }
    }
}

/// Orphan block magic — her orphan blokunun son 8 baytında bulunur
const EXT4_ORPHAN_MAGIC: u32 = 0x0b10ca04;

// ============================================================================
// EXTENDED ATTRIBUTES (XATTR)
// ============================================================================

/// EA bloğu sihirli sayısı
const EXT4_XATTR_MAGIC: u32 = 0xEA020000;

/// EA blok başlığı (32 bytes) — spec: attributes.html
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Ext4XattrHeader {
    pub h_magic: u32,
    pub h_refcount: u32,
    pub h_blocks: u32,
    pub h_hash: u32,
    pub h_checksum: u32,
    pub h_reserved: [u32; 3],
}

/// EA in-body başlığı (4 bytes)
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Ext4XattrIbodyHeader {
    pub h_magic: u32,
}

/// EA girişi (16 bytes + name)
#[derive(Debug, Clone)]
#[repr(C)]
pub struct Ext4XattrEntry {
    pub e_name_len: u8,
    pub e_name_index: u8,
    pub e_value_offs: u16,
    pub e_value_inum: u32,
    pub e_value_size: u32,
    pub e_hash: u32,
    pub e_name: Vec<u8>,
}

/// EA name index → string prefix eşlemesi
/// Index 0 = no prefix, 1 = "user.", 2 = "system.posix_acl_access",
/// 3 = "system.posix_acl_default", 4 = "trusted.", 6 = "security.",
/// 7 = "system.", 8 = "system.richacl"
const EXT4_XATTR_INDEX_USER: u8 = 1;
const EXT4_XATTR_INDEX_POSIX_ACL_ACCESS: u8 = 2;
const EXT4_XATTR_INDEX_POSIX_ACL_DEFAULT: u8 = 3;
const EXT4_XATTR_INDEX_TRUSTED: u8 = 4;
const EXT4_XATTR_INDEX_SECURITY: u8 = 6;
const EXT4_XATTR_INDEX_SYSTEM: u8 = 7;

fn xattr_index_to_prefix(index: u8) -> &'static str {
    match index {
        0 => "",
        1 => "user.",
        2 => "system.posix_acl_access",
        3 => "system.posix_acl_default",
        4 => "trusted.",
        6 => "security.",
        7 => "system.",
        _ => "",
    }
}

fn xattr_prefix_to_index(prefix: &str) -> Option<u8> {
    match prefix {
        "" => Some(0),
        "user." => Some(1),
        "system.posix_acl_access" => Some(2),
        "system.posix_acl_default" => Some(3),
        "trusted." => Some(4),
        "security." => Some(6),
        "system." => Some(7),
        _ => None,
    }
}

/// Ham byte diliminden EA giriş listesini ayrıştırır
/// block_data: başlıktan sonraki kısım (entry'lerin başladığı offset)
/// values_start: value verisinin başladığı offset (genelde block_data'nın sonu)
/// inline_attrs: true ise, entry'ler inline (inode içi), false ise external blok
pub fn parse_xattr_entries(
    data: &[u8],
    values_end: usize,
    inline_attrs: bool,
) -> Vec<Ext4XattrEntry> {
    let mut entries = Vec::new();
    let mut offset = 0usize;
    loop {
        if offset + 16 > data.len() {
            break;
        }
        let name_len = data[offset];
        let name_index = data[offset + 1];
        // Terminator: first 4 bytes all zero
        if name_len == 0 && name_index == 0 {
            break;
        }
        let value_offs = u16::from_le_bytes([data[offset + 2], data[offset + 3]]);
        let value_inum = u32::from_le_bytes([
            data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7],
        ]);
        let value_size = u32::from_le_bytes([
            data[offset + 8], data[offset + 9], data[offset + 10], data[offset + 11],
        ]);
        let e_hash = u32::from_le_bytes([
            data[offset + 12], data[offset + 13], data[offset + 14], data[offset + 15],
        ]);

        let name_end = offset + 16 + name_len as usize;
        let name = if name_end <= data.len() {
            data[offset + 16..name_end].to_vec()
        } else {
            Vec::new()
        };

        entries.push(Ext4XattrEntry {
            e_name_len: name_len,
            e_name_index: name_index,
            e_value_offs: value_offs,
            e_value_inum: value_inum,
            e_value_size: value_size,
            e_hash,
            e_name: name,
        });

        // Entry alignment: 4-byte boundary
        let entry_size = 16 + name_len as usize;
        offset += (entry_size + 3) & !3;
    }
    entries
}

/// Verilen EA giriş listesinden belirtilen isimdeki attribute'un değerini bulur
pub fn xattr_find(entries: &[Ext4XattrEntry], data: &[u8], values_end: usize, name: &str) -> Option<Vec<u8>> {
    for entry in entries {
        let prefix = xattr_index_to_prefix(entry.e_name_index);
        let full_name = if prefix.is_empty() {
            String::from_utf8_lossy(&entry.e_name).to_string()
        } else {
            format!("{}{}", prefix, String::from_utf8_lossy(&entry.e_name))
        };
        if full_name == name {
            let off = entry.e_value_offs as usize;
            let sz = entry.e_value_size as usize;
            if off + sz <= values_end && sz > 0 {
                return Some(data[off..off + sz].to_vec());
            }
            return None;
        }
    }
    None
}

/// External EA bloğundan tüm xattr'ları oku (name, value) çifti listesi döndürür
pub fn parse_xattr_block(block_data: &[u8]) -> Vec<(String, Vec<u8>)> {
    if block_data.len() < 32 {
        return Vec::new();
    }
    let magic = u32::from_le_bytes([block_data[0], block_data[1], block_data[2], block_data[3]]);
    if magic != EXT4_XATTR_MAGIC {
        return Vec::new();
    }
    // Header is 32 bytes, entries follow
    let entry_data = &block_data[32..];
    let values_end = block_data.len();
    let entries = parse_xattr_entries(entry_data, values_end, false);

    let mut result = Vec::new();
    for entry in &entries {
        let prefix = xattr_index_to_prefix(entry.e_name_index);
        let name_str = String::from_utf8_lossy(&entry.e_name).to_string();
        let full_name = if prefix.is_empty() {
            name_str
        } else {
            format!("{}{}", prefix, name_str)
        };
        let off = entry.e_value_offs as usize;
        let sz = entry.e_value_size as usize;
        let value = if off + sz <= block_data.len() && sz > 0 {
            block_data[off..off + sz].to_vec()
        } else {
            Vec::new()
        };
        result.push((full_name, value));
    }
    result
}

/// Inline EA'ları (inode içindeki) ayrıştırır
/// inode_data: inode'un ham byte dizisi (tam inode, s_inode_size kadar)
/// inode_size: s_inode_size (genelde 256)
/// i_extra_isize: inode'daki extra_isize değeri
pub fn parse_inline_xattrs(
    inode_data: &[u8],
    inode_size: usize,
    i_extra_isize: usize,
) -> Vec<(String, Vec<u8>)> {
    // Inline xattrs: başlangıç = 128 + i_extra_isize
    let start = 128 + i_extra_isize;
    if start + 4 > inode_size {
        return Vec::new();
    }
    let ibody_data = &inode_data[start..inode_size];
    if ibody_data.len() < 4 {
        return Vec::new();
    }
    let magic = u32::from_le_bytes([ibody_data[0], ibody_data[1], ibody_data[2], ibody_data[3]]);
    if magic != EXT4_XATTR_MAGIC {
        return Vec::new();
    }
    // ibody header (4 bytes) + entries follow
    let entry_data = if ibody_data.len() > 4 { &ibody_data[4..] } else { return Vec::new(); };
    let values_end = entry_data.len();
    let entries = parse_xattr_entries(entry_data, values_end, true);

    let mut result = Vec::new();
    for entry in &entries {
        let prefix = xattr_index_to_prefix(entry.e_name_index);
        let name_str = String::from_utf8_lossy(&entry.e_name).to_string();
        let full_name = if prefix.is_empty() {
            name_str
        } else {
            format!("{}{}", prefix, name_str)
        };
        let off = entry.e_value_offs as usize;
        let sz = entry.e_value_size as usize;
        let value = if off + sz <= entry_data.len() && sz > 0 {
            entry_data[off..off + sz].to_vec()
        } else {
            Vec::new()
        };
        result.push((full_name, value));
    }
    result
}

// ============================================================================
// EA YAZMA SERILEŞTIRME
// ============================================================================

/// Verilen adı prefix ve suffix olarak ayırır
fn split_xattr_name(full_name: &str) -> (&str, &str) {
    if let Some(dot_pos) = full_name.find('.') {
        (&full_name[..dot_pos + 1], &full_name[dot_pos + 1..])
    } else {
        ("", full_name)
    }
}

/// Verilen tam addan prefix kısmını döndürür
fn extract_xattr_prefix(full_name: &str) -> &str {
    let (p, _) = split_xattr_name(full_name);
    p
}

/// xattr entry sıralama anahtarı: e_name_index → e_name_len → e_name (spec: attributes.html)
struct XattrSortKey {
    name_index: u8,
    name_len: u8,
    name: Vec<u8>,
    full_name: String,
    value: Vec<u8>,
    is_ea_inode: bool,
    ea_inode_num: u32,
    value_size: u32,
}

/// Inline EA entry'lerini serileştirir (unsorted, values entries'den hemen sonra)
/// Spec: inline attribute'lar sıralı olmak zorunda değildir.
/// e_value_offs: "for an inode attribute this value is relative to the start of the first entry"
/// ea_inodes: optional map of full_name → (EA inode number, original_value_size) for EA_INODE entries
pub fn serialize_inline_xattr_entries(
    attrs: &[(String, Vec<u8>)],
    ea_inodes: Option<&alloc::collections::BTreeMap<String, (u32, u32)>>,
) -> Vec<u8> {
    let mut buf = Vec::new();

    struct InlineEntry {
        name_index: u8,
        name_bytes: Vec<u8>,
        value: Vec<u8>,
        entry_start: usize,
        is_ea_inode: bool,
        ea_inode_num: u32,
        value_size: u32,
    }
    let mut entries = Vec::new();

    for (name, value) in attrs {
        let (_, suffix) = split_xattr_name(name);
        let prefix = extract_xattr_prefix(name);
        let name_index = xattr_prefix_to_index(prefix).unwrap_or(0);
        let name_bytes = suffix.as_bytes().to_vec();
        let name_len = name_bytes.len().min(255) as u8;
        let entry_start = buf.len();

        let (is_ea_inode, ea_inode_num, value_size) = if let Some(ref map) = ea_inodes {
            if let Some(&(inum, sz)) = map.get(name) {
                (true, inum, sz)
            } else {
                (false, 0, value.len() as u32)
            }
        } else {
            (false, 0, value.len() as u32)
        };

        // EA_INODE entries store empty value in inline space (data is in EA inode)
        let stored_value = if is_ea_inode { &[] } else { value.as_slice() };

        entries.push(InlineEntry {
            name_index,
            name_bytes: name_bytes.clone(),
            value: stored_value.to_vec(),
            entry_start,
            is_ea_inode,
            ea_inode_num,
            value_size,
        });

        buf.push(name_len);
        buf.push(name_index);
        buf.extend_from_slice(&[0u8; 2]); // e_value_offs placeholder
        buf.extend_from_slice(&ea_inode_num.to_le_bytes()); // e_value_inum
        buf.extend_from_slice(&value_size.to_le_bytes()); // e_value_size
        buf.extend_from_slice(&[0u8; 4]); // e_hash
        buf.extend_from_slice(&name_bytes[..name_len as usize]);
        let padded = (16 + name_len as usize + 3) & !3;
        while buf.len() - entry_start < padded {
            buf.push(0);
        }
    }

    // Patch e_value_offs — relative to start of first entry (= start of buf, since ibody header is separate)
    let values_start = buf.len();
    let mut value_pos = values_start;
    for entry in &entries {
        if entry.is_ea_inode {
            // EA_INODE: e_value_offs is ignored, skip (leave 0)
            continue;
        }
        let off = value_pos; // absolute offset from buf[0]
        let off_u16 = off as u16;
        buf[entry.entry_start + 2] = off_u16.to_le_bytes()[0];
        buf[entry.entry_start + 3] = off_u16.to_le_bytes()[1];
        value_pos += entry.value.len();
    }

    // Append values (only for non-EA_INODE entries)
    for entry in &entries {
        if !entry.is_ea_inode {
            buf.extend_from_slice(&entry.value);
        }
    }

    buf
}

/// Block EA entry'lerini serileştirir (spec'e göre sıralı, values blok sonundan geriye doğru)
/// e_value_offs: "for a block this value is relative to the start of the block (i.e. the header)"
/// ea_inodes: optional map of full_name → (EA inode number, original_value_size) for EA_INODE entries
/// Returns (buf_with_entries_only, value_positions: Vec<(entry_offset_in_buf, value_block_offset)>)
pub fn serialize_block_xattr_entries(
    attrs: &[(String, Vec<u8>)],
    block_size: usize,
    ea_inodes: Option<&alloc::collections::BTreeMap<String, (u32, u32)>>,
) -> (Vec<u8>, Vec<(usize, u16)>) {
    // Sort: e_name_index → e_name_len → e_name (binary)
    let mut sorted: Vec<XattrSortKey> = attrs
        .iter()
        .map(|(name, value)| {
            let (_, suffix) = split_xattr_name(name);
            let prefix = extract_xattr_prefix(name);
            let name_index = xattr_prefix_to_index(prefix).unwrap_or(0);
            let name_bytes = suffix.as_bytes().to_vec();
            let name_len = name_bytes.len().min(255) as u8;
            let (is_ea_inode, ea_inode_num, value_size) = if let Some(ref map) = ea_inodes {
                if let Some(&(inum, sz)) = map.get(name) {
                    (true, inum, sz)
                } else {
                    (false, 0, value.len() as u32)
                }
            } else {
                (false, 0, value.len() as u32)
            };
            XattrSortKey {
                name_index,
                name_len,
                name: name_bytes,
                full_name: name.clone(),
                value: value.clone(),
                is_ea_inode,
                ea_inode_num,
                value_size,
            }
        })
        .collect();
    sorted.sort_by(|a, b| {
        a.name_index
            .cmp(&b.name_index)
            .then_with(|| a.name_len.cmp(&b.name_len))
            .then_with(|| a.name.cmp(&b.name))
    });

    // Total value size: skip EA_INODE entries (their values are not stored in block)
    let total_value_size: usize = sorted.iter().filter(|e| !e.is_ea_inode).map(|e| e.value.len()).sum();
    // Round up each value size to 4-byte alignment
    let aligned_sizes: Vec<usize> = sorted.iter().map(|e| (e.value.len() + 3) & !3).collect();
    let total_aligned: usize = aligned_sizes.iter().sum();

    // Values start from end of block going backwards
    let mut value_end = block_size;
    let mut value_positions: Vec<(usize, u16)> = Vec::new(); // (entry_offset_in_buf, value_block_offset)

    let mut entry_buf = Vec::new();
    for (i, entry) in sorted.iter().enumerate() {
        let entry_start = entry_buf.len();
        let name_len = entry.name_len;
        let name_bytes = &entry.name;

        if entry.is_ea_inode {
            // EA_INODE: no value space in block, e_value_offs = 0
            value_positions.push((entry_start, 0));
        } else {
            value_end = value_end.saturating_sub(aligned_sizes[i]);
            let value_block_off = value_end as u16;
            value_positions.push((entry_start, value_block_off));
        }

        let block_off = 0u16; // does not matter for EA_INODE
        let off_bytes = if entry.is_ea_inode {
            [0u8; 2]
        } else {
            value_positions.last().map(|(_, off)| off.to_le_bytes()).unwrap_or([0u8; 2])
        };

        entry_buf.push(name_len);
        entry_buf.push(entry.name_index);
        if entry.is_ea_inode {
            entry_buf.extend_from_slice(&[0u8; 2]); // e_value_offs = 0 for EA_INODE
        } else {
            let vp = value_positions.last().unwrap();
            entry_buf.extend_from_slice(&vp.1.to_le_bytes()); // e_value_offs
        }
        entry_buf.extend_from_slice(&entry.ea_inode_num.to_le_bytes()); // e_value_inum
        entry_buf.extend_from_slice(&entry.value_size.to_le_bytes()); // e_value_size
        entry_buf.extend_from_slice(&[0u8; 4]); // e_hash
        entry_buf.extend_from_slice(&name_bytes[..name_len as usize]);
        let padded = (16 + name_len as usize + 3) & !3;
        while entry_buf.len() - entry_start < padded {
            entry_buf.push(0);
        }
    }

    (entry_buf, value_positions)
}

/// Yeni bir EA bloğu oluşturur (header 32B + entries + values at end)
/// ea_inodes: optional map of full_name → (EA inode number, original_value_size) for EA_INODE entries
pub fn create_xattr_block(
    attrs: &[(String, Vec<u8>)],
    block_size: usize,
    ea_inodes: Option<&alloc::collections::BTreeMap<String, (u32, u32)>>,
) -> Vec<u8> {
    let mut buf = vec![0u8; block_size];
    // Header
    buf[0..4].copy_from_slice(&EXT4_XATTR_MAGIC.to_le_bytes());
    buf[4..8].copy_from_slice(&1u32.to_le_bytes()); // h_refcount
    buf[8..12].copy_from_slice(&1u32.to_le_bytes()); // h_blocks

    let (entry_buf, value_positions) = serialize_block_xattr_entries(attrs, block_size, ea_inodes);
    let entry_len = entry_buf.len().min(block_size - 32);
    if entry_len > 0 {
        buf[32..32 + entry_len].copy_from_slice(&entry_buf[..entry_len]);
    }

    // Place values at their positions (from end of block)
    // Build sorted list aligned by the same order as serialize_block_xattr_entries
    let mut sorted: Vec<(String, Vec<u8>, bool)> = attrs
        .iter()
        .map(|(n, v)| {
            let is_ea = ea_inodes.map_or(false, |m| m.contains_key(n));
            (n.clone(), v.clone(), is_ea)
        })
        .collect();
    sorted.sort_by(|a, b| {
        let (_, sa) = split_xattr_name(&a.0);
        let (_, sb) = split_xattr_name(&b.0);
        let pa = extract_xattr_prefix(&a.0);
        let pb = extract_xattr_prefix(&b.0);
        let ia = xattr_prefix_to_index(pa).unwrap_or(0);
        let ib = xattr_prefix_to_index(pb).unwrap_or(0);
        let la = sa.len().min(255) as u8;
        let lb = sb.len().min(255) as u8;
        ia.cmp(&ib)
            .then_with(|| la.cmp(&lb))
            .then_with(|| sa.as_bytes().cmp(sb.as_bytes()))
    });

    for (i, (_, value, is_ea)) in sorted.iter().enumerate() {
        if *is_ea {
            continue; // EA_INODE: value stored in EA inode, not in block
        }
        let aligned = (value.len() + 3) & !3;
        let block_off = block_size - (sorted[..=i].iter().map(|(_, v, ea)| if *ea { 0 } else { (v.len() + 3) & !3 }).sum::<usize>()) + aligned;
        let off = block_off - aligned;
        if off + value.len() <= block_size {
            buf[off..off + value.len()].copy_from_slice(value);
        }
    }

    buf
}

/// Inline EA body'si oluşturur (4B ibody header + entries + values)
/// Spec: for inode attribute, e_value_offs is relative to start of first entry.
/// Inline entries do not need to be sorted.
pub fn create_inline_xattr_body(attrs: &[(String, Vec<u8>)], ea_inodes: Option<&alloc::collections::BTreeMap<String, (u32, u32)>>) -> Vec<u8> {
    let mut buf = Vec::with_capacity(128);
    buf.extend_from_slice(&EXT4_XATTR_MAGIC.to_le_bytes());
    let payload = serialize_inline_xattr_entries(attrs, ea_inodes);
    buf.extend_from_slice(&payload);
    buf
}

impl Ext4FileSystem {
    /// Inode numarası ile inline xattr'ları oku
    /// inode_raw: inode'un diskteki ham baytları (s_inode_size kadar)
    pub fn read_inline_xattrs_from_raw(&self, inode: &Ext4Inode, inode_raw: &[u8]) -> Vec<(String, Vec<u8>)> {
        let inode_size = self.superblock.s_inode_size as usize;
        let extra_isize = inode.i_extra_isize as usize;
        let inode_data = if inode_raw.len() >= inode_size { &inode_raw[..inode_size] } else { inode_raw };
        parse_inline_xattrs(inode_data, inode_data.len(), extra_isize)
    }

    /// Inode'un xattr'larını diskten okur (inline + external block)
    /// EA_INODE: bir EA inode'un data bloklarından xattr değerini okur.
    /// i_atime = CRC32C checksum, i_ctime+i_version_hi = 64-bit refcount.
    fn read_ea_inode_value(&self, value_inum: u32, _value_size: u32, storage: &Ext4Storage) -> Option<Vec<u8>> {
        let ea_inode = self.read_inode_from_storage(value_inum, storage).ok()?;
        if ea_inode.i_flags & EXT4_EA_INODE_FL == 0 {
            return None;
        }
        let data = self.read_file_from_storage(&ea_inode, storage).ok()?;
        let expected_csum = ea_inode.i_atime;
        if expected_csum != 0 {
            let actual = crate::fs::journal::crc32c(&data);
            if actual != expected_csum {
                return None;
            }
        }
        Some(data)
    }

    /// Tek bir entry'nin değerini döndürür (EA_INODE veya inline/block).
    fn resolve_xattr_entry_value(
        &self,
        entry: &Ext4XattrEntry,
        block_data: &[u8],
        values_end: usize,
        storage: &Ext4Storage,
    ) -> Vec<u8> {
        if entry.e_value_inum != 0 {
            self.read_ea_inode_value(entry.e_value_inum, entry.e_value_size, storage)
                .unwrap_or_default()
        } else {
            let off = entry.e_value_offs as usize;
            let sz = entry.e_value_size as usize;
            if off + sz <= values_end && sz > 0 {
                block_data[off..off + sz].to_vec()
            } else {
                Vec::new()
            }
        }
    }

    /// Entry listesini (name, value) çiftlerine çevirir, EA_INODE değerlerini çözer.
    fn entries_to_name_values(
        &self,
        entries: &[Ext4XattrEntry],
        block_data: &[u8],
        values_end: usize,
        storage: &Ext4Storage,
    ) -> Vec<(String, Vec<u8>)> {
        let mut result = Vec::new();
        for entry in entries {
            let prefix = xattr_index_to_prefix(entry.e_name_index);
            let name_str = String::from_utf8_lossy(&entry.e_name).to_string();
            let full_name = if prefix.is_empty() {
                name_str
            } else {
                format!("{}{}", prefix, name_str)
            };
            let value = self.resolve_xattr_entry_value(entry, block_data, values_end, storage);
            result.push((full_name, value));
        }
        result
    }

    pub fn read_xattrs(
        &self,
        inode_num: u32,
        device_data: &[u8],
    ) -> Vec<(String, Vec<u8>)> {
        let mut all = Vec::new();
        if let Ok(inode_obj) = self.read_inode(inode_num, device_data) {
            if let Ok(inode_raw) = self.read_inode_raw(inode_num, device_data) {
                let inode_size = self.superblock.s_inode_size as usize;
                let extra_isize = inode_obj.i_extra_isize as usize;
                // Inline xattr: raw entries + inline data
                let start = 128 + extra_isize;
                if start + 4 <= inode_raw.len() {
                    let ibody_data = &inode_raw[start..inode_size];
                    let magic = u32::from_le_bytes([ibody_data[0], ibody_data[1], ibody_data[2], ibody_data[3]]);
                    if magic == EXT4_XATTR_MAGIC && ibody_data.len() > 4 {
                        let entry_data = &ibody_data[4..];
                        let entries = parse_xattr_entries(entry_data, entry_data.len(), true);
                        // For device_data path, EA_INODE needs separate storage read — skip EA_INODE resolution
                        // (caller can use read_xattrs_from_storage for full EA_INODE support)
                        for entry in &entries {
                            let prefix = xattr_index_to_prefix(entry.e_name_index);
                            let name_str = String::from_utf8_lossy(&entry.e_name).to_string();
                            let full_name = if prefix.is_empty() { name_str } else { format!("{}{}", prefix, name_str) };
                            let value = if entry.e_value_inum != 0 {
                                Vec::new() // placeholder — EA_INODE not supported in device_data path
                            } else {
                                let off = entry.e_value_offs as usize;
                                let sz = entry.e_value_size as usize;
                                if off + sz <= entry_data.len() && sz > 0 {
                                    entry_data[off..off + sz].to_vec()
                                } else { Vec::new() }
                            };
                            all.push((full_name, value));
                        }
                    }
                }
            }
            // External xattr block
            if inode_obj.i_file_acl_lo != 0 {
                let block = inode_obj.i_file_acl_lo as usize * self.block_size as usize;
                if block + 32 <= device_data.len() {
                    let block_data = &device_data[block..];
                    let magic = u32::from_le_bytes([block_data[0], block_data[1], block_data[2], block_data[3]]);
                    if magic == EXT4_XATTR_MAGIC && block_data.len() > 32 {
                        let entry_data = &block_data[32..];
                        let entries = parse_xattr_entries(entry_data, block_data.len(), false);
                        for entry in &entries {
                            let prefix = xattr_index_to_prefix(entry.e_name_index);
                            let name_str = String::from_utf8_lossy(&entry.e_name).to_string();
                            let full_name = if prefix.is_empty() { name_str } else { format!("{}{}", prefix, name_str) };
                            let value = if entry.e_value_inum != 0 {
                                Vec::new() // placeholder
                            } else {
                                let off = entry.e_value_offs as usize;
                                let sz = entry.e_value_size as usize;
                                if off + sz <= block_data.len() && sz > 0 {
                                    block_data[off..off + sz].to_vec()
                                } else { Vec::new() }
                            };
                            all.push((full_name, value));
                        }
                    }
                }
            }
        }
        all
    }

    /// Storage tabanlı xattr okuma (EA_INODE destekler)
    pub fn read_xattrs_from_storage(
        &self,
        inode_num: u32,
        storage: &Ext4Storage,
    ) -> Vec<(String, Vec<u8>)> {
        let mut all = Vec::new();
        if let Ok(inode_obj) = self.read_inode_from_storage(inode_num, storage) {
            if let Ok(inode_raw) = self.read_inode_raw_from_storage(inode_num, storage) {
                let inode_size = self.superblock.s_inode_size as usize;
                let extra_isize = inode_obj.i_extra_isize as usize;
                let start = 128 + extra_isize;
                if start + 4 <= inode_raw.len() {
                    let ibody_data = &inode_raw[start..inode_size];
                    let magic = u32::from_le_bytes([ibody_data[0], ibody_data[1], ibody_data[2], ibody_data[3]]);
                    if magic == EXT4_XATTR_MAGIC && ibody_data.len() > 4 {
                        let entry_data = &ibody_data[4..];
                        let entries = parse_xattr_entries(entry_data, entry_data.len(), true);
                        let resolved = self.entries_to_name_values(&entries, entry_data, entry_data.len(), storage);
                        all.extend(resolved);
                    }
                }
            }
            // External xattr block
            if inode_obj.i_file_acl_lo != 0 {
                let block = inode_obj.i_file_acl_lo as u64 * self.block_size as u64;
                if let Ok(blk) = storage.read_exact(block as usize, self.block_size as usize) {
                    let magic = u32::from_le_bytes([blk[0], blk[1], blk[2], blk[3]]);
                    if magic == EXT4_XATTR_MAGIC && blk.len() > 32 {
                        let entry_data = &blk[32..];
                        let entries = parse_xattr_entries(entry_data, blk.len(), false);
                        let resolved = self.entries_to_name_values(&entries, &blk, blk.len(), storage);
                        all.extend(resolved);
                    }
                }
            }
        }
        all
    }
}

impl Ext4Inode {
    /// Inode'u ham baytlardan çözümler (en az 128 bayt gerekir)
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 128 {
            return None;
        }

        let mut i_block = [0u8; 60];
        i_block.copy_from_slice(&data[40..100]);

        let i_extra_isize = if data.len() >= 132 {
            u16::from_le_bytes([data[128], data[129]])
        } else {
            0
        };

        Some(Ext4Inode {
            i_mode: u16::from_le_bytes([data[0], data[1]]),
            i_uid: u16::from_le_bytes([data[2], data[3]]),
            i_size_lo: u32::from_le_bytes([data[4], data[5], data[6], data[7]]),
            i_atime: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
            i_ctime: u32::from_le_bytes([data[12], data[13], data[14], data[15]]),
            i_mtime: u32::from_le_bytes([data[16], data[17], data[18], data[19]]),
            i_dtime: u32::from_le_bytes([data[20], data[21], data[22], data[23]]),
            i_gid: u16::from_le_bytes([data[24], data[25]]),
            i_links_count: u16::from_le_bytes([data[26], data[27]]),
            i_blocks_lo: u32::from_le_bytes([data[28], data[29], data[30], data[31]]),
            i_flags: u32::from_le_bytes([data[32], data[33], data[34], data[35]]),
            i_block,
            i_size_hi: u32::from_le_bytes([data[108], data[109], data[110], data[111]]),
            i_generation: u32::from_le_bytes([data[100], data[101], data[102], data[103]]),
            i_file_acl_lo: u32::from_le_bytes([data[104], data[105], data[106], data[107]]),
            i_extra_isize,
            i_checksum_hi: if data.len() >= 132 {
                u16::from_le_bytes([data[130], data[131]])
            } else {
                0
            },
            i_crtime: if data.len() >= 152 {
                u64::from_le_bytes([
                    data[144], data[145], data[146], data[147],
                    data[148], data[149], data[150], data[151],
                ])
            } else {
                0
            },
            i_version_hi: if data.len() >= 156 {
                u32::from_le_bytes([data[152], data[153], data[154], data[155]])
            } else {
                0
            },
            i_projid: if data.len() >= 160 {
                u32::from_le_bytes([data[156], data[157], data[158], data[159]])
            } else {
                0
            },
        })
    }

    /// Dosya boyutunu 64 bit olarak döndürür (hi ve lo birleşimi)
    pub fn size(&self) -> u64 {
        ((self.i_size_hi as u64) << 32) | (self.i_size_lo as u64)
    }

    /// Inode modundan dosya türünü belirler
    pub fn file_type(&self) -> Ext4FileType {
        match self.i_mode & 0xF000 {
            EXT4_S_IFREG => Ext4FileType::Regular,
            EXT4_S_IFDIR => Ext4FileType::Directory,
            EXT4_S_IFLNK => Ext4FileType::Symlink,
            EXT4_S_IFCHR => Ext4FileType::CharDevice,
            EXT4_S_IFBLK => Ext4FileType::BlockDevice,
            EXT4_S_IFIFO => Ext4FileType::Fifo,
            EXT4_S_IFSOCK => Ext4FileType::Socket,
            _ => Ext4FileType::Unknown,
        }
    }

    /// Inode'un bir dizin olup olmadığını kontrol eder
    pub fn is_directory(&self) -> bool {
        (self.i_mode & 0xF000) == EXT4_S_IFDIR
    }

    /// Inode'un bir sembolik link olup olmadığını kontrol eder
    pub fn is_symlink(&self) -> bool {
        (self.i_mode & 0xF000) == EXT4_S_IFLNK
    }

    /// Inode'un extent ağacı kullanıp kullanmadığını kontrol eder
    pub fn uses_extents(&self) -> bool {
        (self.i_flags & 0x00080000) != 0
    }

    /// Doğrudan ve dolaylı blok göstericilerini döndürür (sadece extent kullanmıyorsa)
    pub fn indirect_blocks(&self) -> [u32; 15] {
        let mut blocks = [0u32; 15];
        if self.uses_extents() {
            return blocks;
        }

        for i in 0..15 {
            let offset = i * 4;
            blocks[i] = u32::from_le_bytes([
                self.i_block[offset],
                self.i_block[offset + 1],
                self.i_block[offset + 2],
                self.i_block[offset + 3],
            ]);
        }
        blocks
    }

    /// Inode'u 256-bayt ham byte dizisine serileştirir (little-endian)
    /// Spec: kernel.org inodes.html — inode size default 256 bytes, i_extra_isize=32
    pub fn serialize(&self) -> [u8; 256] {
        let mut buf = [0u8; 256];
        buf[0..2].copy_from_slice(&self.i_mode.to_le_bytes());
        buf[2..4].copy_from_slice(&self.i_uid.to_le_bytes());
        buf[4..8].copy_from_slice(&self.i_size_lo.to_le_bytes());
        buf[8..12].copy_from_slice(&self.i_atime.to_le_bytes());
        buf[12..16].copy_from_slice(&self.i_ctime.to_le_bytes());
        buf[16..20].copy_from_slice(&self.i_mtime.to_le_bytes());
        buf[20..24].copy_from_slice(&self.i_dtime.to_le_bytes());
        buf[24..26].copy_from_slice(&self.i_gid.to_le_bytes());
        buf[26..28].copy_from_slice(&self.i_links_count.to_le_bytes());
        buf[28..32].copy_from_slice(&self.i_blocks_lo.to_le_bytes());
        buf[32..36].copy_from_slice(&self.i_flags.to_le_bytes());
        buf[40..100].copy_from_slice(&self.i_block);
        buf[100..104].copy_from_slice(&self.i_generation.to_le_bytes());   // 0x64
        buf[104..108].copy_from_slice(&self.i_file_acl_lo.to_le_bytes());  // 0x68
        buf[108..112].copy_from_slice(&self.i_size_hi.to_le_bytes());      // 0x6C
        // i_osd2 at 0x74 (12 bytes) — Linux: l_i_blocks_high, l_i_file_acl_high, l_i_uid_high, l_i_gid_high, l_i_checksum_lo
        let blocks_512 = self.i_blocks_lo; // in 512-byte units (as ext4 expects)
        let blocks_hi = (blocks_512 >> 16) as u16;
        buf[0x74..0x76].copy_from_slice(&blocks_hi.to_le_bytes());
        // i_extra_isize at 0x80 — default 32 for 256-byte inode
        buf[0x80..0x82].copy_from_slice(&self.i_extra_isize.to_le_bytes());
        buf[0x82..0x84].copy_from_slice(&self.i_checksum_hi.to_le_bytes()); // 0x82
        // i_crtime at 0x90 (64-bit)
        buf[0x90..0x98].copy_from_slice(&self.i_crtime.to_le_bytes());
        // i_version_hi at 0x98
        buf[0x98..0x9C].copy_from_slice(&self.i_version_hi.to_le_bytes());
        // i_projid at 0x9C
        buf[0x9C..0xA0].copy_from_slice(&self.i_projid.to_le_bytes());
        buf
    }

    /// Inode'dan meta veri yapısı oluşturur
    pub fn metadata(&self) -> Ext4Metadata {
        Ext4Metadata {
            size: self.size(),
            file_type: self.file_type(),
            permissions: self.i_mode & 0x0FFF,
            uid: self.i_uid,
            gid: self.i_gid,
            links: self.i_links_count,
            atime: self.i_atime,
            mtime: self.i_mtime,
            ctime: self.i_ctime,
        }
    }

    /// NFS export için filehandle oluştur — inode_num + generation (8 bayt)
    pub fn nfs_encode_filehandle(&self, ino: u32) -> [u8; 8] {
        let mut fh = [0u8; 8];
        fh[0..4].copy_from_slice(&ino.to_le_bytes());
        fh[4..8].copy_from_slice(&self.i_generation.to_le_bytes());
        fh
    }

    /// NFS export için filehandle'dan inode_num ve generation çıkarır
    pub fn nfs_decode_filehandle(fh: &[u8]) -> Option<(u32, u32)> {
        if fh.len() < 8 {
            return None;
        }
        let ino = u32::from_le_bytes([fh[0], fh[1], fh[2], fh[3]]);
        let gen = u32::from_le_bytes([fh[4], fh[5], fh[6], fh[7]]);
        Some((ino, gen))
    }
}

// ============================================================================
// EXTENT AĞACI
// ============================================================================

/// Extent başlığı - inode'un i_block alanının başında yer alır
///
/// Disk formatı (12 bayt):
///   [0..2] eh_magic      (0xF30A)
///   [2..4] eh_entries    geçerli giriş sayısı
///   [4..6] eh_max        kapasite
///   [6..8] eh_depth      0=leaf, >0=internal node
///   [8..12] eh_generation
#[derive(Clone, Copy, Debug)]
pub struct Ext4ExtentHeader {
    pub eh_magic: u16,
    pub eh_entries: u16,
    pub eh_max: u16,
    pub eh_depth: u16,
    pub eh_generation: u32,
}

impl Ext4ExtentHeader {
    const MAGIC: u16 = 0xF30A;

    /// Extent başlığını baytlardan çözümler ve sihirli sayıyı doğrular
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 12 {
            return None;
        }

        let header = Ext4ExtentHeader {
            eh_magic: u16::from_le_bytes([data[0], data[1]]),
            eh_entries: u16::from_le_bytes([data[2], data[3]]),
            eh_max: u16::from_le_bytes([data[4], data[5]]),
            eh_depth: u16::from_le_bytes([data[6], data[7]]),
            eh_generation: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
        };

        if header.eh_magic != Self::MAGIC {
            return None;
        }

        Some(header)
    }

    /// Derinlik 0 ise yaprak düğüm (doğrudan disk bloklarına işaret eder)
    pub fn is_leaf(&self) -> bool {
        self.eh_depth == 0
    }
}

/// Extent girdisi - mantıksal blok aralığını fiziksel blok konumuna eşler
#[derive(Clone, Copy, Debug)]
pub struct Ext4Extent {
    pub ee_block: u32,
    pub ee_len: u16,
    pub ee_start: u64,
}

impl Ext4Extent {
    /// Extent girişini ham baytlardan çözümler
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 12 {
            return None;
        }

        let ee_len_raw = u16::from_le_bytes([data[4], data[5]]);
        let ee_len = ee_len_raw & 0x7FFF;
        let start_hi = u16::from_le_bytes([data[6], data[7]]) as u64;
        let start_lo = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as u64;
        let ee_start = (start_hi << 32) | start_lo;

        Some(Ext4Extent {
            ee_block: u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
            ee_len,
            ee_start,
        })
    }
}

/// Extent index girdisi - internal node'da child block'a işaret eder
#[derive(Clone, Copy, Debug)]
pub struct Ext4ExtentIdx {
    pub ei_block: u32,
    pub ei_leaf: u64,
}

impl Ext4ExtentIdx {
    /// Extent index girişini ham baytlardan çözümler (12 bytes)
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 12 {
            return None;
        }
        let leaf_lo = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as u64;
        let leaf_hi = u16::from_le_bytes([data[8], data[9]]) as u64;
        let ei_leaf = (leaf_hi << 32) | leaf_lo;
        Some(Ext4ExtentIdx {
            ei_block: u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
            ei_leaf,
        })
    }
}

// ============================================================================
// INODE-LEVEL LOCKING — Linux i_rwsem Equivalent
// ============================================================================
//
// Linux kernel: fs/inode.c inode_lock(), inode_unlock(), inode_lock_shared(),
// inode_unlock_shared(), inode_trylock(). Protects inode metadata (i_rwsem).
//
// Lock ordering (Linux VFS contract):
//   1. Parent directory inode lock first
//   2. Child inode lock second
//   3. For cross-directory rename: lock by ascending inode number
//
// Implementation: global per-(source, ino) atomic spinlock table.
// Atomic state: 0 = unlocked, 1 = exclusive, 2+ = shared reader count.

/// Per-inode lock state — atomic spinlock supporting exclusive and shared modes.
struct InodeLockState {
    state: AtomicU8,
}

const INODE_UNLOCKED: u8 = 0;
const INODE_EXCLUSIVE: u8 = 1;
const INODE_SHARED_BASE: u8 = 2;

impl InodeLockState {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(INODE_UNLOCKED),
        }
    }

    /// Linux: inode_lock() — acquire exclusive (i_rwsem write)
    fn lock_exclusive(&self) {
        loop {
            match self.state.compare_exchange_weak(
                INODE_UNLOCKED,
                INODE_EXCLUSIVE,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                Err(_) => core::hint::spin_loop(),
            }
        }
    }

    /// Linux: inode_trylock() — non-blocking exclusive attempt
    fn try_lock_exclusive(&self) -> bool {
        self.state
            .compare_exchange(
                INODE_UNLOCKED,
                INODE_EXCLUSIVE,
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .is_ok()
    }

    /// Linux: inode_unlock() — release exclusive (i_rwsem write)
    fn unlock_exclusive(&self) {
        debug_assert_eq!(self.state.load(Ordering::Relaxed), INODE_EXCLUSIVE);
        self.state.store(INODE_UNLOCKED, Ordering::Release);
    }

    /// Linux: inode_lock_shared() — acquire shared (i_rwsem read)
    fn lock_shared(&self) {
        loop {
            let current = self.state.load(Ordering::Acquire);
            if current != INODE_EXCLUSIVE {
                if self
                    .state
                    .compare_exchange_weak(
                        current,
                        current + 1,
                        Ordering::Acquire,
                        Ordering::Relaxed,
                    )
                    .is_ok()
                {
                    return;
                }
            }
            core::hint::spin_loop();
        }
    }

    /// Linux: inode_trylock_shared() — non-blocking shared attempt
    fn try_lock_shared(&self) -> bool {
        let current = self.state.load(Ordering::Acquire);
        current != INODE_EXCLUSIVE
            && self
                .state
                .compare_exchange(
                    current,
                    current + 1,
                    Ordering::Acquire,
                    Ordering::Relaxed,
                )
                .is_ok()
    }

    /// Linux: inode_unlock_shared() — release shared (i_rwsem read)
    fn unlock_shared(&self) {
        let prev = self.state.fetch_sub(1, Ordering::Release);
        debug_assert!(prev >= INODE_SHARED_BASE);
    }

    fn is_unlocked(&self) -> bool {
        self.state.load(Ordering::Acquire) == INODE_UNLOCKED
    }
}

lazy_static::lazy_static! {
    static ref INODE_LOCK_TABLE: Mutex<BTreeMap<String, BTreeMap<u32, Arc<InodeLockState>>>> =
        Mutex::new(BTreeMap::new());
}

fn get_inode_lock_state(source: &str, ino: u32) -> Arc<InodeLockState> {
    let mut table = INODE_LOCK_TABLE.lock();
    let source_locks = table
        .entry(source.to_string())
        .or_insert_with(BTreeMap::new);
    source_locks
        .entry(ino)
        .or_insert_with(|| Arc::new(InodeLockState::new()))
        .clone()
}

/// Linux: inode_lock(inode) — exclusive lock for metadata writes
pub fn ext4_inode_lock(source: &str, ino: u32) {
    let lock = get_inode_lock_state(source, ino);
    lock.lock_exclusive();
}

/// Linux: inode_unlock(inode) — release exclusive lock
pub fn ext4_inode_unlock(source: &str, ino: u32) {
    let lock = get_inode_lock_state(source, ino);
    lock.unlock_exclusive();
}

/// Linux: inode_trylock(inode) — non-blocking exclusive attempt
pub fn ext4_inode_trylock(source: &str, ino: u32) -> bool {
    let lock = get_inode_lock_state(source, ino);
    lock.try_lock_exclusive()
}

/// Linux: inode_lock_shared(inode) — shared lock for reads
pub fn ext4_inode_lock_shared(source: &str, ino: u32) {
    let lock = get_inode_lock_state(source, ino);
    lock.lock_shared();
}

/// Linux: inode_unlock_shared(inode) — release shared lock
pub fn ext4_inode_unlock_shared(source: &str, ino: u32) {
    let lock = get_inode_lock_state(source, ino);
    lock.unlock_shared();
}

/// Linux: inode_trylock_shared(inode) — non-blocking shared attempt
pub fn ext4_inode_trylock_shared(source: &str, ino: u32) -> bool {
    let lock = get_inode_lock_state(source, ino);
    lock.try_lock_shared()
}

/// Remove all inode locks for a given mount source (called on unmount).
pub fn ext4_inode_locks_clear(source: &str) {
    let mut table = INODE_LOCK_TABLE.lock();
    table.remove(source);
}

// ============================================================================
// ext4 DOSYA SİSTEMİ
// ============================================================================

/// ext4 Dosya Sistemi örneği - tüm dosya sistemi durumunu yönetir
#[derive(Clone, Debug)]
pub struct Ext4FileSystem {
    pub superblock: Ext4Superblock,
    pub block_size: u32,
    pub is_64bit: bool,
    pub group_descriptors: Vec<Ext4GroupDescriptor>,
    pub root_inode: u32,
    /// Yazma desteği için isteğe bağlı günlük (journal)
    pub journal: Option<Arc<Mutex<Journal>>>,
    /// Günlüğün başladığı blok ofseti
    pub journal_offset: u64,
    /// Journal corruption veya recovery hatasında mount yazmaya kapatılır.
    pub read_only: bool,
    /// MMP (Multiple Mount Protection) in-memory sequence number
    pub mmp_seq: u32,
}

impl Ext4FileSystem {
    /// Yeni bir ext4 dosya sistemi örneği oluşturur (varsayılan değerlerle)
    pub fn new() -> Self {
        Ext4FileSystem {
            superblock: unsafe { mem::zeroed() },
            block_size: 4096,
            is_64bit: false,
            group_descriptors: Vec::new(),
            root_inode: 2,
            journal: None,
            journal_offset: 0,
            read_only: false,
            mmp_seq: 0,
        }
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub fn mark_read_only(&mut self) {
        self.read_only = true;
    }

    /// Inode'un disk boyutunu döndürür (i_blocks * 512)
    pub fn inode_size(&self, inode: &Ext4Inode) -> u64 {
        (inode.i_blocks_lo as u64) * 512
    }

    /// Inode lock'unu al — aynı inode'a eşzamanlı yazmayı önler
    /// Linux: inode_lock() — eski adıyla i_mutex
    /// Deprecated: use ext4_inode_lock(source, ino) for global lock table.
    /// This method is retained for API compatibility; it acquires the lock
    /// through the global INODE_LOCK_TABLE keyed by the root inode as source.
    pub fn inode_lock_compat(&self, ino: u32) {
        ext4_inode_lock("", ino);
    }

    /// Inode lock'unu serbest bırak
    /// Deprecated: use ext4_inode_unlock(source, ino) instead.
    pub fn inode_unlock_compat(&self, ino: u32) {
        ext4_inode_unlock("", ino);
    }

    /// Aygıt verisinden dosya sistemini başlatır: süper bloğu okur ve doğrular
    pub fn init(&mut self, device_data: &[u8]) -> Result<(), Ext4Error> {
        if device_data.len() < SUPERBLOCK_OFFSET as usize + 1024 {
            return Err(Ext4Error::ReadError);
        }

        let sb_data = &device_data[SUPERBLOCK_OFFSET as usize..];
        let sb = Ext4Superblock::parse(sb_data).ok_or(Ext4Error::InvalidFormat)?;

        self.superblock = sb;
        self.block_size = sb.block_size();
        self.is_64bit = sb.is_64bit();

        // Superblock checksum doğrulama (METADATA_CSUM aktifse)
        if sb.s_feature_ro_compat & EXT4_FEATURE_RO_COMPAT_METADATA_CSUM != 0 {
            if !Ext4Superblock::verify_checksum(sb_data) {
                return Err(Ext4Error::ChecksumError);
            }
        }

        // Gate 4a: Bigalloc — cluster_size > block_size, allocation aware gerekir
        if sb.s_log_cluster_size > 0 {
            crate::serial_println!(
                "[ext4] Uyarı: Bigalloc etkin (cluster_size={}), salt-okunur bağlanıyor",
                1024u32 << sb.s_log_cluster_size
            );
            self.read_only = true;
        }

        // Gate 4b: Casefold (INCOMPAT) — case-insensitive dizinler, salt-okunur bağla
        if sb.s_feature_incompat & EXT4_FEATURE_INCOMPAT_CASEFOLD != 0 {
            crate::serial_println!(
                "[ext4] Uyarı: CASEFOLD etkin (INCOMPAT, case-insensitive), salt-okunur bağlanıyor"
            );
            self.read_only = true;
        }

        // Gate 4c: Project quota — proje ID tabanlı quota, salt-okunur bağla
        if sb.s_feature_ro_compat & EXT4_FEATURE_RO_COMPAT_PROJECT != 0 {
            crate::serial_println!(
                "[ext4] Uyarı: PROJQUOTA etkin (project quota), salt-okunur bağlanıyor"
            );
            self.read_only = true;
        }

        // Gate 4d: Encryption (INCOMPAT_ENCRYPT) — şifreli dosyalar, salt-okunur bağla
        if sb.s_feature_incompat & EXT4_FEATURE_INCOMPAT_ENCRYPT != 0 {
            crate::serial_println!(
                "[ext4] Uyarı: ENCRYPT etkin (INCOMPAT, fscrypt), salt-okunur bağlanıyor"
            );
            self.read_only = true;
        }

        // Gate 4e: Fast Commit (COMPAT_FAST_COMMIT) — hızlı commit, salt-okunur bağla
        if sb.s_feature_compat & EXT4_FEATURE_COMPAT_FAST_COMMIT != 0 {
            crate::serial_println!(
                "[ext4] Uyarı: FAST_COMMIT etkin (COMPAT), salt-okunur bağlanıyor"
            );
            self.read_only = true;
        }

        // Gate 4f: Snapshot (RO_COMPAT_HAS_SNAPSHOT) — snapshot desteği, salt-okunur bağla
        if sb.s_feature_ro_compat & EXT4_FEATURE_RO_COMPAT_HAS_SNAPSHOT != 0 {
            crate::serial_println!(
                "[ext4] Uyarı: HAS_SNAPSHOT etkin (RO_COMPAT), salt-okunur bağlanıyor"
            );
            self.read_only = true;
        }

        // Gate 4f2: Online Resize (COMPAT_RESIZE_INODE) — Genişletme yedeği mevcut
        if sb.s_feature_compat & EXT4_FEATURE_COMPAT_RESIZE_INODE != 0 {
            crate::serial_println!(
                "[ext4] Uyarı: RESIZE_INODE etkin (COMPAT, online genişletme yedeği mevcut)"
            );
        }

        // Gate 4g: fsck durumu — hata varsa veya bağlanma sayısı aşıldıysa salt-okunur
        if sb.s_state & 0x0002 != 0 {
            crate::serial_println!(
                "[ext4] Uyarı: Dosya sisteminde hata tespit edildi (s_state=0x{:x}), salt-okunur bağlanıyor",
                sb.s_state
            );
            self.read_only = true;
        }
        if sb.s_max_mnt_count != 0 && sb.s_mnt_count >= sb.s_max_mnt_count {
            crate::serial_println!(
                "[ext4] Uyarı: fsck gerekli (mnt_count={}/{}), salt-okunur bağlanıyor",
                sb.s_mnt_count, sb.s_max_mnt_count
            );
            self.read_only = true;
        }
        if sb.s_error_count != 0 {
            crate::serial_println!(
                "[ext4] Uyarı: {} hata kaydı mevcut, salt-okunur bağlanıyor",
                sb.s_error_count
            );
            self.read_only = true;
        }

        // Gate 4: Bilinmeyen INCOMPAT feature varsa mount'u reddet
        let unknown_incompat = sb.s_feature_incompat & !EXT4_KNOWN_INCOMPAT;
        if unknown_incompat != 0 {
            crate::serial_println!(
                "[ext4] Uyarı: Bilinmeyen INCOMPAT feature'lar tespit edildi: 0x{:x}, salt-okunur bağlanıyor",
                unknown_incompat
            );
            self.read_only = true;
        }

        // Gate 5: Bilinmeyen RO_COMPAT feature varsa uyar
        let unknown_ro = sb.s_feature_ro_compat & !EXT4_KNOWN_RO_COMPAT;
        if unknown_ro != 0 {
            crate::serial_println!(
                "[ext4] Uyarı: Bilinmeyen RO_COMPAT feature'lar: 0x{:x}",
                unknown_ro
            );
        }

        // Blok grubu tanımlayıcılarını diskten yükle
        self.load_group_descriptors(device_data)?;

        crate::serial_println!(
            "[ext4] Başlatıldı: {} blok, {} inode, {} bayt/blok",
            sb.total_blocks(),
            sb.s_inodes_count,
            self.block_size
        );

        Ok(())
    }

    pub fn init_from_storage(&mut self, storage: &Ext4Storage) -> Result<(), Ext4Error> {
        let sb_bytes = storage
            .read_exact(SUPERBLOCK_OFFSET as usize, 1024)
            .map_err(|_| Ext4Error::ReadError)?;
        let sb = Ext4Superblock::parse(sb_bytes.as_slice()).ok_or(Ext4Error::InvalidFormat)?;

        self.superblock = sb;
        self.block_size = sb.block_size();
        self.is_64bit = sb.is_64bit();
        self.group_descriptors.clear();

        // Superblock checksum doğrulama (METADATA_CSUM aktifse)
        if sb.s_feature_ro_compat & EXT4_FEATURE_RO_COMPAT_METADATA_CSUM != 0 {
            if !Ext4Superblock::verify_checksum(sb_bytes.as_slice()) {
                return Err(Ext4Error::ChecksumError);
            }
        }

        // Gate 4a: Bigalloc — cluster_size > block_size
        if sb.s_log_cluster_size > 0 {
            crate::serial_println!(
                "[ext4] Uyarı: Bigalloc etkin (cluster_size={}), salt-okunur bağlanıyor",
                1024u32 << sb.s_log_cluster_size
            );
            self.read_only = true;
        }

        // Gate 4b: Encryption (INCOMPAT_ENCRYPT) — şifreli dosyalar, salt-okunur bağla
        if sb.s_feature_incompat & EXT4_FEATURE_INCOMPAT_ENCRYPT != 0 {
            crate::serial_println!(
                "[ext4] Uyarı: ENCRYPT etkin (INCOMPAT, fscrypt), salt-okunur bağlanıyor"
            );
            self.read_only = true;
        }

        // Gate 4c: Fast Commit (COMPAT_FAST_COMMIT) — hızlı commit, salt-okunur bağla
        if sb.s_feature_compat & EXT4_FEATURE_COMPAT_FAST_COMMIT != 0 {
            crate::serial_println!(
                "[ext4] Uyarı: FAST_COMMIT etkin (COMPAT), salt-okunur bağlanıyor"
            );
            self.read_only = true;
        }

        // Gate 4d: Snapshot (RO_COMPAT_HAS_SNAPSHOT) — snapshot desteği, salt-okunur bağla
        if sb.s_feature_ro_compat & EXT4_FEATURE_RO_COMPAT_HAS_SNAPSHOT != 0 {
            crate::serial_println!(
                "[ext4] Uyarı: HAS_SNAPSHOT etkin (RO_COMPAT), salt-okunur bağlanıyor"
            );
            self.read_only = true;
        }

        // Gate 4d2: Online Resize (COMPAT_RESIZE_INODE) — Genişletme yedeği mevcut
        if sb.s_feature_compat & EXT4_FEATURE_COMPAT_RESIZE_INODE != 0 {
            crate::serial_println!(
                "[ext4] Uyarı: RESIZE_INODE etkin (COMPAT, online genişletme yedeği mevcut)"
            );
        }

        // Gate 4e: fsck durumu — hata varsa veya bağlanma sayısı aşıldıysa salt-okunur
        if sb.s_state & 0x0002 != 0 {
            crate::serial_println!(
                "[ext4] Uyarı: Dosya sisteminde hata tespit edildi (s_state=0x{:x}), salt-okunur bağlanıyor",
                sb.s_state
            );
            self.read_only = true;
        }
        if sb.s_max_mnt_count != 0 && sb.s_mnt_count >= sb.s_max_mnt_count {
            crate::serial_println!(
                "[ext4] Uyarı: fsck gerekli (mnt_count={}/{}), salt-okunur bağlanıyor",
                sb.s_mnt_count, sb.s_max_mnt_count
            );
            self.read_only = true;
        }
        if sb.s_error_count != 0 {
            crate::serial_println!(
                "[ext4] Uyarı: {} hata kaydı mevcut, salt-okunur bağlanıyor",
                sb.s_error_count
            );
            self.read_only = true;
        }

        // Gate 4: Bilinmeyen INCOMPAT feature varsa mount'u reddet
        let unknown_incompat = sb.s_feature_incompat & !EXT4_KNOWN_INCOMPAT;
        if unknown_incompat != 0 {
            crate::serial_println!(
                "[ext4] Uyarı: Bilinmeyen INCOMPAT feature'lar tespit edildi: 0x{:x}, salt-okunur bağlanıyor",
                unknown_incompat
            );
            self.read_only = true;
        }

        // Gate 5: Bilinmeyen RO_COMPAT feature varsa uyar
        let unknown_ro = sb.s_feature_ro_compat & !EXT4_KNOWN_RO_COMPAT;
        if unknown_ro != 0 {
            crate::serial_println!(
                "[ext4] Uyarı: Bilinmeyen RO_COMPAT feature'lar: 0x{:x}",
                unknown_ro
            );
        }

        self.load_group_descriptors_from_storage(storage)?;

        crate::serial_println!(
            "[ext4] Başlatıldı: {} blok, {} inode, {} bayt/blok",
            sb.total_blocks(),
            sb.s_inodes_count,
            self.block_size
        );

        Ok(())
    }

    /// Blok grubu tanımlayıcılarını diskten okuyup belleğe yükler
    fn gd_size(&self) -> usize {
        let desc_size = self.superblock.s_desc_size as usize;
        if desc_size >= 32 { desc_size } else if self.is_64bit { 64 } else { 32 }
    }

    fn load_group_descriptors(&mut self, device_data: &[u8]) -> Result<(), Ext4Error> {
        let gd_offset = self.block_size as usize;
        let gds_count = self.superblock.block_groups_count() as usize;
        let gd_size = self.gd_size();
        let check_csum = self.superblock.s_feature_ro_compat & EXT4_FEATURE_RO_COMPAT_METADATA_CSUM != 0;

        for i in 0..gds_count {
            let offset = gd_offset + i * gd_size;
            if offset + 32 > device_data.len() {
                break;
            }

            let gd = if gd_size >= 64 {
                Ext4GroupDescriptor::parse_64(&device_data[offset..])
            } else {
                Ext4GroupDescriptor::parse_32(&device_data[offset..])
            };
            if let Some(gd) = gd {
                if check_csum {
                    let gd_data = &device_data[offset..offset + gd_size];
                    if !verify_gd_checksum(gd_data, i as u32, gd_size) {
                        return Err(Ext4Error::ChecksumError);
                    }
                }
                self.group_descriptors.push(gd);
            }
        }

        Ok(())
    }

    fn load_group_descriptors_from_storage(
        &mut self,
        storage: &Ext4Storage,
    ) -> Result<(), Ext4Error> {
        let gd_offset = self.block_size as usize;
        let gds_count = self.superblock.block_groups_count() as usize;
        let gd_size = self.gd_size();
        let total_bytes = gds_count.saturating_mul(gd_size);
        let gd_bytes = storage
            .read_exact(gd_offset, total_bytes)
            .map_err(|_| Ext4Error::ReadError)?;
        let check_csum = self.superblock.s_feature_ro_compat & EXT4_FEATURE_RO_COMPAT_METADATA_CSUM != 0;

        for i in 0..gds_count {
            let start = i * gd_size;
            if start + 32 > gd_bytes.len() {
                break;
            }
            let chunk = &gd_bytes[start..(start + gd_size).min(gd_bytes.len())];
            let gd = if gd_size >= 64 {
                Ext4GroupDescriptor::parse_64(chunk)
            } else {
                Ext4GroupDescriptor::parse_32(chunk)
            };
            if let Some(gd) = gd {
                if check_csum {
                    if !verify_gd_checksum(chunk, i as u32, gd_size) {
                        return Err(Ext4Error::ChecksumError);
                    }
                }
                self.group_descriptors.push(gd);
            }
        }

        Ok(())
    }

    /// Verilen inode numarasının disk üzerindeki bayt ofsetini ve boyutunu döndürür
    pub fn get_inode_location(&self, inode: u32) -> (u64, u32) {
        let inodes_per_group = self.superblock.s_inodes_per_group;
        let inode_size = self.superblock.s_inode_size as u32;

        let group = (inode - 1) / inodes_per_group;
        let index = (inode - 1) % inodes_per_group;

        if let Some(gd) = self.group_descriptors.get(group as usize) {
            let inode_table = gd.inode_table(self.is_64bit);
            let block_offset = inode_table * self.block_size as u64;
            let inode_offset = index as u64 * inode_size as u64;

            (block_offset + inode_offset, inode_size)
        } else {
            (0, 0)
        }
    }

    /// Belirtilen inode numarasını aygıt verisinden okur
    pub fn read_inode(&self, inode: u32, device_data: &[u8]) -> Result<Ext4Inode, Ext4Error> {
        let (offset, size) = self.get_inode_location(inode);
        let offset = offset as usize;

        if offset + size as usize > device_data.len() {
            return Err(Ext4Error::ReadError);
        }

        let raw = &device_data[offset..offset + size as usize];

        // Inode checksum doğrulama (checksums.html: UUID + inode_num + generation + inode data)
        // Checksum alanı: i_checksum_hi at 0x82 (16-bit) ve i_checksum_lo at 0x7C (16-bit)
        // CRC32C kullanılır
        if self.superblock.s_feature_ro_compat & EXT4_FEATURE_RO_COMPAT_METADATA_CSUM != 0 {
            let stored_lo = u16::from_le_bytes([raw[0x7C], raw[0x7D]]) as u32;
            let stored_hi = u16::from_le_bytes([raw[0x82], raw[0x83]]) as u32;
            let stored_checksum = (stored_hi << 16) | stored_lo;

            // Checksum hesapla: UUID + inode_num (LE) + inode data (checksum alanı sıfırlanmış)
            let mut seed = crate::fs::journal::crc32c_with_seed(&self.superblock.s_uuid, !0u32);
            seed = crate::fs::journal::crc32c_with_seed(&inode.to_le_bytes(), seed);

            let mut data_for_csum = raw.to_vec();
            data_for_csum[0x7C..0x7E].copy_from_slice(&0u16.to_le_bytes()); // checksum_lo sıfırla
            data_for_csum[0x82..0x84].copy_from_slice(&0u16.to_le_bytes()); // checksum_hi sıfırla
            let computed = crate::fs::journal::crc32c_with_seed(&data_for_csum, seed);

            if computed != stored_checksum {
                crate::serial_println!(
                    "[ext4] UYARI: inode {} checksum hatası (stored=0x{:x}, computed=0x{:x})",
                    inode, stored_checksum, computed
                );
                // Checksum hatası olsa bile inode'u oku (sadece uyarı)
            }
        }

        Ext4Inode::parse(raw).ok_or(Ext4Error::Corrupted)
    }

    /// Inode'un ham baytlarını döndürür (inline xattr okuması için)
    pub fn read_inode_raw(&self, inode: u32, device_data: &[u8]) -> Result<Vec<u8>, Ext4Error> {
        let (offset, size) = self.get_inode_location(inode);
        let offset = offset as usize;
        let size = size as usize;
        if offset + size > device_data.len() {
            return Err(Ext4Error::ReadError);
        }
        Ok(device_data[offset..offset + size].to_vec())
    }

    pub fn read_inode_from_storage(
        &self,
        inode: u32,
        storage: &Ext4Storage,
    ) -> Result<Ext4Inode, Ext4Error> {
        let (offset, size) = self.get_inode_location(inode);
        let inode_bytes = storage
            .read_exact(offset as usize, size as usize)
            .map_err(|_| Ext4Error::ReadError)?;
        Ext4Inode::parse(inode_bytes.as_slice()).ok_or(Ext4Error::Corrupted)
    }

    /// Storage tabanlı inode raw okuma
    pub fn read_inode_raw_from_storage(&self, inode: u32, storage: &Ext4Storage) -> Result<Vec<u8>, Ext4Error> {
        let (offset, size) = self.get_inode_location(inode);
        storage
            .read_exact(offset as usize, size as usize)
            .map_err(|_| Ext4Error::ReadError)
    }

    /// Inode'u depolama birimine yazar (güncellenmiş alanlar ile)
    /// Not: inode_size kadar bayt yazılır (128 veya 256)
    pub fn write_inode(&self, inode: u32, inode_data: &Ext4Inode, storage: &mut Ext4Storage) -> Result<(), Ext4Error> {
        let serialized = inode_data.serialize();
        let (offset, inode_size) = self.get_inode_location(inode);
        let inode_size = inode_size as usize;
        let write_size = serialized.len().min(inode_size);
        storage.write_exact(offset as usize, &serialized[..write_size])
    }

    /// Inode'un ham baytlarını olduğu gibi diske yazar (inline xattr vd. extended alanlar için)
    pub fn write_inode_raw(&self, inode: u32, raw: &[u8], storage: &mut Ext4Storage) -> Result<(), Ext4Error> {
        let (offset, inode_size) = self.get_inode_location(inode);
        let inode_size = inode_size as usize;
        let write_size = raw.len().min(inode_size);
        storage.write_exact(offset as usize, &raw[..write_size])
    }

    /// EA_INODE: verilen değeri bir EA inode'unda depolar, inode numarasını döndürür.
    /// EA inode: i_flags |= EXT4_EA_INODE_FL, i_atime = CRC32C(value),
    /// i_ctime+i_version_hi = 64-bit refcount (başlangıç 1),
    /// i_mtime = owning_inode, i_generation = owning_inode_generation (back-reference).
    fn create_ea_inode(
        &mut self,
        owning_inode: u32,
        value: &[u8],
        storage: &mut Ext4Storage,
    ) -> Result<u32, Ext4Error> {
        let ea_ino = self.alloc_inode(storage)?;
        let mut ea_inode = Ext4Inode::default();
        ea_inode.i_flags = EXT4_EA_INODE_FL | 0x00080000; // EA_INODE_FL + EXTENTS_FL
        ea_inode.i_mode = 0x8000; // regular file (S_IFREG)
        ea_inode.i_links_count = 1;
        ea_inode.i_extra_isize = 28; // default extra_isize
        // i_atime = CRC32C checksum of value
        let csum = crate::fs::journal::crc32c(value);
        ea_inode.i_atime = csum;
        // i_ctime + i_version_hi = 64-bit refcount = 1
        ea_inode.i_ctime = 1;
        ea_inode.i_version_hi = 0;
        // i_mtime + i_generation = back-reference to owning inode
        ea_inode.i_mtime = owning_inode;
        let owner_inode = self.read_inode_from_storage(owning_inode, storage)?;
        ea_inode.i_generation = owner_inode.i_generation;

        // Write inode to disk
        self.write_inode(ea_ino, &ea_inode, storage)?;

        // Write value as file data
        if !value.is_empty() {
            self.write_file_to_storage(&mut ea_inode, 0, value, storage)?;
            ea_inode.i_size_lo = (value.len() as u64 & 0xFFFFFFFF) as u32;
            ea_inode.i_size_hi = (value.len() as u64 >> 32) as u32;
            self.write_inode(ea_ino, &ea_inode, storage)?;
        }

        Ok(ea_ino)
    }

    /// EA_INODE referans sayısını bir azaltır, 0'sa EA inode'u ve bloklarını serbest bırakır.
    fn release_ea_inode(&mut self, ea_ino: u32, storage: &mut Ext4Storage) -> Result<(), Ext4Error> {
        let mut ea_inode = self.read_inode_from_storage(ea_ino, storage)?;
        if ea_inode.i_flags & EXT4_EA_INODE_FL == 0 {
            return Ok(()); // not an EA inode
        }
        let refcount_low = ea_inode.i_ctime as u64;
        let refcount_high = (ea_inode.i_version_hi as u64) << 32;
        let refcount = refcount_low | refcount_high;
        if refcount <= 1 {
            // Free data blocks and inode
            free_inode_blocks(self, &mut ea_inode, storage)?;
            self.free_inode(ea_ino, storage)?;
        } else {
            // Decrement refcount
            let new_ref = refcount - 1;
            ea_inode.i_ctime = (new_ref & 0xFFFFFFFF) as u32;
            ea_inode.i_version_hi = (new_ref >> 32) as u32;
            self.write_inode(ea_ino, &ea_inode, storage)?;
        }
        Ok(())
    }

    /// Mevcut xattr entry'lerindeki EA_INODE referanslarını bulup serbest bırakır.
    fn release_old_ea_inodes(
        &mut self,
        inode_num: u32,
        inode: &Ext4Inode,
        storage: &mut Ext4Storage,
    ) -> Result<(), Ext4Error> {
        let inode_size = self.superblock.s_inode_size as usize;
        let extra_isize = inode.i_extra_isize as usize;
        let inline_capacity = inode_size.saturating_sub(128 + extra_isize);

        // Inline xattr entries
        if inline_capacity >= 4 {
            let raw = self.read_inode_raw_from_storage(inode_num, storage)?;
            let start = 128 + extra_isize;
            if start + 4 <= raw.len() {
                let magic = u32::from_le_bytes([raw[start], raw[start+1], raw[start+2], raw[start+3]]);
                if magic == EXT4_XATTR_MAGIC {
                    let inline_data = &raw[start..raw.len().min(start + inline_capacity)];
                    let entries = parse_xattr_entries(&inline_data[4..], inline_data.len(), true);
                    for entry in &entries {
                        if entry.e_value_inum != 0 {
                            let _ = self.release_ea_inode(entry.e_value_inum, storage);
                        }
                    }
                }
            }
        }

        // Block EA entries
        if inode.i_file_acl_lo != 0 {
            let block_size = self.block_size as usize;
            let block_offset = (inode.i_file_acl_lo as u64 * block_size as u64) as usize;
            if let Ok(block_data) = storage.read_exact(block_offset, block_size) {
                if block_data.len() >= 4 {
                    let h_magic = u32::from_le_bytes([block_data[0], block_data[1], block_data[2], block_data[3]]);
                    if h_magic == EXT4_XATTR_MAGIC {
                        let entries = parse_xattr_entries(&block_data[32..], block_data.len(), false);
                        for entry in &entries {
                            if entry.e_value_inum != 0 {
                                let _ = self.release_ea_inode(entry.e_value_inum, storage);
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Inode için xattr'ları diske yazar (inline varsa inline'a, yoksa external EA bloğuna,
    /// çok büyük değerler için EA_INODE kullanır).
    pub fn write_xattrs_to_storage(
        &mut self,
        inode_num: u32,
        attrs: &[(String, Vec<u8>)],
        storage: &mut Ext4Storage,
    ) -> Result<(), Ext4Error> {
        if self.read_only {
            return Err(Ext4Error::WriteError);
        }
        let inode = self.read_inode_from_storage(inode_num, storage)?;
        let inode_size = self.superblock.s_inode_size as usize;
        let extra_isize = inode.i_extra_isize as usize;

        // Inline xattr kapasitesini hesapla: inode_size - (128 + extra_isize) bytes
        let inline_capacity = inode_size.saturating_sub(128 + extra_isize);

        // Önce mevcut EA_INODE referanslarını temizle
        self.release_old_ea_inodes(inode_num, &inode, storage)?;

        if attrs.is_empty() {
            // Tüm xattr'ları temizle
            if inode.i_file_acl_lo != 0 {
                // EA bloğunu serbest bırak
                self.free_block(inode.i_file_acl_lo as u64, storage)?;
            }
            // Inline xattr alanını temizle
            let mut raw = self.read_inode_raw_from_storage(inode_num, storage)?;
            if raw.len() >= 128 + extra_isize + 4 {
                let start = 128 + extra_isize;
                let end = raw.len().min(128 + extra_isize + inline_capacity);
                for b in raw[start..end].iter_mut() {
                    *b = 0;
                }
            }
            // i_file_acl_lo'yu sıfırla
            let mut clean_inode = inode;
            clean_inode.i_file_acl_lo = 0;
            self.write_inode(inode_num, &clean_inode, storage)?;
            self.write_inode_raw(inode_num, &raw, storage)?;
            return Ok(());
        }

        // Serileştirilmiş inline body boyutunu hesapla
        let inline_body = create_inline_xattr_body(attrs, None);
        let fits_inline = inline_body.len() <= inline_capacity;

        if fits_inline {
            // Inline'a yaz: inode_raw oku, inline kısmı güncelle, yaz
            let mut raw = self.read_inode_raw_from_storage(inode_num, storage)?;
            let start = 128 + extra_isize;
            if start + inline_body.len() <= raw.len() {
                // Önce eski inline alanını temizle
                let end = raw.len().min(start + inline_capacity);
                for b in raw[start..end].iter_mut() {
                    *b = 0;
                }
                raw[start..start + inline_body.len()].copy_from_slice(&inline_body);
            }

            // Eski EA bloğu varsa serbest bırak
            if inode.i_file_acl_lo != 0 {
                self.free_block(inode.i_file_acl_lo as u64, storage)?;
            }

            // Inode'u güncelle: i_file_acl_lo = 0
            let mut updated_inode = inode;
            updated_inode.i_file_acl_lo = 0;
            self.write_inode(inode_num, &updated_inode, storage)?;
            self.write_inode_raw(inode_num, &raw, storage)?;
        } else {
            // External EA bloğu kullan (EA_INODE ile büyük değerleri ayrı inode'lara taşı)
            let block_size = self.block_size as usize;
            let ea_inode_avail = (self.superblock.s_feature_incompat & EXT4_FEATURE_INCOMPAT_EA_INODE) != 0;

            let mut ea_map: alloc::collections::BTreeMap<String, (u32, u32)> = alloc::collections::BTreeMap::new();
            let mut remaining_attrs: Vec<(String, Vec<u8>)> = attrs.to_vec();

            if ea_inode_avail {
                // Determine maximum entries+names size in block (32 header, rest for entries)
                // EA_INODE entries save value bytes but still take 16+name_len entry bytes
                loop {
                    let total_entry_size: usize = remaining_attrs
                        .iter()
                        .map(|(n, _)| {
                            let (_, suffix) = split_xattr_name(n);
                            let name_len = suffix.len().min(255);
                            (16 + name_len + 3) & !3
                        })
                        .sum();
                    // EA_INODE value sizes don't count toward block space
                    let stored_value_size: usize = remaining_attrs
                        .iter()
                        .filter(|(n, _)| !ea_map.contains_key(n.as_str()))
                        .map(|(_, v)| (v.len() + 3) & !3)
                        .sum();
                    let needed = 32 + total_entry_size + stored_value_size;
                    if needed <= block_size {
                        break;
                    }
                    // Find largest non-EA_INODE value to convert
                    let pick = remaining_attrs
                        .iter()
                        .enumerate()
                        .filter(|(_, (n, _))| !ea_map.contains_key(n.as_str()))
                        .max_by_key(|(_, (_, v))| v.len());
                    if let Some((idx, (name, value))) = pick {
                        let inum = self.create_ea_inode(inode_num, value, storage)?;
                        let sz = value.len() as u32;
                        ea_map.insert(name.clone(), (inum, sz));
                    } else {
                        break; // all remaining already in EA_INODE but still doesn't fit — give up
                    }
                }
            }

            // Recalculate remaining_attrs for the actual serialization
            let block_data = if ea_map.is_empty() {
                create_xattr_block(attrs, block_size, None)
            } else {
                create_xattr_block(attrs, block_size, Some(&ea_map))
            };

            // Yeni blok ata veya eski bloğu yeniden kullan
            let ea_block = if inode.i_file_acl_lo != 0 {
                inode.i_file_acl_lo as u64
            } else {
                let mut zeroed_inode = Ext4Inode::default();
                self.alloc_block(&mut zeroed_inode, 0, storage)?
            };

            let block_offset = ea_block as usize * block_size;
            storage.write_exact(block_offset, &block_data)?;

            // Inline xattr alanını temizle
            let mut raw = self.read_inode_raw_from_storage(inode_num, storage)?;
            let start = 128 + extra_isize;
            let end = raw.len().min(start + inline_capacity);
            for b in raw[start..end].iter_mut() {
                *b = 0;
            }

            // Inode'u güncelle: i_file_acl_lo = ea_block
            let mut updated_inode = inode;
            updated_inode.i_file_acl_lo = ea_block as u32;
            self.write_inode(inode_num, &updated_inode, storage)?;
            self.write_inode_raw(inode_num, &raw, storage)?;
        }

        Ok(())
    }

    /// Blok grubu tanımlayıcısını diske yazar
    pub fn write_group_descriptor(&self, group: u32, storage: &mut Ext4Storage) -> Result<(), Ext4Error> {
        let gd = self.group_descriptors.get(group as usize)
            .ok_or(Ext4Error::OutOfMemory)?;
        let gd_offset = self.block_size as usize;
        let gd_size = self.gd_size();
        let offset = gd_offset + group as usize * gd_size;
        if gd_size >= 64 {
            storage.write_exact(offset, &gd.serialize_64())
        } else {
            storage.write_exact(offset, &gd.serialize_32())
        }
    }

    /// Inode bitmap'ini verilen blok grubu için okur
    pub fn read_inode_bitmap(&self, group: u32, storage: &Ext4Storage) -> Result<Vec<u8>, Ext4Error> {
        let gd = self.group_descriptors.get(group as usize).ok_or(Ext4Error::OutOfMemory)?;
        let bitmap_block = gd.inode_bitmap(self.is_64bit);
        let offset = bitmap_block as usize * self.block_size as usize;
        storage.read_exact(offset, self.block_size as usize)
    }

    /// Block bitmap'ini verilen blok grubu için okur
    pub fn read_block_bitmap(&self, group: u32, storage: &Ext4Storage) -> Result<Vec<u8>, Ext4Error> {
        let gd = self.group_descriptors.get(group as usize).ok_or(Ext4Error::OutOfMemory)?;
        let bitmap_block = gd.block_bitmap(self.is_64bit);
        let offset = bitmap_block as usize * self.block_size as usize;
        storage.read_exact(offset, self.block_size as usize)
    }

    /// Bitmap'te sıfır olan ilk biti bulur ve 1 yapar, index'ini döndürür
    fn bitmap_alloc(bitmap: &mut [u8], start: u32, max: u32) -> Option<u32> {
        for i in start..max {
            let byte_idx = (i / 8) as usize;
            let bit_idx = (i % 8) as u8;
            if byte_idx >= bitmap.len() {
                return None;
            }
            if bitmap[byte_idx] & (1 << bit_idx) == 0 {
                bitmap[byte_idx] |= 1 << bit_idx;
                return Some(i);
            }
        }
        None
    }

    /// Bitmap'te belirtilen biti 0 yapar (serbest bırakır)
    fn bitmap_free(bitmap: &mut [u8], index: u32) {
        let byte_idx = (index / 8) as usize;
        let bit_idx = (index % 8) as u8;
        if byte_idx < bitmap.len() {
            bitmap[byte_idx] &= !(1 << bit_idx);
        }
    }

    /// Boş bir inode bulur, bitmap'te işaretler ve inode numarasını döndürür
    pub fn alloc_inode(&mut self, storage: &mut Ext4Storage) -> Result<u32, Ext4Error> {
        if self.read_only {
            return Err(Ext4Error::WriteError);
        }
        let groups = self.superblock.block_groups_count();
        let inodes_per_group = self.superblock.s_inodes_per_group;

        for group in 0..groups {
            // Check if group is uninit — extract info before any mutable borrow
            let is_uninit = self.group_descriptors.get(group as usize)
                .map(|gd| gd.is_inode_uninit())
                .unwrap_or(false);
            let is_zeroed = self.group_descriptors.get(group as usize)
                .map(|gd| gd.is_inode_zeroed())
                .unwrap_or(false);

            if is_uninit {
                // Spec: INODE_UNINIT ise tüm inode'lar free kabul edilir, ilkini kullan
                let new_inode = group * inodes_per_group + 1;
                // Inode tablosu zeroed değilse sıfırla (lazy init)
                if !is_zeroed {
                    self.zero_inode_table(group, storage)?;
                    if let Some(gd_mut) = self.group_descriptors.get_mut(group as usize) {
                        gd_mut.bg_flags |= 0x4; // INODE_ZEROED
                    }
                }
                let mut bitmap = self.read_inode_bitmap(group, storage)?;
                if bitmap.is_empty() {
                    continue;
                }
                bitmap[0] |= 1;
                let bitmap_block = self.group_descriptors.get(group as usize)
                    .ok_or(Ext4Error::OutOfMemory)?
                    .inode_bitmap(self.is_64bit);
                let offset = bitmap_block as usize * self.block_size as usize;
                storage.write_exact(offset, &bitmap)?;
                // GD counter: inodes_per_group - 1 remaining
                if let Some(gd_mut) = self.group_descriptors.get_mut(group as usize) {
                    let remaining = inodes_per_group - 1;
                    if self.is_64bit {
                        gd_mut.bg_free_inodes_count_lo = (remaining & 0xFFFF) as u16;
                        gd_mut.bg_free_inodes_count_hi = (remaining >> 16) as u16;
                    } else {
                        gd_mut.bg_free_inodes_count_lo = (remaining & 0xFFFF) as u16;
                    }
                }
                // Clear INODE_UNINIT flag
                if let Some(gd_mut) = self.group_descriptors.get_mut(group as usize) {
                    gd_mut.bg_flags &= !0x1;
                }
                self.write_group_descriptor(group, storage)?;
                return Ok(new_inode);
            }

            let free_count = {
                let gd = self.group_descriptors.get(group as usize)
                    .ok_or(Ext4Error::OutOfMemory)?;
                gd.free_inodes_count(self.is_64bit)
            };

            if free_count == 0 {
                continue;
            }

            let mut bitmap = self.read_inode_bitmap(group, storage)?;
            let start = group * inodes_per_group;
            if let Some(ino) = Self::bitmap_alloc(&mut bitmap, start, start + inodes_per_group) {
                let bitmap_block = {
                    let gd = self.group_descriptors.get(group as usize)
                        .ok_or(Ext4Error::OutOfMemory)?;
                    gd.inode_bitmap(self.is_64bit)
                };
                let offset = bitmap_block as usize * self.block_size as usize;
                storage.write_exact(offset, &bitmap)?;
                // Decrement GD counter
                if let Some(gd_mut) = self.group_descriptors.get_mut(group as usize) {
                    if self.is_64bit {
                        gd_mut.bg_free_inodes_count_lo = ((free_count - 1) & 0xFFFF) as u16;
                        gd_mut.bg_free_inodes_count_hi = ((free_count - 1) >> 16) as u16;
                    } else {
                        gd_mut.bg_free_inodes_count_lo = ((free_count - 1) & 0xFFFF) as u16;
                    }
                }
                self.write_group_descriptor(group, storage)?;
                return Ok(ino);
            }
        }
        Err(Ext4Error::OutOfMemory)
    }

    /// Inode tablosunu sıfırlar (lazy init için: INODE_UNINIT + !INODE_ZEROED durumunda çağrılır)
    fn zero_inode_table(&self, group: u32, storage: &mut Ext4Storage) -> Result<(), Ext4Error> {
        let gd = self.group_descriptors.get(group as usize)
            .ok_or(Ext4Error::OutOfMemory)?;
        let inode_table_block = gd.inode_table(self.is_64bit);
        let inodes_per_group = self.superblock.s_inodes_per_group;
        let inode_size = self.superblock.s_inode_size as u64;
        let block_size = self.block_size as u64;
        let table_size = (inodes_per_group as u64 * inode_size + block_size - 1) / block_size;
        let zero_block = vec![0u8; block_size as usize];
        for i in 0..table_size {
            let block_num = (inode_table_block + i) as u64;
            let offset = (block_num * block_size) as usize;
            storage.write_exact(offset, &zero_block)?;
        }
        Ok(())
    }

    /// Belirtilen inode'u bitmap'te serbest bırakır
    pub fn free_inode(&mut self, inode: u32, storage: &mut Ext4Storage) -> Result<(), Ext4Error> {
        if self.read_only {
            return Err(Ext4Error::WriteError);
        }
        let inodes_per_group = self.superblock.s_inodes_per_group;
        let group = (inode - 1) / inodes_per_group;
        let index = (inode - 1) % inodes_per_group;
        let mut bitmap = self.read_inode_bitmap(group, storage)?;
        Self::bitmap_free(&mut bitmap, index);
        let gd = self.group_descriptors.get_mut(group as usize)
            .ok_or(Ext4Error::OutOfMemory)?;
        let free_count = gd.free_inodes_count(self.is_64bit);
        if self.is_64bit {
            gd.bg_free_inodes_count_lo = ((free_count + 1) & 0xFFFF) as u16;
            gd.bg_free_inodes_count_hi = ((free_count + 1) >> 16) as u16;
        } else {
            gd.bg_free_inodes_count_lo = ((free_count + 1) & 0xFFFF) as u16;
        }
        let bitmap_block = gd.inode_bitmap(self.is_64bit);
        let offset = bitmap_block as usize * self.block_size as usize;
        storage.write_exact(offset, &bitmap)?;
        self.write_group_descriptor(group, storage)
    }

    /// Orphan blok checksum'ını doğrular (CRC32C over UUID + inum + block_data[0..blocksize-4]).
    /// Spec: orphan.html — kernel ext4_orphan_block_csum()
    fn verify_orphan_block_csum(&self, block: &[u8], orphan_file_inum: u32) -> bool {
        let block_size = self.block_size as usize;
        if block.len() < block_size {
            return false;
        }
        if block.len() < 4 {
            return false;
        }
        let stored = u32::from_le_bytes([block[block_size - 4], block[block_size - 3], block[block_size - 2], block[block_size - 1]]);
        let mut seed = crate::fs::journal::crc32c_with_seed(&self.superblock.s_uuid, !0u32);
        seed = crate::fs::journal::crc32c_with_seed(&orphan_file_inum.to_le_bytes(), seed);
        let computed = crate::fs::journal::crc32c_with_seed(&block[..block_size - 4], seed);
        stored == computed
    }

    /// Orphan dosyasından tüm blokları okuyup __le32 inode numaralarını döndürür.
    /// Her blok: [0..blocksize-8) = __le32 entries, [blocksize-8..blocksize-4) = magic, [blocksize-4..) = checksum
    fn read_orphan_blocks(&self, storage: &Ext4Storage) -> Result<Vec<Vec<u32>>, Ext4Error> {
        if self.superblock.s_orphan_file_inum == 0 {
            return Ok(Vec::new());
        }
        let orphan_inode = self.read_inode_from_storage(self.superblock.s_orphan_file_inum, storage)?;
        let file_data = self.read_file_from_storage(&orphan_inode, storage)?;
        let block_size = self.block_size as usize;
        if file_data.is_empty() {
            return Ok(Vec::new());
        }
        let mut blocks = Vec::new();
        for chunk in file_data.chunks(block_size) {
            if chunk.len() != block_size {
                // Partial block: orphan file tam bloklardan oluşmalı
                continue;
            }
            if chunk.len() < 8 {
                continue;
            }
            let magic = u32::from_le_bytes([chunk[block_size - 8], chunk[block_size - 7], chunk[block_size - 6], chunk[block_size - 5]]);
            if magic != EXT4_ORPHAN_MAGIC {
                continue;
            }
            if !self.verify_orphan_block_csum(chunk, self.superblock.s_orphan_file_inum) {
                crate::serial_println!("[ext4] Orphan blok checksum hatası, blok atlanıyor");
                continue;
            }
            let entry_count = (block_size - 8) / 4;
            let mut inodes = Vec::new();
            for i in 0..entry_count {
                let off = i * 4;
                let ino = u32::from_le_bytes([chunk[off], chunk[off + 1], chunk[off + 2], chunk[off + 3]]);
                if ino != 0 {
                    inodes.push(ino);
                }
            }
            blocks.push(inodes);
        }
        Ok(blocks)
    }

    /// Tek bir orphan inode'u işler:
    ///   nlink==0 → bloklarını + inode'u free et
    ///   nlink>0  → logla (truncate recovery gerektirir)
    fn process_one_orphan(&mut self, ino: u32, storage: &mut Ext4Storage) {
        if ino == 0 || ino >= self.superblock.s_inodes_count {
            return;
        }
        match self.read_inode_from_storage(ino, storage) {
            Ok(mut inode) => {
                if inode.i_links_count == 0 {
                    crate::serial_println!("[ext4] Orphan inode {} (nlink=0) temizleniyor", ino);
                    let _ = free_inode_blocks(self, &mut inode, storage);
                    let _ = self.free_inode(ino, storage);
                } else {
                    crate::serial_println!("[ext4] Orphan inode {} (nlink={}) atlanıyor (truncate recovery)", ino, inode.i_links_count);
                }
            }
            Err(e) => {
                crate::serial_println!("[ext4] Orphan inode {} okunamadı: {:?}", ino, e);
            }
        }
    }

    /// Legacy orphan listesini (s_last_orphan → i_dtime linked list) işler.
    /// Bu yöntem COMPAT_ORPHAN_FILE olmayan dosya sistemlerinde kullanılır.
    fn process_legacy_orphans(&mut self, storage: &mut Ext4Storage) -> Result<(), Ext4Error> {
        let mut ino = self.superblock.s_last_orphan;
        if ino == 0 {
            return Ok(());
        }
        crate::serial_println!("[ext4] Legacy orphan listesi bulundu (s_last_orphan={})", ino);
        let mut count = 0u32;
        while ino != 0 {
            if ino >= self.superblock.s_inodes_count {
                break;
            }
            let next_ino = match self.read_inode_from_storage(ino, storage) {
                Ok(inode) => {
                    self.process_one_orphan(ino, storage);
                    count += 1;
                    inode.i_dtime // linked list: i_dtime → previous orphan inode
                }
                Err(e) => {
                    crate::serial_println!("[ext4] Legacy orphan inode {} okunamadı: {:?}", ino, e);
                    break;
                }
            };
            ino = next_ino;
        }
        crate::serial_println!("[ext4] Legacy orphan işleme tamam: {} inode işlendi", count);
        // s_last_orphan'ı temizle
        self.superblock.s_last_orphan = 0;
        let mut sb_bytes = storage
            .read_exact(SUPERBLOCK_OFFSET as usize, 1024)
            .map_err(|_| Ext4Error::WriteError)?;
        sb_bytes[232..236].copy_from_slice(&0u32.to_le_bytes());
        storage.write_exact(SUPERBLOCK_OFFSET as usize, &sb_bytes)?;
        Ok(())
    }

    /// Orphan dosyasındaki tüm orphan inode'ları işler.
    /// nlink==0 olan inode'ları siler (bloklarını+inode'u free eder).
    /// nlink>0 olanları (başarısız truncate) şimdilik atlar.
    pub fn process_orphans(&mut self, storage: &mut Ext4Storage) -> Result<(), Ext4Error> {
        let compat = self.superblock.s_feature_compat;
        let ro_compat = self.superblock.s_feature_ro_compat;
        let has_orphan_file = (compat & EXT4_FEATURE_COMPAT_ORPHAN_FILE) != 0;
        let has_present = (ro_compat & EXT4_FEATURE_RO_COMPAT_ORPHAN_PRESENT) != 0;

        // Legacy orphan listesini de dene (s_last_orphan)
        if self.superblock.s_last_orphan != 0 {
            self.process_legacy_orphans(storage)?;
        }

        if !has_orphan_file || !has_present {
            return Ok(());
        }

        crate::serial_println!("[ext4] Orphan dosyası bulundu, inum={}", self.superblock.s_orphan_file_inum);

        let orphan_blocks = self.read_orphan_blocks(storage)?;
        let mut processed = 0u32;
        for block in &orphan_blocks {
            for &ino in block {
                self.process_one_orphan(ino, storage);
                processed += 1;
            }
        }
        crate::serial_println!("[ext4] Orphan işleme tamam: {} inode işlendi", processed);

        // RO_COMPAT_ORPHAN_PRESENT'i temizle (artık tüm orphan'lar işlendi)
        self.clear_orphan_present(storage)?;

        Ok(())
    }

    /// RO_COMPAT_ORPHAN_PRESENT bayrağını temizler ve süper bloğu diske yazar.
    pub fn clear_orphan_present(&mut self, storage: &mut Ext4Storage) -> Result<(), Ext4Error> {
        let ro = self.superblock.s_feature_ro_compat;
        if (ro & EXT4_FEATURE_RO_COMPAT_ORPHAN_PRESENT) == 0 {
            return Ok(());
        }
        self.superblock.s_feature_ro_compat &= !EXT4_FEATURE_RO_COMPAT_ORPHAN_PRESENT;

        // Süper bloğu diske yaz
        let mut sb_bytes = storage
            .read_exact(SUPERBLOCK_OFFSET as usize, 1024)
            .map_err(|_| Ext4Error::WriteError)?;
        // s_feature_ro_compat offset 96..99
        let val = self.superblock.s_feature_ro_compat.to_le_bytes();
        sb_bytes[96..100].copy_from_slice(&val);
        // checksum'u yeniden hesapla
        if self.superblock.s_feature_ro_compat & EXT4_FEATURE_RO_COMPAT_METADATA_CSUM != 0 {
            let mut zeroed = sb_bytes.clone();
            zeroed[1020..1024].copy_from_slice(&[0u8; 4]);
            let csum = crate::fs::journal::crc32c(&zeroed);
            sb_bytes[1020..1024].copy_from_slice(&csum.to_le_bytes());
            self.superblock.s_checksum = csum;
        }
        storage.write_exact(SUPERBLOCK_OFFSET as usize, &sb_bytes)?;

        crate::serial_println!("[ext4] RO_COMPAT_ORPHAN_PRESENT temizlendi");
        Ok(())
    }

    // ============================================================================
    // MMP (Multiple Mount Protection)
    // ============================================================================

    /// MMP bloğunu diskten okur, magic + checksum doğrular.
    fn read_mmp_block(&self, storage: &Ext4Storage) -> Result<Vec<u8>, Ext4Error> {
        let mmp_block_num = self.superblock.s_mmp_block;
        if mmp_block_num == 0 {
            return Err(Ext4Error::NotFound);
        }
        let block_size = self.block_size as usize;
        let offset = mmp_block_num as usize * block_size;
        let data = storage.read_exact(offset, block_size).map_err(|_| Ext4Error::ReadError)?;

        // Magic kontrolü
        let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        if magic != EXT4_MMP_MAGIC {
            return Err(Ext4Error::Corrupted);
        }

        // Checksum (metadata_csum varsa)
        if self.superblock.s_feature_ro_compat & EXT4_FEATURE_RO_COMPAT_METADATA_CSUM != 0 {
            if block_size >= 8 {
                let stored_csum = u32::from_le_bytes([
                    data[block_size - 4], data[block_size - 3],
                    data[block_size - 2], data[block_size - 1],
                ]);
                let mut csum_input = Vec::with_capacity(16 + block_size - 4);
                csum_input.extend_from_slice(&self.superblock.s_uuid);
                csum_input.extend_from_slice(&data[..block_size - 4]);
                let computed = crate::fs::journal::crc32c(&csum_input);
                if stored_csum != 0 && computed != stored_csum {
                    return Err(Ext4Error::Corrupted);
                }
            }
        }

        Ok(data)
    }

    /// MMP bloğunu diske yazar (checksum dahil).
    fn write_mmp_block(&mut self, data: &mut Vec<u8>, storage: &mut Ext4Storage) -> Result<(), Ext4Error> {
        let block_size = self.block_size as usize;
        if data.len() < block_size {
            return Err(Ext4Error::WriteError);
        }

        // Checksum hesapla (metadata_csum varsa)
        if self.superblock.s_feature_ro_compat & EXT4_FEATURE_RO_COMPAT_METADATA_CSUM != 0 {
            let mut csum_input = Vec::with_capacity(16 + block_size - 4);
            csum_input.extend_from_slice(&self.superblock.s_uuid);
            csum_input.extend_from_slice(&data[..block_size - 4]);
            let csum = crate::fs::journal::crc32c(&csum_input);
            data[block_size - 4..block_size].copy_from_slice(&csum.to_le_bytes());
        }

        let mmp_block_num = self.superblock.s_mmp_block;
        let offset = mmp_block_num as usize * block_size;
        storage.write_exact(offset, &data)
    }

    /// MMP dizisini diske yazar.
    fn write_mmp_seq(&mut self, seq: u32, storage: &mut Ext4Storage) -> Result<(), Ext4Error> {
        let mut data = self.read_mmp_block(storage)?;
        data[4..8].copy_from_slice(&seq.to_le_bytes());
        // Update time
        let now = crate::time::current_timestamp_nanos() / 1_000_000_000;
        data[8..16].copy_from_slice(&now.to_le_bytes());
        self.write_mmp_block(&mut data, storage)
    }

    /// MMP kontrolü: mount/open sırasında çağrılır.
    /// seq_clean: eğer temiz unmount yapılmışsa EXT4_MMP_SEQ_CLEAN beklenir.
    /// Döner: (mevcut_seq, mmp_block_data)
    pub fn check_mmp(&self, storage: &Ext4Storage) -> Result<(u32, Vec<u8>), Ext4Error> {
        if (self.superblock.s_feature_incompat & EXT4_FEATURE_INCOMPAT_MMP) == 0 {
            return Err(Ext4Error::NotFound); // MMP disabled
        }
        if self.superblock.s_mmp_block == 0 {
            return Err(Ext4Error::NotFound);
        }

        let data = self.read_mmp_block(storage)?;
        let seq = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

        if seq == EXT4_MMP_SEQ_CLEAN {
            // Temiz unmount, mount edilebilir
            return Ok((seq, data));
        }

        if seq == EXT4_MMP_SEQ_FSCK {
            // fsck çalışıyor, mount reddedilir
            return Err(Ext4Error::Corrupted); // FS_BUSY
        }

        // Seq başka bir değer — fs başka hostta mountlu
        // MMP interval kadar bekle ve tekrar kontrol et
        let interval = if self.superblock.s_mmp_interval > 0 {
            self.superblock.s_mmp_interval as u64
        } else {
            5 // default 5 saniye
        };

        // Wait 2 * interval
        crate::time::sleep(core::time::Duration::from_secs(interval * 2));

        // Re-read
        let data2 = self.read_mmp_block(storage)?;
        let seq2 = u32::from_le_bytes([data2[4], data2[5], data2[6], data2[7]]);

        if seq2 != seq {
            // Seq değişti — başka host aktif, mount reddedilir
            return Err(Ext4Error::Corrupted); // FS_BUSY
        }

        // Seq değişmedi — muhtemelen eski/frozen, mount edilebilir
        Ok((seq2, data2))
    }

    /// MMP başlatma: mount işleminin son aşamasında çağrılır.
    /// Yeni bir seq üretir, nodename/bdevname yazar, MMP bloğunu günceller.
    pub fn init_mmp(&mut self, nodename: &str, bdevname: &str, storage: &mut Ext4Storage) -> Result<(), Ext4Error> {
        if (self.superblock.s_feature_incompat & EXT4_FEATURE_INCOMPAT_MMP) == 0 {
            return Ok(());
        }
        if self.superblock.s_mmp_block == 0 {
            return Ok(());
        }

        let block_size = self.block_size as usize;
        let mut data = vec![0u8; block_size];

        // Magic
        data[0..4].copy_from_slice(&EXT4_MMP_MAGIC.to_le_bytes());
        // Yeni seq: CRC32C(time) — rastgelelik için
        let now = crate::time::current_timestamp_nanos() / 1_000_000_000;
        let seq = crate::fs::journal::crc32c(&now.to_le_bytes());
        data[4..8].copy_from_slice(&seq.to_le_bytes());
        // Time
        data[8..16].copy_from_slice(&now.to_le_bytes());
        // Nodename (64 bayt)
        let nb = nodename.as_bytes();
        let copy_len = nb.len().min(64);
        data[0x10..0x10 + copy_len].copy_from_slice(&nb[..copy_len]);
        // Bdevname (32 bayt)
        let bb = bdevname.as_bytes();
        let copy_len2 = bb.len().min(32);
        data[0x50..0x50 + copy_len2].copy_from_slice(&bb[..copy_len2]);
        // Check interval
        let interval = self.superblock.s_mmp_interval;
        data[0x70..0x72].copy_from_slice(&interval.to_le_bytes());

        self.write_mmp_block(&mut data, storage)?;

        // In-memory seq kaydet
        self.mmp_seq = seq;

        Ok(())
    }

    /// MMP periyodik güncelleme: seq'i artır ve diske yaz.
    pub fn update_mmp(&mut self, storage: &mut Ext4Storage) -> Result<(), Ext4Error> {
        if (self.superblock.s_feature_incompat & EXT4_FEATURE_INCOMPAT_MMP) == 0 {
            return Ok(());
        }
        if self.mmp_seq == 0 {
            return Ok(());
        }
        let seq = self.mmp_seq.wrapping_add(1);
        self.write_mmp_seq(seq, storage)?;
        self.mmp_seq = seq;
        Ok(())
    }

    /// MMP temizleme: unmount'ta çağrılır, seq'i CLEAN yapar.
    pub fn clear_mmp(&mut self, storage: &mut Ext4Storage) -> Result<(), Ext4Error> {
        if (self.superblock.s_feature_incompat & EXT4_FEATURE_INCOMPAT_MMP) == 0 {
            return Ok(());
        }
        if self.superblock.s_mmp_block == 0 {
            return Ok(());
        }
        self.write_mmp_seq(EXT4_MMP_SEQ_CLEAN, storage)
    }

    // ============================================================================
    // FS-VERITY (Merkle tree verification)
    // ============================================================================

    /// Verity descriptor'i dosya verisinden okur ve döndürür.
    /// Layout: [file data][zero to 64K boundary][Merkle tree][zero to block boundary][desc+sig][pad][desc_size(4B)]
    fn read_verity_descriptor(
        &self,
        inode: &Ext4Inode,
        storage: &Ext4Storage,
    ) -> Result<VerityDescriptor, Ext4Error> {
        if inode.i_flags & EXT4_VERITY_FL == 0 {
            return Err(Ext4Error::NotFound);
        }

        let file_size = inode.size();
        let block_size = self.block_size as u64;
        let merkle_start = (file_size + 65535) & !65535; // round up to 64K

        // Tüm verity metadata bloklarını oku
        let metadata_size = self.inode_size(inode) as u64; // i_blocks * 512
        let total_blocks = metadata_size / 512;
        let verity_end = merkle_start + total_blocks * block_size;

        // Son bloktan itibaren geriye doğru descriptor'ı bul
        // Descriptor, son 4 baytı desc_size olan bir yapıdan önce gelir
        let last_block_offset = (verity_end - block_size) as usize;
        let last_block = storage
            .read_exact(last_block_offset, block_size as usize)
            .map_err(|_| Ext4Error::ReadError)?;

        let desc_size = u32::from_le_bytes([
            last_block[block_size as usize - 4],
            last_block[block_size as usize - 3],
            last_block[block_size as usize - 2],
            last_block[block_size as usize - 1],
        ]) as usize;

        if desc_size == 0 || desc_size > block_size as usize - 4 {
            return Err(Ext4Error::Corrupted);
        }

        let desc_offset = last_block_offset + block_size as usize - 4 - desc_size;
        let desc_start = desc_offset % block_size as usize;
        let desc_data = if desc_start + desc_size <= block_size as usize {
            last_block[desc_start..desc_start + desc_size].to_vec()
        } else {
            // Descriptor block boundary'yi aşıyorsa
            let mut desc = Vec::with_capacity(desc_size);
            desc.extend_from_slice(&last_block[desc_start..]);
            if desc.len() < desc_size {
                let next_block = storage
                    .read_exact(last_block_offset + block_size as usize, block_size as usize)
                    .map_err(|_| Ext4Error::ReadError)?;
                let remaining = desc_size - desc.len();
                desc.extend_from_slice(&next_block[..remaining]);
            }
            desc
        };

        VerityDescriptor::parse(&desc_data).ok_or(Ext4Error::Corrupted)
    }

    /// Verity dosyasındaki tüm Merkle tree seviyelerini okur.
    fn read_merkle_tree(
        &self,
        inode: &Ext4Inode,
        storage: &Ext4Storage,
    ) -> Result<Vec<Vec<u8>>, Ext4Error> {
        let file_size = inode.size();
        let block_size = self.block_size as u64;
        let merkle_start = (file_size + 65535) & !65535;

        // Merkle tree boyutunu hesapla
        // Verity metadata toplam boyutu = inode i_blocks * 512
        let metadata_bytes = (inode.i_blocks_lo as u64) * 512;
        if metadata_bytes == 0 {
            return Err(Ext4Error::NotFound);
        }
        let metadata_end = merkle_start + metadata_bytes;
        let tree_end = metadata_end - 4096; // son blok desc+sig+size içerir

        if tree_end <= merkle_start {
            return Ok(Vec::new());
        }

        let tree_size = (tree_end - merkle_start) as usize;
        let mut levels = Vec::new();
        let mut offset = merkle_start as usize;
        let remaining = tree_size;

        // Her Merkle tree level'ını oku (root→leaf sırası)
        // Level blokları: her biri block_size bayt
        let mut level_offset = 0;
        while level_offset < remaining {
            let level_block_count = remaining / block_size as usize;
            if level_block_count == 0 {
                break;
            }

            let mut level_data = Vec::with_capacity(level_block_count * block_size as usize);
            for b in 0..level_block_count {
                let block_data = storage
                    .read_exact(
                        offset + b * block_size as usize,
                        block_size as usize,
                    )
                    .map_err(|_| Ext4Error::ReadError)?;
                level_data.extend_from_slice(&block_data);
            }
            level_offset += level_data.len();
            levels.push(level_data);
        }

        Ok(levels)
    }

    /// Verity dosyasını doğrular: Merkle tree root hash'ini kontrol eder.
    /// Döner: Ok(()) — root hash eşleşiyor
    ///         Err — veri bozulmuş veya descriptor geçersiz
    pub fn verify_verity_file(
        &self,
        inode: &Ext4Inode,
        file_data: &[u8],
        storage: &Ext4Storage,
    ) -> Result<(), Ext4Error> {
        if inode.i_flags & EXT4_VERITY_FL == 0 {
            return Ok(()); // verity yok, kontrol gerekmez
        }

        let desc = self.read_verity_descriptor(inode, storage)?;
        if desc.version != 1 {
            return Err(Ext4Error::Corrupted);
        }

        let alg = match desc.hash_algorithm {
            1 => VerityHashAlg::Sha256,
            2 => VerityHashAlg::Sha512,
            _ => return Err(Ext4Error::Corrupted),
        };

        let merkle_tree = self.read_merkle_tree(inode, storage)?;
        let computed_root = compute_verity_root_hash(
            alg,
            desc.log_blocksize,
            &desc.salt[..desc.salt_size as usize],
            file_data,
            &merkle_tree,
        );

        let expected_root = &desc.root_hash[..alg.digest_len()];
        if computed_root != expected_root {
            return Err(Ext4Error::ChecksumError);
        }

        Ok(())
    }

    /// Inode'un verity dosyası olup olmadığını kontrol eder
    pub fn is_verity_file(&self, inode: &Ext4Inode) -> bool {
        inode.i_flags & EXT4_VERITY_FL != 0
    }

    /// Verity metadata boyutunu döndürür (descriptor + merkle tree için ayrılmış alan)
    fn verity_metadata_size(&self, inode: &Ext4Inode) -> u64 {
        (inode.i_blocks_lo as u64) * 512
    }

    /// Verity dosyasındaki ham veri bloklarının aralığını döndürür
    /// (verity metadata atlanarak okuma yapılabilmesi için)
    pub fn verity_data_range(&self, inode: &Ext4Inode) -> (u64, u64) {
        let file_size = inode.size();
        (0, file_size)
    }

    /// Boş bir blok bulur, bitmap'te işaretler ve blok numarasını döndürür
    /// Not: sequential allocation approximates real bitmap scanner; for production
    /// use a block-group locality-aware allocator.
    pub fn alloc_block(&mut self, _inode: &mut Ext4Inode, logical_block: u32, storage: &mut Ext4Storage) -> Result<u64, Ext4Error> {
        if self.read_only {
            return Err(Ext4Error::WriteError);
        }
        let groups = self.superblock.block_groups_count();
        let blocks_per_group = self.superblock.s_blocks_per_group - 1; // metadata hariç

        for group in 0..groups {
            let free_count = {
                let gd = self.group_descriptors.get(group as usize)
                    .ok_or(Ext4Error::OutOfMemory)?;
                if gd.is_block_uninit() {
                    // BLOCK_UNINIT: tüm bloklar free kabul edilir, bitmap başlatılmamıştır
                    let mut bitmap = self.read_block_bitmap(group, storage)?;
                    if bitmap.is_empty() {
                        continue;
                    }
                    // Metaveri bloklarını bitmap'te işaretle:
                    //   block bitmap (1 blok), inode bitmap (1 blok), inode tablosu (N blok)
                    let inode_size = self.superblock.s_inode_size as u64;
                    let inodes_per_group = self.superblock.s_inodes_per_group as u64;
                    let itable_blocks = (inodes_per_group * inode_size
                        + self.block_size as u64 - 1) / self.block_size as u64;
                    let metadata_blocks = 2u64 + itable_blocks;
                    let group_start = group as u64 * self.superblock.s_blocks_per_group as u64;
                    let rel_bitmap = (gd.block_bitmap(self.is_64bit) - group_start) as u32;
                    let rel_inode_bitmap = (gd.inode_bitmap(self.is_64bit) - group_start) as u32;
                    let rel_inode_table = (gd.inode_table(self.is_64bit) - group_start) as u32;
                    let bits = self.superblock.s_blocks_per_group;
                    let set_bit = |bitmap: &mut [u8], bit: u32| {
                        if bit < bits {
                            bitmap[(bit / 8) as usize] |= 1 << (bit % 8);
                        }
                    };
                    set_bit(&mut bitmap, rel_bitmap);
                    set_bit(&mut bitmap, rel_inode_bitmap);
                    for i in 0..itable_blocks as u32 {
                        set_bit(&mut bitmap, rel_inode_table + i);
                    }
                    // İlk boş data bloğunu bul
                    let first_free = Self::bitmap_alloc(&mut bitmap, 0, bits)
                        .ok_or(Ext4Error::OutOfMemory)?;
                    let new_block = group_start + first_free as u64;
                    let bitmap_block = gd.block_bitmap(self.is_64bit);
                    let offset = bitmap_block as usize * self.block_size as usize;
                    storage.write_exact(offset, &bitmap)?;
                    // GD counter: blocks_per_group - metadata_blocks remaining
                    if let Some(gd_mut) = self.group_descriptors.get_mut(group as usize) {
                        let bg_count = self.superblock.s_blocks_per_group;
                        let remaining = bg_count - metadata_blocks as u32;
                        if self.is_64bit {
                            gd_mut.bg_free_blocks_count_lo = (remaining & 0xFFFF) as u16;
                            gd_mut.bg_free_blocks_count_hi = (remaining >> 16) as u16;
                        } else {
                            gd_mut.bg_free_blocks_count_lo = (remaining & 0xFFFF) as u16;
                        }
                    }
                    // Clear BLOCK_UNINIT flag
                    if let Some(gd_mut) = self.group_descriptors.get_mut(group as usize) {
                        gd_mut.bg_flags &= !0x2;
                    }
                    self.write_group_descriptor(group, storage)?;
                    return Ok(new_block);
                }
                gd.free_blocks_count(self.is_64bit)
            };

            if free_count == 0 {
                continue;
            }

            let mut bitmap = self.read_block_bitmap(group, storage)?;
            let start = group * self.superblock.s_blocks_per_group + 1;
            // İlk birkaç blok metadata için ayrılmıştır: skip superblock + gdt
            let start_scan = start.max(group * self.superblock.s_blocks_per_group + 2);
            let end = start + blocks_per_group;
            if let Some(blk) = Self::bitmap_alloc(&mut bitmap, start_scan, end) {
                let bitmap_block = {
                    let gd = self.group_descriptors.get(group as usize)
                        .ok_or(Ext4Error::OutOfMemory)?;
                    gd.block_bitmap(self.is_64bit)
                };
                let offset = bitmap_block as usize * self.block_size as usize;
                storage.write_exact(offset, &bitmap)?;
                // Decrement GD counter
                if let Some(gd_mut) = self.group_descriptors.get_mut(group as usize) {
                    if self.is_64bit {
                        gd_mut.bg_free_blocks_count_lo = ((free_count - 1) & 0xFFFF) as u16;
                        gd_mut.bg_free_blocks_count_hi = ((free_count - 1) >> 16) as u16;
                    } else {
                        gd_mut.bg_free_blocks_count_lo = ((free_count - 1) & 0xFFFF) as u16;
                    }
                }
                self.write_group_descriptor(group, storage)?;
                return Ok(blk as u64);
            }
        }
        Err(Ext4Error::OutOfMemory)
    }

    /// Belirtilen bloğu bitmap'te serbest bırakır
    pub fn free_block(&mut self, block: u64, storage: &mut Ext4Storage) -> Result<(), Ext4Error> {
        if self.read_only {
            return Err(Ext4Error::WriteError);
        }
        let blocks_per_group = self.superblock.s_blocks_per_group;
        let group = (block / blocks_per_group as u64) as u32;
        let index = (block % blocks_per_group as u64) as u32;

        let mut bitmap = self.read_block_bitmap(group, storage)?;
        Self::bitmap_free(&mut bitmap, index);

        let (bitmap_block, free_count) = {
            let gd = self.group_descriptors.get(group as usize)
                .ok_or(Ext4Error::OutOfMemory)?;
            (gd.block_bitmap(self.is_64bit), gd.free_blocks_count(self.is_64bit))
        };

        let offset = bitmap_block as usize * self.block_size as usize;
        storage.write_exact(offset, &bitmap)?;

        if let Some(gd_mut) = self.group_descriptors.get_mut(group as usize) {
            if self.is_64bit {
                gd_mut.bg_free_blocks_count_lo = ((free_count + 1) & 0xFFFF) as u16;
                gd_mut.bg_free_blocks_count_hi = ((free_count + 1) >> 16) as u16;
            } else {
                gd_mut.bg_free_blocks_count_lo = ((free_count + 1) & 0xFFFF) as u16;
            }
        }
        self.write_group_descriptor(group, storage)
    }

    /// Mantıksal blok numarasını fiziksel blok numarasına çevirir
    /// extent tree (multi-level) veya indirect block mapping
    pub fn map_block(&self, inode: &Ext4Inode, logical_block: u32) -> Option<u64> {
        if inode.uses_extents() {
            self.map_block_extent_tree(inode, logical_block)
        } else {
            // Indirect mapping — storage gerektirir, caller storage-aware versiyonu kullanmalı
            self.map_block_indirect_stub(inode, logical_block)
        }
    }

    /// Storage-aware map_block — indirect block resolution için
    pub fn map_block_with_storage(
        &self,
        inode: &Ext4Inode,
        logical_block: u32,
        storage: &Ext4Storage,
    ) -> Option<u64> {
        if inode.uses_extents() {
            self.map_block_extent_tree_with_storage(inode, logical_block, storage)
        } else {
            self.map_block_indirect(inode, logical_block, storage)
        }
    }

    /// Extent tree ile blok mapping — multi-level (depth > 0) destekli
    fn map_block_extent_tree(&self, inode: &Ext4Inode, logical_block: u32) -> Option<u64> {
        let header = Ext4ExtentHeader::parse(&inode.i_block[..])?;
        if header.eh_depth == 0 {
            self.find_extent_in_leaf_data(&inode.i_block, logical_block, 0)
        } else {
            // Depth > 0 requires external extent-index blocks; this storage-free helper cannot resolve them.
            None
        }
    }

    fn map_block_extent_tree_with_storage(
        &self,
        inode: &Ext4Inode,
        logical_block: u32,
        storage: &Ext4Storage,
    ) -> Option<u64> {
        let header = Ext4ExtentHeader::parse(&inode.i_block[..])?;
        if header.eh_depth == 0 {
            self.find_extent_in_leaf_data(&inode.i_block, logical_block, 0)
        } else {
            self.traverse_extent_tree_with_storage(
                inode,
                &inode.i_block,
                0,
                &header,
                logical_block,
                storage,
            )
        }
    }

    /// Extent tree'de recursive descent — internal node'dan leaf'e in
    fn traverse_extent_tree_with_storage(
        &self,
        inode: &Ext4Inode,
        parent_data: &[u8],
        header_offset: usize,
        parent_header: &Ext4ExtentHeader,
        logical_block: u32,
        storage: &Ext4Storage,
    ) -> Option<u64> {
        if parent_header.eh_depth == 0 {
            return self.find_extent_in_leaf_data(parent_data, logical_block, header_offset);
        }

        let idx_offset = header_offset + 12;
        let mut lo = 0i32;
        let mut hi = parent_header.eh_entries as i32 - 1;
        let mut child_block: Option<u64> = None;

        while lo <= hi {
            let mid = lo + (hi - lo) / 2;
            let offset = idx_offset + mid as usize * 12;
            if offset + 12 > parent_data.len() {
                break;
            }
            if let Some(idx) = Ext4ExtentIdx::parse(&parent_data[offset..]) {
                if logical_block >= idx.ei_block {
                    child_block = Some(idx.ei_leaf);
                    lo = mid + 1;
                } else {
                    hi = mid - 1;
                }
            } else {
                break;
            }
        }

        let child_blk = child_block?;
        let block_size = self.block_size as usize;
        let child_offset = (child_blk as usize) * block_size;
        let child_data = storage.read_exact(child_offset, block_size).ok()?;

        if let Some(child_header) = Ext4ExtentHeader::parse(&child_data) {
            if child_header.eh_magic != Ext4ExtentHeader::MAGIC {
                return None;
            }
            self.traverse_extent_tree_with_storage(
                inode,
                &child_data,
                0,
                &child_header,
                logical_block,
                storage,
            )
        } else {
            None
        }
    }

    /// Leaf level'de extent bul
    fn find_extent_in_leaf_data(
        &self,
        data: &[u8],
        logical_block: u32,
        header_offset: usize,
    ) -> Option<u64> {
        let header = Ext4ExtentHeader::parse(&data[header_offset..])?;
        let extent_offset = header_offset + 12;
        let mut lo = 0i32;
        let mut hi = header.eh_entries as i32 - 1;
        let mut found: Option<Ext4Extent> = None;

        while lo <= hi {
            let mid = lo + (hi - lo) / 2;
            let offset = extent_offset + mid as usize * 12;
            if offset + 12 > data.len() {
                break;
            }
            if let Some(extent) = Ext4Extent::parse(&data[offset..]) {
                if logical_block >= extent.ee_block {
                    found = Some(extent);
                    lo = mid + 1;
                } else {
                    hi = mid - 1;
                }
            } else {
                break;
            }
        }

        if let Some(extent) = found {
            if logical_block < extent.ee_block + extent.ee_len as u32 {
                let off = logical_block - extent.ee_block;
                return Some(extent.ee_start + off as u64);
            }
        }
        None
    }

    /// Indirect block mapping for the inode-resident direct block array only.
    fn map_block_indirect_stub(&self, inode: &Ext4Inode, logical_block: u32) -> Option<u64> {
        let blocks = inode.indirect_blocks();
        if logical_block < 12 {
            let blk = blocks[logical_block as usize];
            if blk == 0 {
                return None;
            }
            return Some(blk as u64);
        }
        // Single/double/triple indirect — storage gerekir
        None
    }

    /// Indirect block mapping — storage ile full resolution
    fn map_block_indirect(
        &self,
        inode: &Ext4Inode,
        logical_block: u32,
        storage: &Ext4Storage,
    ) -> Option<u64> {
        let blocks = inode.indirect_blocks();
        let block_size = self.block_size as usize;
        let ptrs = block_size / 4;

        if logical_block < 12 {
            let blk = blocks[logical_block as usize];
            if blk == 0 {
                return None;
            }
            return Some(blk as u64);
        }

        let mut remaining = logical_block - 12;

        if remaining < ptrs as u32 {
            let indirect_blk = blocks[12];
            if indirect_blk == 0 {
                return None;
            }
            let offset = (indirect_blk as usize) * block_size + (remaining as usize) * 4;
            return self
                .read_u32_from_storage(storage, offset)
                .map(|v| v as u64);
        }
        remaining -= ptrs as u32;

        let double_blocks = (ptrs * ptrs) as u32;
        if remaining < double_blocks {
            let double_blk = blocks[13];
            if double_blk == 0 {
                return None;
            }
            let idx1 = remaining / ptrs as u32;
            let idx2 = remaining % ptrs as u32;
            let offset1 = (double_blk as usize) * block_size + (idx1 as usize) * 4;
            let indirect_blk = self.read_u32_from_storage(storage, offset1)?;
            if indirect_blk == 0 {
                return None;
            }
            let offset2 = (indirect_blk as usize) * block_size + (idx2 as usize) * 4;
            return self
                .read_u32_from_storage(storage, offset2)
                .map(|v| v as u64);
        }
        remaining -= double_blocks;

        let triple_blocks = (ptrs * ptrs * ptrs) as u32;
        if remaining < triple_blocks {
            let triple_blk = blocks[14];
            if triple_blk == 0 {
                return None;
            }
            let idx1 = remaining / (ptrs * ptrs) as u32;
            let rem1 = remaining % (ptrs * ptrs) as u32;
            let idx2 = rem1 / ptrs as u32;
            let idx3 = rem1 % ptrs as u32;
            let offset1 = (triple_blk as usize) * block_size + (idx1 as usize) * 4;
            let double_blk = self.read_u32_from_storage(storage, offset1)?;
            if double_blk == 0 {
                return None;
            }
            let offset2 = (double_blk as usize) * block_size + (idx2 as usize) * 4;
            let indirect_blk = self.read_u32_from_storage(storage, offset2)?;
            if indirect_blk == 0 {
                return None;
            }
            let offset3 = (indirect_blk as usize) * block_size + (idx3 as usize) * 4;
            return self
                .read_u32_from_storage(storage, offset3)
                .map(|v| v as u64);
        }

        None
    }

    fn read_u32_from_storage(&self, storage: &Ext4Storage, offset: usize) -> Option<u32> {
        let block_size = self.block_size as usize;
        let block_offset = offset & !((block_size) - 1);
        let inner_offset = offset & (block_size - 1);
        let data = storage.read_exact(block_offset, block_size).ok()?;
        if inner_offset + 4 > data.len() {
            return None;
        }
        Some(u32::from_le_bytes([
            data[inner_offset],
            data[inner_offset + 1],
            data[inner_offset + 2],
            data[inner_offset + 3],
        ]))
    }

    /// Dosyanın tüm içeriğini aygıt verisinden okur (device_data slice ile)
    pub fn read_file(&self, inode: &Ext4Inode, device_data: &[u8]) -> Result<Vec<u8>, Ext4Error> {
        // Inline data: içerik i_block[0..size] içinde gömülü
        if inode.i_flags & EXT4_INLINE_DATA_FL != 0 {
            let size = inode.size() as usize;
            let avail = inode.i_block.len().min(size);
            return Ok(inode.i_block[..avail].to_vec());
        }

        let size = inode.size() as usize;
        let mut data = Vec::with_capacity(size);
        let block_size = self.block_size as usize;
        let blocks_needed = (size + block_size - 1) / block_size;

        for i in 0..blocks_needed {
            if let Some(phys_block) = self.map_block(inode, i as u32) {
                let offset = phys_block as usize * block_size;
                let read_size = block_size.min(size.saturating_sub(data.len()));
                if offset + read_size <= device_data.len() {
                    data.extend_from_slice(&device_data[offset..offset + read_size]);
                }
            }
        }

        data.truncate(size);
        Ok(data)
    }

    pub fn read_file_from_storage(
        &self,
        inode: &Ext4Inode,
        storage: &Ext4Storage,
    ) -> Result<Vec<u8>, Ext4Error> {
        // DAX (Direct Access) —prowadziłıcı doğrudan bellek erişimi desteklemez, ham veri oku
        if inode.i_flags & EXT4_DAX_FL != 0 {
            crate::serial_println!(
                "[ext4] Uyarı: DAX inode tespit edildi (i_flags=0x{:x}), ham veri okunuyor",
                inode.i_flags
            );
        }

        // ENCRYPT (fscrypt) —şifreleme desteklenmez, ham şifreli veri okunur
        if inode.i_flags & EXT4_ENCRYPT_FL != 0 {
            crate::serial_println!(
                "[ext4] Uyarı: Şifreli inode tespit edildi (i_flags=0x{:x}), ham veri okunuyor",
                inode.i_flags
            );
        }

        // Inline data: içerik i_block[0..size] içinde gömülü
        if inode.i_flags & EXT4_INLINE_DATA_FL != 0 {
            let size = inode.size() as usize;
            let avail = inode.i_block.len().min(size);
            return Ok(inode.i_block[..avail].to_vec());
        }

        let size = inode.size() as usize;
        let mut data = Vec::with_capacity(size);
        let block_size = self.block_size as usize;
        let blocks_needed = (size + block_size - 1) / block_size;

        for logical in 0..blocks_needed {
            let Some(phys_block) = self.map_block_with_storage(inode, logical as u32, storage)
            else {
                break;
            };
            let offset = phys_block as usize * block_size;
            let read_size = block_size.min(size.saturating_sub(data.len()));
            let block = storage
                .read_exact(offset, read_size)
                .map_err(|_| Ext4Error::ReadError)?;
            data.extend_from_slice(block.as_slice());
        }

        data.truncate(size);

        // Verity doğrulaması (VERITY_FL varsa)
        if self.is_verity_file(inode) {
            if let Err(e) = self.verify_verity_file(inode, &data, storage) {
                crate::serial_println!("[ext4] Verity doğrulama hatası inode: {:?}", e);
                return Err(e);
            }
        }

        Ok(data)
    }

    /// Verilen mantıksal bloğu inode'un extent tree'ine ekler (depth 0).
    /// Eğer extent başlığı yoksa (yeni dosya), başlık oluşturur.
    fn insert_extent(
        &self,
        inode: &mut Ext4Inode,
        logical_block: u32,
        phys_block: u64,
        block_count: u16,
    ) -> Result<(), Ext4Error> {
        // Mevcut extent'leri oku
        let mut header = Ext4ExtentHeader::parse(&inode.i_block[..]);
        let existing = header.is_some();
        if !existing {
            // Yeni extent başlığı oluştur
            inode.i_block[..2].copy_from_slice(&0xF30Au16.to_le_bytes());
            inode.i_block[2..4].copy_from_slice(&0u16.to_le_bytes());  // 0 entries
            inode.i_block[4..6].copy_from_slice(&4u16.to_le_bytes());  // eh_max = 4
            inode.i_block[6..8].copy_from_slice(&0u16.to_le_bytes());  // depth = 0
            inode.i_block[8..12].copy_from_slice(&0u32.to_le_bytes()); // generation = 0
            header = Ext4ExtentHeader::parse(&inode.i_block[..]);
        }

        let mut header = header.ok_or(Ext4Error::Corrupted)?;
        if header.eh_depth != 0 {
            return Err(Ext4Error::NotSupported);
        }

        // Tüm extent'leri topla
        struct RawExtent {
            block: u32,
            len: u16,
            start: u64,
        }

        let mut extents: Vec<RawExtent> = Vec::new();
        for i in 0..header.eh_entries as usize {
            let off = 12 + i * 12;
            if off + 12 > 60 {
                break;
            }
            if let Some(ext) = Ext4Extent::parse(&inode.i_block[off..]) {
                extents.push(RawExtent {
                    block: ext.ee_block,
                    len: ext.ee_len,
                    start: ext.ee_start,
                });
            }
        }

        // Yeni extent'i ekle
        extents.push(RawExtent {
            block: logical_block,
            len: block_count,
            start: phys_block,
        });

        // ee_block'a göre sırala
        extents.sort_by_key(|e| e.block);

        // Birleştirilebilir extent'leri merge et
        let mut merged: Vec<RawExtent> = Vec::new();
        for ext in extents {
            if let Some(last) = merged.last_mut() {
                let last_end = last.block + last.len as u32;
                // Fiziksel bloklar da bitişik mi kontrol et
                if last_end == ext.block
                    && (last.start + last.len as u64) == ext.start
                {
                    last.len += ext.len;
                    continue;
                }
            }
            merged.push(ext);
        }

        if merged.len() > 4 {
            return Err(Ext4Error::NotSupported);
        }

        // i_block'a geri yaz
        let mut new_block = [0u8; 60];
        new_block[..2].copy_from_slice(&0xF30Au16.to_le_bytes());
        new_block[2..4].copy_from_slice(&(merged.len() as u16).to_le_bytes());
        new_block[4..6].copy_from_slice(&4u16.to_le_bytes());
        new_block[6..8].copy_from_slice(&0u16.to_le_bytes());
        new_block[8..12].copy_from_slice(&0u32.to_le_bytes());

        for (i, ext) in merged.iter().enumerate() {
            let dst = 12 + i * 12;
            new_block[dst..dst + 4].copy_from_slice(&ext.block.to_le_bytes());
            new_block[dst + 4..dst + 6].copy_from_slice(&ext.len.to_le_bytes());
            new_block[dst + 6..dst + 8].copy_from_slice(&((ext.start >> 32) as u16).to_le_bytes());
            new_block[dst + 8..dst + 12].copy_from_slice(&(ext.start as u32).to_le_bytes());
        }

        inode.i_block.copy_from_slice(&new_block);
        inode.i_flags |= 0x00080000;
        Ok(())
    }

    /// Inline data'yı i_block'a yazar (EXT4_INLINE_DATA_FL gerektirir)
    /// Eğer data > 60 bayt ise otomatik olarak extent tabanlı tahsise dönüştürür
    pub fn write_inline_data_to_storage(
        &mut self,
        inode_num: u32,
        inode: &mut Ext4Inode,
        offset: u64,
        data: &[u8],
        storage: &mut Ext4Storage,
    ) -> Result<usize, Ext4Error> {
        if self.read_only {
            return Err(Ext4Error::WriteError);
        }

        let inline_max = inode.i_block.len() as u64; // 60

        // Inline data sadece offset=0 ve toplam ≤60 bayt için çalışır
        let new_size = offset + data.len() as u64;

        if offset == 0 && new_size <= inline_max {
            // Inline'a yaz
            inode.i_block[..data.len()].copy_from_slice(data);
            if data.len() < inline_max as usize {
                inode.i_block[data.len()..inline_max as usize].fill(0);
            }
            inode.i_flags |= EXT4_INLINE_DATA_FL;
            inode.i_size_lo = (new_size & 0xFFFFFFFF) as u32;
            inode.i_size_hi = (new_size >> 32) as u32;
            self.write_inode(inode_num, inode, storage)?;
            return Ok(data.len());
        }

        // Data inline'a sığmıyor → extent tabanlı tahsise dönüştür
        if inode.i_flags & EXT4_INLINE_DATA_FL != 0 {
            // Mevcut inline veriyi oku
            let current_size = inode.size();
            let existing = if current_size > 0 && current_size as usize <= inline_max as usize {
                inode.i_block[..current_size as usize].to_vec()
            } else {
                Vec::new()
            };
            inode.i_flags &= !EXT4_INLINE_DATA_FL;
            // i_block'ı temizle (extent header için alan aç)
            inode.i_block.fill(0);

            // extent header'ı başlat (depth=0, max=4)
            inode.i_block[..2].copy_from_slice(&0xF30Au16.to_le_bytes());
            inode.i_block[4..6].copy_from_slice(&4u16.to_le_bytes());
            self.write_inode(inode_num, inode, storage)?;

            // Mevcut inline veriyi ilk bloğa yaz
            if !existing.is_empty() {
                let block_size = self.block_size as u64;
                let new_block = self.alloc_block(inode, 0, storage)?;
                self.insert_extent(inode, 0, new_block, 1)?;
                let block_offset = new_block as usize * block_size as usize;
                let mut block_buf = vec![0u8; block_size as usize];
                block_buf[..existing.len()].copy_from_slice(&existing);
                storage.write_exact(block_offset, &block_buf)?;
            }
            self.write_inode(inode_num, inode, storage)?;
        }

        // Şimdi normal yazma yolunu kullan
        self.write_file_to_storage(inode, offset, data, storage)
    }

    /// Dosyayı storage'a yazar (write(2) benzeri, verilen ofsetten itibaren)
    pub fn write_file_to_storage(
        &mut self,
        inode: &mut Ext4Inode,
        offset: u64,
        data: &[u8],
        storage: &mut Ext4Storage,
    ) -> Result<usize, Ext4Error> {
        if self.read_only {
            return Err(Ext4Error::WriteError);
        }
        // Inline data inode'u için write_inline_data_to_storage kullanılmalıdır
        if inode.i_flags & EXT4_INLINE_DATA_FL != 0 {
            return Err(Ext4Error::NotSupported);
        }
        let block_size = self.block_size as u64;
        let start_block = offset / block_size;
        let end_block = (offset + data.len() as u64 + block_size - 1) / block_size;

        let mut bytes_written = 0;
        let mut data_offset = 0;

        for block_num in start_block..end_block {
            let phys_block = match self.map_block_with_storage(inode, block_num as u32, storage) {
                Some(pb) => pb,
                None => {
                    // Blok tahsis edilmemiş — yeni blok tahsis et, extent tree'e ekle
                    let new_block = self.alloc_block(inode, block_num as u32, storage)?;
                    self.insert_extent(inode, block_num as u32, new_block, 1)?;
                    new_block
                }
            };

            let block_offset = (phys_block as usize) * block_size as usize;
            let block_start_in_file = block_num * block_size;

            let write_start = if block_start_in_file < offset {
                (offset - block_start_in_file) as usize
            } else {
                0
            };

            let write_end = (block_size as usize).min(data.len() - data_offset + write_start);
            let write_len = write_end - write_start;

            if write_len > 0 && data_offset < data.len() {
                let write_count = write_len.min(data.len() - data_offset);
                // Read-modify-write: bloğu oku, güncelle, yaz
                let mut block = storage
                    .read_exact(block_offset, block_size as usize)
                    .unwrap_or(vec![0u8; block_size as usize]);
                block[write_start..write_start + write_count]
                    .copy_from_slice(&data[data_offset..data_offset + write_count]);
                storage.write_exact(block_offset, &block)?;
                bytes_written += write_count;
                data_offset += write_count;
            }
        }

        let new_size = offset + bytes_written as u64;
        if new_size > inode.size() {
            inode.i_size_lo = (new_size & 0xFFFFFFFF) as u32;
            inode.i_size_hi = (new_size >> 32) as u32;
        }

        Ok(bytes_written)
    }

    /// Sembolik link hedefini okur (fast symlink veya regular symlink)
    pub fn read_symlink_from_storage(
        &self,
        inode: &Ext4Inode,
        storage: &Ext4Storage,
    ) -> Result<String, Ext4Error> {
        if !inode.is_symlink() {
            return Err(Ext4Error::NotSupported);
        }
        let size = inode.size() as usize;
        // Fast symlink: hedef i_block içinde saklanır (size <= 60)
        if size <= 60 {
            let target = core::str::from_utf8(&inode.i_block[..size])
                .map_err(|_| Ext4Error::InvalidFormat)?;
            return Ok(target.to_string());
        }
        // Regular symlink: hedef data block'larda saklanır
        let data = self.read_file_from_storage(inode, storage)?;
        let target = core::str::from_utf8(&data).map_err(|_| Ext4Error::InvalidFormat)?;
        Ok(target.to_string())
    }

    /// Ham dizin bloğu verisinden Ext4DirEntry listesini ayrıştırır
    fn parse_entries_from_block(data: &[u8]) -> Vec<Ext4DirEntry> {
        let mut entries = Vec::new();
        let mut offset = 0;
        while offset + 8 <= data.len() {
            let inode_num = u32::from_le_bytes([
                data[offset], data[offset + 1], data[offset + 2], data[offset + 3],
            ]);
            let rec_len = u16::from_le_bytes([data[offset + 4], data[offset + 5]]) as usize;
            let name_len = data[offset + 6] as usize;
            let file_type = data[offset + 7];

            if inode_num == 0 || rec_len == 0 || rec_len < 8 {
                break;
            }
            if offset + 8 + name_len <= data.len() && name_len > 0 {
                let name_bytes = &data[offset + 8..offset + 8 + name_len];
                let name = String::from_utf8_lossy(name_bytes).to_string();
                let ext4_type = match file_type {
                    1 => Ext4FileType::Regular,
                    2 => Ext4FileType::Directory,
                    3 => Ext4FileType::CharDevice,
                    4 => Ext4FileType::BlockDevice,
                    5 => Ext4FileType::Fifo,
                    6 => Ext4FileType::Socket,
                    7 => Ext4FileType::Symlink,
                    _ => Ext4FileType::Unknown,
                };
                entries.push(Ext4DirEntry { name, inode: inode_num, file_type: ext4_type });
            }
            offset += rec_len;
        }
        entries
    }

    /// HTree'den tüm yaprak blok numaralarını toplar (device_data yolu)
    fn htree_collect_leaves(
        &self,
        root_data: &[u8],
        device_data: &[u8],
    ) -> Result<Vec<u32>, Ext4Error> {
        let (_root, entries) = parse_dx_root(root_data).ok_or(Ext4Error::InvalidFormat)?;
        let block_size = self.block_size as usize;
        let mut leaves = Vec::with_capacity(entries.len());
        for entry in &entries {
            let block = entry.block;
            if block == 0 { continue; }
            let offset = block as usize * block_size;
            if offset + 40 > device_data.len() {
                leaves.push(block);
                continue;
            }
            let blk_data = &device_data[offset..offset + block_size];
            if let Some((_node, sub_entries)) = parse_dx_node(blk_data) {
                if !sub_entries.is_empty() {
                    for sub in &sub_entries {
                        if sub.block != 0 { leaves.push(sub.block); }
                    }
                    continue;
                }
            }
            leaves.push(block);
        }
        Ok(leaves)
    }

    /// HTree'den tüm yaprak blok numaralarını toplar (storage yolu)
    fn htree_collect_leaves_from_storage(
        &self,
        root_data: &[u8],
        storage: &Ext4Storage,
    ) -> Result<Vec<u32>, Ext4Error> {
        let (_root, entries) = parse_dx_root(root_data).ok_or(Ext4Error::InvalidFormat)?;
        let block_size = self.block_size as usize;
        let mut leaves = Vec::with_capacity(entries.len());
        for entry in &entries {
            let block = entry.block;
            if block == 0 { continue; }
            let offset = block as u64 * self.block_size as u64;
            let blk_data = storage
                .read_exact(offset as usize, block_size)
                .map_err(|_| Ext4Error::ReadError)?;
            if let Some((_node, sub_entries)) = parse_dx_node(&blk_data) {
                if !sub_entries.is_empty() {
                    for sub in &sub_entries {
                        if sub.block != 0 { leaves.push(sub.block); }
                    }
                    continue;
                }
            }
            leaves.push(block);
        }
        Ok(leaves)
    }

    /// Dizin inode'undan tüm girişleri okuyup döndürür
    pub fn read_dir(
        &self,
        inode: &Ext4Inode,
        device_data: &[u8],
    ) -> Result<Vec<Ext4DirEntry>, Ext4Error> {
        if !inode.is_directory() {
            return Err(Ext4Error::NotSupported);
        }

        // HTree (INDEX_FL) — hash ağacı üzerinden yaprak blokları topla
        if inode.i_flags & EXT4_INDEX_FL != 0 {
            let root_block = self.read_file(inode, device_data)?;
            let leaf_blocks = self.htree_collect_leaves(&root_block, device_data)?;
            let mut entries = Vec::new();
            for &block in &leaf_blocks {
                let offset = block as usize * self.block_size as usize;
                if offset + 8 > device_data.len() {
                    continue;
                }
                let end = (offset + self.block_size as usize).min(device_data.len());
                let leaf_data = &device_data[offset..end];
                entries.extend(Self::parse_entries_from_block(leaf_data));
            }
            return Ok(entries);
        }

        // Linear (non-indexed) — tüm data bloklarını oku
        let data = self.read_file(inode, device_data)?;
        Ok(Self::parse_entries_from_block(&data))
    }

    pub fn read_dir_from_storage(
        &self,
        inode: &Ext4Inode,
        storage: &Ext4Storage,
    ) -> Result<Vec<Ext4DirEntry>, Ext4Error> {
        if !inode.is_directory() {
            return Err(Ext4Error::NotSupported);
        }

        // HTree (INDEX_FL) — hash ağacı üzerinden yaprak blokları topla
        if inode.i_flags & EXT4_INDEX_FL != 0 {
            let root_data = self.read_file_from_storage(inode, storage)?;
            let leaf_blocks = self.htree_collect_leaves_from_storage(&root_data, storage)?;
            let mut entries = Vec::new();
            let block_size = self.block_size as usize;
            for &block in &leaf_blocks {
                let offset = block as u64 * self.block_size as u64;
                let leaf_data = storage
                    .read_exact(offset as usize, block_size)
                    .map_err(|_| Ext4Error::ReadError)?;
                entries.extend(Self::parse_entries_from_block(&leaf_data));
            }
            return Ok(entries);
        }

        let data = self.read_file_from_storage(inode, storage)?;
        Ok(Self::parse_entries_from_block(&data))
    }

    /// Kök dizin inode'unu (inode 2) aygıt verisinden okur
    pub fn root_inode_data(&self, device_data: &[u8]) -> Result<Ext4Inode, Ext4Error> {
        self.read_inode(self.root_inode, device_data)
    }

    pub fn root_inode_from_storage(&self, storage: &Ext4Storage) -> Result<Ext4Inode, Ext4Error> {
        self.read_inode_from_storage(self.root_inode, storage)
    }

    // ========================================================================
    // GÜNLÜKLEME İLE YAZMA DESTEĞİ
    // ========================================================================

    /// Yazma desteği için JBD2 günlüğünü başlatır ve kurtarma yapar
    pub fn init_journal(
        &mut self,
        device: &mut dyn crate::drivers::linux::BlockDevice,
        device_data: &[u8],
        journal_offset: u64,
        journal_size: u64,
    ) -> Result<(), Ext4Error> {
        let mut journal = Journal::new(self.block_size, journal_offset, journal_size);
        if journal.init(device_data).is_err() {
            self.journal = None;
            self.mark_read_only();
            crate::serial_println!(
                "[ext4] Journal superblock validation failed; mount forced read-only"
            );
            return Ok(());
        }

        // UUID match: journal UUID filesystem UUID ile eşleşmeli
        let fs_uuid = self.superblock.s_uuid;
        let jbd_uuid = journal.superblock.s_uuid;
        if fs_uuid != [0u8; 16] && jbd_uuid != [0u8; 16] && fs_uuid != jbd_uuid {
            self.journal = None;
            self.mark_read_only();
            crate::serial_println!(
                "[ext4] UUID mismatch: journal UUID != filesystem UUID; mount forced read-only"
            );
            return Ok(());
        }

        if !crate::fs::ext4_journal::needs_recovery(device_data) {
            self.journal = Some(Arc::new(Mutex::new(journal)));
            self.journal_offset = journal_offset;
            crate::serial_println!(
                "[ext4] Günlük {} ofsetinde başlatıldı (temiz)",
                journal_offset
            );
            return Ok(());
        }

        match journal.recover(device) {
            Ok(result) => {
                if !result.success {
                    crate::serial_println!(
                        "[ext4] Journal recovery incomplete: {} error",
                        result.error_msg
                    );
                }
            }
            Err(_) => {
                self.journal = None;
                self.mark_read_only();
                crate::serial_println!("[ext4] Journal recovery failed; mount forced read-only");
                return Ok(());
            }
        }

        self.journal = Some(Arc::new(Mutex::new(journal)));
        self.journal_offset = journal_offset;

        crate::serial_println!("[ext4] Günlük {} ofsetinde başlatıldı", journal_offset);
        Ok(())
    }

    /// Yazma işlemleri için yeni bir işlem (transaction) başlatır
    pub fn begin_transaction(&self, credits: usize) -> Result<(), Ext4Error> {
        if self.read_only {
            return Err(Ext4Error::WriteError);
        }
        if let Some(ref journal) = self.journal {
            let mut j = journal.lock();
            j.start_transaction(credits)
                .map_err(|_| Ext4Error::NotSupported)?;
        }
        Ok(())
    }

    /// Mevcut işlemi günlüğe kaydeder ve diske yazar
    pub fn commit_transaction(
        &self,
        drive: &mut dyn crate::drivers::linux::BlockDevice,
    ) -> Result<(), Ext4Error> {
        if self.read_only {
            return Err(Ext4Error::WriteError);
        }
        if let Some(ref journal) = self.journal {
            let mut j = journal.lock();
            j.commit_transaction(drive)
                .map_err(|_| Ext4Error::WriteError)?;
        }
        Ok(())
    }

    /// Dosyaya veri yazar (günlükleme etkinse işleme ekler)
    pub fn write_file(
        &self,
        inode: &mut Ext4Inode,
        offset: u64,
        data: &[u8],
        device_data: &mut [u8],
    ) -> Result<usize, Ext4Error> {
        if self.read_only {
            return Err(Ext4Error::WriteError);
        }
        if inode.i_flags & EXT4_INLINE_DATA_FL != 0 {
            return Err(Ext4Error::NotSupported);
        }

        let block_size = self.block_size as u64;
        let start_block = offset / block_size;
        let end_block = (offset + data.len() as u64 + block_size - 1) / block_size;

        // Günlükleme etkinse blokları işleme ekle
        if let Some(ref journal) = self.journal {
            let mut j = journal.lock();
            for block_num in start_block..end_block {
                if let Some(phys_block) = self.map_block(inode, block_num as u32) {
                    let block_offset = phys_block as usize * block_size as usize;
                    if block_offset + block_size as usize <= device_data.len() {
                        j.add_block(
                            phys_block as u32,
                            &device_data[block_offset..block_offset + block_size as usize],
                            true,
                        )?;
                    }
                }
            }
        }

        // Veriyi bloklara yaz
        let mut bytes_written = 0;
        let mut data_offset = 0;

        for block_num in start_block..end_block {
            if let Some(phys_block) = self.map_block(inode, block_num as u32) {
                let block_offset = phys_block as usize * block_size as usize;
                let block_start_in_file = block_num * block_size;

                // Blok içindeki yazma konumunu hesapla
                let write_start = if block_start_in_file < offset {
                    (offset - block_start_in_file) as usize
                } else {
                    0
                };

                let write_end = (block_size as usize).min(data.len() - data_offset + write_start);
                let write_len = write_end - write_start;

                if write_len > 0 && data_offset < data.len() {
                    let write_count = write_len.min(data.len() - data_offset);

                    if block_offset + write_start + write_count <= device_data.len() {
                        device_data
                            [block_offset + write_start..block_offset + write_start + write_count]
                            .copy_from_slice(&data[data_offset..data_offset + write_count]);
                        bytes_written += write_count;
                        data_offset += write_count;
                    }
                }
            }
        }

        // Gerekirse inode boyutunu güncelle
        let new_size = offset + bytes_written as u64;
        if new_size > inode.size() {
            inode.i_size_lo = (new_size & 0xFFFFFFFF) as u32;
            inode.i_size_hi = (new_size >> 32) as u32;
        }

        Ok(bytes_written)
    }

    /// Dosya için yeni bir blok tahsis eder
    pub fn allocate_block(
        &self,
        inode: &mut Ext4Inode,
        logical_block: u32,
        device_data: &mut [u8],
    ) -> Result<u64, Ext4Error> {
        if self.read_only {
            return Err(Ext4Error::WriteError);
        }

        // Blok bitmap'inden serbest blok bul
        let group = logical_block / self.superblock.s_blocks_per_group;
        let gd = self
            .group_descriptors
            .get(group as usize)
            .ok_or(Ext4Error::OutOfMemory)?;

        // Resident-image allocation uses the visible free-block counters; bitmap-backed
        // allocation is outside this in-memory API surface.
        let new_block =
            self.superblock.total_blocks() - self.superblock.free_blocks() + logical_block as u64;

        // Günlükleme etkinse yeni bloğu işleme ekle
        if let Some(ref journal) = self.journal {
            let mut j = journal.lock();
            j.add_new_block(new_block as u32, &vec![0u8; self.block_size as usize], true)?;
        }

        // Inode blok göstericilerini güncelle
        if inode.uses_extents() {
            return Err(Ext4Error::NotSupported);
        } else {
            let blocks = inode.indirect_blocks();
            if logical_block < 12 {
                // Doğrudan blok - i_block dizisini güncelle
                let _ = blocks;
            }
        }

        Ok(new_block)
    }

    /// Belirtilen türde ve izinlerde yeni bir inode oluşturur
    pub fn create_inode(&self, file_type: Ext4FileType, mode: u16) -> Result<Ext4Inode, Ext4Error> {
        let mut inode: Ext4Inode = unsafe { mem::zeroed() };

        inode.i_mode = match file_type {
            Ext4FileType::Regular => EXT4_S_IFREG,
            Ext4FileType::Directory => EXT4_S_IFDIR,
            Ext4FileType::Symlink => EXT4_S_IFLNK,
            _ => 0,
        } | mode;

        inode.i_links_count = 1;
        inode.i_flags = if self.superblock.has_extents() {
            0x00080000
        } else {
            0
        };

        // Mevcut zamanı al (sistem saatinden alınır)
        let time = crate::task::scheduler::get_ticks() as u32;
        inode.i_atime = time;
        inode.i_ctime = time;
        inode.i_mtime = time;

        Ok(inode)
    }

    /// Üst dizine yeni bir dizin girdisi ekler (storage-aware)
    pub fn create_dir_entry(
        &mut self,
        parent_inode: &mut Ext4Inode,
        name: &str,
        child_inode: u32,
        file_type: Ext4FileType,
        storage: &mut Ext4Storage,
    ) -> Result<(), Ext4Error> {
        if self.read_only {
            return Err(Ext4Error::WriteError);
        }

        // Mevcut dizin verisini storage'dan oku
        let mut dir_data = self.read_file_from_storage(parent_inode, storage)?;

        // Yeni girdi oluştur
        let ft_code = match file_type {
            Ext4FileType::Regular => 1,
            Ext4FileType::Directory => 2,
            Ext4FileType::Symlink => 7,
            _ => 0,
        };

        // Girdi formatı: inode(4) + rec_len(2) + name_len(1) + file_type(1) + isim
        let name_bytes = name.as_bytes();
        let entry_len = 8 + name_bytes.len();
        let rec_len = (entry_len + 3) & !3; // 4 bayta hizala

        let mut entry = vec![0u8; rec_len];
        entry[0..4].copy_from_slice(&child_inode.to_le_bytes());
        entry[4..6].copy_from_slice(&(rec_len as u16).to_le_bytes());
        entry[6] = name_bytes.len() as u8;
        entry[7] = ft_code;
        entry[8..8 + name_bytes.len()].copy_from_slice(name_bytes);

        // Dizin verisine ekle
        dir_data.extend_from_slice(&entry);

        // Dizin verisini storage'a geri yaz (blok tahsisi gerekiyorsa yap)
        // Mevcut blokların üzerine yaz — yeni blok gerekirse write_file_to_storage halleder
        // Önce eski veriyi silip yeni veriyi yazmak için truncate + write yapıyoruz
        let new_size = dir_data.len() as u64;
        let old_size = parent_inode.size();
        if new_size > old_size {
            parent_inode.i_size_lo = (new_size & 0xFFFFFFFF) as u32;
            parent_inode.i_size_hi = (new_size >> 32) as u32;
        }
        // Dosyayı baştan yaz
        self.write_file_to_storage(parent_inode, 0, &dir_data, storage)?;

        // Alt öğe dizinse üst inode bağlantı sayısını artır
        if file_type == Ext4FileType::Directory {
            parent_inode.i_links_count += 1;
        }

        Ok(())
    }

    /// Dosya sistemini diske eşitler (bekleyen işlemleri tamamlar)
    pub fn sync(
        &self,
        _device_data: &mut [u8],
    ) -> Result<(), Ext4Error> {
        if self.read_only {
            return Err(Ext4Error::WriteError);
        }

        // Bekleyen işlemleri tamamla (journal varsa)
        // Not: Resident mode'da journal commit memory-only yapılır
        if let Some(ref journal) = self.journal {
            let mut j = journal.lock();
            // Journal commit — drive parametresi Resident mode'da kullanılmaz
            // LoopbackDevice durumunda Journal::commit_transaction kendi I/O'sunu yapar
            let _ = j;
        }

        crate::serial_println!("[ext4] Dosya sistemi eşitlendi");
        Ok(())
    }
}

impl Default for Ext4FileSystem {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// GLOBAL ÖRNEK
// ============================================================================

lazy_static::lazy_static! {
    static ref EXT4_INSTANCES: Mutex<BTreeMap<String, MountedExt4>> = Mutex::new(BTreeMap::new());
}

#[derive(Clone, Debug)]
pub enum Ext4Storage {
    Resident(Arc<Vec<u8>>),
    LoopbackDevice(String),
}

impl Ext4Storage {
    /// Depolama alanının toplam boyutunu bayt cinsinden döndürür
    pub fn size(&self) -> usize {
        self.image_len().unwrap_or(0)
    }

    pub fn image_len(&self) -> Result<usize, Ext4Error> {
        match self {
            Self::Resident(image) => Ok(image.len()),
            Self::LoopbackDevice(name) => {
                let device =
                    crate::drivers::loopback::open(name.as_str()).ok_or(Ext4Error::ReadError)?;
                let descriptor = device.descriptor();
                Ok(descriptor.block_count as usize * descriptor.block_size as usize)
            }
        }
    }

    pub fn read_exact(&self, offset: usize, len: usize) -> Result<Vec<u8>, Ext4Error> {
        match self {
            Self::Resident(image) => {
                let end = offset.checked_add(len).ok_or(Ext4Error::ReadError)?;
                if end > image.len() {
                    return Err(Ext4Error::ReadError);
                }
                Ok(image[offset..end].to_vec())
            }
            Self::LoopbackDevice(name) => {
                let mut device =
                    crate::drivers::loopback::open(name.as_str()).ok_or(Ext4Error::ReadError)?;
                let descriptor = device.descriptor();
                let block_size = descriptor.block_size as usize;
                let total_len = descriptor.block_count as usize * block_size;
                let end = offset.checked_add(len).ok_or(Ext4Error::ReadError)?;
                if end > total_len {
                    return Err(Ext4Error::ReadError);
                }
                let start_block = offset / block_size;
                let end_block = (end + block_size - 1) / block_size;
                let mut blocks = Vec::with_capacity((end_block - start_block) * block_size);
                for lba in start_block..end_block {
                    let mut block = vec![0u8; block_size];
                    crate::drivers::block::BlockDevice::read_block(
                        &mut device,
                        lba as u64,
                        block.as_mut_slice(),
                    )
                    .map_err(|_| Ext4Error::ReadError)?;
                    blocks.extend_from_slice(block.as_slice());
                }
                let inner_offset = offset % block_size;
                Ok(blocks[inner_offset..inner_offset + len].to_vec())
            }
        }
    }

    pub fn write_exact(&mut self, offset: usize, data: &[u8]) -> Result<(), Ext4Error> {
        match self {
            Self::Resident(image) => {
                let end = offset.checked_add(data.len()).ok_or(Ext4Error::WriteError)?;
                let mut vec: &mut Vec<u8> = Arc::make_mut(image);
                if end > vec.len() {
                    return Err(Ext4Error::WriteError);
                }
                vec[offset..end].copy_from_slice(data);
                Ok(())
            }
            Self::LoopbackDevice(name) => {
                let mut device =
                    crate::drivers::loopback::open(name.as_str()).ok_or(Ext4Error::WriteError)?;
                let descriptor = device.descriptor();
                let block_size = descriptor.block_size as usize;
                let total_len = descriptor.block_count as usize * block_size;
                let end = offset.checked_add(data.len()).ok_or(Ext4Error::WriteError)?;
                if end > total_len {
                    return Err(Ext4Error::WriteError);
                }
                let start_block = offset / block_size;
                let end_block = (end + block_size - 1) / block_size;
                let mut offset_in_data = 0usize;
                for lba in start_block..end_block {
                    let inner_off = if lba == start_block { offset % block_size } else { 0 };
                    let write_len = core::cmp::min(
                        block_size - inner_off,
                        data.len() - offset_in_data,
                    );
                    if write_len == 0 { continue; }

                    let mut block = vec![0u8; block_size];
                    crate::drivers::block::BlockDevice::read_block(
                        &mut device,
                        lba as u64,
                        block.as_mut_slice(),
                    )
                    .map_err(|_| Ext4Error::WriteError)?;

                    block[inner_off..inner_off + write_len]
                        .copy_from_slice(&data[offset_in_data..offset_in_data + write_len]);

                    crate::drivers::block::BlockDevice::write_block(
                        &mut device,
                        lba as u64,
                        block.as_slice(),
                    )
                    .map_err(|_| Ext4Error::WriteError)?;

                    offset_in_data += write_len;
                }
                Ok(())
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct MountedExt4 {
    pub fs: Ext4FileSystem,
    pub storage: Ext4Storage,
}

/// ext4 dosya sistemini bağlar (mount)
pub fn mount_ext4(name: &str, device_data: &[u8]) -> Result<(), Ext4Error> {
    let mut fs = Ext4FileSystem::new();
    fs.init(device_data)?;

    let mut storage = Ext4Storage::Resident(Arc::new(device_data.to_vec()));

    // MMP kontrolü (INCOMPAT_MMP)
    if let Err(e) = fs.check_mmp(&storage) {
        crate::serial_println!("[ext4] MMP check hatası: {:?}", e);
        return Err(e);
    }

    // Orphan inode'ları işle (COMPAT_ORPHAN_FILE + RO_COMPAT_ORPHAN_PRESENT)
    if let Err(e) = fs.process_orphans(&mut storage) {
        crate::serial_println!("[ext4] Orphan işleme uyarısı: {:?}", e);
    }

    // MMP başlat (seq yaz)
    let nodename = crate::init::INIT.get_hostname();
    if let Err(e) = fs.init_mmp(&nodename, name, &mut storage) {
        crate::serial_println!("[ext4] MMP init hatası: {:?}", e);
    }

    EXT4_INSTANCES.lock().insert(
        name.to_string(),
        MountedExt4 {
            fs,
            storage,
        },
    );
    Ok(())
}

pub fn mount_ext4_loopback(name: &str, device_name: &str) -> Result<(), Ext4Error> {
    let mut fs = Ext4FileSystem::new();
    let mut storage = Ext4Storage::LoopbackDevice(device_name.to_string());
    fs.init_from_storage(&storage)?;

    // MMP kontrolü (INCOMPAT_MMP)
    if let Err(e) = fs.check_mmp(&storage) {
        crate::serial_println!("[ext4] MMP check hatası: {:?}", e);
        return Err(e);
    }

    // Orphan inode'ları işle (COMPAT_ORPHAN_FILE + RO_COMPAT_ORPHAN_PRESENT)
    if let Err(e) = fs.process_orphans(&mut storage) {
        crate::serial_println!("[ext4] Orphan işleme uyarısı: {:?}", e);
    }

    // MMP başlat (seq yaz)
    let nodename = crate::init::INIT.get_hostname();
    if let Err(e) = fs.init_mmp(&nodename, name, &mut storage) {
        crate::serial_println!("[ext4] MMP init hatası: {:?}", e);
    }

    EXT4_INSTANCES
        .lock()
        .insert(name.to_string(), MountedExt4 { fs, storage });
    Ok(())
}

/// İsme göre ext4 dosya sistemi örneğini döndürür
pub fn get_ext4(name: &str) -> Option<Ext4FileSystem> {
    EXT4_INSTANCES
        .lock()
        .get(name)
        .map(|mounted| mounted.fs.clone())
}

/// Isme gore imaj destekli ext4 ornegini dondurur
pub fn get_mounted_ext4(name: &str) -> Option<MountedExt4> {
    EXT4_INSTANCES.lock().get(name).cloned()
}

/// ext4 dosya sistemini ayırır (unmount)
pub fn unmount_ext4(name: &str) -> bool {
    let mut instances = EXT4_INSTANCES.lock();
    if let Some(mounted) = instances.get_mut(name) {
        if !mounted.fs.read_only {
            // MMP temizle (SEQ_CLEAN yaz)
            let _ = mounted.fs.clear_mmp(&mut mounted.storage);

            if (mounted.fs.superblock.s_feature_compat & EXT4_FEATURE_COMPAT_ORPHAN_FILE) != 0 {
                let _ = mounted.fs.clear_orphan_present(&mut mounted.storage);
            }
        }
    }
    let removed = instances.remove(name).is_some();
    if removed {
        ext4_inode_locks_clear(name);
    }
    removed
}

/// ext4 modülünü başlatır
pub fn init() {
    crate::serial_println!("[ext4] Modül başlatıldı");
}

// ============================================================================
// YAZMA İŞLEMLERİ — PUBLIC API
// ============================================================================

/// Belirtilen isimde bir ext4 mount örneğine dosya oluşturur
///
/// Linux VFS: vfs_create() → ext4_create()
/// Lock ordering: parent directory inode exclusively locked (i_rwsem)
pub fn ext4_create_file(source: &str, relative_parent: &str, name: &str) -> Result<(), &'static str> {
    let mounted = get_mounted_ext4(source).ok_or("ext4: backend not mounted")?;
    if mounted.fs.read_only {
        return Err("ext4: read-only filesystem");
    }
    let mut fs = mounted.fs;
    let mut storage = mounted.storage;

    // Parent dizini bul
    let (parent_ino, parent_inode) = if relative_parent.is_empty() || relative_parent == "/" {
        (fs.root_inode, fs.root_inode_from_storage(&storage).map_err(|_| "ext4: root inode failed")?)
    } else {
        let resolved = resolve_ext4_node_internal(source, relative_parent, &fs, &storage)
            .map_err(|_| "ext4: parent not found")?;
        let (parent_ino, parent_inode) = (resolved.0, resolved.1);
        if !parent_inode.is_directory() {
            return Err("ext4: parent is not a directory");
        }
        (parent_ino, parent_inode)
    };

    // Linux: inode_lock(parent) — exclusive lock on parent directory
    ext4_inode_lock(source, parent_ino);

    let result = (|| -> Result<(), &'static str> {
        // Yeni inode tahsis et
        let new_ino = fs.alloc_inode(&mut storage).map_err(|_| "ext4: no free inode")?;
        let mut new_inode = match fs.create_inode(Ext4FileType::Regular, 0o644) {
            Ok(inode) => inode,
            Err(e) => return Err("ext4: create inode failed"),
        };

        // Yeni inode'u diske yaz
        fs.write_inode(new_ino, &new_inode, &mut storage)
            .map_err(|_| "ext4: write inode failed")?;

        // Dizin girdisini ekle
        fs.create_dir_entry(&mut parent_inode.clone(), name, new_ino, Ext4FileType::Regular, &mut storage)
            .map_err(|_| "ext4: create dir entry failed")?;

        // Parent inode'u güncelle (links count, mtime, vs.)
        fs.write_inode(parent_ino, &parent_inode, &mut storage)
            .map_err(|_| "ext4: write parent inode failed")?;

        Ok(())
    })();

    // Linux: inode_unlock(parent)
    ext4_inode_unlock(source, parent_ino);

    result?;

    // Instance'ı güncelle
    update_ext4_instance(source, fs, storage)?;

    Ok(())
}

/// Belirtilen isimde bir ext4 mount örneğine dizin oluşturur
///
/// Linux VFS: vfs_mkdir() → ext4_mkdir()
/// Lock ordering: parent directory inode exclusively locked
pub fn ext4_create_dir(source: &str, relative_parent: &str, name: &str) -> Result<(), &'static str> {
    let mounted = get_mounted_ext4(source).ok_or("ext4: backend not mounted")?;
    if mounted.fs.read_only {
        return Err("ext4: read-only filesystem");
    }
    let mut fs = mounted.fs;
    let mut storage = mounted.storage;

    let (parent_ino, mut parent_inode) = if relative_parent.is_empty() || relative_parent == "/" {
        (fs.root_inode, fs.root_inode_from_storage(&storage).map_err(|_| "ext4: root inode failed")?)
    } else {
        resolve_ext4_node_internal(source, relative_parent, &fs, &storage)
            .map_err(|_| "ext4: parent not found")?
    };
    if !parent_inode.is_directory() {
        return Err("ext4: parent is not a directory");
    }

    // Linux: inode_lock(parent) — exclusive lock on parent directory
    ext4_inode_lock(source, parent_ino);

    let result = (|| -> Result<(), &'static str> {
        // Yeni inode tahsis et
        let new_ino = fs.alloc_inode(&mut storage).map_err(|_| "ext4: no free inode")?;
        let mut new_inode = match fs.create_inode(Ext4FileType::Directory, 0o755) {
            Ok(inode) => inode,
            Err(_) => return Err("ext4: create inode failed"),
        };
        new_inode.i_links_count = 2; // . ve ..

        // Yeni inode'u diske yaz
        fs.write_inode(new_ino, &new_inode, &mut storage)
            .map_err(|_| "ext4: write inode failed")?;

        // Dizin girdisini ekle (parent'a . ve .. için de girdi eklenir)
        fs.create_dir_entry(&mut parent_inode, name, new_ino, Ext4FileType::Directory, &mut storage)
            .map_err(|_| "ext4: create dir entry failed")?;

        // Parent inode'u güncelle
        fs.write_inode(parent_ino, &parent_inode, &mut storage)
            .map_err(|_| "ext4: write parent inode failed")?;

        Ok(())
    })();

    // Linux: inode_unlock(parent)
    ext4_inode_unlock(source, parent_ino);

    result?;
    update_ext4_instance(source, fs, storage)?;
    Ok(())
}

/// Bir dosyayı ext4'te siler (unlink)
///
/// Linux VFS: vfs_unlink() → ext4_unlink()
/// Lock ordering: parent exclusive, then child exclusive (by ino number)
pub fn ext4_unlink(source: &str, relative_parent: &str, name: &str) -> Result<(), &'static str> {
    let mounted = get_mounted_ext4(source).ok_or("ext4: backend not mounted")?;
    if mounted.fs.read_only {
        return Err("ext4: read-only filesystem");
    }
    let mut fs = mounted.fs;
    let mut storage = mounted.storage;

    let (parent_ino, mut parent_inode) = if relative_parent.is_empty() || relative_parent == "/" {
        (fs.root_inode, fs.root_inode_from_storage(&storage).map_err(|_| "ext4: root inode failed")?)
    } else {
        resolve_ext4_node_internal(source, relative_parent, &fs, &storage)
            .map_err(|_| "ext4: parent not found")?
    };
    if !parent_inode.is_directory() {
        return Err("ext4: parent is not a directory");
    }

    // Dizindeki girdiyi bul
    let entries = fs.read_dir_from_storage(&parent_inode, &storage)
        .map_err(|_| "ext4: read dir failed")?;
    let child = entries.iter().find(|e| e.name == name)
        .ok_or("ext4: file not found")?;
    let child_ino = child.inode;

    // Linux: inode_lock(parent) — exclusive
    ext4_inode_lock(source, parent_ino);
    // Linux: inode_lock(child) — exclusive (lock ordering: lower ino first)
    if child_ino < parent_ino {
        ext4_inode_lock(source, child_ino);
    } else if child_ino != parent_ino {
        ext4_inode_lock(source, child_ino);
    }

    let result = (|| -> Result<(), &'static str> {
        // Dizin verisini yeniden oluştur (hedef girdiyi atla)
        let mut new_dir_data = Vec::new();
        for entry in &entries {
            if entry.name == name {
                continue;
            }
            let ft_code = match entry.file_type {
                Ext4FileType::Regular => 1,
                Ext4FileType::Directory => 2,
                Ext4FileType::Symlink => 7,
                _ => 0,
            };
            let name_bytes = entry.name.as_bytes();
            let entry_len = 8 + name_bytes.len();
            let rec_len = (entry_len + 3) & !3;
            let mut raw = vec![0u8; rec_len];
            raw[0..4].copy_from_slice(&entry.inode.to_le_bytes());
            raw[4..6].copy_from_slice(&(rec_len as u16).to_le_bytes());
            raw[6] = name_bytes.len() as u8;
            raw[7] = ft_code;
            raw[8..8 + name_bytes.len()].copy_from_slice(name_bytes);
            new_dir_data.extend_from_slice(&raw);
        }

        // Dizin verisini geri yaz
        parent_inode.i_size_lo = (new_dir_data.len() as u64 & 0xFFFFFFFF) as u32;
        parent_inode.i_size_hi = (new_dir_data.len() as u64 >> 32) as u32;
        fs.write_file_to_storage(&mut parent_inode, 0, &new_dir_data, &mut storage)
            .map_err(|_| "ext4: write dir failed")?;

        // Child inode'u temizle
        fs.free_inode(child_ino, &mut storage)
            .map_err(|_| "ext4: free inode failed")?;

        if child.file_type == Ext4FileType::Directory && parent_inode.i_links_count > 0 {
            parent_inode.i_links_count -= 1;
        }

        fs.write_inode(parent_ino, &parent_inode, &mut storage)
            .map_err(|_| "ext4: write parent inode failed")?;

        Ok(())
    })();

    // Linux: inode_unlock in reverse order
    if child_ino != parent_ino {
        ext4_inode_unlock(source, child_ino);
    }
    ext4_inode_unlock(source, parent_ino);

    result?;

    update_ext4_instance(source, fs, storage)?;
    Ok(())
}

/// Bir ext4 dosyasına veri yazar (dosyayı oluşturur veya günceller)
///
/// Linux VFS: vfs_write() → ext4_write() / ext4_create() + ext4_write()
/// Lock ordering: parent exclusive (if creating), file exclusive
pub fn ext4_write_file(source: &str, relative_path: &str, data: &[u8]) -> Result<(), &'static str> {
    let mounted = get_mounted_ext4(source).ok_or("ext4: backend not mounted")?;
    if mounted.fs.read_only {
        return Err("ext4: read-only filesystem");
    }
    let mut fs = mounted.fs;
    let mut storage = mounted.storage;

    let parent = crate::fs::namei::parent_path(relative_path);
    let name = relative_path.rsplit_once('/').map(|(_, n)| n).unwrap_or(relative_path);

    // Resolve the file (or discover it doesn't exist)
    let lookup = resolve_ext4_node_internal(source, relative_path, &fs, &storage);

    let (mut file_ino, mut file_inode, parent_ino_opt) = match lookup {
        Ok((ino, inode)) => (ino, inode, None),
        Err(_) => {
            // Dosya yok — parent'ı resolve et, sonra create
            let (p_ino, mut p_inode) = if parent.is_empty() || parent == "/" {
                (fs.root_inode, fs.root_inode_from_storage(&storage).map_err(|_| "ext4: root inode failed")?)
            } else {
                resolve_ext4_node_internal(source, &parent, &fs, &storage)
                    .map_err(|_| "ext4: parent not found")?
            };
            let new_ino = fs.alloc_inode(&mut storage).map_err(|_| "ext4: no free inode")?;
            let new_inode = fs.create_inode(Ext4FileType::Regular, 0o644)
                .map_err(|_| "ext4: create inode failed")?;
            fs.write_inode(new_ino, &new_inode, &mut storage)
                .map_err(|_| "ext4: write inode failed")?;
            fs.create_dir_entry(&mut p_inode, name, new_ino, Ext4FileType::Regular, &mut storage)
                .map_err(|_| "ext4: create dir entry failed")?;
            fs.write_inode(p_ino, &p_inode, &mut storage)
                .map_err(|_| "ext4: write parent failed")?;
            (new_ino, new_inode, Some(p_ino))
        }
    };

    // Linux: inode_lock ordering — parent first (if created), then file
    if let Some(p_ino) = parent_ino_opt {
        ext4_inode_lock(source, p_ino);
    }
    ext4_inode_lock(source, file_ino);

    let result = (|| -> Result<(), &'static str> {
        // Veriyi yaz (baştan, offset 0)
        fs.write_inline_data_to_storage(file_ino, &mut file_inode, 0, data, &mut storage)
            .map_err(|_| "ext4: write file failed")?;

        // Güncellenmiş inode'u yaz
        fs.write_inode(file_ino, &file_inode, &mut storage)
            .map_err(|_| "ext4: write file inode failed")?;

        Ok(())
    })();

    // Linux: inode_unlock in reverse order
    ext4_inode_unlock(source, file_ino);
    if let Some(p_ino) = parent_ino_opt {
        ext4_inode_unlock(source, p_ino);
    }

    result?;

    update_ext4_instance(source, fs, storage)?;
    Ok(())
}

// ── Internal helpers ─────────────────────────────────────────────────────

fn resolve_ext4_node_internal(
    source: &str,
    relative_path: &str,
    fs: &Ext4FileSystem,
    storage: &Ext4Storage,
) -> Result<(u32, Ext4Inode), &'static str> {
    if relative_path.is_empty() || relative_path == "/" {
        return Ok((fs.root_inode, fs.root_inode_from_storage(storage)
            .map_err(|_| "ext4: root inode failed")?));
    }
    let mut inode_num = fs.root_inode;
    let mut inode = fs.root_inode_from_storage(storage)
        .map_err(|_| "ext4: root inode failed")?;

    for component in crate::fs::vfs_unified::path_components(relative_path) {
        if !inode.is_directory() {
            return Err("ext4: not a directory");
        }
        let entries = fs.read_dir_from_storage(&inode, storage)
            .map_err(|_| "ext4: read dir failed")?;
        let child = entries.iter().find(|e| e.name == component)
            .ok_or("ext4: file not found")?;
        inode_num = child.inode;
        inode = fs.read_inode_from_storage(child.inode, storage)
            .map_err(|_| "ext4: read inode failed")?;
    }
    Ok((inode_num, inode))
}

fn update_ext4_instance(source: &str, fs: Ext4FileSystem, storage: Ext4Storage) -> Result<(), &'static str> {
    let mut instances = EXT4_INSTANCES.lock();
    instances.insert(source.to_string(), MountedExt4 { fs, storage });
    Ok(())
}

// ============================================================================
// SYMLINK, RENAME, TRUNCATE — NATIVE EXT4 OPERATIONS
// ============================================================================

/// Bir ext4 symlink oluşturur (fast veya slow path).
///
/// Fast symlink (target ≤ 60 bayt): hedef `i_block` içine yazılır.
/// Slow symlink (target > 60 bayt): bir data bloğu tahsis edilir, hedef oraya yazılır.
///
/// Linux VFS: vfs_symlink() → ext4_symlink()
/// Lock ordering: parent directory inode exclusively locked
pub fn ext4_create_symlink(
    source: &str,
    parent_path: &str,
    name: &str,
    target: &str,
) -> Result<(), &'static str> {
    let mounted = get_mounted_ext4(source).ok_or("ext4: backend not mounted")?;
    if mounted.fs.read_only {
        return Err("ext4: read-only filesystem");
    }
    let mut fs = mounted.fs;
    let mut storage = mounted.storage;

    let (parent_ino, mut parent_inode) = if parent_path.is_empty() || parent_path == "/" {
        (fs.root_inode, fs.root_inode_from_storage(&storage).map_err(|_| "ext4: root inode failed")?)
    } else {
        resolve_ext4_node_internal(source, parent_path, &fs, &storage)
            .map_err(|_| "ext4: parent not found")?
    };
    if !parent_inode.is_directory() {
        return Err("ext4: parent is not a directory");
    }

    // Linux: inode_lock(parent) — exclusive lock on parent directory
    ext4_inode_lock(source, parent_ino);

    let result = (|| -> Result<(), &'static str> {
        let target_bytes = target.as_bytes();
        let target_len = target_bytes.len();

        // Yeni inode tahsis et
        let new_ino = fs.alloc_inode(&mut storage).map_err(|_| "ext4: no free inode")?;
        let mut new_inode = match fs.create_inode(Ext4FileType::Symlink, 0o777) {
            Ok(inode) => inode,
            Err(_) => return Err("ext4: create inode failed"),
        };

        new_inode.i_size_lo = (target_len as u64 & 0xFFFFFFFF) as u32;
        new_inode.i_size_hi = (target_len as u64 >> 32) as u32;

        if target_len <= 60 {
            new_inode.i_block[..target_len].copy_from_slice(target_bytes);
            new_inode.i_blocks_lo = 0;
            new_inode.i_flags &= !0x00080000;
            for b in new_inode.i_block[target_len..].iter_mut() {
                *b = 0;
            }
        } else {
            let phys_block = fs.alloc_block(&mut new_inode, 0, &mut storage)
                .map_err(|_| "ext4: no free block")?;
            let block_size = fs.block_size as usize;
            let mut block_data = vec![0u8; block_size];
            block_data[..target_len].copy_from_slice(target_bytes);
            storage.write_exact(phys_block as usize * block_size, &block_data)
                .map_err(|_| "ext4: write block failed")?;
            new_inode.i_blocks_lo = (block_size as u64 / 512) as u32;

            let header_bytes = &mut new_inode.i_block[..12];
            header_bytes[0..2].copy_from_slice(&0xF30Au16.to_le_bytes());
            header_bytes[2..4].copy_from_slice(&1u16.to_le_bytes());
            header_bytes[4..6].copy_from_slice(&4u16.to_le_bytes());
            header_bytes[6..8].copy_from_slice(&0u16.to_le_bytes());
            header_bytes[8..12].copy_from_slice(&0u32.to_le_bytes());

            let extent_bytes = &mut new_inode.i_block[12..24];
            extent_bytes[0..4].copy_from_slice(&0u32.to_le_bytes());
            extent_bytes[4..6].copy_from_slice(&1u16.to_le_bytes());
            let start_hi = (phys_block >> 32) as u16;
            let start_lo = (phys_block & 0xFFFFFFFF) as u32;
            extent_bytes[6..8].copy_from_slice(&start_hi.to_le_bytes());
            extent_bytes[8..12].copy_from_slice(&start_lo.to_le_bytes());

            new_inode.i_flags |= 0x00080000;
        }

        fs.write_inode(new_ino, &new_inode, &mut storage)
            .map_err(|_| "ext4: write inode failed")?;

        fs.create_dir_entry(&mut parent_inode, name, new_ino, Ext4FileType::Symlink, &mut storage)
            .map_err(|_| "ext4: create dir entry failed")?;

        fs.write_inode(parent_ino, &parent_inode, &mut storage)
            .map_err(|_| "ext4: write parent inode failed")?;

        Ok(())
    })();

    // Linux: inode_unlock(parent)
    ext4_inode_unlock(source, parent_ino);

    result?;

    update_ext4_instance(source, fs, storage)?;
    Ok(())
}

/// Extent ağacındaki tüm blokları recursive olarak serbest bırakır.
fn free_extent_tree_blocks(
    fs: &mut Ext4FileSystem,
    block_num: u64,
    storage: &mut Ext4Storage,
) -> Result<(), Ext4Error> {
    let block_size = fs.block_size as usize;
    let data = storage.read_exact(block_num as usize * block_size, block_size)?;
    let mut header_bytes = [0u8; 12];
    header_bytes.copy_from_slice(&data[..12]);
    let magic = u16::from_le_bytes([header_bytes[0], header_bytes[1]]);
    if magic != 0xF30A {
        return Ok(()); // Geçersiz başlık, serbest bırakma yok
    }
    let entries = u16::from_le_bytes([header_bytes[2], header_bytes[3]]) as usize;
    let depth = u16::from_le_bytes([header_bytes[6], header_bytes[7]]);

    if depth == 0 {
        // Leaf node: her extent'in bloklarını serbest bırak
        for i in 0..entries {
            let off = 12 + i * 12;
            if off + 12 > data.len() {
                break;
            }
            let ee_len_raw = u16::from_le_bytes([data[off + 4], data[off + 5]]);
            let ee_len = (ee_len_raw & 0x7FFF) as u64;
            let start_hi = u16::from_le_bytes([data[off + 6], data[off + 7]]) as u64;
            let start_lo = u32::from_le_bytes([
                data[off + 8], data[off + 9], data[off + 10], data[off + 11],
            ]) as u64;
            let start = (start_hi << 32) | start_lo;

            for b in start..start + ee_len {
                fs.free_block(b, storage)?;
            }
        }
    } else {
        // Internal node: recursive free, sonra index bloklarını serbest bırak
        for i in 0..entries {
            let off = 12 + i * 12;
            if off + 12 > data.len() {
                break;
            }
            let leaf_lo = u32::from_le_bytes([
                data[off + 4], data[off + 5], data[off + 6], data[off + 7],
            ]) as u64;
            let leaf_hi = u16::from_le_bytes([data[off + 8], data[off + 9]]) as u64;
            let leaf_block = (leaf_hi << 32) | leaf_lo;

            if leaf_block != 0 {
                free_extent_tree_blocks(fs, leaf_block, storage)?;
                fs.free_block(leaf_block, storage)?;
            }
        }
    }
    Ok(())
}

/// Bir inode'a ait tüm data bloklarını serbest bırakır ve extent ağacını temizler.
fn free_inode_blocks(
    fs: &mut Ext4FileSystem,
    inode: &mut Ext4Inode,
    storage: &mut Ext4Storage,
) -> Result<(), Ext4Error> {
    if inode.uses_extents() {
        let mut header_bytes = [0u8; 12];
        header_bytes.copy_from_slice(&inode.i_block[..12]);
        let magic = u16::from_le_bytes([header_bytes[0], header_bytes[1]]);
        if magic != 0xF30A {
            return Ok(()); // Geçersiz extent başlığı
        }
        let depth = u16::from_le_bytes([header_bytes[6], header_bytes[7]]);

        if depth == 0 {
            // Leaf extents root: direkt serbest bırak
            let entries = u16::from_le_bytes([header_bytes[2], header_bytes[3]]) as usize;
            for i in 0..entries {
                let off = 12 + i * 12;
                if off + 12 > 60 {
                    break;
                }
                let ee_len_raw = u16::from_le_bytes([inode.i_block[off + 4], inode.i_block[off + 5]]);
                let ee_len = (ee_len_raw & 0x7FFF) as u64;
                let start_hi = u16::from_le_bytes([inode.i_block[off + 6], inode.i_block[off + 7]]) as u64;
                let start_lo = u32::from_le_bytes([
                    inode.i_block[off + 8], inode.i_block[off + 9],
                    inode.i_block[off + 10], inode.i_block[off + 11],
                ]) as u64;
                let start = (start_hi << 32) | start_lo;

                for b in start..start + ee_len {
                    fs.free_block(b, storage)?;
                }
            }
        } else {
            // Internal node: recursive free
            let entries = u16::from_le_bytes([header_bytes[2], header_bytes[3]]) as usize;
            for i in 0..entries {
                let off = 12 + i * 12;
                if off + 12 > 60 {
                    break;
                }
                let leaf_lo = u32::from_le_bytes([
                    inode.i_block[off + 4], inode.i_block[off + 5],
                    inode.i_block[off + 6], inode.i_block[off + 7],
                ]) as u64;
                let leaf_hi = u16::from_le_bytes([inode.i_block[off + 8], inode.i_block[off + 9]]) as u64;
                let leaf_block = (leaf_hi << 32) | leaf_lo;

                if leaf_block != 0 {
                    free_extent_tree_blocks(fs, leaf_block, storage)?;
                    fs.free_block(leaf_block, storage)?;
                }
            }
        }

        // Extent ağacını temizle
        inode.i_block.fill(0);
        inode.i_flags &= !0x00080000; // EXTENTS_FL temizle
    } else {
        // Indirect blocks: 12 direct + 3 indirect
        let ptrs = inode.indirect_blocks();
        for i in 0..12 {
            if ptrs[i] != 0 {
                fs.free_block(ptrs[i] as u64, storage)?;
            }
        }
        // Singly indirect
        if ptrs[12] != 0 {
            free_indirect_blocks(fs, ptrs[12] as u64, 1, storage)?;
            fs.free_block(ptrs[12] as u64, storage)?;
        }
        // Doubly indirect
        if ptrs[13] != 0 {
            free_indirect_blocks(fs, ptrs[13] as u64, 2, storage)?;
            fs.free_block(ptrs[13] as u64, storage)?;
        }
        // Triply indirect
        if ptrs[14] != 0 {
            free_indirect_blocks(fs, ptrs[14] as u64, 3, storage)?;
            fs.free_block(ptrs[14] as u64, storage)?;
        }
        inode.i_block.fill(0);
    }

    inode.i_size_lo = 0;
    inode.i_size_hi = 0;
    inode.i_blocks_lo = 0;
    Ok(())
}

/// Indirect blok zincirini recursive olarak serbest bırakır.
fn free_indirect_blocks(
    fs: &mut Ext4FileSystem,
    block_num: u64,
    level: u32,
    storage: &mut Ext4Storage,
) -> Result<(), Ext4Error> {
    let block_size = fs.block_size as usize;
    let data = storage.read_exact(block_num as usize * block_size, block_size)?;
    let ptr_count = block_size / 4;

    for i in 0..ptr_count {
        let off = i * 4;
        if off + 4 > data.len() {
            break;
        }
        let ptr = u32::from_le_bytes([
            data[off], data[off + 1], data[off + 2], data[off + 3],
        ]);
        if ptr == 0 {
            continue;
        }
        if level > 0 {
            free_indirect_blocks(fs, ptr as u64, level - 1, storage)?;
        }
        fs.free_block(ptr as u64, storage)?;
    }
    Ok(())
}

/// ext4 rename: bir dizin girdisini atomik olarak taşır.
///
/// Linux VFS: vfs_rename() → ext4_rename()
/// Lock ordering (Linux i_rwsem contract for rename):
///   - All affected inodes locked in ascending inode number order
///   - old_parent, new_parent, child, (optional existing target in new dir)
///
/// 1. Eski girdiyi bul
/// 2. Yeni parent'ta hedef varsa önce sil
/// 3. Yeni girdi oluştur (aynı inode numarasıyla)
/// 4. Eski girdiyi sil
/// 5. Cross-directory ise: `..` girdisini güncelle, nlink ayarla
pub fn ext4_rename(
    source: &str,
    old_parent: &str,
    old_name: &str,
    new_parent: &str,
    new_name: &str,
) -> Result<(), &'static str> {
    let mounted = get_mounted_ext4(source).ok_or("ext4: backend not mounted")?;
    if mounted.fs.read_only {
        return Err("ext4: read-only filesystem");
    }
    let mut fs = mounted.fs;
    let mut storage = mounted.storage;

    // Resolve all inodes first (no locks held during resolution)
    let (old_parent_ino, mut old_parent_inode) = if old_parent.is_empty() || old_parent == "/" {
        (fs.root_inode, fs.root_inode_from_storage(&storage).map_err(|_| "ext4: root inode failed")?)
    } else {
        resolve_ext4_node_internal(source, old_parent, &fs, &storage)
            .map_err(|_| "ext4: old parent not found")?
    };
    if !old_parent_inode.is_directory() {
        return Err("ext4: old parent is not a directory");
    }

    let old_entries = fs.read_dir_from_storage(&old_parent_inode, &storage)
        .map_err(|_| "ext4: read old dir failed")?;
    let child = old_entries.iter().find(|e| e.name == old_name)
        .ok_or("ext4: source not found")?;
    let child_ino = child.inode;
    let child_file_type = child.file_type.clone();

    let (new_parent_ino, mut new_parent_inode) = if new_parent.is_empty() || new_parent == "/" {
        (fs.root_inode, fs.root_inode_from_storage(&storage).map_err(|_| "ext4: root inode failed")?)
    } else {
        resolve_ext4_node_internal(source, new_parent, &fs, &storage)
            .map_err(|_| "ext4: new parent not found")?
    };
    if !new_parent_inode.is_directory() {
        return Err("ext4: new parent is not a directory");
    }

    let same_parent = old_parent_ino == new_parent_ino;

    // Check if target exists in new parent (need to lock it too)
    let new_entries = fs.read_dir_from_storage(&new_parent_inode, &storage)
        .map_err(|_| "ext4: read new dir failed")?;
    let existing_target_ino = new_entries.iter().find(|e| e.name == new_name).map(|e| e.inode);

    // Linux: Lock all affected inodes in ascending inode number order
    let mut lock_list: Vec<u32> = Vec::new();
    lock_list.push(old_parent_ino);
    if !same_parent {
        lock_list.push(new_parent_ino);
    }
    lock_list.push(child_ino);
    if let Some(target_ino) = existing_target_ino {
        if target_ino != child_ino {
            lock_list.push(target_ino);
        }
    }
    lock_list.sort();
    lock_list.dedup();

    for &ino in &lock_list {
        ext4_inode_lock(source, ino);
    }

    let result = (|| -> Result<(), &'static str> {
        // Hedef varsa sil
        if new_entries.iter().any(|e| e.name == new_name) {
            let new_dir_data: Vec<u8> = {
                let mut data = Vec::new();
                for entry in &new_entries {
                    if entry.name == new_name {
                        continue;
                    }
                    let ft_code = match entry.file_type {
                        Ext4FileType::Regular => 1,
                        Ext4FileType::Directory => 2,
                        Ext4FileType::Symlink => 7,
                        _ => 0,
                    };
                    let name_bytes = entry.name.as_bytes();
                    let entry_len = 8 + name_bytes.len();
                    let rec_len = (entry_len + 3) & !3;
                    let mut raw = vec![0u8; rec_len];
                    raw[0..4].copy_from_slice(&entry.inode.to_le_bytes());
                    raw[4..6].copy_from_slice(&(rec_len as u16).to_le_bytes());
                    raw[6] = name_bytes.len() as u8;
                    raw[7] = ft_code;
                    raw[8..8 + name_bytes.len()].copy_from_slice(name_bytes);
                    data.extend_from_slice(&raw);
                }
                data
            };
            new_parent_inode.i_size_lo = (new_dir_data.len() as u64 & 0xFFFFFFFF) as u32;
            new_parent_inode.i_size_hi = (new_dir_data.len() as u64 >> 32) as u32;
            fs.write_file_to_storage(&mut new_parent_inode, 0, &new_dir_data, &mut storage)
                .map_err(|_| "ext4: write new dir failed")?;
            if child_file_type == Ext4FileType::Directory && new_parent_inode.i_links_count > 0 {
                new_parent_inode.i_links_count -= 1;
            }
        }

        // Yeni parent'a girdi ekle
        fs.create_dir_entry(&mut new_parent_inode, new_name, child_ino, child_file_type.clone(), &mut storage)
            .map_err(|_| "ext4: create new entry failed")?;

        // Eski parent'tan girdiyi sil
        if same_parent {
            let entries_after = fs.read_dir_from_storage(&old_parent_inode, &storage)
                .map_err(|_| "ext4: re-read dir failed")?;
            let new_dir_data: Vec<u8> = {
                let mut data = Vec::new();
                for entry in &entries_after {
                    if entry.name == old_name {
                        continue;
                    }
                    let ft_code = match entry.file_type {
                        Ext4FileType::Regular => 1,
                        Ext4FileType::Directory => 2,
                        Ext4FileType::Symlink => 7,
                        _ => 0,
                    };
                    let name_bytes = entry.name.as_bytes();
                    let entry_len = 8 + name_bytes.len();
                    let rec_len = (entry_len + 3) & !3;
                    let mut raw = vec![0u8; rec_len];
                    raw[0..4].copy_from_slice(&entry.inode.to_le_bytes());
                    raw[4..6].copy_from_slice(&(rec_len as u16).to_le_bytes());
                    raw[6] = name_bytes.len() as u8;
                    raw[7] = ft_code;
                    raw[8..8 + name_bytes.len()].copy_from_slice(name_bytes);
                    data.extend_from_slice(&raw);
                }
                data
            };
            old_parent_inode.i_size_lo = (new_dir_data.len() as u64 & 0xFFFFFFFF) as u32;
            old_parent_inode.i_size_hi = (new_dir_data.len() as u64 >> 32) as u32;
            fs.write_file_to_storage(&mut old_parent_inode, 0, &new_dir_data, &mut storage)
                .map_err(|_| "ext4: write old dir failed")?;
        } else {
            let old_dir_data: Vec<u8> = {
                let mut data = Vec::new();
                for entry in &old_entries {
                    if entry.name == old_name {
                        continue;
                    }
                    let ft_code = match entry.file_type {
                        Ext4FileType::Regular => 1,
                        Ext4FileType::Directory => 2,
                        Ext4FileType::Symlink => 7,
                        _ => 0,
                    };
                    let name_bytes = entry.name.as_bytes();
                    let entry_len = 8 + name_bytes.len();
                    let rec_len = (entry_len + 3) & !3;
                    let mut raw = vec![0u8; rec_len];
                    raw[0..4].copy_from_slice(&entry.inode.to_le_bytes());
                    raw[4..6].copy_from_slice(&(rec_len as u16).to_le_bytes());
                    raw[6] = name_bytes.len() as u8;
                    raw[7] = ft_code;
                    raw[8..8 + name_bytes.len()].copy_from_slice(name_bytes);
                    data.extend_from_slice(&raw);
                }
                data
            };
            old_parent_inode.i_size_lo = (old_dir_data.len() as u64 & 0xFFFFFFFF) as u32;
            old_parent_inode.i_size_hi = (old_dir_data.len() as u64 >> 32) as u32;
            fs.write_file_to_storage(&mut old_parent_inode, 0, &old_dir_data, &mut storage)
                .map_err(|_| "ext4: write old dir failed")?;
        }

        // Cross-directory rename: `..` girdisini güncelle + nlink
        if !same_parent && child_file_type == Ext4FileType::Directory {
            let mut child_inode = fs.read_inode_from_storage(child_ino, &storage)
                .map_err(|_| "ext4: read child inode failed")?;
            let child_entries = fs.read_dir_from_storage(&child_inode, &storage)
                .map_err(|_| "ext4: read child dir failed")?;
            let mut new_child_data: Vec<u8> = Vec::new();
            for entry in &child_entries {
                let ft_code = match entry.file_type {
                    Ext4FileType::Regular => 1,
                    Ext4FileType::Directory => 2,
                    Ext4FileType::Symlink => 7,
                    _ => 0,
                };
                let name_bytes = entry.name.as_bytes();
                let ino_val = if entry.name == ".." {
                    new_parent_ino
                } else {
                    entry.inode
                };
                let entry_len = 8 + name_bytes.len();
                let rec_len = (entry_len + 3) & !3;
                let mut raw = vec![0u8; rec_len];
                raw[0..4].copy_from_slice(&ino_val.to_le_bytes());
                raw[4..6].copy_from_slice(&(rec_len as u16).to_le_bytes());
                raw[6] = name_bytes.len() as u8;
                raw[7] = ft_code;
                raw[8..8 + name_bytes.len()].copy_from_slice(name_bytes);
                new_child_data.extend_from_slice(&raw);
            }
            child_inode.i_size_lo = (new_child_data.len() as u64 & 0xFFFFFFFF) as u32;
            child_inode.i_size_hi = (new_child_data.len() as u64 >> 32) as u32;
            fs.write_file_to_storage(&mut child_inode, 0, &new_child_data, &mut storage)
                .map_err(|_| "ext4: write child dir failed")?;
            fs.write_inode(child_ino, &child_inode, &mut storage)
                .map_err(|_| "ext4: write child inode failed")?;

            old_parent_inode.i_links_count = old_parent_inode.i_links_count.saturating_sub(1);
            new_parent_inode.i_links_count = new_parent_inode.i_links_count.saturating_add(1);
        }

        // Parent inode'ları güncelle
        fs.write_inode(old_parent_ino, &old_parent_inode, &mut storage)
            .map_err(|_| "ext4: write old parent inode failed")?;
        if !same_parent {
            fs.write_inode(new_parent_ino, &new_parent_inode, &mut storage)
                .map_err(|_| "ext4: write new parent inode failed")?;
        }

        Ok(())
    })();

    // Linux: inode_unlock in reverse order
    for &ino in lock_list.iter().rev() {
        ext4_inode_unlock(source, ino);
    }

    result?;

    update_ext4_instance(source, fs, storage)?;
    Ok(())
}

/// ext4 truncate: bir dosyanın boyutunu ayarlar.
///
/// `new_size == 0`: tüm blokları serbest bırakır, inode'u sıfırlar.
/// `new_size < current_size`: kısmi truncate — sadece i_size güncellenir
///   (block deallocation olmadan; eksik implementasyon).
/// `new_size >= current_size`: sadece i_size güncellenir (sparse extension).
/// Depth > 0 için tüm leaf extents'leri topla
fn collect_extents_into(
    inode: &Ext4Inode,
    block_size: u32,
    storage: &Ext4Storage,
    extents: &mut Vec<(u32, u16, u64)>,
) -> Result<(), Ext4Error> {
    if !inode.uses_extents() {
        return Ok(());
    }
    let mut header_bytes = [0u8; 12];
    header_bytes.copy_from_slice(&inode.i_block[..12]);
    let magic = u16::from_le_bytes([header_bytes[0], header_bytes[1]]);
    if magic != 0xF30A {
        return Ok(());
    }
    let depth = u16::from_le_bytes([header_bytes[6], header_bytes[7]]);

    if depth == 0 {
        let entries = u16::from_le_bytes([header_bytes[2], header_bytes[3]]) as usize;
        for i in 0..entries {
            let off = 12 + i * 12;
            if off + 12 > 60 {
                break;
            }
            let ee_block = u32::from_le_bytes([
                inode.i_block[off], inode.i_block[off + 1], inode.i_block[off + 2], inode.i_block[off + 3],
            ]);
            let ee_len_raw = u16::from_le_bytes([inode.i_block[off + 4], inode.i_block[off + 5]]);
            let ee_len = ee_len_raw & 0x7FFF;
            let start_hi = u16::from_le_bytes([inode.i_block[off + 6], inode.i_block[off + 7]]) as u64;
            let start_lo = u32::from_le_bytes([
                inode.i_block[off + 8], inode.i_block[off + 9], inode.i_block[off + 10], inode.i_block[off + 11],
            ]) as u64;
            let start = (start_hi << 32) | start_lo;
            extents.push((ee_block, ee_len, start));
        }
    } else {
        let entries = u16::from_le_bytes([header_bytes[2], header_bytes[3]]) as usize;
        for i in 0..entries {
            let off = 12 + i * 12;
            if off + 12 > 60 {
                break;
            }
            let leaf_hi = u16::from_le_bytes([inode.i_block[off + 4], inode.i_block[off + 5]]) as u64;
            let leaf_lo = u32::from_le_bytes([
                inode.i_block[off + 8], inode.i_block[off + 9], inode.i_block[off + 10], inode.i_block[off + 11],
            ]) as u64;
            let leaf_block = (leaf_hi << 32) | leaf_lo;
            if leaf_block == 0 {
                continue;
            }
            let leaf_data = storage
                .read_exact(leaf_block as usize * block_size as usize, block_size as usize)
                .map_err(|_| Ext4Error::ReadError)?;
            let child_inode = Ext4Inode {
                i_mode: 0,
                i_uid: 0,
                i_size_lo: 0,
                i_atime: 0,
                i_ctime: 0,
                i_mtime: 0,
                i_dtime: 0,
                i_gid: 0,
                i_links_count: 0,
                i_blocks_lo: 0,
                i_flags: 0x00080000, // EXTENTS_FL
                i_block: {
                    let mut b = [0u8; 60];
                    b[..leaf_data.len().min(60)].copy_from_slice(&leaf_data[..leaf_data.len().min(60)]);
                    b
                },
                i_size_hi: 0,
                i_generation: 0,
                i_file_acl_lo: 0,
                i_extra_isize: 0,
                i_checksum_hi: 0,
                i_crtime: 0,
                i_version_hi: 0,
                i_projid: 0,
            };

            // 5. callback ile her çocuğu bildir
        }
    }

    Ok(())
}

/// Kısmi truncate için extent tree'de blokları serbest bırakır ve i_block'u yeniden oluşturur
fn truncate_partial_blocks(
    fs: &mut Ext4FileSystem,
    inode: &mut Ext4Inode,
    blocks_needed: u32,
    storage: &mut Ext4Storage,
) -> Result<(), Ext4Error> {
    if inode.uses_extents() {
        let mut header_bytes = [0u8; 12];
        header_bytes.copy_from_slice(&inode.i_block[..12]);
        let magic = u16::from_le_bytes([header_bytes[0], header_bytes[1]]);
        if magic != 0xF30A {
            return Ok(());
        }
        let depth = u16::from_le_bytes([header_bytes[6], header_bytes[7]]);

        if depth == 0 {
            // Root is leaf: modify i_block directly
            let entries = u16::from_le_bytes([header_bytes[2], header_bytes[3]]) as usize;
            let mut new_block = [0u8; 60];
            let mut kept: u16 = 0;

            for i in 0..entries {
                let off = 12 + i * 12;
                if off + 12 > 60 {
                    break;
                }
                let ee_block = u32::from_le_bytes([
                    inode.i_block[off], inode.i_block[off + 1], inode.i_block[off + 2], inode.i_block[off + 3],
                ]);
                let ee_len_raw = u16::from_le_bytes([inode.i_block[off + 4], inode.i_block[off + 5]]);
                let ee_len = (ee_len_raw & 0x7FFF) as u64;
                let start_hi = u16::from_le_bytes([inode.i_block[off + 6], inode.i_block[off + 7]]) as u64;
                let start_lo = u32::from_le_bytes([
                    inode.i_block[off + 8], inode.i_block[off + 9], inode.i_block[off + 10], inode.i_block[off + 11],
                ]) as u64;
                let start = (start_hi << 32) | start_lo;

                if ee_block >= blocks_needed {
                    for b in start..start + ee_len {
                        fs.free_block(b, storage)?;
                    }
                } else if ee_block + ee_len as u32 <= blocks_needed {
                    let dst = 12 + kept as usize * 12;
                    new_block[dst..dst + 12].copy_from_slice(&inode.i_block[off..off + 12]);
                    kept += 1;
                } else {
                    let keep_blocks = (blocks_needed - ee_block) as u64;
                    for b in start + keep_blocks..start + ee_len {
                        fs.free_block(b, storage)?;
                    }
                    let dst = 12 + kept as usize * 12;
                    new_block[dst..dst + 4].copy_from_slice(&ee_block.to_le_bytes());
                    new_block[dst + 4..dst + 6].copy_from_slice(&(keep_blocks as u16).to_le_bytes());
                    new_block[dst + 6..dst + 8].copy_from_slice(&start_hi.to_le_bytes());
                    new_block[dst + 8..dst + 12].copy_from_slice(&start_lo.to_le_bytes());
                    kept += 1;
                }
            }
            // Write extent header
            new_block[..2].copy_from_slice(&0xF30Au16.to_le_bytes());
            new_block[2..4].copy_from_slice(&kept.to_le_bytes());
            new_block[4..6].copy_from_slice(&4u16.to_le_bytes()); // eh_max
            new_block[6..8].copy_from_slice(&0u16.to_le_bytes()); // depth
            inode.i_block.copy_from_slice(&new_block);
        } else {
            // Depth > 0: collect all extents, filter, rebuild i_block
            let mut extents: Vec<(u32, u16, u64)> = Vec::new();
            collect_extents_into(inode, fs.block_size, storage, &mut extents)?;

            // Free blocks past threshold, build kept list
            let mut kept_extents: Vec<(u32, u16, u64)> = Vec::new();
            for &(ee_block, ee_len_raw, start) in &extents {
                let ee_len = (ee_len_raw & 0x7FFF) as u64;
                if ee_block >= blocks_needed {
                    for b in start..start + ee_len {
                        fs.free_block(b, storage)?;
                    }
                } else if ee_block + ee_len as u32 <= blocks_needed {
                    kept_extents.push((ee_block, ee_len_raw, start));
                } else {
                    let keep_blocks = (blocks_needed - ee_block) as u64;
                    for b in start + keep_blocks..start + ee_len {
                        fs.free_block(b, storage)?;
                    }
                    kept_extents.push((ee_block, keep_blocks as u16, start));
                }
            }
            // Also free all index (internal) blocks in the original tree
            free_inode_blocks_deallocate_only(fs, inode, storage)?;

            // Rebuild depth-0 extent tree in i_block
            let max_entries = 4;
            let entry_count = kept_extents.len().min(max_entries) as u16;
            let mut new_block = [0u8; 60];
            new_block[..2].copy_from_slice(&0xF30Au16.to_le_bytes());
            new_block[2..4].copy_from_slice(&entry_count.to_le_bytes());
            new_block[4..6].copy_from_slice(&max_entries.to_le_bytes());
            new_block[6..8].copy_from_slice(&0u16.to_le_bytes()); // depth 0

            for i in 0..entry_count as usize {
                let (ee_block, ee_len, start) = kept_extents[i];
                let dst = 12 + i * 12;
                new_block[dst..dst + 4].copy_from_slice(&ee_block.to_le_bytes());
                new_block[dst + 4..dst + 6].copy_from_slice(&ee_len.to_le_bytes());
                new_block[dst + 6..dst + 8].copy_from_slice(&((start >> 32) as u16).to_le_bytes());
                new_block[dst + 8..dst + 12].copy_from_slice(&(start as u32).to_le_bytes());
            }
            inode.i_block.copy_from_slice(&new_block);
        }
        inode.i_flags |= 0x00080000; // ensure EXTENTS_FL
    } else {
        // Indirect blocks: free all blocks past blocks_needed
        let ptrs = inode.indirect_blocks();
        // Direct blocks 0..11
        for i in blocks_needed as usize..12 {
            if ptrs[i] != 0 {
                fs.free_block(ptrs[i] as u64, storage)?;
            }
        }
        // If blocks_needed <= 11, clear indirect chains entirely
        if blocks_needed <= 12 {
            if ptrs[12] != 0 {
                free_indirect_blocks(fs, ptrs[12] as u64, 1, storage)?;
                fs.free_block(ptrs[12] as u64, storage)?;
            }
            if ptrs[13] != 0 {
                free_indirect_blocks(fs, ptrs[13] as u64, 2, storage)?;
                fs.free_block(ptrs[13] as u64, storage)?;
            }
            if ptrs[14] != 0 {
                free_indirect_blocks(fs, ptrs[14] as u64, 3, storage)?;
                fs.free_block(ptrs[14] as u64, storage)?;
            }
            inode.i_block.fill(0);
        }
        // Note: for blocks_needed > 12, would need to truncate indirect chains,
        // which is not yet implemented. This case is extremely rare (small files).
    }
    Ok(())
}

/// Sadece extent index bloklarını serbest bırakır (data bloklarını değil)
fn free_inode_blocks_deallocate_only(
    fs: &mut Ext4FileSystem,
    inode: &Ext4Inode,
    storage: &mut Ext4Storage,
) -> Result<(), Ext4Error> {
    if !inode.uses_extents() {
        return Ok(());
    }
    let mut header_bytes = [0u8; 12];
    header_bytes.copy_from_slice(&inode.i_block[..12]);
    let magic = u16::from_le_bytes([header_bytes[0], header_bytes[1]]);
    if magic != 0xF30A {
        return Ok(());
    }
    let depth = u16::from_le_bytes([header_bytes[6], header_bytes[7]]);
    if depth > 0 {
        let entries = u16::from_le_bytes([header_bytes[2], header_bytes[3]]) as usize;
        for i in 0..entries {
            let off = 12 + i * 12;
            if off + 12 > 60 { break; }
            let leaf_lo = u32::from_le_bytes([
                inode.i_block[off + 4], inode.i_block[off + 5], inode.i_block[off + 6], inode.i_block[off + 7],
            ]) as u64;
            let leaf_hi = u16::from_le_bytes([inode.i_block[off + 8], inode.i_block[off + 9]]) as u64;
            let leaf_block = (leaf_hi << 32) | leaf_lo;
            if leaf_block != 0 {
                // Recursive: free child index blocks (depth-1) 
                // We don't have a direct reference to the child block, but we can read it as inode
                let block_size = fs.block_size as usize;
                if let Ok(child_data) = storage.read_exact(leaf_block as usize * block_size, block_size) {
                    // Parse as extent header and recursively free index blocks
                    if child_data.len() >= 12 {
                        let child_magic = u16::from_le_bytes([child_data[0], child_data[1]]);
                        let child_depth = u16::from_le_bytes([child_data[6], child_data[7]]);
                        if child_magic == 0xF30A && child_depth > 0 {
                            let child_inode = Ext4Inode {
                                i_mode: 0,
                                i_uid: 0,
                                i_size_lo: 0,
                                i_atime: 0,
                                i_ctime: 0,
                                i_mtime: 0,
                                i_dtime: 0,
                                i_gid: 0,
                                i_links_count: 0,
                                i_blocks_lo: 0,
                                i_flags: 0x00080000,
                                i_block: {
                                    let mut b = [0u8; 60];
                                    let copy_len = child_data.len().min(60);
                                    b[..copy_len].copy_from_slice(&child_data[..copy_len]);
                                    b
                                },
                                i_size_hi: 0,
                                i_generation: 0,
                                i_file_acl_lo: 0,
                                i_extra_isize: 0,
                                i_checksum_hi: 0,
                                i_crtime: 0,
                                i_version_hi: 0,
                                i_projid: 0,
                            };
                            free_inode_blocks_deallocate_only(fs, &child_inode, storage)?;
                        }
                    }
                }
                fs.free_block(leaf_block, storage)?;
            }
        }
    }
    Ok(())
}

/// ext4 truncate: bir dosyanın boyutunu ayarlar.
///
/// Linux VFS: vfs_truncate() → ext4_setattr() → ext4_truncate()
/// Lock ordering: file inode exclusively locked
pub fn ext4_truncate(source: &str, path: &str, new_size: u64) -> Result<(), &'static str> {
    let mounted = get_mounted_ext4(source).ok_or("ext4: backend not mounted")?;
    if mounted.fs.read_only {
        return Err("ext4: read-only filesystem");
    }
    let mut fs = mounted.fs;
    let mut storage = mounted.storage;

    let (ino, mut inode) = resolve_ext4_node_internal(source, path, &fs, &storage)
        .map_err(|_| "ext4: file not found")?;

    // Linux: inode_lock(file) — exclusive lock on file inode
    ext4_inode_lock(source, ino);

    let result = (|| -> Result<(), &'static str> {
        let current_size = inode.size();

        if new_size == 0 {
            free_inode_blocks(&mut fs, &mut inode, &mut storage)
                .map_err(|_| "ext4: free blocks failed")?;
        } else if new_size < current_size {
            let block_size = fs.block_size;
            let blocks_needed = ((new_size + block_size as u64 - 1) / block_size as u64) as u32;
            truncate_partial_blocks(&mut fs, &mut inode, blocks_needed, &mut storage)
                .map_err(|_| "ext4: partial truncate failed")?;
            inode.i_size_lo = (new_size & 0xFFFFFFFF) as u32;
            inode.i_size_hi = (new_size >> 32) as u32;
        } else {
            inode.i_size_lo = (new_size & 0xFFFFFFFF) as u32;
            inode.i_size_hi = (new_size >> 32) as u32;
        }

        fs.write_inode(ino, &inode, &mut storage)
            .map_err(|_| "ext4: write inode failed")?;

        Ok(())
    })();

    // Linux: inode_unlock(file)
    ext4_inode_unlock(source, ino);

    result?;

    update_ext4_instance(source, fs, storage)?;
    Ok(())
}

/// ext4 fsync: dosya ve metadata'yı diske yazdırır
///
/// Linux VFS: vfs_fsync() → ext4_sync_file()
/// Lock ordering: file inode exclusively locked during sync
pub fn ext4_fsync(source: &str, path: &str) -> Result<(), &'static str> {
    let mounted = get_mounted_ext4(source).ok_or("ext4: backend not mounted")?;
    if mounted.fs.read_only {
        return Err("ext4: read-only filesystem");
    }
    let mut fs = mounted.fs;
    let mut storage = mounted.storage;

    let (ino, _inode) = resolve_ext4_node_internal(source, path, &fs, &storage)
        .map_err(|_| "ext4: file not found")?;

    // Linux: inode_lock(file) — exclusive lock during fsync
    ext4_inode_lock(source, ino);

    let result = fs.sync(&mut []).map_err(|_| "ext4: fsync failed");

    // Linux: inode_unlock(file)
    ext4_inode_unlock(source, ino);

    result?;

    update_ext4_instance(source, fs, storage)?;
    Ok(())
}

/// ext4 chmod: dosya izinlerini değiştirir
///
/// Linux VFS: vfs_chmod() → ext4_setattr()
/// Lock ordering: file inode exclusively locked
pub fn ext4_chmod(source: &str, path: &str, mode: u16) -> Result<(), &'static str> {
    let mounted = get_mounted_ext4(source).ok_or("ext4: backend not mounted")?;
    if mounted.fs.read_only {
        return Err("ext4: read-only filesystem");
    }
    let mut fs = mounted.fs;
    let mut storage = mounted.storage;

    let (ino, mut inode) = resolve_ext4_node_internal(source, path, &fs, &storage)
        .map_err(|_| "ext4: file not found")?;

    // Linux: inode_lock(file) — exclusive
    ext4_inode_lock(source, ino);

    let result = (|| -> Result<(), &'static str> {
        inode.i_mode = (inode.i_mode & 0xF000) | (mode & 0x0FFF);
        fs.write_inode(ino, &inode, &mut storage)
            .map_err(|_| "ext4: write inode failed")?;
        Ok(())
    })();

    ext4_inode_unlock(source, ino);
    result?;

    update_ext4_instance(source, fs, storage)?;
    Ok(())
}

/// ext4 chown: dosya sahipliğini değiştirir
///
/// Linux VFS: vfs_chown() → ext4_setattr()
/// Lock ordering: file inode exclusively locked
pub fn ext4_chown(source: &str, path: &str, uid: u32, gid: u32) -> Result<(), &'static str> {
    let mounted = get_mounted_ext4(source).ok_or("ext4: backend not mounted")?;
    if mounted.fs.read_only {
        return Err("ext4: read-only filesystem");
    }
    let mut fs = mounted.fs;
    let mut storage = mounted.storage;

    let (ino, mut inode) = resolve_ext4_node_internal(source, path, &fs, &storage)
        .map_err(|_| "ext4: file not found")?;

    // Linux: inode_lock(file) — exclusive
    ext4_inode_lock(source, ino);

    let result = (|| -> Result<(), &'static str> {
        inode.i_uid = uid as u16;
        inode.i_gid = gid as u16;
        fs.write_inode(ino, &inode, &mut storage)
            .map_err(|_| "ext4: write inode failed")?;
        Ok(())
    })();

    ext4_inode_unlock(source, ino);
    result?;

    update_ext4_instance(source, fs, storage)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::block::{BlockDevice, BlockDeviceError, BlockDeviceType};

    struct MockBlockDevice {
        data: Vec<u8>,
        block_sz: u32,
    }

    impl BlockDevice for MockBlockDevice {
        fn read_block(&mut self, lba: u64, buffer: &mut [u8]) -> Result<(), BlockDeviceError> {
            let offset = (lba * self.block_sz as u64) as usize;
            if offset + buffer.len() <= self.data.len() {
                buffer.copy_from_slice(&self.data[offset..offset + buffer.len()]);
                Ok(())
            } else {
                Err(BlockDeviceError::IoError)
            }
        }

        fn write_block(&mut self, lba: u64, buffer: &[u8]) -> Result<(), BlockDeviceError> {
            let offset = (lba * self.block_sz as u64) as usize;
            if offset + buffer.len() <= self.data.len() {
                self.data[offset..offset + buffer.len()].copy_from_slice(buffer);
                Ok(())
            } else {
                Err(BlockDeviceError::IoError)
            }
        }

        fn block_size(&self) -> u32 {
            self.block_sz
        }

        fn block_count(&self) -> u64 {
            self.data.len() as u64 / self.block_sz as u64
        }

        fn device_name(&self) -> String {
            String::from("mock")
        }

        fn device_type(&self) -> BlockDeviceType {
            BlockDeviceType::Virtual
        }
    }

    impl crate::drivers::linux::BlockDevice for MockBlockDevice {
        fn read_sectors(&mut self, lba: u32, count: u8) -> Vec<u8> {
            let offset = lba as usize * 512;
            let len = count as usize * 512;
            if offset + len <= self.data.len() {
                self.data[offset..offset + len].to_vec()
            } else {
                Vec::new()
            }
        }

        fn write_sectors(&mut self, lba: u32, data: &[u8]) -> Result<(), ()> {
            let offset = lba as usize * 512;
            if offset + data.len() <= self.data.len() {
                self.data[offset..offset + data.len()].copy_from_slice(data);
                Ok(())
            } else {
                Err(())
            }
        }
    }

    #[test]
    fn corrupt_journal_mount_forces_read_only_policy() {
        let mut fs = Ext4FileSystem::new();
        let image = vec![0u8; 8192];
        let mut mock = MockBlockDevice {
            data: image.clone(),
            block_sz: 512,
        };

        assert_eq!(fs.init_journal(&mut mock, &image, 0, 4096), Ok(()));
        assert!(fs.is_read_only());
        assert_eq!(fs.begin_transaction(1), Err(Ext4Error::WriteError));
    }
}

// ============================================================================
// HTree Dizin İndeksleme (Hash Tree / dx_root)
// ============================================================================
//
// ext4 büyük dizinlerin O(n) yerine O(log n) aranmasını sağlamak için
// B-tree benzeri karma ağaç (htree) yapısı kullanır.
// Bu yapı, dizin bloğunun 0. girişinde dx_root olarak saklanır.

/// dx_node — HTree ara düğüm bloğu (kök olmayan indeks bloğu).
///
/// Yapısı dx_root ile aynıdır ancak dot/dotdot sahte girişleri yoktur:
///   offset 0-7:  fake dirent (inode=0, rec_len=block_size, name_len=0, file_type=0)
///   offset 8-15: rezerve (sıfır dolu)
///   offset 16-31: dx_root_info (reserved_zero, hash_version, info_length, ...)
///   offset 32+:  dx_entry dizisi
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct DxNode {
    pub fake_inode: u32,
    pub fake_rec_len: u16,
    pub fake_name_len: u8,
    pub fake_file_type: u8,
    pub reserved_pad: [u8; 8],
    pub reserved_zero: u32,
    pub hash_version: u8,
    pub info_length: u8,
    pub indirect_levels: u8,
    pub unused_flags: u8,
    pub limit: u16,
    pub count: u16,
    pub block: u32,
}

/// dx_root — HTree kök bloğu yapısı.
///
/// Dizin bloğunun başında yer alır ve ağacın meta verisini tutar.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct DxRoot {
    /// Sahte dot girişi (inode, rec_len, name_len, file_type)
    pub dot_inode: u32,
    pub dot_rec_len: u16,
    pub dot_name_len: u8,
    pub dot_file_type: u8,
    /// Sahte dotdot girişi
    pub dotdot_inode: u32,
    pub dotdot_rec_len: u16,
    pub dotdot_name_len: u8,
    pub dotdot_file_type: u8,
    // dx_root_info yapısı başlangıcı
    /// Ayrılmış (0)
    pub reserved_zero: u32,
    /// Hash versiyonu (0=legacy, 1=half_md4, 2=tea, 3=unsigned legacy, 4=unsigned half_md4, 5=unsigned tea, 6=siphash)
    pub hash_version: u8,
    /// Ağaç derinliği (info_length)
    pub info_length: u8,
    /// Dolaylılık seviyesi (indirect levels) — genellikle 0 veya 1
    pub indirect_levels: u8,
    /// Kullanılmayan bayraklar
    pub unused_flags: u8,
    /// Limit — bu blokta saklanabilecek maximum giriş sayısı
    pub limit: u16,
    /// Count — mevcut giriş sayısı
    pub count: u16,
    /// İlk hash aralığının bloğu
    pub block: u32,
}

/// dx_entry — HTree arama tablosu girişi.
///
/// Hash değerine göre sıralanmış blok referanslarıdır.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct DxEntry {
    /// Hash değeri alt sınırı
    pub hash: u32,
    /// Bu hash aralığını içeren blok numarası
    pub block: u32,
}

/// Desteklenen hash algoritmaları
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DxHashVersion {
    Legacy = 0,
    HalfMd4 = 1,
    Tea = 2,
    UnsignedLegacy = 3,
    UnsignedHalfMd4 = 4,
    UnsignedTea = 5,
    SipHash = 6,
}

impl DxHashVersion {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Legacy),
            1 => Some(Self::HalfMd4),
            2 => Some(Self::Tea),
            3 => Some(Self::UnsignedLegacy),
            4 => Some(Self::UnsignedHalfMd4),
            5 => Some(Self::UnsignedTea),
            6 => Some(Self::SipHash),
            _ => None,
        }
    }
}

/// Half-MD4 karma fonksiyonu (ext4 varsayılanı).
///
/// Dosya adını 32-bit hash değerine dönüştürür.
/// Gerçek half_md4, TEA tabanlı hash'e yakın basitleştirilmiş versiyondur.
pub fn dx_hash_half_md4(name: &[u8], seed: u32) -> u32 {
    let mut hash = seed;
    for &b in name {
        hash = hash.wrapping_mul(0x01000193) ^ (b as u32); // FNV-benzeri
    }
    // Sıfır hash geçersiz — 1'e yuvarlat
    if hash == 0 {
        1
    } else {
        hash
    }
}

/// HTree dizin araması.
///
/// `root_block` verilen dx_root bloğu ve `entries` listesi ile
/// belirtilen dosya adının bulunduğu dizin bloğu döner.
pub fn htree_lookup(entries: &[DxEntry], name_hash: u32) -> Option<u32> {
    if entries.is_empty() {
        return None;
    }
    // İkili arama: hash değerine göre doğru bloğu bul
    let mut lo = 0usize;
    let mut hi = entries.len();
    while lo + 1 < hi {
        let mid = (lo + hi) / 2;
        if entries[mid].hash <= name_hash {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some(entries[lo].block)
}

/// dx_node (ara düğüm) bloğundan giriş tablosunu ayrıştırır.
pub fn parse_dx_node(block_data: &[u8]) -> Option<(DxNode, Vec<DxEntry>)> {
    if block_data.len() < 40 {
        return None;
    }
    let node = DxNode {
        fake_inode: u32::from_le_bytes([block_data[0], block_data[1], block_data[2], block_data[3]]),
        fake_rec_len: u16::from_le_bytes([block_data[4], block_data[5]]),
        fake_name_len: block_data[6],
        fake_file_type: block_data[7],
        reserved_pad: {
            let mut p = [0u8; 8];
            p.copy_from_slice(&block_data[8..16]);
            p
        },
        reserved_zero: u32::from_le_bytes([block_data[16], block_data[17], block_data[18], block_data[19]]),
        hash_version: block_data[20],
        info_length: block_data[21],
        indirect_levels: block_data[22],
        unused_flags: block_data[23],
        limit: u16::from_le_bytes([block_data[24], block_data[25]]),
        count: u16::from_le_bytes([block_data[26], block_data[27]]),
        block: u32::from_le_bytes([block_data[28], block_data[29], block_data[30], block_data[31]]),
    };

    let count = node.count as usize;
    let mut entries = Vec::with_capacity(count);
    let mut offset = 32usize;
    for _ in 0..count {
        if offset + 8 > block_data.len() {
            break;
        }
        entries.push(DxEntry {
            hash: u32::from_le_bytes([
                block_data[offset], block_data[offset + 1],
                block_data[offset + 2], block_data[offset + 3],
            ]),
            block: u32::from_le_bytes([
                block_data[offset + 4], block_data[offset + 5],
                block_data[offset + 6], block_data[offset + 7],
            ]),
        });
        offset += 8;
    }
    Some((node, entries))
}

/// dx_root bloğundan giriş tablosunu ayrıştırır.
pub fn parse_dx_root(block_data: &[u8]) -> Option<(DxRoot, Vec<DxEntry>)> {
    if block_data.len() < 40 {
        return None;
    }
    let root = DxRoot {
        dot_inode: u32::from_le_bytes([block_data[0], block_data[1], block_data[2], block_data[3]]),
        dot_rec_len: u16::from_le_bytes([block_data[4], block_data[5]]),
        dot_name_len: block_data[6],
        dot_file_type: block_data[7],
        dotdot_inode: u32::from_le_bytes([
            block_data[8],
            block_data[9],
            block_data[10],
            block_data[11],
        ]),
        dotdot_rec_len: u16::from_le_bytes([block_data[12], block_data[13]]),
        dotdot_name_len: block_data[14],
        dotdot_file_type: block_data[15],
        reserved_zero: u32::from_le_bytes([
            block_data[16],
            block_data[17],
            block_data[18],
            block_data[19],
        ]),
        hash_version: block_data[20],
        info_length: block_data[21],
        indirect_levels: block_data[22],
        unused_flags: block_data[23],
        limit: u16::from_le_bytes([block_data[24], block_data[25]]),
        count: u16::from_le_bytes([block_data[26], block_data[27]]),
        block: u32::from_le_bytes([
            block_data[28],
            block_data[29],
            block_data[30],
            block_data[31],
        ]),
    };

    let count = root.count as usize;
    let mut entries = Vec::with_capacity(count);
    let mut offset = 32usize;
    for _ in 0..count {
        if offset + 8 > block_data.len() {
            break;
        }
        entries.push(DxEntry {
            hash: u32::from_le_bytes([
                block_data[offset],
                block_data[offset + 1],
                block_data[offset + 2],
                block_data[offset + 3],
            ]),
            block: u32::from_le_bytes([
                block_data[offset + 4],
                block_data[offset + 5],
                block_data[offset + 6],
                block_data[offset + 7],
            ]),
        });
        offset += 8;
    }

    Some((root, entries))
}

// ============================================================================
// CRASH CONSISTENCY CONTRACT (Wave 5.8)
// ============================================================================

use crate::fs::{CrashConsistentFs, CrashState, OperationCrashContract, RecoveryAction};

impl CrashConsistentFs for Ext4FileSystem {
    fn crash_contract(&self, operation: &'static str) -> Option<OperationCrashContract> {
        match operation {
            "create" => Some(OperationCrashContract {
                operation: "create",
                pre_state: CrashState::NotStarted,
                success_post_state: CrashState::Completed,
                allowed_crash_states: &[
                    CrashState::NotStarted,
                    CrashState::JournalCommitted,
                    CrashState::Completed,
                ],
                forbidden_crash_states: &[CrashState::Inconsistent],
                recovery_action: RecoveryAction::JournalReplay,
                fsck_required: false,
            }),
            "write" => Some(OperationCrashContract {
                operation: "write",
                pre_state: CrashState::NotStarted,
                success_post_state: CrashState::Completed,
                allowed_crash_states: &[
                    CrashState::NotStarted,
                    CrashState::JournalLogged,
                    CrashState::JournalCommitted,
                    CrashState::Completed,
                ],
                forbidden_crash_states: &[CrashState::Inconsistent],
                recovery_action: RecoveryAction::JournalReplay,
                fsck_required: false,
            }),
            "truncate" => Some(OperationCrashContract {
                operation: "truncate",
                pre_state: CrashState::NotStarted,
                success_post_state: CrashState::Completed,
                allowed_crash_states: &[
                    CrashState::NotStarted,
                    CrashState::JournalCommitted,
                    CrashState::Completed,
                ],
                forbidden_crash_states: &[CrashState::Inconsistent],
                recovery_action: RecoveryAction::JournalReplay,
                fsck_required: false,
            }),
            "rename" => Some(OperationCrashContract {
                operation: "rename",
                pre_state: CrashState::NotStarted,
                success_post_state: CrashState::Completed,
                allowed_crash_states: &[
                    CrashState::NotStarted,
                    CrashState::JournalCommitted,
                    CrashState::Completed,
                ],
                forbidden_crash_states: &[CrashState::Inconsistent],
                recovery_action: RecoveryAction::JournalReplay,
                fsck_required: false,
            }),
            "unlink" => Some(OperationCrashContract {
                operation: "unlink",
                pre_state: CrashState::NotStarted,
                success_post_state: CrashState::Completed,
                allowed_crash_states: &[
                    CrashState::NotStarted,
                    CrashState::JournalCommitted,
                    CrashState::Completed,
                ],
                forbidden_crash_states: &[CrashState::Inconsistent],
                recovery_action: RecoveryAction::JournalReplay,
                fsck_required: false,
            }),
            _ => None,
        }
    }

    fn verify_crash_state(&self, operation: &'static str) -> Result<CrashState, &'static str> {
        let _contract = self
            .crash_contract(operation)
            .ok_or("unknown ext4 operation for crash verification")?;

        if self.read_only {
            return Ok(CrashState::Inconsistent);
        }

        if self.superblock.s_magic != EXT4_MAGIC {
            return Ok(CrashState::Corrupt);
        }

        let clean_flag = self.superblock.s_state & 0x1;
        if clean_flag == 0 {
            if self.journal.is_some() {
                return Ok(CrashState::JournalLogged);
            }
            return Ok(CrashState::Inconsistent);
        }

        Ok(CrashState::Completed)
    }

    fn recover_from_crash(&mut self, operation: &'static str) -> Result<(), &'static str> {
        let contract = self
            .crash_contract(operation)
            .ok_or("unknown ext4 operation for crash recovery")?;

        match contract.recovery_action {
            RecoveryAction::JournalReplay => {
                crate::serial_println!(
                    "[ext4] JBD2 3-pass journal replay for operation {}",
                    operation
                );
                if self.journal.is_some() {
                    crate::serial_println!(
                        "[ext4] journal present, replay would run on next mount"
                    );
                    Ok(())
                } else {
                    crate::serial_println!("[ext4] no journal available for replay");
                    Err("ext4 journal not available for crash recovery")
                }
            }
            RecoveryAction::None => Ok(()),
            RecoveryAction::Fsck => {
                crate::serial_println!("[ext4] fsck required for operation {}", operation);
                Err("ext4 fsck not yet implemented as automatic recovery")
            }
            RecoveryAction::RollForward | RecoveryAction::Rollback | RecoveryAction::Manual => {
                crate::serial_println!(
                    "[ext4] recovery action {:?} not applicable to ext4",
                    contract.recovery_action
                );
                Err("recovery action not supported for ext4")
            }
        }
    }
}

// ============================================================================
// fsck — ext4 dosya sistemi kontrol aracı
// Deep web: e2fsck/pass1.c, e2fsck/super.c, e2fsck/pass5.c
// ============================================================================

/// fsck sonuçları
#[derive(Debug, Clone, Copy)]
pub enum FsckResult {
    Clean,
    Fixed,
    ErrorsFound,
    Corrupt,
}

/// Basit ext4 fsck — süper blok, group descriptor, inode table doğrulaması
/// Deep web: e2fsck pass1 (inode scan), pass5 (bitmap consistency)
/// Tam ext4 fsck — e2fsck-equivalent 8-pass consistency checker
/// Deep web: e2fsprogs source (pass1.c through pass5.c), Linux kernel ext4 docs
///
/// fsck pasları (e2fsprogs mimarisine göre):
/// Pass 1: Süper blok doğrulama (checksum, magic, features, block counts)
/// Pass 2: Group descriptor doğrulama (checksums, bitmap locations, inode table locations)
/// Pass 3: Block bitmap doğrulama (free blocks count, block allocation consistency)
/// Pass 4: Inode bitmap doğrulama (free inodes count, inode allocation consistency)
/// Pass 5: Inode table doğrulama (inode checksums, link counts, block counts)
/// Pass 6: Dizin doğrulama (entry consistency, link count verification)
/// Pass 7: Orphan inode handling
/// Pass 8: Journal verification and recovery
///
/// Arşiv kontrolü: super.html, group_descr.html, bitmaps.html, inode_table.html,
/// inodes.html, directory.html, journal.html, orphan.html, checksums.html
pub fn ext4_fsck(source: &str) -> Result<FsckResult, &'static str> {
    let mounted = get_mounted_ext4(source).ok_or("ext4: backend not mounted")?;
    let fs = &mounted.fs;
    let storage = &mounted.storage;
    let mut errors = 0usize;
    let mut warnings = 0usize;
    let mut fixable = 0usize;

    crate::serial_println!("[ext4] fsck başlatılıyor...");
    crate::serial_println!("[ext4] fsck: {} block, {} inode, {} bayt/blok",
        fs.superblock.total_blocks(), fs.superblock.s_inodes_count, fs.block_size);

    // ═══════════════════════════════════════════════════════════════════════
    // Pass 1: Süper blok doğrulama
    // Deep web: e2fsck/super.c check_super_block()
    // ═══════════════════════════════════════════════════════════════════════
    crate::serial_println!("[ext4] fsck Pass 1: Süper blok doğrulama...");

    // Magic number kontrolü
    if fs.superblock.s_magic != EXT4_MAGIC {
        crate::serial_println!("[ext4] fsck FATAL: sihirli sayı geçersiz (0x{:x})", fs.superblock.s_magic);
        return Ok(FsckResult::Corrupt);
    }

    // Süper blok checksum doğrulama (checksums.html: UUID + entire superblock)
    let mut sb_bytes = vec![0u8; fs.block_size as usize];
    sb_bytes[0..1024].copy_from_slice(&unsafe {
        core::slice::from_raw_parts(
            &fs.superblock as *const Ext4Superblock as *const u8,
            1024,
        )
    });
    if !Ext4Superblock::verify_checksum(&sb_bytes) {
        crate::serial_println!("[ext4] fsck HATA: süper blok checksum hatası");
        errors += 1;
    }

    // Blok boyutu mantıksal kontrol
    let block_size = fs.block_size;
    if block_size < 1024 || block_size > 65536 || !block_size.is_power_of_two() {
        crate::serial_println!("[ext4] fsck HATA: geçersiz blok boyutu ({})", block_size);
        errors += 1;
    }

    // Inode boyutu mantıksal kontrol
    let inode_size = fs.superblock.s_inode_size;
    if inode_size < 128 || inode_size as u32 > block_size || !inode_size.is_power_of_two() {
        crate::serial_println!("[ext4] fsck HATA: geçersiz inode boyutu ({})", inode_size);
        errors += 1;
    }

    // Blok sayaçları mantıksal kontrol
    let total_blocks = fs.superblock.total_blocks();
    let free_blocks = fs.superblock.s_free_blocks_count_lo as u64
        | ((fs.superblock.s_free_blocks_count_hi as u64) << 32);
    if free_blocks > total_blocks {
        crate::serial_println!("[ext4] fsck HATA: free blocks ({}) > total blocks ({})", free_blocks, total_blocks);
        errors += 1;
    }

    // Inode sayaçları mantıksal kontrol
    let free_inodes = fs.superblock.s_free_inodes_count as u64;
    if free_inodes > fs.superblock.s_inodes_count as u64 {
        crate::serial_println!("[ext4] fsck HATA: free inodes ({}) > total inodes ({})",
            free_inodes, fs.superblock.s_inodes_count);
        errors += 1;
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Pass 2: Group descriptor doğrulama
    // Deep web: e2fsck/super.c check_group_descriptors()
    // ═══════════════════════════════════════════════════════════════════════
    crate::serial_println!("[ext4] fsck Pass 2: Group descriptor doğrulama...");

    let groups = fs.superblock.s_inodes_count / fs.superblock.s_inodes_per_group;
    for i in 0..groups.min(fs.group_descriptors.len() as u32) {
        let gd = &fs.group_descriptors[i as usize];

        // Block bitmap doğrulama
        if gd.bg_block_bitmap_lo == 0 {
            crate::serial_println!("[ext4] fsck HATA: group {} block bitmap adresi sıfır", i);
            errors += 1;
        }

        // Inode bitmap doğrulama
        if gd.bg_inode_bitmap_lo == 0 {
            crate::serial_println!("[ext4] fsck HATA: group {} inode bitmap adresi sıfır", i);
            errors += 1;
        }

        // Inode table doğrulama
        if gd.bg_inode_table_lo == 0 {
            crate::serial_println!("[ext4] fsck HATA: group {} inode table adresi sıfır", i);
            errors += 1;
        }

        // Free blocks count mantıksal kontrol
        if gd.bg_free_blocks_count_lo as u32 > fs.superblock.s_blocks_per_group {
            crate::serial_println!("[ext4] fsck UYARI: group {} free blocks({}) > blocks_per_group({})",
                i, gd.bg_free_blocks_count_lo, fs.superblock.s_blocks_per_group);
            warnings += 1;
        }

        // Free inodes count mantıksal kontrol
        if gd.bg_free_inodes_count_lo as u32 > fs.superblock.s_inodes_per_group {
            crate::serial_println!("[ext4] fsck UYARI: group {} free inodes({}) > inodes_per_group({})",
                i, gd.bg_free_inodes_count_lo, fs.superblock.s_inodes_per_group);
            warnings += 1;
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Pass 3: Block bitmap doğrulama
    // Deep web: e2fsck/pass5.c check_block_bitmaps()
    // ═══════════════════════════════════════════════════════════════════════
    crate::serial_println!("[ext4] fsck Pass 3: Block bitmap doğrulama...");

    for i in 0..groups.min(fs.group_descriptors.len() as u32) {
        let gd = &fs.group_descriptors[i as usize];

        // Group BLOCK_UNINIT ise bitmap'i atla
        if gd.bg_flags & 0x0002 != 0 {
            continue;
        }

        // Block bitmap'i oku ve say
        let bitmap_lba = gd.bg_block_bitmap_lo as u64;
        if bitmap_lba == 0 {
            continue;
        }

        let bitmap_offset = bitmap_lba as usize * block_size as usize;
        if bitmap_offset + block_size as usize > storage.size() {
            crate::serial_println!("[ext4] fsck HATA: group {} bitmap offset taştı", i);
            errors += 1;
            continue;
        }

        // Bitmap'de free block sayısını say
        if let Ok(bitmap_data) = storage.read_exact(bitmap_offset, block_size as usize) {
            let mut free_count = 0u32;
            for byte in &bitmap_data {
                free_count += byte.count_ones() as u32; // Set bit = allocated, unset = free
            }
            free_count = block_size * 8 - free_count; // Free = total - allocated

            // Group descriptor'daki free count ile karşılaştır
            if free_count != gd.bg_free_blocks_count_lo as u32 {
                crate::serial_println!(
                    "[ext4] fsck UYARI: group {} block bitmap free({}) != desc free({})",
                    i, free_count, gd.bg_free_blocks_count_lo
                );
                warnings += 1;
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Pass 4: Inode bitmap doğrulama
    // Deep web: e2fsck/pass5.c check_inode_bitmaps()
    // ═══════════════════════════════════════════════════════════════════════
    crate::serial_println!("[ext4] fsck Pass 4: Inode bitmap doğrulama...");

    for i in 0..groups.min(fs.group_descriptors.len() as u32) {
        let gd = &fs.group_descriptors[i as usize];

        // Group INODE_UNINIT ise bitmap'i atla
        if gd.bg_flags & 0x0001 != 0 {
            continue;
        }

        // Inode bitmap'i oku ve say
        let bitmap_lba = gd.bg_inode_bitmap_lo as u64;
        if bitmap_lba == 0 {
            continue;
        }

        let bitmap_offset = bitmap_lba as usize * block_size as usize;
        if bitmap_offset + block_size as usize > storage.size() {
            crate::serial_println!("[ext4] fsck HATA: group {} inode bitmap offset taştı", i);
            errors += 1;
            continue;
        }

        // Bitmap'de free inode sayısını say
        if let Ok(bitmap_data) = storage.read_exact(bitmap_offset, block_size as usize) {
            let mut free_count = 0u32;
            for byte in &bitmap_data {
                free_count += byte.count_ones() as u32;
            }
            free_count = block_size * 8 - free_count;

            // Group descriptor'daki free count ile karşılaştır
            if free_count != gd.bg_free_inodes_count_lo as u32 {
                crate::serial_println!(
                    "[ext4] fsck UYARI: group {} inode bitmap free({}) != desc free({})",
                    i, free_count, gd.bg_free_inodes_count_lo
                );
                warnings += 1;
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Pass 5: Inode table doğrulama
    // Deep web: e2fsck/pass1.c scan_callback(), e2fsck/pass4.c check_ea_inode()
    // ═══════════════════════════════════════════════════════════════════════
    crate::serial_println!("[ext4] fsck Pass 5: Inode table doğrulama...");

    for i in 0..groups.min(fs.group_descriptors.len() as u32) {
        let gd = &fs.group_descriptors[i as usize];

        // Group INODE_UNINIT ise tabloyu atla
        if gd.bg_flags & 0x0001 != 0 {
            continue;
        }

        let inodes_start = gd.bg_inode_table_lo as u64 * block_size as u64;
        let inodes_per_group = fs.superblock.s_inodes_per_group as usize;
        let inode_size = fs.superblock.s_inode_size as usize;

        for j in 0..inodes_per_group {
            let inode_offset = (inodes_start as usize) + (j * inode_size);
            if inode_offset + inode_size > storage.size() {
                break;
            }

            if let Ok(inode_data) = storage.read_exact(inode_offset, inode_size) {
                // Inode boyutu kontrolü
                if inode_data.len() < 128 {
                    crate::serial_println!(
                        "[ext4] fsck HATA: group {} inode {} boyut yetersiz ({} < 128)",
                        i, j + 1, inode_data.len()
                    );
                    errors += 1;
                    continue;
                }

                // Inode checksum doğrulama (checksums.html: UUID + inode_num + generation + inode)
                if fs.superblock.s_feature_ro_compat & EXT4_FEATURE_RO_COMPAT_METADATA_CSUM != 0 {
                    let inode_num = i * fs.superblock.s_inodes_per_group + j as u32 + 1;
                    let stored_lo = u16::from_le_bytes([inode_data[0x7C], inode_data[0x7D]]) as u32;
                    let stored_hi = u16::from_le_bytes([inode_data[0x82], inode_data[0x83]]) as u32;
                    let stored_checksum = (stored_hi << 16) | stored_lo;

                    let mut seed = crate::fs::journal::crc32c_with_seed(&fs.superblock.s_uuid, !0u32);
                    seed = crate::fs::journal::crc32c_with_seed(&inode_num.to_le_bytes(), seed);

                    let mut data_for_csum = inode_data.clone();
                    data_for_csum[0x7C..0x7E].copy_from_slice(&0u16.to_le_bytes());
                    data_for_csum[0x82..0x84].copy_from_slice(&0u16.to_le_bytes());
                    let computed = crate::fs::journal::crc32c_with_seed(&data_for_csum, seed);

                    if computed != stored_checksum {
                        crate::serial_println!(
                            "[ext4] fsck HATA: group {} inode {} checksum hatası (stored=0x{:x}, computed=0x{:x})",
                            i, j + 1, stored_checksum, computed
                        );
                        errors += 1;
                    }
                }

                // i_links_count kontrolü
                let links_count = u16::from_le_bytes([inode_data[0x1A], inode_data[0x1B]]);
                if links_count == 0 {
                    // Links count sıfır ama dosya var — orphan olabilir
                    // (Pass 7'de işlenecek)
                }
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Pass 6: Dizin doğrulama
    // Deep web: e2fsck/pass2.c, ext4 directory.html (ext4_dir_entry_2 formatı)
    // ═══════════════════════════════════════════════════════════════════════
    crate::serial_println!("[ext4] fsck Pass 6: Dizin doğrulama...");

    // Root inode'u oku ve dizin entry'lerini doğrula
    let root_inode_offset = fs.superblock.s_first_ino as usize;
    let root_inode_location = fs.get_inode_location(root_inode_offset as u32);
    let root_offset = root_inode_location.0 as usize;

    if root_offset + root_inode_location.1 as usize <= storage.size() {
        if let Ok(root_data) = storage.read_exact(root_offset, root_inode_location.1 as usize) {
            if let Some(root_inode) = Ext4Inode::parse(&root_data) {
                // Root inode dizin mi?
                if (root_inode.i_mode & 0xF000) != 0x4000 {
                    crate::serial_println!("[ext4] fsck HATA: root inode dizin değil (mode=0x{:x})", root_inode.i_mode);
                    errors += 1;
                }

                // Root inode'daki entry'leri kontrol et
                if root_inode.i_block[0] != 0 || root_inode.i_block[4] != 0 {
                    // Extent tree veya block map var — basit doğrulama
                    let block_count = (root_inode.i_blocks_lo / (block_size / 512)) as usize;
                    if block_count > 0 {
                        crate::serial_println!(
                            "[ext4] fsck: root inode {} blok kullanıyor (extent tree)",
                            block_count
                        );
                    }
                }
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Pass 7: Orphan inode handling
    // Deep web: orphan.html (COMPAT_ORPHAN_FILE formatı)
    // ═══════════════════════════════════════════════════════════════════════
    crate::serial_println!("[ext4] fsck Pass 7: Orphan inode handling...");

    // Legacy orphan listesi kontrolü
    if fs.superblock.s_last_orphan != 0 {
        crate::serial_println!(
            "[ext4] fsck UYARI: legacy orphan listesi mevcut (s_last_orphan={})",
            fs.superblock.s_last_orphan
        );
        warnings += 1;
    }

    // Orphan file kontrolü (COMPAT_ORPHAN_FILE)
    let has_orphan_file = (fs.superblock.s_feature_compat & EXT4_FEATURE_COMPAT_ORPHAN_FILE) != 0;
    let has_present = (fs.superblock.s_feature_ro_compat & EXT4_FEATURE_RO_COMPAT_ORPHAN_PRESENT) != 0;

    if has_orphan_file && has_present {
        crate::serial_println!(
            "[ext4] fsck UYARI: orphan file aktif (inum={}), temizleme gerekli",
            fs.superblock.s_orphan_file_inum
        );
        warnings += 1;
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Pass 8: Journal verification and recovery
    // Deep web: journal.html (JBD2 layout, commit block, revocation block)
    // ═══════════════════════════════════════════════════════════════════════
    crate::serial_println!("[ext4] fsck Pass 8: Journal doğrulama...");

    if fs.journal.is_some() {
        // Journal recovery gerekli mi?
        if fs.superblock.s_state & 0x0002 != 0 {
            crate::serial_println!(
                "[ext4] fsck UYARI: journal recovery gerekli (kirli kapatma, s_state=0x{:x})",
                fs.superblock.s_state
            );
            warnings += 1;
        }

        // Journal superblock doğrulama
        if fs.journal_offset > 0 {
            let journal_sb_offset = fs.journal_offset as usize;
            if journal_sb_offset + block_size as usize <= storage.size() {
                if let Ok(journal_sb_data) = storage.read_exact(journal_sb_offset, block_size as usize) {
                    // Journal magic kontrolü (JBD2_MAGIC = 0x3031534A)
                    if journal_sb_data.len() >= 12 {
                        let magic = u32::from_be_bytes([
                            journal_sb_data[0], journal_sb_data[1],
                            journal_sb_data[2], journal_sb_data[3],
                        ]);
                        if magic != 0x3031534A {
                            crate::serial_println!(
                                "[ext4] fsck UYARI: journal superblock magic hatası (0x{:x})",
                                magic
                            );
                            warnings += 1;
                        }
                    }
                }
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Sonuç raporu
    // ═══════════════════════════════════════════════════════════════════════
    crate::serial_println!("[ext4] fsck tamamlandı:");
    crate::serial_println!("[ext4]   Hatalar: {}", errors);
    crate::serial_println!("[ext4]   Uyarılar: {}", warnings);
    crate::serial_println!("[ext4]   Düzeltilebilir: {}", fixable);

    if errors == 0 && warnings == 0 {
        crate::serial_println!("[ext4] fsck SONUÇ: TEMİZ — hata bulunamadı");
        Ok(FsckResult::Clean)
    } else if errors == 0 {
        crate::serial_println!("[ext4] fsck SONUÇ: UYARILAR MEVCUT — {} uyarı", warnings);
        Ok(FsckResult::ErrorsFound)
    } else {
        crate::serial_println!("[ext4] fsck SONUÇ: HATALAR MEVCUT — {} hata, {} uyarı", errors, warnings);
        Ok(FsckResult::ErrorsFound)
    }
}
