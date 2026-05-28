//! # SquashFS 4.0 Read-Only Mount Module
//!
//! SquashFS is a compressed read-only filesystem commonly used for live CDs,
//! embedded systems, and firmware images. This module implements a complete
//! SquashFS 4.0 parser and reader for the echOS kernel.
//!
//! ## On-Disk Layout
//!
//! ```text
//!  ---------------
//! |  superblock   |  (96 bytes at offset 0)
//! |---------------|
//! |  compression  |  (optional, always uncompressed metadata block)
//! |    options    |
//! |---------------|
//! |  datablocks   |
//! |  & fragments  |
//! |---------------|
//! |  inode table  |  (metadata blocks, 8KB uncompressed each)
//! |---------------|
//! |   directory   |
//! |     table     |  (metadata blocks, 8KB uncompressed each)
//! |---------------|
//! |   fragment    |
//! |    table      |  (optional)
//! |---------------|
//! |    export     |
//! |    table      |  (optional)
//! |---------------|
//! |    uid/gid    |
//! |  lookup table |
//! |---------------|
//! |     xattr     |
//! |     table     |  (optional)
//!  ---------------
//! ```
//!
//! ## Features
//!
//! - Superblock parsing and validation
//! - Basic and extended inodes (directory, file, symlink)
//! - Directory traversal with directory headers and entries
//! - File reading with per-block decompression
//! - Fragment table support
//! - Compressors: gzip(1), lzo(3), xz(4), lz4(5), zstd(6)
//! - Metadata block decompression (8KB blocks with 2-byte length prefix)
//! - Global mount registry with concurrent access

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// Magic Numbers & Constants
// ============================================================================

/// SquashFS superblock magic: 0x73717368 ("hsqs" little-endian)
pub const SQUASHFS_MAGIC: u32 = 0x73717368;

/// Superblock size in bytes
pub const SQUASHFS_SUPERBLOCK_SIZE: usize = 96;

/// Metadata block uncompressed size
pub const SQUASHFS_METADATA_SIZE: usize = 8192;

/// Metadata block length prefix top bit = uncompressed flag
pub const SQUASHFS_COMPRESSED_BIT: u16 = 1 << 15;

/// Fragment block uncompressed flag (bit 24)
pub const SQUASHFS_FRAGMENT_COMPRESSED_BIT: u32 = 1 << 24;

/// Data block uncompressed flag (bit 24)
pub const SQUASHFS_DATA_BLOCK_COMPRESSED_BIT: u32 = 1 << 24;

/// Invalid fragment index marker
pub const SQUASHFS_INVALID_FRAG: u32 = 0xFFFFFFFF;

/// Invalid table start marker
pub const SQUASHFS_INVALID_TABLE: u64 = 0xFFFFFFFFFFFFFFFF;

/// Compressor IDs
pub const SQUASHFS_COMP_GZIP: u16 = 1;
pub const SQUASHFS_COMP_LZMA: u16 = 2;
pub const SQUASHFS_COMP_LZO: u16 = 3;
pub const SQUASHFS_COMP_XZ: u16 = 4;
pub const SQUASHFS_COMP_LZ4: u16 = 5;
pub const SQUASHFS_COMP_ZSTD: u16 = 6;

/// Superblock flags
pub const SQUASHFS_UNCOMPRESSED_INODES: u16 = 0x0001;
pub const SQUASHFS_UNCOMPRESSED_DATA: u16 = 0x0002;
pub const SQUASHFS_COMP_OPT: u16 = 0x0004;
pub const SQUASHFS_UNCOMPRESSED_FRAGMENTS: u16 = 0x0008;
pub const SQUASHFS_NO_FRAGMENTS: u16 = 0x0010;

/// Inode types
pub const SQUASHFS_DIR_TYPE: u16 = 1;
pub const SQUASHFS_FILE_TYPE: u16 = 2;
pub const SQUASHFS_SYMLINK_TYPE: u16 = 3;
pub const SQUASHFS_BLKDEV_TYPE: u16 = 4;
pub const SQUASHFS_CHRDEV_TYPE: u16 = 5;
pub const SQUASHFS_FIFO_TYPE: u16 = 6;
pub const SQUASHFS_SOCKET_TYPE: u16 = 7;
pub const SQUASHFS_DIR_EXT_TYPE: u16 = 8;
pub const SQUASHFS_FILE_EXT_TYPE: u16 = 9;
pub const SQUASHFS_SYMLINK_EXT_TYPE: u16 = 10;
pub const SQUASHFS_BLKDEV_EXT_TYPE: u16 = 11;
pub const SQUASHFS_CHRDEV_EXT_TYPE: u16 = 12;
pub const SQUASHFS_FIFO_EXT_TYPE: u16 = 13;
pub const SQUASHFS_SOCKET_EXT_TYPE: u16 = 14;

// ============================================================================
// On-Disk Structures
// ============================================================================

/// SquashFS superblock (96 bytes at offset 0)
#[derive(Clone, Debug)]
pub struct SquashfsSuperblock {
    pub magic: u32,
    pub inodes: u32,
    pub mkfs_time: u32,
    pub block_size: u32,
    pub fragments: u32,
    pub compression_id: u16,
    pub block_log: u16,
    pub flags: u16,
    pub no_ids: u16,
    pub s_major: u16,
    pub s_minor: u16,
    pub root_inode_ref: u64,
    pub bytes_used: u64,
    pub id_table_start: u64,
    pub xattr_id_table_start: u64,
    pub inode_table_start: u64,
    pub directory_table_start: u64,
    pub fragment_table_start: u64,
    pub export_table_start: u64,
}

impl SquashfsSuperblock {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < SQUASHFS_SUPERBLOCK_SIZE {
            return None;
        }

        let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        if magic != SQUASHFS_MAGIC {
            return None;
        }

        let s_major = u16::from_le_bytes([data[0x1C], data[0x1D]]);
        let s_minor = u16::from_le_bytes([data[0x1E], data[0x1F]]);
        if s_major != 4 || s_minor != 0 {
            return None;
        }

        let frag_table = u64::from_le_bytes([
            data[0x50], data[0x51], data[0x52], data[0x53], data[0x54], data[0x55], data[0x56],
            data[0x57],
        ]);

        Some(Self {
            magic,
            inodes: u32::from_le_bytes([data[0x04], data[0x05], data[0x06], data[0x07]]),
            mkfs_time: u32::from_le_bytes([data[0x08], data[0x09], data[0x0A], data[0x0B]]),
            block_size: u32::from_le_bytes([data[0x0C], data[0x0D], data[0x0E], data[0x0F]]),
            fragments: u32::from_le_bytes([data[0x10], data[0x11], data[0x12], data[0x13]]),
            compression_id: u16::from_le_bytes([data[0x14], data[0x15]]),
            block_log: u16::from_le_bytes([data[0x16], data[0x17]]),
            flags: u16::from_le_bytes([data[0x18], data[0x19]]),
            no_ids: u16::from_le_bytes([data[0x1A], data[0x1B]]),
            s_major,
            s_minor,
            root_inode_ref: u64::from_le_bytes([
                data[0x20], data[0x21], data[0x22], data[0x23], data[0x24], data[0x25], data[0x26],
                data[0x27],
            ]),
            bytes_used: u64::from_le_bytes([
                data[0x28], data[0x29], data[0x2A], data[0x2B], data[0x2C], data[0x2D], data[0x2E],
                data[0x2F],
            ]),
            id_table_start: u64::from_le_bytes([
                data[0x30], data[0x31], data[0x32], data[0x33], data[0x34], data[0x35], data[0x36],
                data[0x37],
            ]),
            xattr_id_table_start: u64::from_le_bytes([
                data[0x38], data[0x39], data[0x3A], data[0x3B], data[0x3C], data[0x3D], data[0x3E],
                data[0x3F],
            ]),
            inode_table_start: u64::from_le_bytes([
                data[0x40], data[0x41], data[0x42], data[0x43], data[0x44], data[0x45], data[0x46],
                data[0x47],
            ]),
            directory_table_start: u64::from_le_bytes([
                data[0x48], data[0x49], data[0x4A], data[0x4B], data[0x4C], data[0x4D], data[0x4E],
                data[0x4F],
            ]),
            fragment_table_start: frag_table,
            export_table_start: u64::from_le_bytes([
                data[0x58], data[0x59], data[0x5A], data[0x5B], data[0x5C], data[0x5D], data[0x5E],
                data[0x5F],
            ]),
        })
    }

    pub fn block_size(&self) -> u32 {
        self.block_size
    }

    pub fn compression_id(&self) -> u16 {
        self.compression_id
    }

    pub fn inode_table_start(&self) -> u64 {
        self.inode_table_start
    }

    pub fn directory_table_start(&self) -> u64 {
        self.directory_table_start
    }

    pub fn root_inode_ref(&self) -> u64 {
        self.root_inode_ref
    }

    pub fn bytes_used(&self) -> u64 {
        self.bytes_used
    }

    pub fn has_fragments(&self) -> bool {
        self.flags & SQUASHFS_NO_FRAGMENTS == 0
            && self.fragment_table_start != SQUASHFS_INVALID_TABLE
    }

    pub fn inodes_uncompressed(&self) -> bool {
        self.flags & SQUASHFS_UNCOMPRESSED_INODES != 0
    }

    pub fn data_uncompressed(&self) -> bool {
        self.flags & SQUASHFS_UNCOMPRESSED_DATA != 0
    }

    pub fn fragments_uncompressed(&self) -> bool {
        self.flags & SQUASHFS_UNCOMPRESSED_FRAGMENTS != 0
    }

    pub fn compressor_name(&self) -> &'static str {
        match self.compression_id {
            SQUASHFS_COMP_GZIP => "gzip",
            SQUASHFS_COMP_LZMA => "lzma",
            SQUASHFS_COMP_LZO => "lzo",
            SQUASHFS_COMP_XZ => "xz",
            SQUASHFS_COMP_LZ4 => "lz4",
            SQUASHFS_COMP_ZSTD => "zstd",
            _ => "unknown",
        }
    }
}

/// Inode reference: encodes metablock offset (bits 16-48) and byte offset (bits 0-15)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SquashfsInodeRef {
    pub raw: u64,
}

impl SquashfsInodeRef {
    pub fn new(raw: u64) -> Self {
        Self { raw }
    }

    /// Metablock offset (added to table_start to get absolute position)
    pub fn metablock_offset(&self) -> u64 {
        (self.raw >> 16) & 0xFFFFFFFFFFFF
    }

    /// Byte offset into the uncompressed metadata block
    pub fn byte_offset(&self) -> usize {
        (self.raw & 0xFFFF) as usize
    }

    pub fn from_parts(metablock_offset: u64, byte_offset: usize) -> Self {
        Self {
            raw: ((metablock_offset & 0xFFFFFFFFFFFF) << 16) | (byte_offset as u64 & 0xFFFF),
        }
    }
}

/// SquashFS inode types
#[derive(Clone, Debug)]
pub enum SquashfsInode {
    BasicDirectory {
        mode: u16,
        uid: u16,
        gid: u16,
        mtime: u32,
        start_block: u32,
        link_count: u32,
        file_size: u16,
        offset: u16,
        parent_inode: u32,
    },
    ExtendedDirectory {
        mode: u16,
        uid: u16,
        gid: u16,
        mtime: u32,
        link_count: u32,
        file_size: u32,
        start_block: u32,
        parent_inode: u32,
        index_count: u32,
        offset: u32,
        xattr: u32,
    },
    BasicFile {
        mode: u16,
        uid: u16,
        gid: u16,
        mtime: u32,
        start_block: u32,
        fragment_index: u32,
        block_offset: u32,
        file_size: u32,
        block_sizes: Vec<u32>,
    },
    ExtendedFile {
        mode: u16,
        uid: u16,
        gid: u16,
        mtime: u32,
        start_block: u64,
        file_size: u64,
        sparse: u64,
        link_count: u32,
        fragment_index: u32,
        block_offset: u32,
        xattr: u32,
        block_sizes: Vec<u32>,
    },
    BasicSymlink {
        mode: u16,
        uid: u16,
        gid: u16,
        mtime: u32,
        symlink_target: String,
    },
    ExtendedSymlink {
        mode: u16,
        uid: u16,
        gid: u16,
        mtime: u32,
        symlink_target: String,
        xattr: u32,
    },
    BasicDevice {
        inode_type: u16,
        mode: u16,
        uid: u16,
        gid: u16,
        mtime: u32,
        device_number: u32,
    },
    ExtendedDevice {
        inode_type: u16,
        mode: u16,
        uid: u16,
        gid: u16,
        mtime: u32,
        device_number: u32,
        xattr: u32,
    },
    BasicFifo {
        mode: u16,
        uid: u16,
        gid: u16,
        mtime: u32,
    },
    ExtendedFifo {
        mode: u16,
        uid: u16,
        gid: u16,
        mtime: u32,
        xattr: u32,
    },
}

impl SquashfsInode {
    pub fn inode_type(&self) -> u16 {
        match self {
            SquashfsInode::BasicDirectory { .. } => SQUASHFS_DIR_TYPE,
            SquashfsInode::ExtendedDirectory { .. } => SQUASHFS_DIR_EXT_TYPE,
            SquashfsInode::BasicFile { .. } => SQUASHFS_FILE_TYPE,
            SquashfsInode::ExtendedFile { .. } => SQUASHFS_FILE_EXT_TYPE,
            SquashfsInode::BasicSymlink { .. } => SQUASHFS_SYMLINK_TYPE,
            SquashfsInode::ExtendedSymlink { .. } => SQUASHFS_SYMLINK_EXT_TYPE,
            SquashfsInode::BasicDevice { inode_type, .. } => *inode_type,
            SquashfsInode::ExtendedDevice { inode_type, .. } => *inode_type,
            SquashfsInode::BasicFifo { .. } => SQUASHFS_FIFO_TYPE,
            SquashfsInode::ExtendedFifo { .. } => SQUASHFS_FIFO_EXT_TYPE,
        }
    }

    pub fn is_dir(&self) -> bool {
        matches!(
            self,
            SquashfsInode::BasicDirectory { .. } | SquashfsInode::ExtendedDirectory { .. }
        )
    }

    pub fn is_file(&self) -> bool {
        matches!(
            self,
            SquashfsInode::BasicFile { .. } | SquashfsInode::ExtendedFile { .. }
        )
    }

    pub fn is_symlink(&self) -> bool {
        matches!(
            self,
            SquashfsInode::BasicSymlink { .. } | SquashfsInode::ExtendedSymlink { .. }
        )
    }

    pub fn file_size(&self) -> u64 {
        match self {
            SquashfsInode::BasicDirectory { file_size, .. } => (*file_size as u64) + 3,
            SquashfsInode::ExtendedDirectory { file_size, .. } => *file_size as u64,
            SquashfsInode::BasicFile { file_size, .. } => *file_size as u64,
            SquashfsInode::ExtendedFile { file_size, .. } => *file_size,
            SquashfsInode::BasicSymlink { symlink_target, .. } => symlink_target.len() as u64,
            SquashfsInode::ExtendedSymlink { symlink_target, .. } => symlink_target.len() as u64,
            _ => 0,
        }
    }

    pub fn mode(&self) -> u16 {
        match self {
            SquashfsInode::BasicDirectory { mode, .. } => *mode,
            SquashfsInode::ExtendedDirectory { mode, .. } => *mode,
            SquashfsInode::BasicFile { mode, .. } => *mode,
            SquashfsInode::ExtendedFile { mode, .. } => *mode,
            SquashfsInode::BasicSymlink { mode, .. } => *mode,
            SquashfsInode::ExtendedSymlink { mode, .. } => *mode,
            SquashfsInode::BasicDevice { mode, .. } => *mode,
            SquashfsInode::ExtendedDevice { mode, .. } => *mode,
            SquashfsInode::BasicFifo { mode, .. } => *mode,
            SquashfsInode::ExtendedFifo { mode, .. } => *mode,
        }
    }

    pub fn symlink_target(&self) -> Option<&str> {
        match self {
            SquashfsInode::BasicSymlink { symlink_target, .. } => Some(symlink_target),
            SquashfsInode::ExtendedSymlink { symlink_target, .. } => Some(symlink_target),
            _ => None,
        }
    }
}

/// Directory entry (on-disk format)
#[derive(Clone, Debug)]
pub struct SquashfsDirent {
    pub offset: i16,
    pub inode_number: i16,
    pub entry_type: u16,
    pub name_size: u16,
}

impl SquashfsDirent {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }
        Some(Self {
            offset: i16::from_le_bytes([data[0], data[1]]),
            inode_number: i16::from_le_bytes([data[2], data[3]]),
            entry_type: u16::from_le_bytes([data[4], data[5]]),
            name_size: u16::from_le_bytes([data[6], data[7]]),
        })
    }
}

/// Directory entry with resolved name and inode reference
#[derive(Clone, Debug)]
pub struct SquashfsDirentWithNames {
    pub name: String,
    pub inode_ref: SquashfsInodeRef,
    pub entry_type: u16,
}

/// Directory header (groups entries with same start_block)
#[derive(Clone, Debug)]
struct SquashfsDirHeader {
    pub count: u32,
    pub start_block: u32,
    pub inode_number: u32,
}

impl SquashfsDirHeader {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 12 {
            return None;
        }
        Some(Self {
            count: u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
            start_block: u32::from_le_bytes([data[4], data[5], data[6], data[7]]),
            inode_number: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
        })
    }
}

/// Fragment table entry
#[derive(Clone, Copy, Debug)]
struct SquashfsFragmentEntry {
    pub start: u64,
    pub size: u32,
}

impl SquashfsFragmentEntry {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 16 {
            return None;
        }
        Some(Self {
            start: u64::from_le_bytes([
                data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
            ]),
            size: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
        })
    }

    pub fn is_compressed(&self) -> bool {
        self.size & SQUASHFS_FRAGMENT_COMPRESSED_BIT == 0
    }

    pub fn compressed_size(&self) -> u32 {
        self.size & 0x00FFFFFF
    }
}

// ============================================================================
// Storage
// ============================================================================

#[derive(Clone, Debug)]
pub enum SquashfsStorage {
    Resident(Arc<Vec<u8>>),
}

impl SquashfsStorage {
    pub fn read_range(&self, offset: usize, len: usize) -> Result<Vec<u8>, &'static str> {
        let end = offset
            .checked_add(len)
            .ok_or("squashfs: address overflow while reading")?;
        match self {
            SquashfsStorage::Resident(image) => {
                if end > image.len() {
                    return Err("squashfs: read exceeds resident image");
                }
                Ok(image[offset..end].to_vec())
            }
        }
    }
}

// ============================================================================
// Mounted SquashFS
// ============================================================================

#[derive(Clone, Debug)]
pub struct MountedSquashfs {
    pub fs: SquashfsFilesystem,
    pub storage: SquashfsStorage,
}

// ============================================================================
// SquashFS Filesystem Manager
// ============================================================================

#[derive(Debug)]
pub struct SquashfsFilesystem {
    pub superblock: SquashfsSuperblock,
    pub mount_point: String,
    inode_cache: Mutex<BTreeMap<u64, SquashfsInode>>,
    dir_cache: Mutex<BTreeMap<u64, Vec<SquashfsDirentWithNames>>>,
    fragment_entries: Mutex<Vec<SquashfsFragmentEntry>>,
    fragments_loaded: AtomicU64,
}

impl Clone for SquashfsFilesystem {
    fn clone(&self) -> Self {
        Self {
            superblock: self.superblock.clone(),
            mount_point: self.mount_point.clone(),
            inode_cache: Mutex::new(self.inode_cache.lock().clone()),
            dir_cache: Mutex::new(self.dir_cache.lock().clone()),
            fragment_entries: Mutex::new(self.fragment_entries.lock().clone()),
            fragments_loaded: AtomicU64::new(self.fragments_loaded.load(Ordering::Acquire)),
        }
    }
}

impl SquashfsFilesystem {
    pub fn new(sb: SquashfsSuperblock, mount_point: &str) -> Self {
        Self {
            superblock: sb,
            mount_point: String::from(mount_point),
            inode_cache: Mutex::new(BTreeMap::new()),
            dir_cache: Mutex::new(BTreeMap::new()),
            fragment_entries: Mutex::new(Vec::new()),
            fragments_loaded: AtomicU64::new(0),
        }
    }

    pub fn mount_from_data(disk_data: &[u8], mount_point: &str) -> Result<(), &'static str> {
        if disk_data.len() < SQUASHFS_SUPERBLOCK_SIZE {
            return Err("squashfs: disk too small for superblock");
        }

        let sb = SquashfsSuperblock::from_bytes(disk_data)
            .ok_or("squashfs: invalid superblock magic or version")?;

        if sb.block_size < 4096 || sb.block_size > 1048576 {
            return Err("squashfs: invalid block size (must be 4K-1M)");
        }

        if (sb.block_size & (sb.block_size - 1)) != 0 {
            return Err("squashfs: block size must be a power of 2");
        }

        let storage = SquashfsStorage::Resident(Arc::new(disk_data.to_vec()));
        let fs = Self::new(sb, mount_point);

        fs.load_fragment_table(&storage)?;

        let root_ref = SquashfsInodeRef::new(fs.superblock.root_inode_ref);
        let root_inode = fs.get_inode(root_ref)?;
        if !root_inode.is_dir() {
            return Err("squashfs: root inode is not a directory");
        }
        fs.inode_cache.lock().insert(root_ref.raw, root_inode);

        fs.print_info();

        SQUASHFS_FILESYSTEMS
            .lock()
            .insert(mount_point.to_string(), MountedSquashfs { fs, storage });

        SQUASHFS_MOUNT_COUNT.fetch_add(1, Ordering::AcqRel);

        Ok(())
    }

    pub fn resolve_path(&self, path: &str) -> Result<SquashfsInodeRef, &'static str> {
        let trimmed = path.trim_matches('/');
        if trimmed.is_empty() {
            return Ok(SquashfsInodeRef::new(self.superblock.root_inode_ref));
        }

        let mut current = SquashfsInodeRef::new(self.superblock.root_inode_ref);

        for component in trimmed.split('/').filter(|c| !c.is_empty()) {
            let inode = self.get_inode(current)?;
            if !inode.is_dir() {
                return Err("squashfs: parent is not a directory");
            }

            let entries = self.list_directory(current)?;
            let next = entries
                .iter()
                .find(|e| e.name == component)
                .map(|e| e.inode_ref)
                .ok_or("squashfs: component not found")?;

            current = next;
        }

        Ok(current)
    }

    pub fn get_inode(&self, inode_ref: SquashfsInodeRef) -> Result<SquashfsInode, &'static str> {
        if let Some(cached) = self.inode_cache.lock().get(&inode_ref.raw) {
            return Ok(cached.clone());
        }

        let storage = self.storage_ref()?;
        let table_start = self.superblock.inode_table_start as usize;
        let metablock_offset = inode_ref.metablock_offset() as usize;
        let byte_offset = inode_ref.byte_offset();

        let metablock_data = read_metadata_block(
            &storage,
            table_start + metablock_offset,
            self.superblock.inodes_uncompressed(),
            self.superblock.compression_id,
        )?;

        if byte_offset >= metablock_data.len() {
            return Err("squashfs: inode byte offset exceeds metablock");
        }

        let inode = parse_inode(&metablock_data[byte_offset..], inode_ref)?;

        self.inode_cache.lock().insert(inode_ref.raw, inode.clone());

        Ok(inode)
    }

    pub fn list_directory(
        &self,
        inode_ref: SquashfsInodeRef,
    ) -> Result<Vec<SquashfsDirentWithNames>, &'static str> {
        if let Some(cached) = self.dir_cache.lock().get(&inode_ref.raw) {
            return Ok(cached.clone());
        }

        let inode = self.get_inode(inode_ref)?;
        if !inode.is_dir() {
            return Err("squashfs: not a directory");
        }

        let (dir_start_block, dir_file_size) = match &inode {
            SquashfsInode::BasicDirectory {
                start_block,
                file_size,
                ..
            } => (*start_block as u64, (*file_size as u64) + 3),
            SquashfsInode::ExtendedDirectory {
                start_block,
                file_size,
                ..
            } => (*start_block as u64, *file_size as u64),
            _ => return Err("squashfs: not a directory inode"),
        };

        let storage = self.storage_ref()?;
        let table_start = self.superblock.directory_table_start as usize;

        let mut all_entries = Vec::new();
        let mut pos: usize = 0;

        while (pos as u64) < dir_file_size {
            let metablock_data = read_metadata_block(
                &storage,
                table_start + (dir_start_block as usize) + pos,
                self.superblock.inodes_uncompressed(),
                self.superblock.compression_id,
            )?;

            let mut local_pos = 0;
            while local_pos < metablock_data.len() && (pos + local_pos) < dir_file_size as usize {
                if local_pos + 12 > metablock_data.len() {
                    break;
                }

                let dh = SquashfsDirHeader::from_bytes(&metablock_data[local_pos..])
                    .ok_or("squashfs: invalid directory header")?;
                local_pos += 12;

                let entry_count = (dh.count + 1) as usize;

                for _ in 0..entry_count {
                    if local_pos + 8 > metablock_data.len() {
                        break;
                    }

                    let de = SquashfsDirent::from_bytes(&metablock_data[local_pos..])
                        .ok_or("squashfs: invalid directory entry")?;
                    local_pos += 8;

                    let name_len = (de.name_size as usize) + 1;
                    if local_pos + name_len > metablock_data.len() {
                        break;
                    }

                    let name_bytes = &metablock_data[local_pos..local_pos + name_len];
                    local_pos += name_len;

                    let name = core::str::from_utf8(name_bytes)
                        .map_err(|_| "squashfs: invalid utf-8 in directory entry")?;

                    let inode_offset = dh.inode_number as i32 + de.inode_number as i32;
                    let child_ref = SquashfsInodeRef::from_parts(
                        dir_start_block + (dh.start_block as u64),
                        inode_offset as usize,
                    );

                    all_entries.push(SquashfsDirentWithNames {
                        name: String::from(name),
                        inode_ref: child_ref,
                        entry_type: de.entry_type,
                    });
                }
            }

            pos += metablock_data.len();
        }

        self.dir_cache
            .lock()
            .insert(inode_ref.raw, all_entries.clone());

        Ok(all_entries)
    }

    pub fn read_file(
        &self,
        inode_ref: SquashfsInodeRef,
        storage: &SquashfsStorage,
    ) -> Result<Vec<u8>, &'static str> {
        let inode = self.get_inode(inode_ref)?;

        let (file_size, block_sizes, fragment_index, block_offset, start_block) = match &inode {
            SquashfsInode::BasicFile {
                file_size,
                block_sizes,
                fragment_index,
                block_offset,
                start_block,
                ..
            } => (
                *file_size as u64,
                block_sizes.clone(),
                *fragment_index,
                *block_offset,
                *start_block as u64,
            ),
            SquashfsInode::ExtendedFile {
                file_size,
                block_sizes,
                fragment_index,
                block_offset,
                start_block,
                ..
            } => (
                *file_size,
                block_sizes.clone(),
                *fragment_index,
                *block_offset,
                *start_block,
            ),
            SquashfsInode::BasicSymlink { symlink_target, .. } => {
                return Ok(symlink_target.as_bytes().to_vec());
            }
            SquashfsInode::ExtendedSymlink { symlink_target, .. } => {
                return Ok(symlink_target.as_bytes().to_vec());
            }
            _ => return Err("squashfs: not a regular file or symlink"),
        };

        let bs = self.superblock.block_size as usize;
        let num_full_blocks = block_sizes.len();
        let mut result = Vec::with_capacity(file_size as usize);

        let mut data_pos: usize = start_block as usize;

        for i in 0..num_full_blocks {
            let block_info = block_sizes[i];
            let compressed_size = (block_info & 0x00FFFFFF) as usize;
            let is_compressed = (block_info & SQUASHFS_DATA_BLOCK_COMPRESSED_BIT) == 0;

            let raw_data = storage.read_range(data_pos, compressed_size)?;
            data_pos += compressed_size;

            let remaining = file_size as usize - result.len();
            let block_data = if self.superblock.data_uncompressed() || is_compressed {
                raw_data
            } else {
                decompress_data(&raw_data, bs.max(remaining), self.superblock.compression_id)?
            };

            let to_copy = block_data.len().min(remaining);
            result.extend_from_slice(&block_data[..to_copy]);
        }

        if fragment_index != SQUASHFS_INVALID_FRAG {
            let frag_entries = self.fragment_entries.lock();
            if (fragment_index as usize) >= frag_entries.len() {
                return Err("squashfs: fragment index out of range");
            }

            let frag_entry = frag_entries[fragment_index as usize];
            let frag_compressed_size = frag_entry.compressed_size() as usize;
            let frag_is_compressed =
                frag_entry.is_compressed() && !self.superblock.fragments_uncompressed();

            let frag_raw = storage.read_range(frag_entry.start as usize, frag_compressed_size)?;

            let frag_data = if frag_is_compressed {
                decompress_data(
                    &frag_raw,
                    bs.max(self.superblock.block_size as usize),
                    self.superblock.compression_id,
                )?
            } else {
                frag_raw
            };

            let frag_start = block_offset as usize;
            let remaining = file_size as usize - result.len();
            let frag_end = (frag_start + remaining).min(frag_data.len());

            if frag_start < frag_data.len() {
                result.extend_from_slice(&frag_data[frag_start..frag_end]);
            }
        }

        result.truncate(file_size as usize);
        Ok(result)
    }

    pub fn print_info(&self) {
        crate::serial_println!("[SquashFS] === Filesystem Info ===");
        crate::serial_println!("[SquashFS] Magic: 0x{:08X}", self.superblock.magic);
        crate::serial_println!(
            "[SquashFS] Block size: {} bytes",
            self.superblock.block_size
        );
        crate::serial_println!(
            "[SquashFS] Compressor: {} (id={})",
            self.superblock.compressor_name(),
            self.superblock.compression_id
        );
        crate::serial_println!("[SquashFS] Total inodes: {}", self.superblock.inodes);
        crate::serial_println!("[SquashFS] Bytes used: {}", self.superblock.bytes_used);
        crate::serial_println!("[SquashFS] Fragments: {}", self.superblock.fragments);
        crate::serial_println!(
            "[SquashFS] Version: {}.{}",
            self.superblock.s_major,
            self.superblock.s_minor
        );
        crate::serial_println!("[SquashFS] Mount: {}", self.mount_point);
    }

    fn load_fragment_table(&self, storage: &SquashfsStorage) -> Result<(), &'static str> {
        if !self.superblock.has_fragments() || self.superblock.fragments == 0 {
            self.fragments_loaded.store(1, Ordering::Release);
            return Ok(());
        }

        let frag_table_start = self.superblock.fragment_table_start as usize;
        let num_fragments = self.superblock.fragments as usize;
        let entries_per_metablock = SQUASHFS_METADATA_SIZE / 16;
        let num_metablocks = (num_fragments + entries_per_metablock - 1) / entries_per_metablock;

        let mut all_entries = Vec::with_capacity(num_fragments);

        for i in 0..num_metablocks {
            let index_offset = frag_table_start + (i * 8);
            let metablock_ptr_data = storage.read_range(index_offset, 8)?;
            let metablock_start = u64::from_le_bytes([
                metablock_ptr_data[0],
                metablock_ptr_data[1],
                metablock_ptr_data[2],
                metablock_ptr_data[3],
                metablock_ptr_data[4],
                metablock_ptr_data[5],
                metablock_ptr_data[6],
                metablock_ptr_data[7],
            ]) as usize;

            let metablock_data = read_metadata_block(
                storage,
                metablock_start,
                self.superblock.fragments_uncompressed(),
                self.superblock.compression_id,
            )?;

            let entries_in_block = if i + 1 == num_metablocks {
                num_fragments - (i * entries_per_metablock)
            } else {
                entries_per_metablock
            };

            for j in 0..entries_in_block {
                let off = j * 16;
                if off + 16 > metablock_data.len() {
                    break;
                }
                if let Some(entry) = SquashfsFragmentEntry::from_bytes(&metablock_data[off..]) {
                    all_entries.push(entry);
                }
            }
        }

        *self.fragment_entries.lock() = all_entries;
        self.fragments_loaded.store(1, Ordering::Release);
        Ok(())
    }

    fn storage_ref(&self) -> Result<SquashfsStorage, &'static str> {
        let registry = SQUASHFS_FILESYSTEMS.lock();
        let mounted = registry
            .get(&self.mount_point)
            .ok_or("squashfs: mount point not found in registry")?;
        Ok(mounted.storage.clone())
    }
}

// ============================================================================
// Metadata Block Reading
// ============================================================================

fn read_metadata_block(
    storage: &SquashfsStorage,
    offset: usize,
    force_uncompressed: bool,
    compression_id: u16,
) -> Result<Vec<u8>, &'static str> {
    let len_data = storage.read_range(offset, 2)?;
    let len_le = u16::from_le_bytes([len_data[0], len_data[1]]);

    let compressed = (len_le & SQUASHFS_COMPRESSED_BIT) == 0 && !force_uncompressed;
    let size = (len_le & !SQUASHFS_COMPRESSED_BIT) as usize;

    if size == 0 || size > SQUASHFS_METADATA_SIZE * 2 {
        return Err("squashfs: invalid metadata block size");
    }

    let raw_data = storage.read_range(offset + 2, size)?;

    if compressed {
        decompress_metadata(&raw_data, compression_id)
    } else {
        Ok(raw_data)
    }
}

fn decompress_metadata(data: &[u8], compression_id: u16) -> Result<Vec<u8>, &'static str> {
    decompress_data(data, SQUASHFS_METADATA_SIZE * 2, compression_id)
}

fn decompress_data(
    data: &[u8],
    max_output: usize,
    compression_id: u16,
) -> Result<Vec<u8>, &'static str> {
    match compression_id {
        SQUASHFS_COMP_GZIP => crate::compression::deflate::decompress_deflate(data, max_output),
        SQUASHFS_COMP_LZO => crate::compression::lzo1x::decompress_lzo1x(data, max_output),
        SQUASHFS_COMP_XZ => Err("squashfs: XZ decompression not available"),
        SQUASHFS_COMP_LZ4 => match crate::compression::lz4::decompress_lz4(data, max_output) {
            Ok(result) => Ok(result),
            Err(_) => match crate::compression::lz4::decompress_lz4_unbounded(data) {
                Ok(result) => {
                    if result.len() > max_output {
                        Ok(result[..max_output].to_vec())
                    } else {
                        Ok(result)
                    }
                }
                Err(_) => Err("squashfs: LZ4 decompression failed"),
            },
        },
        SQUASHFS_COMP_ZSTD => crate::compression::zstd::decompress_zstd(data, max_output),
        _ => Err("squashfs: unsupported compressor"),
    }
}

// ============================================================================
// Inode Parsing
// ============================================================================

fn parse_inode(data: &[u8], inode_ref: SquashfsInodeRef) -> Result<SquashfsInode, &'static str> {
    if data.len() < 2 {
        return Err("squashfs: inode data too short");
    }

    let inode_type = u16::from_le_bytes([data[0], data[1]]);

    match inode_type {
        SQUASHFS_DIR_TYPE => parse_basic_dir_inode(data, inode_ref),
        SQUASHFS_FILE_TYPE => parse_basic_file_inode(data, inode_ref),
        SQUASHFS_SYMLINK_TYPE => parse_basic_symlink_inode(data),
        SQUASHFS_BLKDEV_TYPE | SQUASHFS_CHRDEV_TYPE => parse_basic_device_inode(data, inode_type),
        SQUASHFS_FIFO_TYPE | SQUASHFS_SOCKET_TYPE => parse_basic_fifo_inode(data, inode_type),
        SQUASHFS_DIR_EXT_TYPE => parse_extended_dir_inode(data),
        SQUASHFS_FILE_EXT_TYPE => parse_extended_file_inode(data, inode_ref),
        SQUASHFS_SYMLINK_EXT_TYPE => parse_extended_symlink_inode(data),
        SQUASHFS_BLKDEV_EXT_TYPE | SQUASHFS_CHRDEV_EXT_TYPE => {
            parse_extended_device_inode(data, inode_type)
        }
        SQUASHFS_FIFO_EXT_TYPE | SQUASHFS_SOCKET_EXT_TYPE => {
            parse_extended_fifo_inode(data, inode_type)
        }
        _ => Err("squashfs: unknown inode type"),
    }
}

fn parse_basic_dir_inode(
    data: &[u8],
    _inode_ref: SquashfsInodeRef,
) -> Result<SquashfsInode, &'static str> {
    if data.len() < 26 {
        return Err("squashfs: basic dir inode too short");
    }
    Ok(SquashfsInode::BasicDirectory {
        mode: u16::from_le_bytes([data[2], data[3]]),
        uid: u16::from_le_bytes([data[4], data[5]]),
        gid: u16::from_le_bytes([data[6], data[7]]),
        mtime: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
        start_block: u32::from_le_bytes([data[12], data[13], data[14], data[15]]),
        link_count: u32::from_le_bytes([data[16], data[17], data[18], data[19]]) + 1,
        file_size: u16::from_le_bytes([data[20], data[21]]),
        offset: u16::from_le_bytes([data[22], data[23]]),
        parent_inode: u32::from_le_bytes([data[24], data[25], data[26], data[27]]),
    })
}

fn parse_basic_file_inode(
    data: &[u8],
    inode_ref: SquashfsInodeRef,
) -> Result<SquashfsInode, &'static str> {
    if data.len() < 24 {
        return Err("squashfs: basic file inode too short");
    }

    let file_size = u32::from_le_bytes([data[24], data[25], data[26], data[27]]);
    let fragment_index = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);
    let block_offset = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);

    let bs = 1 << 16;
    let num_blocks = if fragment_index != SQUASHFS_INVALID_FRAG {
        if file_size == 0 {
            0
        } else {
            ((file_size as usize + bs - 1) / bs).saturating_sub(1)
        }
    } else {
        if file_size == 0 {
            0
        } else {
            (file_size as usize + bs - 1) / bs
        }
    };

    let mut block_sizes = Vec::with_capacity(num_blocks);
    let mut pos = 32;
    for _ in 0..num_blocks {
        if pos + 4 > data.len() {
            return Err("squashfs: truncated block sizes");
        }
        let bs_val = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
        pos += 4;
        block_sizes.push(bs_val);
    }

    Ok(SquashfsInode::BasicFile {
        mode: u16::from_le_bytes([data[2], data[3]]),
        uid: u16::from_le_bytes([data[4], data[5]]),
        gid: u16::from_le_bytes([data[6], data[7]]),
        mtime: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
        start_block: u32::from_le_bytes([data[12], data[13], data[14], data[15]]),
        fragment_index,
        block_offset,
        file_size,
        block_sizes,
    })
}

fn parse_basic_symlink_inode(data: &[u8]) -> Result<SquashfsInode, &'static str> {
    if data.len() < 16 {
        return Err("squashfs: basic symlink inode too short");
    }

    let symlink_size = u32::from_le_bytes([data[12], data[13], data[14], data[15]]) as usize;
    if data.len() < 16 + symlink_size {
        return Err("squashfs: truncated symlink target");
    }

    let target = core::str::from_utf8(&data[16..16 + symlink_size])
        .map_err(|_| "squashfs: invalid utf-8 in symlink target")?;

    Ok(SquashfsInode::BasicSymlink {
        mode: u16::from_le_bytes([data[2], data[3]]),
        uid: u16::from_le_bytes([data[4], data[5]]),
        gid: u16::from_le_bytes([data[6], data[7]]),
        mtime: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
        symlink_target: String::from(target),
    })
}

fn parse_basic_device_inode(data: &[u8], inode_type: u16) -> Result<SquashfsInode, &'static str> {
    if data.len() < 16 {
        return Err("squashfs: basic device inode too short");
    }
    Ok(SquashfsInode::BasicDevice {
        inode_type,
        mode: u16::from_le_bytes([data[2], data[3]]),
        uid: u16::from_le_bytes([data[4], data[5]]),
        gid: u16::from_le_bytes([data[6], data[7]]),
        mtime: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
        device_number: u32::from_le_bytes([data[12], data[13], data[14], data[15]]),
    })
}

fn parse_basic_fifo_inode(data: &[u8], inode_type: u16) -> Result<SquashfsInode, &'static str> {
    if data.len() < 12 {
        return Err("squashfs: basic fifo/socket inode too short");
    }
    Ok(SquashfsInode::BasicFifo {
        mode: u16::from_le_bytes([data[2], data[3]]),
        uid: u16::from_le_bytes([data[4], data[5]]),
        gid: u16::from_le_bytes([data[6], data[7]]),
        mtime: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
    })
}

fn parse_extended_dir_inode(data: &[u8]) -> Result<SquashfsInode, &'static str> {
    if data.len() < 40 {
        return Err("squashfs: extended dir inode too short");
    }
    Ok(SquashfsInode::ExtendedDirectory {
        mode: u16::from_le_bytes([data[2], data[3]]),
        uid: u16::from_le_bytes([data[4], data[5]]),
        gid: u16::from_le_bytes([data[6], data[7]]),
        mtime: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
        link_count: u32::from_le_bytes([data[12], data[13], data[14], data[15]]) + 1,
        file_size: u32::from_le_bytes([data[16], data[17], data[18], data[19]]),
        start_block: u32::from_le_bytes([data[20], data[21], data[22], data[23]]),
        parent_inode: u32::from_le_bytes([data[24], data[25], data[26], data[27]]),
        index_count: u32::from_le_bytes([data[28], data[29], data[30], data[31]]),
        offset: u32::from_le_bytes([data[32], data[33], data[34], data[35]]),
        xattr: u32::from_le_bytes([data[36], data[37], data[38], data[39]]),
    })
}

fn parse_extended_file_inode(
    data: &[u8],
    _inode_ref: SquashfsInodeRef,
) -> Result<SquashfsInode, &'static str> {
    if data.len() < 48 {
        return Err("squashfs: extended file inode too short");
    }

    let file_size = u64::from_le_bytes([
        data[24], data[25], data[26], data[27], data[28], data[29], data[30], data[31],
    ]);
    let fragment_index = u32::from_le_bytes([data[40], data[41], data[42], data[43]]);
    let block_offset = u32::from_le_bytes([data[44], data[45], data[46], data[47]]);

    let bs = 1 << 16;
    let num_blocks = if fragment_index != SQUASHFS_INVALID_FRAG {
        if file_size == 0 {
            0
        } else {
            ((file_size as usize + bs - 1) / bs).saturating_sub(1)
        }
    } else {
        if file_size == 0 {
            0
        } else {
            (file_size as usize + bs - 1) / bs
        }
    };

    let mut block_sizes = Vec::with_capacity(num_blocks);
    let mut pos = 56;
    for _ in 0..num_blocks {
        if pos + 4 > data.len() {
            return Err("squashfs: truncated block sizes in extended file");
        }
        let bs_val = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
        pos += 4;
        block_sizes.push(bs_val);
    }

    Ok(SquashfsInode::ExtendedFile {
        mode: u16::from_le_bytes([data[2], data[3]]),
        uid: u16::from_le_bytes([data[4], data[5]]),
        gid: u16::from_le_bytes([data[6], data[7]]),
        mtime: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
        start_block: u64::from_le_bytes([
            data[12], data[13], data[14], data[15], data[16], data[17], data[18], data[19],
        ]),
        file_size,
        sparse: u64::from_le_bytes([
            data[20], data[21], data[22], data[23], data[24], data[25], data[26], data[27],
        ]),
        link_count: u32::from_le_bytes([data[32], data[33], data[34], data[35]]),
        fragment_index,
        block_offset,
        xattr: u32::from_le_bytes([data[48], data[49], data[50], data[51]]),
        block_sizes,
    })
}

fn parse_extended_symlink_inode(data: &[u8]) -> Result<SquashfsInode, &'static str> {
    if data.len() < 24 {
        return Err("squashfs: extended symlink inode too short");
    }

    let symlink_size = u32::from_le_bytes([data[12], data[13], data[14], data[15]]) as usize;
    if data.len() < 20 + symlink_size {
        return Err("squashfs: truncated extended symlink target");
    }

    let target = core::str::from_utf8(&data[20..20 + symlink_size])
        .map_err(|_| "squashfs: invalid utf-8 in extended symlink target")?;

    let xattr_off = 20 + symlink_size;
    let xattr = if xattr_off + 4 <= data.len() {
        u32::from_le_bytes([
            data[xattr_off],
            data[xattr_off + 1],
            data[xattr_off + 2],
            data[xattr_off + 3],
        ])
    } else {
        0
    };

    Ok(SquashfsInode::ExtendedSymlink {
        mode: u16::from_le_bytes([data[2], data[3]]),
        uid: u16::from_le_bytes([data[4], data[5]]),
        gid: u16::from_le_bytes([data[6], data[7]]),
        mtime: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
        symlink_target: String::from(target),
        xattr,
    })
}

fn parse_extended_device_inode(
    data: &[u8],
    inode_type: u16,
) -> Result<SquashfsInode, &'static str> {
    if data.len() < 20 {
        return Err("squashfs: extended device inode too short");
    }
    Ok(SquashfsInode::ExtendedDevice {
        inode_type,
        mode: u16::from_le_bytes([data[2], data[3]]),
        uid: u16::from_le_bytes([data[4], data[5]]),
        gid: u16::from_le_bytes([data[6], data[7]]),
        mtime: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
        device_number: u32::from_le_bytes([data[12], data[13], data[14], data[15]]),
        xattr: u32::from_le_bytes([data[16], data[17], data[18], data[19]]),
    })
}

fn parse_extended_fifo_inode(data: &[u8], inode_type: u16) -> Result<SquashfsInode, &'static str> {
    if data.len() < 16 {
        return Err("squashfs: extended fifo/socket inode too short");
    }
    Ok(SquashfsInode::ExtendedFifo {
        mode: u16::from_le_bytes([data[2], data[3]]),
        uid: u16::from_le_bytes([data[4], data[5]]),
        gid: u16::from_le_bytes([data[6], data[7]]),
        mtime: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
        xattr: u32::from_le_bytes([data[12], data[13], data[14], data[15]]),
    })
}

// ============================================================================
// Global Registry
// ============================================================================

lazy_static::lazy_static! {
    static ref SQUASHFS_FILESYSTEMS: Mutex<BTreeMap<String, MountedSquashfs>> =
        Mutex::new(BTreeMap::new());
}

static SQUASHFS_MOUNT_COUNT: AtomicU64 = AtomicU64::new(0);

pub fn init() {
    crate::serial_println!("[SquashFS] SquashFS 4.0 filesystem module initialized");
    crate::serial_println!("[SquashFS] Compressors: gzip, lzo, lz4, zstd (xz unavailable)");
    crate::serial_println!(
        "[SquashFS] Features: basic/extended inodes, fragments, directory traversal"
    );
}

pub fn mount_from_data(disk_data: &[u8], mount_point: &str) -> Result<(), &'static str> {
    SquashfsFilesystem::mount_from_data(disk_data, mount_point)
}

pub fn unmount_squashfs(mount_point: &str) -> bool {
    let result = SQUASHFS_FILESYSTEMS.lock().remove(mount_point).is_some();
    if result {
        SQUASHFS_MOUNT_COUNT.fetch_sub(1, Ordering::AcqRel);
    }
    result
}

pub fn mounted_count() -> u64 {
    SQUASHFS_MOUNT_COUNT.load(Ordering::Acquire)
}

pub fn get_mounted_squashfs(mount_point: &str) -> Option<MountedSquashfs> {
    SQUASHFS_FILESYSTEMS.lock().get(mount_point).cloned()
}
