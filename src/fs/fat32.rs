//! # echOS FAT32 Dosya Sistemi Sürücüsü
//! 
//! Read-Write destekli temel FAT32 implementasyonu.
//! MBR partition tablosunu parse eder, BPB (BIOS Parameter Block) okur ve
//! dosya/dizin işlemlerini yürütür.

use crate::drivers::ata::AtaDrive;
use alloc::vec::Vec;
use alloc::string::String;
use core::convert::TryInto;

#[repr(C, packed)]
/// MBR Partition Entry yapısı (16 byte).
#[repr(C, packed)]
struct PartitionEntry {
    status: u8,
    start_head: u8,
    start_sector_cyl: u16,
    partition_type: u8,
    end_head: u8,
    end_sector_cyl: u16,
    lba_start: u32,
    sector_count: u32,
}

#[repr(C, packed)]
/// BIOS Parameter Block (BPB) ve FAT32 Extended yapıları.
#[repr(C, packed)]
struct Bpb {
    jmp_boot: [u8; 3],
    oem_name: [u8; 8],
    bytes_per_sector: u16,
    sectors_per_cluster: u8,
    reserved_sectors: u16,
    num_fats: u8,
    root_entry_count: u16,
    total_sectors_16: u16,
    media: u8,
    fat_size_16: u16,
    sectors_per_track: u16,
    num_heads: u16,
    hidden_sectors: u32,
    total_sectors_32: u32,
    // FAT32 specific
    fat_size_32: u32,
    ext_flags: u16,
    fs_version: u16,
    root_cluster: u32,
    fs_info: u16,
    backup_boot_sector: u16,
    reserved: [u8; 12],
    drive_number: u8,
    reserved1: u8,
    boot_signature: u8,
    volume_id: u32,
    volume_label: [u8; 11],
    fs_type: [u8; 8],
}

#[derive(Debug, Clone)]
/// Dosya veya Dizin girdisini temsil eder.
pub struct DirEntry {
    pub name: String,
    pub size: u32,
    pub is_dir: bool,
    pub cluster: u32,
}

/// FAT32 Sürücüsü.
/// ATA sürücüsü üzerinde çalışır.
pub struct Fat32Driver<'a> {
    drive: &'a mut AtaDrive,
    partition_lba: u32,
    sectors_per_cluster: u8,
    bytes_per_sector: u16,
    reserved_sectors: u16,
    num_fats: u8,
    fat_size: u32,
    root_cluster: u32,
    fat_start_lba: u32,
    data_start_lba: u32,
}

impl<'a> Fat32Driver<'a> {
    /// Yeni bir FAT32 sürücüsü oluşturur.
    /// Disk üzerindeki MBR ve BPB'yi okuyarak başlatır.
    pub fn new(drive: &'a mut AtaDrive) -> Option<Self> {
        // 1. Read MBR (LBA 0)
        let mbr_data = drive.read_sectors(0, 1);
        
        // Check signature (0x55AA)
        if mbr_data[510] != 0x55 || mbr_data[511] != 0xAA {
            return None; // Invalid MBR
        }

        // Get first partition entry (offset 446)
        // lba_start is at offset 8 within the entry (446 + 8 = 454)
        let partition_lba = u32::from_le_bytes(mbr_data[454..458].try_into().ok()?);
        
        if partition_lba == 0 {
             // Maybe it's a superfloppy (no MBR), try LBA 0 as BPB
             // But for now, assume MBR exists
        }

        // 2. Read BPB (Partition Start)
        let bpb_data = drive.read_sectors(partition_lba, 1);
        
        // Parse BPB fields manually to avoid unsafe casting issues or alignment
        let bytes_per_sector = u16::from_le_bytes(bpb_data[11..13].try_into().ok()?);
        let sectors_per_cluster = bpb_data[13];
        let reserved_sectors = u16::from_le_bytes(bpb_data[14..16].try_into().ok()?);
        let num_fats = bpb_data[16];
        let fat_size = u32::from_le_bytes(bpb_data[36..40].try_into().ok()?);
        let root_cluster = u32::from_le_bytes(bpb_data[44..48].try_into().ok()?);
        
        let fat_start_lba = partition_lba + reserved_sectors as u32;
        let data_start_lba = fat_start_lba + (num_fats as u32 * fat_size);

        Some(Self {
            drive,
            partition_lba,
            sectors_per_cluster,
            bytes_per_sector,
            reserved_sectors,
            num_fats,
            fat_size,
            root_cluster,
            fat_start_lba,
            data_start_lba,
        })
    }
    
    /// Cluster numarasını LBA adresine çevirir.
    fn cluster_to_lba(&self, cluster: u32) -> u32 {
        self.data_start_lba + ((cluster - 2) * self.sectors_per_cluster as u32)
    }

    /// Root dizinini listeler.
    pub fn list_root(&mut self) -> Vec<DirEntry> {
        self.list_dir(self.root_cluster)
    }
    
    /// List directory entries at a given cluster
    /// Verilen cluster'daki dizin içeriğini listeler.
    pub fn list_dir(&mut self, cluster: u32) -> Vec<DirEntry> {
        let mut entries = Vec::new();
        let mut current_cluster = cluster;
        
        loop {
            // Read cluster
            let data = self.read_cluster(current_cluster);
            
            // Parse directory entries (32 bytes each)
            for chunk in data.chunks(32) {
                if chunk[0] == 0 {
                    return entries; // End of directory
                }
                if chunk[0] == 0xE5 {
                    continue; // Deleted entry
                }
                
                // Attributes at offset 11
                let attr = chunk[11];
                if attr == 0x0F {
                    continue; // Long File Name (LFN) entry - skip for now
                }
                
                // Parse 8.3 name
                let mut name = String::new();
                for i in 0..8 {
                    if chunk[i] != 0x20 {
                        name.push(chunk[i] as char);
                    }
                }
                
                // Extension
                let mut ext = String::new();
                for i in 8..11 {
                    if chunk[i] != 0x20 {
                        ext.push(chunk[i] as char);
                    }
                }
                
                if !ext.is_empty() {
                    name.push('.');
                    name.push_str(&ext);
                }
                
                let cluster_hi = u16::from_le_bytes(chunk[20..22].try_into().unwrap());
                let cluster_lo = u16::from_le_bytes(chunk[26..28].try_into().unwrap());
                let entry_cluster = ((cluster_hi as u32) << 16) | (cluster_lo as u32);
                let size = u32::from_le_bytes(chunk[28..32].try_into().unwrap());
                
                entries.push(DirEntry {
                    name,
                    size,
                    is_dir: (attr & 0x10) != 0,
                    cluster: entry_cluster,
                });
            }
            
            // Follow FAT chain
            match self.get_next_cluster(current_cluster) {
                Some(next) => current_cluster = next,
                None => break,
            }
        }
        
        entries
    }
    
    /// Read a single cluster
    /// Bir cluster okur.
    fn read_cluster(&mut self, cluster: u32) -> Vec<u8> {
        let lba = self.cluster_to_lba(cluster);
        self.drive.read_sectors(lba, self.sectors_per_cluster)
    }
    
    /// Get next cluster from FAT table
    /// FAT tablosundan bir sonraki cluster numarasını alır.
    fn get_next_cluster(&mut self, cluster: u32) -> Option<u32> {
        // Each FAT32 entry is 4 bytes
        let fat_offset = cluster * 4;
        let fat_sector = self.fat_start_lba + (fat_offset / self.bytes_per_sector as u32);
        let entry_offset = (fat_offset % self.bytes_per_sector as u32) as usize;
        
        let fat_data = self.drive.read_sectors(fat_sector, 1);
        let next_cluster = u32::from_le_bytes(
            fat_data[entry_offset..entry_offset + 4].try_into().unwrap()
        ) & 0x0FFFFFFF; // FAT32 uses 28 bits
        
        // Check for end-of-chain markers
        if next_cluster >= 0x0FFFFFF8 {
            None // End of chain
        } else if next_cluster == 0 || next_cluster == 1 {
            None // Reserved/invalid
        } else {
            Some(next_cluster)
        }
    }
    
    /// Read file contents
    /// Dosya içeriğini okur.
    pub fn read_file(&mut self, entry: &DirEntry) -> Vec<u8> {
        if entry.is_dir {
            return Vec::new(); // Can't read directory as file
        }
        
        let mut data = Vec::new();
        let mut remaining = entry.size as usize;
        let mut current_cluster = entry.cluster;
        let cluster_size = self.sectors_per_cluster as usize * self.bytes_per_sector as usize;
        
        loop {
            let cluster_data = self.read_cluster(current_cluster);
            
            // Append only what we need
            let to_copy = core::cmp::min(remaining, cluster_size);
            data.extend_from_slice(&cluster_data[..to_copy]);
            remaining -= to_copy;
            
            if remaining == 0 {
                break;
            }
            
            // Follow FAT chain
            match self.get_next_cluster(current_cluster) {
                Some(next) => current_cluster = next,
                None => break,
            }
        }
        
        data
    }
    
    /// Find entry by path (e.g., "/subdir/file.txt")
    /// Path ile dosya arar (örn: "/subdir/file.txt")
    pub fn find_entry(&mut self, path: &str) -> Option<DirEntry> {
        let path = path.trim_start_matches('/');
        if path.is_empty() {
            return None;
        }
        
        let parts: Vec<&str> = path.split('/').collect();
        let mut current_cluster = self.root_cluster;
        
        for (i, part) in parts.iter().enumerate() {
            let entries = self.list_dir(current_cluster);
            let upper_part = part.to_uppercase();
            
            let found = entries.into_iter().find(|e| e.name.to_uppercase() == upper_part);
            
            match found {
                Some(entry) => {
                    if i == parts.len() - 1 {
                        // Last part - return the entry
                        return Some(entry);
                    } else if entry.is_dir {
                        // Navigate into directory
                        current_cluster = entry.cluster;
                    } else {
                        // Not a directory but not last part
                        return None;
                    }
                }
                None => return None,
            }
        }
        
        None
    }
    
    // ==================== YAZMA DESTEĞİ ====================
    
    /// FAT tablosunda boş bir cluster bulur ve tahsis eder.
    
    /// Find a free cluster in the FAT table
    fn allocate_cluster(&mut self) -> Option<u32> {
        // Start searching from cluster 2 (first data cluster)
        let total_clusters = (self.fat_size * self.bytes_per_sector as u32) / 4;
        
        for cluster in 2..total_clusters {
            if let Some(entry) = self.get_fat_entry(cluster) {
                if entry == 0 {
                    // Found free cluster, mark as end-of-chain
                    self.set_fat_entry(cluster, 0x0FFFFFFF);
                    return Some(cluster);
                }
            }
        }
        None
    }
    
    /// Get FAT entry value for a cluster
    /// FAT tablosundan bir girdiyi okur.
    fn get_fat_entry(&mut self, cluster: u32) -> Option<u32> {
        let fat_offset = cluster * 4;
        let fat_sector = self.fat_start_lba + (fat_offset / self.bytes_per_sector as u32);
        let entry_offset = (fat_offset % self.bytes_per_sector as u32) as usize;
        
        let fat_data = self.drive.read_sectors(fat_sector, 1);
        if entry_offset + 4 > fat_data.len() {
            return None;
        }
        
        Some(u32::from_le_bytes(
            fat_data[entry_offset..entry_offset + 4].try_into().ok()?
        ) & 0x0FFFFFFF)
    }
    
    /// Set FAT entry value for a cluster
    /// FAT tablosuna değer yazar (Cluster Chain güncelleme).
    fn set_fat_entry(&mut self, cluster: u32, value: u32) {
        let fat_offset = cluster * 4;
        let fat_sector = self.fat_start_lba + (fat_offset / self.bytes_per_sector as u32);
        let entry_offset = (fat_offset % self.bytes_per_sector as u32) as usize;
        
        // Read current sector
        let mut fat_data = self.drive.read_sectors(fat_sector, 1);
        
        // Modify entry (preserve high 4 bits)
        let existing = u32::from_le_bytes(
            fat_data[entry_offset..entry_offset + 4].try_into().unwrap()
        );
        let new_value = (existing & 0xF0000000) | (value & 0x0FFFFFFF);
        fat_data[entry_offset..entry_offset + 4].copy_from_slice(&new_value.to_le_bytes());
        
        // Write back to all FATs
        for fat_num in 0..self.num_fats as u32 {
            let fat_lba = fat_sector + (fat_num * self.fat_size);
            let _ = self.drive.write_sectors(fat_lba, &fat_data);
        }
    }
    
    /// Write data to a cluster
    /// Bir cluster'a veri yazar.
    fn write_cluster(&mut self, cluster: u32, data: &[u8]) {
        let lba = self.cluster_to_lba(cluster);
        let cluster_size = self.sectors_per_cluster as usize * self.bytes_per_sector as usize;
        
        // Pad data to cluster size
        let mut buffer = vec![0u8; cluster_size];
        let copy_len = data.len().min(cluster_size);
        buffer[..copy_len].copy_from_slice(&data[..copy_len]);
        
        let _ = self.drive.write_sectors(lba, &buffer);
    }
    
    /// Write file contents (overwrites existing file)
    /// Mevcut bir dosyaya yazar (Üzerine yazar).
    pub fn write_file(&mut self, entry: &DirEntry, data: &[u8]) -> bool {
        if entry.is_dir {
            return false;
        }
        
        let cluster_size = self.sectors_per_cluster as usize * self.bytes_per_sector as usize;
        let mut remaining = data;
        let mut current_cluster = entry.cluster;
        let mut prev_cluster = 0u32;
        
        while !remaining.is_empty() {
            if current_cluster < 2 || current_cluster >= 0x0FFFFFF8 {
                // Need to allocate new cluster
                if let Some(new_cluster) = self.allocate_cluster() {
                    if prev_cluster >= 2 {
                        self.set_fat_entry(prev_cluster, new_cluster);
                    }
                    current_cluster = new_cluster;
                } else {
                    return false; // No space
                }
            }
            
            let chunk_size = remaining.len().min(cluster_size);
            self.write_cluster(current_cluster, &remaining[..chunk_size]);
            remaining = &remaining[chunk_size..];
            
            prev_cluster = current_cluster;
            current_cluster = self.get_next_cluster(current_cluster).unwrap_or(0x0FFFFFFF);
        }
        
        // Mark end of chain
        if prev_cluster >= 2 {
            self.set_fat_entry(prev_cluster, 0x0FFFFFFF);
        }
        
        true
    }
    
    /// Create a new file in the specified directory cluster
    /// Belirtilen dizinde yeni dosya oluşturur.
    pub fn create_file(&mut self, dir_cluster: u32, name: &str, data: &[u8]) -> Option<DirEntry> {
        // Allocate first cluster for file data
        let first_cluster = self.allocate_cluster()?;
        
        // Create directory entry
        let mut dir_entry_bytes = [0u8; 32];
        
        // Format name as 8.3 (uppercase, space-padded)
        let name_upper = name.to_uppercase();
        let (base, ext) = if let Some(dot_pos) = name_upper.rfind('.') {
            (&name_upper[..dot_pos], &name_upper[dot_pos + 1..])
        } else {
            (name_upper.as_str(), "")
        };
        
        // Fill name (8 chars, space-padded)
        for (i, c) in base.chars().take(8).enumerate() {
            dir_entry_bytes[i] = c as u8;
        }
        for i in base.len().min(8)..8 {
            dir_entry_bytes[i] = 0x20; // Space padding
        }
        
        // Fill extension (3 chars, space-padded)
        for (i, c) in ext.chars().take(3).enumerate() {
            dir_entry_bytes[8 + i] = c as u8;
        }
        for i in ext.len().min(3)..3 {
            dir_entry_bytes[8 + i] = 0x20;
        }
        
        // Attributes (0x20 = Archive)
        dir_entry_bytes[11] = 0x20;
        
        // First cluster (high word at 20-21, low word at 26-27)
        dir_entry_bytes[20] = (first_cluster >> 16) as u8;
        dir_entry_bytes[21] = (first_cluster >> 24) as u8;
        dir_entry_bytes[26] = first_cluster as u8;
        dir_entry_bytes[27] = (first_cluster >> 8) as u8;
        
        // File size
        let size = data.len() as u32;
        dir_entry_bytes[28..32].copy_from_slice(&size.to_le_bytes());
        
        // Find empty slot in directory
        let dir_data = self.read_cluster(dir_cluster);
        let mut found_slot = None;
        
        for (i, chunk) in dir_data.chunks(32).enumerate() {
            if chunk[0] == 0 || chunk[0] == 0xE5 {
                found_slot = Some(i * 32);
                break;
            }
        }
        
        if let Some(offset) = found_slot {
            // Write directory entry
            let mut new_dir_data = dir_data;
            new_dir_data[offset..offset + 32].copy_from_slice(&dir_entry_bytes);
            self.write_cluster(dir_cluster, &new_dir_data);
            
            // Write file data
            let cluster_size = self.sectors_per_cluster as usize * self.bytes_per_sector as usize;
            let mut remaining = data;
            let mut current_cluster = first_cluster;
            let mut prev_cluster = first_cluster;
            
            while !remaining.is_empty() {
                let chunk_size = remaining.len().min(cluster_size);
                self.write_cluster(current_cluster, &remaining[..chunk_size]);
                remaining = &remaining[chunk_size..];
                
                if !remaining.is_empty() {
                    prev_cluster = current_cluster;
                    if let Some(next) = self.allocate_cluster() {
                        self.set_fat_entry(current_cluster, next);
                        current_cluster = next;
                    } else {
                        return None; // No space
                    }
                }
            }
            
            // Mark end of chain
            self.set_fat_entry(current_cluster, 0x0FFFFFFF);
            
            Some(DirEntry {
                name: name.to_string(),
                size,
                is_dir: false,
                cluster: first_cluster,
            })
        } else {
            // Free the allocated cluster
            self.set_fat_entry(first_cluster, 0);
            None
        }
    }
    
    /// Delete a file (mark as deleted, free clusters)
    /// Dosya siler.
    pub fn delete_file(&mut self, dir_cluster: u32, name: &str) -> bool {
        let name_upper = name.to_uppercase();
        
        let dir_data = self.read_cluster(dir_cluster);
        
        for (i, chunk) in dir_data.chunks(32).enumerate() {
            if chunk[0] == 0 {
                break;
            }
            if chunk[0] == 0xE5 || chunk[11] == 0x0F {
                continue;
            }
            
            // Parse name
            let mut entry_name = String::new();
            for j in 0..8 {
                if chunk[j] != 0x20 {
                    entry_name.push(chunk[j] as char);
                }
            }
            let mut ext = String::new();
            for j in 8..11 {
                if chunk[j] != 0x20 {
                    ext.push(chunk[j] as char);
                }
            }
            if !ext.is_empty() {
                entry_name.push('.');
                entry_name.push_str(&ext);
            }
            
            if entry_name.to_uppercase() == name_upper {
                // Get cluster chain and free it
                let cluster_hi = u16::from_le_bytes(chunk[20..22].try_into().unwrap());
                let cluster_lo = u16::from_le_bytes(chunk[26..28].try_into().unwrap());
                let mut cluster = ((cluster_hi as u32) << 16) | (cluster_lo as u32);
                
                while cluster >= 2 && cluster < 0x0FFFFFF8 {
                    let next = self.get_fat_entry(cluster).unwrap_or(0x0FFFFFFF);
                    self.set_fat_entry(cluster, 0);
                    cluster = next;
                }
                
                // Mark directory entry as deleted
                let mut new_dir_data = dir_data.clone();
                new_dir_data[i * 32] = 0xE5;
                self.write_cluster(dir_cluster, &new_dir_data);
                
                return true;
            }
        }
        
        false
    }
}

use alloc::vec;
use alloc::string::ToString;
