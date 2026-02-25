//! # echOS FAT32/exFAT File System
//!
//! FAT32 and exFAT file system implementation for USB drives and SD cards.

use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use alloc::format;

// ============================================================================
// FAT32 CONSTANTS
// ============================================================================

const FAT32_BOOT_SECTOR: usize = 0;
const FAT32_SIGNATURE: u16 = 0x28;
const FAT32_CLUSTER_SIZE: u32 = 4096;
const FAT32_ROOT_DIR_CLUSTER: u32 = 2;

// FAT entry values
const FAT32_FREE: u32 = 0x00000000;
const FAT32_RESERVED: u32 = 0x0FFFFFF0;
const FAT32_BAD: u32 = 0x0FFFFFF7;
const FAT32_EOF: u32 = 0x0FFFFFF8;
const FAT32_EOF_MASK: u32 = 0x0FFFFFFF;

// Directory entry attributes
const ATTR_READ_ONLY: u8 = 0x01;
const ATTR_HIDDEN: u8 = 0x02;
const ATTR_SYSTEM: u8 = 0x04;
const ATTR_VOLUME_ID: u8 = 0x08;
const ATTR_DIRECTORY: u8 = 0x10;
const ATTR_ARCHIVE: u8 = 0x20;
const ATTR_LONG_NAME: u8 = 0x0F;

// ============================================================================
// FAT32 BOOT SECTOR
// ============================================================================

#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct Fat32BootSector {
    // Jump instruction and OEM name
    pub jump_boot: [u8; 3],
    pub oem_name: [u8; 8],
    
    // BIOS Parameter Block (BPB)
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub reserved_sector_count: u16,
    pub num_fats: u8,
    pub root_entry_count: u16,
    pub total_sectors_16: u16,
    pub media_type: u8,
    pub sectors_per_fat_16: u16,
    pub sectors_per_track: u16,
    pub num_heads: u16,
    pub hidden_sectors: u32,
    pub total_sectors_32: u32,
    
    // FAT32 extended BPB
    pub sectors_per_fat_32: u32,
    pub ext_flags: u16,
    pub fs_version: u16,
    pub root_cluster: u32,
    pub fs_info_sector: u16,
    pub backup_boot_sector: u16,
    pub reserved: [u8; 12],
    pub drive_number: u8,
    pub reserved1: u8,
    pub boot_signature: u8,
    pub volume_id: u32,
    pub volume_label: [u8; 11],
    pub file_system_type: [u8; 8],
    pub boot_code: [u8; 420],
    pub signature: u16,
}

// ============================================================================
// FAT32 DIRECTORY ENTRY
// ============================================================================

#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct Fat32DirEntry {
    pub name: [u8; 11],
    pub attr: u8,
    pub reserved: u8,
    pub create_time_tenth: u8,
    pub create_time: u16,
    pub create_date: u16,
    pub last_access_date: u16,
    pub cluster_high: u16,
    pub modify_time: u16,
    pub modify_date: u16,
    pub cluster_low: u16,
    pub file_size: u32,
}

impl Fat32DirEntry {
    /// Check if entry is empty
    pub fn is_empty(&self) -> bool {
        self.name[0] == 0x00
    }

    /// Check if entry is deleted
    pub fn is_deleted(&self) -> bool {
        self.name[0] == 0xE5
    }

    /// Check if entry is a directory
    pub fn is_directory(&self) -> bool {
        (self.attr & ATTR_DIRECTORY) != 0
    }

    /// Check if entry is a volume label
    pub fn is_volume_label(&self) -> bool {
        (self.attr & ATTR_VOLUME_ID) != 0
    }

    /// Check if entry is long name
    pub fn is_long_name(&self) -> bool {
        self.attr == ATTR_LONG_NAME
    }

    /// Get cluster number
    pub fn cluster(&self) -> u32 {
        ((self.cluster_high as u32) << 16) | (self.cluster_low as u32)
    }

    /// Get file size
    pub fn file_size(&self) -> u32 {
        self.file_size
    }

    /// Get name as string (8.3 format)
    pub fn name_str(&self) -> String {
        let mut result = String::new();
        let name = &self.name;
        
        // Extract base name (first 8 chars, strip trailing spaces)
        let mut base_end = 8;
        while base_end > 0 && name[base_end - 1] == b' ' {
            base_end -= 1;
        }
        for i in 0..base_end {
            if name[i] != 0 {
                result.push(name[i] as char);
            }
        }
        
        // Extract extension (last 3 chars, strip trailing spaces)
        let mut ext_end = 3;
        while ext_end > 0 && name[8 + ext_end - 1] == b' ' {
            ext_end -= 1;
        }
        if ext_end > 0 {
            result.push('.');
            for i in 0..ext_end {
                if name[8 + i] != 0 {
                    result.push(name[8 + i] as char);
                }
            }
        }
        
        result
    }
}

// ============================================================================
// FAT32 FILE SYSTEM
// ============================================================================

#[derive(Clone, Debug)]
pub struct Fat32Fs {
    pub boot_sector: Fat32BootSector,
    pub fat_start: u32,
    pub fat_size: u32,
    pub data_start: u32,
    pub cluster_size: u32,
    pub total_clusters: u32,
    pub root_cluster: u32,
    pub sector_size: u32,
}

impl Fat32Fs {
    /// Parse FAT32 boot sector
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 512 {
            return None;
        }

        let boot: Fat32BootSector = unsafe { core::ptr::read(data.as_ptr() as *const Fat32BootSector) };
        
        // Validate signature
        if boot.signature != 0xAA55 {
            return None;
        }

        // Check for FAT32
        let bytes_per_sector = boot.bytes_per_sector as u32;
        let sectors_per_cluster = boot.sectors_per_cluster as u32;
        let reserved_sectors = boot.reserved_sector_count as u32;
        let sectors_per_fat = boot.sectors_per_fat_32;
        
        let fat_start = reserved_sectors;
        let data_start = fat_start + (boot.num_fats as u32 * sectors_per_fat);
        let cluster_size = bytes_per_sector * sectors_per_cluster;
        
        // Calculate total clusters
        let total_sectors = if boot.total_sectors_16 != 0 {
            boot.total_sectors_16 as u32
        } else {
            boot.total_sectors_32
        };
        let data_sectors = total_sectors - data_start;
        let total_clusters = data_sectors / sectors_per_cluster;

        Some(Fat32Fs {
            boot_sector: boot,
            fat_start,
            fat_size: sectors_per_fat,
            data_start,
            cluster_size,
            total_clusters,
            root_cluster: boot.root_cluster,
            sector_size: bytes_per_sector,
        })
    }

    /// Read FAT entry
    pub fn read_fat_entry(&self, fat_data: &[u8], cluster: u32) -> u32 {
        let offset = (cluster * 4) as usize;
        if offset + 4 > fat_data.len() {
            return FAT32_EOF;
        }
        u32::from_le_bytes([fat_data[offset], fat_data[offset + 1], fat_data[offset + 2], fat_data[offset + 3]]) & 0x0FFFFFFF
    }

    /// Write FAT entry
    pub fn write_fat_entry(&self, fat_data: &mut [u8], cluster: u32, value: u32) {
        let offset = (cluster * 4) as usize;
        if offset + 4 <= fat_data.len() {
            let val = (value & 0x0FFFFFFF) | 0xF0000000;
            let bytes = val.to_le_bytes();
            fat_data[offset] = bytes[0];
            fat_data[offset + 1] = bytes[1];
            fat_data[offset + 2] = bytes[2];
            fat_data[offset + 3] = bytes[3];
        }
    }

    /// Check if cluster is EOF
    pub fn is_eof(&self, cluster: u32) -> bool {
        cluster >= FAT32_EOF
    }

    /// Check if cluster is free
    pub fn is_free(&self, cluster: u32) -> bool {
        cluster == FAT32_FREE
    }

    /// Convert cluster to sector
    pub fn cluster_to_sector(&self, cluster: u32) -> u32 {
        self.data_start + (cluster - 2) * (self.cluster_size / self.sector_size)
    }

    /// Find free cluster
    pub fn find_free_cluster(&self, fat_data: &[u8]) -> Option<u32> {
        for cluster in 2..self.total_clusters {
            if self.read_fat_entry(fat_data, cluster) == FAT32_FREE {
                return Some(cluster);
            }
        }
        None
    }

    /// Allocate cluster chain
    pub fn allocate_clusters(&self, fat_data: &mut [u8], count: u32) -> Option<u32> {
        let mut first_cluster: Option<u32> = None;
        let mut prev_cluster: u32 = 0;
        let mut allocated = 0;

        for cluster in 2..self.total_clusters {
            if self.read_fat_entry(fat_data, cluster) == FAT32_FREE {
                if first_cluster.is_none() {
                    first_cluster = Some(cluster);
                } else {
                    // Link to previous cluster
                    self.write_fat_entry(fat_data, prev_cluster, cluster);
                }
                prev_cluster = cluster;
                allocated += 1;
                
                if allocated >= count {
                    // Mark end of chain
                    self.write_fat_entry(fat_data, cluster, FAT32_EOF);
                    return first_cluster;
                }
            }
        }

        // Not enough free clusters
        if allocated > 0 {
            // Mark end of partial chain
            self.write_fat_entry(fat_data, prev_cluster, FAT32_EOF);
        }
        first_cluster
    }

    /// Free cluster chain
    pub fn free_clusters(&self, fat_data: &mut [u8], start_cluster: u32) {
        let mut cluster = start_cluster;
        while !self.is_eof(cluster) && !self.is_free(cluster) {
            let next = self.read_fat_entry(fat_data, cluster);
            self.write_fat_entry(fat_data, cluster, FAT32_FREE);
            cluster = next;
        }
    }
}

// ============================================================================
// FAT32 FILE
// ============================================================================

#[derive(Clone, Debug)]
pub struct Fat32File {
    pub name: String,
    pub cluster: u32,
    pub size: u32,
    pub is_dir: bool,
    pub attributes: u8,
}

impl Fat32File {
    pub fn from_entry(entry: &Fat32DirEntry) -> Self {
        Fat32File {
            name: entry.name_str(),
            cluster: entry.cluster(),
            size: entry.file_size(),
            is_dir: entry.is_directory(),
            attributes: entry.attr,
        }
    }
}

// ============================================================================
// exFAT CONSTANTS
// ============================================================================

const EXFAT_SIGNATURE: u32 = 0xAA550000;

// ============================================================================
// exFAT BOOT SECTOR
// ============================================================================

#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct ExFatBootSector {
    pub jump_boot: [u8; 3],
    pub file_system_name: [u8; 8],
    pub partition_offset: u64,
    pub volume_length: u64,
    pub fat_offset: u32,
    pub fat_length: u32,
    pub cluster_heap_offset: u32,
    pub cluster_count: u32,
    pub first_cluster_of_root_dir: u32,
    pub volume_serial_number: u32,
    pub file_system_revision: u16,
    pub volume_flags: u16,
    pub bytes_per_sector_shift: u8,
    pub sectors_per_cluster_shift: u8,
    pub number_of_fats: u8,
    pub drive_select: u8,
    pub percent_in_use: u8,
    pub reserved: [u8; 7],
    pub boot_code: [u8; 390],
    pub signature: u16,
}

// ============================================================================
// exFAT FILE ATTRIBUTE
// ============================================================================

#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct ExFatFileAttribute {
    pub entry_type: u8,
    pub entry_count: u8,
    pub checksum: u16,
    pub attributes: u16,
    pub reserved1: [u8; 2],
    pub create_timestamp: u32,
    pub last_modified_timestamp: u32,
    pub last_accessed_timestamp: u32,
    pub create_10ms_increment: u8,
    pub last_modified_10ms_increment: u8,
    pub create_timezone: u8,
    pub last_modified_timezone: u8,
    pub last_accessed_timezone: u8,
}

// ============================================================================
// exFAT STREAM EXTENSION
// ============================================================================

#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct ExFatStreamExtension {
    pub entry_type: u8,
    pub general_secondary_flags: u8,
    pub reserved1: u8,
    pub name_length: u8,
    pub name_hash: u16,
    pub reserved2: [u8; 2],
    pub valid_data_length: u64,
    pub reserved3: [u8; 4],
    pub first_cluster: u32,
    pub data_length: u64,
}

// ============================================================================
// exFAT FILE NAME
// ============================================================================

#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct ExFatFileName {
    pub entry_type: u8,
    pub general_secondary_flags: u8,
    pub name: [u16; 15],
}

// ============================================================================
// exFAT FILE SYSTEM
// ============================================================================

#[derive(Clone, Debug)]
pub struct ExFatFs {
    pub boot_sector: ExFatBootSector,
    pub sector_size: u32,
    pub cluster_size: u32,
    pub fat_offset: u32,
    pub fat_length: u32,
    pub cluster_heap_offset: u32,
    pub cluster_count: u32,
    pub root_cluster: u32,
}

impl ExFatFs {
    /// Parse exFAT boot sector
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 512 {
            return None;
        }

        let boot: ExFatBootSector = unsafe { core::ptr::read(data.as_ptr() as *const ExFatBootSector) };
        
        // Validate signature
        if boot.signature != 0xAA55 {
            return None;
        }

        let sector_size = 1u32 << boot.bytes_per_sector_shift;
        let cluster_size = sector_size << boot.sectors_per_cluster_shift;

        Some(ExFatFs {
            boot_sector: boot,
            sector_size,
            cluster_size,
            fat_offset: boot.fat_offset,
            fat_length: boot.fat_length,
            cluster_heap_offset: boot.cluster_heap_offset,
            cluster_count: boot.cluster_count,
            root_cluster: boot.first_cluster_of_root_dir,
        })
    }

    /// Read FAT entry
    pub fn read_fat_entry(&self, fat_data: &[u8], cluster: u32) -> u32 {
        let offset = (cluster * 4) as usize;
        if offset + 4 > fat_data.len() {
            return 0xFFFFFFFF;
        }
        u32::from_le_bytes([fat_data[offset], fat_data[offset + 1], fat_data[offset + 2], fat_data[offset + 3]])
    }

    /// Write FAT entry
    pub fn write_fat_entry(&self, fat_data: &mut [u8], cluster: u32, value: u32) {
        let offset = (cluster * 4) as usize;
        if offset + 4 <= fat_data.len() {
            let bytes = value.to_le_bytes();
            fat_data[offset] = bytes[0];
            fat_data[offset + 1] = bytes[1];
            fat_data[offset + 2] = bytes[2];
            fat_data[offset + 3] = bytes[3];
        }
    }

    /// Check if cluster is EOF
    pub fn is_eof(&self, cluster: u32) -> bool {
        cluster >= 0xFFFFFFF8
    }

    /// Check if cluster is free
    pub fn is_free(&self, cluster: u32) -> bool {
        cluster == 0
    }

    /// Convert cluster to sector
    pub fn cluster_to_sector(&self, cluster: u32) -> u32 {
        self.cluster_heap_offset + (cluster - 2) * (self.cluster_size / self.sector_size)
    }
}

// ============================================================================
// FILE SYSTEM ERROR
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FatError {
    InvalidBootSector,
    InvalidCluster,
    FileNotFound,
    NotAFile,
    NotADirectory,
    DiskError,
    OutOfSpace,
    InvalidName,
    DirectoryNotEmpty,
}

// ============================================================================
// FAT MANAGER
// ============================================================================

use spin::Mutex;

static FAT32_INSTANCES: Mutex<Vec<Fat32Fs>> = Mutex::new(Vec::new());
static EXFAT_INSTANCES: Mutex<Vec<ExFatFs>> = Mutex::new(Vec::new());

/// Detect file system type from boot sector
pub fn detect_filesystem(data: &[u8]) -> Option<FilesystemType> {
    if data.len() < 512 {
        return None;
    }

    // Check for exFAT first (has specific signature pattern)
    if data[3..11] == *b"EXFAT   " {
        return Some(FilesystemType::ExFat);
    }

    // Check for FAT32
    if data[510] == 0x55 && data[511] == 0xAA {
        // Check file system type string
        if &data[54..62] == b"FAT32   " || &data[82..90] == b"FAT32   " {
            return Some(FilesystemType::Fat32);
        }
    }

    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilesystemType {
    Fat32,
    ExFat,
}

/// Initialize FAT file system
pub fn init() {
    crate::serial_println!("[FAT] File system module initialized");
}

/// Mount FAT32 file system
pub fn mount_fat32(data: &[u8]) -> Option<usize> {
    let fs = Fat32Fs::parse(data)?;
    let mut instances = FAT32_INSTANCES.lock();
    instances.push(fs);
    Some(instances.len() - 1)
}

/// Mount exFAT file system
pub fn mount_exfat(data: &[u8]) -> Option<usize> {
    let fs = ExFatFs::parse(data)?;
    let mut instances = EXFAT_INSTANCES.lock();
    instances.push(fs);
    Some(instances.len() - 1)
}

/// Get FAT32 instance
pub fn get_fat32(index: usize) -> Option<Fat32Fs> {
    FAT32_INSTANCES.lock().get(index).cloned()
}

/// Get exFAT instance
pub fn get_exfat(index: usize) -> Option<ExFatFs> {
    EXFAT_INSTANCES.lock().get(index).cloned()
}
