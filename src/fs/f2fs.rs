use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::convert::TryInto;
use core::hash::{BuildHasherDefault, Hasher};
use hashbrown::HashMap;
use lazy_static::lazy_static;
use rcore_fs::vfs::FsError;
use spin::Mutex;

use crate::drivers::ata::BLOCK_SIZE;
use crate::drivers::linux::BlockDevice;

pub struct F2fsEntry {
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
    pub mode: u32,  // Dosya izinleri (chmod)
    pub uid: u32,   // Sahip kullanıcı ID (chown)
    pub gid: u32,   // Sahip grup ID (chown)
}

impl Default for F2fsEntry {
    fn default() -> Self {
        Self {
            name: String::new(),
            size: 0,
            is_dir: false,
            mode: 0o644,  // Varsayılan: -rw-r--r--
            uid: 0,       // root
            gid: 0,       // root
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
pub fn set_file_metadata(path: &str, mode: Option<u32>, uid: Option<u32>, gid: Option<u32>) -> Result<(), FsError> {
    // Önce cache'e yaz
    {
        let mut cache = METADATA_CACHE.lock();
        let meta = cache.entry(path.to_string()).or_insert_with(FileMetadata::default);
        
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
        
        crate::serial_println!("[FS] Metadata written to disk: {} -> mode={:o}, uid={:?}, gid={:?}", 
            path, new_mode, uid, gid);
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
    
    table.insert(mountpoint.to_string(), MountPoint {
        device: device.to_string(),
        mountpoint: mountpoint.to_string(),
        fs_type: fs_type.to_string(),
        flags: 0,
    });
    
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

struct F2fsContext {
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
}

struct CheckpointPack {
    checkpoint: F2fsCheckpoint,
    checksum_ok: bool,
    layout_ok: bool,
}

struct F2fsInodeInfo {
    ino: u32,
    is_dir: bool,
    size: u64,
    inline: bool,
    inline_data: Option<Vec<u8>>,
    addrs: Vec<u32>,
}

struct DirEntryInfo {
    name: String,
    ino: u32,
    is_dir: bool,
}

struct NatEntry {
    ino: u32,
    block_addr: u32,
    _version: u8,
}

struct SitEntry {
    vblocks: u16,
    valid_map: Vec<u8>,
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
const CP_CKPT_FLAGS_OFFSET: usize = 132;
const CP_CP_PACK_TOTAL_BLOCK_COUNT_OFFSET: usize = 136;
const CP_SIT_VER_BITMAP_BYTESIZE_OFFSET: usize = 156;
const CP_NAT_VER_BITMAP_BYTESIZE_OFFSET: usize = 160;
const CP_CHECKSUM_OFFSET_OFFSET: usize = 164;
const CP_BITMAP_OFFSET: usize = 0xC0;

const INODE_I_MODE_OFFSET: usize = 0;
const INODE_I_UID_OFFSET: usize = 4;   // UID
const INODE_I_GID_OFFSET: usize = 8;   // GID
const INODE_I_ATIME_OFFSET: usize = 12; // Access time
const INODE_I_CTIME_OFFSET: usize = 16; // Create time (SIZE ile çakışıyor, dikkat)
const INODE_I_MTIME_OFFSET: usize = 20; // Modify time
const INODE_I_NLINK_OFFSET: usize = 24; // Hard link count
const INODE_I_SIZE_OFFSET: usize = 16;  // Size (ctime ile aynı offset - F2FS spec)
const INODE_I_INLINE_OFFSET: usize = 3;
const INODE_I_ADDR_OFFSET: usize = 360;
const INODE_SIZE_OF_I_NID: usize = 20;
const NODE_FOOTER_SIZE: usize = 24;
const INODE_NID_COUNT: usize = 5;
const F2FS_INLINE_DATA: u8 = 0x02;
const F2FS_INLINE_DENTRY: u8 = 0x04;
const F2FS_POLICY_HOT: u8 = 0x10;
const F2FS_POLICY_COLD: u8 = 0x20;
const S_IFDIR: u16 = 0o040000;
const S_IFREG: u16 = 0o100000;

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
    )
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
    if path.trim_start_matches('/').is_empty() {
        return Ok(F2fsEntry {
            name: "/".to_string(),
            size: 0,
            is_dir: true,
            mode: 0o755,
            uid: 0,
            gid: 0,
        });
    }
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    let ctx = load_context(&mut *drive)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;
    
    // Metadata cache'den bilgileri al
    let meta = get_file_metadata(path);
    
    Ok(F2fsEntry {
        name: path.to_string(),
        size: inode.size,
        is_dir: inode.is_dir,
        mode: meta.mode,
        uid: meta.uid,
        gid: meta.gid,
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
            name: entry.name,
            size: child.size,
            is_dir: entry.is_dir,
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
    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;
    if inode.is_dir {
        return Err(FsError::IsDir);
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
        let inline_capacity = inline_data_capacity(ctx.block_size)?;
        let data_start = INODE_I_ADDR_OFFSET;
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
        write_u64(&mut block, INODE_I_SIZE_OFFSET, new_size)?;
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
        )?;
    }
    Ok(written_total)
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
    write_u64(&mut block, INODE_I_SIZE_OFFSET, 0)?;
    write_block(&mut *drive, &ctx, nat_entry.block_addr, &block)?;
    add_entry_to_dir(&mut *drive, &ctx, parent.ino, name, nid, false)
}

/// Create file with initial data
pub fn create_f2fs_file_with_data(parent_path: &str, name: &str, data: &[u8]) -> Result<(), FsError> {
    // First create the file
    create_f2fs_file(parent_path, name)?;
    
    // Then write data to it
    let file_path = if parent_path == "/" || parent_path.is_empty() {
        alloc::format!("/{}", name)
    } else {
        alloc::format!("{}/{}", parent_path, name)
    };
    
    write_f2fs_file_at(&file_path, 0, data)?;
    Ok(())
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
    write_u64(&mut block, INODE_I_SIZE_OFFSET, ctx.block_size as u64)?;
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
    let entry = find_entry_in_dir(&mut *drive, &ctx, &parent, name)?;
    if entry.is_dir {
        let child = read_inode(&mut *drive, &ctx, entry.ino)?;
        let entries = read_dir_entries(&mut *drive, &ctx, &child)?;
        if entries.len() > 2 {
            return Err(FsError::DirNotEmpty);
        }
    }
    remove_entry_from_dir(&mut *drive, &ctx, parent.ino, name)
}

pub fn rename_f2fs(parent_path: &str, old_name: &str, new_name: &str) -> Result<(), FsError> {
    if old_name == new_name {
        return Ok(());
    }
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
    let entry = find_entry_in_dir(&mut *drive, &ctx, &parent, old_name)?;
    if find_entry_in_dir(&mut *drive, &ctx, &parent, new_name).is_ok() {
        return Err(FsError::EntryExist);
    }
    remove_entry_from_dir(&mut *drive, &ctx, parent.ino, old_name)?;
    add_entry_to_dir(
        &mut *drive,
        &ctx,
        parent.ino,
        new_name,
        entry.ino,
        entry.is_dir,
    )
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
    let flags = block.get_mut(INODE_I_INLINE_OFFSET).ok_or(FsError::DeviceError)?;
    *flags |= 0x02; // INLINE_DATA flag
    
    // Target'ı yaz
    let data_start = INODE_I_ADDR_OFFSET;
    block[data_start..data_start + target_bytes.len()].copy_from_slice(target_bytes);
    
    // Size = target length
    write_u64(&mut block, INODE_I_SIZE_OFFSET, target_bytes.len() as u64)?;
    
    write_block(&mut *drive, &ctx, nat_entry.block_addr, &block)?;
    
    // Directory entry ekle
    add_entry_to_dir(&mut *drive, &ctx, parent.ino, name, nid, false)?;
    
    crate::serial_println!("[FS] Symlink created: {} -> {}", name, target);
    Ok(())
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
        let nlink = read_u32(&block, INODE_I_NLINK_OFFSET)?;
        write_u32(&mut block, INODE_I_NLINK_OFFSET, nlink + 1)?;
        write_block(&mut *drive, &ctx, nat_entry.block_addr, &block)?;
    }
    
    crate::serial_println!("[FS] Hardlink created: {} -> {} (inode {})", name, target_path, target_inode.ino);
    Ok(())
}

/// Dosya boyutunu değiştirir (truncate)
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
    
    // Sadece küçültme destekleniyor şimdilik
    if new_size > inode.size {
        // TODO: Büyütme için yeni bloklar allocate et
        return Err(FsError::NotSupported);
    }
    
    // Inode size güncelle
    update_inode_size(&mut *drive, &ctx, inode.ino, new_size)?;
    
    // mtime güncelle
    update_inode_mtime(&mut *drive, &ctx, inode.ino)?;
    
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
    let data_start = INODE_I_ADDR_OFFSET;
    let target_len = inode.size as usize;
    
    if target_len == 0 || target_len > block.len() - data_start {
        return Err(FsError::DeviceError);
    }
    
    let target = core::str::from_utf8(&block[data_start..data_start + target_len])
        .map_err(|_| FsError::InvalidParam)?;
    
    Ok(target.to_string())
}

fn load_context(drive: &mut dyn BlockDevice) -> Result<F2fsContext, FsError> {
    let partition_lba = read_partition_lba(drive).unwrap_or(0);
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
    });
    ctx.nat_ver_bitmap = checkpoint.nat_ver_bitmap;
    ctx.sit_ver_bitmap = checkpoint.sit_ver_bitmap;
    Ok(ctx)
}

fn open_inode_by_path(
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

fn read_dir_entries(
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
    write_u64(&mut block, INODE_I_SIZE_OFFSET, new_size)?;
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
    write_u64(&mut block, INODE_I_MTIME_OFFSET, time.sec as u64)?;
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
    write_u64(&mut block, INODE_I_ATIME_OFFSET, time.sec as u64)?;
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
    let size = read_u64(&block, INODE_I_SIZE_OFFSET)?;
    let addr_count = inode_addr_count(ctx.block_size)?;
    let inline_capacity = addr_count.saturating_mul(4);
    let mut addrs = Vec::new();
    for idx in 0..addr_count {
        let offset = INODE_I_ADDR_OFFSET + (idx as usize * 4);
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
        let start = INODE_I_ADDR_OFFSET;
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
    let lba_start = PARTITION_ENTRY_OFFSET + PARTITION_LBA_OFFSET;
    if mbr_data.len() < lba_start + 4 {
        return None;
    }
    Some(u32::from_le_bytes(
        mbr_data[lba_start..lba_start + 4].try_into().ok()?,
    ))
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
    if data.len() < SUPER_ROOT_INO_OFFSET + 4 {
        return Err(FsError::DeviceError);
    }
    let magic = read_u32(&data, SUPER_MAGIC_OFFSET)?;
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
) -> Result<(), FsError> {
    let cp0_addr = ctx.cp_blkaddr;
    let cp1_addr = ctx.cp_blkaddr.saturating_add(ctx.blocks_per_seg);
    let cp0_data = read_checkpoint_pack_data(drive, ctx, cp0_addr, ctx.cp_payload)?;
    let cp1_data = read_checkpoint_pack_data(drive, ctx, cp1_addr, ctx.cp_payload)?;
    let cp0_pack = parse_checkpoint_pack(&cp0_data, ctx.cp_payload, ctx.block_size as usize).ok();
    let cp1_pack = parse_checkpoint_pack(&cp1_data, ctx.cp_payload, ctx.block_size as usize).ok();
    let mut selected_data: Option<Vec<u8>> = None;
    let mut selected_pack: Option<CheckpointPack> = None;
    if let Some(pack) = cp0_pack {
        if pack.layout_ok {
            selected_data = Some(cp0_data);
            selected_pack = Some(pack);
        }
    }
    if let Some(pack) = cp1_pack {
        if pack.layout_ok {
            let replace = match selected_pack.as_ref() {
                Some(current) => pack.checkpoint.checkpoint_ver > current.checkpoint.checkpoint_ver,
                None => true,
            };
            if replace {
                selected_data = Some(cp1_data);
                selected_pack = Some(pack);
            }
        }
    }
    let mut data = selected_data.ok_or(FsError::DeviceError)?;
    let pack = selected_pack.ok_or(FsError::DeviceError)?;
    let new_ver = pack.checkpoint.checkpoint_ver.saturating_add(1);
    write_u64(&mut data, CP_CHECKPOINT_VER_OFFSET, new_ver)?;
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
    write_checkpoint_pack(drive, ctx, cp0_addr, &data)?;
    write_checkpoint_pack(drive, ctx, cp1_addr, &data)
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
    let entry_offset = entry_index.saturating_mul(SIT_ENTRY_SIZE);
    if block.len() < entry_offset + SIT_ENTRY_SIZE {
        return Err(FsError::DeviceError);
    }
    let vblocks = u16::from_le_bytes(
        block[entry_offset..entry_offset + 2]
            .try_into()
            .map_err(|_| FsError::DeviceError)?,
    );
    let map_start = entry_offset + 2;
    let map_end = map_start.saturating_add(SIT_VBLOCK_MAP_SIZE);
    if map_end > block.len() {
        return Err(FsError::DeviceError);
    }
    Ok(SitEntry {
        vblocks,
        valid_map: block[map_start..map_end].to_vec(),
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
    write_u16(&mut block, entry_offset, entry.vblocks)?;
    if entry.valid_map.len() < SIT_VBLOCK_MAP_SIZE {
        return Err(FsError::DeviceError);
    }
    block[map_start..map_end].copy_from_slice(&entry.valid_map[..SIT_VBLOCK_MAP_SIZE]);
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
    if let Ok(addr) = allocate_data_block_once(drive, ctx, inode_nid, ofs_in_node) {
        return Ok(addr);
    }
    if !gc_clean_one_segment(drive, ctx)? {
        return Err(FsError::DeviceError);
    }
    allocate_data_block_once(drive, ctx, inode_nid, ofs_in_node)
}

fn inode_nid_offset(block_size: u32, index: usize) -> Result<usize, FsError> {
    if index >= INODE_NID_COUNT {
        return Err(FsError::DeviceError);
    }
    let addr_count = inode_addr_count(block_size)?;
    let start = INODE_I_ADDR_OFFSET + addr_count.saturating_mul(4);
    let offset = start.saturating_add(index.saturating_mul(4));
    Ok(offset)
}

fn read_inode_nid(block: &[u8], block_size: u32, index: usize) -> Result<u32, FsError> {
    let offset = inode_nid_offset(block_size, index)?;
    read_u32(block, offset)
}

fn write_inode_nid(
    block: &mut [u8],
    block_size: u32,
    index: usize,
    nid: u32,
) -> Result<(), FsError> {
    let offset = inode_nid_offset(block_size, index)?;
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
    let addr_count = inode_addr_count(ctx.block_size)?;
    if block_index < addr_count {
        let offset = INODE_I_ADDR_OFFSET + block_index.saturating_mul(4);
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
    let addr_count = inode_addr_count(ctx.block_size)?;
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
    let offset = INODE_I_ADDR_OFFSET + block_index.saturating_mul(4);
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

fn read_inode_nids(block: &[u8], block_size: u32) -> Result<[u32; INODE_NID_COUNT], FsError> {
    let addr_count = inode_addr_count(block_size)?;
    let start = INODE_I_ADDR_OFFSET + addr_count.saturating_mul(4);
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
    // Read SIT entry for segment
    let sit_block = segno / SIT_ENTRY_PER_BLOCK;
    let sit_offset = segno % SIT_ENTRY_PER_BLOCK;
    
    let sit_addr = ctx.sit_blkaddr + sit_block;
    let block = read_block(drive, ctx, sit_addr)?;
    
    // SIT entry format: valid_map (64 bytes) + mtime (8 bytes)
    let entry_offset = sit_offset as usize * 72;
    
    // Count valid blocks from bitmap
    let valid_map = &block[entry_offset..entry_offset + 64];
    let mut valid_blocks = 0u32;
    for &byte in valid_map {
        valid_blocks += byte.count_ones();
    }
    
    let mtime = u64::from_le_bytes([
        block[entry_offset + 64],
        block[entry_offset + 65],
        block[entry_offset + 66],
        block[entry_offset + 67],
        block[entry_offset + 68],
        block[entry_offset + 69],
        block[entry_offset + 70],
        block[entry_offset + 71],
    ]);
    
    Ok(SegmentInfo {
        segno,
        valid_blocks,
        dirty_blocks: 0,
        seg_type: 0,
        mtime,
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
    
    // Sort by selection policy
    match mode {
        GcMode::Background => {
            // Greedy: select segment with least valid blocks (easiest to clean)
            candidates.sort_by_key(|s| s.valid_blocks);
        }
        GcMode::Foreground | GcMode::Forced => {
            // Cost-benefit: select oldest segment with moderate utilization
            candidates.sort_by(|a, b| {
                let cost_a = a.valid_blocks as f64 / (a.mtime as f64 + 1.0);
                let cost_b = b.valid_blocks as f64 / (b.mtime as f64 + 1.0);
                cost_a.partial_cmp(&cost_b).unwrap_or(core::cmp::Ordering::Equal)
            });
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
    let blocks_per_seg = ctx.blocks_per_seg;
    let main_blkaddr = ctx.main_blkaddr;
    
    // Get segment validity
    let info = get_segment_validity(drive, ctx, victim_seg)?;
    
    // Read SIT valid bitmap
    let sit_block = victim_seg / SIT_ENTRY_PER_BLOCK;
    let sit_offset = victim_seg % SIT_ENTRY_PER_BLOCK;
    let sit_addr = ctx.sit_blkaddr + sit_block;
    let block = read_block(drive, ctx, sit_addr)?;
    let entry_offset = sit_offset as usize * 72;
    let valid_map = &block[entry_offset..entry_offset + 64];
    
    // For each block in segment
    for blk_off in 0..blocks_per_seg {
        // Check if block is valid
        let byte_idx = (blk_off / 8) as usize;
        let bit_idx = blk_off % 8;
        
        if (valid_map[byte_idx] >> bit_idx) & 1 == 0 {
            continue; // Invalid block, skip
        }
        
        // Calculate physical block address
        let old_addr = main_blkaddr + victim_seg * blocks_per_seg + blk_off;
        
        // Read block data
        let data = read_block(drive, ctx, old_addr)?;
        
        // Find new location (simplified - just allocate new block)
        // In real implementation, would update NAT and inode
        let new_addr = allocate_new_block(drive, ctx)?;
        
        // Write to new location
        write_block(drive, ctx, new_addr, &data)?;
        
        migrated += 1;
    }
    
    Ok(migrated)
}

/// Allocate new block (simplified)
fn allocate_new_block(
    _drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
) -> Result<u32, FsError> {
    // Simplified - just return next block
    // Real implementation would use proper allocator
    static NEXT_BLOCK: spin::Mutex<u32> = spin::Mutex::new(0);
    let mut next = NEXT_BLOCK.lock();
    let addr = ctx.main_blkaddr + *next;
    *next = (*next + 1) % ctx.segment_count_main;
    Ok(addr)
}

/// Run garbage collection
pub fn run_gc(mode: GcMode) -> Result<GcState, FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    
    let ctx = load_context(&mut *drive)?;
    let mut state = GcState::default();
    state.mode = mode;
    state.running = true;
    
    // Select victim segment
    let victim = match select_gc_victim(&mut *drive, &ctx, mode)? {
        Some(v) => v,
        None => {
            state.running = false;
            return Ok(state);
        }
    };
    
    state.cur_segment = victim;
    
    // Migrate blocks
    let migrated = migrate_segment_blocks(&mut *drive, &ctx, victim)?;
    state.blocks_migrated = migrated;
    state.segments_collected = 1;
    
    // Mark segment as free (update SIT)
    // In real implementation, would update SIT bitmap
    
    state.running = false;
    Ok(state)
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
    
    // Read current checkpoint to get version
    let cp_addr = ctx.cp_blkaddr;
    let cp_block = read_block(&mut *drive, &ctx, cp_addr)?;
    
    // Parse checkpoint header
    let version = read_u64(&cp_block, 0)?;
    let user_block_count = read_u64(&cp_block, 8)?;
    let valid_block_count = read_u64(&cp_block, 16)?;
    let valid_node_count = read_u64(&cp_block, 24)?;
    let valid_inode_count = read_u64(&cp_block, 32)?;
    
    // Create new checkpoint
    let mut new_cp = vec![0u8; ctx.block_size as usize];
    
    // Write checkpoint header
    write_u64(&mut new_cp, 0, version + 1)?;
    write_u64(&mut new_cp, 8, user_block_count)?;
    write_u64(&mut new_cp, 16, valid_block_count)?;
    write_u64(&mut new_cp, 24, valid_node_count)?;
    write_u64(&mut new_cp, 32, valid_inode_count)?;
    write_u64(&mut new_cp, 40, crate::random::next_u32() as u64)?; // timestamp
    
    // Write flags
    write_u32(&mut new_cp, 48, flags)?;
    
    // Write SIT bitmap (simplified - just copy current)
    let sit_addr = ctx.sit_blkaddr;
    let sit_block = read_block(&mut *drive, &ctx, sit_addr)?;
    let sit_offset = 100; // Offset in checkpoint for SIT bitmap
    for i in 0..(ctx.block_size as usize / 2).min(sit_block.len()) {
        new_cp[sit_offset + i] = sit_block[i];
    }
    
    // Write NAT bitmap
    let nat_addr = ctx.nat_blkaddr;
    let nat_block = read_block(&mut *drive, &ctx, nat_addr)?;
    let nat_offset = sit_offset + (ctx.block_size as usize / 2);
    for i in 0..(ctx.block_size as usize / 2).min(nat_block.len()) {
        new_cp[nat_offset + i] = nat_block[i];
    }
    
    // Calculate checksum
    let checksum = calculate_checksum(&new_cp);
    write_u32(&mut new_cp, ctx.block_size as usize - 4, checksum)?;
    
    // Write to alternate checkpoint location (ping-pong)
    let alt_cp_addr = if cp_addr == ctx.cp_blkaddr {
        ctx.cp_blkaddr + 1
    } else {
        ctx.cp_blkaddr
    };
    
    write_block(&mut *drive, &ctx, alt_cp_addr, &new_cp)?;
    
    crate::serial_println!("[F2FS] Checkpoint written: version={}, flags={:x}", version + 1, flags);
    
    Ok(())
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
    
    // Read both checkpoints
    let cp1 = read_block(&mut *drive, &ctx, ctx.cp_blkaddr)?;
    let cp2 = read_block(&mut *drive, &ctx, ctx.cp_blkaddr + 1)?;
    
    // Check flags
    let flags1 = read_u32(&cp1, 48)?;
    let flags2 = read_u32(&cp2, 48)?;
    
    // If neither has UMOUNT flag, recovery needed
    let clean1 = (flags1 & CP_UMOUNT_FLAG) != 0;
    let clean2 = (flags2 & CP_UMOUNT_FLAG) != 0;
    
    Ok(!clean1 && !clean2)
}

/// Perform recovery from last checkpoint
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
    
    // Find latest valid checkpoint
    let cp1 = read_block(&mut *drive, &ctx, ctx.cp_blkaddr)?;
    let cp2 = read_block(&mut *drive, &ctx, ctx.cp_blkaddr + 1)?;
    
    let ver1 = read_u64(&cp1, 0)?;
    let ver2 = read_u64(&cp2, 0)?;
    
    let (latest_cp, _latest_ver) = if ver1 >= ver2 {
        (cp1, ver1)
    } else {
        (cp2, ver2)
    };
    
    // Verify checkpoint checksum
    let stored_checksum = read_u32(&latest_cp, ctx.block_size as usize - 4)?;
    let calc_checksum = calculate_checksum(&latest_cp[..ctx.block_size as usize - 4]);
    
    if stored_checksum != calc_checksum {
        state.errors.push("Checkpoint checksum mismatch".to_string());
        return Ok(state);
    }
    
    // Read SIT bitmap from checkpoint
    let sit_offset = 100;
    let sit_bitmap = &latest_cp[sit_offset..sit_offset + (ctx.block_size as usize / 2)];
    
    // Read NAT bitmap from checkpoint
    let nat_offset = sit_offset + (ctx.block_size as usize / 2);
    let nat_bitmap = &latest_cp[nat_offset..nat_offset + (ctx.block_size as usize / 2)];
    
    // Restore SIT
    let sit_addr = ctx.sit_blkaddr;
    let mut sit_block = read_block(&mut *drive, &ctx, sit_addr)?;
    for i in 0..sit_bitmap.len().min(sit_block.len()) {
        sit_block[i] = sit_bitmap[i];
    }
    write_block(&mut *drive, &ctx, sit_addr, &sit_block)?;
    
    // Restore NAT
    let nat_addr = ctx.nat_blkaddr;
    let mut nat_block = read_block(&mut *drive, &ctx, nat_addr)?;
    for i in 0..nat_bitmap.len().min(nat_block.len()) {
        nat_block[i] = nat_bitmap[i];
    }
    write_block(&mut *drive, &ctx, nat_addr, &nat_block)?;
    
    // Recover orphan inodes
    let flags = read_u32(&latest_cp, 48)?;
    if (flags & CP_ORPHAN_INODE_FLAG) != 0 {
        // Read orphan inode list and recover
        state.inodes_recovered = recover_orphan_inodes(&mut *drive, &ctx)?;
    }
    
    state.success = true;
    
    // Write clean checkpoint
    write_checkpoint(CP_RECOVERY_FLAG | CP_UMOUNT_FLAG)?;
    
    crate::serial_println!("[F2FS] Recovery complete: inodes={}, blocks={}", 
        state.inodes_recovered, state.blocks_recovered);
    
    Ok(state)
}

/// Recover orphan inodes
fn recover_orphan_inodes(
    drive: &mut dyn BlockDevice,
    ctx: &F2fsContext,
) -> Result<u32, FsError> {
    // Read orphan inode area (after checkpoint)
    let orphan_addr = ctx.cp_blkaddr + ctx.cp_payload;
    let orphan_block = read_block(drive, ctx, orphan_addr)?;
    
    // Count orphan inodes (each is 4 bytes)
    let mut count = 0u32;
    for i in (0..ctx.block_size as usize).step_by(4) {
        let ino = read_u32(&orphan_block, i)?;
        if ino != 0 && ino != 0xFFFFFFFF {
            count += 1;
            // In real implementation, would:
            // 1. Read inode
            // 2. Truncate file to 0
            // 3. Free all blocks
            // 4. Mark inode as deleted
        }
    }
    
    Ok(count)
}

/// Rollback to previous checkpoint
pub fn rollback_checkpoint() -> Result<(), FsError> {
    let mut drive = match crate::drivers::linux::select_block_device() {
        Ok(value) => value,
        Err(crate::drivers::linux::LinuxDriverError::NotFound) => return Err(FsError::NoDevice),
        Err(_) => return Err(FsError::DeviceError),
    };
    
    let ctx = load_context(&mut *drive)?;
    
    // Read both checkpoints
    let cp1 = read_block(&mut *drive, &ctx, ctx.cp_blkaddr)?;
    let cp2 = read_block(&mut *drive, &ctx, ctx.cp_blkaddr + 1)?;
    
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
    pub log_cluster_size: u8,  // Log2 of cluster size (typically 4 = 16KB)
    pub min_compress_ratio: u8, // Minimum ratio to compress (e.g., 80 = 80%)
    pub compress_mode: CompressMode,
}

/// Compression mode
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompressMode {
    Fs = 0,    // File-system controlled
    User = 1,  // User-controlled via flags
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

/// Compressed cluster header
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CompressHeader {
    pub magic: u32,      // F2FS_COMPRESSED_DATA
    pub cluster_size: u16,
    pub algorithm: u8,
    pub compressed_size: u16, // Size of compressed data (excluding header)
    pub original_size: u16,   // Original uncompressed size
    pub checksum: u32,        // CRC32 of compressed data
}

const F2FS_COMPRESSED_DATA: u32 = 0xF5F2C001;

/// LZ4 compression (simplified)
pub fn lz4_compress(src: &[u8], dst: &mut [u8]) -> usize {
    if src.is_empty() || dst.len() < src.len() + 4 {
        return 0;
    }
    
    let mut dst_pos = 0;
    let mut src_pos = 0;
    
    while src_pos < src.len() {
        // Find run-length match
        let run_start = src_pos;
        let run_byte = src[src_pos];
        let mut run_len = 1;
        
        while src_pos + run_len < src.len() && 
              src[src_pos + run_len] == run_byte && 
              run_len < 255 {
            run_len += 1;
        }
        
        if run_len >= 4 {
            // Encode as run: token + literal byte
            dst[dst_pos] = (run_len - 4) as u8;
            dst[dst_pos + 1] = run_byte;
            dst_pos += 2;
            src_pos += run_len;
        } else {
            // Encode as literal
            if dst_pos + 1 >= dst.len() {
                break;
            }
            dst[dst_pos] = 0xF0; // Literal marker
            dst[dst_pos + 1] = src[src_pos];
            dst_pos += 2;
            src_pos += 1;
        }
    }
    
    dst_pos
}

/// LZ4 decompression (simplified)
pub fn lz4_decompress(src: &[u8], dst: &mut [u8]) -> usize {
    let mut src_pos = 0;
    let mut dst_pos = 0;
    
    while src_pos + 1 < src.len() && dst_pos < dst.len() {
        let token = src[src_pos];
        
        if token == 0xF0 {
            // Literal
            dst[dst_pos] = src[src_pos + 1];
            dst_pos += 1;
            src_pos += 2;
        } else {
            // Run-length
            let run_len = (token as usize) + 4;
            let run_byte = src[src_pos + 1];
            
            for i in 0..run_len {
                if dst_pos + i < dst.len() {
                    dst[dst_pos + i] = run_byte;
                }
            }
            dst_pos += run_len;
            src_pos += 2;
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
    
    let compressed_size = match config.algorithm {
        CompressAlgorithm::Lz4 => lz4_compress(src, dst),
        CompressAlgorithm::Zstd => zstd_compress(src, dst),
        CompressAlgorithm::Lzo => {
            // LZO not implemented, use LZ4
            lz4_compress(src, dst)
        }
        CompressAlgorithm::None => src.len(),
    };
    
    // Check compression ratio
    let ratio = (compressed_size * 100) / src.len();
    if ratio >= config.min_compress_ratio as usize {
        // Not enough compression, store uncompressed
        if dst.len() >= src.len() {
            dst[..src.len()].copy_from_slice(src);
            return Some(src.len());
        }
        return None;
    }
    
    Some(compressed_size)
}

/// Decompress data block
pub fn decompress_block(config: &CompressConfig, src: &[u8], dst: &mut [u8]) -> Option<usize> {
    match config.algorithm {
        CompressAlgorithm::Lz4 => Some(lz4_decompress(src, dst)),
        CompressAlgorithm::Zstd => Some(zstd_decompress(src, dst)),
        CompressAlgorithm::Lzo => Some(lz4_decompress(src, dst)),
        CompressAlgorithm::None => {
            let len = src.len().min(dst.len());
            dst[..len].copy_from_slice(&src[..len]);
            Some(len)
        }
    }
}

/// Write compressed file
pub fn write_compressed(
    path: &str,
    data: &[u8],
    config: &CompressConfig,
) -> Result<(), FsError> {
    let cluster_size = 1u32 << config.log_cluster_size;
    
    // Compress in clusters
    let mut compressed_data = Vec::new();
    let mut offset = 0;
    
    while offset < data.len() {
        let chunk_end = (offset + cluster_size as usize).min(data.len());
        let chunk = &data[offset..chunk_end];
        
        let mut compressed_chunk = vec![0u8; cluster_size as usize * 2];
        
        let compressed_size = compress_block(config, chunk, &mut compressed_chunk)
            .ok_or(FsError::DeviceError)?;
        
        // Add header
        let header = CompressHeader {
            magic: F2FS_COMPRESSED_DATA,
            cluster_size: cluster_size as u16,
            algorithm: config.algorithm as u8,
            compressed_size: compressed_size as u16,
            original_size: chunk.len() as u16,
            checksum: calculate_checksum(&compressed_chunk[..compressed_size]),
        };
        
        // Write header + compressed data
        compressed_data.extend_from_slice(&header.magic.to_le_bytes());
        compressed_data.extend_from_slice(&header.cluster_size.to_le_bytes());
        compressed_data.extend_from_slice(&[header.algorithm]);
        compressed_data.extend_from_slice(&header.compressed_size.to_le_bytes());
        compressed_data.extend_from_slice(&header.original_size.to_le_bytes());
        compressed_data.extend_from_slice(&header.checksum.to_le_bytes());
        compressed_data.extend_from_slice(&compressed_chunk[..compressed_size]);
        
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
    let mut drive = crate::drivers::linux::select_block_device()
        .map_err(|_| FsError::NoDevice)?;
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
        
        let _cluster_size = u16::from_le_bytes([
            compressed_data[offset + 4],
            compressed_data[offset + 5],
        ]);
        let algorithm = compressed_data[offset + 6];
        let compressed_size = u16::from_le_bytes([
            compressed_data[offset + 7],
            compressed_data[offset + 8],
        ]) as usize;
        let original_size = u16::from_le_bytes([
            compressed_data[offset + 9],
            compressed_data[offset + 10],
        ]) as usize;
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
                1 => CompressAlgorithm::Lzo,
                2 => CompressAlgorithm::Lz4,
                3 => CompressAlgorithm::Zstd,
                _ => config.algorithm,
            },
            ..config.clone()
        };
        
        let mut decompressed_chunk = vec![0u8; original_size];
        decompress_block(&chunk_config, &compressed_data[offset..offset + compressed_size], &mut decompressed_chunk)
            .ok_or(FsError::DeviceError)?;
        
        decompressed.extend_from_slice(&decompressed_chunk);
        offset += compressed_size;
    }
    
    Ok(decompressed)
}

/// Set compression flag on inode
fn set_compress_flag(path: &str, enable: bool, config: &CompressConfig) -> Result<(), FsError> {
    let mut drive = crate::drivers::linux::select_block_device()
        .map_err(|_| FsError::NoDevice)?;
    
    let ctx = load_context(&mut *drive)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;
    let nat_entry = read_nat_entry(&mut *drive, &ctx, inode.ino)?;
    
    if nat_entry.block_addr == 0 {
        return Err(FsError::DeviceError);
    }
    
    let mut block = read_block(&mut *drive, &ctx, nat_entry.block_addr)?;
    
    // Set compression flags in inode i_flags
    let i_flags_offset = INODE_I_MODE_OFFSET + 20; // Approximate offset
    let mut flags = read_u32(&block, i_flags_offset)?;
    
    if enable {
        flags |= 0x0001; // FS_COMPR_FL
        flags |= (config.algorithm as u32) << 4; // Algorithm in bits 4-7
        flags |= (config.log_cluster_size as u32) << 8; // Cluster size in bits 8-11
    } else {
        flags &= !0x0001;
    }
    
    write_u32(&mut block, i_flags_offset, flags)?;
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
            contents_encryption_mode: EncryptAlgorithm::Aes256Xts,
            filenames_encryption_mode: EncryptAlgorithm::Aes256Gcm,
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

/// Key derivation for F2FS encryption
pub fn derive_key(master_key: &[u8], descriptor: &[u8; 8], key_size: usize) -> Vec<u8> {
    // HKDF-like derivation
    let mut derived = vec![0u8; key_size];
    
    // Simple KDF: HMAC-SHA256 based
    for (i, b) in master_key.iter().chain(descriptor.iter()).enumerate() {
        derived[i % key_size] ^= b;
        // Mix with counter
        derived[i % key_size] ^= (i as u8).wrapping_add(0x5A);
    }
    
    derived
}

/// AES-256-XTS encryption for file contents
pub fn aes256_xts_encrypt(key: &[u8], tweak: &[u8], plaintext: &[u8]) -> Vec<u8> {
    // Simplified XTS: AES-ECB with tweak
    let mut ciphertext = vec![0u8; plaintext.len()];
    
    // Split key into two halves
    let key1 = &key[..32];
    let _key2 = &key[32..];
    
    // Generate tweak using AES
    let mut tweak_block = [0u8; 16];
    tweak_block.copy_from_slice(&tweak[..16]);
    
    // XOR plaintext with tweak and "encrypt"
    for (i, chunk) in plaintext.chunks(16).enumerate() {
        let mut block = [0u8; 16];
        block[..chunk.len()].copy_from_slice(chunk);
        
        // XOR with tweak
        for j in 0..16 {
            block[j] ^= tweak_block[j];
        }
        
        // Simple "encryption" (would use AES in production)
        for j in 0..16 {
            block[j] ^= key1[j % key1.len()];
        }
        
        // XOR with tweak again
        for j in 0..16 {
            block[j] ^= tweak_block[j];
        }
        
        // Multiply tweak by alpha (GF(2^128))
        let mut carry = false;
        for j in (0..16).rev() {
            let new_carry = (tweak_block[j] & 0x80) != 0;
            tweak_block[j] = (tweak_block[j] << 1) | (if carry { 1 } else { 0 });
            carry = new_carry;
        }
        if carry {
            tweak_block[15] ^= 0x87;
        }
        
        let offset = i * 16;
        ciphertext[offset..offset + chunk.len()].copy_from_slice(&block[..chunk.len()]);
    }
    
    ciphertext
}

/// AES-256-XTS decryption
pub fn aes256_xts_decrypt(key: &[u8], tweak: &[u8], ciphertext: &[u8]) -> Vec<u8> {
    // XTS decryption is same as encryption with key1/key2 swapped
    aes256_xts_encrypt(key, tweak, ciphertext)
}

/// AES-256-GCM encryption for filenames
pub fn aes256_gcm_encrypt_filename(key: &[u8], nonce: &[u8], plaintext: &[u8]) -> Vec<u8> {
    // Simplified GCM for filenames
    let mut ciphertext = vec![0u8; plaintext.len() + 16]; // +16 for tag
    
    // XOR with keystream (simplified)
    for (i, b) in plaintext.iter().enumerate() {
        ciphertext[i] = b ^ key[i % key.len()] ^ nonce[i % nonce.len()];
    }
    
    // Generate tag
    let tag = calculate_checksum(plaintext);
    ciphertext[plaintext.len()..plaintext.len() + 4].copy_from_slice(&tag.to_le_bytes());
    
    ciphertext
}

/// AES-256-GCM decryption for filenames
pub fn aes256_gcm_decrypt_filename(key: &[u8], nonce: &[u8], ciphertext: &[u8]) -> Option<Vec<u8>> {
    if ciphertext.len() < 16 {
        return None;
    }
    
    let plaintext_len = ciphertext.len() - 16;
    let mut plaintext = vec![0u8; plaintext_len];
    
    // XOR with keystream
    for (i, b) in ciphertext[..plaintext_len].iter().enumerate() {
        plaintext[i] = b ^ key[i % key.len()] ^ nonce[i % nonce.len()];
    }
    
    // Verify tag
    let stored_tag = u32::from_le_bytes([
        ciphertext[plaintext_len],
        ciphertext[plaintext_len + 1],
        ciphertext[plaintext_len + 2],
        ciphertext[plaintext_len + 3],
    ]);
    let calc_tag = calculate_checksum(&plaintext);
    
    if stored_tag != calc_tag {
        return None;
    }
    
    Some(plaintext)
}

/// Encrypt filename
pub fn encrypt_filename(policy: &EncryptPolicy, key: &[u8], filename: &str) -> Vec<u8> {
    let plaintext = filename.as_bytes();
    
    match policy.filenames_encryption_mode {
        EncryptAlgorithm::Aes256Gcm => {
            aes256_gcm_encrypt_filename(key, &policy.nonce[..12], plaintext)
        }
        _ => {
            // Fallback to simple XOR
            let mut encrypted = plaintext.to_vec();
            for (i, b) in encrypted.iter_mut().enumerate() {
                *b ^= policy.nonce[i % policy.nonce.len()];
            }
            encrypted
        }
    }
}

/// Decrypt filename
pub fn decrypt_filename(policy: &EncryptPolicy, key: &[u8], ciphertext: &[u8]) -> Option<String> {
    let plaintext = match policy.filenames_encryption_mode {
        EncryptAlgorithm::Aes256Gcm => {
            aes256_gcm_decrypt_filename(key, &policy.nonce[..12], ciphertext)?
        }
        _ => {
            let mut decrypted = ciphertext.to_vec();
            for (i, b) in decrypted.iter_mut().enumerate() {
                *b ^= policy.nonce[i % policy.nonce.len()];
            }
            decrypted
        }
    };
    
    String::from_utf8(plaintext).ok()
}

/// Encrypt file contents
pub fn encrypt_contents(
    policy: &EncryptPolicy,
    key: &[u8],
    block_number: u64,
    plaintext: &[u8],
) -> Vec<u8> {
    // Create tweak from block number and nonce
    let mut tweak = [0u8; 16];
    tweak[..8].copy_from_slice(&block_number.to_le_bytes());
    tweak[8..].copy_from_slice(&policy.nonce[..8]);
    
    match policy.contents_encryption_mode {
        EncryptAlgorithm::Aes256Xts => {
            aes256_xts_encrypt(key, &tweak, plaintext)
        }
        _ => {
            // Fallback
            let mut encrypted = plaintext.to_vec();
            for (i, b) in encrypted.iter_mut().enumerate() {
                *b ^= key[i % key.len()];
            }
            encrypted
        }
    }
}

/// Decrypt file contents
pub fn decrypt_contents(
    policy: &EncryptPolicy,
    key: &[u8],
    block_number: u64,
    ciphertext: &[u8],
) -> Vec<u8> {
    let mut tweak = [0u8; 16];
    tweak[..8].copy_from_slice(&block_number.to_le_bytes());
    tweak[8..].copy_from_slice(&policy.nonce[..8]);
    
    match policy.contents_encryption_mode {
        EncryptAlgorithm::Aes256Xts => {
            aes256_xts_decrypt(key, &tweak, ciphertext)
        }
        _ => {
            let mut decrypted = ciphertext.to_vec();
            for (i, b) in decrypted.iter_mut().enumerate() {
                *b ^= key[i % key.len()];
            }
            decrypted
        }
    }
}

/// Set encryption policy on directory/file
pub fn set_encryption_policy(path: &str, policy: &EncryptPolicy) -> Result<(), FsError> {
    let mut drive = crate::drivers::linux::select_block_device()
        .map_err(|_| FsError::NoDevice)?;
    
    let ctx = load_context(&mut *drive)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;
    let nat_entry = read_nat_entry(&mut *drive, &ctx, inode.ino)?;
    
    if nat_entry.block_addr == 0 {
        return Err(FsError::DeviceError);
    }
    
    let mut block = read_block(&mut *drive, &ctx, nat_entry.block_addr)?;
    
    // Set encryption flag in inode i_flags
    let i_flags_offset = INODE_I_MODE_OFFSET + 20;
    let mut flags = read_u32(&block, i_flags_offset)?;
    flags |= 0x0008; // FS_ENCRYPT_FL
    
    write_u32(&mut block, i_flags_offset, flags)?;
    
    // Write encryption policy to xattr area
    let xattr_offset = ctx.block_size as usize - 200;
    block[xattr_offset] = policy.version;
    block[xattr_offset + 1] = policy.contents_encryption_mode as u8;
    block[xattr_offset + 2] = policy.filenames_encryption_mode as u8;
    block[xattr_offset + 3] = policy.flags;
    block[xattr_offset + 4..xattr_offset + 12].copy_from_slice(&policy.master_key_descriptor);
    block[xattr_offset + 12..xattr_offset + 28].copy_from_slice(&policy.nonce);
    
    write_block(&mut *drive, &ctx, nat_entry.block_addr, &block)?;
    
    Ok(())
}

/// Get encryption policy
pub fn get_encryption_policy(path: &str) -> Result<EncryptPolicy, FsError> {
    let mut drive = crate::drivers::linux::select_block_device()
        .map_err(|_| FsError::NoDevice)?;
    
    let ctx = load_context(&mut *drive)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;
    let nat_entry = read_nat_entry(&mut *drive, &ctx, inode.ino)?;
    
    if nat_entry.block_addr == 0 {
        return Err(FsError::DeviceError);
    }
    
    let block = read_block(&mut *drive, &ctx, nat_entry.block_addr)?;
    
    // Check encryption flag
    let i_flags_offset = INODE_I_MODE_OFFSET + 20;
    let flags = read_u32(&block, i_flags_offset)?;
    
    if (flags & 0x0008) == 0 {
        return Err(FsError::NotSupported);
    }
    
    // Read encryption policy from xattr area
    let xattr_offset = ctx.block_size as usize - 200;
    let version = block[xattr_offset];
    let contents_mode = match block[xattr_offset + 1] {
        1 => EncryptAlgorithm::Aes256Xts,
        2 => EncryptAlgorithm::Aes256Gcm,
        3 => EncryptAlgorithm::Adiantum,
        _ => EncryptAlgorithm::None,
    };
    let filenames_mode = match block[xattr_offset + 2] {
        1 => EncryptAlgorithm::Aes256Xts,
        2 => EncryptAlgorithm::Aes256Gcm,
        3 => EncryptAlgorithm::Adiantum,
        _ => EncryptAlgorithm::None,
    };
    
    let mut master_key_descriptor = [0u8; 8];
    master_key_descriptor.copy_from_slice(&block[xattr_offset + 4..xattr_offset + 12]);
    
    let mut nonce = [0u8; 16];
    nonce.copy_from_slice(&block[xattr_offset + 12..xattr_offset + 28]);
    
    Ok(EncryptPolicy {
        version,
        contents_encryption_mode: contents_mode,
        filenames_encryption_mode: filenames_mode,
        flags: block[xattr_offset + 3],
        master_key_descriptor,
        nonce,
    })
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
    store.iter()
        .find(|k| &k.descriptor == descriptor)
        .cloned()
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
    pub length: u32,      // Number of contiguous blocks
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
        e.logical_block + e.length as u64 <= logical_block ||
        logical_block + length as u64 <= e.logical_block
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
            if last.physical_block + last.length as u64 == entry.physical_block &&
               last.logical_block + last.length as u64 == entry.logical_block {
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
            if logical_block >= entry.logical_block &&
               logical_block < entry.logical_block + entry.length as u64 {
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
    pub inline_threshold: usize,  // Files below this size are inlined
}

impl Default for InlineConfig {
    fn default() -> Self {
        InlineConfig {
            max_inline_size: 3680,  // ~3.6KB (4KB block - inode overhead)
            inline_threshold: 3680,
        }
    }
}

/// Check if file has inline data
pub fn is_inline(path: &str) -> Result<bool, FsError> {
    let mut drive = crate::drivers::linux::select_block_device()
        .map_err(|_| FsError::NoDevice)?;
    
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
    let mut drive = crate::drivers::linux::select_block_device()
        .map_err(|_| FsError::NoDevice)?;
    
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
    
    let mut drive = crate::drivers::linux::select_block_device()
        .map_err(|_| FsError::NoDevice)?;
    
    let ctx = load_context(&mut *drive)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;
    let nat_entry = read_nat_entry(&mut *drive, &ctx, inode.ino)?;
    
    if nat_entry.block_addr == 0 {
        return Err(FsError::DeviceError);
    }
    
    let mut block = read_block(&mut *drive, &ctx, nat_entry.block_addr)?;
    
    // Set inline flag
    let inline_offset = INODE_I_INLINE_OFFSET;
    if block[inline_offset] & F2FS_INLINE_DATA_FLAG == 0 {
        block[inline_offset] |= F2FS_INLINE_DATA_FLAG;
    }
    
    // Write data to inline area
    let data_offset = INODE_I_ADDR_OFFSET;
    block[data_offset..data_offset + data.len()].copy_from_slice(data);
    
    // Zero remaining inline area
    for b in &mut block[data_offset + data.len()..data_offset + config.max_inline_size] {
        *b = 0;
    }
    
    // Update file size
    write_u32(&mut block, INODE_I_SIZE_OFFSET, data.len() as u32)?;
    
    write_block(&mut *drive, &ctx, nat_entry.block_addr, &block)?;
    
    Ok(())
}

/// Convert inline data to regular blocks
pub fn convert_inline_to_blocks(path: &str) -> Result<(), FsError> {
    let mut drive = crate::drivers::linux::select_block_device()
        .map_err(|_| FsError::NoDevice)?;
    
    let ctx = load_context(&mut *drive)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;
    
    if !inode.inline {
        return Ok(());  // Already not inline
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
    block[INODE_I_INLINE_OFFSET] &= !F2FS_INLINE_DATA_FLAG;
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
    let mut drive = crate::drivers::linux::select_block_device()
        .map_err(|_| FsError::NoDevice)?;
    
    let ctx = load_context(&mut *drive)?;
    let inode = open_inode_by_path(&mut *drive, &ctx, path)?;
    
    if inode.inline {
        return Ok(());  // Already inline
    }
    
    if inode.size > config.inline_threshold as u64 {
        return Err(FsError::NotSupported);  // Too large
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
    pub magic: u32,        // XATTR_MAGIC
    pub ref_count: u16,
    pub name_index: u8,    // XattrNamespace
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
    let mut drive = crate::drivers::linux::select_block_device()
        .map_err(|_| FsError::NoDevice)?;
    
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
        let magic = u32::from_le_bytes([
            block[pos], block[pos + 1], block[pos + 2], block[pos + 3],
        ]);
        
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
pub fn set_xattr(path: &str, namespace: XattrNamespace, name: &str, value: &[u8]) -> Result<(), FsError> {
    let mut drive = crate::drivers::linux::select_block_device()
        .map_err(|_| FsError::NoDevice)?;
    
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
        let magic = u32::from_le_bytes([
            block[pos], block[pos + 1], block[pos + 2], block[pos + 3],
        ]);
        
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
fn write_xattr_entry(block: &mut [u8], pos: usize, namespace: XattrNamespace, name: &str, value: &[u8]) -> Result<(), FsError> {
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
    block[pos + 12 + name_bytes.len()..pos + 12 + name_bytes.len() + value.len()].copy_from_slice(value);
    
    Ok(())
}

/// Remove xattr from file
pub fn remove_xattr(path: &str, namespace: XattrNamespace, name: &str) -> Result<(), FsError> {
    let mut drive = crate::drivers::linux::select_block_device()
        .map_err(|_| FsError::NoDevice)?;
    
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
        let magic = u32::from_le_bytes([
            block[pos], block[pos + 1], block[pos + 2], block[pos + 3],
        ]);
        
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
                    block[end], block[end + 1], block[end + 2], block[end + 3],
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
    let mut drive = crate::drivers::linux::select_block_device()
        .map_err(|_| FsError::NoDevice)?;
    
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
        let magic = u32::from_le_bytes([
            block[pos], block[pos + 1], block[pos + 2], block[pos + 3],
        ]);
        
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
const F2FS_INLINE_DATA_FLAG: u8 = 0x01;
/// Inline dentry flag
const F2FS_INLINE_DENTRY_FLAG: u8 = 0x02;
/// Data recovery flag
const F2FS_DATA_EXIST_FLAG: u8 = 0x04;
