//! # EROFS (Enhanced Read-Only File System)
//!
//! EROFS is a space-efficient read-only filesystem used in Android and embedded Linux.
//! It supports compressed and uncompressed images, compact/extended inodes, and
//! inline data for small files.
//!
//! ## On-Disk Layout
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │  Superblock (offset 1024, 128 bytes base)           │
//! ├─────────────────────────────────────────────────────┤
//! │  Inode Metadata Zone (meta_blkaddr * block_size)    │
//! │    [32 * nid per inode]                             │
//! ├─────────────────────────────────────────────────────┤
//! │  Data Zone (file data blocks)                       │
//! ├─────────────────────────────────────────────────────┤
//! │  Xattr Zone (xattr_blkaddr * block_size)            │
//! └─────────────────────────────────────────────────────┘
//! ```
//!
//! ## Features
//!
//! - Compact (32-byte) and Extended (64-byte) inodes
//! - FLAT_PLAIN and FLAT_INLINE data layouts
//! - LZ4/ZSTD compressed data blocks
//! - CRC32-C superblock checksum verification
//! - Sorted directory entries with binary search

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// Magic Numbers & Constants
// ============================================================================

/// EROFS superblock magic: 0xE0F5E1E2
pub const EROFS_MAGIC: u32 = 0xE0F5E1E2;

/// Superblock offset in bytes
pub const EROFS_SUPER_OFFSET: usize = 1024;

/// Base superblock size in bytes
pub const EROFS_SUPERBLOCK_BASE_SIZE: usize = 128;

/// Compact inode size in bytes
pub const EROFS_INODE_COMPACT_SIZE: usize = 32;

/// Extended inode size in bytes
pub const EROFS_INODE_EXTENDED_SIZE: usize = 64;

/// Directory entry size in bytes
pub const EROFS_DIRENT_SIZE: usize = 12;

/// Inode slot size (NID-to-offset multiplier)
pub const EROFS_INODE_SLOT_BITS: u8 = 5;
pub const EROFS_INODE_SLOT_SIZE: u32 = 1 << EROFS_INODE_SLOT_BITS;

// Feature flags
pub const EROFS_FEATURE_COMPAT_SB_CHKSUM: u32 = 0x00000001;
pub const EROFS_FEATURE_COMPAT_MTIME: u32 = 0x00000002;

// i_format data layout values
pub const EROFS_INODE_FLAT_PLAIN: u8 = 0;
pub const EROFS_INODE_FLAT_INLINE: u8 = 2;

// File type values
pub const EROFS_FT_UNKNOWN: u8 = 0;
pub const EROFS_FT_REG_FILE: u8 = 1;
pub const EROFS_FT_DIR: u8 = 2;
pub const EROFS_FT_CHRDEV: u8 = 3;
pub const EROFS_FT_BLKDEV: u8 = 4;
pub const EROFS_FT_FIFO: u8 = 5;
pub const EROFS_FT_SOCK: u8 = 6;
pub const EROFS_FT_SYMLINK: u8 = 7;

// i_format bit positions
pub const EROFS_I_VERSION_BIT: u16 = 0;
pub const EROFS_I_DATA_LAYOUT_BITS: u16 = 1;
pub const EROFS_I_DATA_LAYOUT_MASK: u16 = 0x000E;

// POSIX mode bits
pub const EROFS_S_IFMT: u16 = 0o170000;
pub const EROFS_S_IFREG: u16 = 0o100000;
pub const EROFS_S_IFDIR: u16 = 0o040000;
pub const EROFS_S_IFLNK: u16 = 0o120000;
pub const EROFS_S_IFCHR: u16 = 0o020000;
pub const EROFS_S_IFBLK: u16 = 0o060000;
pub const EROFS_S_IFIFO: u16 = 0o010000;
pub const EROFS_S_IFSOCK: u16 = 0o140000;

// ============================================================================
// CRC32-C
// ============================================================================

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

// ============================================================================
// On-Disk Structures
// ============================================================================

/// EROFS Superblock (at offset 1024)
#[derive(Clone, Debug)]
pub struct ErofsSuperblock {
    pub magic: u32,
    pub checksum: u32,
    pub feature_compat: u32,
    pub blkszbits: u8,
    pub sb_extslots: u8,
    pub rootnid: u16,
    pub inos: u64,
    pub epoch: u64,
    pub fixed_nsec: u32,
    pub blocks: u32,
    pub meta_blkaddr: u32,
    pub xattr_blkaddr: u32,
    pub uuid: [u8; 16],
    pub volume_name: [u8; 16],
    pub feature_incompat: u32,
    pub is_compressed: u16,
    pub dirblkbits: u8,
    pub reserved: [u8; 23],
}

impl ErofsSuperblock {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < EROFS_SUPER_OFFSET + EROFS_SUPERBLOCK_BASE_SIZE {
            return None;
        }

        let sb = &data[EROFS_SUPER_OFFSET..];

        let magic = u32::from_le_bytes([sb[0], sb[1], sb[2], sb[3]]);
        if magic != EROFS_MAGIC {
            return None;
        }

        let mut uuid = [0u8; 16];
        uuid.copy_from_slice(&sb[0x30..0x40]);

        let mut volume_name = [0u8; 16];
        volume_name.copy_from_slice(&sb[0x40..0x50]);

        let mut reserved = [0u8; 23];
        let reserved_src = &sb[0x6C..0x80];
        let copy_len = core::cmp::min(reserved.len(), reserved_src.len());
        reserved[..copy_len].copy_from_slice(&reserved_src[..copy_len]);

        Some(Self {
            magic,
            checksum: u32::from_le_bytes([sb[0x04], sb[0x05], sb[0x06], sb[0x07]]),
            feature_compat: u32::from_le_bytes([sb[0x08], sb[0x09], sb[0x0A], sb[0x0B]]),
            blkszbits: sb[0x0C],
            sb_extslots: sb[0x0D],
            rootnid: u16::from_le_bytes([sb[0x0E], sb[0x0F]]),
            inos: u64::from_le_bytes([
                sb[0x10], sb[0x11], sb[0x12], sb[0x13], sb[0x14], sb[0x15], sb[0x16], sb[0x17],
            ]),
            epoch: u64::from_le_bytes([
                sb[0x18], sb[0x19], sb[0x1A], sb[0x1B], sb[0x1C], sb[0x1D], sb[0x1E], sb[0x1F],
            ]),
            fixed_nsec: u32::from_le_bytes([sb[0x20], sb[0x21], sb[0x22], sb[0x23]]),
            blocks: u32::from_le_bytes([sb[0x24], sb[0x25], sb[0x26], sb[0x27]]),
            meta_blkaddr: u32::from_le_bytes([sb[0x28], sb[0x29], sb[0x2A], sb[0x2B]]),
            xattr_blkaddr: u32::from_le_bytes([sb[0x2C], sb[0x2D], sb[0x2E], sb[0x2F]]),
            uuid,
            volume_name,
            feature_incompat: u32::from_le_bytes([sb[0x50], sb[0x51], sb[0x52], sb[0x53]]),
            is_compressed: u16::from_le_bytes([sb[0x54], sb[0x55]]),
            dirblkbits: sb[0x5A],
            reserved,
        })
    }

    pub fn block_size(&self) -> u32 {
        1u32 << self.blkszbits
    }

    pub fn root_nid(&self) -> u64 {
        self.rootnid as u64
    }

    pub fn meta_blkaddr(&self) -> u32 {
        self.meta_blkaddr
    }

    pub fn volume_name_str(&self) -> &str {
        let end = self.volume_name.iter().position(|&b| b == 0).unwrap_or(16);
        core::str::from_utf8(&self.volume_name[..end]).unwrap_or("")
    }

    pub fn verify_checksum(&self, raw_sb_block: &[u8]) -> Result<(), &'static str> {
        if self.feature_compat & EROFS_FEATURE_COMPAT_SB_CHKSUM == 0 {
            return Ok(());
        }
        let computed = crc32c(raw_sb_block);
        if computed == self.checksum {
            Ok(())
        } else {
            Err("erofs: superblock checksum mismatch")
        }
    }

    pub fn is_compressed(&self) -> bool {
        self.is_compressed != 0
    }
}

/// Unified EROFS inode (handles both compact and extended formats)
#[derive(Clone, Debug)]
pub struct ErofsInode {
    pub nid: u64,
    pub is_extended: bool,
    pub i_mode: u16,
    pub i_nlink: u32,
    pub i_size: u64,
    pub start_block: u32,
    pub i_ino: u32,
    pub i_uid: u32,
    pub i_gid: u32,
    pub data_layout: u8,
    pub i_mtime: u64,
    pub i_mtime_nsec: u32,
}

impl ErofsInode {
    pub fn from_bytes(data: &[u8], nid: u64) -> Option<Self> {
        if data.len() < EROFS_INODE_COMPACT_SIZE {
            return None;
        }

        let i_format = u16::from_le_bytes([data[0], data[1]]);
        let is_extended = (i_format & (1 << EROFS_I_VERSION_BIT)) != 0;

        let data_layout = ((i_format & EROFS_I_DATA_LAYOUT_MASK) >> EROFS_I_DATA_LAYOUT_BITS) as u8;

        if data_layout >= 5 {
            return None;
        }

        if is_extended {
            if data.len() < EROFS_INODE_EXTENDED_SIZE {
                return None;
            }
            Some(Self {
                nid,
                is_extended: true,
                i_mode: u16::from_le_bytes([data[0x04], data[0x05]]),
                i_nlink: u32::from_le_bytes([data[0x2C], data[0x2D], data[0x2E], data[0x2F]]),
                i_size: u64::from_le_bytes([
                    data[0x08], data[0x09], data[0x0A], data[0x0B], data[0x0C], data[0x0D],
                    data[0x0E], data[0x0F],
                ]),
                start_block: u32::from_le_bytes([data[0x10], data[0x11], data[0x12], data[0x13]]),
                i_ino: u32::from_le_bytes([data[0x14], data[0x15], data[0x16], data[0x17]]),
                i_uid: u32::from_le_bytes([data[0x18], data[0x19], data[0x1A], data[0x1B]]),
                i_gid: u32::from_le_bytes([data[0x1C], data[0x1D], data[0x1E], data[0x1F]]),
                data_layout,
                i_mtime: u64::from_le_bytes([
                    data[0x20], data[0x21], data[0x22], data[0x23], data[0x24], data[0x25],
                    data[0x26], data[0x27],
                ]),
                i_mtime_nsec: u32::from_le_bytes([data[0x28], data[0x29], data[0x2A], data[0x2B]]),
            })
        } else {
            Some(Self {
                nid,
                is_extended: false,
                i_mode: u16::from_le_bytes([data[0x04], data[0x05]]),
                i_nlink: u16::from_le_bytes([data[0x06], data[0x07]]) as u32,
                i_size: u32::from_le_bytes([data[0x08], data[0x09], data[0x0A], data[0x0B]]) as u64,
                start_block: u32::from_le_bytes([data[0x10], data[0x11], data[0x12], data[0x13]]),
                i_ino: u32::from_le_bytes([data[0x14], data[0x15], data[0x16], data[0x17]]),
                i_uid: u16::from_le_bytes([data[0x18], data[0x19]]) as u32,
                i_gid: u16::from_le_bytes([data[0x1A], data[0x1B]]) as u32,
                data_layout,
                i_mtime: 0,
                i_mtime_nsec: 0,
            })
        }
    }

    pub fn is_file(&self) -> bool {
        self.i_mode & EROFS_S_IFMT == EROFS_S_IFREG
    }

    pub fn is_dir(&self) -> bool {
        self.i_mode & EROFS_S_IFMT == EROFS_S_IFDIR
    }

    pub fn is_symlink(&self) -> bool {
        self.i_mode & EROFS_S_IFMT == EROFS_S_IFLNK
    }

    pub fn size(&self) -> u64 {
        self.i_size
    }

    pub fn start_block(&self) -> u32 {
        self.start_block
    }
}

/// EROFS directory entry (12 bytes on-disk)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErofsDirent {
    pub nid: u64,
    pub nameoff: u16,
    pub file_type: u8,
}

impl ErofsDirent {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < EROFS_DIRENT_SIZE {
            return None;
        }
        Some(Self {
            nid: u64::from_le_bytes([
                data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
            ]),
            nameoff: u16::from_le_bytes([data[8], data[9]]),
            file_type: data[0x0A],
        })
    }
}

/// Directory entry with resolved filename
#[derive(Clone, Debug)]
pub struct ErofsDirentWithNames {
    pub nid: u64,
    pub name: String,
    pub file_type: u8,
}

// ============================================================================
// Storage
// ============================================================================

#[derive(Clone, Debug)]
pub enum ErofsStorage {
    Resident(Arc<Vec<u8>>),
}

impl ErofsStorage {
    pub fn read_range(&self, offset: usize, len: usize) -> Result<Vec<u8>, &'static str> {
        let end = offset
            .checked_add(len)
            .ok_or("erofs: address overflow while reading")?;
        match self {
            ErofsStorage::Resident(image) => {
                if end > image.len() {
                    return Err("erofs: read exceeds resident image");
                }
                Ok(image[offset..end].to_vec())
            }
        }
    }
}

// ============================================================================
// Mounted EROFS
// ============================================================================

#[derive(Clone, Debug)]
pub struct MountedErofs {
    pub fs: ErofsFilesystem,
    pub storage: ErofsStorage,
}

// ============================================================================
// EROFS Filesystem Manager
// ============================================================================

/// EROFS filesystem manager with interior-mutable caches
#[derive(Debug)]
pub struct ErofsFilesystem {
    pub superblock: ErofsSuperblock,
    pub mount_point: String,
    inode_cache: RefCell<BTreeMap<u64, ErofsInode>>,
    dir_cache: RefCell<BTreeMap<u64, Vec<ErofsDirentWithNames>>>,
}

impl Clone for ErofsFilesystem {
    fn clone(&self) -> Self {
        Self {
            superblock: self.superblock.clone(),
            mount_point: self.mount_point.clone(),
            inode_cache: RefCell::new(self.inode_cache.borrow().clone()),
            dir_cache: RefCell::new(self.dir_cache.borrow().clone()),
        }
    }
}

impl ErofsFilesystem {
    pub fn new(sb: ErofsSuperblock, mount_point: &str) -> Self {
        Self {
            superblock: sb,
            mount_point: String::from(mount_point),
            inode_cache: RefCell::new(BTreeMap::new()),
            dir_cache: RefCell::new(BTreeMap::new()),
        }
    }

    pub fn mount_from_data(disk_data: &[u8], mount_point: &str) -> Result<(), &'static str> {
        if disk_data.len() < EROFS_SUPER_OFFSET + EROFS_SUPERBLOCK_BASE_SIZE {
            return Err("erofs: disk too small for superblock");
        }

        let sb = ErofsSuperblock::from_bytes(disk_data).ok_or("erofs: invalid superblock magic")?;

        if sb.blkszbits < 9 {
            return Err("erofs: invalid block size bits (must be >= 9)");
        }

        let sb_block =
            &disk_data[EROFS_SUPER_OFFSET..EROFS_SUPER_OFFSET + EROFS_SUPERBLOCK_BASE_SIZE];
        sb.verify_checksum(sb_block)?;

        let storage = ErofsStorage::Resident(Arc::new(disk_data.to_vec()));
        let fs = Self::new(sb, mount_point);
        fs.print_info();

        EROFS_FILESYSTEMS
            .lock()
            .insert(mount_point.to_string(), MountedErofs { fs, storage });

        Ok(())
    }

    pub fn resolve_path(&self, path: &str) -> Result<u64, &'static str> {
        let trimmed = path.trim_matches('/');
        if trimmed.is_empty() {
            return Ok(self.superblock.root_nid());
        }

        let mut current = self.superblock.root_nid();

        for component in trimmed.split('/').filter(|c| !c.is_empty()) {
            if !self.get_inode(current)?.is_dir() {
                return Err("erofs: parent is not a directory");
            }

            let entries = self.list_directory(current)?;
            let next = entries
                .iter()
                .find(|e| e.name == component)
                .map(|e| e.nid)
                .ok_or("erofs: component not found")?;

            current = next;
        }

        Ok(current)
    }

    pub fn get_inode(&self, nid: u64) -> Result<ErofsInode, &'static str> {
        if let Some(cached) = self.inode_cache.borrow().get(&nid) {
            return Ok(cached.clone());
        }

        let bs = self.superblock.block_size() as usize;
        let meta_start = (self.superblock.meta_blkaddr as usize) * bs;
        let inode_offset = meta_start + (EROFS_INODE_SLOT_SIZE as usize) * (nid as usize);

        let storage = self.storage_ref()?;
        let inode_data = storage.read_range(inode_offset, EROFS_INODE_EXTENDED_SIZE)?;

        let inode = ErofsInode::from_bytes(&inode_data, nid).ok_or("erofs: invalid inode data")?;

        self.inode_cache.borrow_mut().insert(nid, inode.clone());

        Ok(inode)
    }

    pub fn list_directory(&self, nid: u64) -> Result<Vec<ErofsDirentWithNames>, &'static str> {
        let inode = self.get_inode(nid)?;
        if !inode.is_dir() {
            return Err("erofs: not a directory");
        }

        if let Some(cached) = self.dir_cache.borrow().get(&nid) {
            return Ok(cached.clone());
        }

        let bs = self.superblock.block_size() as usize;
        let dirblkbits = self.superblock.dirblkbits;
        let dir_block_size = bs << dirblkbits;

        let storage = self.storage_ref()?;
        let mut all_entries = Vec::new();

        let num_blocks = if inode.size() == 0 {
            1
        } else {
            ((inode.size() as usize + dir_block_size - 1) / dir_block_size).max(1)
        };

        for block_idx in 0..num_blocks {
            let block_offset = (inode.start_block as usize) * bs + block_idx * dir_block_size;
            let block_data = storage.read_range(block_offset, dir_block_size)?;

            let entries = self.parse_dir_block(&block_data, dir_block_size)?;
            all_entries.extend(entries);
        }

        self.dir_cache.borrow_mut().insert(nid, all_entries.clone());

        Ok(all_entries)
    }

    fn parse_dir_block(
        &self,
        block: &[u8],
        block_size: usize,
    ) -> Result<Vec<ErofsDirentWithNames>, &'static str> {
        if block.len() < EROFS_DIRENT_SIZE {
            return Err("erofs: directory block too small");
        }

        let first_nameoff = u16::from_le_bytes([block[8], block[9]]);
        let total_entries = (first_nameoff as usize) / EROFS_DIRENT_SIZE;

        if total_entries == 0 || total_entries * EROFS_DIRENT_SIZE > block.len() {
            return Err("erofs: invalid directory entry count");
        }

        let mut dirents = Vec::with_capacity(total_entries);
        for i in 0..total_entries {
            let off = i * EROFS_DIRENT_SIZE;
            let de = ErofsDirent::from_bytes(&block[off..off + EROFS_DIRENT_SIZE])
                .ok_or("erofs: invalid dirent")?;
            dirents.push(de);
        }

        let mut entries = Vec::with_capacity(total_entries);
        for (i, de) in dirents.iter().enumerate() {
            let name_start = de.nameoff as usize;
            let name_end = if i + 1 < total_entries {
                dirents[i + 1].nameoff as usize
            } else {
                block[name_start..block_size]
                    .iter()
                    .position(|&b| b == 0)
                    .map(|p| name_start + p)
                    .unwrap_or(block_size)
            };

            if name_start >= block_size || name_end > block_size || name_end <= name_start {
                return Err("erofs: invalid dirent name offsets");
            }

            let name = core::str::from_utf8(&block[name_start..name_end])
                .map_err(|_| "erofs: invalid utf-8 in directory entry")?;

            entries.push(ErofsDirentWithNames {
                nid: de.nid,
                name: String::from(name),
                file_type: de.file_type,
            });
        }

        Ok(entries)
    }

    pub fn read_file(&self, nid: u64, storage: &ErofsStorage) -> Result<Vec<u8>, &'static str> {
        let inode = self.get_inode(nid)?;
        if !inode.is_file() && !inode.is_symlink() {
            return Err("erofs: not a regular file or symlink");
        }

        let bs = self.superblock.block_size() as usize;
        let file_size = inode.size() as usize;

        match inode.data_layout {
            EROFS_INODE_FLAT_PLAIN => {
                let data_offset = inode.start_block as usize * bs;
                if self.superblock.is_compressed() {
                    self.read_compressed_plain(storage, inode.start_block, file_size, bs)
                } else {
                    storage.read_range(data_offset, file_size)
                }
            }
            EROFS_INODE_FLAT_INLINE => self.read_inline_file(&inode, storage, file_size, bs),
            _ => Err("erofs: unsupported data layout"),
        }
    }

    fn read_compressed_plain(
        &self,
        storage: &ErofsStorage,
        start_block: u32,
        file_size: usize,
        bs: usize,
    ) -> Result<Vec<u8>, &'static str> {
        let num_blocks = (file_size + bs - 1) / bs;
        let mut result = Vec::with_capacity(file_size);

        for i in 0..num_blocks {
            let block_offset = (start_block as usize + i) * bs;
            let raw_block = storage.read_range(block_offset, bs)?;
            let chunk_size = if i + 1 < num_blocks {
                bs
            } else {
                file_size - result.len()
            };
            let decompressed = decompress_erofs_data(&raw_block, chunk_size)?;
            result.extend_from_slice(&decompressed[..chunk_size.min(decompressed.len())]);
        }

        result.truncate(file_size);
        Ok(result)
    }

    fn read_inline_file(
        &self,
        inode: &ErofsInode,
        storage: &ErofsStorage,
        file_size: usize,
        bs: usize,
    ) -> Result<Vec<u8>, &'static str> {
        let meta_start = (self.superblock.meta_blkaddr as usize) * bs;
        let inode_offset = meta_start + (EROFS_INODE_SLOT_SIZE as usize) * (inode.nid as usize);
        let inode_in_block = inode_offset % bs;

        let inode_raw_size = if inode.is_extended {
            EROFS_INODE_EXTENDED_SIZE
        } else {
            EROFS_INODE_COMPACT_SIZE
        };

        let inline_data_start = inode_in_block + inode_raw_size;

        if inline_data_start >= bs {
            return Err("erofs: inline data offset exceeds block size");
        }

        let inline_capacity = bs - inline_data_start;

        let mut result = vec![0u8; file_size];

        if file_size <= inline_capacity {
            let inline_data = storage.read_range(inode_offset + inline_data_start, file_size)?;
            result[..file_size].copy_from_slice(&inline_data[..file_size]);
            return Ok(result);
        }

        let inline_data = storage.read_range(inode_offset + inline_data_start, inline_capacity)?;
        result[..inline_capacity].copy_from_slice(&inline_data[..inline_capacity]);

        let remaining = file_size - inline_capacity;
        if remaining > 0 {
            let data_start = inode.start_block as usize * bs;
            let block_data = storage.read_range(data_start, remaining)?;

            if self.superblock.is_compressed() {
                let decompressed = decompress_erofs_data(&block_data, remaining)?;
                let copy_len = core::cmp::min(decompressed.len(), remaining);
                result[inline_capacity..inline_capacity + copy_len]
                    .copy_from_slice(&decompressed[..copy_len]);
            } else {
                let copy_len = core::cmp::min(block_data.len(), remaining);
                result[inline_capacity..inline_capacity + copy_len]
                    .copy_from_slice(&block_data[..copy_len]);
            }
        }

        Ok(result)
    }

    pub fn print_info(&self) {
        crate::serial_println!("[EROFS] === Filesystem Info ===");
        crate::serial_println!("[EROFS] Label: {}", self.superblock.volume_name_str());
        crate::serial_println!("[EROFS] Block size: {} bytes", self.superblock.block_size());
        crate::serial_println!("[EROFS] Total inodes: {}", self.superblock.inos);
        crate::serial_println!("[EROFS] Total blocks: {}", self.superblock.blocks);
        crate::serial_println!("[EROFS] Meta blkaddr: {}", self.superblock.meta_blkaddr);
        crate::serial_println!("[EROFS] Xattr blkaddr: {}", self.superblock.xattr_blkaddr);
        crate::serial_println!(
            "[EROFS] Compressed: {}",
            if self.superblock.is_compressed() {
                "yes"
            } else {
                "no"
            }
        );
        crate::serial_println!("[EROFS] Root NID: {}", self.superblock.root_nid());
        crate::serial_println!("[EROFS] Mount: {}", self.mount_point);
    }

    fn storage_ref(&self) -> Result<ErofsStorage, &'static str> {
        let registry = EROFS_FILESYSTEMS.lock();
        let mounted = registry
            .get(&self.mount_point)
            .ok_or("erofs: mount point not found in registry")?;
        Ok(mounted.storage.clone())
    }
}

fn decompress_erofs_data(data: &[u8], decompressed_size: usize) -> Result<Vec<u8>, &'static str> {
    match crate::compression::lz4::decompress_lz4(data, decompressed_size) {
        Ok(result) => Ok(result),
        Err(_) => crate::compression::zstd::decompress_zstd(data, decompressed_size),
    }
}

// ============================================================================
// Global Registry
// ============================================================================

lazy_static::lazy_static! {
    static ref EROFS_FILESYSTEMS: Mutex<BTreeMap<String, MountedErofs>> =
        Mutex::new(BTreeMap::new());
}

static EROFS_MOUNT_COUNT: AtomicU64 = AtomicU64::new(0);

pub fn init() {
    crate::serial_println!("[EROFS] EROFS filesystem module initialized");
    crate::serial_println!(
        "[EROFS] Features: compact/extended inodes, FLAT_PLAIN, FLAT_INLINE, LZ4/ZSTD"
    );
}

pub fn mount_from_data(disk_data: &[u8], mount_point: &str) -> Result<(), &'static str> {
    if disk_data.len() < EROFS_SUPER_OFFSET + EROFS_SUPERBLOCK_BASE_SIZE {
        return Err("erofs: disk too small for superblock");
    }

    let sb = ErofsSuperblock::from_bytes(disk_data).ok_or("erofs: invalid superblock magic")?;

    if sb.blkszbits < 9 {
        return Err("erofs: invalid block size bits (must be >= 9)");
    }

    let sb_block = &disk_data[EROFS_SUPER_OFFSET..EROFS_SUPER_OFFSET + EROFS_SUPERBLOCK_BASE_SIZE];
    sb.verify_checksum(sb_block)?;

    let storage = ErofsStorage::Resident(Arc::new(disk_data.to_vec()));
    let fs = ErofsFilesystem::new(sb, mount_point);

    let root_nid = fs.superblock.root_nid();
    let root_inode = fs.get_inode(root_nid)?;
    fs.inode_cache.borrow_mut().insert(root_nid, root_inode);

    fs.print_info();

    EROFS_FILESYSTEMS
        .lock()
        .insert(mount_point.to_string(), MountedErofs { fs, storage });

    EROFS_MOUNT_COUNT.fetch_add(1, Ordering::AcqRel);

    Ok(())
}

pub fn unmount_erofs(mount_point: &str) -> bool {
    let result = EROFS_FILESYSTEMS.lock().remove(mount_point).is_some();
    if result {
        EROFS_MOUNT_COUNT.fetch_sub(1, Ordering::AcqRel);
    }
    result
}

pub fn mounted_count() -> u64 {
    EROFS_MOUNT_COUNT.load(Ordering::Acquire)
}

pub fn get_mounted_erofs(mount_point: &str) -> Option<MountedErofs> {
    EROFS_FILESYSTEMS.lock().get(mount_point).cloned()
}
