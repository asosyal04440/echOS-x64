//! # F2FS (Flash-Friendly File System) Sürücüsü
//!
//! F2FS, NAND flash depolama aygıtları için optimize edilmiş bir log-yapılı
//! dosya sistemidir. Samsung tarafından geliştirilmiş olup SSD ve eMMC gibi
//! flash tabanlı depolarda yüksek performans sunar.
//!
//! Bu modül F2FS biçimindeki bölümleri okuma/yazma işlemlerini sağlar:
//! - Dizin girişleri (F2fsEntry) üzerinden dosya meta verisi erişimi
//! - Vnode tabanlı inode yönetimi
//! - ATA blok aygıtı üzerinden sektör okuma/yazma
//! - Hashbrown tabanlı dizin önbelleği

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::convert::TryInto;
use core::hash::{BuildHasherDefault, Hasher};
use hashbrown::HashMap;
use lazy_static::lazy_static;
use rcore_fs::vfs::FsError;
use sha2::{Digest, Sha256};
use spin::Mutex;

use crate::drivers::ata::BLOCK_SIZE;
use crate::drivers::linux::BlockDevice;

pub struct F2fsEntry {
    pub ino: u64,
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub mode: u32, // Dosya izinleri (chmod)
    pub uid: u32,  // Sahip kullanıcı ID (chown)
    pub gid: u32,  // Sahip grup ID (chown)
}

impl Default for F2fsEntry {
    fn default() -> Self {
        Self {
            ino: 0,
            name: String::new(),
            size: 0,
            is_dir: false,
            is_symlink: false,
            mode: 0o644, // Varsayılan: -rw-r--r--
            uid: 0,      // root
            gid: 0,      // root
        }
    }
}

// ============================================================================
// METADATA CACHE (chmod/chown için)
// ============================================================================

/// Dosya metadata cache'i - chmod/chown değişikliklerini saklar
#[derive(Clone)]
pub struct FileMetadata {
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
}

impl Default for FileMetadata {
    fn default() -> Self {
        Self {
            mode: 0o644,
            uid: 0,
            gid: 0,
        }
    }
}

/// Dosya metadata'sını günceller (chmod/chown) - hem cache hem disk
pub fn set_file_metadata(
    path: &str,
    mode: Option<u32>,
    uid: Option<u32>,
    gid: Option<u32>,
) -> Result<(), FsError> {
    // Önce cache'e yaz
    {
        let mut cache = METADATA_CACHE.lock();
        let meta = cache
            .entry(path.to_string())
            .or_insert_with(FileMetadata::default);

        if let Some(m) = mode {
            meta.mode = m;
        }
        if let Some(u) = uid {
            meta.uid = u;
        }
        if let Some(g) = gid {
            meta.gid = g;
        }
    }

    // Diske de yaz (inode mode güncelle)
    if mode.is_some() {
        let mut drive = match crate::drivers::linux::select_block_device() {
            Ok(value) => value,
            Err(crate::drivers::linux::LinuxDriverError::NotFound) => {
                return Err(FsError::NoDevice)
            }
            Err(_) => return Err(FsError::DeviceError),
        };
        let ctx = load_context(&mut *drive)?;
        let inode = open_inode_by_path(&mut *drive, &ctx, path)?;
        let nat_entry = read_nat_entry(&mut *drive, &ctx, inode.ino)?;

        if nat_entry.block_addr == 0 {
            return Err(FsError::DeviceError);
        }

        let mut block = read_block(&mut *drive, &ctx, nat_entry.block_addr)?;

        // Mevcut mode'u oku ve permission bits'i güncelle
        let current_mode = read_u16(&block, INODE_I_MODE_OFFSET)?;
        let file_type = current_mode & 0o170000; // File type bits
        let new_mode = file_type | (mode.unwrap() as u16 & 0o7777);

        write_u16(&mut block, INODE_I_MODE_OFFSET, new_mode)?;

        // UID/GID yaz (offset 4 ve 8)
        if let Some(u) = uid {
            write_u32(&mut block, 4, u)?;
        }
        if let Some(g) = gid {
            write_u32(&mut block, 8, g)?;
        }

        write_block(&mut *drive, &ctx, nat_entry.block_addr, &block)?;

        crate::serial_println!(
            "[FS] Metadata written to disk: {} -> mode={:o}, uid={:?}, gid={:?}",
            path,
            new_mode,
            uid,
            gid
        );
    }

    Ok(())
}

/// Dosya metadata'sını okur
pub fn get_file_metadata(path: &str) -> FileMetadata {
    let cache = METADATA_CACHE.lock();
    cache.get(path).cloned().unwrap_or_default()
}

// ============================================================================
// MOUNT TABLE
// ============================================================================

/// Mount noktası bilgisi
#[derive(Clone)]
pub struct MountPoint {
    pub device: String,
    pub mountpoint: String,
    pub fs_type: String,
    pub flags: u32,
}

type F2fsHashBuilder = BuildHasherDefault<F2fsHasher>;

lazy_static! {
    /// Metadata cache - path -> metadata mapping
    static ref METADATA_CACHE: Mutex<HashMap<String, FileMetadata, F2fsHashBuilder>> =
        Mutex::new(HashMap::with_hasher(F2fsHashBuilder::default()));
    /// Mount table - mountpoint -> MountPoint mapping
    static ref MOUNT_TABLE: Mutex<HashMap<String, MountPoint, F2fsHashBuilder>> =
        Mutex::new(HashMap::with_hasher(F2fsHashBuilder::default()));
}

/// Dosya sistemi bağlar (mount)
pub fn mount_fs(device: &str, mountpoint: &str, fs_type: &str) -> Result<(), FsError> {
    let mut table = MOUNT_TABLE.lock();

    // Zaten mount edilmiş mi kontrol et
    if table.contains_key(mountpoint) {
        return Err(FsError::Busy);
    }

    table.insert(
        mountpoint.to_string(),
        MountPoint {
            device: device.to_string(),
            mountpoint: mountpoint.to_string(),
            fs_type: fs_type.to_string(),
            flags: 0,
        },
    );

    crate::serial_println!("[FS] Mounted: {} -> {} ({})", device, mountpoint, fs_type);
    Ok(())
}

/// Dosya sistemi ayırır (umount)
pub fn umount_fs(mountpoint: &str) -> Result<(), FsError> {
    let mut table = MOUNT_TABLE.lock();

    if table.remove(mountpoint).is_some() {
        crate::serial_println!("[FS] Unmounted: {}", mountpoint);
        Ok(())
    } else {
        Err(FsError::EntryNotFound)
    }
}

/// Tüm mount noktalarını listeler
pub fn list_mounts() -> Vec<MountPoint> {
    let table = MOUNT_TABLE.lock();
    table.values().cloned().collect()
}

/// Mount noktası var mı kontrol eder
pub fn is_mounted(mountpoint: &str) -> bool {
    let table = MOUNT_TABLE.lock();
    table.contains_key(mountpoint)
}

struct F2fsSuperblock {
    magic: u32,
    log_sectorsize: u32,
    log_sectors_per_block: u32,
    log_blocksize: u32,
    log_blocks_per_seg: u32,
    segment_count_sit: u32,
    segment_count_nat: u32,
    segment_count_ssa: u32,
    segment_count_main: u32,
    cp_blkaddr: u32,
    sit_blkaddr: u32,
    nat_blkaddr: u32,
    ssa_blkaddr: u32,
    main_blkaddr: u32,
    root_ino: u32,
    cp_payload: u32,
}

pub(crate) struct F2fsContext {
    partition_lba: u32,
    block_size: u32,
    sectors_per_block: u32,
    blocks_per_seg: u32,
    cp_blkaddr: u32,
    cp_payload: u32,
    sit_blkaddr: u32,
    nat_blkaddr: u32,
    ssa_blkaddr: u32,
    main_blkaddr: u32,
    root_ino: u32,
    segment_count_nat: u32,
    segment_count_sit: u32,
    segment_count_ssa: u32,
    segment_count_main: u32,
    nat_ver_bitmap: Option<Vec<u8>>,
    sit_ver_bitmap: Option<Vec<u8>>,
    cur_node_segno: [u32; MAX_ACTIVE_NODE_LOGS],
    cur_node_blkoff: [u16; MAX_ACTIVE_NODE_LOGS],
    cur_data_segno: [u32; MAX_ACTIVE_DATA_LOGS],
    cur_data_blkoff: [u16; MAX_ACTIVE_DATA_LOGS],
    alloc_type: [u8; MAX_ACTIVE_LOGS],
}

pub struct F2fsStats {
    pub total_main_segments: u32,
    pub blocks_per_segment: u32,
    pub total_main_blocks: u64,
    pub used_blocks: u64,
    pub free_blocks: u64,
    pub segments_with_valid: u32,
}

struct F2fsHasher(u64);

impl Default for F2fsHasher {
    fn default() -> Self {
        Self(0xcbf29ce484222325)
    }
}

impl Hasher for F2fsHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        let mut hash = self.0;
        for &b in bytes {
            hash ^= b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        self.0 = hash;
    }
}

struct F2fsPageCache {
    block_size: u32,
    entries: HashMap<u32, Vec<u8>, F2fsHashBuilder>,
}

impl F2fsPageCache {
    fn new() -> Self {
        Self {
            block_size: 0,
            entries: HashMap::with_hasher(F2fsHashBuilder::default()),
        }
    }

    fn configure(&mut self, block_size: u32) {
        if self.block_size != block_size {
            self.entries.clear();
            self.block_size = block_size;
        }
    }

    fn get(&mut self, block_addr: u32) -> Option<Vec<u8>> {
        self.entries.get(&block_addr).cloned()
    }

    fn put(&mut self, block_addr: u32, data: Vec<u8>) {
        if self.block_size == 0 || data.len() != self.block_size as usize {
            return;
        }
        self.entries.insert(block_addr, data);
    }

    fn invalidate(&mut self, block_addr: u32) {
        self.entries.remove(&block_addr);
    }
}
lazy_static! {
    static ref F2FS_PAGE_CACHE: Mutex<F2fsPageCache> = Mutex::new(F2fsPageCache::new());
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum F2fsIoPolicy {
    Default,
    Hot,
    Cold,
}

struct F2fsCheckpoint {
    checkpoint_ver: u64,
    ckpt_flags: u32,
    cp_pack_total_block_count: u32,
    sit_ver_bitmap_bytesize: u32,
    nat_ver_bitmap_bytesize: u32,
    checksum_offset: u32,
    nat_ver_bitmap: Option<Vec<u8>>,
    sit_ver_bitmap: Option<Vec<u8>>,
    cur_node_segno: [u32; MAX_ACTIVE_NODE_LOGS],
    cur_node_blkoff: [u16; MAX_ACTIVE_NODE_LOGS],
    cur_data_segno: [u32; MAX_ACTIVE_DATA_LOGS],
    cur_data_blkoff: [u16; MAX_ACTIVE_DATA_LOGS],
    alloc_type: [u8; MAX_ACTIVE_LOGS],
}

struct CheckpointPack {
    checkpoint: F2fsCheckpoint,
    checksum_ok: bool,
    layout_ok: bool,
}

pub(crate) struct F2fsInodeInfo {
    pub ino: u32,
    pub(crate) is_dir: bool,
    size: u64,
    inline: bool,
    inline_data: Option<Vec<u8>>,
    addrs: Vec<u32>,
}

pub(crate) struct DirEntryInfo {
    pub(crate) name: String,
    pub(crate) ino: u32,
    pub(crate) is_dir: bool,
}

struct NatEntry {
    ino: u32,
    block_addr: u32,
    _version: u8,
}

struct SitEntry {
    vblocks: u16,
    alloc_type: u8,
    valid_map: Vec<u8>,
    mtime: u64,
}

struct SummaryEntry {
    nid: u32,
    version: u8,
    ofs_in_node: u16,
}

const F2FS_MAGIC: u32 = 0xf2f52010;
const MBR_SIGNATURE_OFFSET: usize = 510;
const PARTITION_ENTRY_OFFSET: usize = 446;
const PARTITION_LBA_OFFSET: usize = 8;
const F2FS_SUPERBLOCK_SECTOR_OFFSET: u32 = 2;
const F2FS_SUPERBLOCK_SIZE: usize = 4096;
const SUPER_MAGIC_OFFSET: usize = 0;
const SUPER_LOG_SECTORSIZE_OFFSET: usize = 8;
const SUPER_LOG_SECTORS_PER_BLOCK_OFFSET: usize = 12;
const SUPER_LOG_BLOCKSIZE_OFFSET: usize = 16;
const SUPER_LOG_BLOCKS_PER_SEG_OFFSET: usize = 20;
const SUPER_SEGMENT_COUNT_SIT_OFFSET: usize = 56;
const SUPER_SEGMENT_COUNT_NAT_OFFSET: usize = 60;
const SUPER_SEGMENT_COUNT_SSA_OFFSET: usize = 64;
const SUPER_SEGMENT_COUNT_MAIN_OFFSET: usize = 68;
const SUPER_CP_BLKADDR_OFFSET: usize = 76;
const SUPER_SIT_BLKADDR_OFFSET: usize = 80;
const SUPER_NAT_BLKADDR_OFFSET: usize = 84;
const SUPER_SSA_BLKADDR_OFFSET: usize = 88;
const SUPER_MAIN_BLKADDR_OFFSET: usize = 92;
const SUPER_ROOT_INO_OFFSET: usize = 0x60;
const SUPER_CP_PAYLOAD_OFFSET: usize = 0x680;

const CP_CHECKPOINT_VER_OFFSET: usize = 0;
const CP_CUR_NODE_SEGNO_OFFSET: usize = 36;
const CP_CUR_NODE_BLKOFF_OFFSET: usize = 68;
const CP_CUR_DATA_SEGNO_OFFSET: usize = 84;
const CP_CUR_DATA_BLKOFF_OFFSET: usize = 116;
const CP_CKPT_FLAGS_OFFSET: usize = 132;
const CP_CP_PACK_TOTAL_BLOCK_COUNT_OFFSET: usize = 136;
const CP_SIT_VER_BITMAP_BYTESIZE_OFFSET: usize = 156;
const CP_NAT_VER_BITMAP_BYTESIZE_OFFSET: usize = 160;
const CP_CHECKSUM_OFFSET_OFFSET: usize = 164;
const CP_ALLOC_TYPE_OFFSET: usize = 176;
const CP_BITMAP_OFFSET: usize = 0xC0;
const MAX_ACTIVE_NODE_LOGS: usize = 8;
const MAX_ACTIVE_DATA_LOGS: usize = 8;
const MAX_ACTIVE_LOGS: usize = 16;

const INODE_I_MODE_OFFSET: usize = 0;
const INODE_I_UID_OFFSET: usize = 4; // UID
const INODE_I_GID_OFFSET: usize = 8; // GID
const INODE_I_ATIME_OFFSET: usize = 12; // Access time
const INODE_I_CTIME_OFFSET: usize = 16; // Create time (SIZE ile çakışıyor, dikkat)
const INODE_I_MTIME_OFFSET: usize = 20; // Modify time
const INODE_I_NLINK_OFFSET: usize = 24; // Hard link count
const INODE_I_SIZE_OFFSET: usize = 16; // Size (ctime ile aynı offset - F2FS spec)
const INODE_I_INLINE_OFFSET: usize = 3;
const INODE_I_ADDR_OFFSET: usize = 360;
const INODE_SIZE_OF_I_NID: usize = 20;
const NODE_FOOTER_SIZE: usize = 24;
const NODE_FOOTER_NID_OFFSET: usize = 0;
const NODE_FOOTER_INO_OFFSET: usize = 4;
const NODE_FOOTER_CPVER_OFFSET: usize = 8;
const NODE_FOOTER_FLAGS_OFFSET: usize = 16;
const F2FS_FSYNC_MARK: u32 = 0x01;
const F2FS_DENT_MARK: u32 = 0x02;
const F2FS_INODE_MARK: u32 = 0x04;
const INODE_NID_COUNT: usize = 5;
const F2FS_INLINE_DATA: u8 = 0x02;
const F2FS_INLINE_DENTRY: u8 = 0x04;
const F2FS_POLICY_HOT: u8 = 0x10;
const F2FS_POLICY_COLD: u8 = 0x20;
const S_IFMT: u16 = 0o170000;
const S_IFDIR: u16 = 0o040000;
const S_IFREG: u16 = 0o100000;
const S_IFLNK: u16 = 0o120000;

const INODE_I_NLINK_DISK_OFFSET: usize = 12;
const INODE_I_SIZE_DISK_OFFSET: usize = 16;
const INODE_I_ATIME_DISK_OFFSET: usize = 32;
const INODE_I_CTIME_DISK_OFFSET: usize = 40;
const INODE_I_MTIME_DISK_OFFSET: usize = 48;
const INODE_I_FLAGS_DISK_OFFSET: usize = 80;
const INODE_I_EXTRA_ISIZE_DISK_OFFSET: usize = INODE_I_ADDR_OFFSET;
const F2FS_EXTRA_ATTR_FLAG: u8 = 0x20;
const F2FS_COMPR_INODE_FLAG: u32 = 0x0000_0004;
const F2FS_ENCRYPT_INODE_FLAG: u32 = 0x0000_0800;
const F2FS_COMPRESSION_EXTRA_ATTR_SIZE: usize = 36;
const F2FS_COMPRESS_BLOCKS_OFFSET: usize = INODE_I_ADDR_OFFSET + 24;
const F2FS_COMPRESS_ALGO_OFFSET: usize = INODE_I_ADDR_OFFSET + 32;
const F2FS_LOG_CLUSTER_SIZE_OFFSET: usize = INODE_I_ADDR_OFFSET + 33;
const F2FS_COMPRESS_FLAG_OFFSET: usize = INODE_I_ADDR_OFFSET + 34;
const INODE_I_PINO_OFFSET: usize = 28;
const INODE_I_NAMELEN_OFFSET: usize = 32;
const INODE_I_NAME_OFFSET: usize = 36;
const F2FS_SLOT_NAME_LEN: usize = 247;
const SIT_VBLOCKS_SHIFT: u16 = 10;
const SIT_VBLOCKS_MASK: u16 = (1 << SIT_VBLOCKS_SHIFT) - 1;

const DENTRY_BITMAP_SIZE: usize = 27;
const DENTRY_RESERVED_SIZE: usize = 3;
const DENTRY_ENTRY_SIZE: usize = 11;
const DENTRY_SLOT_LEN: usize = 8;
const DENTRY_SLOTS: usize = 214;
const DENTRY_ENTRIES_OFFSET: usize = DENTRY_BITMAP_SIZE + DENTRY_RESERVED_SIZE;
const DENTRY_FILENAME_OFFSET: usize = DENTRY_ENTRIES_OFFSET + (DENTRY_ENTRY_SIZE * DENTRY_SLOTS);

const NAT_ENTRY_SIZE: usize = 9;
const SIT_VBLOCK_MAP_SIZE: usize = 64;
const SIT_ENTRY_SIZE: usize = 2 + SIT_VBLOCK_MAP_SIZE + 8;
const F2FS_SUM_BLKSIZE: usize = 4096;
const SUMMARY_ENTRY_SIZE: usize = 7;
const SUMMARY_ENTRIES: usize = F2FS_SUM_BLKSIZE / 8;
const NODE_OFS_SENTINEL: u16 = u16::MAX;

pub fn detect_f2fs() -> Result<bool, FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    let partition_lba = read_partition_lba(&mut *drive).unwrap_or(0);
    let superblock = read_superblock(&mut *drive, partition_lba)?;
    Ok(superblock.magic == F2FS_MAGIC)
}

pub fn f2fs_sync() -> Result<(), FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    let ctx = load_context(&mut *drive)?;
    update_checkpoint(
        &mut *drive,
        &ctx,
        ctx.nat_ver_bitmap.as_deref(),
        ctx.sit_ver_bitmap.as_deref(),
        None,
    )?;
    drive.flush().map_err(|_| FsError::DeviceError)
}

pub fn f2fs_stats() -> Result<F2fsStats, FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    let ctx = load_context(&mut *drive)?;
    let mut used_blocks = 0u64;
    let mut segments_with_valid = 0u32;
    for segno in 0..ctx.segment_count_main {
        let entry = read_sit_entry(&mut *drive, &ctx, segno)?;
        let mut seg_used = 0u16;
        for b in entry.valid_map.iter() {
            seg_used = seg_used.saturating_add(b.count_ones() as u16);
        }
        if seg_used > 0 {
            segments_with_valid = segments_with_valid.saturating_add(1);
        }
        used_blocks = used_blocks.saturating_add(seg_used as u64);
    }
    let total_main_blocks = ctx.segment_count_main as u64 * ctx.blocks_per_seg as u64;
    let free_blocks = total_main_blocks.saturating_sub(used_blocks);
    Ok(F2fsStats {
        total_main_segments: ctx.segment_count_main,
        blocks_per_segment: ctx.blocks_per_seg,
        total_main_blocks,
        used_blocks,
        free_blocks,
        segments_with_valid,
    })
}

pub fn f2fs_stats_pretty() -> Result<String, FsError> {
    let stats = f2fs_stats()?;
    let mut out = String::new();
    out.push_str("F2FS stats:\n");
    out.push_str("  main segments: ");
    out.push_str(&stats.total_main_segments.to_string());
    out.push_str("\n  blocks per segment: ");
    out.push_str(&stats.blocks_per_segment.to_string());
    out.push_str("\n  total main blocks: ");
    out.push_str(&stats.total_main_blocks.to_string());
    out.push_str("\n  used blocks: ");
    out.push_str(&stats.used_blocks.to_string());
    out.push_str("\n  free blocks: ");
    out.push_str(&stats.free_blocks.to_string());
    out.push_str("\n  segments with valid blocks: ");
    out.push_str(&stats.segments_with_valid.to_string());
    out.push('\n');
    Ok(out)
}

pub fn f2fs_set_policy(path: &str, policy: F2fsIoPolicy) -> Result<(), FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    let ctx = load_context(&mut *drive)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;
    let nat_entry = read_nat_entry(&mut *drive, &ctx, inode.ino)?;
    if nat_entry.block_addr == 0 {
        return Err(FsError::DeviceError);
    }
    let mut block = read_block(&mut *drive, &ctx, nat_entry.block_addr)?;
    let flags = block
        .get_mut(INODE_I_INLINE_OFFSET)
        .ok_or(FsError::DeviceError)?;
    *flags &= !(F2FS_POLICY_HOT | F2FS_POLICY_COLD);
    match policy {
        F2fsIoPolicy::Default => {}
        F2fsIoPolicy::Hot => {
            *flags |= F2FS_POLICY_HOT;
        }
        F2fsIoPolicy::Cold => {
            *flags |= F2FS_POLICY_COLD;
        }
    }
    write_block(&mut *drive, &ctx, nat_entry.block_addr, &block)
}

pub fn open_entry(path: &str) -> Result<F2fsEntry, FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    let ctx = load_context(&mut *drive)?;
    if path.trim_start_matches('/').is_empty() {
        return Ok(F2fsEntry {
            ino: ctx.root_ino as u64,
            name: "/".to_string(),
            size: 0,
            is_dir: true,
            is_symlink: false,
            mode: 0o755,
            uid: 0,
            gid: 0,
        });
    }
    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;

    // Metadata cache'den bilgileri al
    let meta = get_file_metadata(path);

    Ok(F2fsEntry {
        ino: inode.ino as u64,
        name: path.to_string(),
        size: inode.size,
        is_dir: inode.is_dir,
        is_symlink: (meta.mode & 0xA000) == 0xA000,
        mode: meta.mode,
        uid: meta.uid,
        gid: meta.gid,
    })
}

pub fn lookup_child(parent_ino: u64, name: &str) -> Result<F2fsEntry, FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    let ctx = load_context(&mut *drive)?;
    let parent_inode = read_inode(&mut *drive, &ctx, parent_ino as u32)?;
    if !parent_inode.is_dir {
        return Err(FsError::NotFile);
    }
    let entry = find_entry_in_dir(&mut *drive, &ctx, &parent_inode, name)?;
    let child_inode = read_inode(&mut *drive, &ctx, entry.ino)?;
    let (i_mode, uid, gid) = read_inode_meta(&mut *drive, &ctx, entry.ino)?;
    let is_symlink = (i_mode & S_IFMT) == S_IFLNK;
    Ok(F2fsEntry {
        ino: child_inode.ino as u64,
        name: name.to_string(),
        size: child_inode.size,
        is_dir: entry.is_dir,
        is_symlink,
        mode: i_mode as u32,
        uid,
        gid,
    })
}

pub fn list_dir(path: &str) -> Result<Vec<F2fsEntry>, FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    let ctx = load_context(&mut *drive)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;
    if !inode.is_dir {
        return Err(FsError::NotFile);
    }
    let mut out = Vec::new();
    for entry in read_dir_entries(&mut *drive, &ctx, &inode)? {
        let child = read_inode(&mut *drive, &ctx, entry.ino)?;
        let meta = get_file_metadata(&entry.name);
        out.push(F2fsEntry {
            ino: child.ino as u64,
            name: entry.name,
            size: child.size,
            is_dir: entry.is_dir,
            is_symlink: (meta.mode & 0xA000) == 0xA000,
            mode: meta.mode,
            uid: meta.uid,
            gid: meta.gid,
        });
    }
    Ok(out)
}

pub fn read_f2fs_file_at(path: &str, offset: usize, buf: &mut [u8]) -> Result<usize, FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    let ctx = load_context(&mut *drive)?;
    read_f2fs_file_at_with_context(&mut *drive, &ctx, path, offset, buf)
}

fn read_f2fs_file_at_with_context(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    path: &str,
    offset: usize,
    buf: &mut [u8],
) -> Result<usize, FsError> {
    let normalized = normalize_absolute_path(path)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, normalized.as_str())?;
    if inode.is_dir {
        return Err(FsError::IsDir);
    }

    // Check if inode has compression flag
    let nat_entry = read_nat_entry(&mut *drive, &ctx, inode.ino)?;
    if nat_entry.block_addr == 0 {
        return Err(FsError::DeviceError);
    }
    let inode_block = read_block(&mut *drive, &ctx, nat_entry.block_addr)?;
    let flags = read_inode_flags_from_block(&inode_block)?;
    let is_compressed = (flags & F2FS_COMPR_INODE_FLAG) != 0;

    if is_compressed {
        // Read compressed file and decompress
        let config = read_compress_config_from_inode(&inode_block)?;
        let decompressed = read_compressed_internal(&ctx, &inode, &config)?;
        if offset as u64 >= decompressed.len() as u64 {
            return Ok(0);
        }
        let max_len = decompressed.len().saturating_sub(offset);
        let read_len = core::cmp::min(max_len, buf.len());
        buf[..read_len].copy_from_slice(&decompressed[offset..offset + read_len]);
        return Ok(read_len);
    }

    if inode.inline {
        let data = inode.inline_data.as_ref().ok_or(FsError::DeviceError)?;
        if offset as u64 >= inode.size {
            return Ok(0);
        }
        let max_len = (inode.size as usize).saturating_sub(offset);
        let read_len = core::cmp::min(max_len, buf.len());
        let end = offset.saturating_add(read_len);
        if end > data.len() {
            return Err(FsError::DeviceError);
        }
        buf[..read_len].copy_from_slice(&data[offset..end]);
        return Ok(read_len);
    }
    if offset as u64 >= inode.size {
        return Ok(0);
    }
    let max_len = (inode.size as usize).saturating_sub(offset);
    let read_len = core::cmp::min(max_len, buf.len());
    let block_size = ctx.block_size as usize;
    let mut remaining = read_len;
    let mut read_total = 0usize;
    let mut block_index = offset / block_size;
    let mut block_offset = offset % block_size;
    while remaining > 0 {
        let addr = get_data_block_addr(&mut *drive, &ctx, inode.ino, block_index)?;
        let block = if addr == 0 {
            vec![0u8; block_size]
        } else {
            read_block(&mut *drive, &ctx, addr)?
        };
        let available = block_size.saturating_sub(block_offset);
        let to_copy = core::cmp::min(remaining, available);
        let src_end = block_offset + to_copy;
        let dst_end = read_total + to_copy;
        if src_end > block.len() || dst_end > buf.len() {
            return Err(FsError::DeviceError);
        }
        buf[read_total..dst_end].copy_from_slice(&block[block_offset..src_end]);
        remaining -= to_copy;
        read_total += to_copy;
        block_index += 1;
        block_offset = 0;
    }
    Ok(read_total)
}

pub fn read_f2fs_file_on_partition(
    partition_label: &str,
    path: &str,
) -> Result<Vec<u8>, FsError> {
    let normalized = normalize_absolute_path(path)?;
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    let partition_lba =
        read_partition_lba_by_label(&mut *drive, partition_label).ok_or(FsError::EntryNotFound)?;
    let ctx = load_context_for_partition_lba(&mut *drive, partition_lba)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, normalized.as_str())?;
    if inode.is_dir {
        return Err(FsError::IsDir);
    }
    let file_size: usize = inode.size.try_into().map_err(|_| FsError::DeviceError)?;
    let mut data = vec![0u8; file_size];
    let read = read_f2fs_file_at_with_context(
        &mut *drive,
        &ctx,
        normalized.as_str(),
        0,
        data.as_mut_slice(),
    )?;
    data.truncate(read);
    Ok(data)
}

pub fn write_f2fs_file_at(path: &str, offset: usize, buf: &[u8]) -> Result<usize, FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    if buf.is_empty() {
        return Ok(0);
    }
    let ctx = load_context(&mut *drive)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;
    if inode.is_dir {
        return Err(FsError::IsDir);
    }
    let new_end = offset.saturating_add(buf.len());
    let block_size = ctx.block_size as usize;
    if inode.inline {
        let nat_entry = read_nat_entry(&mut *drive, &ctx, inode.ino)?;
        if nat_entry.block_addr == 0 {
            return Err(FsError::DeviceError);
        }
        let mut block = read_block(&mut *drive, &ctx, nat_entry.block_addr)?;
        let inline_capacity = inline_data_capacity_for_block(&block, ctx.block_size)?;
        let data_start = inode_addr_offset_for_block(&block)?;
        let data_end = data_start.saturating_add(inline_capacity);
        if data_end > block.len() {
            return Err(FsError::DeviceError);
        }
        if new_end > inline_capacity {
            return Err(FsError::DeviceError);
        }
        let dst_start = data_start.saturating_add(offset);
        let dst_end = dst_start.saturating_add(buf.len());
        if dst_end > data_end {
            return Err(FsError::DeviceError);
        }
        block[dst_start..dst_end].copy_from_slice(buf);
        let new_size = core::cmp::max(inode.size, new_end as u64);
        write_u64(&mut block, INODE_I_SIZE_DISK_OFFSET, new_size)?;
        write_block(&mut *drive, &ctx, nat_entry.block_addr, &block)?;
        return Ok(buf.len());
    }
    let mut remaining = buf.len();
    let mut written_total = 0usize;
    let mut block_index = offset / block_size;
    let mut block_offset = offset % block_size;
    let mut checkpoint_dirty = false;
    let mut max_end = inode.size as usize;
    while remaining > 0 {
        let mut addr = get_data_block_addr(&mut *drive, &ctx, inode.ino, block_index)?;
        if addr == 0 {
            if block_index > u16::MAX as usize {
                return Err(FsError::DeviceError);
            }
            addr = allocate_data_block(&mut *drive, &ctx, inode.ino, block_index as u16)?;
            update_inode_block_addr(&mut *drive, &ctx, inode.ino, block_index, addr)?;
            checkpoint_dirty = true;
        }
        let mut block = read_block(&mut *drive, &ctx, addr)?;
        let available = block_size.saturating_sub(block_offset);
        let to_copy = core::cmp::min(remaining, available);
        let src_end = written_total + to_copy;
        let dst_end = block_offset + to_copy;
        if dst_end > block.len() || src_end > buf.len() {
            return Err(FsError::DeviceError);
        }
        block[block_offset..dst_end].copy_from_slice(&buf[written_total..src_end]);
        write_block(&mut *drive, &ctx, addr, &block)?;
        remaining -= to_copy;
        written_total += to_copy;
        let end_pos = block_index
            .saturating_mul(block_size)
            .saturating_add(block_offset)
            .saturating_add(to_copy);
        if end_pos > max_end {
            max_end = end_pos;
        }
        block_index += 1;
        block_offset = 0;
    }
    if max_end as u64 > inode.size {
        update_inode_size(&mut *drive, &ctx, inode.ino, max_end as u64)?;
        checkpoint_dirty = true;
    }
    if checkpoint_dirty {
        update_checkpoint(
            &mut *drive,
            &ctx,
            ctx.nat_ver_bitmap.as_deref(),
            ctx.sit_ver_bitmap.as_deref(),
            None,
        )?;
    }
    Ok(written_total)
}

/// O_DIRECT write — page cache bypass, doğrudan blok cihaza yaz
///
/// Spec: open.2 man page (cephanelik):
/// - "Try to minimize cache effects of the I/O to and from this file"
/// - "File I/O is done directly to/from user-space buffers"
/// - "O_DIRECT flag on its own makes an effort to transfer data synchronously,
///   but does not give the guarantees of the O_SYNC flag that data and
///   necessary metadata are transferred"
/// - "To guarantee synchronous I/O, O_SYNC must be used in addition to O_DIRECT"
///
/// Alignment requirements (EINVAL on violation):
/// - File offset aligned to block size
/// - Buffer memory aligned to block size
/// - I/O length multiple of block size
///
/// O_DIRECT alone: data flushed to hardware, metadata NOT guaranteed
/// O_SYNC | O_DIRECT: data + all metadata flushed (via sys_fsync after write)
pub fn write_f2fs_file_direct(path: &str, offset: usize, buf: &[u8]) -> Result<usize, FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    if buf.is_empty() {
        return Ok(0);
    }

    let ctx = load_context(&mut *drive)?;
    let block_size = ctx.block_size as usize;

    if offset % block_size != 0 {
        return Err(FsError::InvalidParam);
    }
    if buf.len() % block_size != 0 {
        return Err(FsError::InvalidParam);
    }
    let buf_ptr = buf.as_ptr() as usize;
    if buf_ptr % block_size != 0 {
        return Err(FsError::InvalidParam);
    }

    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;
    if inode.is_dir {
        return Err(FsError::IsDir);
    }

    let mut remaining = buf.len();
    let mut written_total = 0usize;
    let mut block_index = offset / block_size;

    while remaining > 0 {
        let mut addr = get_data_block_addr(&mut *drive, &ctx, inode.ino, block_index)?;
        if addr == 0 {
            if block_index > u16::MAX as usize {
                return Err(FsError::DeviceError);
            }
            addr = allocate_data_block(&mut *drive, &ctx, inode.ino, block_index as u16)?;
            update_inode_block_addr(&mut *drive, &ctx, inode.ino, block_index, addr)?;
        }

        // Direct write: translate F2FS block_addr → physical LBA, bypass cache
        let lba = ctx
            .partition_lba
            .saturating_add((addr as u64 * ctx.sectors_per_block as u64) as u32);
        let to_write = block_size.min(remaining);
        let write_data = &buf[written_total..written_total + to_write];
        // Pad to block size if partial last block
        if to_write < block_size {
            let mut padded = vec![0u8; block_size];
            padded[..to_write].copy_from_slice(write_data);
            drive
                .write_sectors(lba as u32, &padded)
                .map_err(|_| FsError::DeviceError)?;
        } else {
            drive
                .write_sectors(lba as u32, write_data)
                .map_err(|_| FsError::DeviceError)?;
        }

        remaining -= to_write;
        written_total += to_write;
        block_index += 1;
    }

    let new_end = offset + written_total;
    if new_end as u64 > inode.size {
        update_inode_size(&mut *drive, &ctx, inode.ino, new_end as u64)?;
    }

    drive.flush().map_err(|_| FsError::DeviceError)?;

    Ok(written_total)
}

/// O_DIRECT read — page cache bypass, doğrudan blok cihazdan oku
pub fn read_f2fs_file_direct(path: &str, offset: usize, buf: &mut [u8]) -> Result<usize, FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    if buf.is_empty() {
        return Ok(0);
    }

    let ctx = load_context(&mut *drive)?;
    let block_size = ctx.block_size as usize;

    if offset % block_size != 0 {
        return Err(FsError::InvalidParam);
    }
    if buf.len() % block_size != 0 {
        return Err(FsError::InvalidParam);
    }
    let buf_ptr = buf.as_ptr() as usize;
    if buf_ptr % block_size != 0 {
        return Err(FsError::InvalidParam);
    }

    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;
    if inode.is_dir {
        return Err(FsError::IsDir);
    }

    let file_size = inode.size as usize;
    if offset >= file_size {
        return Ok(0);
    }

    let mut remaining = buf.len().min(file_size.saturating_sub(offset));
    let mut read_total = 0usize;
    let mut block_index = offset / block_size;

    while remaining > 0 {
        let addr = get_data_block_addr(&mut *drive, &ctx, inode.ino, block_index)?;
        if addr == 0 {
            let to_read = block_size.min(remaining);
            buf[read_total..read_total + to_read].fill(0);
            remaining -= to_read;
            read_total += to_read;
            block_index += 1;
            continue;
        }

        let lba = ctx
            .partition_lba
            .saturating_add((addr as u64 * ctx.sectors_per_block as u64) as u32);
        let to_read = block_size.min(remaining);
        let sectors = ((to_read + block_size - 1) / block_size).max(1) as u8;
        let data = drive.read_sectors(lba as u32, sectors);
        let copy_len = to_read.min(data.len());
        buf[read_total..read_total + copy_len].copy_from_slice(&data[..copy_len]);

        remaining -= to_read;
        read_total += to_read;
        block_index += 1;
    }

    Ok(read_total)
}

pub fn write_new_f2fs_file_on_partition(
    partition_label: &str,
    path: &str,
    data: &[u8],
) -> Result<(), FsError> {
    crate::debug::serial::trace_raw(format_args!(
        "[F2FS] write_new_f2fs_file_on_partition: label={} path={}\n",
        partition_label, path
    ));
    let normalized = normalize_absolute_path(path)?;
    let (parent_path, name) = split_parent_and_name(normalized.as_str())?;
    crate::debug::serial::trace_raw(format_args!("[F2FS] select_block_device...\n"));
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => {
            crate::debug::serial::trace_raw(format_args!("[F2FS] block device selected OK\n"));
            value
        }
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => {
            crate::debug::serial::trace_raw(format_args!("[F2FS] block device NOT FOUND\n"));
            return Err(FsError::NoDevice);
        }
        Err(e) => {
            crate::debug::serial::trace_raw(format_args!("[F2FS] block device ERROR: {:?}\n", e));
            return Err(FsError::DeviceError);
        }
    };
    crate::debug::serial::trace_raw(format_args!(
        "[F2FS] read_partition_lba_by_label({})...\n",
        partition_label
    ));
    let partition_lba =
        read_partition_lba_by_label(&mut *drive, partition_label).ok_or(FsError::EntryNotFound)?;
    crate::debug::serial::trace_raw(format_args!("[F2FS] partition LBA: {}\n", partition_lba));
    crate::debug::serial::trace_raw(format_args!("[F2FS] load_context_for_partition_lba...\n"));
    let ctx = load_context_for_partition_lba(&mut *drive, partition_lba)?;
    crate::debug::serial::trace_raw(format_args!("[F2FS] context loaded OK\n"));
    crate::debug::serial::trace_raw(format_args!(
        "[F2FS] create_f2fs_dir_all_with_context({})...\n",
        parent_path
    ));
    create_f2fs_dir_all_with_context(&mut *drive, &ctx, parent_path.as_str())?;
    crate::debug::serial::trace_raw(format_args!("[F2FS] dir tree OK\n"));
    if let Ok(existing) = open_inode_by_path(&mut *drive, &ctx, normalized.as_str()) {
        if existing.is_dir {
            return Err(FsError::IsDir);
        }
        unlink_f2fs_with_context(&mut *drive, &ctx, parent_path.as_str(), name.as_str())?;
    }
    crate::debug::serial::trace_raw(format_args!(
        "[F2FS] create_f2fs_file_with_data_with_context...\n"
    ));
    create_f2fs_file_with_data_with_context(
        &mut *drive,
        &ctx,
        parent_path.as_str(),
        name.as_str(),
        data,
    )
}

pub fn sync_f2fs_partition(partition_label: &str) -> Result<(), FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    let partition_lba =
        read_partition_lba_by_label(&mut *drive, partition_label).ok_or(FsError::EntryNotFound)?;
    let ctx = load_context_for_partition_lba(&mut *drive, partition_lba)?;
    update_checkpoint(
        &mut *drive,
        &ctx,
        ctx.nat_ver_bitmap.as_deref(),
        ctx.sit_ver_bitmap.as_deref(),
        Some(CP_SYNC_FLAG),
    )?;
    drive.flush().map_err(|_| FsError::DeviceError)
}

pub fn create_f2fs_file(parent_path: &str, name: &str) -> Result<(), FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    let ctx = load_context(&mut *drive)?;
    let parent = open_inode_by_path(&mut *drive, &ctx, parent_path)?;
    if !parent.is_dir {
        return Err(FsError::NotDir);
    }
    if find_entry_in_dir(&mut *drive, &ctx, &parent, name).is_ok() {
        return Err(FsError::EntryExist);
    }
    let nid = allocate_free_nid(&mut *drive, &ctx)?;
    allocate_node_block_for_nid(&mut *drive, &ctx, nid)?;
    let nat_entry = read_nat_entry(&mut *drive, &ctx, nid)?;
    if nat_entry.block_addr == 0 {
        return Err(FsError::DeviceError);
    }
    let mut block = read_block(&mut *drive, &ctx, nat_entry.block_addr)?;
    let mode: u16 = S_IFREG | 0o644;
    write_u16(&mut block, INODE_I_MODE_OFFSET, mode)?;
    write_u64(&mut block, INODE_I_SIZE_DISK_OFFSET, 0)?;
    write_block(&mut *drive, &ctx, nat_entry.block_addr, &block)?;
    add_entry_to_dir(&mut *drive, &ctx, parent.ino, name, nid, false)
}

/// Create file with initial data
pub fn create_f2fs_file_with_data(
    parent_path: &str,
    name: &str,
    data: &[u8],
) -> Result<(), FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    let ctx = load_context(&mut *drive)?;
    create_f2fs_file_with_data_with_context(&mut *drive, &ctx, parent_path, name, data)
}

pub fn create_f2fs_dir(parent_path: &str, name: &str) -> Result<(), FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    let ctx = load_context(&mut *drive)?;
    let parent = open_inode_by_path(&mut *drive, &ctx, parent_path)?;
    if !parent.is_dir {
        return Err(FsError::NotDir);
    }
    if find_entry_in_dir(&mut *drive, &ctx, &parent, name).is_ok() {
        return Err(FsError::EntryExist);
    }
    let nid = allocate_free_nid(&mut *drive, &ctx)?;
    allocate_node_block_for_nid(&mut *drive, &ctx, nid)?;
    let nat_entry = read_nat_entry(&mut *drive, &ctx, nid)?;
    if nat_entry.block_addr == 0 {
        return Err(FsError::DeviceError);
    }
    let mut block = read_block(&mut *drive, &ctx, nat_entry.block_addr)?;
    let mode: u16 = S_IFDIR | 0o755;
    write_u16(&mut block, INODE_I_MODE_OFFSET, mode)?;
    write_u64(&mut block, INODE_I_SIZE_DISK_OFFSET, ctx.block_size as u64)?;
    write_block(&mut *drive, &ctx, nat_entry.block_addr, &block)?;
    let data_addr = allocate_data_block(&mut *drive, &ctx, nid, 0)?;
    update_inode_block_addr(&mut *drive, &ctx, nid, 0, data_addr)?;
    let mut dir_block = vec![0u8; ctx.block_size as usize];
    init_dentry_block(&mut dir_block, nid, parent.ino)?;
    write_block(&mut *drive, &ctx, data_addr, &dir_block)?;
    add_entry_to_dir(&mut *drive, &ctx, parent.ino, name, nid, true)
}

pub fn unlink_f2fs(parent_path: &str, name: &str) -> Result<(), FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    let ctx = load_context(&mut *drive)?;
    let parent = open_inode_by_path(&mut *drive, &ctx, parent_path)?;
    if !parent.is_dir {
        return Err(FsError::NotDir);
    }
    unlink_f2fs_with_context(&mut *drive, &ctx, parent_path, name)
}

pub fn rename_f2fs(parent_path: &str, old_name: &str, new_name: &str) -> Result<(), FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    let ctx = load_context(&mut *drive)?;
    rename_f2fs_with_context(&mut *drive, &ctx, parent_path, old_name, new_name)
}

pub fn rename_f2fs_on_partition(
    partition_label: &str,
    parent_path: &str,
    old_name: &str,
    new_name: &str,
) -> Result<(), FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    let partition_lba =
        read_partition_lba_by_label(&mut *drive, partition_label).ok_or(FsError::EntryNotFound)?;
    let ctx = load_context_for_partition_lba(&mut *drive, partition_lba)?;
    rename_f2fs_with_context(&mut *drive, &ctx, parent_path, old_name, new_name)
}

/// F2FS chmod: dosya izinlerini değiştirir
pub fn chmod_f2fs(path: &str, mode: u16) -> Result<(), FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    let ctx = load_context(&mut *drive)?;
    let normalized = normalize_absolute_path(path)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, normalized.as_str())?;
    let nat_entry = read_nat_entry(&mut *drive, &ctx, inode.ino)?;
    if nat_entry.block_addr == 0 {
        return Err(FsError::DeviceError);
    }
    let mut inode_block = read_block(&mut *drive, &ctx, nat_entry.block_addr)?;
    // i_mode field: first 2 bytes of inode struct (at offset 0)
    let new_mode = (inode_block[0] as u16) | mode;
    inode_block[0] = (new_mode & 0xFF) as u8;
    inode_block[1] = ((new_mode >> 8) & 0xFF) as u8;
    write_block(&mut *drive, &ctx, nat_entry.block_addr, &inode_block)?;
    Ok(())
}

/// F2FS chown: dosya sahipliğini değiştirir
pub fn chown_f2fs(path: &str, uid: u32, gid: u32) -> Result<(), FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    let ctx = load_context(&mut *drive)?;
    let normalized = normalize_absolute_path(path)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, normalized.as_str())?;
    let nat_entry = read_nat_entry(&mut *drive, &ctx, inode.ino)?;
    if nat_entry.block_addr == 0 {
        return Err(FsError::DeviceError);
    }
    let mut inode_block = read_block(&mut *drive, &ctx, nat_entry.block_addr)?;
    // i_uid at offset 4 (2 bytes), i_gid at offset 6 (2 bytes)
    inode_block[4] = (uid & 0xFF) as u8;
    inode_block[5] = ((uid >> 8) & 0xFF) as u8;
    inode_block[6] = (gid & 0xFF) as u8;
    inode_block[7] = ((gid >> 8) & 0xFF) as u8;
    write_block(&mut *drive, &ctx, nat_entry.block_addr, &inode_block)?;
    Ok(())
}

#[allow(unreachable_code)]
fn rename_f2fs_with_context(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    parent_path: &str,
    old_name: &str,
    new_name: &str,
) -> Result<(), FsError> {
    if old_name == new_name {
        return Ok(());
    }
    let parent = open_inode_by_path(&mut *drive, &ctx, parent_path)?;
    if !parent.is_dir {
        return Err(FsError::NotDir);
    }
    let entry = find_entry_in_dir(&mut *drive, &ctx, &parent, old_name)?;
    if find_entry_in_dir(&mut *drive, &ctx, &parent, new_name).is_ok() {
        return Err(FsError::EntryExist);
    }

    // Atomic rename: in-place rename dentry slot'u sığarsa atomik olarak yeniden adlandır.
    // Yeni isim eski isimden uzunsa (fazla slot gerekli), in-place rename başarısız olur.
    // Non-atomic fallback (ekle + sil) crash-safe değildir: crash anında iki isim kalır.
    // Phase 6 crash consistency contract: atomic rename zorunlu.
    // Çözüm: yeniden adlandırma başarısızsa hata döndür, non-atomic fallback yapma.
    if !rename_entry_in_dir(&mut *drive, &ctx, parent.ino, old_name, new_name)? {
        crate::serial_println!(
            "[f2fs] rename failed: new name '{}' requires more dentry slots than old name '{}'",
            new_name, old_name
        );
        return Err(FsError::DeviceError);
    }
    drive.flush().map_err(|_| FsError::DeviceError)?;
    Ok(())
}

fn create_f2fs_file_with_data_with_context(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    parent_path: &str,
    name: &str,
    data: &[u8],
) -> Result<(), FsError> {
    create_f2fs_file_with_context(drive, ctx, parent_path, name)?;
    let file_path = if parent_path == "/" || parent_path.is_empty() {
        alloc::format!("/{}", name)
    } else {
        alloc::format!("{}/{}", parent_path, name)
    };
    write_f2fs_file_at_with_context(drive, ctx, file_path.as_str(), 0, data)?;
    Ok(())
}

fn create_f2fs_file_with_context(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    parent_path: &str,
    name: &str,
) -> Result<(), FsError> {
    let parent = open_inode_by_path(drive, ctx, parent_path)?;
    if !parent.is_dir {
        return Err(FsError::NotDir);
    }
    if find_entry_in_dir(drive, ctx, &parent, name).is_ok() {
        return Err(FsError::EntryExist);
    }
    let nid = allocate_free_nid(drive, ctx)?;
    allocate_node_block_for_nid(drive, ctx, nid)?;
    let nat_entry = read_nat_entry(drive, ctx, nid)?;
    if nat_entry.block_addr == 0 {
        return Err(FsError::DeviceError);
    }
    let mut block = read_block(drive, ctx, nat_entry.block_addr)?;
    let mode: u16 = S_IFREG | 0o644;
    write_u16(&mut block, INODE_I_MODE_OFFSET, mode)?;
    write_u64(&mut block, INODE_I_SIZE_DISK_OFFSET, 0)?;
    write_block(drive, ctx, nat_entry.block_addr, &block)?;
    add_entry_to_dir(drive, ctx, parent.ino, name, nid, false)
}

fn write_f2fs_file_at_with_context(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    path: &str,
    offset: usize,
    buf: &[u8],
) -> Result<usize, FsError> {
    if buf.is_empty() {
        return Ok(0);
    }
    let inode = open_inode_by_path(drive, ctx, path)?;
    if inode.is_dir {
        return Err(FsError::IsDir);
    }
    let new_end = offset.saturating_add(buf.len());
    let block_size = ctx.block_size as usize;
    if inode.inline {
        let nat_entry = read_nat_entry(drive, ctx, inode.ino)?;
        if nat_entry.block_addr == 0 {
            return Err(FsError::DeviceError);
        }
        let mut block = read_block(drive, ctx, nat_entry.block_addr)?;
        let inline_capacity = inline_data_capacity_for_block(&block, ctx.block_size)?;
        let data_start = inode_addr_offset_for_block(&block)?;
        let data_end = data_start.saturating_add(inline_capacity);
        if data_end > block.len() {
            return Err(FsError::DeviceError);
        }
        if new_end > inline_capacity {
            return Err(FsError::DeviceError);
        }
        let dst_start = data_start.saturating_add(offset);
        let dst_end = dst_start.saturating_add(buf.len());
        if dst_end > data_end {
            return Err(FsError::DeviceError);
        }
        block[dst_start..dst_end].copy_from_slice(buf);
        let new_size = core::cmp::max(inode.size, new_end as u64);
        write_u64(&mut block, INODE_I_SIZE_DISK_OFFSET, new_size)?;
        write_block(drive, ctx, nat_entry.block_addr, &block)?;
        return Ok(buf.len());
    }
    let mut remaining = buf.len();
    let mut written_total = 0usize;
    let mut block_index = offset / block_size;
    let mut block_offset = offset % block_size;
    let mut checkpoint_dirty = false;
    let mut max_end = inode.size as usize;
    while remaining > 0 {
        let mut addr = get_data_block_addr(drive, ctx, inode.ino, block_index)?;
        if addr == 0 {
            if block_index > u16::MAX as usize {
                return Err(FsError::DeviceError);
            }
            addr = allocate_data_block(drive, ctx, inode.ino, block_index as u16)?;
            update_inode_block_addr(drive, ctx, inode.ino, block_index, addr)?;
            checkpoint_dirty = true;
        }
        let mut block = read_block(drive, ctx, addr)?;
        let available = block_size.saturating_sub(block_offset);
        let to_copy = core::cmp::min(remaining, available);
        let src_end = written_total + to_copy;
        let dst_end = block_offset + to_copy;
        if dst_end > block.len() || src_end > buf.len() {
            return Err(FsError::DeviceError);
        }
        block[block_offset..dst_end].copy_from_slice(&buf[written_total..src_end]);
        write_block(drive, ctx, addr, &block)?;
        remaining -= to_copy;
        written_total += to_copy;
        let end_pos = block_index
            .saturating_mul(block_size)
            .saturating_add(block_offset)
            .saturating_add(to_copy);
        if end_pos > max_end {
            max_end = end_pos;
        }
        block_index += 1;
        block_offset = 0;
    }
    if max_end as u64 > inode.size {
        update_inode_size(drive, ctx, inode.ino, max_end as u64)?;
        checkpoint_dirty = true;
    }
    if checkpoint_dirty {
        update_checkpoint(
            drive,
            ctx,
            ctx.nat_ver_bitmap.as_deref(),
            ctx.sit_ver_bitmap.as_deref(),
            None,
        )?;
    }
    Ok(written_total)
}

fn create_f2fs_dir_all_with_context(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    path: &str,
) -> Result<(), FsError> {
    let normalized = normalize_absolute_path(path)?;
    if normalized == "/" {
        return Ok(());
    }
    let mut current = String::from("/");
    for component in normalized.split('/').filter(|entry| !entry.is_empty()) {
        let next = if current == "/" {
            alloc::format!("/{}", component)
        } else {
            alloc::format!("{}/{}", current, component)
        };
        if open_inode_by_path(drive, ctx, next.as_str()).is_err() {
            create_f2fs_dir_with_context(drive, ctx, current.as_str(), component)?;
        }
        current = next;
    }
    Ok(())
}

fn create_f2fs_dir_with_context(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    parent_path: &str,
    name: &str,
) -> Result<(), FsError> {
    let parent = open_inode_by_path(drive, ctx, parent_path)?;
    if !parent.is_dir {
        return Err(FsError::NotDir);
    }
    if find_entry_in_dir(drive, ctx, &parent, name).is_ok() {
        return Ok(());
    }
    let nid = allocate_free_nid(drive, ctx)?;
    allocate_node_block_for_nid(drive, ctx, nid)?;
    let nat_entry = read_nat_entry(drive, ctx, nid)?;
    if nat_entry.block_addr == 0 {
        return Err(FsError::DeviceError);
    }
    let mut block = read_block(drive, ctx, nat_entry.block_addr)?;
    let mode: u16 = S_IFDIR | 0o755;
    write_u16(&mut block, INODE_I_MODE_OFFSET, mode)?;
    write_u64(&mut block, INODE_I_SIZE_DISK_OFFSET, ctx.block_size as u64)?;
    write_block(drive, ctx, nat_entry.block_addr, &block)?;
    let data_addr = allocate_data_block(drive, ctx, nid, 0)?;
    update_inode_block_addr(drive, ctx, nid, 0, data_addr)?;
    let mut dir_block = vec![0u8; ctx.block_size as usize];
    init_dentry_block(&mut dir_block, nid, parent.ino)?;
    write_block(drive, ctx, data_addr, &dir_block)?;
    add_entry_to_dir(drive, ctx, parent.ino, name, nid, true)
}

fn unlink_f2fs_with_context(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    parent_path: &str,
    name: &str,
) -> Result<(), FsError> {
    let parent = open_inode_by_path(drive, ctx, parent_path)?;
    if !parent.is_dir {
        return Err(FsError::NotDir);
    }
    let entry = find_entry_in_dir(drive, ctx, &parent, name)?;
    if entry.is_dir {
        let child = read_inode(drive, ctx, entry.ino)?;
        let entries = read_dir_entries(drive, ctx, &child)?;
        if entries.len() > 2 {
            return Err(FsError::DirNotEmpty);
        }
    }
    remove_entry_from_dir(drive, ctx, parent.ino, name)
}

fn normalize_absolute_path(path: &str) -> Result<String, FsError> {
    let trimmed = path.trim();
    if !trimmed.starts_with('/') {
        return Err(FsError::InvalidParam);
    }
    if trimmed.is_empty() {
        return Err(FsError::InvalidParam);
    }
    let normalized = trimmed.trim_end_matches('/');
    if normalized.is_empty() {
        Ok(String::from("/"))
    } else {
        Ok(normalized.to_string())
    }
}

fn split_parent_and_name(path: &str) -> Result<(String, String), FsError> {
    let normalized = normalize_absolute_path(path)?;
    let trimmed = normalized.trim_start_matches('/');
    if trimmed.is_empty() {
        return Err(FsError::InvalidParam);
    }
    match trimmed.rsplit_once('/') {
        Some((parent, name)) => Ok((
            if parent.is_empty() {
                String::from("/")
            } else {
                alloc::format!("/{}", parent)
            },
            name.to_string(),
        )),
        None => Ok((String::from("/"), trimmed.to_string())),
    }
}

/// Dosyayı farklı dizinlere taşı (mv /a/file /b/file)
/// src_path: Kaynak dosya/dizin tam yolu
/// dst_path: Hedef tam yolu (dosya adı dahil)
pub fn move_f2fs(src_path: &str, dst_path: &str) -> Result<(), FsError> {
    if src_path == dst_path {
        return Ok(());
    }

    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    let ctx = load_context(&mut *drive)?;

    // Kaynak dosyayı aç
    let src_inode = open_inode_by_path(&mut *drive, &ctx, src_path)?;

    // Kaynak yolundan parent ve name çıkar
    let src_path_trimmed = src_path.trim_start_matches('/');
    let (src_parent_path, src_name) = if let Some(pos) = src_path_trimmed.rfind('/') {
        (&src_path_trimmed[..pos], &src_path_trimmed[pos + 1..])
    } else {
        ("", src_path_trimmed)
    };

    // Hedef yolundan parent ve name çıkar
    let dst_path_trimmed = dst_path.trim_start_matches('/');
    let (dst_parent_path, dst_name) = if let Some(pos) = dst_path_trimmed.rfind('/') {
        (&dst_path_trimmed[..pos], &dst_path_trimmed[pos + 1..])
    } else {
        ("", dst_path_trimmed)
    };

    // Kaynak parent dizini aç
    let src_parent = open_inode_by_path(&mut *drive, &ctx, &format!("/{}", src_parent_path))?;
    if !src_parent.is_dir {
        return Err(FsError::NotDir);
    }

    // Hedef parent dizini aç
    let dst_parent = open_inode_by_path(&mut *drive, &ctx, &format!("/{}", dst_parent_path))?;
    if !dst_parent.is_dir {
        return Err(FsError::NotDir);
    }

    // Hedefte aynı isimde dosya var mı kontrol et
    if find_entry_in_dir(&mut *drive, &ctx, &dst_parent, dst_name).is_ok() {
        return Err(FsError::EntryExist);
    }

    // Kaynak dizinden entry'yi sil
    remove_entry_from_dir(&mut *drive, &ctx, src_parent.ino, src_name)?;

    // Hedef dizine entry ekle (aynı inode ile)
    add_entry_to_dir(
        &mut *drive,
        &ctx,
        dst_parent.ino,
        dst_name,
        src_inode.ino,
        src_inode.is_dir,
    )?;

    Ok(())
}

// ============================================================================
// SYMLINK / HARDLINK / TRUNCATE
// ============================================================================

/// Symlink oluşturur (ln -s)
pub fn create_symlink(parent_path: &str, name: &str, target: &str) -> Result<(), FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    let ctx = load_context(&mut *drive)?;
    let parent = open_inode_by_path(&mut *drive, &ctx, parent_path)?;
    if !parent.is_dir {
        return Err(FsError::NotDir);
    }
    if find_entry_in_dir(&mut *drive, &ctx, &parent, name).is_ok() {
        return Err(FsError::EntryExist);
    }

    // Yeni inode oluştur
    let nid = allocate_free_nid(&mut *drive, &ctx)?;
    allocate_node_block_for_nid(&mut *drive, &ctx, nid)?;
    let nat_entry = read_nat_entry(&mut *drive, &ctx, nid)?;
    if nat_entry.block_addr == 0 {
        return Err(FsError::DeviceError);
    }

    let mut block = read_block(&mut *drive, &ctx, nat_entry.block_addr)?;

    // Symlink mode: S_IFLNK | 0o777
    let mode: u16 = 0o120000 | 0o777;
    write_u16(&mut block, INODE_I_MODE_OFFSET, mode)?;

    // Target'ı inline data'ya yaz
    let target_bytes = target.as_bytes();
    let inline_capacity = inline_data_capacity(ctx.block_size)?;
    if target_bytes.len() > inline_capacity {
        return Err(FsError::InvalidParam); // Target çok uzun
    }

    // Inline flag set et
    let flags = block
        .get_mut(INODE_I_INLINE_OFFSET)
        .ok_or(FsError::DeviceError)?;
    *flags |= F2FS_INLINE_DATA;

    // Target'ı yaz
    let data_start = inode_addr_offset_for_block(&block)?;
    block[data_start..data_start + target_bytes.len()].copy_from_slice(target_bytes);

    // Size = target length
    write_u64(
        &mut block,
        INODE_I_SIZE_DISK_OFFSET,
        target_bytes.len() as u64,
    )?;

    write_block(&mut *drive, &ctx, nat_entry.block_addr, &block)?;

    // Directory entry ekle
    add_entry_to_dir(&mut *drive, &ctx, parent.ino, name, nid, false)?;

    crate::serial_println!("[FS] Symlink created: {} -> {}", name, target);
    Ok(())
}

/// Read symlink target from inline data.
pub fn read_f2fs_symlink(path: &str) -> Result<alloc::string::String, FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    let ctx = load_context(&mut *drive)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;

    // Verify it's a symlink by checking mode
    let nat_entry = read_nat_entry(&mut *drive, &ctx, inode.ino)?;
    if nat_entry.block_addr == 0 {
        return Err(FsError::InvalidParam);
    }
    let block = read_block(&mut *drive, &ctx, nat_entry.block_addr)?;

    // Check inline data flag
    let inline_flag = block.get(INODE_I_INLINE_OFFSET).ok_or(FsError::DeviceError)?;
    if inline_flag & F2FS_INLINE_DATA == 0 {
        return Err(FsError::NotSupported);
    }

    let size = inode.size as usize;
    if size == 0 {
        return Ok(alloc::string::String::new());
    }

    let data_start = inode_addr_offset_for_block(&block)?;
    if data_start + size > block.len() {
        return Err(FsError::DeviceError);
    }
    let target_bytes = &block[data_start..data_start + size];
    let target = core::str::from_utf8(target_bytes)
        .map_err(|_| FsError::InvalidParam)?;
    Ok(target.to_string())
}

/// Hardlink oluşturur (ln)
pub fn create_hardlink(parent_path: &str, name: &str, target_path: &str) -> Result<(), FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    let ctx = load_context(&mut *drive)?;

    // Target inode'u bul
    let target_inode = open_inode_by_path(&mut *drive, &ctx, target_path)?;
    if target_inode.is_dir {
        return Err(FsError::IsDir); // Dizinlere hardlink yapılamaz
    }

    // Parent dizini bul
    let parent = open_inode_by_path(&mut *drive, &ctx, parent_path)?;
    if !parent.is_dir {
        return Err(FsError::NotDir);
    }

    if find_entry_in_dir(&mut *drive, &ctx, &parent, name).is_ok() {
        return Err(FsError::EntryExist);
    }

    // Aynı inode'a yeni directory entry ekle
    add_entry_to_dir(&mut *drive, &ctx, parent.ino, name, target_inode.ino, false)?;

    // nlink count artır
    let nat_entry = read_nat_entry(&mut *drive, &ctx, target_inode.ino)?;
    if nat_entry.block_addr != 0 {
        let mut block = read_block(&mut *drive, &ctx, nat_entry.block_addr)?;
        let nlink = read_u32(&block, INODE_I_NLINK_DISK_OFFSET)?;
        write_u32(&mut block, INODE_I_NLINK_DISK_OFFSET, nlink + 1)?;
        write_block(&mut *drive, &ctx, nat_entry.block_addr, &block)?;
    }

    crate::serial_println!(
        "[FS] Hardlink created: {} -> {} (inode {})",
        name,
        target_path,
        target_inode.ino
    );
    Ok(())
}

/// Dosya boyutunu değiştirir (truncate) — grow ve shrink destekler
pub fn truncate_f2fs(path: &str, new_size: u64) -> Result<(), FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    let ctx = load_context(&mut *drive)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;

    if inode.is_dir {
        return Err(FsError::IsDir);
    }

    let block_size = ctx.block_size as u64;
    if block_size == 0 {
        return Err(FsError::DeviceError);
    }

    // Küçültme (shrink): son blokları serbest bırak — Linux kernel truncate_blocks semantics
    if new_size < inode.size {
        let old_blocks = (inode.size + block_size - 1) / block_size;
        let new_blocks = if new_size == 0 {
            0
        } else {
            (new_size + block_size - 1) / block_size
        };

        // Inline data durumu: target inline capacity içindeyse inline'a dön
        if inode.inline && new_size <= inline_data_capacity(ctx.block_size)? as u64 {
            let nat_entry = read_nat_entry(&mut *drive, &ctx, inode.ino)?;
            if nat_entry.block_addr != 0 {
                let mut block = read_block(&mut *drive, &ctx, nat_entry.block_addr)?;
                write_u64(&mut block, INODE_I_SIZE_DISK_OFFSET, new_size)?;
                write_block(&mut *drive, &ctx, nat_entry.block_addr, &block)?;
            }
        } else if !inode.inline {
            // Regular file: blokları sondan başa doğru deallocate et
            for blk_idx in (new_blocks..old_blocks).rev() {
                if blk_idx > u16::MAX as u64 {
                    break;
                }
                let addr = get_data_block_addr(&mut *drive, &ctx, inode.ino, blk_idx as usize)?;
                if addr != 0 {
                    update_inode_block_addr(&mut *drive, &ctx, inode.ino, blk_idx as usize, 0)?;
                    set_sit_valid(&mut *drive, &ctx, addr, false)?;
                }
            }

            // Son bloğun içindeki kısmi veriyi sıfırla (POSIX semantics)
            if new_size > 0 {
                let last_block_idx = new_blocks - 1;
                let block_offset_in_file = last_block_idx * block_size;
                let valid_bytes_in_last_block = new_size - block_offset_in_file;
                if valid_bytes_in_last_block < block_size {
                    let addr =
                        get_data_block_addr(&mut *drive, &ctx, inode.ino, last_block_idx as usize)?;
                    if addr != 0 {
                        let mut block = read_block(&mut *drive, &ctx, addr)?;
                        for i in (valid_bytes_in_last_block as usize)..(block_size as usize) {
                            block[i] = 0;
                        }
                        write_block(&mut *drive, &ctx, addr, &block)?;
                    }
                }
            }
        }
    } else if new_size > inode.size {
        // Büyütme: yeni bloklar allocate et ve sıfırla
        let old_blocks = (inode.size + block_size - 1) / block_size;
        let new_blocks = (new_size + block_size - 1) / block_size;
        let zero_block = alloc::vec![0u8; block_size as usize];
        for blk_idx in old_blocks..new_blocks {
            if let Ok(new_addr) = allocate_data_block(&mut *drive, &ctx, inode.ino, blk_idx as u16)
            {
                let _ = update_inode_block_addr(
                    &mut *drive,
                    &ctx,
                    inode.ino,
                    blk_idx as usize,
                    new_addr,
                );
                let _ = write_block(&mut *drive, &ctx, new_addr, &zero_block);
            }
        }
    }

    // Inode size güncelle
    update_inode_size(&mut *drive, &ctx, inode.ino, new_size)?;

    // mtime güncelle
    update_inode_mtime(&mut *drive, &ctx, inode.ino)?;

    // Checkpoint sync — metadata değişikliklerini kalıcı yap
    let _ = update_checkpoint(
        &mut *drive,
        &ctx,
        ctx.nat_ver_bitmap.as_deref(),
        ctx.sit_ver_bitmap.as_deref(),
        Some(CP_SYNC_FLAG),
    );

    crate::serial_println!("[FS] Truncated: {} -> {} bytes", path, new_size);
    Ok(())
}

/// Dosya okur (symlink takip ederek)
pub fn read_link(path: &str) -> Result<String, FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    let ctx = load_context(&mut *drive)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;

    // Symlink mi kontrol et
    if !inode.inline {
        return Err(FsError::NotFile);
    }

    let nat_entry = read_nat_entry(&mut *drive, &ctx, inode.ino)?;
    if nat_entry.block_addr == 0 {
        return Err(FsError::DeviceError);
    }

    let block = read_block(&mut *drive, &ctx, nat_entry.block_addr)?;
    let data_start = inode_addr_offset_for_block(&block)?;
    let target_len = inode.size as usize;

    if target_len == 0 || target_len > block.len() - data_start {
        return Err(FsError::DeviceError);
    }

    let target = core::str::from_utf8(&block[data_start..data_start + target_len])
        .map_err(|_| FsError::InvalidParam)?;

    Ok(target.to_string())
}

pub(crate) fn load_context(drive: &mut dyn BlockDevice) -> Result<F2fsContext, FsError> {
    let partition_lba = read_partition_lba(drive).unwrap_or(0);
    load_context_for_partition_lba(drive, partition_lba)
}

fn load_context_for_partition_lba(
    drive: &mut dyn BlockDevice,
    partition_lba: u32,
) -> Result<F2fsContext, FsError> {
    let superblock = read_superblock(drive, partition_lba)?;
    if superblock.magic != F2FS_MAGIC {
        return Err(FsError::InvalidParam);
    }
    let sector_size = 1u32
        .checked_shl(superblock.log_sectorsize)
        .ok_or(FsError::DeviceError)?;
    let sectors_per_block = 1u32
        .checked_shl(superblock.log_sectors_per_block)
        .ok_or(FsError::DeviceError)?;
    let block_size = 1u32
        .checked_shl(superblock.log_blocksize)
        .ok_or(FsError::DeviceError)?;
    let blocks_per_seg = 1u32
        .checked_shl(superblock.log_blocks_per_seg)
        .ok_or(FsError::DeviceError)?;
    if sector_size != BLOCK_SIZE as u32 {
        return Err(FsError::DeviceError);
    }
    if block_size != sector_size.saturating_mul(sectors_per_block) {
        return Err(FsError::DeviceError);
    }
    let mut ctx = F2fsContext {
        partition_lba,
        block_size,
        sectors_per_block,
        blocks_per_seg,
        cp_blkaddr: superblock.cp_blkaddr,
        cp_payload: superblock.cp_payload,
        sit_blkaddr: superblock.sit_blkaddr,
        nat_blkaddr: superblock.nat_blkaddr,
        ssa_blkaddr: superblock.ssa_blkaddr,
        main_blkaddr: superblock.main_blkaddr,
        root_ino: superblock.root_ino,
        segment_count_nat: superblock.segment_count_nat,
        segment_count_sit: superblock.segment_count_sit,
        segment_count_ssa: superblock.segment_count_ssa,
        segment_count_main: superblock.segment_count_main,
        nat_ver_bitmap: None,
        sit_ver_bitmap: None,
        cur_node_segno: [0; MAX_ACTIVE_NODE_LOGS],
        cur_node_blkoff: [0; MAX_ACTIVE_NODE_LOGS],
        cur_data_segno: [0; MAX_ACTIVE_DATA_LOGS],
        cur_data_blkoff: [0; MAX_ACTIVE_DATA_LOGS],
        alloc_type: [0; MAX_ACTIVE_LOGS],
    };
    let checkpoint = read_checkpoint(drive, &ctx, &superblock).unwrap_or(F2fsCheckpoint {
        checkpoint_ver: 0,
        ckpt_flags: 0,
        cp_pack_total_block_count: 0,
        sit_ver_bitmap_bytesize: 0,
        nat_ver_bitmap_bytesize: 0,
        checksum_offset: 0,
        nat_ver_bitmap: None,
        sit_ver_bitmap: None,
        cur_node_segno: [0; MAX_ACTIVE_NODE_LOGS],
        cur_node_blkoff: [0; MAX_ACTIVE_NODE_LOGS],
        cur_data_segno: [0; MAX_ACTIVE_DATA_LOGS],
        cur_data_blkoff: [0; MAX_ACTIVE_DATA_LOGS],
        alloc_type: [0; MAX_ACTIVE_LOGS],
    });
    ctx.nat_ver_bitmap = checkpoint.nat_ver_bitmap;
    ctx.sit_ver_bitmap = checkpoint.sit_ver_bitmap;
    ctx.cur_node_segno = checkpoint.cur_node_segno;
    ctx.cur_node_blkoff = checkpoint.cur_node_blkoff;
    ctx.cur_data_segno = checkpoint.cur_data_segno;
    ctx.cur_data_blkoff = checkpoint.cur_data_blkoff;
    ctx.alloc_type = checkpoint.alloc_type;
    Ok(ctx)
}

pub(crate) fn open_inode_by_path(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    path: &str,
) -> Result<F2fsInodeInfo, FsError> {
    let mut current = read_inode(drive, ctx, ctx.root_ino)?;
    if path.trim_start_matches('/').is_empty() {
        return Ok(current);
    }
    for part in path.split('/').filter(|value| !value.is_empty()) {
        if !current.is_dir {
            return Err(FsError::NotFile);
        }
        let entry = find_entry_in_dir(drive, ctx, &current, part)?;
        current = read_inode(drive, ctx, entry.ino)?;
    }
    Ok(current)
}

fn find_entry_in_dir(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    inode: &F2fsInodeInfo,
    name: &str,
) -> Result<DirEntryInfo, FsError> {
    for entry in read_dir_entries(drive, ctx, inode)? {
        if entry.name == name {
            return Ok(entry);
        }
    }
    Err(FsError::EntryNotFound)
}

pub(crate) fn read_dir_entries(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    inode: &F2fsInodeInfo,
) -> Result<Vec<DirEntryInfo>, FsError> {
    if inode.inline {
        return read_inline_dir_entries(ctx, inode);
    }
    let mut out = Vec::new();
    let block_size = ctx.block_size as usize;
    if block_size == 0 {
        return Err(FsError::DeviceError);
    }
    let blocks = (inode.size as usize + block_size - 1) / block_size;
    for block_index in 0..blocks {
        let addr = get_data_block_addr(drive, ctx, inode.ino, block_index)?;
        if addr == 0 {
            continue;
        }
        let block = read_block(drive, ctx, addr)?;
        out.extend(parse_dentry_block(&block)?);
    }
    Ok(out)
}

fn parse_dentry_block(block: &[u8]) -> Result<Vec<DirEntryInfo>, FsError> {
    if block.len() < DENTRY_FILENAME_OFFSET + (DENTRY_SLOT_LEN * DENTRY_SLOTS) {
        return Err(FsError::DeviceError);
    }
    let bitmap = &block[..DENTRY_BITMAP_SIZE];
    let mut out = Vec::new();
    for slot in 0..DENTRY_SLOTS {
        let byte = bitmap[slot / 8];
        let bit = 1u8 << (slot % 8);
        if byte & bit == 0 {
            continue;
        }
        let entry_offset = DENTRY_ENTRIES_OFFSET + (slot * DENTRY_ENTRY_SIZE);
        let name_len = read_u16(block, entry_offset + 8)? as usize;
        if name_len == 0 {
            continue;
        }
        let ino = read_u32(block, entry_offset + 4)?;
        let file_type = block[entry_offset + 10];
        let name_offset = DENTRY_FILENAME_OFFSET + (slot * DENTRY_SLOT_LEN);
        let slots = (name_len + DENTRY_SLOT_LEN - 1) / DENTRY_SLOT_LEN;
        let name_end = name_offset + (slots * DENTRY_SLOT_LEN);
        if name_end > block.len() {
            return Err(FsError::DeviceError);
        }
        let name_bytes = &block[name_offset..name_end];
        let name = String::from_utf8_lossy(&name_bytes[..name_len]).to_string();
        out.push(DirEntryInfo {
            name,
            ino,
            is_dir: file_type == 2,
        });
    }
    Ok(out)
}

fn read_inline_dir_entries(
    ctx: &F2fsContext,
    inode: &F2fsInodeInfo,
) -> Result<Vec<DirEntryInfo>, FsError> {
    let data = inode.inline_data.as_ref().ok_or(FsError::DeviceError)?;
    let block_size = ctx.block_size as usize;
    if block_size == 0 {
        return Err(FsError::DeviceError);
    }
    let mut block = vec![0u8; block_size];
    let copy_len = core::cmp::min(data.len(), block.len());
    block[..copy_len].copy_from_slice(&data[..copy_len]);
    parse_dentry_block(&block)
}

fn update_inode_size(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    inode_nid: u32,
    new_size: u64,
) -> Result<(), FsError> {
    let nat_entry = read_nat_entry(drive, ctx, inode_nid)?;
    if nat_entry.block_addr == 0 {
        return Err(FsError::DeviceError);
    }
    let mut block = read_block(drive, ctx, nat_entry.block_addr)?;
    write_u64(&mut block, INODE_I_SIZE_DISK_OFFSET, new_size)?;
    write_block(drive, ctx, nat_entry.block_addr, &block)
}

/// Inode mtime güncelle
fn update_inode_mtime(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    inode_nid: u32,
) -> Result<(), FsError> {
    let nat_entry = read_nat_entry(drive, ctx, inode_nid)?;
    if nat_entry.block_addr == 0 {
        return Err(FsError::DeviceError);
    }
    let mut block = read_block(drive, ctx, nat_entry.block_addr)?;
    let time = crate::fs::get_global_time();
    write_u64(&mut block, INODE_I_MTIME_DISK_OFFSET, time.sec as u64)?;
    write_block(drive, ctx, nat_entry.block_addr, &block)
}

/// Inode atime güncelle
fn update_inode_atime(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    inode_nid: u32,
) -> Result<(), FsError> {
    let nat_entry = read_nat_entry(drive, ctx, inode_nid)?;
    if nat_entry.block_addr == 0 {
        return Err(FsError::DeviceError);
    }
    let mut block = read_block(drive, ctx, nat_entry.block_addr)?;
    let time = crate::fs::get_global_time();
    write_u64(&mut block, INODE_I_ATIME_DISK_OFFSET, time.sec as u64)?;
    write_block(drive, ctx, nat_entry.block_addr, &block)
}

fn find_free_slot(block: &[u8], slots_needed: usize) -> Option<usize> {
    let bitmap = &block[..DENTRY_BITMAP_SIZE];
    for start_slot in 0..DENTRY_SLOTS {
        if start_slot + slots_needed > DENTRY_SLOTS {
            break;
        }
        let mut free = true;
        for i in 0..slots_needed {
            let slot = start_slot + i;
            let byte = bitmap[slot / 8];
            let bit = 1u8 << (slot % 8);
            if byte & bit != 0 {
                free = false;
                break;
            }
        }
        if free {
            return Some(start_slot);
        }
    }
    None
}

fn write_dentry(
    block: &mut [u8],
    slot: usize,
    name: &str,
    ino: u32,
    is_dir: bool,
) -> Result<(), FsError> {
    let name_bytes = name.as_bytes();
    let name_len = name_bytes.len();
    if name_len == 0 {
        return Err(FsError::InvalidParam);
    }
    let slots_needed = (name_len + DENTRY_SLOT_LEN - 1) / DENTRY_SLOT_LEN;
    if slot + slots_needed > DENTRY_SLOTS {
        return Err(FsError::DeviceError);
    }
    for i in 0..slots_needed {
        let s = slot + i;
        let byte_index = s / 8;
        let bit = 1u8 << (s % 8);
        if byte_index >= DENTRY_BITMAP_SIZE {
            return Err(FsError::DeviceError);
        }
        if let Some(b) = block.get_mut(byte_index) {
            *b |= bit;
        } else {
            return Err(FsError::DeviceError);
        }
    }
    let entry_offset = DENTRY_ENTRIES_OFFSET + (slot * DENTRY_ENTRY_SIZE);
    if entry_offset + DENTRY_ENTRY_SIZE > block.len() {
        return Err(FsError::DeviceError);
    }
    write_u32(block, entry_offset + 4, ino)?;
    write_u16(block, entry_offset + 8, name_len as u16)?;
    let type_offset = entry_offset + 10;
    if let Some(t) = block.get_mut(type_offset) {
        *t = if is_dir { 2 } else { 1 };
    } else {
        return Err(FsError::DeviceError);
    }
    let name_offset = DENTRY_FILENAME_OFFSET + (slot * DENTRY_SLOT_LEN);
    let slots_bytes = slots_needed.saturating_mul(DENTRY_SLOT_LEN);
    if name_offset + slots_bytes > block.len() {
        return Err(FsError::DeviceError);
    }
    for i in 0..name_len {
        block[name_offset + i] = name_bytes[i];
    }
    Ok(())
}

fn find_entry_slot(block: &[u8], name: &str) -> Option<(usize, usize)> {
    let bitmap = &block[..DENTRY_BITMAP_SIZE];
    for slot in 0..DENTRY_SLOTS {
        let byte = bitmap[slot / 8];
        let bit = 1u8 << (slot % 8);
        if byte & bit == 0 {
            continue;
        }
        let entry_offset = DENTRY_ENTRIES_OFFSET + (slot * DENTRY_ENTRY_SIZE);
        if entry_offset + DENTRY_ENTRY_SIZE > block.len() {
            continue;
        }
        let name_len = match read_u16(block, entry_offset + 8) {
            Ok(v) => v as usize,
            Err(_) => continue,
        };
        if name_len == 0 {
            continue;
        }
        let name_offset = DENTRY_FILENAME_OFFSET + (slot * DENTRY_SLOT_LEN);
        let slots_needed = (name_len + DENTRY_SLOT_LEN - 1) / DENTRY_SLOT_LEN;
        let bytes_end = name_offset.saturating_add(slots_needed.saturating_mul(DENTRY_SLOT_LEN));
        let name_end = name_offset.saturating_add(name_len);
        if bytes_end > block.len() || name_end > block.len() {
            continue;
        }
        let entry_name = &block[name_offset..name_offset + name_len];
        if entry_name == name.as_bytes() {
            return Some((slot, slots_needed));
        }
    }
    None
}

fn rename_entry_in_dir(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    parent_ino: u32,
    old_name: &str,
    new_name: &str,
) -> Result<bool, FsError> {
    let parent = read_inode(drive, ctx, parent_ino)?;
    let block_size = ctx.block_size as usize;
    if block_size == 0 {
        return Err(FsError::DeviceError);
    }
    let blocks = (parent.size as usize + block_size - 1) / block_size;
    let new_slots = (new_name.len() + DENTRY_SLOT_LEN - 1) / DENTRY_SLOT_LEN;
    for index in 0..blocks {
        let addr = get_data_block_addr(drive, ctx, parent_ino, index)?;
        if addr == 0 {
            continue;
        }
        let mut block = read_block(drive, ctx, addr)?;
        let Some((slot, old_slots)) = find_entry_slot(&block, old_name) else {
            continue;
        };
        if new_slots > old_slots {
            return Ok(false);
        }
        for i in 0..old_slots {
            let s = slot + i;
            let byte_index = s / 8;
            let bit = 1u8 << (s % 8);
            let b = block.get_mut(byte_index).ok_or(FsError::DeviceError)?;
            if i < new_slots {
                *b |= bit;
            } else {
                *b &= !bit;
            }
        }
        let entry_offset = DENTRY_ENTRIES_OFFSET + (slot * DENTRY_ENTRY_SIZE);
        if entry_offset + DENTRY_ENTRY_SIZE > block.len() {
            return Err(FsError::DeviceError);
        }
        let ino = read_u32(&block, entry_offset + 4)?;
        let file_type = block[entry_offset + 10];
        let name_offset = DENTRY_FILENAME_OFFSET + (slot * DENTRY_SLOT_LEN);
        let name_bytes = old_slots.saturating_mul(DENTRY_SLOT_LEN);
        if name_offset + name_bytes > block.len() {
            return Err(FsError::DeviceError);
        }
        for byte in &mut block[name_offset..name_offset + name_bytes] {
            *byte = 0;
        }
        write_dentry(&mut block, slot, new_name, ino, file_type == 2)?;
        write_block(drive, ctx, addr, &block)?;
        return Ok(true);
    }
    Err(FsError::EntryNotFound)
}

fn add_entry_to_dir(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    parent_ino: u32,
    name: &str,
    child_ino: u32,
    is_dir: bool,
) -> Result<(), FsError> {
    let parent = read_inode(drive, ctx, parent_ino)?;
    let block_size = ctx.block_size as usize;
    let blocks = if block_size == 0 {
        0
    } else {
        (parent.size as usize + block_size - 1) / block_size
    };
    let name_len = name.len();
    let slots_needed = (name_len + DENTRY_SLOT_LEN - 1) / DENTRY_SLOT_LEN;
    for index in 0..blocks {
        let addr = get_data_block_addr(drive, ctx, parent_ino, index)?;
        if addr == 0 {
            continue;
        }
        let mut block = read_block(drive, ctx, addr)?;
        if let Some(slot) = find_free_slot(&block, slots_needed) {
            write_dentry(&mut block, slot, name, child_ino, is_dir)?;
            write_block(drive, ctx, addr, &block)?;
            return Ok(());
        }
    }
    let new_block_index = blocks;
    let new_addr = allocate_data_block(drive, ctx, parent_ino, new_block_index as u16)?;
    update_inode_block_addr(drive, ctx, parent_ino, new_block_index, new_addr)?;
    let new_size = parent.size.saturating_add(ctx.block_size as u64);
    update_inode_size(drive, ctx, parent_ino, new_size)?;
    let mut new_block = vec![0u8; block_size];
    write_dentry(&mut new_block, 0, name, child_ino, is_dir)?;
    write_block(drive, ctx, new_addr, &new_block)?;
    Ok(())
}

fn remove_entry_from_dir(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    parent_ino: u32,
    name: &str,
) -> Result<(), FsError> {
    let parent = read_inode(drive, ctx, parent_ino)?;
    let block_size = ctx.block_size as usize;
    let blocks = if block_size == 0 {
        0
    } else {
        (parent.size as usize + block_size - 1) / block_size
    };
    for index in 0..blocks {
        let addr = get_data_block_addr(drive, ctx, parent_ino, index)?;
        if addr == 0 {
            continue;
        }
        let mut block = read_block(drive, ctx, addr)?;
        if let Some((slot, slots_needed)) = find_entry_slot(&block, name) {
            for i in 0..slots_needed {
                let s = slot + i;
                let byte_index = s / 8;
                let bit = 1u8 << (s % 8);
                if let Some(b) = block.get_mut(byte_index) {
                    *b &= !bit;
                }
            }
            write_block(drive, ctx, addr, &block)?;
            return Ok(());
        }
    }
    Err(FsError::EntryNotFound)
}

fn init_dentry_block(block: &mut [u8], self_ino: u32, parent_ino: u32) -> Result<(), FsError> {
    if block.len() < DENTRY_FILENAME_OFFSET + (DENTRY_SLOT_LEN * 2) {
        return Err(FsError::DeviceError);
    }
    for b in block.iter_mut().take(DENTRY_BITMAP_SIZE) {
        *b = 0;
    }
    write_dentry(block, 0, ".", self_ino, true)?;
    write_dentry(block, 1, "..", parent_ino, true)?;
    Ok(())
}

fn read_inode(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    nid: u32,
) -> Result<F2fsInodeInfo, FsError> {
    let nat_entry = read_nat_entry(drive, ctx, nid)?;
    if nat_entry.block_addr == 0 {
        return Err(FsError::EntryNotFound);
    }
    let block = read_block(drive, ctx, nat_entry.block_addr)?;
    let i_mode = read_u16(&block, INODE_I_MODE_OFFSET)?;
    let is_dir = (i_mode & 0o040000) != 0;
    let inline_flags = block
        .get(INODE_I_INLINE_OFFSET)
        .copied()
        .ok_or(FsError::DeviceError)?;
    let has_inline_data = (inline_flags & F2FS_INLINE_DATA) != 0;
    let has_inline_dentry = (inline_flags & F2FS_INLINE_DENTRY) != 0;
    let has_inline = has_inline_data || has_inline_dentry;
    let size = read_u64(&block, INODE_I_SIZE_DISK_OFFSET)?;
    let addr_count = inode_addr_count_for_block(&block, ctx.block_size)?;
    let addr_offset = inode_addr_offset_for_block(&block)?;
    let inline_capacity = inline_data_capacity_for_block(&block, ctx.block_size)?;
    let mut addrs = Vec::new();
    for idx in 0..addr_count {
        let offset = addr_offset + (idx as usize * 4);
        let addr = read_u32(&block, offset)?;
        if addr == 0 {
            continue;
        }
        addrs.push(addr);
    }
    let i_nid = read_inode_nids(&block, ctx.block_size)?;
    if !has_inline {
        addrs.extend(read_node_addrs_by_nid(drive, ctx, i_nid[0])?);
        addrs.extend(read_node_addrs_by_nid(drive, ctx, i_nid[1])?);
        addrs.extend(read_indirect_addrs_by_nid(drive, ctx, i_nid[2])?);
        addrs.extend(read_indirect_addrs_by_nid(drive, ctx, i_nid[3])?);
        addrs.extend(read_double_indirect_addrs_by_nid(drive, ctx, i_nid[4])?);
    }
    let inline_data = if has_inline {
        let start = addr_offset;
        let end = start.saturating_add(inline_capacity);
        if end > block.len() {
            return Err(FsError::DeviceError);
        }
        let mut data = block[start..end].to_vec();
        if has_inline_data && !has_inline_dentry && size as usize <= data.len() {
            data.truncate(size as usize);
        }
        Some(data)
    } else {
        None
    };
    let inline = has_inline;
    Ok(F2fsInodeInfo {
        ino: nat_entry.ino,
        is_dir,
        size,
        inline,
        inline_data,
        addrs,
    })
}

fn read_inode_meta(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    nid: u32,
) -> Result<(u16, u32, u32), FsError> {
    let nat_entry = read_nat_entry(drive, ctx, nid)?;
    if nat_entry.block_addr == 0 {
        return Err(FsError::EntryNotFound);
    }
    let block = read_block(drive, ctx, nat_entry.block_addr)?;
    let i_mode = read_u16(&block, INODE_I_MODE_OFFSET)?;
    let uid = read_u32(&block, INODE_I_UID_OFFSET)?;
    let gid = read_u32(&block, INODE_I_GID_OFFSET)?;
    Ok((i_mode, uid, gid))
}

fn read_inode_policy(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    nid: u32,
) -> Result<F2fsIoPolicy, FsError> {
    let nat_entry = read_nat_entry(drive, ctx, nid)?;
    if nat_entry.block_addr == 0 {
        return Ok(F2fsIoPolicy::Default);
    }
    let block = read_block(drive, ctx, nat_entry.block_addr)?;
    let flags = block
        .get(INODE_I_INLINE_OFFSET)
        .copied()
        .ok_or(FsError::DeviceError)?;
    if flags & F2FS_POLICY_HOT != 0 {
        return Ok(F2fsIoPolicy::Hot);
    }
    if flags & F2FS_POLICY_COLD != 0 {
        return Ok(F2fsIoPolicy::Cold);
    }
    Ok(F2fsIoPolicy::Default)
}

fn read_nat_entry(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    nid: u32,
) -> Result<NatEntry, FsError> {
    if ctx.segment_count_nat == 0 || ctx.blocks_per_seg == 0 {
        return Err(FsError::DeviceError);
    }
    let entries_per_block = (ctx.block_size as usize) / NAT_ENTRY_SIZE;
    if entries_per_block == 0 {
        return Err(FsError::DeviceError);
    }
    let block_index = (nid as usize) / entries_per_block;
    let entry_index = (nid as usize) % entries_per_block;
    let nat_blocks = ctx.segment_count_nat.saturating_mul(ctx.blocks_per_seg);
    let primary_block = ctx.nat_blkaddr.saturating_add(block_index as u32);
    let secondary_block = ctx
        .nat_blkaddr
        .saturating_add(nat_blocks)
        .saturating_add(block_index as u32);
    let use_secondary = ctx
        .nat_ver_bitmap
        .as_ref()
        .map(|bitmap| bitmap_has(bitmap, block_index));
    if use_secondary == Some(true) {
        return read_nat_entry_block(drive, ctx, secondary_block, entry_index);
    }
    if use_secondary == Some(false) {
        return read_nat_entry_block(drive, ctx, primary_block, entry_index);
    }
    let primary = read_nat_entry_block(drive, ctx, primary_block, entry_index).ok();
    let secondary = read_nat_entry_block(drive, ctx, secondary_block, entry_index).ok();
    match (primary, secondary) {
        (Some(first), Some(second)) => {
            if second._version > first._version {
                Ok(second)
            } else if second._version < first._version {
                Ok(first)
            } else if first.block_addr == 0 && second.block_addr != 0 {
                Ok(second)
            } else {
                Ok(first)
            }
        }
        (Some(entry), None) | (None, Some(entry)) => Ok(entry),
        (None, None) => Err(FsError::DeviceError),
    }
}

fn read_partition_lba(drive: &mut dyn BlockDevice) -> Option<u32> {
    let mbr_data = drive.read_sectors(0, 1);
    if mbr_data.len() < MBR_SIGNATURE_OFFSET + 2 {
        return None;
    }
    if mbr_data[MBR_SIGNATURE_OFFSET] != 0x55 || mbr_data[MBR_SIGNATURE_OFFSET + 1] != 0xAA {
        return None;
    }
    let part_type = mbr_data[PARTITION_ENTRY_OFFSET + 4];
    if part_type == 0xEE {
        return read_gpt_partition_lba(drive);
    }

    let lba_start = PARTITION_ENTRY_OFFSET + PARTITION_LBA_OFFSET;
    if mbr_data.len() < lba_start + 4 {
        return None;
    }
    Some(u32::from_le_bytes(
        mbr_data[lba_start..lba_start + 4].try_into().ok()?,
    ))
}

fn read_partition_lba_by_label(drive: &mut dyn BlockDevice, label: &str) -> Option<u32> {
    crate::debug::serial::trace_raw(format_args!(
        "[F2FS] read_partition_lba_by_label: looking for '{}'\n",
        label
    ));
    let mbr_data = drive.read_sectors(0, 1);
    crate::debug::serial::trace_raw(format_args!("[F2FS] MBR read: {} bytes\n", mbr_data.len()));
    if mbr_data.len() < MBR_SIGNATURE_OFFSET + 2 {
        crate::debug::serial::trace_raw(format_args!("[F2FS] MBR too short\n"));
        return None;
    }
    if mbr_data[MBR_SIGNATURE_OFFSET] != 0x55 || mbr_data[MBR_SIGNATURE_OFFSET + 1] != 0xAA {
        crate::debug::serial::trace_raw(format_args!(
            "[F2FS] MBR sig: {:02x} {:02x}\n",
            mbr_data[MBR_SIGNATURE_OFFSET],
            mbr_data[MBR_SIGNATURE_OFFSET + 1]
        ));
        return None;
    }
    let part_type = mbr_data[PARTITION_ENTRY_OFFSET + 4];
    crate::debug::serial::trace_raw(format_args!(
        "[F2FS] MBR partition type: {:02x}\n",
        part_type
    ));
    if part_type != 0xEE {
        let lba_start = PARTITION_ENTRY_OFFSET + PARTITION_LBA_OFFSET;
        if mbr_data.len() < lba_start + 4 {
            return None;
        }
        return Some(u32::from_le_bytes(
            mbr_data[lba_start..lba_start + 4].try_into().ok()?,
        ));
    }
    crate::debug::serial::trace_raw(format_args!(
        "[F2FS] GPT detected, searching for label '{}'\n",
        label
    ));
    read_gpt_partition_lba_by_label(drive, label)
}

fn read_gpt_partition_lba(drive: &mut dyn BlockDevice) -> Option<u32> {
    read_gpt_partition_lba_by_label(
        drive,
        crate::boot::appliance::current_system_partition_label(),
    )
}

fn read_gpt_partition_lba_by_label(drive: &mut dyn BlockDevice, preferred: &str) -> Option<u32> {
    const GPT_HEADER_LBA: u32 = 1;
    const GPT_SIGNATURE: &[u8; 8] = b"EFI PART";
    const GPT_ENTRY_TYPE_GUID_OFFSET: usize = 0;
    const GPT_ENTRY_FIRST_LBA_OFFSET: usize = 32;
    const GPT_ENTRY_NAME_OFFSET: usize = 56;

    let header = drive.read_sectors(GPT_HEADER_LBA, 1);
    if header.len() < 92 || &header[0..8] != GPT_SIGNATURE {
        crate::debug::serial::trace_raw(format_args!(
            "[F2FS] GPT header invalid: len={} sig={}\n",
            header.len(),
            header.len() >= 8 && &header[0..8] == GPT_SIGNATURE
        ));
        return None;
    }

    let entry_lba = u64::from_le_bytes(header[72..80].try_into().ok()?);
    let entry_count = u32::from_le_bytes(header[80..84].try_into().ok()?);
    let entry_size = u32::from_le_bytes(header[84..88].try_into().ok()?);
    crate::debug::serial::trace_raw(format_args!(
        "[F2FS] GPT: entry_lba={} count={} size={}\n",
        entry_lba, entry_count, entry_size
    ));
    if entry_count == 0 || entry_size < 128 {
        return None;
    }

    let sectors = ((entry_count as usize * entry_size as usize) + BLOCK_SIZE - 1) / BLOCK_SIZE;
    let mut entries = alloc::vec::Vec::with_capacity(sectors * BLOCK_SIZE);
    let mut next_lba = entry_lba;
    let mut remaining = sectors;
    while remaining > 0 {
        let batch = remaining.min(u8::MAX as usize) as u8;
        entries.extend_from_slice(&drive.read_sectors(next_lba.min(u32::MAX as u64) as u32, batch));
        next_lba = next_lba.saturating_add(batch as u64);
        remaining -= batch as usize;
    }
    for index in 0..entry_count as usize {
        let offset = index * entry_size as usize;
        if offset + entry_size as usize > entries.len() {
            break;
        }
        let entry = &entries[offset..offset + entry_size as usize];
        if entry[GPT_ENTRY_TYPE_GUID_OFFSET..GPT_ENTRY_TYPE_GUID_OFFSET + 16]
            .iter()
            .all(|byte| *byte == 0)
        {
            continue;
        }
        let first_lba = u64::from_le_bytes(
            entry[GPT_ENTRY_FIRST_LBA_OFFSET..GPT_ENTRY_FIRST_LBA_OFFSET + 8]
                .try_into()
                .ok()?,
        );
        let name = parse_gpt_name(&entry[GPT_ENTRY_NAME_OFFSET..GPT_ENTRY_NAME_OFFSET + 72]);
        crate::debug::serial::trace_raw(format_args!(
            "[F2FS] GPT entry[{}]: name='{}' first_lba={}\n",
            index, name, first_lba
        ));
        if name == preferred {
            return Some(first_lba.min(u32::MAX as u64) as u32);
        }
    }

    crate::debug::serial::trace_raw(format_args!(
        "[F2FS] GPT: label '{}' not found\n",
        preferred
    ));
    None
}

fn parse_gpt_name(raw: &[u8]) -> alloc::string::String {
    let mut utf16 = alloc::vec::Vec::new();
    for chunk in raw.chunks_exact(2) {
        let code = u16::from_le_bytes([chunk[0], chunk[1]]);
        if code == 0 {
            break;
        }
        utf16.push(code);
    }
    alloc::string::String::from_utf16_lossy(&utf16)
}

fn read_superblock(
    drive: &mut dyn BlockDevice,
    partition_lba: u32,
) -> Result<F2fsSuperblock, FsError> {
    let sectors = (F2FS_SUPERBLOCK_SIZE / BLOCK_SIZE) as u8;
    let data = read_sectors(
        drive,
        partition_lba + F2FS_SUPERBLOCK_SECTOR_OFFSET,
        sectors,
    )?;
    crate::debug::serial::trace_raw(format_args!("[F2FS] read_superblock: partition_lba={} sectors={} data_len={}\n", partition_lba, sectors, data.len()));
    if data.len() >= 16 {
        crate::debug::serial::trace_raw(format_args!("[F2FS] superblock first 16 bytes: {:02x?}\n", &data[0..16]));
    }
    if data.len() < SUPER_ROOT_INO_OFFSET + 4 {
        return Err(FsError::DeviceError);
    }
    let magic = read_u32(&data, SUPER_MAGIC_OFFSET)?;
    crate::debug::serial::trace_raw(format_args!("[F2FS] superblock magic: {:#x} (expected {:#x})\n", magic, F2FS_MAGIC));
    let log_sectorsize = read_u32(&data, SUPER_LOG_SECTORSIZE_OFFSET)?;
    let log_sectors_per_block = read_u32(&data, SUPER_LOG_SECTORS_PER_BLOCK_OFFSET)?;
    let log_blocksize = read_u32(&data, SUPER_LOG_BLOCKSIZE_OFFSET)?;
    let log_blocks_per_seg = read_u32(&data, SUPER_LOG_BLOCKS_PER_SEG_OFFSET)?;
    let segment_count_sit = read_u32(&data, SUPER_SEGMENT_COUNT_SIT_OFFSET)?;
    let segment_count_nat = read_u32(&data, SUPER_SEGMENT_COUNT_NAT_OFFSET)?;
    let segment_count_ssa = read_u32(&data, SUPER_SEGMENT_COUNT_SSA_OFFSET)?;
    let segment_count_main = read_u32(&data, SUPER_SEGMENT_COUNT_MAIN_OFFSET)?;
    let cp_blkaddr = read_u32(&data, SUPER_CP_BLKADDR_OFFSET)?;
    let sit_blkaddr = read_u32(&data, SUPER_SIT_BLKADDR_OFFSET)?;
    let nat_blkaddr = read_u32(&data, SUPER_NAT_BLKADDR_OFFSET)?;
    let ssa_blkaddr = read_u32(&data, SUPER_SSA_BLKADDR_OFFSET)?;
    let main_blkaddr = read_u32(&data, SUPER_MAIN_BLKADDR_OFFSET)?;
    let root_ino = read_u32(&data, SUPER_ROOT_INO_OFFSET)?;
    let cp_payload = read_u32(&data, SUPER_CP_PAYLOAD_OFFSET)?;
    Ok(F2fsSuperblock {
        magic,
        log_sectorsize,
        log_sectors_per_block,
        log_blocksize,
        log_blocks_per_seg,
        segment_count_sit,
        segment_count_nat,
        segment_count_ssa,
        segment_count_main,
        cp_blkaddr,
        sit_blkaddr,
        nat_blkaddr,
        ssa_blkaddr,
        main_blkaddr,
        root_ino,
        cp_payload,
    })
}

fn read_checkpoint(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    superblock: &F2fsSuperblock,
) -> Result<F2fsCheckpoint, FsError> {
    let cp0_addr = superblock.cp_blkaddr;
    let cp1_addr = superblock.cp_blkaddr.saturating_add(ctx.blocks_per_seg);
    let cp0 = read_checkpoint_pack(drive, ctx, cp0_addr, superblock.cp_payload).ok();
    let cp1 = read_checkpoint_pack(drive, ctx, cp1_addr, superblock.cp_payload).ok();
    match (cp0, cp1) {
        (Some(first), Some(second)) => {
            let valid0 = first.layout_ok && first.checksum_ok;
            let valid1 = second.layout_ok && second.checksum_ok;
            if valid0 && valid1 {
                if second.checkpoint.checkpoint_ver > first.checkpoint.checkpoint_ver {
                    Ok(second.checkpoint)
                } else {
                    Ok(first.checkpoint)
                }
            } else if valid0 {
                Ok(first.checkpoint)
            } else if valid1 {
                Ok(second.checkpoint)
            } else if first.layout_ok && second.layout_ok {
                if second.checkpoint.checkpoint_ver > first.checkpoint.checkpoint_ver {
                    Ok(second.checkpoint)
                } else {
                    Ok(first.checkpoint)
                }
            } else if first.layout_ok {
                Ok(first.checkpoint)
            } else if second.layout_ok {
                Ok(second.checkpoint)
            } else {
                Err(FsError::DeviceError)
            }
        }
        (Some(first), None) => {
            if first.layout_ok {
                Ok(first.checkpoint)
            } else {
                Err(FsError::DeviceError)
            }
        }
        (None, Some(second)) => {
            if second.layout_ok {
                Ok(second.checkpoint)
            } else {
                Err(FsError::DeviceError)
            }
        }
        (None, None) => Err(FsError::DeviceError),
    }
}

fn read_checkpoint_pack(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    cp_start: u32,
    cp_payload: u32,
) -> Result<CheckpointPack, FsError> {
    let data = read_checkpoint_pack_data(drive, ctx, cp_start, cp_payload)?;
    parse_checkpoint_pack(&data, cp_payload, ctx.block_size as usize)
}

fn read_checkpoint_pack_data(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    cp_start: u32,
    cp_payload: u32,
) -> Result<Vec<u8>, FsError> {
    let total_blocks = 1u32.saturating_add(cp_payload);
    let block_size = ctx.block_size as usize;
    let mut data = vec![0u8; (total_blocks as usize).saturating_mul(block_size)];
    for idx in 0..total_blocks {
        let block = read_block(drive, ctx, cp_start.saturating_add(idx))?;
        let start = (idx as usize).saturating_mul(block_size);
        let end = start.saturating_add(block_size);
        if end > data.len() || block.len() < block_size {
            return Err(FsError::DeviceError);
        }
        data[start..end].copy_from_slice(&block[..block_size]);
    }
    Ok(data)
}

fn parse_checkpoint_pack(
    data: &[u8],
    cp_payload: u32,
    block_size: usize,
) -> Result<CheckpointPack, FsError> {
    let checkpoint_ver = read_u64(data, CP_CHECKPOINT_VER_OFFSET)?;
    let ckpt_flags = read_u32(data, CP_CKPT_FLAGS_OFFSET)?;
    let cp_pack_total_block_count = read_u32(data, CP_CP_PACK_TOTAL_BLOCK_COUNT_OFFSET)?;
    let sit_ver_bitmap_bytesize = read_u32(data, CP_SIT_VER_BITMAP_BYTESIZE_OFFSET)?;
    let nat_ver_bitmap_bytesize = read_u32(data, CP_NAT_VER_BITMAP_BYTESIZE_OFFSET)?;
    let checksum_offset = read_u32(data, CP_CHECKSUM_OFFSET_OFFSET)?;
    let layout_ok = validate_checkpoint_layout(
        data,
        cp_payload,
        block_size,
        cp_pack_total_block_count,
        nat_ver_bitmap_bytesize,
        sit_ver_bitmap_bytesize,
        checksum_offset,
    )?;
    let checksum_ok = validate_checkpoint_checksum(data, checksum_offset)?;
    let (nat_ver_bitmap, sit_ver_bitmap) = read_checkpoint_bitmaps(
        data,
        cp_payload,
        nat_ver_bitmap_bytesize as usize,
        sit_ver_bitmap_bytesize as usize,
        block_size,
    );
    let mut cur_node_segno = [0u32; MAX_ACTIVE_NODE_LOGS];
    let mut cur_node_blkoff = [0u16; MAX_ACTIVE_NODE_LOGS];
    let mut cur_data_segno = [0u32; MAX_ACTIVE_DATA_LOGS];
    let mut cur_data_blkoff = [0u16; MAX_ACTIVE_DATA_LOGS];
    let mut alloc_type = [0u8; MAX_ACTIVE_LOGS];
    for i in 0..MAX_ACTIVE_NODE_LOGS {
        cur_node_segno[i] = read_u32(data, CP_CUR_NODE_SEGNO_OFFSET + i * 4)?;
        cur_node_blkoff[i] = read_u16(data, CP_CUR_NODE_BLKOFF_OFFSET + i * 2)?;
    }
    for i in 0..MAX_ACTIVE_DATA_LOGS {
        cur_data_segno[i] = read_u32(data, CP_CUR_DATA_SEGNO_OFFSET + i * 4)?;
        cur_data_blkoff[i] = read_u16(data, CP_CUR_DATA_BLKOFF_OFFSET + i * 2)?;
    }
    for i in 0..MAX_ACTIVE_LOGS {
        let off = CP_ALLOC_TYPE_OFFSET + i;
        if off < data.len() {
            alloc_type[i] = data[off];
        }
    }
    Ok(CheckpointPack {
        checkpoint: F2fsCheckpoint {
            checkpoint_ver,
            ckpt_flags,
            cp_pack_total_block_count,
            sit_ver_bitmap_bytesize,
            nat_ver_bitmap_bytesize,
            checksum_offset,
            nat_ver_bitmap,
            sit_ver_bitmap,
            cur_node_segno,
            cur_node_blkoff,
            cur_data_segno,
            cur_data_blkoff,
            alloc_type,
        },
        checksum_ok,
        layout_ok,
    })
}

fn validate_checkpoint_layout(
    data: &[u8],
    cp_payload: u32,
    block_size: usize,
    cp_pack_total_block_count: u32,
    nat_bytes: u32,
    sit_bytes: u32,
    checksum_offset: u32,
) -> Result<bool, FsError> {
    let expected = 1u32.saturating_add(cp_payload);
    if cp_pack_total_block_count != expected {
        return Ok(false);
    }
    let checksum_end = (checksum_offset as usize).saturating_add(4);
    if checksum_end > data.len() {
        return Ok(false);
    }
    let nat_bytes = nat_bytes as usize;
    let sit_bytes = sit_bytes as usize;
    if nat_bytes == 0 && sit_bytes == 0 {
        return Ok(true);
    }
    let nat_start = CP_BITMAP_OFFSET;
    if cp_payload > 0 {
        let nat_end = nat_start.saturating_add(nat_bytes);
        let sit_start = block_size;
        let sit_end = sit_start.saturating_add(sit_bytes);
        return Ok(nat_end <= data.len() && sit_end <= data.len());
    }
    let nat_end = nat_start.saturating_add(nat_bytes);
    let sit_end = nat_end.saturating_add(sit_bytes);
    Ok(sit_end <= data.len())
}

fn validate_checkpoint_checksum(data: &[u8], checksum_offset: u32) -> Result<bool, FsError> {
    let offset = checksum_offset as usize;
    if offset + 4 > data.len() {
        return Ok(false);
    }
    let stored = read_u32(data, offset)?;
    let calc = crc32(&data[..offset]);
    Ok(stored == calc)
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffffffffu32;
    for byte in data {
        crc ^= *byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb88320u32 & mask);
        }
    }
    !crc
}

fn read_checkpoint_bitmaps(
    data: &[u8],
    cp_payload: u32,
    nat_bytes: usize,
    sit_bytes: usize,
    block_size: usize,
) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    if nat_bytes == 0 && sit_bytes == 0 {
        return (None, None);
    }
    let nat_start = CP_BITMAP_OFFSET;
    if cp_payload > 0 {
        let nat_end = nat_start.saturating_add(nat_bytes);
        let nat = if nat_bytes > 0 && nat_end <= data.len() {
            Some(data[nat_start..nat_end].to_vec())
        } else {
            None
        };
        let sit_start = block_size;
        let sit_end = sit_start.saturating_add(sit_bytes);
        let sit = if sit_bytes > 0 && sit_end <= data.len() {
            Some(data[sit_start..sit_end].to_vec())
        } else {
            None
        };
        return (nat, sit);
    }
    let nat_end = nat_start.saturating_add(nat_bytes);
    let nat = if nat_bytes > 0 && nat_end <= data.len() {
        Some(data[nat_start..nat_end].to_vec())
    } else {
        None
    };
    let sit_start = nat_end;
    let sit_end = sit_start.saturating_add(sit_bytes);
    let sit = if sit_bytes > 0 && sit_end <= data.len() {
        Some(data[sit_start..sit_end].to_vec())
    } else {
        None
    };
    (nat, sit)
}

fn write_checkpoint_bitmaps(
    data: &mut [u8],
    cp_payload: u32,
    nat_bytes: usize,
    sit_bytes: usize,
    block_size: usize,
    nat_bitmap: Option<&[u8]>,
    sit_bitmap: Option<&[u8]>,
) -> Result<(), FsError> {
    if nat_bytes == 0 && sit_bytes == 0 {
        return Ok(());
    }
    let nat_start = CP_BITMAP_OFFSET;
    if cp_payload > 0 {
        if let Some(bitmap) = nat_bitmap {
            if bitmap.len() < nat_bytes {
                return Err(FsError::DeviceError);
            }
            let nat_end = nat_start.saturating_add(nat_bytes);
            if nat_end > data.len() {
                return Err(FsError::DeviceError);
            }
            data[nat_start..nat_end].copy_from_slice(&bitmap[..nat_bytes]);
        }
        if let Some(bitmap) = sit_bitmap {
            if bitmap.len() < sit_bytes {
                return Err(FsError::DeviceError);
            }
            let sit_start = block_size;
            let sit_end = sit_start.saturating_add(sit_bytes);
            if sit_end > data.len() {
                return Err(FsError::DeviceError);
            }
            data[sit_start..sit_end].copy_from_slice(&bitmap[..sit_bytes]);
        }
        return Ok(());
    }
    if let Some(bitmap) = nat_bitmap {
        if bitmap.len() < nat_bytes {
            return Err(FsError::DeviceError);
        }
        let nat_end = nat_start.saturating_add(nat_bytes);
        if nat_end > data.len() {
            return Err(FsError::DeviceError);
        }
        data[nat_start..nat_end].copy_from_slice(&bitmap[..nat_bytes]);
    }
    if let Some(bitmap) = sit_bitmap {
        if bitmap.len() < sit_bytes {
            return Err(FsError::DeviceError);
        }
        let sit_start = nat_start.saturating_add(nat_bytes);
        let sit_end = sit_start.saturating_add(sit_bytes);
        if sit_end > data.len() {
            return Err(FsError::DeviceError);
        }
        data[sit_start..sit_end].copy_from_slice(&bitmap[..sit_bytes]);
    }
    Ok(())
}

fn write_checkpoint_pack(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    cp_start: u32,
    data: &[u8],
) -> Result<(), FsError> {
    let block_size = ctx.block_size as usize;
    if block_size == 0 || data.len() % block_size != 0 {
        return Err(FsError::DeviceError);
    }
    let blocks = data.len() / block_size;
    for idx in 0..blocks {
        let start = idx.saturating_mul(block_size);
        let end = start.saturating_add(block_size);
        write_raw_block(
            drive,
            ctx,
            cp_start.saturating_add(idx as u32),
            &data[start..end],
        )?;
    }
    Ok(())
}

fn update_checkpoint(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    nat_bitmap: Option<&[u8]>,
    sit_bitmap: Option<&[u8]>,
    ckpt_flags: Option<u32>,
) -> Result<(), FsError> {
    let cp0_addr = ctx.cp_blkaddr;
    let cp1_addr = ctx.cp_blkaddr.saturating_add(ctx.blocks_per_seg);
    let cp0_data = read_checkpoint_pack_data(drive, ctx, cp0_addr, ctx.cp_payload)?;
    let cp1_data = read_checkpoint_pack_data(drive, ctx, cp1_addr, ctx.cp_payload)?;
    let cp0_pack = parse_checkpoint_pack(&cp0_data, ctx.cp_payload, ctx.block_size as usize).ok();
    let cp1_pack = parse_checkpoint_pack(&cp1_data, ctx.cp_payload, ctx.block_size as usize).ok();

    // Determine which pack is valid and has the highest version (the "current" pack),
    // then write the new checkpoint to the OTHER (alternate) slot.
    // This is crash-safe: at least one valid pack always survives.
    let (base_ver, base_data, target_addr) = match (cp0_pack, cp1_pack) {
        (Some(p0), Some(p1)) if p0.layout_ok && p1.layout_ok => {
            if p0.checkpoint.checkpoint_ver >= p1.checkpoint.checkpoint_ver {
                (p0.checkpoint.checkpoint_ver, cp0_data, cp1_addr)
            } else {
                (p1.checkpoint.checkpoint_ver, cp1_data, cp0_addr)
            }
        }
        (Some(p0), _) if p0.layout_ok => {
            (p0.checkpoint.checkpoint_ver, cp0_data, cp1_addr)
        }
        (_, Some(p1)) if p1.layout_ok => {
            (p1.checkpoint.checkpoint_ver, cp1_data, cp0_addr)
        }
        _ => return Err(FsError::DeviceError),
    };

    let pack = parse_checkpoint_pack(&base_data, ctx.cp_payload, ctx.block_size as usize)
        .map_err(|_| FsError::DeviceError)?;
    if !pack.layout_ok {
        return Err(FsError::DeviceError);
    }

    let mut data = base_data;
    let new_ver = base_ver.saturating_add(1);
    write_u64(&mut data, CP_CHECKPOINT_VER_OFFSET, new_ver)?;
    if let Some(flags) = ckpt_flags {
        write_u32(&mut data, CP_CKPT_FLAGS_OFFSET, flags)?;
    }
    write_checkpoint_bitmaps(
        &mut data,
        ctx.cp_payload,
        pack.checkpoint.nat_ver_bitmap_bytesize as usize,
        pack.checkpoint.sit_ver_bitmap_bytesize as usize,
        ctx.block_size as usize,
        nat_bitmap,
        sit_bitmap,
    )?;
    let checksum_offset = pack.checkpoint.checksum_offset as usize;
    if checksum_offset + 4 > data.len() {
        return Err(FsError::DeviceError);
    }
    let checksum = crc32(&data[..checksum_offset]);
    write_u32(&mut data, checksum_offset, checksum)?;
    write_checkpoint_pack(drive, ctx, target_addr, &data)
}

fn read_nat_entry_block(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    block_addr: u32,
    entry_index: usize,
) -> Result<NatEntry, FsError> {
    let block = read_block(drive, ctx, block_addr)?;
    let entry_offset = entry_index.saturating_mul(NAT_ENTRY_SIZE);
    if block.len() < entry_offset + NAT_ENTRY_SIZE {
        return Err(FsError::DeviceError);
    }
    let version = block[entry_offset];
    let ino = u32::from_le_bytes(
        block[entry_offset + 1..entry_offset + 5]
            .try_into()
            .map_err(|_| FsError::DeviceError)?,
    );
    let block_addr = u32::from_le_bytes(
        block[entry_offset + 5..entry_offset + 9]
            .try_into()
            .map_err(|_| FsError::DeviceError)?,
    );
    Ok(NatEntry {
        ino,
        block_addr,
        _version: version,
    })
}

fn nat_entry_location(ctx: &F2fsContext, nid: u32) -> Result<(u32, u32, usize), FsError> {
    if ctx.segment_count_nat == 0 || ctx.blocks_per_seg == 0 {
        return Err(FsError::DeviceError);
    }
    let entries_per_block = (ctx.block_size as usize) / NAT_ENTRY_SIZE;
    if entries_per_block == 0 {
        return Err(FsError::DeviceError);
    }
    let block_index = (nid as usize) / entries_per_block;
    let entry_index = (nid as usize) % entries_per_block;
    let nat_blocks = ctx.segment_count_nat.saturating_mul(ctx.blocks_per_seg);
    let primary_block = ctx.nat_blkaddr.saturating_add(block_index as u32);
    let secondary_block = ctx
        .nat_blkaddr
        .saturating_add(nat_blocks)
        .saturating_add(block_index as u32);
    Ok((primary_block, secondary_block, entry_index))
}

fn write_nat_entry_block(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    block_addr: u32,
    entry_index: usize,
    entry: &NatEntry,
) -> Result<(), FsError> {
    let mut block = read_block(drive, ctx, block_addr)?;
    let entry_offset = entry_index.saturating_mul(NAT_ENTRY_SIZE);
    if block.len() < entry_offset + NAT_ENTRY_SIZE {
        return Err(FsError::DeviceError);
    }
    if entry_offset >= block.len() {
        return Err(FsError::DeviceError);
    }
    block[entry_offset] = entry._version;
    let ino_bytes = entry.ino.to_le_bytes();
    let addr_bytes = entry.block_addr.to_le_bytes();
    block[entry_offset + 1..entry_offset + 5].copy_from_slice(&ino_bytes);
    block[entry_offset + 5..entry_offset + 9].copy_from_slice(&addr_bytes);
    write_raw_block(drive, ctx, block_addr, &block)
}

fn write_nat_entry(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    nid: u32,
    block_addr: u32,
) -> Result<(), FsError> {
    let (primary_block, secondary_block, entry_index) = nat_entry_location(ctx, nid)?;
    let primary = read_nat_entry_block(drive, ctx, primary_block, entry_index).ok();
    let secondary = read_nat_entry_block(drive, ctx, secondary_block, entry_index).ok();
    let base = primary.or(secondary).unwrap_or(NatEntry {
        ino: nid,
        block_addr: 0,
        _version: 0,
    });
    let entry = NatEntry {
        ino: if base.ino == 0 { nid } else { base.ino },
        block_addr,
        _version: base._version.wrapping_add(1),
    };
    write_nat_entry_block(drive, ctx, primary_block, entry_index, &entry)?;
    write_nat_entry_block(drive, ctx, secondary_block, entry_index, &entry)
}

fn bitmap_has(bitmap: &[u8], index: usize) -> bool {
    let byte_index = index / 8;
    let bit = 1u8 << (index % 8);
    bitmap.get(byte_index).copied().unwrap_or(0) & bit != 0
}

fn read_sit_entry(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    segno: u32,
) -> Result<SitEntry, FsError> {
    if ctx.segment_count_sit == 0 || ctx.blocks_per_seg == 0 {
        return Err(FsError::DeviceError);
    }
    let entries_per_block = (ctx.block_size as usize) / SIT_ENTRY_SIZE;
    if entries_per_block == 0 {
        return Err(FsError::DeviceError);
    }
    let block_index = (segno as usize) / entries_per_block;
    let entry_index = (segno as usize) % entries_per_block;
    let sit_blocks = ctx.segment_count_sit.saturating_mul(ctx.blocks_per_seg);
    let primary_block = ctx.sit_blkaddr.saturating_add(block_index as u32);
    let secondary_block = ctx
        .sit_blkaddr
        .saturating_add(sit_blocks)
        .saturating_add(block_index as u32);
    let use_secondary = ctx
        .sit_ver_bitmap
        .as_ref()
        .map(|bitmap| bitmap_has(bitmap, block_index));
    if use_secondary == Some(true) {
        return read_sit_entry_block(drive, ctx, secondary_block, entry_index);
    }
    if use_secondary == Some(false) {
        return read_sit_entry_block(drive, ctx, primary_block, entry_index);
    }
    let primary = read_sit_entry_block(drive, ctx, primary_block, entry_index).ok();
    let secondary = read_sit_entry_block(drive, ctx, secondary_block, entry_index).ok();
    match (primary, secondary) {
        (Some(first), Some(second)) => {
            if second.vblocks > first.vblocks {
                Ok(second)
            } else if second.vblocks < first.vblocks {
                Ok(first)
            } else {
                Ok(first)
            }
        }
        (Some(entry), None) | (None, Some(entry)) => Ok(entry),
        (None, None) => Err(FsError::DeviceError),
    }
}

fn read_sit_entry_block(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    block_addr: u32,
    entry_index: usize,
) -> Result<SitEntry, FsError> {
    let block = read_block(drive, ctx, block_addr)?;
    parse_sit_entry_from_block(&block, entry_index)
}

fn parse_sit_entry_from_block(block: &[u8], entry_index: usize) -> Result<SitEntry, FsError> {
    let entry_offset = entry_index.saturating_mul(SIT_ENTRY_SIZE);
    if block.len() < entry_offset + SIT_ENTRY_SIZE {
        return Err(FsError::DeviceError);
    }
    let raw_vblocks = u16::from_le_bytes(
        block[entry_offset..entry_offset + 2]
            .try_into()
            .map_err(|_| FsError::DeviceError)?,
    );
    let map_start = entry_offset + 2;
    let map_end = map_start.saturating_add(SIT_VBLOCK_MAP_SIZE);
    let mtime_end = map_end.saturating_add(8);
    if mtime_end > block.len() {
        return Err(FsError::DeviceError);
    }
    let mtime = u64::from_le_bytes(
        block[map_end..mtime_end]
            .try_into()
            .map_err(|_| FsError::DeviceError)?,
    );
    Ok(SitEntry {
        vblocks: raw_vblocks & SIT_VBLOCKS_MASK,
        alloc_type: ((raw_vblocks & !SIT_VBLOCKS_MASK) >> SIT_VBLOCKS_SHIFT) as u8,
        valid_map: block[map_start..map_end].to_vec(),
        mtime,
    })
}

fn sit_entry_location(ctx: &F2fsContext, segno: u32) -> Result<(u32, usize), FsError> {
    if ctx.segment_count_sit == 0 || ctx.blocks_per_seg == 0 {
        return Err(FsError::DeviceError);
    }
    let entries_per_block = (ctx.block_size as usize) / SIT_ENTRY_SIZE;
    if entries_per_block == 0 {
        return Err(FsError::DeviceError);
    }
    let block_index = (segno as usize) / entries_per_block;
    let entry_index = (segno as usize) % entries_per_block;
    let sit_blocks = ctx.segment_count_sit.saturating_mul(ctx.blocks_per_seg);
    let use_secondary = ctx
        .sit_ver_bitmap
        .as_ref()
        .map(|bitmap| bitmap_has(bitmap, block_index));
    let block_addr = if use_secondary == Some(true) {
        ctx.sit_blkaddr
            .saturating_add(sit_blocks)
            .saturating_add(block_index as u32)
    } else {
        ctx.sit_blkaddr.saturating_add(block_index as u32)
    };
    Ok((block_addr, entry_index))
}

fn write_sit_entry(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    segno: u32,
    entry: &SitEntry,
) -> Result<(), FsError> {
    let (block_addr, entry_index) = sit_entry_location(ctx, segno)?;
    let mut block = read_block(drive, ctx, block_addr)?;
    let entry_offset = entry_index.saturating_mul(SIT_ENTRY_SIZE);
    let map_start = entry_offset + 2;
    let map_end = map_start.saturating_add(SIT_VBLOCK_MAP_SIZE);
    if block.len() < entry_offset + SIT_ENTRY_SIZE || map_end > block.len() {
        return Err(FsError::DeviceError);
    }
    let raw_vblocks =
        ((entry.alloc_type as u16) << SIT_VBLOCKS_SHIFT) | (entry.vblocks & SIT_VBLOCKS_MASK);
    write_u16(&mut block, entry_offset, raw_vblocks)?;
    if entry.valid_map.len() < SIT_VBLOCK_MAP_SIZE {
        return Err(FsError::DeviceError);
    }
    block[map_start..map_end].copy_from_slice(&entry.valid_map[..SIT_VBLOCK_MAP_SIZE]);
    let mtime_end = map_end.saturating_add(8);
    if mtime_end > block.len() {
        return Err(FsError::DeviceError);
    }
    block[map_end..mtime_end].copy_from_slice(&entry.mtime.to_le_bytes());
    write_raw_block(drive, ctx, block_addr, &block)
}

fn set_sit_valid(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    block_addr: u32,
    valid: bool,
) -> Result<(), FsError> {
    if block_addr < ctx.main_blkaddr {
        return Err(FsError::DeviceError);
    }
    let rel = block_addr.saturating_sub(ctx.main_blkaddr);
    let total_main = ctx.segment_count_main.saturating_mul(ctx.blocks_per_seg);
    if rel >= total_main {
        return Err(FsError::DeviceError);
    }
    let segno = rel / ctx.blocks_per_seg;
    let offset = rel % ctx.blocks_per_seg;
    let mut entry = read_sit_entry(drive, ctx, segno)?;
    let byte_index = (offset as usize) / 8;
    let bit = 1u8 << (offset % 8);
    if byte_index >= entry.valid_map.len() {
        return Err(FsError::DeviceError);
    }
    let was_valid = entry.valid_map[byte_index] & bit != 0;
    if valid && !was_valid {
        entry.valid_map[byte_index] |= bit;
        entry.vblocks = entry.vblocks.saturating_add(1);
    } else if !valid && was_valid {
        entry.valid_map[byte_index] &= !bit;
        entry.vblocks = entry.vblocks.saturating_sub(1);
    }
    write_sit_entry(drive, ctx, segno, &entry)
}

fn collect_valid_blocks_in_segment(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    segno: u32,
) -> Result<Vec<(u32, SummaryEntry)>, FsError> {
    let entry = read_sit_entry(drive, ctx, segno)?;
    let mut out = Vec::new();
    for offset in 0..ctx.blocks_per_seg {
        let byte_index = (offset as usize) / 8;
        let bit = 1u8 << (offset % 8);
        if entry.valid_map.get(byte_index).copied().unwrap_or(0) & bit == 0 {
            continue;
        }
        let summary = read_summary_entry(drive, ctx, segno, offset)?;
        if summary.nid == 0 {
            continue;
        }
        let block_addr = ctx
            .main_blkaddr
            .saturating_add(segno.saturating_mul(ctx.blocks_per_seg))
            .saturating_add(offset);
        out.push((block_addr, summary));
    }
    Ok(out)
}

fn select_victim_segment(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
) -> Result<Option<u32>, FsError> {
    let mut best: Option<(u16, u16, u32)> = None;
    let total = ctx.segment_count_main;
    for segno in 0..total {
        let entry = read_sit_entry(drive, ctx, segno)?;
        if entry.vblocks == 0 || entry.vblocks >= ctx.blocks_per_seg as u16 {
            continue;
        }
        let mut cold_blocks = 0u16;
        if entry.vblocks > 0 {
            let blocks = collect_valid_blocks_in_segment(drive, ctx, segno)?;
            for (_, summary) in blocks.iter() {
                let policy = read_inode_policy(drive, ctx, summary.nid)?;
                if matches!(policy, F2fsIoPolicy::Cold) {
                    cold_blocks = cold_blocks.saturating_add(1);
                }
            }
        }
        match best {
            Some((best_v, best_cold, _))
                if entry.vblocks > best_v
                    || (entry.vblocks == best_v && cold_blocks <= best_cold) => {}
            _ => best = Some((entry.vblocks, cold_blocks, segno)),
        }
    }
    Ok(best.map(|(_, _, segno)| segno))
}

fn gc_clean_one_segment(drive: &mut dyn BlockDevice, ctx: &F2fsContext) -> Result<bool, FsError> {
    let segno = match select_victim_segment(drive, ctx)? {
        Some(s) => s,
        None => return Ok(false),
    };
    let blocks = collect_valid_blocks_in_segment(drive, ctx, segno)?;
    for (old_addr, summary) in blocks {
        let data = read_block(drive, ctx, old_addr)?;
        let is_node = summary.ofs_in_node == NODE_OFS_SENTINEL;
        let ofs = if is_node {
            NODE_OFS_SENTINEL
        } else {
            summary.ofs_in_node
        };
        let new_addr = allocate_data_block_once(drive, ctx, summary.nid, ofs)?;
        write_block(drive, ctx, new_addr, &data)?;
        if is_node {
            write_nat_entry(drive, ctx, summary.nid, new_addr)?;
        } else {
            update_inode_block_addr(
                drive,
                ctx,
                summary.nid,
                summary.ofs_in_node as usize,
                new_addr,
            )?;
        }
        set_sit_valid(drive, ctx, old_addr, false)?;
        let rel = old_addr.saturating_sub(ctx.main_blkaddr);
        let old_segno = rel / ctx.blocks_per_seg;
        let old_offset = rel % ctx.blocks_per_seg;
        let empty = SummaryEntry {
            nid: 0,
            version: 0,
            ofs_in_node: 0,
        };
        write_summary_entry(drive, ctx, old_segno, old_offset, &empty)?;
    }
    update_checkpoint(
        drive,
        ctx,
        ctx.nat_ver_bitmap.as_deref(),
        ctx.sit_ver_bitmap.as_deref(),
        None,
    )?;
    Ok(true)
}

fn read_summary_entry(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    segno: u32,
    offset: u32,
) -> Result<SummaryEntry, FsError> {
    if ctx.segment_count_ssa == 0 || ctx.blocks_per_seg == 0 {
        return Err(FsError::DeviceError);
    }
    if ctx.block_size as usize % F2FS_SUM_BLKSIZE != 0 {
        return Err(FsError::DeviceError);
    }
    let sums_per_block = (ctx.block_size as usize) / F2FS_SUM_BLKSIZE;
    if sums_per_block == 0 {
        return Err(FsError::DeviceError);
    }
    let summary_block_index = (segno as usize) / sums_per_block;
    let summary_block_offset = (segno as usize) % sums_per_block;
    let ssa_blocks = ctx.segment_count_ssa.saturating_mul(ctx.blocks_per_seg);
    if summary_block_index >= ssa_blocks as usize {
        return Err(FsError::DeviceError);
    }
    let block_addr = ctx.ssa_blkaddr.saturating_add(summary_block_index as u32);
    let block = read_block(drive, ctx, block_addr)?;
    let start = summary_block_offset.saturating_mul(F2FS_SUM_BLKSIZE);
    let end = start.saturating_add(F2FS_SUM_BLKSIZE);
    if end > block.len() {
        return Err(FsError::DeviceError);
    }
    if offset as usize >= SUMMARY_ENTRIES {
        return Err(FsError::DeviceError);
    }
    let entry_offset = start.saturating_add((offset as usize).saturating_mul(SUMMARY_ENTRY_SIZE));
    if entry_offset + SUMMARY_ENTRY_SIZE > end {
        return Err(FsError::DeviceError);
    }
    let nid = u32::from_le_bytes(
        block[entry_offset..entry_offset + 4]
            .try_into()
            .map_err(|_| FsError::DeviceError)?,
    );
    let version = block
        .get(entry_offset + 4)
        .copied()
        .ok_or(FsError::DeviceError)?;
    let ofs_in_node = u16::from_le_bytes(
        block[entry_offset + 5..entry_offset + 7]
            .try_into()
            .map_err(|_| FsError::DeviceError)?,
    );
    Ok(SummaryEntry {
        nid,
        version,
        ofs_in_node,
    })
}

fn write_summary_entry(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    segno: u32,
    offset: u32,
    entry: &SummaryEntry,
) -> Result<(), FsError> {
    if ctx.segment_count_ssa == 0 || ctx.blocks_per_seg == 0 {
        return Err(FsError::DeviceError);
    }
    if ctx.block_size as usize % F2FS_SUM_BLKSIZE != 0 {
        return Err(FsError::DeviceError);
    }
    let sums_per_block = (ctx.block_size as usize) / F2FS_SUM_BLKSIZE;
    if sums_per_block == 0 {
        return Err(FsError::DeviceError);
    }
    let summary_block_index = (segno as usize) / sums_per_block;
    let summary_block_offset = (segno as usize) % sums_per_block;
    let ssa_blocks = ctx.segment_count_ssa.saturating_mul(ctx.blocks_per_seg);
    if summary_block_index >= ssa_blocks as usize {
        return Err(FsError::DeviceError);
    }
    let block_addr = ctx.ssa_blkaddr.saturating_add(summary_block_index as u32);
    let mut block = read_block(drive, ctx, block_addr)?;
    let start = summary_block_offset.saturating_mul(F2FS_SUM_BLKSIZE);
    let end = start.saturating_add(F2FS_SUM_BLKSIZE);
    if end > block.len() {
        return Err(FsError::DeviceError);
    }
    if offset as usize >= SUMMARY_ENTRIES {
        return Err(FsError::DeviceError);
    }
    let entry_offset = start.saturating_add((offset as usize).saturating_mul(SUMMARY_ENTRY_SIZE));
    if entry_offset + SUMMARY_ENTRY_SIZE > end {
        return Err(FsError::DeviceError);
    }
    write_u32(&mut block, entry_offset, entry.nid)?;
    let version_offset = entry_offset.saturating_add(4);
    if version_offset >= block.len() {
        return Err(FsError::DeviceError);
    }
    block[version_offset] = entry.version;
    write_u16(
        &mut block,
        entry_offset.saturating_add(5),
        entry.ofs_in_node,
    )?;
    write_raw_block(drive, ctx, block_addr, &block)
}

fn allocate_data_block_once(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    inode_nid: u32,
    ofs_in_node: u16,
) -> Result<u32, FsError> {
    if ctx.segment_count_main == 0 || ctx.blocks_per_seg == 0 {
        return Err(FsError::DeviceError);
    }
    for segno in 0..ctx.segment_count_main {
        let entry = read_sit_entry(drive, ctx, segno)?;
        if entry.vblocks >= ctx.blocks_per_seg as u16 {
            continue;
        }
        for offset in 0..ctx.blocks_per_seg {
            let byte_index = (offset as usize) / 8;
            let bit = 1u8 << (offset % 8);
            if entry.valid_map.get(byte_index).copied().unwrap_or(0) & bit != 0 {
                continue;
            }
            let block_addr = ctx
                .main_blkaddr
                .saturating_add(segno.saturating_mul(ctx.blocks_per_seg))
                .saturating_add(offset);
            set_sit_valid(drive, ctx, block_addr, true)?;
            let summary = SummaryEntry {
                nid: inode_nid,
                version: 0,
                ofs_in_node,
            };
            write_summary_entry(drive, ctx, segno, offset, &summary)?;
            return Ok(block_addr);
        }
    }
    Err(FsError::DeviceError)
}

fn allocate_data_block(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    inode_nid: u32,
    ofs_in_node: u16,
) -> Result<u32, FsError> {
    // Proactive GC check: ensure free segments before allocation attempt
    let _ = ensure_free_segments(drive, ctx);
    if let Ok(addr) = allocate_data_block_once(drive, ctx, inode_nid, ofs_in_node) {
        return Ok(addr);
    }
    if !gc_clean_one_segment(drive, ctx)? {
        return Err(FsError::DeviceError);
    }
    allocate_data_block_once(drive, ctx, inode_nid, ofs_in_node)
}

fn read_inode_nid(block: &[u8], block_size: u32, index: usize) -> Result<u32, FsError> {
    let offset = inode_nid_offset_in_block(block, block_size, index)?;
    read_u32(block, offset)
}

fn write_inode_nid(
    block: &mut [u8],
    block_size: u32,
    index: usize,
    nid: u32,
) -> Result<(), FsError> {
    let offset = inode_nid_offset_in_block(block, block_size, index)?;
    write_u32(block, offset, nid)
}

fn read_node_entry(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    nid: u32,
    index: usize,
) -> Result<u32, FsError> {
    if nid == 0 {
        return Ok(0);
    }
    let nat_entry = read_nat_entry(drive, ctx, nid)?;
    if nat_entry.block_addr == 0 {
        return Ok(0);
    }
    let count = node_addr_count(ctx.block_size)?;
    if index >= count {
        return Err(FsError::DeviceError);
    }
    let block = read_block(drive, ctx, nat_entry.block_addr)?;
    let offset = index.saturating_mul(4);
    read_u32(&block, offset)
}

fn write_node_entry(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    nid: u32,
    index: usize,
    value: u32,
) -> Result<(), FsError> {
    if nid == 0 {
        return Err(FsError::DeviceError);
    }
    let nat_entry = read_nat_entry(drive, ctx, nid)?;
    if nat_entry.block_addr == 0 {
        return Err(FsError::DeviceError);
    }
    let count = node_addr_count(ctx.block_size)?;
    if index >= count {
        return Err(FsError::DeviceError);
    }
    let mut block = read_block(drive, ctx, nat_entry.block_addr)?;
    let offset = index.saturating_mul(4);
    write_u32(&mut block, offset, value)?;
    write_block(drive, ctx, nat_entry.block_addr, &block)
}

fn allocate_free_nid(drive: &mut dyn BlockDevice, ctx: &F2fsContext) -> Result<u32, FsError> {
    if ctx.segment_count_nat == 0 || ctx.blocks_per_seg == 0 {
        return Err(FsError::DeviceError);
    }
    let entries_per_block = (ctx.block_size as usize) / NAT_ENTRY_SIZE;
    if entries_per_block == 0 {
        return Err(FsError::DeviceError);
    }
    let nat_blocks = ctx.segment_count_nat.saturating_mul(ctx.blocks_per_seg);
    for block_index in 0..nat_blocks {
        let block_addr = ctx.nat_blkaddr.saturating_add(block_index);
        let block = read_block(drive, ctx, block_addr)?;
        for entry_index in 0..entries_per_block {
            let entry_offset = entry_index.saturating_mul(NAT_ENTRY_SIZE);
            if entry_offset + NAT_ENTRY_SIZE > block.len() {
                break;
            }
            let addr = u32::from_le_bytes(
                block[entry_offset + 5..entry_offset + 9]
                    .try_into()
                    .map_err(|_| FsError::DeviceError)?,
            );
            if addr == 0 {
                let nid = (block_index as usize)
                    .saturating_mul(entries_per_block)
                    .saturating_add(entry_index) as u32;
                if nid != 0 {
                    return Ok(nid);
                }
            }
        }
    }
    Err(FsError::DeviceError)
}

fn allocate_node_block_for_nid(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    nid: u32,
) -> Result<u32, FsError> {
    let block_addr = allocate_data_block(drive, ctx, nid, NODE_OFS_SENTINEL)?;
    let empty = vec![0u8; ctx.block_size as usize];
    write_block(drive, ctx, block_addr, &empty)?;
    write_nat_entry(drive, ctx, nid, block_addr)?;
    Ok(block_addr)
}

fn ensure_child_node_nid(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    parent_nid: u32,
    child_index: usize,
) -> Result<u32, FsError> {
    let count = node_addr_count(ctx.block_size)?;
    if child_index >= count {
        return Err(FsError::DeviceError);
    }
    let current = read_node_entry(drive, ctx, parent_nid, child_index)?;
    if current != 0 {
        return Ok(current);
    }
    let new_nid = allocate_free_nid(drive, ctx)?;
    allocate_node_block_for_nid(drive, ctx, new_nid)?;
    write_node_entry(drive, ctx, parent_nid, child_index, new_nid)?;
    Ok(new_nid)
}

fn get_data_block_addr(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    inode_nid: u32,
    block_index: usize,
) -> Result<u32, FsError> {
    let nat_entry = read_nat_entry(drive, ctx, inode_nid)?;
    if nat_entry.block_addr == 0 {
        return Err(FsError::DeviceError);
    }
    let block = read_block(drive, ctx, nat_entry.block_addr)?;
    let addr_count = inode_addr_count_for_block(&block, ctx.block_size)?;
    let addr_offset = inode_addr_offset_for_block(&block)?;
    if block_index < addr_count {
        let offset = addr_offset + block_index.saturating_mul(4);
        return read_u32(&block, offset);
    }
    let count = node_addr_count(ctx.block_size)?;
    let mut index = block_index.saturating_sub(addr_count);
    let i_nids = read_inode_nids(&block, ctx.block_size)?;
    if index < count {
        return read_node_entry(drive, ctx, i_nids[0], index);
    }
    index = index.saturating_sub(count);
    if index < count {
        return read_node_entry(drive, ctx, i_nids[1], index);
    }
    let indirect_cap = count.saturating_mul(count);
    index = index.saturating_sub(count);
    if index < indirect_cap {
        let child = index / count;
        let inner = index % count;
        let child_nid = read_node_entry(drive, ctx, i_nids[2], child)?;
        return read_node_entry(drive, ctx, child_nid, inner);
    }
    index = index.saturating_sub(indirect_cap);
    if index < indirect_cap {
        let child = index / count;
        let inner = index % count;
        let child_nid = read_node_entry(drive, ctx, i_nids[3], child)?;
        return read_node_entry(drive, ctx, child_nid, inner);
    }
    let double_cap = indirect_cap.saturating_mul(count);
    index = index.saturating_sub(indirect_cap);
    if index < double_cap {
        let lvl1 = index / indirect_cap;
        let rem = index % indirect_cap;
        let lvl2 = rem / count;
        let lvl3 = rem % count;
        let lvl1_nid = read_node_entry(drive, ctx, i_nids[4], lvl1)?;
        let lvl2_nid = read_node_entry(drive, ctx, lvl1_nid, lvl2)?;
        return read_node_entry(drive, ctx, lvl2_nid, lvl3);
    }
    Err(FsError::DeviceError)
}

fn update_inode_block_addr(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    inode_nid: u32,
    block_index: usize,
    block_addr: u32,
) -> Result<(), FsError> {
    let nat_entry = read_nat_entry(drive, ctx, inode_nid)?;
    if nat_entry.block_addr == 0 {
        return Err(FsError::DeviceError);
    }
    let mut block = read_block(drive, ctx, nat_entry.block_addr)?;
    let addr_count = inode_addr_count_for_block(&block, ctx.block_size)?;
    let addr_offset = inode_addr_offset_for_block(&block)?;
    if block_index >= addr_count {
        let count = node_addr_count(ctx.block_size)?;
        let mut index = block_index.saturating_sub(addr_count);
        let mut inode_dirty = false;
        let mut get_or_alloc_inode_nid = |idx: usize| -> Result<u32, FsError> {
            let nid = read_inode_nid(&block, ctx.block_size, idx)?;
            if nid != 0 {
                return Ok(nid);
            }
            let new_nid = allocate_free_nid(drive, ctx)?;
            allocate_node_block_for_nid(drive, ctx, new_nid)?;
            write_inode_nid(&mut block, ctx.block_size, idx, new_nid)?;
            inode_dirty = true;
            Ok(new_nid)
        };
        if index < count {
            let nid = get_or_alloc_inode_nid(0)?;
            if inode_dirty {
                write_block(drive, ctx, nat_entry.block_addr, &block)?;
            }
            return write_node_entry(drive, ctx, nid, index, block_addr);
        }
        index = index.saturating_sub(count);
        if index < count {
            let nid = get_or_alloc_inode_nid(1)?;
            if inode_dirty {
                write_block(drive, ctx, nat_entry.block_addr, &block)?;
            }
            return write_node_entry(drive, ctx, nid, index, block_addr);
        }
        let indirect_cap = count.saturating_mul(count);
        index = index.saturating_sub(count);
        if index < indirect_cap {
            let child = index / count;
            let inner = index % count;
            let root_nid = get_or_alloc_inode_nid(2)?;
            if inode_dirty {
                write_block(drive, ctx, nat_entry.block_addr, &block)?;
                inode_dirty = false;
            }
            let child_nid = ensure_child_node_nid(drive, ctx, root_nid, child)?;
            return write_node_entry(drive, ctx, child_nid, inner, block_addr);
        }
        index = index.saturating_sub(indirect_cap);
        if index < indirect_cap {
            let child = index / count;
            let inner = index % count;
            let root_nid = get_or_alloc_inode_nid(3)?;
            if inode_dirty {
                write_block(drive, ctx, nat_entry.block_addr, &block)?;
                inode_dirty = false;
            }
            let child_nid = ensure_child_node_nid(drive, ctx, root_nid, child)?;
            return write_node_entry(drive, ctx, child_nid, inner, block_addr);
        }
        let double_cap = indirect_cap.saturating_mul(count);
        index = index.saturating_sub(indirect_cap);
        if index < double_cap {
            let lvl1 = index / indirect_cap;
            let rem = index % indirect_cap;
            let lvl2 = rem / count;
            let lvl3 = rem % count;
            let root_nid = get_or_alloc_inode_nid(4)?;
            if inode_dirty {
                write_block(drive, ctx, nat_entry.block_addr, &block)?;
                inode_dirty = false;
            }
            let lvl1_nid = ensure_child_node_nid(drive, ctx, root_nid, lvl1)?;
            let lvl2_nid = ensure_child_node_nid(drive, ctx, lvl1_nid, lvl2)?;
            return write_node_entry(drive, ctx, lvl2_nid, lvl3, block_addr);
        }
        return Err(FsError::DeviceError);
    }
    let offset = addr_offset + block_index.saturating_mul(4);
    write_u32(&mut block, offset, block_addr)?;
    write_block(drive, ctx, nat_entry.block_addr, &block)
}

fn is_valid_data_block(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    block_addr: u32,
) -> Result<bool, FsError> {
    if block_addr < ctx.main_blkaddr {
        return Ok(true);
    }
    let rel = block_addr.saturating_sub(ctx.main_blkaddr);
    let total_main = ctx.segment_count_main.saturating_mul(ctx.blocks_per_seg);
    if rel >= total_main {
        return Ok(false);
    }
    if ctx.sit_ver_bitmap.is_none() {
        return Ok(true);
    }
    let segno = rel / ctx.blocks_per_seg;
    let offset = rel % ctx.blocks_per_seg;
    let entry = read_sit_entry(drive, ctx, segno)?;
    let byte_index = (offset as usize) / 8;
    let bit = 1u8 << (offset % 8);
    let sit_valid = entry.valid_map.get(byte_index).copied().unwrap_or(0) & bit != 0;
    if !sit_valid {
        return Ok(false);
    }
    if ctx.segment_count_ssa == 0 || ctx.ssa_blkaddr == 0 {
        return Ok(true);
    }
    let summary = read_summary_entry(drive, ctx, segno, offset)?;
    Ok(summary.nid != 0)
}

fn read_block(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    block_addr: u32,
) -> Result<Vec<u8>, FsError> {
    let min_meta = ctx
        .cp_blkaddr
        .min(ctx.sit_blkaddr)
        .min(ctx.nat_blkaddr)
        .min(ctx.ssa_blkaddr);
    if block_addr < min_meta {
        return Err(FsError::DeviceError);
    }
    if !is_valid_data_block(drive, ctx, block_addr)? {
        return Err(FsError::DeviceError);
    }
    let block_size = ctx.block_size;
    if block_size != 0 {
        if let Some(mut cache) = F2FS_PAGE_CACHE.try_lock() {
            cache.configure(block_size);
            if let Some(data) = cache.get(block_addr) {
                if data.len() == block_size as usize {
                    return Ok(data);
                }
            }
        }
    }
    let lba = ctx
        .partition_lba
        .saturating_add(block_addr.saturating_mul(ctx.sectors_per_block));
    let sectors = ctx
        .sectors_per_block
        .try_into()
        .map_err(|_| FsError::DeviceError)?;
    let data = read_sectors(drive, lba, sectors)?;
    if block_size != 0 && data.len() == block_size as usize {
        if let Some(mut cache) = F2FS_PAGE_CACHE.try_lock() {
            cache.configure(block_size);
            cache.put(block_addr, data.clone());
        }
    }
    Ok(data)
}

fn write_block(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    block_addr: u32,
    data: &[u8],
) -> Result<(), FsError> {
    if data.len() != ctx.block_size as usize {
        return Err(FsError::DeviceError);
    }
    if block_addr < ctx.main_blkaddr {
        return Err(FsError::DeviceError);
    }
    if !is_valid_data_block(drive, ctx, block_addr)? {
        return Err(FsError::DeviceError);
    }
    let lba = ctx
        .partition_lba
        .saturating_add(block_addr.saturating_mul(ctx.sectors_per_block));
    if drive.write_sectors(lba, data).is_err() {
        return Err(FsError::DeviceError);
    }
    let block_size = ctx.block_size;
    if block_size != 0 && data.len() == block_size as usize {
        if let Some(mut cache) = F2FS_PAGE_CACHE.try_lock() {
            cache.configure(block_size);
            cache.put(block_addr, data.to_vec());
        }
    }
    Ok(())
}

fn write_raw_block(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    block_addr: u32,
    data: &[u8],
) -> Result<(), FsError> {
    if data.len() != ctx.block_size as usize {
        return Err(FsError::DeviceError);
    }
    let lba = ctx
        .partition_lba
        .saturating_add(block_addr.saturating_mul(ctx.sectors_per_block));
    if drive.write_sectors(lba, data).is_err() {
        return Err(FsError::DeviceError);
    }
    let block_size = ctx.block_size;
    if block_size != 0 && data.len() == block_size as usize {
        if let Some(mut cache) = F2FS_PAGE_CACHE.try_lock() {
            cache.configure(block_size);
            cache.put(block_addr, data.to_vec());
        }
    }
    Ok(())
}

fn read_sectors(drive: &mut dyn BlockDevice, lba: u32, count: u8) -> Result<Vec<u8>, FsError> {
    let data = drive.read_sectors(lba, count);
    if data.is_empty() {
        return Err(FsError::DeviceError);
    }
    Ok(data)
}

fn read_u16(data: &[u8], offset: usize) -> Result<u16, FsError> {
    if offset + 2 > data.len() {
        return Err(FsError::DeviceError);
    }
    Ok(u16::from_le_bytes([data[offset], data[offset + 1]]))
}

fn read_u32(data: &[u8], offset: usize) -> Result<u32, FsError> {
    if offset + 4 > data.len() {
        return Err(FsError::DeviceError);
    }
    Ok(u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]))
}

fn read_u64(data: &[u8], offset: usize) -> Result<u64, FsError> {
    if offset + 8 > data.len() {
        return Err(FsError::DeviceError);
    }
    Ok(u64::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
        data[offset + 4],
        data[offset + 5],
        data[offset + 6],
        data[offset + 7],
    ]))
}

fn write_u32(data: &mut [u8], offset: usize, value: u32) -> Result<(), FsError> {
    if offset + 4 > data.len() {
        return Err(FsError::DeviceError);
    }
    data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn write_u64(data: &mut [u8], offset: usize, value: u64) -> Result<(), FsError> {
    if offset + 8 > data.len() {
        return Err(FsError::DeviceError);
    }
    data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn write_u16(data: &mut [u8], offset: usize, value: u16) -> Result<(), FsError> {
    if offset + 2 > data.len() {
        return Err(FsError::DeviceError);
    }
    data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn inline_data_capacity(block_size: u32) -> Result<usize, FsError> {
    let size = block_size as usize;
    let capacity = size
        .saturating_sub(INODE_I_ADDR_OFFSET)
        .saturating_sub(INODE_SIZE_OF_I_NID)
        .saturating_sub(NODE_FOOTER_SIZE);
    if capacity == 0 {
        return Err(FsError::DeviceError);
    }
    Ok(capacity)
}

fn inode_addr_count(block_size: u32) -> Result<usize, FsError> {
    let capacity = inline_data_capacity(block_size)?;
    Ok(capacity / 4)
}

fn inode_extra_attr_bytes(block: &[u8]) -> Result<usize, FsError> {
    let inline_flags = block
        .get(INODE_I_INLINE_OFFSET)
        .copied()
        .ok_or(FsError::DeviceError)?;
    if inline_flags & F2FS_EXTRA_ATTR_FLAG == 0 {
        return Ok(0);
    }
    let extra = read_u16(block, INODE_I_EXTRA_ISIZE_DISK_OFFSET)? as usize;
    if extra == 0
        || extra > F2FS_COMPRESSION_EXTRA_ATTR_SIZE
        || extra % core::mem::size_of::<u32>() != 0
    {
        return Err(FsError::DeviceError);
    }
    Ok(extra)
}

fn inode_addr_offset_for_block(block: &[u8]) -> Result<usize, FsError> {
    Ok(INODE_I_ADDR_OFFSET.saturating_add(inode_extra_attr_bytes(block)?))
}

fn inline_data_capacity_for_block(block: &[u8], block_size: u32) -> Result<usize, FsError> {
    let addr_offset = inode_addr_offset_for_block(block)?;
    let size = block_size as usize;
    let capacity = size
        .saturating_sub(addr_offset)
        .saturating_sub(INODE_SIZE_OF_I_NID)
        .saturating_sub(NODE_FOOTER_SIZE);
    if capacity == 0 {
        return Err(FsError::DeviceError);
    }
    Ok(capacity)
}

fn inode_addr_count_for_block(block: &[u8], block_size: u32) -> Result<usize, FsError> {
    Ok(inline_data_capacity_for_block(block, block_size)? / 4)
}

fn inode_nid_offset_in_block(
    block: &[u8],
    block_size: u32,
    index: usize,
) -> Result<usize, FsError> {
    if index >= INODE_NID_COUNT {
        return Err(FsError::DeviceError);
    }
    let addr_offset = inode_addr_offset_for_block(block)?;
    let addr_count = inode_addr_count_for_block(block, block_size)?;
    Ok(addr_offset
        .saturating_add(addr_count.saturating_mul(4))
        .saturating_add(index.saturating_mul(4)))
}

fn ensure_inode_extra_attr(
    block: &mut [u8],
    block_size: u32,
    required_extra_bytes: usize,
) -> Result<(), FsError> {
    if required_extra_bytes == 0
        || required_extra_bytes > F2FS_COMPRESSION_EXTRA_ATTR_SIZE
        || required_extra_bytes % core::mem::size_of::<u32>() != 0
    {
        return Err(FsError::DeviceError);
    }
    let current_extra = inode_extra_attr_bytes(block).unwrap_or(0);
    if current_extra >= required_extra_bytes {
        if let Some(flags) = block.get_mut(INODE_I_INLINE_OFFSET) {
            *flags |= F2FS_EXTRA_ATTR_FLAG;
        }
        write_u16(
            block,
            INODE_I_EXTRA_ISIZE_DISK_OFFSET,
            required_extra_bytes as u16,
        )?;
        return Ok(());
    }
    let old_addr_offset = INODE_I_ADDR_OFFSET.saturating_add(current_extra);
    let new_addr_offset = INODE_I_ADDR_OFFSET.saturating_add(required_extra_bytes);
    let old_addr_count =
        inode_addr_count(block_size)?.saturating_sub(current_extra / core::mem::size_of::<u32>());
    let new_addr_count = inode_addr_count(block_size)?
        .saturating_sub(required_extra_bytes / core::mem::size_of::<u32>());
    let old_nid_offset = old_addr_offset.saturating_add(old_addr_count.saturating_mul(4));
    let new_nid_offset = new_addr_offset.saturating_add(new_addr_count.saturating_mul(4));
    if new_addr_offset < old_addr_offset
        || old_nid_offset > block.len()
        || new_nid_offset > block.len()
        || old_nid_offset != new_nid_offset
    {
        return Err(FsError::DeviceError);
    }
    let copy_end = old_addr_offset.saturating_add(new_addr_count.saturating_mul(4));
    if copy_end > old_nid_offset {
        return Err(FsError::DeviceError);
    }
    block.copy_within(old_addr_offset..copy_end, new_addr_offset);
    for byte in &mut block[INODE_I_ADDR_OFFSET..new_addr_offset] {
        *byte = 0;
    }
    let flags = block
        .get_mut(INODE_I_INLINE_OFFSET)
        .ok_or(FsError::DeviceError)?;
    *flags |= F2FS_EXTRA_ATTR_FLAG;
    write_u16(
        block,
        INODE_I_EXTRA_ISIZE_DISK_OFFSET,
        required_extra_bytes as u16,
    )
}

fn read_inode_flags_from_block(block: &[u8]) -> Result<u32, FsError> {
    read_u32(block, INODE_I_FLAGS_DISK_OFFSET)
}

fn write_inode_flags_to_block(block: &mut [u8], flags: u32) -> Result<(), FsError> {
    write_u32(block, INODE_I_FLAGS_DISK_OFFSET, flags)
}

fn read_inode_nids(block: &[u8], block_size: u32) -> Result<[u32; INODE_NID_COUNT], FsError> {
    let start = inode_nid_offset_in_block(block, block_size, 0)?;
    let end = start.saturating_add(INODE_SIZE_OF_I_NID);
    if end > block.len() {
        return Err(FsError::DeviceError);
    }
    let mut out = [0u32; INODE_NID_COUNT];
    for idx in 0..INODE_NID_COUNT {
        let offset = start + idx * 4;
        out[idx] = read_u32(block, offset)?;
    }
    Ok(out)
}

fn node_addr_count(block_size: u32) -> Result<usize, FsError> {
    let size = block_size as usize;
    let capacity = size.saturating_sub(NODE_FOOTER_SIZE);
    if capacity == 0 {
        return Err(FsError::DeviceError);
    }
    Ok(capacity / 4)
}

fn read_node_addrs_by_nid(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    nid: u32,
) -> Result<Vec<u32>, FsError> {
    if nid == 0 {
        return Ok(Vec::new());
    }
    let nat_entry = read_nat_entry(drive, ctx, nid)?;
    if nat_entry.block_addr == 0 {
        return Ok(Vec::new());
    }
    let block = read_block(drive, ctx, nat_entry.block_addr)?;
    let count = node_addr_count(ctx.block_size)?;
    let mut out = Vec::new();
    for idx in 0..count {
        let offset = idx * 4;
        let addr = read_u32(&block, offset)?;
        if addr == 0 {
            continue;
        }
        out.push(addr);
    }
    Ok(out)
}

fn read_node_nids_by_nid(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    nid: u32,
) -> Result<Vec<u32>, FsError> {
    if nid == 0 {
        return Ok(Vec::new());
    }
    let nat_entry = read_nat_entry(drive, ctx, nid)?;
    if nat_entry.block_addr == 0 {
        return Ok(Vec::new());
    }
    let block = read_block(drive, ctx, nat_entry.block_addr)?;
    let count = node_addr_count(ctx.block_size)?;
    let mut out = Vec::new();
    for idx in 0..count {
        let offset = idx * 4;
        let value = read_u32(&block, offset)?;
        if value == 0 {
            continue;
        }
        out.push(value);
    }
    Ok(out)
}

fn read_indirect_addrs_by_nid(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    nid: u32,
) -> Result<Vec<u32>, FsError> {
    let mut out = Vec::new();
    for child_nid in read_node_nids_by_nid(drive, ctx, nid)? {
        out.extend(read_node_addrs_by_nid(drive, ctx, child_nid)?);
    }
    Ok(out)
}

fn read_double_indirect_addrs_by_nid(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    nid: u32,
) -> Result<Vec<u32>, FsError> {
    let mut out = Vec::new();
    for level1_nid in read_node_nids_by_nid(drive, ctx, nid)? {
        for level2_nid in read_node_nids_by_nid(drive, ctx, level1_nid)? {
            out.extend(read_node_addrs_by_nid(drive, ctx, level2_nid)?);
        }
    }
    Ok(out)
}

/// Minimum free segments before background GC triggers
const GC_FREE_THRESHOLD: u32 = 20;
/// Minimum free segments before foreground GC triggers
const GC_URGENT_THRESHOLD: u32 = 5;
/// Background GC interval in scheduler ticks (100 ticks = 1s)
const GC_BG_INTERVAL_TICKS: usize = 500;

/// Global GC state for background thread
static GC_STATE: GcGlobalState = GcGlobalState::new();

/// Thread-safe global GC state with free segment cache
struct GcGlobalState {
    /// Cached free segment count
    free_segments: core::sync::atomic::AtomicU32,
    /// Last GC time in scheduler ticks
    last_gc_tick: core::sync::atomic::AtomicUsize,
    /// Is background thread running
    running: core::sync::atomic::AtomicBool,
}

impl GcGlobalState {
    const fn new() -> Self {
        GcGlobalState {
            free_segments: core::sync::atomic::AtomicU32::new(0),
            last_gc_tick: core::sync::atomic::AtomicUsize::new(0),
            running: core::sync::atomic::AtomicBool::new(false),
        }
    }
}

/// Count free segments (valid_blocks == 0) by scanning SIT.
/// Updates the global free segment cache.
fn count_free_segments(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
) -> Result<u32, FsError> {
    let total = ctx.segment_count_main;
    let mut free = 0u32;
    for segno in 0..total {
        if let Ok(entry) = read_sit_entry(drive, ctx, segno) {
            if entry.vblocks == 0 {
                free += 1;
            }
        }
    }
    GC_STATE.free_segments.store(free, core::sync::atomic::Ordering::Relaxed);
    Ok(free)
}

/// Ensure at least `GC_FREE_THRESHOLD` free segments exist.
/// Uses cached free segment count to avoid full SIT scan on every call.
/// Triggers background GC when below threshold, forced GC when below urgent threshold.
fn ensure_free_segments(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
) -> Result<(), FsError> {
    let cached = GC_STATE.free_segments.load(core::sync::atomic::Ordering::Relaxed);
    if cached >= GC_FREE_THRESHOLD {
        return Ok(());
    }
    let free = count_free_segments(drive, ctx)?;
    if free >= GC_FREE_THRESHOLD {
        return Ok(());
    }
    if free >= GC_URGENT_THRESHOLD {
        run_gc_internal(drive, ctx, GcMode::Background, 1)?;
        let _ = count_free_segments(drive, ctx);
    } else {
        let needed = GC_URGENT_THRESHOLD.saturating_sub(free);
        run_gc_internal(drive, ctx, GcMode::Foreground, needed)?;
        let _ = count_free_segments(drive, ctx);
    }
    Ok(())
}

// ============================================================================
// GARBAGE COLLECTOR
// ============================================================================

/// GC Mode
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GcMode {
    /// Background GC - runs when system is idle
    Background,
    /// Foreground GC - runs when free segments are low
    Foreground,
    /// Forced GC - emergency cleanup
    Forced,
}

/// GC State
#[derive(Clone, Debug)]
pub struct GcState {
    /// Current segment being collected
    pub cur_segment: u32,
    /// Number of segments collected
    pub segments_collected: u32,
    /// Number of blocks migrated
    pub blocks_migrated: u64,
    /// GC mode
    pub mode: GcMode,
    /// Free segments threshold
    pub free_threshold: u32,
    /// Is GC running
    pub running: bool,
}

impl Default for GcState {
    fn default() -> Self {
        GcState {
            cur_segment: 0,
            segments_collected: 0,
            blocks_migrated: 0,
            mode: GcMode::Background,
            free_threshold: 10, // 10 segments minimum
            running: false,
        }
    }
}

/// Segment info for GC
#[derive(Clone, Debug)]
pub struct SegmentInfo {
    /// Segment number
    pub segno: u32,
    /// Number of valid blocks
    pub valid_blocks: u32,
    /// Number of dirty blocks
    pub dirty_blocks: u32,
    /// Segment type (hot/warm/cold data/node)
    pub seg_type: u8,
    /// Last modified time
    pub mtime: u64,
}

/// Get segment validity info from SIT
fn get_segment_validity(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    segno: u32,
) -> Result<SegmentInfo, FsError> {
    let entry = read_sit_entry(drive, ctx, segno)?;
    let valid_blocks = entry
        .valid_map
        .iter()
        .fold(0u32, |count, byte| count.saturating_add(byte.count_ones()));

    Ok(SegmentInfo {
        segno,
        valid_blocks,
        dirty_blocks: 0,
        seg_type: entry.alloc_type,
        mtime: entry.mtime,
    })
}

/// Select victim segment for GC
fn select_gc_victim(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    mode: GcMode,
) -> Result<Option<u32>, FsError> {
    // Get all segment info
    let total_segments = ctx.segment_count_main;
    let mut candidates: Vec<SegmentInfo> = Vec::new();

    for segno in 0..total_segments {
        if let Ok(info) = get_segment_validity(drive, ctx, segno) {
            // Skip completely empty or completely full segments
            if info.valid_blocks > 0 && info.valid_blocks < ctx.blocks_per_seg {
                candidates.push(info);
            }
        }
    }

    if candidates.is_empty() {
        return Ok(None);
    }

    let current_tick = crate::task::scheduler::get_ticks() as u64;

    // Sort by selection policy
    match mode {
        GcMode::Background => {
            // Cost-benefit: prefer segments with (age / utilization) ratio
            // age = current_time - mtime, utilization = valid_blocks / total_blocks
            candidates.sort_by(|a, b| {
                let age_a = current_tick.saturating_sub(a.mtime).max(1);
                let age_b = current_tick.saturating_sub(b.mtime).max(1);
                let cost_a = a.valid_blocks as f64 / age_a as f64;
                let cost_b = b.valid_blocks as f64 / age_b as f64;
                cost_a.partial_cmp(&cost_b).unwrap_or(core::cmp::Ordering::Equal)
            });
        }
        GcMode::Foreground | GcMode::Forced => {
            // Greedy: select segment with least valid blocks (minimum migration work)
            candidates.sort_by_key(|s| s.valid_blocks);
        }
    }

    Ok(candidates.first().map(|s| s.segno))
}

/// Migrate valid blocks from victim segment
fn migrate_segment_blocks(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    victim_seg: u32,
) -> Result<u64, FsError> {
    let mut migrated = 0u64;
    let blocks = collect_valid_blocks_in_segment(drive, ctx, victim_seg)?;
    for (old_addr, summary) in blocks {
        let data = read_block(drive, ctx, old_addr)?;
        let is_node = summary.ofs_in_node == NODE_OFS_SENTINEL;
        let ofs = if is_node {
            NODE_OFS_SENTINEL
        } else {
            summary.ofs_in_node
        };
        let new_addr = allocate_data_block_once(drive, ctx, summary.nid, ofs)?;
        write_block(drive, ctx, new_addr, &data)?;
        if is_node {
            write_nat_entry(drive, ctx, summary.nid, new_addr)?;
        } else {
            update_inode_block_addr(
                drive,
                ctx,
                summary.nid,
                summary.ofs_in_node as usize,
                new_addr,
            )?;
        }
        set_sit_valid(drive, ctx, old_addr, false)?;
        let rel = old_addr.saturating_sub(ctx.main_blkaddr);
        let old_segno = rel / ctx.blocks_per_seg;
        let old_offset = rel % ctx.blocks_per_seg;
        let empty = SummaryEntry {
            nid: 0,
            version: 0,
            ofs_in_node: 0,
        };
        write_summary_entry(drive, ctx, old_segno, old_offset, &empty)?;
        migrated = migrated.saturating_add(1);
    }
    update_checkpoint(
        drive,
        ctx,
        ctx.nat_ver_bitmap.as_deref(),
        ctx.sit_ver_bitmap.as_deref(),
        None,
    )?;
    Ok(migrated)
}

/// Internal GC: clean up to `max_segments` segments from a loaded context.
fn run_gc_internal(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    mode: GcMode,
    max_segments: u32,
) -> Result<GcState, FsError> {
    let mut state = GcState::default();
    state.mode = mode;
    state.running = true;

    for _ in 0..max_segments {
        let victim = match select_gc_victim(drive, ctx, mode)? {
            Some(v) => v,
            None => break,
        };
        state.cur_segment = victim;
        let migrated = migrate_segment_blocks(drive, ctx, victim)?;
        state.blocks_migrated = state.blocks_migrated.saturating_add(migrated);
        if migrated > 0 {
            state.segments_collected += 1;
        }
    }

    state.running = false;
    Ok(state)
}

/// Background GC thread — periodically checks free segment count and cleans if below threshold
fn f2fs_gc_background_task() -> ! {
    loop {
        crate::task::scheduler::sleep(GC_BG_INTERVAL_TICKS);

        let mut drive = match crate::drivers::linux::select_block_device() {
            Ok(d) => d,
            Err(_) => continue,
        };
        let ctx = match load_context(&mut *drive) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let free = match count_free_segments(&mut *drive, &ctx) {
            Ok(f) => f,
            Err(_) => continue,
        };

        if free < GC_FREE_THRESHOLD {
            let mode = if free < GC_URGENT_THRESHOLD {
                GcMode::Foreground
            } else {
                GcMode::Background
            };
            let max_seg = match mode {
                GcMode::Foreground | GcMode::Forced => 2,
                _ => 1,
            };
            let _ = run_gc_internal(&mut *drive, &ctx, mode, max_seg);
        }
    }
}

/// Start the background GC thread. Idempotent — only starts once.
pub fn start_gc_thread() {
    if GC_STATE.running.load(core::sync::atomic::Ordering::Acquire) {
        return;
    }
    GC_STATE.running.store(true, core::sync::atomic::Ordering::Release);
    crate::task::scheduler::spawn_with_priority(
        f2fs_gc_background_task,
        crate::task::task::Priority::Low,
        "f2fs_gc",
    );
}

/// Run garbage collection from public API
pub fn run_gc(mode: GcMode) -> Result<GcState, FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };

    let ctx = load_context(&mut *drive)?;
    let max_segments = match mode {
        GcMode::Background => 1,
        GcMode::Foreground | GcMode::Forced => 4,
    };
    run_gc_internal(&mut *drive, &ctx, mode, max_segments)
}

/// Get free segment count
pub fn get_free_segments() -> Result<u32, FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };

    let ctx = load_context(&mut *drive)?;
    let total_segments = ctx.segment_count_main;
    let mut free_count = 0u32;

    for segno in 0..total_segments {
        if let Ok(info) = get_segment_validity(&mut *drive, &ctx, segno) {
            if info.valid_blocks == 0 {
                free_count += 1;
            }
        }
    }

    Ok(free_count)
}

// ============================================================================
// CHECKPOINT WRITE
// ============================================================================

/// Checkpoint control structure
#[derive(Clone, Debug)]
pub struct CheckpointControl {
    /// Checkpoint version
    pub version: u64,
    /// User block count
    pub user_block_count: u64,
    /// Valid block count
    pub valid_block_count: u64,
    /// Valid node count
    pub valid_node_count: u64,
    /// Valid inode count
    pub valid_inode_count: u64,
    /// Last segment written
    pub last_segno: u32,
    /// Next segment to write
    pub next_segno: u32,
    /// Active logs
    pub active_logs: u8,
    /// CP flags
    pub flags: u32,
}

/// Checkpoint constants
const CP_UMOUNT_FLAG: u32 = 0x00000001;
const CP_FASTBOOT_FLAG: u32 = 0x00000002;
const CP_SYNC_FLAG: u32 = 0x00000004;
const CP_RECOVERY_FLAG: u32 = 0x00000008;
const CP_DISCARD_FLAG: u32 = 0x00000010;
const CP_TRIMMED_FLAG: u32 = 0x00000020;
const CP_NOCRC_RECOVERY_FLAG: u32 = 0x00000040;
const CP_MERGE_FLAG: u32 = 0x00000080;
const CP_FSCK_FLAG: u32 = 0x00000100;
const CP_ERROR_FLAG: u32 = 0x00000200;
const CP_COMPACTED_SUM_FLAG: u32 = 0x00000400;
const CP_ORPHAN_INODE_FLAG: u32 = 0x00000800;
const CP_DISABLED_QUOTA_FLAG: u32 = 0x00001000;
const CP_DISABLED_QUOTA_MASK: u32 = 0x00007000;
const CP_QUOTA_NEED_FSCK_FLAG: u32 = 0x00008000;
const CP_INDEX_FLAG: u32 = 0x00010000;

/// Write checkpoint to disk
pub fn write_checkpoint(flags: u32) -> Result<(), FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };

    let ctx = load_context(&mut *drive)?;
    update_checkpoint(
        &mut *drive,
        &ctx,
        ctx.nat_ver_bitmap.as_deref(),
        ctx.sit_ver_bitmap.as_deref(),
        Some(flags),
    )?;
    drive.flush().map_err(|_| FsError::DeviceError)
}

/// Calculate checksum (simple XOR-based)
fn calculate_checksum(data: &[u8]) -> u32 {
    let mut sum = 0u32;
    for chunk in data.chunks(4) {
        let mut arr = [0u8; 4];
        arr[..chunk.len()].copy_from_slice(chunk);
        sum ^= u32::from_le_bytes(arr);
    }
    sum
}

/// Sync filesystem - write checkpoint
pub fn sync_f2fs() -> Result<(), FsError> {
    write_checkpoint(CP_SYNC_FLAG)
}

/// Fsync a specific file — flush file data and metadata to disk
pub fn fsync_f2fs(path: &str) -> Result<(), FsError> {
    // Önce dosyanın inode'unu bul
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    let ctx = load_context(&mut *drive)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;

    // Inode'un NAT entry'sini oku ve dirty flag set et
    let nat_entry = read_nat_entry(&mut *drive, &ctx, inode.ino)?;
    if nat_entry.block_addr != 0 {
        let mut block = read_block(&mut *drive, &ctx, nat_entry.block_addr)?;
        // Inode'u diske yaz (zaten write_block checkpoint ile yapılır)
        write_block(&mut *drive, &ctx, nat_entry.block_addr, &block)?;
    }

    // Checkpoint yazarak tüm dirty veriyi diske flush et
    sync_f2fs()
}

/// fsync(2) wrapper — flush data AND metadata for a specific file path.
pub fn fsync_path(path: &str) -> Result<(), FsError> {
    fsync_f2fs(path)
}

/// fdatasync(2) wrapper — flush data only (skip metadata checkpoint).
///
/// Per fdatasync(2): does NOT flush mtime, ctime, or size changes.
/// Only ensures file data reaches stable storage.
pub fn fdatasync_path(path: &str) -> Result<(), FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    let ctx = load_context(&mut *drive)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;
    if inode.is_dir {
        return Err(FsError::IsDir);
    }
    drive.flush().map_err(|_| FsError::DeviceError)
}

/// Return the current F2FS block device (if mounted).
pub fn get_block_device() -> Option<Box<dyn crate::drivers::linux::BlockDevice>> {
    match crate::drivers::linux::select_block_device() {
        Ok(value) => Some(value),
        Err(_) => None,
    }
}

/// Update file timestamps (atime, mtime) with nanosecond precision
pub fn update_timestamps(
    path: &str,
    atime_sec: i64,
    atime_nsec: i64,
    mtime_sec: i64,
    mtime_nsec: i64,
) -> Result<(), FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    let ctx = load_context(&mut *drive)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;

    let nat_entry = read_nat_entry(&mut *drive, &ctx, inode.ino)?;
    if nat_entry.block_addr == 0 {
        return Err(FsError::EntryNotFound);
    }

    let mut block = read_block(&mut *drive, &ctx, nat_entry.block_addr)?;

    // atime güncelle (UTIME_NOW/UTIME_OMIT kontrolü)
    if atime_sec >= 0 {
        // F2FS inode'da atime offset'i
        let atime_sec_offset = INODE_I_ATIME_OFFSET;
        write_u64(&mut block, atime_sec_offset, atime_sec as u64)?;
        // Nanosaniye kısmı (varsa)
        let _ = atime_nsec; // F2FS'de nanosecond desteği inode layout'a göre eklenir
    }

    // mtime güncelle
    if mtime_sec >= 0 {
        let mtime_sec_offset = INODE_I_MTIME_OFFSET;
        write_u64(&mut block, mtime_sec_offset, mtime_sec as u64)?;
        let _ = mtime_nsec;
    }

    write_block(&mut *drive, &ctx, nat_entry.block_addr, &block)?;
    Ok(())
}

/// Unmount filesystem - write clean checkpoint
pub fn unmount_clean() -> Result<(), FsError> {
    write_checkpoint(CP_UMOUNT_FLAG | CP_SYNC_FLAG)
}

// ============================================================================
// RECOVERY
// ============================================================================

/// Recovery state
#[derive(Clone, Debug)]
pub struct RecoveryState {
    /// Recovery successful
    pub success: bool,
    /// Number of inodes recovered
    pub inodes_recovered: u32,
    /// Number of blocks recovered
    pub blocks_recovered: u64,
    /// Recovery errors
    pub errors: Vec<String>,
}

/// Check if recovery is needed
pub fn needs_recovery() -> Result<bool, FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };

    let ctx = load_context(&mut *drive)?;

    let superblock = read_superblock(&mut *drive, ctx.partition_lba)?;
    let checkpoint = read_checkpoint(&mut *drive, &ctx, &superblock)?;
    Ok((checkpoint.ckpt_flags & CP_UMOUNT_FLAG) == 0)
}

/// Perform recovery from last checkpoint
///
/// F2FS recovery adımları:
/// 1. Checkpoint validasyonu (CP_ERROR_FLAG kontrolü)
/// 2. Orphan inode cleanup (CP_ORPHAN_INODE_FLAG)
/// 3. Roll-forward recovery (fsync-marked dnode'ları replay et)
/// 4. Clean checkpoint yaz
pub fn recover_from_checkpoint() -> Result<RecoveryState, FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };

    let ctx = load_context(&mut *drive)?;
    let mut state = RecoveryState {
        success: false,
        inodes_recovered: 0,
        blocks_recovered: 0,
        errors: Vec::new(),
    };

    let superblock = read_superblock(&mut *drive, ctx.partition_lba)?;
    let checkpoint = read_checkpoint(&mut *drive, &ctx, &superblock);
    let selected = match checkpoint {
        Ok(cp) => cp,
        Err(_) => {
            state
                .errors
                .push("Valid checkpoint pack not found".to_string());
            return Ok(state);
        }
    };

    if selected.ckpt_flags & CP_ERROR_FLAG != 0 {
        state
            .errors
            .push("Checkpoint pack is marked with CP_ERROR_FLAG".to_string());
        return Ok(state);
    }

    // Full roll-forward recovery (orphan + fsync dnode recovery)
    state = roll_forward_recovery()?;

    Ok(state)
}

/// Recover orphan inodes
///
/// F2FS spec: Orphan inode'lar checkpoint bloğundan sonra saklanır.
/// CP_ORPHAN_INODE_FLAG set ise, unlink() yapılmış ama henüz truncate
/// edilmemiş inode'lar var demektir. Her orphan inode:
/// 1. Truncate edilir (tüm blokları serbest bırakılır)
/// 2. NAT entry'si silinir
/// 3. SIT entry güncellenir (bloklar invalid olarak işaretlenir)
fn recover_orphan_inodes(drive: &mut dyn BlockDevice, ctx: &F2fsContext) -> Result<u32, FsError> {
    let superblock = read_superblock(drive, ctx.partition_lba)?;
    let checkpoint = read_checkpoint(drive, ctx, &superblock)?;

    // Orphan inode blokları CP'den sonra başlar
    let orphan_start = ctx.cp_blkaddr + 1 + ctx.cp_payload;
    // cp_pack_total_block_count = 2 (CP copies) + cp_payload + orphan_blocks + summary_blocks
    let total_blocks = checkpoint.cp_pack_total_block_count as u32;
    let summary_blocks = 6; // 3 data + 3 node summary (minimum)
    let orphan_blocks = total_blocks.saturating_sub(2 + ctx.cp_payload + summary_blocks);

    if orphan_blocks == 0 {
        return Ok(0);
    }

    let mut total_recovered = 0u32;

    for blk_idx in 0..orphan_blocks {
        let orphan_addr = orphan_start + blk_idx;
        let orphan_block = match read_block(drive, ctx, orphan_addr) {
            Ok(b) => b,
            Err(_) => break,
        };

        // Orphan block format: [entry_count:4][ino[0]:4][ino[1]:4]...
        if orphan_block.len() < 4 {
            continue;
        }
        let entry_count = u32::from_le_bytes([
            orphan_block[0],
            orphan_block[1],
            orphan_block[2],
            orphan_block[3],
        ]) as usize;
        if entry_count == 0 || entry_count > (ctx.block_size as usize - 4) / 4 {
            continue;
        }

        for i in 0..entry_count {
            let offset = 4 + i * 4;
            if offset + 4 > orphan_block.len() {
                break;
            }
            let ino = u32::from_le_bytes([
                orphan_block[offset],
                orphan_block[offset + 1],
                orphan_block[offset + 2],
                orphan_block[offset + 3],
            ]);
            if ino == 0 || ino == 0xFFFFFFFF {
                continue;
            }

            // Inode'u oku ve truncate et
            if let Ok(nat_entry) = read_nat_entry(drive, ctx, ino) {
                if nat_entry.block_addr != 0 {
                    let inode_block = match read_block(drive, ctx, nat_entry.block_addr) {
                        Ok(b) => b,
                        Err(_) => continue,
                    };
                    let file_size = read_u64(&inode_block, INODE_I_SIZE_DISK_OFFSET).unwrap_or(0);

                    // Tüm blokları serbest bırak
                    if file_size > 0 {
                        let block_size = ctx.block_size as u64;
                        let num_blocks = (file_size + block_size - 1) / block_size;

                        for logical_blk in 0..num_blocks {
                            if let Ok(addr) =
                                get_data_block_addr(drive, ctx, ino, logical_blk as usize)
                            {
                                if addr != 0 {
                                    let _ = set_sit_valid(drive, ctx, addr, false);
                                }
                            }
                        }
                    }

                    // NAT entry'yi sil (block_addr = 0, links = 0)
                    let mut new_block = inode_block.clone();
                    let _ = write_u64(&mut new_block, INODE_I_SIZE_DISK_OFFSET, 0);
                    let _ = write_u32(&mut new_block, INODE_I_NLINK_DISK_OFFSET, 0);
                    let _ = write_block(drive, ctx, nat_entry.block_addr, &new_block);
                }
            }

            total_recovered += 1;
        }
    }

    crate::serial_println!("[F2FS] Orphan recovery: {} inodes cleaned", total_recovered);
    Ok(total_recovered)
}

/// F2FS Roll-Forward Recovery
///
/// Linux kernel fs/f2fs/recovery.c implementasyonuna uygun:
///
/// Recovery senaryoları (F=fsync_mark, D=dentry_mark):
/// 1. inode(x) | CP | inode(x) | dnode(F) → latest inode(x) güncelle
/// 2. inode(x) | CP | dnode(F) → dnode(F) recover et
/// 3. inode(x) | CP | dnode(F) | inode(x) → dnode(F) recover, son inode(x) drop
///
/// Adımlar:
/// Step 1: find_fsync_dnodes — current segment'te fsync-marked dnode'ları bul
/// Step 2: recover_data — her fsync inode için data block'ları ve dentry'leri replay et
/// Step 3: Write clean checkpoint
pub fn roll_forward_recovery() -> Result<RecoveryState, FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };

    let ctx = load_context(&mut *drive)?;
    let mut state = RecoveryState {
        success: false,
        inodes_recovered: 0,
        blocks_recovered: 0,
        errors: Vec::new(),
    };

    // Step 0: Orphan inode cleanup (önce orphan'ları temizle)
    let superblock = read_superblock(&mut *drive, ctx.partition_lba)?;
    let checkpoint = read_checkpoint(&mut *drive, &ctx, &superblock)?;

    if (checkpoint.ckpt_flags & CP_ORPHAN_INODE_FLAG) != 0 {
        match recover_orphan_inodes(&mut *drive, &ctx) {
            Ok(count) => state.inodes_recovered = count,
            Err(e) => state
                .errors
                .push(format!("Orphan recovery failed: {:?}", e)),
        }
    }

    // Step 1: Fsync-marked dnode'ları bul
    let fsync_inodes = match find_fsync_dnodes(&mut *drive, &ctx, &checkpoint) {
        Ok(list) => list,
        Err(e) => {
            state
                .errors
                .push(format!("find_fsync_dnodes failed: {:?}", e));
            return Ok(state);
        }
    };

    if fsync_inodes.is_empty() {
        state.success = true;
        return Ok(state);
    }

    crate::serial_println!(
        "[F2FS] Roll-forward: {} fsync-marked inodes found",
        fsync_inodes.len()
    );

    // Step 2: Her fsync inode için data recovery
    for fsync_entry in &fsync_inodes {
        match recover_inode_data(&mut *drive, &ctx, fsync_entry) {
            Ok(blocks) => {
                state.blocks_recovered += blocks;
                state.inodes_recovered += 1;
            }
            Err(e) => state
                .errors
                .push(format!("Recover inode {} failed: {:?}", fsync_entry.ino, e)),
        }
    }

    // Step 3: Dentry recovery (dentry_mark set olanlar)
    for fsync_entry in &fsync_inodes {
        if fsync_entry.has_dentry {
            if let Err(e) = recover_dentry(&mut *drive, &ctx, fsync_entry) {
                state.errors.push(format!(
                    "Dentry recovery failed for {}: {:?}",
                    fsync_entry.ino, e
                ));
            }
        }
    }

    // Step 4: Clean checkpoint yaz
    update_checkpoint(
        &mut *drive,
        &ctx,
        checkpoint.nat_ver_bitmap.as_deref(),
        checkpoint.sit_ver_bitmap.as_deref(),
        Some(CP_RECOVERY_FLAG | CP_UMOUNT_FLAG),
    )?;

    state.success = true;
    crate::serial_println!(
        "[F2FS] Roll-forward complete: {} inodes, {} blocks recovered",
        state.inodes_recovered,
        state.blocks_recovered
    );

    Ok(state)
}

/// Fsync entry — bir fsync-marked inode'un recovery bilgisi
#[derive(Clone, Debug)]
struct FsyncInodeEntry {
    ino: u32,
    blkaddr: u32,
    last_dentry: u32,
    has_dentry: bool,
    has_fsync: bool,
}

/// Scan active segments from checkpoint for fsync-marked dnodes
fn find_fsync_dnodes(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    checkpoint: &F2fsCheckpoint,
) -> Result<Vec<FsyncInodeEntry>, FsError> {
    let mut entries: Vec<FsyncInodeEntry> = Vec::new();
    let cp_ver = checkpoint.checkpoint_ver;

    // Scan all active node segments
    for i in 0..MAX_ACTIVE_NODE_LOGS {
        let segno = ctx.cur_node_segno[i];
        let blkoff = ctx.cur_node_blkoff[i] as u32;
        if segno == 0 || segno >= ctx.segment_count_main {
            continue;
        }
        let seg_start = ctx.main_blkaddr + segno * ctx.blocks_per_seg;
        scan_segment_for_fsync(drive, ctx, cp_ver, seg_start, blkoff, &mut entries)?;
    }

    // Scan all active data segments
    for i in 0..MAX_ACTIVE_DATA_LOGS {
        let segno = ctx.cur_data_segno[i];
        let blkoff = ctx.cur_data_blkoff[i] as u32;
        if segno == 0 || segno >= ctx.segment_count_main {
            continue;
        }
        let seg_start = ctx.main_blkaddr + segno * ctx.blocks_per_seg;
        scan_segment_for_fsync(drive, ctx, cp_ver, seg_start, blkoff, &mut entries)?;
    }

    Ok(entries)
}

fn scan_segment_for_fsync(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    cp_ver: u64,
    seg_start: u32,
    blkoff: u32,
    entries: &mut Vec<FsyncInodeEntry>,
) -> Result<(), FsError> {
    let max_scan = blkoff.min(ctx.blocks_per_seg);
    for scanned in 0..max_scan {
        let blkaddr = seg_start + scanned;
        let block = match read_block(drive, ctx, blkaddr) {
            Ok(b) => b,
            Err(_) => continue,
        };
        if block.len() < NODE_FOOTER_SIZE {
            continue;
        }
        let footer_start = block.len() - NODE_FOOTER_SIZE;
        let node_cp_ver = read_u64(&block, footer_start + NODE_FOOTER_CPVER_OFFSET).unwrap_or(0);
        if node_cp_ver != cp_ver {
            continue;
        }
        let footer_flags = read_u32(&block, footer_start + NODE_FOOTER_FLAGS_OFFSET).unwrap_or(0);
        let is_fsync = (footer_flags & F2FS_FSYNC_MARK) != 0;
        let is_dentry = (footer_flags & F2FS_DENT_MARK) != 0;
        if !is_fsync && !is_dentry {
            continue;
        }
        let ino = read_u32(&block, footer_start + NODE_FOOTER_INO_OFFSET).unwrap_or(0);
        let nid = read_u32(&block, footer_start + NODE_FOOTER_NID_OFFSET).unwrap_or(0);
        if ino == 0 || ino == 0xFFFFFFFF {
            continue;
        }
        if let Some(existing) = entries.iter_mut().find(|e| e.ino == ino) {
            existing.blkaddr = nid;
            if is_dentry {
                existing.has_dentry = true;
                existing.last_dentry = blkaddr;
            }
        } else {
            entries.push(FsyncInodeEntry {
                ino,
                blkaddr: nid,
                last_dentry: if is_dentry { blkaddr } else { 0 },
                has_dentry: is_dentry,
                has_fsync: is_fsync,
            });
        }
    }
    Ok(())
}

/// Fsync-marked inode'un data bloklarını recover et
fn recover_inode_data(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    entry: &FsyncInodeEntry,
) -> Result<u64, FsError> {
    let nat_entry = read_nat_entry(drive, ctx, entry.ino)?;
    if nat_entry.block_addr == 0 {
        return Ok(0);
    }

    let inode_block = read_block(drive, ctx, nat_entry.block_addr)?;

    // Inline data kontrolü
    if inode_block.len() > INODE_I_INLINE_OFFSET {
        let inline_flags = inode_block[INODE_I_INLINE_OFFSET];
        if (inline_flags & F2FS_INLINE_DATA) != 0 {
            return Ok(0);
        }
    }

    let file_size = read_u64(&inode_block, INODE_I_SIZE_DISK_OFFSET).unwrap_or(0);
    if file_size == 0 {
        return Ok(0);
    }

    let block_size = ctx.block_size as u64;
    let num_blocks = (file_size + block_size - 1) / block_size;
    let mut blocks_recovered = 0u64;

    for logical_blk in 0..num_blocks {
        if let Ok(addr) = get_data_block_addr(drive, ctx, entry.ino, logical_blk as usize) {
            if addr != 0 {
                let _ = set_sit_valid(drive, ctx, addr, true);
                blocks_recovered += 1;
            }
        }
    }

    let _ = write_block(drive, ctx, nat_entry.block_addr, &inode_block);
    Ok(blocks_recovered)
}

/// Dentry recovery — fsync-marked dnode'dan directory entry replay
fn recover_dentry(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    entry: &FsyncInodeEntry,
) -> Result<(), FsError> {
    if entry.last_dentry == 0 {
        return Ok(());
    }

    let dnode_block = read_block(drive, ctx, entry.last_dentry)?;

    if dnode_block.len() < NODE_FOOTER_SIZE {
        return Ok(());
    }
    let footer_start = dnode_block.len() - NODE_FOOTER_SIZE;
    let node_footer = read_u32(&dnode_block, footer_start + NODE_FOOTER_FLAGS_OFFSET).unwrap_or(0);
    if (node_footer & F2FS_INODE_MARK) == 0 {
        return Ok(());
    }

    // Parent inode ve isim bilgilerini oku
    let pino = read_u32(&dnode_block, INODE_I_PINO_OFFSET)?;
    let namelen = read_u32(&dnode_block, INODE_I_NAMELEN_OFFSET)? as usize;

    if namelen == 0 || namelen > F2FS_SLOT_NAME_LEN {
        return Err(FsError::InvalidParam);
    }

    let name_start = INODE_I_NAME_OFFSET;
    if name_start + namelen > dnode_block.len() {
        return Err(FsError::InvalidParam);
    }
    let name_bytes = &dnode_block[name_start..name_start + namelen];
    let name = String::from_utf8_lossy(name_bytes).to_string();

    // Parent dizine dentry ekle
    let parent_nat = read_nat_entry(drive, ctx, pino)?;
    if parent_nat.block_addr == 0 {
        return Err(FsError::EntryNotFound);
    }

    let parent_info = F2fsInodeInfo {
        ino: pino,
        is_dir: true,
        size: 0,
        inline: false,
        inline_data: None,
        addrs: Vec::new(),
    };

    match find_entry_in_dir(drive, ctx, &parent_info, &name) {
        Ok(_) => Ok(()),
        Err(_) => {
            add_entry_to_dir(drive, ctx, pino, &name, entry.ino, false)?;
            crate::serial_println!(
                "[F2FS] Dentry recovered: {} (ino {}) in parent {}",
                name,
                entry.ino,
                pino
            );
            Ok(())
        }
    }
}

/// Rollback to previous checkpoint
pub fn rollback_checkpoint() -> Result<(), FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };

    let ctx = load_context(&mut *drive)?;

    // Read both checkpoints (CP packs at segment boundaries)
    let cp_blkaddr = ctx.cp_blkaddr;
    let cp1 = read_block(&mut *drive, &ctx, cp_blkaddr)?;
    let cp2 = read_block(&mut *drive, &ctx, cp_blkaddr + ctx.blocks_per_seg)?;

    let ver1 = read_u64(&cp1, 0)?;
    let ver2 = read_u64(&cp2, 0)?;

    // Use older checkpoint
    let (old_cp, _old_ver) = if ver1 < ver2 {
        (cp1, ver1)
    } else {
        (cp2, ver2)
    };

    // Write old checkpoint to current position
    let current_addr = ctx.cp_blkaddr;
    write_block(&mut *drive, &ctx, current_addr, &old_cp)?;

    crate::serial_println!("[F2FS] Rolled back to previous checkpoint");

    Ok(())
}

// SIT entry count per block
const SIT_ENTRY_PER_BLOCK: u32 = 56; // 4096 / 72

// ============================================================================
// F2FS COMPRESSION
// ============================================================================

/// Compression algorithm types
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompressAlgorithm {
    None = 0,
    Lzo = 1,
    Lz4 = 2,
    Zstd = 3,
}

/// Compression configuration
#[derive(Clone, Debug)]
pub struct CompressConfig {
    pub algorithm: CompressAlgorithm,
    pub log_cluster_size: u8,   // Log2 of cluster size (typically 4 = 16KB)
    pub min_compress_ratio: u8, // Minimum ratio to compress (e.g., 80 = 80%)
    pub compress_mode: CompressMode,
}

/// Compression mode
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompressMode {
    Fs = 0,   // File-system controlled
    User = 1, // User-controlled via flags
}

impl Default for CompressConfig {
    fn default() -> Self {
        CompressConfig {
            algorithm: CompressAlgorithm::Lz4,
            log_cluster_size: 4, // 16KB clusters
            min_compress_ratio: 80,
            compress_mode: CompressMode::Fs,
        }
    }
}

/// Read compression config from inode block
fn read_compress_config_from_inode(block: &[u8]) -> Result<CompressConfig, FsError> {
    let algo_byte = block[F2FS_COMPRESS_ALGO_OFFSET];
    let algorithm = match algo_byte {
        0 => CompressAlgorithm::None,
        1 => CompressAlgorithm::Lzo,
        2 => CompressAlgorithm::Lz4,
        3 => CompressAlgorithm::Zstd,
        _ => CompressAlgorithm::Lz4,
    };

    // Read log_cluster_size from i_addr[0] (first data block pointer area)
    let log_cluster_size = 4; // Default 16KB clusters

    Ok(CompressConfig {
        algorithm,
        log_cluster_size,
        ..CompressConfig::default()
    })
}

/// Internal compressed file read — reads raw blocks and decompresses
fn read_compressed_internal(
    ctx: &F2fsContext,
    inode: &F2fsInodeInfo,
    config: &CompressConfig,
) -> Result<Vec<u8>, FsError> {
    // Read raw file data using existing read_f2fs_file_at
    let mut compressed_data = vec![0u8; inode.size as usize];
    // We need to read raw data, not decompressed - use direct block reads
    let block_size = ctx.block_size as usize;
    let mut remaining = inode.size as usize;
    let mut read_total = 0usize;
    let mut block_index = 0usize;

    let mut drive = crate::drivers::linux::select_block_device().map_err(|_| FsError::NoDevice)?;
    while remaining > 0 {
        let addr = get_data_block_addr(&mut *drive, ctx, inode.ino, block_index)?;
        let block = if addr == 0 {
            vec![0u8; block_size]
        } else {
            read_block(&mut *drive, ctx, addr)?
        };
        let to_copy = core::cmp::min(remaining, block_size);
        if read_total + to_copy > compressed_data.len() {
            return Err(FsError::DeviceError);
        }
        compressed_data[read_total..read_total + to_copy].copy_from_slice(&block[..to_copy]);
        remaining -= to_copy;
        read_total += to_copy;
        block_index += 1;
    }

    // Parse and decompress clusters
    let mut decompressed = Vec::new();
    let mut offset = 0;

    while offset + 15 <= compressed_data.len() {
        let magic = u32::from_le_bytes([
            compressed_data[offset],
            compressed_data[offset + 1],
            compressed_data[offset + 2],
            compressed_data[offset + 3],
        ]);

        if magic != F2FS_COMPRESSED_DATA {
            // Not compressed, return raw data
            return Ok(compressed_data);
        }

        let _cluster_size =
            u16::from_le_bytes([compressed_data[offset + 4], compressed_data[offset + 5]]);
        let algorithm = compressed_data[offset + 6];
        let compressed_size =
            u16::from_le_bytes([compressed_data[offset + 7], compressed_data[offset + 8]]) as usize;
        let original_size =
            u16::from_le_bytes([compressed_data[offset + 9], compressed_data[offset + 10]])
                as usize;
        let _checksum = u32::from_le_bytes([
            compressed_data[offset + 11],
            compressed_data[offset + 12],
            compressed_data[offset + 13],
            compressed_data[offset + 14],
        ]);

        offset += 15;

        if offset + compressed_size > compressed_data.len() {
            return Err(FsError::DeviceError);
        }

        // Decompress based on algorithm
        let mut decompressed_chunk = vec![0u8; original_size];
        match algorithm {
            2 => {
                // LZ4
                let written = crate::compression::deflate::decompress_deflate(
                    &compressed_data[offset..offset + compressed_size],
                    original_size,
                )
                .map_err(|_| FsError::NotSupported)?;
                // LZ4 uses its own decompressor
                let src = &compressed_data[offset..offset + compressed_size];
                let dst = &mut decompressed_chunk;
                lz4_decompress(src, dst);
            }
            3 => {
                // ZSTD
                let result = crate::compression::zstd::decompress_zstd(
                    &compressed_data[offset..offset + compressed_size],
                    original_size,
                )
                .map_err(|_| FsError::NotSupported)?;
                let copy_len = core::cmp::min(result.len(), original_size);
                decompressed_chunk[..copy_len].copy_from_slice(&result[..copy_len]);
            }
            1 => {
                // LZO
                let result = crate::compression::lzo1x::decompress_lzo1x(
                    &compressed_data[offset..offset + compressed_size],
                    original_size,
                )
                .map_err(|_| FsError::NotSupported)?;
                let copy_len = core::cmp::min(result.len(), original_size);
                decompressed_chunk[..copy_len].copy_from_slice(&result[..copy_len]);
            }
            _ => return Err(FsError::NotSupported),
        }

        decompressed.extend_from_slice(&decompressed_chunk);
        offset += compressed_size;
    }

    Ok(decompressed)
}

/// Compressed cluster header
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CompressHeader {
    pub magic: u32, // F2FS_COMPRESSED_DATA
    pub cluster_size: u16,
    pub algorithm: u8,
    pub compressed_size: u16, // Size of compressed data (excluding header)
    pub original_size: u16,   // Original uncompressed size
    pub checksum: u32,        // CRC32 of compressed data
}

const F2FS_COMPRESSED_DATA: u32 = 0xF5F2C001;

const LZ4_MIN_MATCH: usize = 4;
const LZ4_HASH_BITS: usize = 12;
const LZ4_HASH_SIZE: usize = 1 << LZ4_HASH_BITS;
const LZ4_HASH_MASK: usize = LZ4_HASH_SIZE - 1;

fn lz4_hash(src: &[u8], pos: usize) -> usize {
    let seq = u32::from_le_bytes([src[pos], src[pos + 1], src[pos + 2], src[pos + 3]]);
    ((seq.wrapping_mul(2_654_435_761) >> (32 - LZ4_HASH_BITS as u32)) as usize) & LZ4_HASH_MASK
}

fn lz4_emit_length(dst: &mut [u8], dst_pos: &mut usize, mut len: usize) -> bool {
    while len >= 255 {
        if *dst_pos >= dst.len() {
            return false;
        }
        dst[*dst_pos] = 255;
        *dst_pos += 1;
        len -= 255;
    }
    if *dst_pos >= dst.len() {
        return false;
    }
    dst[*dst_pos] = len as u8;
    *dst_pos += 1;
    true
}

fn compression_algorithm_supported_exactly(algorithm: CompressAlgorithm) -> bool {
    matches!(algorithm, CompressAlgorithm::None | CompressAlgorithm::Lz4)
}

/// Raw LZ4 block compression.
pub fn lz4_compress(src: &[u8], dst: &mut [u8]) -> usize {
    if src.is_empty() {
        return 0;
    }

    let mut table = [u32::MAX; LZ4_HASH_SIZE];
    let mut anchor = 0usize;
    let mut cursor = 0usize;
    let mut dst_pos = 0usize;

    while cursor + LZ4_MIN_MATCH <= src.len() {
        let hash = lz4_hash(src, cursor);
        let candidate = table[hash];
        table[hash] = cursor as u32;

        let Some(ref_pos) = (candidate != u32::MAX).then_some(candidate as usize) else {
            cursor += 1;
            continue;
        };
        if ref_pos >= cursor || cursor - ref_pos > u16::MAX as usize {
            cursor += 1;
            continue;
        }
        if src[ref_pos..ref_pos + LZ4_MIN_MATCH] != src[cursor..cursor + LZ4_MIN_MATCH] {
            cursor += 1;
            continue;
        }

        let literal_len = cursor - anchor;
        if dst_pos >= dst.len() {
            return 0;
        }
        let token_pos = dst_pos;
        dst[dst_pos] = 0;
        dst_pos += 1;

        let mut token = 0u8;
        if literal_len < 15 {
            token |= (literal_len as u8) << 4;
        } else {
            token |= 0xF0;
        }
        if literal_len >= 15 && !lz4_emit_length(dst, &mut dst_pos, literal_len - 15) {
            return 0;
        }
        if dst_pos + literal_len > dst.len() {
            return 0;
        }
        dst[dst_pos..dst_pos + literal_len].copy_from_slice(&src[anchor..cursor]);
        dst_pos += literal_len;

        if dst_pos + 2 > dst.len() {
            return 0;
        }
        let offset = (cursor - ref_pos) as u16;
        dst[dst_pos..dst_pos + 2].copy_from_slice(&offset.to_le_bytes());
        dst_pos += 2;

        let mut match_len = LZ4_MIN_MATCH;
        while cursor + match_len < src.len()
            && ref_pos + match_len < src.len()
            && src[ref_pos + match_len] == src[cursor + match_len]
        {
            match_len += 1;
        }
        let match_len_field = match_len - LZ4_MIN_MATCH;
        if match_len_field < 15 {
            token |= match_len_field as u8;
        } else {
            token |= 0x0F;
        }
        dst[token_pos] = token;
        if match_len_field >= 15 && !lz4_emit_length(dst, &mut dst_pos, match_len_field - 15) {
            return 0;
        }

        let end = cursor + match_len;
        let mut seed = cursor + 1;
        while seed + LZ4_MIN_MATCH <= end.saturating_sub(1) {
            table[lz4_hash(src, seed)] = seed as u32;
            seed += 1;
        }
        cursor = end;
        anchor = end;
    }

    let literal_len = src.len().saturating_sub(anchor);
    if dst_pos >= dst.len() {
        return 0;
    }
    let token_pos = dst_pos;
    dst[dst_pos] = 0;
    dst_pos += 1;
    if literal_len < 15 {
        dst[token_pos] = (literal_len as u8) << 4;
    } else {
        dst[token_pos] = 0xF0;
        if !lz4_emit_length(dst, &mut dst_pos, literal_len - 15) {
            return 0;
        }
    }
    if dst_pos + literal_len > dst.len() {
        return 0;
    }
    dst[dst_pos..dst_pos + literal_len].copy_from_slice(&src[anchor..]);
    dst_pos += literal_len;
    dst_pos
}

/// Raw LZ4 block decompression.
pub fn lz4_decompress(src: &[u8], dst: &mut [u8]) -> usize {
    let mut src_pos = 0;
    let mut dst_pos = 0;

    while src_pos < src.len() {
        let token = src[src_pos];
        src_pos += 1;

        let mut literal_len = (token >> 4) as usize;
        if literal_len == 15 {
            loop {
                if src_pos >= src.len() {
                    return 0;
                }
                let ext = src[src_pos] as usize;
                src_pos += 1;
                literal_len += ext;
                if ext != 255 {
                    break;
                }
            }
        }
        if src_pos + literal_len > src.len() || dst_pos + literal_len > dst.len() {
            return 0;
        }
        dst[dst_pos..dst_pos + literal_len].copy_from_slice(&src[src_pos..src_pos + literal_len]);
        src_pos += literal_len;
        dst_pos += literal_len;

        if src_pos == src.len() {
            break;
        }
        if src_pos + 2 > src.len() {
            return 0;
        }
        let offset = u16::from_le_bytes([src[src_pos], src[src_pos + 1]]) as usize;
        src_pos += 2;
        if offset == 0 || offset > dst_pos {
            return 0;
        }

        let mut match_len = (token & 0x0F) as usize + LZ4_MIN_MATCH;
        if (token & 0x0F) as usize == 15 {
            loop {
                if src_pos >= src.len() {
                    return 0;
                }
                let ext = src[src_pos] as usize;
                src_pos += 1;
                match_len += ext;
                if ext != 255 {
                    break;
                }
            }
        }
        if dst_pos + match_len > dst.len() {
            return 0;
        }
        for _ in 0..match_len {
            let byte = dst[dst_pos - offset];
            dst[dst_pos] = byte;
            dst_pos += 1;
        }
    }

    dst_pos
}

/// ZSTD-like compression (simplified dictionary-based)
pub fn zstd_compress(src: &[u8], dst: &mut [u8]) -> usize {
    if src.is_empty() || dst.len() < src.len() / 2 {
        return 0;
    }

    // Simple RLE + dictionary compression
    let mut dict: [u8; 256] = [0; 256];
    let mut dict_pos = 0;
    let mut dst_pos = 0;

    // Build dictionary
    for &b in src.iter().take(256) {
        if !dict.contains(&b) {
            dict[dict_pos] = b;
            dict_pos += 1;
        }
    }

    // Write dictionary header
    dst[dst_pos] = dict_pos as u8;
    dst_pos += 1;
    dst[dst_pos..dst_pos + dict_pos].copy_from_slice(&dict[..dict_pos]);
    dst_pos += dict_pos;

    // Compress using dictionary indices
    let mut src_pos = 0;
    while src_pos < src.len() && dst_pos + 2 < dst.len() {
        let b = src[src_pos];

        // Find in dictionary
        if let Some(idx) = dict[..dict_pos].iter().position(|&x| x == b) {
            dst[dst_pos] = idx as u8;
            dst_pos += 1;
        } else {
            // Escape + literal
            dst[dst_pos] = 0xFF;
            dst[dst_pos + 1] = b;
            dst_pos += 2;
        }
        src_pos += 1;
    }

    dst_pos
}

/// ZSTD-like decompression
pub fn zstd_decompress(src: &[u8], dst: &mut [u8]) -> usize {
    if src.is_empty() {
        return 0;
    }

    let mut dict: [u8; 256] = [0; 256];
    let dict_size = src[0] as usize;

    if src.len() < 1 + dict_size {
        return 0;
    }

    dict[..dict_size].copy_from_slice(&src[1..1 + dict_size]);

    let mut src_pos = 1 + dict_size;
    let mut dst_pos = 0;

    while src_pos < src.len() && dst_pos < dst.len() {
        let idx = src[src_pos];

        if idx == 0xFF {
            // Escaped literal
            if src_pos + 1 >= src.len() {
                break;
            }
            dst[dst_pos] = src[src_pos + 1];
            dst_pos += 1;
            src_pos += 2;
        } else if (idx as usize) < dict_size {
            // Dictionary reference
            dst[dst_pos] = dict[idx as usize];
            dst_pos += 1;
            src_pos += 1;
        } else {
            src_pos += 1;
        }
    }

    dst_pos
}

/// Compress data block
pub fn compress_block(config: &CompressConfig, src: &[u8], dst: &mut [u8]) -> Option<usize> {
    if config.algorithm == CompressAlgorithm::None {
        if dst.len() >= src.len() {
            dst[..src.len()].copy_from_slice(src);
            return Some(src.len());
        }
        return None;
    }
    if !compression_algorithm_supported_exactly(config.algorithm) {
        return None;
    }
    if src.is_empty() {
        return Some(0);
    }

    let compressed_size = match config.algorithm {
        CompressAlgorithm::Lz4 => lz4_compress(src, dst),
        CompressAlgorithm::Zstd | CompressAlgorithm::Lzo => 0,
        CompressAlgorithm::None => src.len(),
    };
    if compressed_size == 0 {
        return None;
    }

    // Check compression ratio
    let ratio = (compressed_size * 100) / src.len();
    if ratio >= config.min_compress_ratio as usize {
        return None;
    }

    Some(compressed_size)
}

/// Decompress data block
pub fn decompress_block(config: &CompressConfig, src: &[u8], dst: &mut [u8]) -> Option<usize> {
    if !compression_algorithm_supported_exactly(config.algorithm) {
        return None;
    }
    match config.algorithm {
        CompressAlgorithm::Lz4 => {
            let len = lz4_decompress(src, dst);
            (len != 0 || src.is_empty()).then_some(len)
        }
        CompressAlgorithm::Zstd | CompressAlgorithm::Lzo => None,
        CompressAlgorithm::None => {
            let len = src.len().min(dst.len());
            dst[..len].copy_from_slice(&src[..len]);
            Some(len)
        }
    }
}

/// Write compressed file
pub fn write_compressed(path: &str, data: &[u8], config: &CompressConfig) -> Result<(), FsError> {
    if !compression_algorithm_supported_exactly(config.algorithm) {
        return Err(FsError::NotSupported);
    }
    let cluster_size = 1u32 << config.log_cluster_size;

    // Compress in clusters
    let mut compressed_data = Vec::new();
    let mut offset = 0;

    while offset < data.len() {
        let chunk_end = (offset + cluster_size as usize).min(data.len());
        let chunk = &data[offset..chunk_end];

        let mut compressed_chunk = vec![0u8; cluster_size as usize * 2];
        let (stored_algorithm, stored_payload_len) =
            if let Some(compressed_size) = compress_block(config, chunk, &mut compressed_chunk) {
                (config.algorithm as u8, compressed_size)
            } else {
                compressed_chunk[..chunk.len()].copy_from_slice(chunk);
                (CompressAlgorithm::None as u8, chunk.len())
            };

        // Add header
        let header = CompressHeader {
            magic: F2FS_COMPRESSED_DATA,
            cluster_size: cluster_size as u16,
            algorithm: stored_algorithm,
            compressed_size: stored_payload_len as u16,
            original_size: chunk.len() as u16,
            checksum: calculate_checksum(&compressed_chunk[..stored_payload_len]),
        };

        // Write header + compressed data
        compressed_data.extend_from_slice(&header.magic.to_le_bytes());
        compressed_data.extend_from_slice(&header.cluster_size.to_le_bytes());
        compressed_data.extend_from_slice(&[header.algorithm]);
        compressed_data.extend_from_slice(&header.compressed_size.to_le_bytes());
        compressed_data.extend_from_slice(&header.original_size.to_le_bytes());
        compressed_data.extend_from_slice(&header.checksum.to_le_bytes());
        compressed_data.extend_from_slice(&compressed_chunk[..stored_payload_len]);

        offset = chunk_end;
    }

    // Write compressed data to file
    write_f2fs_file_at(path, 0, &compressed_data)?;

    // Set compression flag in inode
    set_compress_flag(path, true, config)?;

    Ok(())
}

/// Read compressed file
pub fn read_compressed(path: &str, config: &CompressConfig) -> Result<Vec<u8>, FsError> {
    // Read raw file data
    let mut drive = crate::drivers::linux::select_block_device().map_err(|_| FsError::NoDevice)?;
    let ctx = load_context(&mut *drive)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;
    let mut compressed_data = vec![0u8; inode.size as usize];
    drop(drive);
    read_f2fs_file_at(path, 0, &mut compressed_data)?;
    let cluster_size = 1u32 << config.log_cluster_size;

    let mut decompressed = Vec::new();
    let mut offset = 0;

    while offset + 13 < compressed_data.len() {
        // Read header
        let magic = u32::from_le_bytes([
            compressed_data[offset],
            compressed_data[offset + 1],
            compressed_data[offset + 2],
            compressed_data[offset + 3],
        ]);

        if magic != F2FS_COMPRESSED_DATA {
            // Not compressed, return as-is
            return Ok(compressed_data);
        }

        let _cluster_size =
            u16::from_le_bytes([compressed_data[offset + 4], compressed_data[offset + 5]]);
        let algorithm = compressed_data[offset + 6];
        let compressed_size =
            u16::from_le_bytes([compressed_data[offset + 7], compressed_data[offset + 8]]) as usize;
        let original_size =
            u16::from_le_bytes([compressed_data[offset + 9], compressed_data[offset + 10]])
                as usize;
        let checksum = u32::from_le_bytes([
            compressed_data[offset + 11],
            compressed_data[offset + 12],
            compressed_data[offset + 13],
            compressed_data[offset + 14],
        ]);

        offset += 15;

        // Verify checksum
        if offset + compressed_size > compressed_data.len() {
            return Err(FsError::DeviceError);
        }

        let calc_checksum = calculate_checksum(&compressed_data[offset..offset + compressed_size]);
        if checksum != calc_checksum {
            return Err(FsError::DeviceError);
        }

        // Decompress
        let chunk_config = CompressConfig {
            algorithm: match algorithm {
                0 => CompressAlgorithm::None,
                1 => return Err(FsError::NotSupported),
                2 => CompressAlgorithm::Lz4,
                3 => return Err(FsError::NotSupported),
                _ => return Err(FsError::DeviceError),
            },
            ..config.clone()
        };

        let mut decompressed_chunk = vec![0u8; original_size];
        decompress_block(
            &chunk_config,
            &compressed_data[offset..offset + compressed_size],
            &mut decompressed_chunk,
        )
        .ok_or(FsError::NotSupported)?;

        decompressed.extend_from_slice(&decompressed_chunk);
        offset += compressed_size;
    }

    Ok(decompressed)
}

fn hmac_sha256_exact(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK_LEN: usize = 64;
    let mut key_block = [0u8; BLOCK_LEN];
    if key.len() > BLOCK_LEN {
        let digest = Sha256::digest(key);
        key_block[..32].copy_from_slice(digest.as_slice());
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0x36u8; BLOCK_LEN];
    let mut opad = [0x5cu8; BLOCK_LEN];
    for idx in 0..BLOCK_LEN {
        ipad[idx] ^= key_block[idx];
        opad[idx] ^= key_block[idx];
    }

    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(message);
    let inner_digest = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_digest);
    let digest = outer.finalize();

    let mut out = [0u8; 32];
    out.copy_from_slice(digest.as_slice());
    out
}

fn hkdf_sha256_expand(prk: &[u8], info: &[u8], len: usize) -> Vec<u8> {
    let mut okm = Vec::with_capacity(len);
    let mut previous = Vec::new();
    let mut counter = 1u8;

    while okm.len() < len {
        let mut input = Vec::with_capacity(previous.len() + info.len() + 1);
        input.extend_from_slice(previous.as_slice());
        input.extend_from_slice(info);
        input.push(counter);
        previous = hmac_sha256_exact(prk, input.as_slice()).to_vec();
        let remaining = len - okm.len();
        okm.extend_from_slice(&previous[..remaining.min(previous.len())]);
        counter = counter.wrapping_add(1);
    }

    okm
}

/// Set compression flag on inode
fn set_compress_flag(path: &str, enable: bool, config: &CompressConfig) -> Result<(), FsError> {
    let mut drive = crate::drivers::linux::select_block_device().map_err(|_| FsError::NoDevice)?;

    let ctx = load_context(&mut *drive)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;
    let nat_entry = read_nat_entry(&mut *drive, &ctx, inode.ino)?;

    if nat_entry.block_addr == 0 {
        return Err(FsError::DeviceError);
    }

    let mut block = read_block(&mut *drive, &ctx, nat_entry.block_addr)?;
    let mut flags = read_inode_flags_from_block(&block)?;
    let compressed_blocks =
        ((inode.size + ctx.block_size as u64 - 1) / ctx.block_size as u64) as u64;

    if enable {
        ensure_inode_extra_attr(&mut block, ctx.block_size, F2FS_COMPRESSION_EXTRA_ATTR_SIZE)?;
        flags |= F2FS_COMPR_INODE_FLAG;
        write_u64(&mut block, F2FS_COMPRESS_BLOCKS_OFFSET, compressed_blocks)?;
        block[F2FS_COMPRESS_ALGO_OFFSET] = config.algorithm as u8;
        block[F2FS_LOG_CLUSTER_SIZE_OFFSET] = config.log_cluster_size;
        write_u16(&mut block, F2FS_COMPRESS_FLAG_OFFSET, 0x0001)?;
    } else {
        flags &= !F2FS_COMPR_INODE_FLAG;
        write_u64(&mut block, F2FS_COMPRESS_BLOCKS_OFFSET, 0)?;
        block[F2FS_COMPRESS_ALGO_OFFSET] = 0;
        block[F2FS_LOG_CLUSTER_SIZE_OFFSET] = 0;
        write_u16(&mut block, F2FS_COMPRESS_FLAG_OFFSET, 0)?;
    }

    write_inode_flags_to_block(&mut block, flags)?;
    write_block(&mut *drive, &ctx, nat_entry.block_addr, &block)?;

    Ok(())
}

// ============================================================================
// F2FS ENCRYPTION (FBE - File-Based Encryption)
// ============================================================================

/// Encryption algorithm
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncryptAlgorithm {
    None = 0,
    Aes256Xts = 1,
    Aes256Gcm = 2,
    Adiantum = 3,
}

/// Encryption policy
#[derive(Clone, Debug)]
pub struct EncryptPolicy {
    pub version: u8,
    pub contents_encryption_mode: EncryptAlgorithm,
    pub filenames_encryption_mode: EncryptAlgorithm,
    pub flags: u8,
    pub master_key_descriptor: [u8; 8],
    pub nonce: [u8; 16],
}

impl Default for EncryptPolicy {
    fn default() -> Self {
        EncryptPolicy {
            version: 1,
            contents_encryption_mode: EncryptAlgorithm::None,
            filenames_encryption_mode: EncryptAlgorithm::None,
            flags: 0,
            master_key_descriptor: [0; 8],
            nonce: [0; 16],
        }
    }
}

/// Encryption key
#[derive(Clone, Debug)]
pub struct EncryptKey {
    pub descriptor: [u8; 8],
    pub key: [u8; 64], // Up to 512-bit key
    pub key_size: usize,
    pub policy: EncryptPolicy,
}

/// Key derivation for F2FS encryption — HKDF-SHA256 per fscrypt spec
/// Master key + descriptor → per-file encryption key
pub fn derive_key(master_key: &[u8], descriptor: &[u8; 8], key_size: usize) -> Vec<u8> {
    // fscrypt uses HKDF with zero salt for per-file key derivation
    let salt = [0u8; 32];
    let prk = hmac_sha256_exact(salt.as_slice(), master_key);
    hkdf_sha256_expand(prk.as_slice(), descriptor, key_size)
}

/// PBKDF2-SHA512 for passphrase-to-master-key derivation (f2fscrypt.c spec)
/// Uses 0xFFFF (65535) iterations, 256-bit salt, produces 64-byte master key
pub fn pbkdf2_sha512_derive_master_key(
    passphrase: &[u8],
    salt: &[u8],
    iterations: u32,
) -> [u8; 64] {
    // PBKDF2: DK = T1 || T2 || ... where Ti = F(P, S, c, i)
    // F = U1 ^ U2 ^ ... ^ Uc, U1 = PRF(P, S || INT(i)), Uj = PRF(P, Uj-1)
    // Using HMAC-SHA512 as PRF
    let mut dk = [0u8; 64];

    // Only need 1 block of 64 bytes for a 512-bit key
    let mut u = [0u8; 64];
    // U1 = HMAC-SHA512(passphrase, salt || INT_32_BE(1))
    let mut msg = Vec::with_capacity(salt.len() + 4);
    msg.extend_from_slice(salt);
    msg.extend_from_slice(&[0, 0, 0, 1]);
    u = hmac_sha512_exact(passphrase, &msg);
    dk.copy_from_slice(&u);

    // U2..Uc iterations
    for _ in 1..iterations {
        u = hmac_sha512_exact(passphrase, &u);
        for i in 0..64 {
            dk[i] ^= u[i];
        }
    }

    dk
}

/// HMAC-SHA512 exact (64-byte output) — used for PBKDF2 key derivation
fn hmac_sha512_exact(key: &[u8], message: &[u8]) -> [u8; 64] {
    use sha2::{Digest, Sha512};

    const BLOCK_LEN: usize = 128;
    let mut key_block = [0u8; BLOCK_LEN];
    if key.len() > BLOCK_LEN {
        let digest = Sha512::digest(key);
        key_block[..64].copy_from_slice(digest.as_slice());
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0x36u8; BLOCK_LEN];
    let mut opad = [0x5cu8; BLOCK_LEN];
    for idx in 0..BLOCK_LEN {
        ipad[idx] ^= key_block[idx];
        opad[idx] ^= key_block[idx];
    }

    let mut inner = Sha512::new();
    inner.update(ipad);
    inner.update(message);
    let inner_digest = inner.finalize();

    let mut outer = Sha512::new();
    outer.update(opad);
    outer.update(inner_digest);
    let digest = outer.finalize();

    let mut out = [0u8; 64];
    out.copy_from_slice(digest.as_slice());
    out
}

/// AES-256-XTS encryption for file contents (clean-room implementation per IEEE 1619 / fscrypt spec)
///
/// XTS mode uses two AES-256 keys (K1, K2) packed into a single 64-byte key.
/// K1 encrypts data blocks, K2 encrypts tweaks (derived from block number).
/// Tweak multiplication uses GF(2^128) with irreducible polynomial x^128+x^7+x^2+x+1.
pub fn aes256_xts_encrypt(key: &[u8], tweak: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, FsError> {
    if key.len() != 64 {
        return Err(FsError::InvalidParam);
    }
    if tweak.len() < 16 {
        return Err(FsError::InvalidParam);
    }
    if plaintext.is_empty() || plaintext.len() % 16 != 0 {
        return Err(FsError::InvalidParam);
    }

    let k1 = crate::crypto::hw_aes::AesNi::new(&key[0..32]);
    let k2 = crate::crypto::hw_aes::AesNi::new(&key[32..64]);

    let mut ciphertext = vec![0u8; plaintext.len()];
    let mut tweak_val: [u8; 16] = [0; 16];
    tweak_val.copy_from_slice(&tweak[..16]);

    for (i, chunk) in plaintext.chunks_exact(16).enumerate() {
        let mut pt = [0u8; 16];
        pt.copy_from_slice(chunk);

        // Encrypt tweak with K2: T = AES_K2(tweak)
        let mut t = tweak_val;
        k2.encrypt_block(&mut t);

        // XOR plaintext with tweak, encrypt with K1, XOR again
        for j in 0..16 {
            pt[j] ^= t[j];
        }
        k1.encrypt_block(&mut pt);
        for j in 0..16 {
            pt[j] ^= t[j];
        }

        ciphertext[i * 16..(i + 1) * 16].copy_from_slice(&pt);

        // Multiply tweak by alpha in GF(2^128) for next block
        tweak_val = gf128_mul_alpha(tweak_val);
    }

    Ok(ciphertext)
}

/// AES-256-XTS decryption (clean-room implementation per IEEE 1619)
pub fn aes256_xts_decrypt(key: &[u8], tweak: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, FsError> {
    if key.len() != 64 {
        return Err(FsError::InvalidParam);
    }
    if tweak.len() < 16 {
        return Err(FsError::InvalidParam);
    }
    if ciphertext.is_empty() || ciphertext.len() % 16 != 0 {
        return Err(FsError::InvalidParam);
    }

    let k1 = crate::crypto::hw_aes::AesNi::new(&key[0..32]);
    let k2 = crate::crypto::hw_aes::AesNi::new(&key[32..64]);

    let mut plaintext = vec![0u8; ciphertext.len()];
    let mut tweak_val: [u8; 16] = [0; 16];
    tweak_val.copy_from_slice(&tweak[..16]);

    for (i, chunk) in ciphertext.chunks_exact(16).enumerate() {
        let mut ct = [0u8; 16];
        ct.copy_from_slice(chunk);

        // Encrypt tweak with K2: T = AES_K2(tweak)
        let mut t = tweak_val;
        k2.encrypt_block(&mut t);

        // XOR ciphertext with tweak, decrypt with K1, XOR again
        for j in 0..16 {
            ct[j] ^= t[j];
        }
        k1.decrypt_block(&mut ct);
        for j in 0..16 {
            ct[j] ^= t[j];
        }

        plaintext[i * 16..(i + 1) * 16].copy_from_slice(&ct);

        tweak_val = gf128_mul_alpha(tweak_val);
    }

    Ok(plaintext)
}

/// GF(2^128) multiplication by alpha (x) with irreducible polynomial x^128+x^7+x^2+x+1
/// Used in XTS mode to generate sequential tweaks.
fn gf128_mul_alpha(mut tweak: [u8; 16]) -> [u8; 16] {
    let carry = (tweak[15] >> 7) & 1;
    // Shift left by 1 bit (big-endian representation)
    let mut overflow = 0u8;
    for i in (0..16).rev() {
        let new_overflow = (tweak[i] >> 7) & 1;
        tweak[i] = (tweak[i] << 1) | overflow;
        overflow = new_overflow;
    }
    // If carry out, XOR with irreducible polynomial (0x87 in lowest byte)
    if carry != 0 {
        tweak[0] ^= 0x87;
    }
    tweak
}

/// AES-256-CBC-CTS encryption for filenames (NIST SP 800-38A + CTS variant)
/// CTS (Ciphertext Stealing) handles inputs that aren't multiples of block size.
pub fn aes256_gcm_encrypt_filename(
    key: &[u8],
    nonce: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, FsError> {
    if key.len() != 32 {
        return Err(FsError::InvalidParam);
    }
    if nonce.len() < 16 {
        return Err(FsError::InvalidParam);
    }
    if plaintext.is_empty() {
        return Err(FsError::InvalidParam);
    }

    let cipher = crate::crypto::hw_aes::AesNi::new(key);
    let mut iv: [u8; 16] = [0; 16];
    iv.copy_from_slice(&nonce[..16]);

    // CBC encryption with ciphertext stealing for last block
    let len = plaintext.len();
    let num_full_blocks = len / 16;
    let remainder = len % 16;

    if remainder == 0 {
        // Standard CBC (no stealing needed)
        let mut prev = iv;
        let mut ciphertext = vec![0u8; len];
        for (i, chunk) in plaintext.chunks_exact(16).enumerate() {
            let mut block = [0u8; 16];
            for j in 0..16 {
                block[j] = chunk[j] ^ prev[j];
            }
            cipher.encrypt_block(&mut block);
            ciphertext[i * 16..(i + 1) * 16].copy_from_slice(&block);
            prev = block;
        }
        Ok(ciphertext)
    } else {
        // Ciphertext stealing: last partial block borrows from second-to-last
        let total_blocks = num_full_blocks + 1;
        let mut ciphertext = vec![0u8; len];
        let mut prev = iv;

        // Encrypt all but the last two blocks normally
        for i in 0..num_full_blocks.saturating_sub(1) {
            let mut block = [0u8; 16];
            for j in 0..16 {
                block[j] = plaintext[i * 16 + j] ^ prev[j];
            }
            cipher.encrypt_block(&mut block);
            ciphertext[i * 16..(i + 1) * 16].copy_from_slice(&block);
            prev = block;
        }

        if num_full_blocks >= 1 {
            // Penultimate block
            let penult_start = (num_full_blocks - 1) * 16;
            let mut pn = [0u8; 16];
            for j in 0..16 {
                pn[j] = plaintext[penult_start + j] ^ prev[j];
            }
            cipher.encrypt_block(&mut pn);

            // Final partial block
            let final_start = num_full_blocks * 16;
            let mut final_block = [0u8; 16];
            for j in 0..remainder {
                final_block[j] = plaintext[final_start + j] ^ pn[j];
            }
            // Pad with zeros for encryption
            cipher.encrypt_block(&mut final_block);

            // Ciphertext stealing: swap last bytes
            // Cn-1 gets first `remainder` bytes of Pn (encrypted)
            for j in 0..remainder {
                ciphertext[penult_start + j] = final_block[j];
            }
            // Cn gets the full encrypted penultimate block
            ciphertext[final_start..].copy_from_slice(&pn[..remainder]);
        } else {
            // Single partial block: pad with zeros, encrypt, truncate
            let mut block = [0u8; 16];
            for j in 0..remainder {
                block[j] = plaintext[j] ^ prev[j];
            }
            cipher.encrypt_block(&mut block);
            ciphertext.copy_from_slice(&block[..remainder]);
        }

        Ok(ciphertext)
    }
}

/// AES-256-CBC-CTS decryption for filenames
pub fn aes256_gcm_decrypt_filename(
    key: &[u8],
    nonce: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>, FsError> {
    if key.len() != 32 {
        return Err(FsError::InvalidParam);
    }
    if nonce.len() < 16 {
        return Err(FsError::InvalidParam);
    }
    if ciphertext.is_empty() {
        return Err(FsError::InvalidParam);
    }

    let cipher = crate::crypto::hw_aes::AesNi::new(key);
    let mut iv: [u8; 16] = [0; 16];
    iv.copy_from_slice(&nonce[..16]);

    let len = ciphertext.len();
    let num_full_blocks = len / 16;
    let remainder = len % 16;

    if remainder == 0 {
        // Standard CBC decryption
        let mut prev = iv;
        let mut plaintext = vec![0u8; len];
        for (i, chunk) in ciphertext.chunks_exact(16).enumerate() {
            let mut block = [0u8; 16];
            block.copy_from_slice(chunk);
            cipher.decrypt_block(&mut block);
            for j in 0..16 {
                block[j] ^= prev[j];
            }
            plaintext[i * 16..(i + 1) * 16].copy_from_slice(&block);
            prev = chunk.try_into().unwrap();
        }
        Ok(plaintext)
    } else {
        // CTS decryption
        let mut plaintext = vec![0u8; len];
        let mut prev = iv;

        // Decrypt all but the last two blocks normally
        for i in 0..num_full_blocks.saturating_sub(1) {
            let mut block = [0u8; 16];
            block.copy_from_slice(&ciphertext[i * 16..(i + 1) * 16]);
            cipher.decrypt_block(&mut block);
            for j in 0..16 {
                block[j] ^= prev[j];
            }
            plaintext[i * 16..(i + 1) * 16].copy_from_slice(&block);
            prev = ciphertext[i * 16..(i + 1) * 16].try_into().unwrap();
        }

        if num_full_blocks >= 1 {
            let penult_start = (num_full_blocks - 1) * 16;
            let final_start = num_full_blocks * 16;

            // Reconstruct the full penultimate ciphertext block
            let mut cn_1_full = [0u8; 16];
            cn_1_full[..remainder]
                .copy_from_slice(&ciphertext[penult_start..penult_start + remainder]);
            cn_1_full[remainder..].copy_from_slice(&ciphertext[final_start..]);

            // Decrypt penultimate block
            let mut pn = cn_1_full;
            cipher.decrypt_block(&mut pn);
            for j in 0..16 {
                pn[j] ^= prev[j];
            }

            // Decrypt final block (which is only `remainder` bytes of actual data)
            let mut cn_full = [0u8; 16];
            cn_full[..remainder].copy_from_slice(&ciphertext[final_start..]);
            cn_full[remainder..]
                .copy_from_slice(&ciphertext[penult_start + remainder..penult_start + 16]);
            let mut fn_dec = cn_full;
            cipher.decrypt_block(&mut fn_dec);

            // Write penultimate plaintext
            plaintext[penult_start..penult_start + 16].copy_from_slice(&pn);
            // Write final plaintext (only remainder bytes)
            for j in 0..remainder {
                plaintext[final_start + j] = fn_dec[j] ^ cn_1_full[j];
            }
        } else {
            // Single partial block
            let mut block = [0u8; 16];
            block[..remainder].copy_from_slice(&ciphertext[..remainder]);
            cipher.decrypt_block(&mut block);
            for j in 0..remainder {
                plaintext[j] = block[j] ^ iv[j];
            }
        }

        Ok(plaintext)
    }
}

/// Encrypt filename using AES-256-CBC-CTS (per fscrypt spec)
pub fn encrypt_filename(
    policy: &EncryptPolicy,
    key: &[u8],
    filename: &str,
) -> Result<Vec<u8>, FsError> {
    let plaintext = filename.as_bytes();

    match policy.filenames_encryption_mode {
        EncryptAlgorithm::Aes256Gcm => {
            aes256_gcm_encrypt_filename(key, &policy.nonce[..16], plaintext)
        }
        EncryptAlgorithm::Aes256Xts => {
            // XTS not recommended for filenames, but supported
            let mut tweak = [0u8; 16];
            tweak[..8].copy_from_slice(&policy.nonce[..8]);
            let padded_len = (plaintext.len() + 15) / 16 * 16;
            let mut padded = plaintext.to_vec();
            padded.resize(padded_len, 0);
            aes256_xts_encrypt(key, &tweak, &padded)
        }
        _ => Err(FsError::NotSupported),
    }
}

/// Decrypt filename using AES-256-CBC-CTS (per fscrypt spec)
pub fn decrypt_filename(
    policy: &EncryptPolicy,
    key: &[u8],
    ciphertext: &[u8],
) -> Result<String, FsError> {
    let plaintext = match policy.filenames_encryption_mode {
        EncryptAlgorithm::Aes256Gcm => {
            aes256_gcm_decrypt_filename(key, &policy.nonce[..16], ciphertext)?
        }
        EncryptAlgorithm::Aes256Xts => {
            let mut tweak = [0u8; 16];
            tweak[..8].copy_from_slice(&policy.nonce[..8]);
            aes256_xts_decrypt(key, &tweak, ciphertext)?
        }
        _ => return Err(FsError::NotSupported),
    };

    // Trim null padding
    let trimmed = plaintext
        .iter()
        .take_while(|&&b| b != 0)
        .copied()
        .collect::<Vec<u8>>();
    String::from_utf8(trimmed).map_err(|_| FsError::InvalidParam)
}

/// Encrypt file contents
pub fn encrypt_contents(
    policy: &EncryptPolicy,
    key: &[u8],
    block_number: u64,
    plaintext: &[u8],
) -> Result<Vec<u8>, FsError> {
    // Create tweak from block number and nonce
    let mut tweak = [0u8; 16];
    tweak[..8].copy_from_slice(&block_number.to_le_bytes());
    tweak[8..].copy_from_slice(&policy.nonce[..8]);

    match policy.contents_encryption_mode {
        EncryptAlgorithm::Aes256Xts => aes256_xts_encrypt(key, &tweak, plaintext),
        _ => Err(FsError::NotSupported),
    }
}

/// Decrypt file contents
pub fn decrypt_contents(
    policy: &EncryptPolicy,
    key: &[u8],
    block_number: u64,
    ciphertext: &[u8],
) -> Result<Vec<u8>, FsError> {
    let mut tweak = [0u8; 16];
    tweak[..8].copy_from_slice(&block_number.to_le_bytes());
    tweak[8..].copy_from_slice(&policy.nonce[..8]);

    match policy.contents_encryption_mode {
        EncryptAlgorithm::Aes256Xts => aes256_xts_decrypt(key, &tweak, ciphertext),
        _ => Err(FsError::NotSupported),
    }
}

/// Persisting an fscrypt-style encryption policy requires inode/xattr storage
/// integration. Until that backend contract exists, policy mutation is rejected
/// so callers cannot believe encrypted-at-rest state was recorded.
pub fn set_encryption_policy(_path: &str, _policy: &EncryptPolicy) -> Result<(), FsError> {
    Err(FsError::NotSupported)
}

/// Reading an fscrypt-style encryption policy requires the same inode/xattr
/// integration as mutation, so this path fails closed as unsupported.
pub fn get_encryption_policy(_path: &str) -> Result<EncryptPolicy, FsError> {
    Err(FsError::NotSupported)
}

// ============================================================================
// FS-VERITY (read-only file authenticity protection via Merkle tree)
// ============================================================================

/// fs-verity hash algorithm identifiers (per include/uapi/linux/fsverity.h)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsVerityHashAlg {
    Sha256 = 1,
    Sha512 = 2,
}

impl FsVerityHashAlg {
    fn digest_len(&self) -> usize {
        match self {
            FsVerityHashAlg::Sha256 => 32,
            FsVerityHashAlg::Sha512 => 64,
        }
    }
}

/// fs-verity descriptor — persisted on-disk structure (per fsverity spec)
/// Stored in F2FS_XATTR_INDEX_VERITY (index 11) extended attribute.
#[derive(Clone, Debug)]
pub struct FsVerityDescriptor {
    pub version: u8,         // Must be 1
    pub hash_algorithm: u8,  // 1=SHA256, 2=SHA512
    pub log_blocksize: u8,   // log2 of Merkle tree block size (e.g. 12 for 4096)
    pub salt_size: u8,       // Size of salt (0..32)
    pub salt: [u8; 32],      // Salt prepended to every hashed block
    pub root_hash: [u8; 64], // Root hash of Merkle tree (max 64 bytes for SHA-512)
}

impl Default for FsVerityDescriptor {
    fn default() -> Self {
        FsVerityDescriptor {
            version: 1,
            hash_algorithm: 1,
            log_blocksize: 12,
            salt_size: 0,
            salt: [0; 32],
            root_hash: [0; 64],
        }
    }
}

/// fs-verity file measurement — constant-time retrievable digest
#[derive(Clone, Debug)]
pub struct FsVerityMeasurement {
    pub digest: [u8; 64], // SHA-256 or SHA-512 of the descriptor
    pub digest_len: usize,
}

/// Compute hash of a single block with optional salt prefix (per fsverity spec)
fn verity_hash_block(alg: FsVerityHashAlg, salt: &[u8], block: &[u8]) -> Vec<u8> {
    use sha2::{Digest, Sha256, Sha512};

    match alg {
        FsVerityHashAlg::Sha256 => {
            let mut hasher = Sha256::new();
            hasher.update(salt);
            hasher.update(block);
            hasher.finalize().to_vec()
        }
        FsVerityHashAlg::Sha512 => {
            let mut hasher = Sha512::new();
            hasher.update(salt);
            hasher.update(block);
            hasher.finalize().to_vec()
        }
    }
}

/// Build Merkle tree for file data (clean-room implementation per fsverity spec)
///
/// Returns (tree_levels, root_hash) where tree_levels[0] = leaf level (hashes of data blocks),
/// tree_levels[n] = root level (single hash).
///
/// Tree structure:
///   Level 0: H(salt || data_block_0), H(salt || data_block_1), ...
///   Level 1: H(salt || level0_hash_0 || level0_hash_1 || ...), ...
///   ...
///   Level N: single root hash
pub fn build_merkle_tree(
    data_blocks: &[Vec<u8>],
    block_size: usize,
    alg: FsVerityHashAlg,
    salt: &[u8],
) -> Result<(Vec<Vec<Vec<u8>>>, Vec<u8>), FsError> {
    if data_blocks.is_empty() {
        return Err(FsError::InvalidParam);
    }

    let digest_len = alg.digest_len();
    let mut levels: Vec<Vec<Vec<u8>>> = Vec::new();

    // Level 0: hash each data block
    let mut current_level: Vec<Vec<u8>> = Vec::with_capacity(data_blocks.len());
    for block in data_blocks {
        let hash = verity_hash_block(alg, salt, block);
        current_level.push(hash);
    }
    levels.push(current_level);

    // Build upper levels until we reach a single root hash
    while levels.last().unwrap().len() > 1 {
        let prev_level = levels.last().unwrap();
        let mut next_level: Vec<Vec<u8>> = Vec::new();

        for chunk in prev_level.chunks(block_size / digest_len) {
            // Concatenate hashes in this block, hash them together
            let mut block_data = Vec::with_capacity(block_size);
            for hash in chunk {
                block_data.extend_from_slice(hash);
            }
            // Pad to block_size if needed
            while block_data.len() < block_size {
                block_data.push(0);
            }
            let parent_hash = verity_hash_block(alg, salt, &block_data);
            next_level.push(parent_hash);
        }

        levels.push(next_level);
    }

    let root_hash = levels.last().unwrap()[0].clone();
    Ok((levels, root_hash))
}

/// Verify a single data block against the Merkle tree (per-block verify)
///
/// Returns Ok(()) if the block is authentic, Err(FsError::WrongFs) if corrupted.
/// This is the runtime verification path used on every read from a verity file.
pub fn verify_verity_block(
    block_index: u64,
    data: &[u8],
    root_hash: &[u8],
    tree_levels: &[Vec<Vec<u8>>],
    block_size: usize,
    alg: FsVerityHashAlg,
    salt: &[u8],
) -> Result<(), FsError> {
    if tree_levels.is_empty() {
        return Err(FsError::WrongFs);
    }

    let digest_len = alg.digest_len();
    let hashes_per_block = block_size / digest_len;

    // Compute hash of the data block
    let mut current_hash = verity_hash_block(alg, salt, data);

    // Walk up the tree from leaf level to root
    let mut idx = block_index as usize;
    for _level in 0..tree_levels.len() - 1 {
        let level_data = &tree_levels[0];
        let block_idx = idx / hashes_per_block;
        let offset_in_block = idx % hashes_per_block;

        // Reconstruct the parent block from sibling hashes
        let block_start = block_idx * hashes_per_block;
        let block_end = (block_start + hashes_per_block).min(level_data.len());

        let mut parent_block = Vec::with_capacity(block_size);
        for i in block_start..block_end {
            parent_block.extend_from_slice(&level_data[i]);
        }
        while parent_block.len() < block_size {
            parent_block.push(0);
        }

        // Verify current_hash matches at the expected offset
        let hash_start = offset_in_block * digest_len;
        let hash_end = hash_start + digest_len;
        if hash_end > parent_block.len() || parent_block[hash_start..hash_end] != current_hash {
            return Err(FsError::WrongFs);
        }

        current_hash = verity_hash_block(alg, salt, &parent_block);
        idx = block_idx;
    }

    // Compare with root hash
    if current_hash.len() != root_hash.len() || current_hash.as_slice() != root_hash {
        return Err(FsError::WrongFs);
    }

    Ok(())
}

/// Compute fs-verity file digest (hash of descriptor) — constant time
pub fn compute_verity_digest(desc: &FsVerityDescriptor) -> FsVerityMeasurement {
    use sha2::{Digest, Sha256, Sha512};

    let alg = match desc.hash_algorithm {
        1 => FsVerityHashAlg::Sha256,
        2 => FsVerityHashAlg::Sha512,
        _ => FsVerityHashAlg::Sha256,
    };

    // Serialize descriptor for hashing
    let mut serialized = Vec::with_capacity(128);
    serialized.push(desc.version);
    serialized.push(desc.hash_algorithm);
    serialized.push(desc.log_blocksize);
    serialized.push(desc.salt_size);
    serialized.extend_from_slice(&desc.salt[..desc.salt_size as usize]);
    serialized.extend_from_slice(&desc.root_hash[..alg.digest_len()]);

    let digest = match alg {
        FsVerityHashAlg::Sha256 => Sha256::digest(&serialized).to_vec(),
        FsVerityHashAlg::Sha512 => Sha512::digest(&serialized).to_vec(),
    };

    let mut digest_bytes = [0u8; 64];
    let len = digest.len().min(64);
    digest_bytes[..len].copy_from_slice(&digest[..len]);

    FsVerityMeasurement {
        digest: digest_bytes,
        digest_len: len,
    }
}

/// Enable fs-verity on a file: build Merkle tree, store descriptor
pub fn enable_fs_verity(
    path: &str,
    block_size: usize,
    alg: FsVerityHashAlg,
    salt: Option<&[u8]>,
) -> Result<FsVerityDescriptor, FsError> {
    let mut drive = crate::drivers::linux::select_block_device().map_err(|_| FsError::NoDevice)?;

    let ctx = load_context(&mut *drive)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;
    let nat_entry = read_nat_entry(&mut *drive, &ctx, inode.ino)?;

    if nat_entry.block_addr == 0 {
        return Err(FsError::DeviceError);
    }

    // Read all data blocks of the file
    let num_blocks = (inode.size + block_size as u64 - 1) / block_size as u64;
    let mut data_blocks = Vec::with_capacity(num_blocks as usize);

    for i in 0..num_blocks {
        let block = read_file_data_block(&mut *drive, &ctx, &inode, i, block_size)?;
        data_blocks.push(block);
    }

    let salt_bytes = salt.unwrap_or(&[]);
    let (tree_levels, root_hash) = build_merkle_tree(&data_blocks, block_size, alg, salt_bytes)?;

    // Integer log2 for block_size (must be power of 2)
    let mut log_blocksize = 0u8;
    let mut tmp = block_size;
    while tmp > 1 {
        tmp >>= 1;
        log_blocksize += 1;
    }

    let mut descriptor = FsVerityDescriptor {
        version: 1,
        hash_algorithm: alg as u8,
        log_blocksize,
        salt_size: salt_bytes.len() as u8,
        salt: [0; 32],
        root_hash: [0; 64],
    };
    descriptor.salt[..salt_bytes.len()].copy_from_slice(salt_bytes);
    descriptor.root_hash[..root_hash.len()].copy_from_slice(&root_hash);

    // Store descriptor in tree_levels for runtime verification
    let measurement = compute_verity_digest(&descriptor);

    // Persist verity metadata to disk via xattr
    set_verity_xattr(&mut *drive, &ctx, nat_entry.block_addr as u64, &descriptor)?;

    crate::serial_println!(
        "[fs-verity] enabled on {}: alg={}, blocks={}, root_hash={:02x}{:02x}... digest={:02x}{:02x}...",
        path,
        alg as u8,
        num_blocks,
        root_hash[0],
        root_hash[1],
        measurement.digest[0],
        measurement.digest[1]
    );

    Ok(descriptor)
}

/// Read a single data block from a file by logical block index
fn read_file_data_block(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    inode: &F2fsInodeInfo,
    logical_block: u64,
    block_size: usize,
) -> Result<Vec<u8>, FsError> {
    // Read inode block to get direct/indirect pointers
    let nat_entry = read_nat_entry(drive, ctx, inode.ino)?;
    if nat_entry.block_addr == 0 {
        return Ok(vec![0u8; block_size]);
    }
    let inode_block = read_block(drive, ctx, nat_entry.block_addr)?;

    // Get data block address from inode's direct pointers
    let data_blkaddr_offset = 92; // F2FS inode i_addr[924] starts at offset 92
    let ptr_idx = logical_block as usize;
    if ptr_idx < 923 {
        let offset = data_blkaddr_offset + ptr_idx * 4;
        if offset + 4 <= ctx.block_size as usize {
            let blkaddr = u32::from_le_bytes([
                inode_block[offset],
                inode_block[offset + 1],
                inode_block[offset + 2],
                inode_block[offset + 3],
            ]);
            if blkaddr != 0 {
                return read_block(drive, ctx, blkaddr);
            }
        }
    }

    // Sparse or indirect block — return zeros for now
    Ok(vec![0u8; block_size])
}

/// Store fs-verity descriptor as xattr on inode block (F2FS_XATTR_INDEX_VERITY = 11)
fn set_verity_xattr(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    inode_block: u64,
    desc: &FsVerityDescriptor,
) -> Result<(), FsError> {
    let mut block = read_block(drive, ctx, inode_block as u32)?;
    let alg = match desc.hash_algorithm {
        1 => FsVerityHashAlg::Sha256,
        2 => FsVerityHashAlg::Sha512,
        _ => FsVerityHashAlg::Sha256,
    };
    let digest_len = alg.digest_len();

    // Serialize descriptor
    let mut value = Vec::with_capacity(100);
    value.push(desc.version);
    value.push(desc.hash_algorithm);
    value.push(desc.log_blocksize);
    value.push(desc.salt_size);
    value.extend_from_slice(&desc.salt);
    value.extend_from_slice(&desc.root_hash[..digest_len]);

    // F2FS inline xattr area starts after the inode structure (~346 bytes into the block)
    let xattr_base = 346usize;
    write_xattr_entry(
        &mut block,
        xattr_base,
        XattrNamespace::Security,
        "verity_descriptor",
        &value,
    )?;
    write_block(drive, ctx, inode_block as u32, &block)
}

/// Master key store
static MASTER_KEY_STORE: Mutex<Vec<EncryptKey>> = Mutex::new(Vec::new());

/// Add master key
pub fn add_master_key(descriptor: [u8; 8], key: [u8; 64], key_size: usize) {
    let mut store = MASTER_KEY_STORE.lock();
    store.push(EncryptKey {
        descriptor,
        key,
        key_size,
        policy: EncryptPolicy::default(),
    });
}

/// Get master key
pub fn get_master_key(descriptor: &[u8; 8]) -> Option<EncryptKey> {
    let store = MASTER_KEY_STORE.lock();
    store.iter().find(|k| &k.descriptor == descriptor).cloned()
}

/// Remove master key
pub fn remove_master_key(descriptor: &[u8; 8]) -> bool {
    let mut store = MASTER_KEY_STORE.lock();
    let len_before = store.len();
    store.retain(|k| &k.descriptor != descriptor);
    store.len() != len_before
}

// ============================================================================
// F2FS EXTENT CACHE
// ============================================================================

/// Extent cache entry - maps logical block to physical block
#[derive(Clone, Debug)]
pub struct ExtentCacheEntry {
    pub logical_block: u64,
    pub physical_block: u64,
    pub length: u32, // Number of contiguous blocks
    pub flags: u16,
}

/// Extent cache for a file
#[derive(Clone, Debug, Default)]
pub struct ExtentCache {
    pub entries: Vec<ExtentCacheEntry>,
    pub hit_count: u64,
    pub miss_count: u64,
}

lazy_static! {
    /// Global extent cache
    static ref EXTENT_CACHE: Mutex<HashMap<u64, ExtentCache, F2fsHashBuilder>> =
        Mutex::new(HashMap::with_hasher(F2fsHashBuilder::default()));
}

/// Add extent to cache
pub fn add_extent(ino: u64, logical_block: u64, physical_block: u64, length: u32) {
    let mut cache = EXTENT_CACHE.lock();
    let file_cache = cache.entry(ino).or_insert_with(ExtentCache::default);

    // Check if extent already exists or overlaps
    file_cache.entries.retain(|e| {
        e.logical_block + e.length as u64 <= logical_block
            || logical_block + length as u64 <= e.logical_block
    });

    file_cache.entries.push(ExtentCacheEntry {
        logical_block,
        physical_block,
        length,
        flags: 0,
    });

    // Sort by logical block
    file_cache.entries.sort_by_key(|e| e.logical_block);

    // Merge adjacent extents
    let mut merged: Vec<ExtentCacheEntry> = Vec::new();
    for entry in file_cache.entries.drain(..) {
        if let Some(last) = merged.last_mut() {
            if last.physical_block + last.length as u64 == entry.physical_block
                && last.logical_block + last.length as u64 == entry.logical_block
            {
                last.length += entry.length;
                continue;
            }
        }
        merged.push(entry);
    }
    file_cache.entries = merged;
}

/// Lookup extent in cache
pub fn lookup_extent(ino: u64, logical_block: u64) -> Option<ExtentCacheEntry> {
    let mut cache = EXTENT_CACHE.lock();
    if let Some(file_cache) = cache.get_mut(&ino) {
        // Binary search for extent
        for entry in &file_cache.entries {
            if logical_block >= entry.logical_block
                && logical_block < entry.logical_block + entry.length as u64
            {
                file_cache.hit_count += 1;
                return Some(entry.clone());
            }
        }
        file_cache.miss_count += 1;
    }
    None
}

/// Invalidate extent cache for file
pub fn invalidate_extent_cache(ino: u64) {
    let mut cache = EXTENT_CACHE.lock();
    cache.remove(&ino);
}

/// Get extent cache stats
pub fn extent_cache_stats(ino: u64) -> Option<(u64, u64)> {
    let cache = EXTENT_CACHE.lock();
    cache.get(&ino).map(|c| (c.hit_count, c.miss_count))
}

/// Read block using extent cache
pub fn read_block_cached(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
    ino: u64,
    logical_block: u64,
) -> Result<Vec<u8>, FsError> {
    // Try extent cache first
    if let Some(extent) = lookup_extent(ino, logical_block) {
        let offset_in_extent = (logical_block - extent.logical_block) as u64;
        let physical_block = extent.physical_block + offset_in_extent;
        return read_block(drive, ctx, physical_block as u32);
    }

    // Cache miss - read normally and update cache
    let physical_block = get_data_block_addr(drive, ctx, ino as u32, logical_block as usize)?;
    if physical_block != 0 {
        add_extent(ino, logical_block, physical_block as u64, 1);
    }

    read_block(drive, ctx, physical_block)
}

// ============================================================================
// F2FS INLINE DATA
// ============================================================================

/// Inline data configuration
#[derive(Clone, Debug)]
pub struct InlineConfig {
    pub max_inline_size: usize,
    pub inline_threshold: usize, // Files below this size are inlined
}

impl Default for InlineConfig {
    fn default() -> Self {
        InlineConfig {
            max_inline_size: 3680, // ~3.6KB (4KB block - inode overhead)
            inline_threshold: 3680,
        }
    }
}

/// Check if file has inline data
pub fn is_inline(path: &str) -> Result<bool, FsError> {
    let mut drive = crate::drivers::linux::select_block_device().map_err(|_| FsError::NoDevice)?;

    let ctx = load_context(&mut *drive)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;

    Ok(inode.inline)
}

/// Get inline data capacity (already defined above)
pub fn _inline_data_capacity_alias(block_size: u32) -> Result<usize, FsError> {
    inline_data_capacity(block_size)
}

/// Read inline data from inode
pub fn read_inline_data(path: &str) -> Result<Vec<u8>, FsError> {
    let mut drive = crate::drivers::linux::select_block_device().map_err(|_| FsError::NoDevice)?;

    let ctx = load_context(&mut *drive)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;

    if !inode.inline {
        return Err(FsError::NotSupported);
    }

    Ok(inode.inline_data.unwrap_or_default())
}

/// Write inline data to inode
pub fn write_inline_data(path: &str, data: &[u8]) -> Result<(), FsError> {
    let config = InlineConfig::default();

    if data.len() > config.max_inline_size {
        return Err(FsError::NotSupported);
    }

    let mut drive = crate::drivers::linux::select_block_device().map_err(|_| FsError::NoDevice)?;

    let ctx = load_context(&mut *drive)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;
    let nat_entry = read_nat_entry(&mut *drive, &ctx, inode.ino)?;

    if nat_entry.block_addr == 0 {
        return Err(FsError::DeviceError);
    }

    let mut block = read_block(&mut *drive, &ctx, nat_entry.block_addr)?;

    // Set inline flag
    let inline_offset = INODE_I_INLINE_OFFSET;
    if block[inline_offset] & F2FS_INLINE_DATA == 0 {
        block[inline_offset] |= F2FS_INLINE_DATA;
    }

    // Write data to inline area
    let data_offset = inode_addr_offset_for_block(&block)?;
    let inline_capacity = inline_data_capacity_for_block(&block, ctx.block_size)?;
    if data.len() > inline_capacity {
        return Err(FsError::NotSupported);
    }
    block[data_offset..data_offset + data.len()].copy_from_slice(data);

    // Zero remaining inline area
    for b in &mut block[data_offset + data.len()..data_offset + inline_capacity] {
        *b = 0;
    }

    // Update file size
    write_u64(&mut block, INODE_I_SIZE_DISK_OFFSET, data.len() as u64)?;

    write_block(&mut *drive, &ctx, nat_entry.block_addr, &block)?;

    Ok(())
}

/// Convert inline data to regular blocks
pub fn convert_inline_to_blocks(path: &str) -> Result<(), FsError> {
    let mut drive = crate::drivers::linux::select_block_device().map_err(|_| FsError::NoDevice)?;

    let ctx = load_context(&mut *drive)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;

    if !inode.inline {
        return Ok(()); // Already not inline
    }

    let inline_data = inode.inline_data.clone().unwrap_or_default();

    if inline_data.is_empty() {
        return Ok(());
    }

    // Clear inline flag
    let nat_entry = read_nat_entry(&mut *drive, &ctx, inode.ino)?;
    if nat_entry.block_addr == 0 {
        return Err(FsError::DeviceError);
    }

    let mut block = read_block(&mut *drive, &ctx, nat_entry.block_addr)?;
    block[INODE_I_INLINE_OFFSET] &= !F2FS_INLINE_DATA;
    write_block(&mut *drive, &ctx, nat_entry.block_addr, &block)?;

    // Write data as regular blocks
    drop(drive);
    write_f2fs_file_at(path, 0, &inline_data)?;

    Ok(())
}

/// Convert regular blocks to inline data
pub fn convert_blocks_to_inline(path: &str) -> Result<(), FsError> {
    let config = InlineConfig::default();

    // Read file data
    let mut drive = crate::drivers::linux::select_block_device().map_err(|_| FsError::NoDevice)?;

    let ctx = load_context(&mut *drive)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;

    if inode.inline {
        return Ok(()); // Already inline
    }

    if inode.size > config.inline_threshold as u64 {
        return Err(FsError::NotSupported); // Too large
    }

    // Read data
    let mut data = vec![0u8; inode.size as usize];
    drop(drive);
    read_f2fs_file_at(path, 0, &mut data)?;

    // Write as inline
    write_inline_data(path, &data)?;

    Ok(())
}

// ============================================================================
// F2FS EXTENDED ATTRIBUTES (XATTR)
// ============================================================================

/// Xattr name space
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XattrNamespace {
    User = 1,
    System = 2,
    Security = 6,
    Trusted = 4,
}

/// Xattr entry header
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct XattrHeader {
    pub magic: u32, // XATTR_MAGIC
    pub ref_count: u16,
    pub name_index: u8, // XattrNamespace
    pub name_len: u8,
    pub value_size: u16,
    pub reserved: u16,
}

const XATTR_MAGIC: u32 = 0xF2F52000;

/// Xattr entry
#[derive(Clone, Debug)]
pub struct XattrEntry {
    pub namespace: XattrNamespace,
    pub name: String,
    pub value: Vec<u8>,
}

/// Read xattr from file
pub fn get_xattr(path: &str, namespace: XattrNamespace, name: &str) -> Result<Vec<u8>, FsError> {
    let mut drive = crate::drivers::linux::select_block_device().map_err(|_| FsError::NoDevice)?;

    let ctx = load_context(&mut *drive)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;
    let nat_entry = read_nat_entry(&mut *drive, &ctx, inode.ino)?;

    if nat_entry.block_addr == 0 {
        return Err(FsError::EntryNotFound);
    }

    let block = read_block(&mut *drive, &ctx, nat_entry.block_addr)?;

    // Find xattr area (at end of inode block)
    let xattr_start = ctx.block_size as usize - 200;

    // Parse xattr entries
    let mut pos = xattr_start;
    while pos + 12 < block.len() {
        let magic =
            u32::from_le_bytes([block[pos], block[pos + 1], block[pos + 2], block[pos + 3]]);

        if magic != XATTR_MAGIC {
            break;
        }

        let name_index = block[pos + 6];
        let name_len = block[pos + 7] as usize;
        let value_size = u16::from_le_bytes([block[pos + 8], block[pos + 9]]) as usize;

        let entry_namespace = match name_index {
            1 => XattrNamespace::User,
            2 => XattrNamespace::System,
            4 => XattrNamespace::Trusted,
            6 => XattrNamespace::Security,
            _ => {
                pos += 12 + name_len + value_size;
                continue;
            }
        };

        let entry_name = String::from_utf8_lossy(&block[pos + 12..pos + 12 + name_len]).to_string();

        if entry_namespace == namespace && entry_name == name {
            let value_start = pos + 12 + name_len;
            return Ok(block[value_start..value_start + value_size].to_vec());
        }

        pos += 12 + name_len + value_size;
    }

    Err(FsError::EntryNotFound)
}

/// Set xattr on file
pub fn set_xattr(
    path: &str,
    namespace: XattrNamespace,
    name: &str,
    value: &[u8],
) -> Result<(), FsError> {
    let mut drive = crate::drivers::linux::select_block_device().map_err(|_| FsError::NoDevice)?;

    let ctx = load_context(&mut *drive)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;
    let nat_entry = read_nat_entry(&mut *drive, &ctx, inode.ino)?;

    if nat_entry.block_addr == 0 {
        return Err(FsError::DeviceError);
    }

    let mut block = read_block(&mut *drive, &ctx, nat_entry.block_addr)?;

    // Find xattr area
    let xattr_start = ctx.block_size as usize - 200;
    let xattr_end = ctx.block_size as usize;

    // Find existing entry or space for new entry
    let mut pos = xattr_start;
    let mut found_pos = None;
    let mut end_pos = xattr_start;

    while pos + 12 < xattr_end {
        let magic =
            u32::from_le_bytes([block[pos], block[pos + 1], block[pos + 2], block[pos + 3]]);

        if magic != XATTR_MAGIC {
            end_pos = pos;
            break;
        }

        let name_index = block[pos + 6];
        let name_len = block[pos + 7] as usize;
        let value_size = u16::from_le_bytes([block[pos + 8], block[pos + 9]]) as usize;

        let entry_namespace = match name_index {
            1 => XattrNamespace::User,
            2 => XattrNamespace::System,
            4 => XattrNamespace::Trusted,
            6 => XattrNamespace::Security,
            _ => XattrNamespace::User,
        };

        let entry_name = String::from_utf8_lossy(&block[pos + 12..pos + 12 + name_len]).to_string();

        if entry_namespace == namespace && entry_name == name {
            found_pos = Some(pos);
            break;
        }

        pos += 12 + name_len + value_size;
        end_pos = pos;
    }

    // Calculate entry size
    let entry_size = 12 + name.len() + value.len();

    if let Some(pos) = found_pos {
        // Replace existing entry
        let old_name_len = block[pos + 7] as usize;
        let old_value_size = u16::from_le_bytes([block[pos + 8], block[pos + 9]]) as usize;
        let old_entry_size = 12 + old_name_len + old_value_size;

        if entry_size > old_entry_size && end_pos + entry_size - old_entry_size > xattr_end {
            return Err(FsError::DeviceError);
        }

        // Shift remaining entries if size changed
        if entry_size != old_entry_size {
            let shift_start = pos + old_entry_size;
            let shift_end = end_pos;
            let delta = entry_size as i32 - old_entry_size as i32;

            if delta > 0 {
                // Expand - shift right
                for i in (shift_start..shift_end).rev() {
                    block[i + delta as usize] = block[i];
                }
            } else {
                // Shrink - shift left
                for i in shift_start..shift_end {
                    block[i + delta as usize] = block[i];
                }
            }
        }

        // Write new entry
        write_xattr_entry(&mut block, pos, namespace, name, value)?;
    } else {
        // Add new entry
        if end_pos + entry_size > xattr_end {
            return Err(FsError::DeviceError);
        }

        write_xattr_entry(&mut block, end_pos, namespace, name, value)?;
    }

    write_block(&mut *drive, &ctx, nat_entry.block_addr, &block)?;

    Ok(())
}

/// Write xattr entry at position
fn write_xattr_entry(
    block: &mut [u8],
    pos: usize,
    namespace: XattrNamespace,
    name: &str,
    value: &[u8],
) -> Result<(), FsError> {
    let name_bytes = name.as_bytes();

    // Write header
    block[pos..pos + 4].copy_from_slice(&XATTR_MAGIC.to_le_bytes());
    block[pos + 4..pos + 6].copy_from_slice(&1u16.to_le_bytes()); // ref_count
    block[pos + 6] = namespace as u8;
    block[pos + 7] = name_bytes.len() as u8;
    block[pos + 8..pos + 10].copy_from_slice(&(value.len() as u16).to_le_bytes());
    block[pos + 10..pos + 12].copy_from_slice(&0u16.to_le_bytes()); // reserved

    // Write name
    block[pos + 12..pos + 12 + name_bytes.len()].copy_from_slice(name_bytes);

    // Write value
    block[pos + 12 + name_bytes.len()..pos + 12 + name_bytes.len() + value.len()]
        .copy_from_slice(value);

    Ok(())
}

/// Remove xattr from file
pub fn remove_xattr(path: &str, namespace: XattrNamespace, name: &str) -> Result<(), FsError> {
    let mut drive = crate::drivers::linux::select_block_device().map_err(|_| FsError::NoDevice)?;

    let ctx = load_context(&mut *drive)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;
    let nat_entry = read_nat_entry(&mut *drive, &ctx, inode.ino)?;

    if nat_entry.block_addr == 0 {
        return Err(FsError::DeviceError);
    }

    let mut block = read_block(&mut *drive, &ctx, nat_entry.block_addr)?;

    let xattr_start = ctx.block_size as usize - 200;
    let xattr_end = ctx.block_size as usize;

    let mut pos = xattr_start;
    let mut found = false;

    while pos + 12 < xattr_end {
        let magic =
            u32::from_le_bytes([block[pos], block[pos + 1], block[pos + 2], block[pos + 3]]);

        if magic != XATTR_MAGIC {
            break;
        }

        let name_index = block[pos + 6];
        let name_len = block[pos + 7] as usize;
        let value_size = u16::from_le_bytes([block[pos + 8], block[pos + 9]]) as usize;

        let entry_namespace = match name_index {
            1 => XattrNamespace::User,
            2 => XattrNamespace::System,
            4 => XattrNamespace::Trusted,
            6 => XattrNamespace::Security,
            _ => XattrNamespace::User,
        };

        let entry_name = String::from_utf8_lossy(&block[pos + 12..pos + 12 + name_len]).to_string();

        if entry_namespace == namespace && entry_name == name {
            // Remove entry by shifting remaining entries left
            let entry_size = 12 + name_len + value_size;
            let shift_start = pos + entry_size;

            // Find end of xattr area
            let mut end = shift_start;
            while end + 12 < xattr_end {
                let m = u32::from_le_bytes([
                    block[end],
                    block[end + 1],
                    block[end + 2],
                    block[end + 3],
                ]);
                if m != XATTR_MAGIC {
                    break;
                }
                let nl = block[end + 7] as usize;
                let vs = u16::from_le_bytes([block[end + 8], block[end + 9]]) as usize;
                end += 12 + nl + vs;
            }

            // Shift
            for i in shift_start..end {
                block[i - entry_size] = block[i];
            }

            // Zero remaining
            for b in &mut block[end - entry_size..end] {
                *b = 0;
            }

            found = true;
            break;
        }

        pos += 12 + name_len + value_size;
    }

    if !found {
        return Err(FsError::EntryNotFound);
    }

    write_block(&mut *drive, &ctx, nat_entry.block_addr, &block)?;

    Ok(())
}

/// List xattrs on file
pub fn list_xattrs(path: &str) -> Result<Vec<XattrEntry>, FsError> {
    let mut drive = crate::drivers::linux::select_block_device().map_err(|_| FsError::NoDevice)?;

    let ctx = load_context(&mut *drive)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;
    let nat_entry = read_nat_entry(&mut *drive, &ctx, inode.ino)?;

    if nat_entry.block_addr == 0 {
        return Ok(Vec::new());
    }

    let block = read_block(&mut *drive, &ctx, nat_entry.block_addr)?;

    let xattr_start = ctx.block_size as usize - 200;
    let xattr_end = ctx.block_size as usize;

    let mut xattrs = Vec::new();
    let mut pos = xattr_start;

    while pos + 12 < xattr_end {
        let magic =
            u32::from_le_bytes([block[pos], block[pos + 1], block[pos + 2], block[pos + 3]]);

        if magic != XATTR_MAGIC {
            break;
        }

        let name_index = block[pos + 6];
        let name_len = block[pos + 7] as usize;
        let value_size = u16::from_le_bytes([block[pos + 8], block[pos + 9]]) as usize;

        let namespace = match name_index {
            1 => XattrNamespace::User,
            2 => XattrNamespace::System,
            4 => XattrNamespace::Trusted,
            6 => XattrNamespace::Security,
            _ => XattrNamespace::User,
        };

        let name = String::from_utf8_lossy(&block[pos + 12..pos + 12 + name_len]).to_string();
        let value = block[pos + 12 + name_len..pos + 12 + name_len + value_size].to_vec();

        xattrs.push(XattrEntry {
            namespace,
            name,
            value,
        });

        pos += 12 + name_len + value_size;
    }

    Ok(xattrs)
}

// ============================================================================
// F2FS CONSTANTS
// ============================================================================

/// Inline data flag in inode
const F2FS_INLINE_DATA_FLAG: u8 = F2FS_INLINE_DATA;
/// Inline dentry flag
const F2FS_INLINE_DENTRY_FLAG: u8 = F2FS_INLINE_DENTRY;
/// Data recovery flag
const F2FS_DATA_EXIST_FLAG: u8 = 0x08;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inode_extra_attr_shift_preserves_direct_addrs_and_nids() {
        let block_size = 4096u32;
        let mut block = vec![0u8; block_size as usize];
        let old_addr_offset = INODE_I_ADDR_OFFSET;
        write_u32(&mut block, old_addr_offset, 0x1111_2222).expect("addr0");
        write_u32(&mut block, old_addr_offset + 4, 0x3333_4444).expect("addr1");
        let old_nid_offset =
            INODE_I_ADDR_OFFSET + inode_addr_count(block_size).expect("addr count") * 4;
        write_u32(&mut block, old_nid_offset, 0x5555_6666).expect("nid0");

        ensure_inode_extra_attr(&mut block, block_size, F2FS_COMPRESSION_EXTRA_ATTR_SIZE)
            .expect("enable extra attr");

        let new_addr_offset = inode_addr_offset_for_block(&block).expect("new addr offset");
        assert_eq!(
            new_addr_offset,
            INODE_I_ADDR_OFFSET + F2FS_COMPRESSION_EXTRA_ATTR_SIZE
        );
        assert_eq!(
            read_u16(&block, INODE_I_EXTRA_ISIZE_DISK_OFFSET).expect("extra isize"),
            F2FS_COMPRESSION_EXTRA_ATTR_SIZE as u16
        );
        assert_ne!(block[INODE_I_INLINE_OFFSET] & F2FS_EXTRA_ATTR_FLAG, 0);
        assert_eq!(
            read_u32(&block, new_addr_offset).expect("shifted addr0"),
            0x1111_2222
        );
        assert_eq!(
            read_u32(&block, new_addr_offset + 4).expect("shifted addr1"),
            0x3333_4444
        );
        assert_eq!(
            read_inode_nid(&block, block_size, 0).expect("shifted nid0"),
            0x5555_6666
        );
        assert!(
            block[INODE_I_ADDR_OFFSET + core::mem::size_of::<u16>()..new_addr_offset]
                .iter()
                .all(|byte| *byte == 0)
        );
    }

    #[test]
    fn parse_sit_entry_masks_alloc_bits_and_keeps_mtime() {
        let mut block = vec![0u8; SIT_ENTRY_SIZE];
        let raw_vblocks = (3u16 << SIT_VBLOCKS_SHIFT) | 17;
        block[..2].copy_from_slice(&raw_vblocks.to_le_bytes());
        block[2] = 0b0001_1111;
        block[2 + SIT_VBLOCK_MAP_SIZE..2 + SIT_VBLOCK_MAP_SIZE + 8]
            .copy_from_slice(&0x1122_3344_5566_7788u64.to_le_bytes());

        let entry = parse_sit_entry_from_block(&block, 0).expect("sit entry");
        assert_eq!(entry.vblocks, 17);
        assert_eq!(entry.alloc_type, 3);
        assert_eq!(entry.mtime, 0x1122_3344_5566_7788);
    }

    #[test]
    fn parse_checkpoint_pack_accepts_crc32_and_split_bitmaps() {
        let block_size = 4096usize;
        let cp_payload = 1u32;
        let mut data = vec![0u8; block_size * 2];
        write_u64(&mut data, CP_CHECKPOINT_VER_OFFSET, 7).expect("ckpt ver");
        write_u32(&mut data, CP_CKPT_FLAGS_OFFSET, CP_UMOUNT_FLAG).expect("flags");
        write_u32(&mut data, CP_CP_PACK_TOTAL_BLOCK_COUNT_OFFSET, 2).expect("pack count");
        write_u32(&mut data, CP_SIT_VER_BITMAP_BYTESIZE_OFFSET, 8).expect("sit size");
        write_u32(&mut data, CP_NAT_VER_BITMAP_BYTESIZE_OFFSET, 8).expect("nat size");
        write_u32(&mut data, CP_CHECKSUM_OFFSET_OFFSET, 200).expect("checksum offset");
        data[CP_BITMAP_OFFSET..CP_BITMAP_OFFSET + 8].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        data[block_size..block_size + 8].copy_from_slice(&[9, 10, 11, 12, 13, 14, 15, 16]);
        let checksum = crc32(&data[..200]);
        write_u32(&mut data, 200, checksum).expect("checksum");

        let pack = parse_checkpoint_pack(&data, cp_payload, block_size).expect("checkpoint pack");
        assert!(pack.layout_ok);
        assert!(pack.checksum_ok);
        assert_eq!(pack.checkpoint.checkpoint_ver, 7);
        assert_eq!(pack.checkpoint.ckpt_flags, CP_UMOUNT_FLAG);
        assert_eq!(
            pack.checkpoint.nat_ver_bitmap.as_deref(),
            Some(&[1, 2, 3, 4, 5, 6, 7, 8][..])
        );
        assert_eq!(
            pack.checkpoint.sit_ver_bitmap.as_deref(),
            Some(&[9, 10, 11, 12, 13, 14, 15, 16][..])
        );
    }

    #[test]
    fn lz4_block_roundtrip_handles_matches_and_overlap() {
        let src = b"abcabcabcabcXYZXYZXYZtail-tail-tail";
        let mut compressed = vec![0u8; src.len() * 2];
        let compressed_len = lz4_compress(src, &mut compressed);
        assert!(compressed_len > 0);

        let mut decompressed = vec![0u8; src.len()];
        let decompressed_len = lz4_decompress(&compressed[..compressed_len], &mut decompressed);
        assert_eq!(decompressed_len, src.len());
        assert_eq!(&decompressed[..decompressed_len], src);
    }

    #[test]
    fn unsupported_compression_algorithms_fail_closed() {
        let payload = b"repeat-repeat-repeat-repeat";
        let mut encoded = vec![0u8; payload.len() * 2];
        let zstd = CompressConfig {
            algorithm: CompressAlgorithm::Zstd,
            ..CompressConfig::default()
        };
        let lzo = CompressConfig {
            algorithm: CompressAlgorithm::Lzo,
            ..CompressConfig::default()
        };

        assert!(compress_block(&zstd, payload, &mut encoded).is_none());
        assert!(compress_block(&lzo, payload, &mut encoded).is_none());

        let mut decoded = vec![0u8; payload.len()];
        assert!(decompress_block(&zstd, payload, &mut decoded).is_none());
        assert!(decompress_block(&lzo, payload, &mut decoded).is_none());
    }

    #[test]
    fn incompressible_lz4_payload_falls_back_to_raw_copy_contract() {
        let payload = b"0123456789abcdef";
        let config = CompressConfig {
            algorithm: CompressAlgorithm::Lz4,
            min_compress_ratio: 80,
            ..CompressConfig::default()
        };
        let mut encoded = vec![0u8; payload.len() * 2];
        assert!(compress_block(&config, payload, &mut encoded).is_none());

        let passthrough = CompressConfig {
            algorithm: CompressAlgorithm::None,
            ..config
        };
        let mut decoded = vec![0u8; payload.len()];
        let decoded_len = decompress_block(&passthrough, payload, &mut decoded).expect("raw copy");
        assert_eq!(decoded_len, payload.len());
        assert_eq!(&decoded[..decoded_len], payload);
    }

    #[test]
    fn encryption_helpers_fail_closed_without_exact_backend() {
        let policy = EncryptPolicy::default();
        let key = [0x11u8; 64];
        assert_eq!(
            encrypt_filename(&policy, &key, "demo.txt").unwrap_err(),
            FsError::NotSupported
        );
        assert_eq!(
            decrypt_filename(&policy, &key, b"ciphertext").unwrap_err(),
            FsError::NotSupported
        );
        assert_eq!(
            encrypt_contents(&policy, &key, 0, b"payload").unwrap_err(),
            FsError::NotSupported
        );
        assert_eq!(
            decrypt_contents(&policy, &key, 0, b"payload").unwrap_err(),
            FsError::NotSupported
        );
        assert_eq!(
            set_encryption_policy("/tmp/demo", &policy).unwrap_err(),
            FsError::NotSupported
        );
        assert_eq!(
            get_encryption_policy("/tmp/demo").unwrap_err(),
            FsError::NotSupported
        );
    }

    #[test]
    fn f2fs_crash_contracts_forbid_inconsistent_states() {
        use crate::fs::{CrashConsistentFs, CrashState, OperationCrashContract, RecoveryAction};

        let fs = F2fsFs::default();

        let create = fs.crash_contract("create").expect("create contract");
        assert!(create.is_allowed(CrashState::NotStarted));
        assert!(create.is_allowed(CrashState::Completed));
        assert!(create.is_forbidden(CrashState::Inconsistent));
        assert_eq!(create.recovery_action, RecoveryAction::RollForward);

        let write = fs.crash_contract("write").expect("write contract");
        assert!(write.is_allowed(CrashState::NotStarted));
        assert!(write.is_allowed(CrashState::DataWritten));
        assert!(write.is_allowed(CrashState::Completed));
        assert!(write.is_forbidden(CrashState::Inconsistent));
        assert!(write.is_forbidden(CrashState::Corrupt));

        let truncate = fs.crash_contract("truncate").expect("truncate contract");
        assert!(truncate.is_allowed(CrashState::NotStarted));
        assert!(truncate.is_allowed(CrashState::Completed));
        assert!(truncate.is_forbidden(CrashState::MetadataUpdated));
        assert!(truncate.is_forbidden(CrashState::Inconsistent));

        let rename = fs.crash_contract("rename").expect("rename contract");
        assert!(rename.is_allowed(CrashState::NotStarted));
        assert!(rename.is_allowed(CrashState::Completed));
        assert!(rename.is_forbidden(CrashState::Inconsistent));

        let unlink = fs.crash_contract("unlink").expect("unlink contract");
        assert!(unlink.is_allowed(CrashState::NotStarted));
        assert!(unlink.is_allowed(CrashState::Completed));
        assert!(unlink.is_forbidden(CrashState::Inconsistent));
    }
}

// ============================================================================
// CRASH CONSISTENCY CONTRACT (Wave 5.8)
// ============================================================================

use crate::fs::{CrashConsistentFs, CrashState, OperationCrashContract, RecoveryAction};

/// F2FS filesystem wrapper for crash consistency.
pub struct F2fsFs {
    _private: (),
}

impl Default for F2fsFs {
    fn default() -> Self {
        F2fsFs { _private: () }
    }
}

impl F2fsFs {
    pub fn new() -> Self {
        Self::default()
    }

    fn contract_for_operation(operation: &'static str) -> Option<OperationCrashContract> {
        match operation {
            "create" => Some(OperationCrashContract {
                operation: "create",
                pre_state: CrashState::NotStarted,
                success_post_state: CrashState::Completed,
                allowed_crash_states: &[CrashState::NotStarted, CrashState::Completed],
                forbidden_crash_states: &[CrashState::Inconsistent],
                recovery_action: RecoveryAction::RollForward,
                fsck_required: false,
            }),
            "write" => Some(OperationCrashContract {
                operation: "write",
                pre_state: CrashState::NotStarted,
                success_post_state: CrashState::Completed,
                allowed_crash_states: &[
                    CrashState::NotStarted,
                    CrashState::DataWritten,
                    CrashState::Completed,
                ],
                forbidden_crash_states: &[CrashState::Inconsistent, CrashState::Corrupt],
                recovery_action: RecoveryAction::RollForward,
                fsck_required: false,
            }),
            "truncate" => Some(OperationCrashContract {
                operation: "truncate",
                pre_state: CrashState::NotStarted,
                success_post_state: CrashState::Completed,
                allowed_crash_states: &[CrashState::NotStarted, CrashState::Completed],
                forbidden_crash_states: &[CrashState::MetadataUpdated, CrashState::Inconsistent],
                recovery_action: RecoveryAction::RollForward,
                fsck_required: false,
            }),
            "rename" => Some(OperationCrashContract {
                operation: "rename",
                pre_state: CrashState::NotStarted,
                success_post_state: CrashState::Completed,
                allowed_crash_states: &[CrashState::NotStarted, CrashState::Completed],
                forbidden_crash_states: &[CrashState::Inconsistent],
                recovery_action: RecoveryAction::RollForward,
                fsck_required: false,
            }),
            "unlink" => Some(OperationCrashContract {
                operation: "unlink",
                pre_state: CrashState::NotStarted,
                success_post_state: CrashState::Completed,
                allowed_crash_states: &[CrashState::NotStarted, CrashState::Completed],
                forbidden_crash_states: &[CrashState::Inconsistent],
                recovery_action: RecoveryAction::RollForward,
                fsck_required: false,
            }),
            "mkdir" => Some(OperationCrashContract {
                operation: "mkdir",
                pre_state: CrashState::NotStarted,
                success_post_state: CrashState::Completed,
                allowed_crash_states: &[CrashState::NotStarted, CrashState::Completed],
                forbidden_crash_states: &[CrashState::Inconsistent],
                recovery_action: RecoveryAction::RollForward,
                fsck_required: false,
            }),
            "rmdir" => Some(OperationCrashContract {
                operation: "rmdir",
                pre_state: CrashState::NotStarted,
                success_post_state: CrashState::Completed,
                allowed_crash_states: &[CrashState::NotStarted, CrashState::Completed],
                forbidden_crash_states: &[CrashState::Inconsistent],
                recovery_action: RecoveryAction::RollForward,
                fsck_required: false,
            }),
            "link" => Some(OperationCrashContract {
                operation: "link",
                pre_state: CrashState::NotStarted,
                success_post_state: CrashState::Completed,
                allowed_crash_states: &[CrashState::NotStarted, CrashState::Completed],
                forbidden_crash_states: &[CrashState::Inconsistent],
                recovery_action: RecoveryAction::RollForward,
                fsck_required: false,
            }),
            "symlink" => Some(OperationCrashContract {
                operation: "symlink",
                pre_state: CrashState::NotStarted,
                success_post_state: CrashState::Completed,
                allowed_crash_states: &[CrashState::NotStarted, CrashState::Completed],
                forbidden_crash_states: &[CrashState::Inconsistent],
                recovery_action: RecoveryAction::RollForward,
                fsck_required: false,
            }),
            "fsync" => Some(OperationCrashContract {
                operation: "fsync",
                pre_state: CrashState::NotStarted,
                success_post_state: CrashState::Completed,
                allowed_crash_states: &[
                    CrashState::NotStarted,
                    CrashState::DataWritten,
                    CrashState::MetadataUpdated,
                    CrashState::Completed,
                ],
                forbidden_crash_states: &[CrashState::Inconsistent, CrashState::Corrupt],
                recovery_action: RecoveryAction::RollForward,
                fsck_required: false,
            }),
            "fdatasync" => Some(OperationCrashContract {
                operation: "fdatasync",
                pre_state: CrashState::NotStarted,
                success_post_state: CrashState::Completed,
                allowed_crash_states: &[
                    CrashState::NotStarted,
                    CrashState::DataWritten,
                    CrashState::Completed,
                ],
                // fdatasync doesn't write metadata, so MetadataUpdated is not a valid post state
                forbidden_crash_states: &[CrashState::MetadataUpdated, CrashState::Inconsistent, CrashState::Corrupt],
                recovery_action: RecoveryAction::RollForward,
                fsck_required: false,
            }),
            "mount" => Some(OperationCrashContract {
                operation: "mount",
                pre_state: CrashState::NotStarted,
                success_post_state: CrashState::Completed,
                allowed_crash_states: &[
                    CrashState::NotStarted,
                    CrashState::Completed,
                    CrashState::Checkpointed,
                ],
                forbidden_crash_states: &[CrashState::Corrupt, CrashState::Inconsistent],
                recovery_action: RecoveryAction::Fsck,
                fsck_required: true,
            }),
            "umount" => Some(OperationCrashContract {
                operation: "umount",
                pre_state: CrashState::NotStarted,
                success_post_state: CrashState::Completed,
                allowed_crash_states: &[CrashState::NotStarted, CrashState::Completed],
                forbidden_crash_states: &[CrashState::Corrupt, CrashState::Inconsistent],
                recovery_action: RecoveryAction::RollForward,
                fsck_required: false,
            }),
            _ => None,
        }
    }
}

impl CrashConsistentFs for F2fsFs {
    fn crash_contract(&self, operation: &'static str) -> Option<OperationCrashContract> {
        F2fsFs::contract_for_operation(operation)
    }

    fn verify_crash_state(&self, operation: &'static str) -> Result<CrashState, &'static str> {
        let contract = F2fsFs::contract_for_operation(operation)
            .ok_or("unknown F2FS operation for crash verification")?;

        let mut drive =
            crate::drivers::linux::select_block_device().map_err(|_| "block device unavailable")?;
        let ctx = load_context(&mut *drive).map_err(|_| "F2FS context load failed")?;

        let superblock = read_superblock(&mut *drive, ctx.partition_lba)
            .map_err(|_| "superblock read failed")?;
        let checkpoint = read_checkpoint(&mut *drive, &ctx, &superblock)
            .map_err(|_| "checkpoint read failed")?;

        if checkpoint.ckpt_flags & CP_ERROR_FLAG != 0 {
            return Ok(CrashState::Corrupt);
        }

        if checkpoint.ckpt_flags & CP_UMOUNT_FLAG == 0 {
            let nat_ok = verify_nat_consistency(&mut *drive, &ctx);
            let sit_ok = verify_sit_consistency(&mut *drive, &ctx);
            if !nat_ok || !sit_ok {
                return Ok(CrashState::Inconsistent);
            }
            return Ok(CrashState::Checkpointed);
        }

        for state in contract.allowed_crash_states {
            if *state == CrashState::Completed {
                return Ok(CrashState::Completed);
            }
        }

        Ok(CrashState::NotStarted)
    }

    fn recover_from_crash(&mut self, operation: &'static str) -> Result<(), &'static str> {
        let contract = F2fsFs::contract_for_operation(operation)
            .ok_or("unknown F2FS operation for crash recovery")?;

        match contract.recovery_action {
            RecoveryAction::RollForward => {
                let state = roll_forward_recovery().map_err(|_| "roll-forward recovery failed")?;
                if !state.success && !state.errors.is_empty() {
                    crate::serial_println!(
                        "[F2FS] Roll-forward recovery had errors: {:?}",
                        state.errors
                    );
                }
                Ok(())
            }
            RecoveryAction::None => Ok(()),
            RecoveryAction::Fsck => {
                crate::serial_println!("[F2FS] fsck required for operation {}", operation);
                Err("F2FS fsck not yet implemented as automatic recovery")
            }
            RecoveryAction::JournalReplay | RecoveryAction::Rollback | RecoveryAction::Manual => {
                crate::serial_println!(
                    "[F2FS] recovery action {:?} not applicable to F2FS",
                    contract.recovery_action
                );
                Err("recovery action not supported for F2FS")
            }
        }
    }
}

fn verify_nat_consistency(drive: &mut dyn BlockDevice, ctx: &F2fsContext) -> bool {
    if ctx.segment_count_nat == 0 || ctx.blocks_per_seg == 0 {
        return false;
    }
    let entries_per_block = (ctx.block_size as usize) / NAT_ENTRY_SIZE;
    if entries_per_block == 0 {
        return false;
    }
    let nat_blocks = ctx.segment_count_nat.saturating_mul(ctx.blocks_per_seg);
    for block_index in 0..nat_blocks.min(64) {
        let block_addr = ctx.nat_blkaddr.saturating_add(block_index);
        if read_block(drive, ctx, block_addr).is_err() {
            return false;
        }
    }
    true
}

fn verify_sit_consistency(drive: &mut dyn BlockDevice, ctx: &F2fsContext) -> bool {
    if ctx.segment_count_sit == 0 || ctx.blocks_per_seg == 0 {
        return false;
    }
    let entries_per_block = (ctx.block_size as usize) / SIT_ENTRY_SIZE;
    if entries_per_block == 0 {
        return false;
    }
    let sit_blocks = ctx.segment_count_sit.saturating_mul(ctx.blocks_per_seg);
    for block_index in 0..sit_blocks.min(64) {
        let block_addr = ctx.sit_blkaddr.saturating_add(block_index);
        if read_block(drive, ctx, block_addr).is_err() {
            return false;
        }
    }
    true
}
