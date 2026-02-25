//! # ext4 File System
//!
//! Fourth Extended Filesystem implementation with journaling support

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;
use alloc::sync::Arc;
use spin::Mutex;
use core::mem;

use super::ext4_journal::{Journal, JournalError, Transaction, TransactionState};

// ============================================================================
// ext4 CONSTANTS
// ============================================================================

/// ext4 magic number
const EXT4_MAGIC: u16 = 0xEF53;

/// Superblock offset (1024 bytes from start)
const SUPERBLOCK_OFFSET: u64 = 1024;

/// Inode types
const EXT4_S_IFIFO: u16 = 0x1000;
const EXT4_S_IFCHR: u16 = 0x2000;
const EXT4_S_IFDIR: u16 = 0x4000;
const EXT4_S_IFBLK: u16 = 0x6000;
const EXT4_S_IFREG: u16 = 0x8000;
const EXT4_S_IFLNK: u16 = 0xA000;
const EXT4_S_IFSOCK: u16 = 0xC000;

/// Feature flags
const EXT4_FEATURE_INCOMPAT_EXTENTS: u32 = 0x0040;
const EXT4_FEATURE_INCOMPAT_64BIT: u32 = 0x0080;

// ============================================================================
// FILE TYPES
// ============================================================================

/// File type enumeration
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

/// Directory entry
#[derive(Clone, Debug)]
pub struct Ext4DirEntry {
    pub name: String,
    pub inode: u32,
    pub file_type: Ext4FileType,
}

/// File metadata
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

/// File system error
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ext4Error {
    InvalidFormat,
    ReadError,
    WriteError,
    NotFound,
    NotSupported,
    OutOfMemory,
    Corrupted,
    JournalError,
}

impl From<super::ext4_journal::JournalError> for Ext4Error {
    fn from(_: super::ext4_journal::JournalError) -> Self {
        Ext4Error::JournalError
    }
}

// ============================================================================
// SUPERBLOCK
// ============================================================================

/// ext4 Superblock (key fields only)
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
    pub s_blocks_count_hi: u32,
    pub s_free_blocks_count_hi: u32,
}

impl Ext4Superblock {
    /// Parse superblock from bytes
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 1024 {
            return None;
        }

        let magic = u16::from_le_bytes([data[56], data[57]]);
        if magic != EXT4_MAGIC {
            return None;
        }

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
            s_blocks_count_hi: u32::from_le_bytes([data[336], data[337], data[338], data[339]]),
            s_free_blocks_count_hi: u32::from_le_bytes([data[340], data[341], data[342], data[343]]),
        })
    }

    /// Get block size
    pub fn block_size(&self) -> u32 {
        1024 << self.s_log_block_size
    }

    /// Get total blocks
    pub fn total_blocks(&self) -> u64 {
        ((self.s_blocks_count_hi as u64) << 32) | (self.s_blocks_count_lo as u64)
    }

    /// Get free blocks
    pub fn free_blocks(&self) -> u64 {
        ((self.s_free_blocks_count_hi as u64) << 32) | (self.s_free_blocks_count_lo as u64)
    }

    /// Get block groups count
    pub fn block_groups_count(&self) -> u32 {
        let total = self.total_blocks();
        ((total + self.s_blocks_per_group as u64 - 1) / self.s_blocks_per_group as u64) as u32
    }

    /// Check if 64-bit mode
    pub fn is_64bit(&self) -> bool {
        (self.s_feature_incompat & EXT4_FEATURE_INCOMPAT_64BIT) != 0
    }

    /// Check if extents are used
    pub fn has_extents(&self) -> bool {
        (self.s_feature_incompat & EXT4_FEATURE_INCOMPAT_EXTENTS) != 0
    }
}

// ============================================================================
// BLOCK GROUP DESCRIPTOR
// ============================================================================

/// Block Group Descriptor
#[derive(Clone, Copy, Debug)]
pub struct Ext4GroupDescriptor {
    pub bg_block_bitmap_lo: u32,
    pub bg_inode_bitmap_lo: u32,
    pub bg_inode_table_lo: u32,
    pub bg_free_blocks_count_lo: u16,
    pub bg_free_inodes_count_lo: u16,
    pub bg_block_bitmap_hi: u32,
    pub bg_inode_bitmap_hi: u32,
    pub bg_inode_table_hi: u32,
}

impl Ext4GroupDescriptor {
    /// Parse from bytes (32-byte format)
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
            bg_block_bitmap_hi: 0,
            bg_inode_bitmap_hi: 0,
            bg_inode_table_hi: 0,
        })
    }

    /// Get block bitmap location
    pub fn block_bitmap(&self, is_64bit: bool) -> u64 {
        if is_64bit {
            ((self.bg_block_bitmap_hi as u64) << 32) | self.bg_block_bitmap_lo as u64
        } else {
            self.bg_block_bitmap_lo as u64
        }
    }

    /// Get inode table location
    pub fn inode_table(&self, is_64bit: bool) -> u64 {
        if is_64bit {
            ((self.bg_inode_table_hi as u64) << 32) | self.bg_inode_table_lo as u64
        } else {
            self.bg_inode_table_lo as u64
        }
    }
}

// ============================================================================
// INODE
// ============================================================================

/// ext4 Inode structure (key fields)
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
}

impl Ext4Inode {
    /// Parse inode from bytes
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 128 {
            return None;
        }

        let mut i_block = [0u8; 60];
        i_block.copy_from_slice(&data[40..100]);

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
        })
    }

    /// Get file size
    pub fn size(&self) -> u64 {
        ((self.i_size_hi as u64) << 32) | (self.i_size_lo as u64)
    }

    /// Get file type
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

    /// Check if directory
    pub fn is_directory(&self) -> bool {
        (self.i_mode & 0xF000) == EXT4_S_IFDIR
    }

    /// Check if uses extents
    pub fn uses_extents(&self) -> bool {
        (self.i_flags & 0x00080000) != 0
    }

    /// Get indirect block pointers
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

    /// Get metadata
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
}

// ============================================================================
// EXTENT TREE
// ============================================================================

/// Extent header
#[derive(Clone, Copy, Debug)]
pub struct Ext4ExtentHeader {
    pub eh_magic: u16,
    pub eh_entries: u16,
    pub eh_depth: u16,
}

impl Ext4ExtentHeader {
    const MAGIC: u16 = 0xF30A;

    /// Parse from bytes
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 12 {
            return None;
        }

        let header = Ext4ExtentHeader {
            eh_magic: u16::from_le_bytes([data[0], data[1]]),
            eh_entries: u16::from_le_bytes([data[2], data[3]]),
            eh_depth: u16::from_le_bytes([data[6], data[7]]),
        };

        if header.eh_magic != Self::MAGIC {
            return None;
        }

        Some(header)
    }

    /// Check if leaf node
    pub fn is_leaf(&self) -> bool {
        self.eh_depth == 0
    }
}

/// Extent entry
#[derive(Clone, Copy, Debug)]
pub struct Ext4Extent {
    pub ee_block: u32,
    pub ee_len: u16,
    pub ee_start: u64,
}

impl Ext4Extent {
    /// Parse from bytes
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 12 {
            return None;
        }

        Some(Ext4Extent {
            ee_block: u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
            ee_len: u16::from_le_bytes([data[4], data[5]]) & 0x7FFF,
            ee_start: u64::from_le_bytes([data[8], data[9], data[10], data[11], 0, 0, 0, 0]),
        })
    }
}

// ============================================================================
// ext4 FILE SYSTEM
// ============================================================================

/// ext4 File System instance
#[derive(Clone, Debug)]
pub struct Ext4FileSystem {
    pub superblock: Ext4Superblock,
    pub block_size: u32,
    pub is_64bit: bool,
    pub group_descriptors: Vec<Ext4GroupDescriptor>,
    pub root_inode: u32,
    /// Optional journal for write support
    pub journal: Option<Arc<Mutex<Journal>>>,
    /// Journal offset in blocks
    pub journal_offset: u64,
}

impl Ext4FileSystem {
    /// Create new ext4 filesystem instance
    pub fn new() -> Self {
        Ext4FileSystem {
            superblock: unsafe { mem::zeroed() },
            block_size: 4096,
            is_64bit: false,
            group_descriptors: Vec::new(),
            root_inode: 2,
            journal: None,
            journal_offset: 0,
        }
    }

    /// Initialize from device data
    pub fn init(&mut self, device_data: &[u8]) -> Result<(), Ext4Error> {
        if device_data.len() < SUPERBLOCK_OFFSET as usize + 1024 {
            return Err(Ext4Error::ReadError);
        }

        let sb_data = &device_data[SUPERBLOCK_OFFSET as usize..];
        let sb = Ext4Superblock::parse(sb_data).ok_or(Ext4Error::InvalidFormat)?;

        self.superblock = sb;
        self.block_size = sb.block_size();
        self.is_64bit = sb.is_64bit();

        // Load group descriptors
        self.load_group_descriptors(device_data)?;

        crate::serial_println!("[ext4] Initialized: {} blocks, {} inodes, {} bytes/block",
            sb.total_blocks(), sb.s_inodes_count, self.block_size);

        Ok(())
    }

    /// Load group descriptors
    fn load_group_descriptors(&mut self, device_data: &[u8]) -> Result<(), Ext4Error> {
        let gd_offset = self.block_size as usize;
        let gds_count = self.superblock.block_groups_count() as usize;

        for i in 0..gds_count {
            let offset = gd_offset + i * 32;
            if offset + 32 > device_data.len() {
                break;
            }

            if let Some(gd) = Ext4GroupDescriptor::parse_32(&device_data[offset..]) {
                self.group_descriptors.push(gd);
            }
        }

        Ok(())
    }

    /// Get inode location
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

    /// Read inode
    pub fn read_inode(&self, inode: u32, device_data: &[u8]) -> Result<Ext4Inode, Ext4Error> {
        let (offset, size) = self.get_inode_location(inode);
        let offset = offset as usize;

        if offset + size as usize > device_data.len() {
            return Err(Ext4Error::ReadError);
        }

        Ext4Inode::parse(&device_data[offset..]).ok_or(Ext4Error::Corrupted)
    }

    /// Map logical block to physical block
    pub fn map_block(&self, inode: &Ext4Inode, logical_block: u32) -> Option<u64> {
        if inode.uses_extents() {
            // Parse extent header from i_block
            let header = Ext4ExtentHeader::parse(&inode.i_block[12..])?;
            
            if !header.is_leaf() {
                return None; // Multi-level extent trees not supported yet
            }

            // Parse extents
            for i in 0..header.eh_entries as usize {
                let offset = 12 + i * 12;
                if offset + 12 > inode.i_block.len() {
                    break;
                }

                if let Some(extent) = Ext4Extent::parse(&inode.i_block[offset..]) {
                    let start = extent.ee_block;
                    let len = extent.ee_len as u32;

                    if logical_block >= start && logical_block < start + len {
                        let offset = logical_block - start;
                        return Some(extent.ee_start + offset as u64);
                    }
                }
            }
        } else {
            // Indirect blocks
            let blocks = inode.indirect_blocks();
            if logical_block < 12 {
                return Some(blocks[logical_block as usize] as u64);
            }
        }

        None
    }

    /// Read file data
    pub fn read_file(&self, inode: &Ext4Inode, device_data: &[u8]) -> Result<Vec<u8>, Ext4Error> {
        let size = inode.size() as usize;
        let mut data = Vec::with_capacity(size);
        let block_size = self.block_size as usize;

        let blocks_needed = (size + block_size - 1) / block_size;

        for i in 0..blocks_needed {
            if let Some(phys_block) = self.map_block(inode, i as u32) {
                let offset = phys_block as usize * block_size;
                let read_size = block_size.min(size - data.len());

                if offset + read_size <= device_data.len() {
                    data.extend_from_slice(&device_data[offset..offset + read_size]);
                }
            }
        }

        data.truncate(size);
        Ok(data)
    }

    /// Read directory entries
    pub fn read_dir(&self, inode: &Ext4Inode, device_data: &[u8]) -> Result<Vec<Ext4DirEntry>, Ext4Error> {
        if !inode.is_directory() {
            return Err(Ext4Error::NotSupported);
        }

        let data = self.read_file(inode, device_data)?;
        let mut entries = Vec::new();
        let mut offset = 0;

        while offset + 8 <= data.len() {
            let inode_num = u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]);
            let rec_len = u16::from_le_bytes([data[offset + 4], data[offset + 5]]) as usize;
            let name_len = data[offset + 6] as usize;
            let file_type = data[offset + 7];

            if inode_num == 0 || rec_len == 0 {
                break;
            }

            if offset + 8 + name_len <= data.len() {
                let name_bytes = &data[offset + 8..offset + 8 + name_len];
                let name = String::from_utf8_lossy(name_bytes).to_string();

                let ext4_type = match file_type {
                    1 => Ext4FileType::Regular,
                    2 => Ext4FileType::Directory,
                    7 => Ext4FileType::Symlink,
                    _ => Ext4FileType::Unknown,
                };

                entries.push(Ext4DirEntry {
                    name,
                    inode: inode_num,
                    file_type: ext4_type,
                });
            }

            offset += rec_len;
        }

        Ok(entries)
    }

    /// Get root inode
    pub fn root_inode_data(&self, device_data: &[u8]) -> Result<Ext4Inode, Ext4Error> {
        self.read_inode(self.root_inode, device_data)
    }

    // ========================================================================
    // WRITE SUPPORT WITH JOURNALING
    // ========================================================================

    /// Initialize journal for write support
    pub fn init_journal(&mut self, device_data: &[u8], journal_offset: u64, journal_size: u64) -> Result<(), Ext4Error> {
        let mut journal = Journal::new(self.block_size, journal_offset, journal_size);
        journal.init(device_data).map_err(|_| Ext4Error::NotSupported)?;
        
        // Recover any uncommitted transactions
        journal.recover(device_data).map_err(|_| Ext4Error::Corrupted)?;
        
        self.journal = Some(Arc::new(Mutex::new(journal)));
        self.journal_offset = journal_offset;
        
        crate::serial_println!("[ext4] Journal initialized at offset {}", journal_offset);
        Ok(())
    }

    /// Start a new transaction for writes
    pub fn begin_transaction(&self, credits: usize) -> Result<(), Ext4Error> {
        if let Some(ref journal) = self.journal {
            let mut j = journal.lock();
            j.start_transaction(credits).map_err(|_| Ext4Error::NotSupported)?;
        }
        Ok(())
    }

    /// Commit current transaction
    pub fn commit_transaction(&self) -> Result<(), Ext4Error> {
        if let Some(ref journal) = self.journal {
            let mut j = journal.lock();
            j.commit_transaction().map_err(|_| Ext4Error::WriteError)?;
        }
        Ok(())
    }

    /// Write data to a file (with journaling if enabled)
    pub fn write_file(&self, inode: &mut Ext4Inode, offset: u64, data: &[u8], device_data: &mut [u8]) -> Result<usize, Ext4Error> {
        let block_size = self.block_size as u64;
        let start_block = offset / block_size;
        let end_block = (offset + data.len() as u64 + block_size - 1) / block_size;
        
        // Add blocks to transaction if journaling
        if let Some(ref journal) = self.journal {
            let mut j = journal.lock();
            for block_num in start_block..end_block {
                if let Some(phys_block) = self.map_block(inode, block_num as u32) {
                    let block_offset = phys_block as usize * block_size as usize;
                    if block_offset + block_size as usize <= device_data.len() {
                        j.add_block(phys_block as u32, &device_data[block_offset..block_offset + block_size as usize], true)?;
                    }
                }
            }
        }

        // Write data to blocks
        let mut bytes_written = 0;
        let mut data_offset = 0;
        
        for block_num in start_block..end_block {
            if let Some(phys_block) = self.map_block(inode, block_num as u32) {
                let block_offset = phys_block as usize * block_size as usize;
                let block_start_in_file = block_num * block_size;
                
                // Calculate write position within block
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
                        device_data[block_offset + write_start..block_offset + write_start + write_count]
                            .copy_from_slice(&data[data_offset..data_offset + write_count]);
                        bytes_written += write_count;
                        data_offset += write_count;
                    }
                }
            }
        }

        // Update inode size if needed
        let new_size = offset + bytes_written as u64;
        if new_size > inode.size() {
            inode.i_size_lo = (new_size & 0xFFFFFFFF) as u32;
            inode.i_size_hi = (new_size >> 32) as u32;
        }

        Ok(bytes_written)
    }

    /// Allocate a new block for a file
    pub fn allocate_block(&self, inode: &mut Ext4Inode, logical_block: u32, device_data: &mut [u8]) -> Result<u64, Ext4Error> {
        // Find a free block from block bitmap
        let group = logical_block / self.superblock.s_blocks_per_group;
        let gd = self.group_descriptors.get(group as usize).ok_or(Ext4Error::OutOfMemory)?;
        
        // For now, use a simple allocation strategy
        // In real implementation, would scan block bitmap
        let new_block = self.superblock.total_blocks() - self.superblock.free_blocks() + logical_block as u64;
        
        // Add to journal if enabled
        if let Some(ref journal) = self.journal {
            let mut j = journal.lock();
            j.add_new_block(new_block as u32, &vec![0u8; self.block_size as usize], true)?;
        }

        // Update inode block pointers
        if inode.uses_extents() {
            // Would need to update extent tree
            // For now, placeholder
        } else {
            let blocks = inode.indirect_blocks();
            if logical_block < 12 {
                // Direct block
                // Would update i_block array
                let _ = blocks;
            }
        }

        Ok(new_block)
    }

    /// Create a new inode
    pub fn create_inode(&self, file_type: Ext4FileType, mode: u16) -> Result<Ext4Inode, Ext4Error> {
        let mut inode: Ext4Inode = unsafe { mem::zeroed() };
        
        inode.i_mode = match file_type {
            Ext4FileType::Regular => EXT4_S_IFREG,
            Ext4FileType::Directory => EXT4_S_IFDIR,
            Ext4FileType::Symlink => EXT4_S_IFLNK,
            _ => 0,
        } | mode;
        
        inode.i_links_count = 1;
        inode.i_flags = if self.superblock.has_extents() { 0x00080000 } else { 0 };
        
        // Get current time (would use system time)
        let time = crate::task::scheduler::get_ticks() as u32;
        inode.i_atime = time;
        inode.i_ctime = time;
        inode.i_mtime = time;
        
        Ok(inode)
    }

    /// Create a directory entry
    pub fn create_dir_entry(&self, parent_inode: &mut Ext4Inode, name: &str, child_inode: u32, file_type: Ext4FileType, device_data: &mut [u8]) -> Result<(), Ext4Error> {
        // Read existing directory data
        let mut dir_data = self.read_file(parent_inode, device_data)?;
        
        // Create new entry
        let ft_code = match file_type {
            Ext4FileType::Regular => 1,
            Ext4FileType::Directory => 2,
            Ext4FileType::Symlink => 7,
            _ => 0,
        };
        
        // Entry: inode(4) + rec_len(2) + name_len(1) + file_type(1) + name
        let name_bytes = name.as_bytes();
        let entry_len = 8 + name_bytes.len();
        let rec_len = (entry_len + 3) & !3; // Align to 4 bytes
        
        let mut entry = vec![0u8; rec_len];
        entry[0..4].copy_from_slice(&child_inode.to_le_bytes());
        entry[4..6].copy_from_slice(&(rec_len as u16).to_le_bytes());
        entry[6] = name_bytes.len() as u8;
        entry[7] = ft_code;
        entry[8..8 + name_bytes.len()].copy_from_slice(name_bytes);
        
        // Append to directory data
        dir_data.extend_from_slice(&entry);
        
        // Write back
        // Would need to write through journal
        
        // Update parent link count if directory
        if file_type == Ext4FileType::Directory {
            parent_inode.i_links_count += 1;
        }
        
        Ok(())
    }

    /// Sync filesystem to disk
    pub fn sync(&self, device_data: &mut [u8]) -> Result<(), Ext4Error> {
        // Commit any pending transaction
        if let Some(ref journal) = self.journal {
            let mut j = journal.lock();
            j.commit_transaction().map_err(|_| Ext4Error::WriteError)?;
        }
        
        // Write superblock
        let sb_offset = SUPERBLOCK_OFFSET as usize;
        // Would serialize and write superblock
        
        crate::serial_println!("[ext4] Filesystem synced");
        Ok(())
    }
}

impl Default for Ext4FileSystem {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// GLOBAL INSTANCE
// ============================================================================

lazy_static::lazy_static! {
    static ref EXT4_INSTANCES: Mutex<BTreeMap<String, Ext4FileSystem>> = Mutex::new(BTreeMap::new());
}

/// Mount ext4 filesystem
pub fn mount_ext4(name: &str, device_data: &[u8]) -> Result<(), Ext4Error> {
    let mut fs = Ext4FileSystem::new();
    fs.init(device_data)?;

    EXT4_INSTANCES.lock().insert(name.to_string(), fs);
    Ok(())
}

/// Get ext4 filesystem
pub fn get_ext4(name: &str) -> Option<Ext4FileSystem> {
    EXT4_INSTANCES.lock().get(name).cloned()
}

/// Unmount ext4 filesystem
pub fn unmount_ext4(name: &str) -> bool {
    EXT4_INSTANCES.lock().remove(name).is_some()
}

/// Initialize ext4 module
pub fn init() {
    crate::serial_println!("[ext4] Module initialized");
}
