//! # echOS FAT32/exFAT Dosya Sistemi
//!
//! USB sürücüler ve SD kartlar için FAT32 ve exFAT dosya sistemi uygulaması.
//!
//! ## FAT32 Disk Yapısı (ASCII Diyagram)
//! ```text
//! FAT32 Bölüm Düzeni:
//! ┌────────────────────────────────────────────────────────────┐
//! │  Sektör 0      │  Önyükleme Sektörü (BPB + imza 0xAA55)  │
//! ├────────────────────────────────────────────────────────────┤
//! │  FSInfo         │  Serbest küme sayısı ve sonraki boş       │
//! ├────────────────────────────────────────────────────────────┤
//! │  Ayrılmış       │  reserved_sector_count kadar sektör       │
//! ├────────────────────────────────────────────────────────────┤
//! │  FAT 1          │  Dosya Tahsis Tablosu (küme zinciri)      │
//! ├────────────────────────────────────────────────────────────┤
//! │  FAT 2          │  FAT yedek kopyası                        │
//! ├────────────────────────────────────────────────────────────┤
//! │  Veri Bölgesi   │  Kümeler: Dizinler ve dosya verileri      │
//! │  (Küme 2+)      │  Root dizin: root_cluster (genellikle 2)  │
//! └────────────────────────────────────────────────────────────┘
//!
//! Küme Zinciri: FAT tablosunda her küme, bir sonraki kümeye işaret eder.
//!   0x0FFFFFF8+ = Zincir sonu (EOF)
//!   0x00000000  = Serbest küme
//!   0x0FFFFFF7  = Bozuk küme
//! ```

use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

// ============================================================================
// FAT32 SABİTLERİ
// ============================================================================

const FAT32_BOOT_SECTOR: usize = 0;
const FAT32_SIGNATURE: u16 = 0x28;
const FAT32_CLUSTER_SIZE: u32 = 4096;
const FAT32_ROOT_DIR_CLUSTER: u32 = 2;

// FAT girişi özel değerleri
const FAT32_FREE: u32 = 0x00000000;
const FAT32_RESERVED: u32 = 0x0FFFFFF0;
const FAT32_BAD: u32 = 0x0FFFFFF7;
const FAT32_EOF: u32 = 0x0FFFFFF8;
const FAT32_EOF_MASK: u32 = 0x0FFFFFFF;

// Dizin girdisi öznitelikleri
const ATTR_READ_ONLY: u8 = 0x01;
const ATTR_HIDDEN: u8 = 0x02;
const ATTR_SYSTEM: u8 = 0x04;
const ATTR_VOLUME_ID: u8 = 0x08;
const ATTR_DIRECTORY: u8 = 0x10;
const ATTR_ARCHIVE: u8 = 0x20;
const ATTR_LONG_NAME: u8 = 0x0F;

// ============================================================================
// FAT32 ÖNYÜKLEME SEKTÖRÜ
// ============================================================================

/// FAT32 Önyükleme Sektörü - BIOS Parametre Bloğu (BPB) ve genişletilmiş BPB içerir
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct Fat32BootSector {
    // Atlama komutu ve OEM adı
    pub jump_boot: [u8; 3],
    pub oem_name: [u8; 8],

    // BIOS Parametre Bloğu (BPB)
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

    // FAT32 genişletilmiş BPB
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
// FAT32 DİZİN GİRDİSİ
// ============================================================================

/// FAT32 Dizin Girdisi - 8.3 dosya adı, öznitelik, zaman ve küme bilgisi tutar
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
    /// Girdinin boş olup olmadığını kontrol eder (ilk bayt 0x00 ise boş)
    pub fn is_empty(&self) -> bool {
        self.name[0] == 0x00
    }

    /// Girdinin silinmiş olup olmadığını kontrol eder (ilk bayt 0xE5 ise silinmiş)
    pub fn is_deleted(&self) -> bool {
        self.name[0] == 0xE5
    }

    /// Girdinin bir dizin olup olmadığını kontrol eder
    pub fn is_directory(&self) -> bool {
        (self.attr & ATTR_DIRECTORY) != 0
    }

    /// Girdinin bir birim etiketi olup olmadığını kontrol eder
    pub fn is_volume_label(&self) -> bool {
        (self.attr & ATTR_VOLUME_ID) != 0
    }

    /// Girdinin uzun dosya adı (LFN) girdisi olup olmadığını kontrol eder
    pub fn is_long_name(&self) -> bool {
        self.attr == ATTR_LONG_NAME
    }

    /// Girdi için küme numarasını döndürür (yüksek ve düşük 16 bit birleşimi)
    pub fn cluster(&self) -> u32 {
        ((self.cluster_high as u32) << 16) | (self.cluster_low as u32)
    }

    /// Dosya boyutunu döndürür
    pub fn file_size(&self) -> u32 {
        self.file_size
    }

    /// Dosya adını 8.3 formatında string olarak döndürür
    pub fn name_str(&self) -> String {
        let mut result = String::new();
        let name = &self.name;

        // Ana adı al (ilk 8 karakter, sondaki boşlukları çıkar)
        let mut base_end = 8;
        while base_end > 0 && name[base_end - 1] == b' ' {
            base_end -= 1;
        }
        for i in 0..base_end {
            if name[i] != 0 {
                result.push(name[i] as char);
            }
        }

        // Uzantıyı al (son 3 karakter, sondaki boşlukları çıkar)
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
// FAT32 DOSYA SİSTEMİ
// ============================================================================

/// FAT32 Dosya Sistemi örneği - bölüm parametrelerini ve hesaplanan ofsesleri tutar
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
    /// FAT32 önyükleme sektörünü çözümler ve dosya sistemi parametrelerini hesaplar
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 512 {
            return None;
        }

        let boot: Fat32BootSector =
            unsafe { core::ptr::read(data.as_ptr() as *const Fat32BootSector) };

        // İmzayı doğrula
        if boot.signature != 0xAA55 {
            return None;
        }

        // FAT32 parametrelerini hesapla
        let bytes_per_sector = boot.bytes_per_sector as u32;
        let sectors_per_cluster = boot.sectors_per_cluster as u32;
        let reserved_sectors = boot.reserved_sector_count as u32;
        let sectors_per_fat = boot.sectors_per_fat_32;

        let fat_start = reserved_sectors;
        let data_start = fat_start + (boot.num_fats as u32 * sectors_per_fat);
        let cluster_size = bytes_per_sector * sectors_per_cluster;

        // Toplam küme sayısını hesapla
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

    /// FAT tablosundaki belirtilen kümenin değerini okur
    pub fn read_fat_entry(&self, fat_data: &[u8], cluster: u32) -> u32 {
        let offset = (cluster * 4) as usize;
        if offset + 4 > fat_data.len() {
            return FAT32_EOF;
        }
        u32::from_le_bytes([
            fat_data[offset],
            fat_data[offset + 1],
            fat_data[offset + 2],
            fat_data[offset + 3],
        ]) & 0x0FFFFFFF
    }

    /// FAT tablosundaki belirtilen kümenin değerini yazar
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

    /// Kümenin zincir sonu olup olmadığını kontrol eder (0x0FFFFFF8 ve üzeri)
    pub fn is_eof(&self, cluster: u32) -> bool {
        cluster >= FAT32_EOF
    }

    /// Kümenin serbest olup olmadığını kontrol eder (değer 0)
    pub fn is_free(&self, cluster: u32) -> bool {
        cluster == FAT32_FREE
    }

    /// Küme numarasını sektör numarasına dönüştürür
    pub fn cluster_to_sector(&self, cluster: u32) -> u32 {
        self.data_start + (cluster - 2) * (self.cluster_size / self.sector_size)
    }

    /// FAT tablosunda ilk serbest kümeyi bulur
    pub fn find_free_cluster(&self, fat_data: &[u8]) -> Option<u32> {
        for cluster in 2..self.total_clusters {
            if self.read_fat_entry(fat_data, cluster) == FAT32_FREE {
                return Some(cluster);
            }
        }
        None
    }

    /// Belirtilen sayıda kümeyi zincirleme olarak tahsis eder
    pub fn allocate_clusters(&self, fat_data: &mut [u8], count: u32) -> Option<u32> {
        let mut first_cluster: Option<u32> = None;
        let mut prev_cluster: u32 = 0;
        let mut allocated = 0;

        for cluster in 2..self.total_clusters {
            if self.read_fat_entry(fat_data, cluster) == FAT32_FREE {
                if first_cluster.is_none() {
                    first_cluster = Some(cluster);
                } else {
                    // Önceki kümeye bağla
                    self.write_fat_entry(fat_data, prev_cluster, cluster);
                }
                prev_cluster = cluster;
                allocated += 1;

                if allocated >= count {
                    // Zincir sonunu işaretle
                    self.write_fat_entry(fat_data, cluster, FAT32_EOF);
                    return first_cluster;
                }
            }
        }

        // Yeterli serbest küme yok
        if allocated > 0 {
            // Kısmi zincirin sonunu işaretle
            self.write_fat_entry(fat_data, prev_cluster, FAT32_EOF);
        }
        first_cluster
    }

    /// Başlangıç kümesinden itibaren tüm küme zincirini serbest bırakır
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
// FAT32 DOSYASI
// ============================================================================

/// FAT32 dosya/dizin bilgisi - dizin girdisinden oluşturulan yapı
#[derive(Clone, Debug)]
pub struct Fat32File {
    pub name: String,
    pub cluster: u32,
    pub size: u32,
    pub is_dir: bool,
    pub attributes: u8,
}

impl Fat32File {
    /// Dizin girdisinden dosya bilgisi oluşturur
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
// exFAT SABİTLERİ
// ============================================================================

const EXFAT_SIGNATURE: u32 = 0xAA550000;

// ============================================================================
// exFAT ÖNYÜKLEME SEKTÖRÜ
// ============================================================================

/// exFAT Önyükleme Sektörü - büyük birimler için geliştirilmiş FAT yapısı
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
// exFAT DOSYA ÖZNİTELİĞİ
// ============================================================================

/// exFAT Dosya Öznitelik Girdisi - oluşturma/değiştirme zamanları ve özellikler
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
// exFAT AKIŞ UZANTISI
// ============================================================================

/// exFAT Akış Uzantısı Girdisi - veri uzunluğu ve küme bilgisi tutar
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
// exFAT DOSYA ADI
// ============================================================================

/// exFAT Dosya Adı Girdisi - UTF-16LE kodlamalı dosya adını tutar
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct ExFatFileName {
    pub entry_type: u8,
    pub general_secondary_flags: u8,
    pub name: [u16; 15],
}

// ============================================================================
// exFAT DOSYA SİSTEMİ
// ============================================================================

/// exFAT Dosya Sistemi örneği - parametreler ve hesaplanan değerler
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
    /// exFAT önyükleme sektörünü çözümler ve dosya sistemi parametrelerini hesaplar
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 512 {
            return None;
        }

        if data[3..11] != *b"EXFAT   " {
            return None;
        }
        let signature = u16::from_le_bytes([data[510], data[511]]);
        if signature != 0xAA55 {
            return None;
        }

        let bytes_per_sector_shift = data[108];
        let sectors_per_cluster_shift = data[109];
        if bytes_per_sector_shift > 20 || sectors_per_cluster_shift > 25 {
            return None;
        }
        let sector_size = 1u32.checked_shl(bytes_per_sector_shift as u32)?;
        let sectors_per_cluster = 1u32.checked_shl(sectors_per_cluster_shift as u32)?;
        let cluster_size = sector_size.checked_mul(sectors_per_cluster)?;
        if sector_size < 512 || cluster_size == 0 {
            return None;
        }

        let fat_offset = u32::from_le_bytes([data[80], data[81], data[82], data[83]]);
        let fat_length = u32::from_le_bytes([data[84], data[85], data[86], data[87]]);
        let cluster_heap_offset = u32::from_le_bytes([data[88], data[89], data[90], data[91]]);
        let cluster_count = u32::from_le_bytes([data[92], data[93], data[94], data[95]]);
        let root_cluster = u32::from_le_bytes([data[96], data[97], data[98], data[99]]);
        if fat_length == 0 || cluster_count == 0 || root_cluster < 2 {
            return None;
        }

        let mut jump_boot = [0u8; 3];
        jump_boot.copy_from_slice(&data[0..3]);
        let mut file_system_name = [0u8; 8];
        file_system_name.copy_from_slice(&data[3..11]);
        let mut boot_code = [0u8; 390];
        boot_code.copy_from_slice(&data[120..510]);

        let boot = ExFatBootSector {
            jump_boot,
            file_system_name,
            partition_offset: u64::from_le_bytes([
                data[64], data[65], data[66], data[67], data[68], data[69], data[70], data[71],
            ]),
            volume_length: u64::from_le_bytes([
                data[72], data[73], data[74], data[75], data[76], data[77], data[78], data[79],
            ]),
            fat_offset,
            fat_length,
            cluster_heap_offset,
            cluster_count,
            first_cluster_of_root_dir: root_cluster,
            volume_serial_number: u32::from_le_bytes([data[100], data[101], data[102], data[103]]),
            file_system_revision: u16::from_le_bytes([data[104], data[105]]),
            volume_flags: u16::from_le_bytes([data[106], data[107]]),
            bytes_per_sector_shift,
            sectors_per_cluster_shift,
            number_of_fats: data[110],
            drive_select: data[111],
            percent_in_use: data[112],
            reserved: [
                data[113], data[114], data[115], data[116], data[117], data[118], data[119],
            ],
            boot_code,
            signature,
        };

        Some(ExFatFs {
            boot_sector: boot,
            sector_size,
            cluster_size,
            fat_offset,
            fat_length,
            cluster_heap_offset,
            cluster_count,
            root_cluster,
        })
    }

    /// FAT tablosundaki belirtilen kümenin değerini okur
    pub fn read_fat_entry(&self, fat_data: &[u8], cluster: u32) -> u32 {
        let offset = (cluster * 4) as usize;
        if offset + 4 > fat_data.len() {
            return 0xFFFFFFFF;
        }
        u32::from_le_bytes([
            fat_data[offset],
            fat_data[offset + 1],
            fat_data[offset + 2],
            fat_data[offset + 3],
        ])
    }

    /// FAT tablosundaki belirtilen kümenin değerini yazar
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

    /// Kümenin zincir sonu olup olmadığını kontrol eder (0xFFFFFFF8 ve üzeri)
    pub fn is_eof(&self, cluster: u32) -> bool {
        cluster >= 0xFFFFFFF8
    }

    /// Kümenin serbest olup olmadığını kontrol eder (değer 0)
    pub fn is_free(&self, cluster: u32) -> bool {
        cluster == 0
    }

    /// Küme numarasını sektör numarasına dönüştürür
    pub fn cluster_to_sector(&self, cluster: u32) -> u32 {
        self.cluster_heap_offset + (cluster - 2) * (self.cluster_size / self.sector_size)
    }
}

// ============================================================================
// DOSYA SİSTEMİ HATASI
// ============================================================================

/// FAT/exFAT işlemlerinde oluşabilecek hata türleri
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
// FAT YÖNETİCİSİ
// ============================================================================

use spin::Mutex;

#[derive(Clone, Debug)]
pub struct MountedFat32 {
    pub fs: Fat32Fs,
    pub storage: Fat32Storage,
}

#[derive(Clone, Debug)]
pub struct MountedExFat {
    pub fs: ExFatFs,
    pub storage: Fat32Storage,
}

#[derive(Clone, Debug)]
pub enum Fat32Storage {
    Resident(Arc<Vec<u8>>),
    LoopbackDevice(String),
}

impl Fat32Storage {
    pub fn image_len(&self) -> Result<usize, &'static str> {
        match self {
            Self::Resident(image) => Ok(image.len()),
            Self::LoopbackDevice(name) => {
                let device = crate::drivers::loopback::open(name.as_str())
                    .ok_or("fat32: loopback device not found")?;
                Ok(device.descriptor().block_count as usize
                    * device.descriptor().block_size as usize)
            }
        }
    }

    pub fn read_exact(&self, offset: usize, len: usize) -> Result<Vec<u8>, &'static str> {
        match self {
            Self::Resident(image) => {
                if offset.checked_add(len).ok_or("fat32: offset overflow")? > image.len() {
                    return Err("fat32: read exceeds mounted image");
                }
                Ok(image[offset..offset + len].to_vec())
            }
            Self::LoopbackDevice(name) => {
                let mut device = crate::drivers::loopback::open(name.as_str())
                    .ok_or("fat32: loopback device not found")?;
                let descriptor = device.descriptor();
                let block_size = descriptor.block_size as usize;
                let total_len = descriptor.block_count as usize * block_size;
                if offset.checked_add(len).ok_or("fat32: offset overflow")? > total_len {
                    return Err("fat32: read exceeds mounted image");
                }
                let start_block = offset / block_size;
                let end_block = (offset + len + block_size - 1) / block_size;
                let mut blocks = Vec::with_capacity((end_block - start_block) * block_size);
                for lba in start_block..end_block {
                    let mut block = vec![0u8; block_size];
                    crate::drivers::block::BlockDevice::read_block(
                        &mut device,
                        lba as u64,
                        &mut block,
                    )
                    .map_err(|_| "fat32: loopback block read failed")?;
                    blocks.extend_from_slice(block.as_slice());
                }
                let inner_offset = offset % block_size;
                Ok(blocks[inner_offset..inner_offset + len].to_vec())
            }
        }
    }
}

static FAT32_INSTANCES: Mutex<Vec<Option<MountedFat32>>> = Mutex::new(Vec::new());
static EXFAT_INSTANCES: Mutex<Vec<Option<MountedExFat>>> = Mutex::new(Vec::new());

/// Önyükleme sektöründen dosya sistemi türünü tespit eder
pub fn detect_filesystem(data: &[u8]) -> Option<FilesystemType> {
    if data.len() < 512 {
        return None;
    }

    // Önce exFAT'ı kontrol et (özel imza dizisi: "EXFAT   ")
    if data[3..11] == *b"EXFAT   " {
        return Some(FilesystemType::ExFat);
    }

    // FAT32'yi kontrol et
    if data[510] == 0x55 && data[511] == 0xAA {
        // Dosya sistemi türü dizesini kontrol et
        if &data[54..62] == b"FAT32   " || &data[82..90] == b"FAT32   " {
            return Some(FilesystemType::Fat32);
        }
    }

    None
}

/// Desteklenen FAT tabanlı dosya sistemi türleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilesystemType {
    Fat32,
    ExFat,
}

/// FAT dosya sistemi modülünü başlatır
pub fn init() {
    crate::serial_println!("[FAT] Dosya sistemi modülü başlatıldı");
}

/// FAT32 dosya sistemini bağlar ve örnekler listesine ekler
pub fn mount_fat32(data: &[u8]) -> Option<usize> {
    let fs = Fat32Fs::parse(data)?;
    let mut instances = FAT32_INSTANCES.lock();
    let mounted = MountedFat32 {
        fs,
        storage: Fat32Storage::Resident(Arc::new(data.to_vec())),
    };
    if let Some(index) = instances.iter().position(|entry| entry.is_none()) {
        instances[index] = Some(mounted);
        Some(index)
    } else {
        instances.push(Some(mounted));
        Some(instances.len() - 1)
    }
}

/// Loopback block device üstünden FAT32 backend bağlar.
pub fn mount_fat32_loopback(device_name: &str) -> Option<usize> {
    let mut device = crate::drivers::loopback::open(device_name)?;
    let descriptor = device.descriptor();
    let sector_size = descriptor.block_size as usize;
    if sector_size < 512 {
        return None;
    }
    let mut sector0 = vec![0u8; sector_size];
    crate::drivers::block::BlockDevice::read_block(&mut device, 0, &mut sector0).ok()?;
    let fs = Fat32Fs::parse(&sector0)?;
    let mut instances = FAT32_INSTANCES.lock();
    let mounted = MountedFat32 {
        fs,
        storage: Fat32Storage::LoopbackDevice(device_name.to_string()),
    };
    if let Some(index) = instances.iter().position(|entry| entry.is_none()) {
        instances[index] = Some(mounted);
        Some(index)
    } else {
        instances.push(Some(mounted));
        Some(instances.len() - 1)
    }
}

/// exFAT dosya sistemini bağlar ve örnekler listesine ekler
pub fn mount_exfat(data: &[u8]) -> Option<usize> {
    let fs = ExFatFs::parse(data)?;
    let mut instances = EXFAT_INSTANCES.lock();
    let mounted = MountedExFat {
        fs,
        storage: Fat32Storage::Resident(Arc::new(data.to_vec())),
    };
    if let Some(index) = instances.iter().position(|entry| entry.is_none()) {
        instances[index] = Some(mounted);
        Some(index)
    } else {
        instances.push(Some(mounted));
        Some(instances.len() - 1)
    }
}

/// Loopback block device üstünden exFAT backend bağlar.
pub fn mount_exfat_loopback(device_name: &str) -> Option<usize> {
    let mut device = crate::drivers::loopback::open(device_name)?;
    let descriptor = device.descriptor();
    let sector_size = descriptor.block_size as usize;
    if sector_size < 512 {
        return None;
    }
    let mut sector0 = vec![0u8; sector_size];
    crate::drivers::block::BlockDevice::read_block(&mut device, 0, &mut sector0).ok()?;
    let fs = ExFatFs::parse(&sector0)?;
    let mut instances = EXFAT_INSTANCES.lock();
    let mounted = MountedExFat {
        fs,
        storage: Fat32Storage::LoopbackDevice(device_name.to_string()),
    };
    if let Some(index) = instances.iter().position(|entry| entry.is_none()) {
        instances[index] = Some(mounted);
        Some(index)
    } else {
        instances.push(Some(mounted));
        Some(instances.len() - 1)
    }
}

/// İndekse göre FAT32 örneğini döndürür
pub fn get_fat32(index: usize) -> Option<Fat32Fs> {
    FAT32_INSTANCES
        .lock()
        .get(index)
        .and_then(|mounted| mounted.as_ref().map(|mounted| mounted.fs.clone()))
}

/// Indekse gore imaj destekli FAT32 ornegini dondurur
pub fn get_mounted_fat32(index: usize) -> Option<MountedFat32> {
    FAT32_INSTANCES
        .lock()
        .get(index)
        .and_then(|entry| entry.clone())
}

/// İndekse göre exFAT örneğini döndürür
pub fn get_exfat(index: usize) -> Option<ExFatFs> {
    EXFAT_INSTANCES
        .lock()
        .get(index)
        .and_then(|mounted| mounted.as_ref().map(|mounted| mounted.fs.clone()))
}

pub fn get_mounted_exfat(index: usize) -> Option<MountedExFat> {
    EXFAT_INSTANCES
        .lock()
        .get(index)
        .and_then(|entry| entry.clone())
}

pub fn unmount_fat32(index: usize) -> bool {
    let mut instances = FAT32_INSTANCES.lock();
    let Some(entry) = instances.get_mut(index) else {
        return false;
    };
    entry.take().is_some()
}

pub fn unmount_exfat(index: usize) -> bool {
    let mut instances = EXFAT_INSTANCES.lock();
    let Some(entry) = instances.get_mut(index) else {
        return false;
    };
    entry.take().is_some()
}
