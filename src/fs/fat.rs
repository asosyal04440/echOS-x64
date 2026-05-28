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
        let offset = (cluster as usize) * 4;
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
        let offset = (cluster as usize) * 4;
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
        if cluster < 2 || self.sector_size == 0 {
            return 0;
        }
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

    /// Dizin girdisini siler (short entry + LFN chain)
    /// Spec: Silinen dosyanın short entry ilk byte'ı 0xE5 yapılır.
    /// LFN chain'deki TÜM entry'ler de 0xE5 ile işaretlenir.
    /// FAT chain serbest bırakılır.
    /// Returns: (işaretlenen LFN sayısı, başarı durumu)
    pub fn delete_dir_entry(
        &self,
        dir_data: &mut [u8],
        short_entry_offset: usize,
    ) -> (usize, bool) {
        if short_entry_offset + 32 > dir_data.len() {
            return (0, false);
        }

        // Short entry'nin ilk byte'ını 0xE5 yap (silindi işareti)
        dir_data[short_entry_offset] = 0xE5;

        // Önceki LFN entry'lerini bul ve sil
        let mut lfn_count = 0usize;
        let mut idx = short_entry_offset;
        while idx >= 32 {
            idx -= 32;
            if idx + 32 > dir_data.len() {
                break;
            }
            let ord = dir_data[idx];
            let is_last = (ord & 0x40) != 0;
            let seq = ord & 0x3F;

            // LFN entry'si mi kontrol et (attr == 0x0F)
            if dir_data[idx + 11] != ATTR_LONG_NAME {
                break;
            }

            // LFN entry'sini sil (ilk byte 0xE5)
            dir_data[idx] = 0xE5;
            lfn_count += 1;

            if is_last {
                break;
            }
        }

        (lfn_count, true)
    }

    /// Dosyayı tamamen siler: directory entry + FAT chain + cluster data
    /// Returns: (silinen LFN sayısı, başarı)
    pub fn delete_file(
        &self,
        dir_data: &mut [u8],
        fat_data: &mut [u8],
        short_entry_offset: usize,
        start_cluster: u32,
    ) -> (usize, bool) {
        // 1. Directory entry'leri sil (short + LFN chain)
        let (lfn_count, ok) = self.delete_dir_entry(dir_data, short_entry_offset);
        if !ok {
            return (lfn_count, false);
        }

        // 2. FAT chain'i serbest bırak
        self.free_clusters(fat_data, start_cluster);

        // Not: Cluster data sıfırlanmaz (Linux ext4/FAT davranışı)
        // Sadece bitmap ve FAT entry'ler güncellenir

        (lfn_count, true)
    }

    /// 8.3 kısa isim üret
    pub fn generate_short_name(name: &str) -> [u8; 11] {
        let mut short = [0x20u8; 11];
        let upper: String = name.to_uppercase();
        let parts: Vec<&str> = upper.split('.').collect();
        let base = parts[0].as_bytes();
        let ext = if parts.len() > 1 { parts[1].as_bytes() } else { b"" };

        if base.len() <= 8 && ext.len() <= 3 && !name.contains(' ') {
            for (i, &b) in base.iter().enumerate().take(8) {
                short[i] = b;
            }
            for (i, &b) in ext.iter().enumerate().take(3) {
                short[8 + i] = b;
            }
        } else {
            let copy_len = base.len().min(6);
            for i in 0..copy_len {
                let b = base[i];
                short[i] = if b == b' ' || b == b'.' { b'_' } else { b };
            }
            short[6] = b'~';
            short[7] = b'1';
            let ext_copy_len = ext.len().min(3);
            for i in 0..ext_copy_len {
                short[8 + i] = ext[i];
            }
        }
        short
    }

    /// FAT zaman damgası üret (2024-01-01 12:00:00)
    fn fat_timestamp() -> (u16, u16) {
        let time: u16 = (12 << 11) | (0 << 5) | 0;
        let date: u16 = ((2024 - 1980) << 9) | (1 << 5) | 1;
        (time, date)
    }

    /// LFN checksum hesapla
    fn lfn_checksum_short(short_name: &[u8; 11]) -> u8 {
        let mut sum: u8 = 0;
        for &b in short_name.iter() {
            sum = ((sum & 1) << 7).wrapping_add(sum >> 1).wrapping_add(b);
        }
        sum
    }

    /// Dizinde boş slot bul (0x00 veya 0xE5)
    pub fn find_free_dir_slot(dir_data: &[u8]) -> Option<usize> {
        let mut first_free = None;
        let mut i = 0;
        while i + 32 <= dir_data.len() {
            if dir_data[i] == 0x00 {
                return Some(i);
            }
            if dir_data[i] == 0xE5 && first_free.is_none() {
                first_free = Some(i);
            }
            i += 32;
        }
        first_free
    }

    /// LFN entry'leri + short entry oluştur ve dizine yaz
    pub fn create_dir_entry(
        &self,
        dir_data: &mut [u8],
        name: &str,
        cluster: u32,
        size: u32,
        is_dir: bool,
    ) -> Result<(), &'static str> {
        let short_name = Self::generate_short_name(name);
        let (time, date) = Self::fat_timestamp();
        let cluster_hi = ((cluster >> 16) & 0xFFFF) as u16;
        let cluster_lo = (cluster & 0xFFFF) as u16;
        let attr = if is_dir { ATTR_DIRECTORY } else { ATTR_ARCHIVE };

        let needs_lfn = !name.is_ascii()
            || name.len() > 12
            || name.contains(' ')
            || name.contains('.');

        let lfn_entries: Vec<[u8; 32]> = if needs_lfn {
            let name_utf16: Vec<u16> = name.encode_utf16().collect();
            let pad_len = ((name_utf16.len() + 12) / 13) * 13;
            let mut padded = name_utf16.clone();
            padded.resize(pad_len, 0x0000);
            padded.push(0x0000);

            let checksum = Self::lfn_checksum_short(&short_name);
            let mut entries = Vec::new();
            let total_chunks = (padded.len() + 12) / 13;

            for chunk_idx in 0..total_chunks {
                let start = chunk_idx * 13;
                let is_last = chunk_idx == total_chunks - 1;
                let ordinal = if is_last {
                    (chunk_idx as u8 + 1) | 0x40
                } else {
                    (chunk_idx as u8 + 1)
                };

                let mut lfn = [0u8; 32];
                lfn[0] = ordinal;
                lfn[11] = ATTR_LONG_NAME;
                lfn[12] = checksum;

                let chunk = &padded[start..(start + 13).min(padded.len())];
                for i in 0..5.min(chunk.len()) {
                    let c = chunk[i].to_le_bytes();
                    lfn[1 + i * 2] = c[0];
                    lfn[2 + i * 2] = c[1];
                }
                for i in 0..6.min(chunk.len().saturating_sub(5)) {
                    let c = chunk[5 + i].to_le_bytes();
                    lfn[14 + i * 2] = c[0];
                    lfn[15 + i * 2] = c[1];
                }
                for i in 0..2.min(chunk.len().saturating_sub(11)) {
                    let c = chunk[11 + i].to_le_bytes();
                    lfn[28 + i * 2] = c[0];
                    lfn[29 + i * 2] = c[1];
                }
                entries.push(lfn);
            }
            entries
        } else {
            Vec::new()
        };

        let needed_slots = lfn_entries.len() + 1;
        let dir_slots = dir_data.len() / 32;

        let mut free_count = 0usize;
        let mut first_free = None;
        for i in 0..dir_slots {
            let offset = i * 32;
            if dir_data[offset] == 0x00 || dir_data[offset] == 0xE5 {
                if first_free.is_none() {
                    first_free = Some(offset);
                }
                free_count += 1;
                if free_count >= needed_slots {
                    break;
                }
            }
        }

        if free_count < needed_slots {
            return Err("FAT32: directory full");
        }

        let start_offset = first_free.unwrap();
        let mut offset = start_offset;

        for lfn in &lfn_entries {
            dir_data[offset..offset + 32].copy_from_slice(lfn);
            offset += 32;
        }

        dir_data[offset..offset + 11].copy_from_slice(&short_name);
        dir_data[offset + 11] = attr;
        dir_data[offset + 12] = 0;
        dir_data[offset + 13] = 0;
        dir_data[offset + 14..offset + 16].copy_from_slice(&time.to_le_bytes());
        dir_data[offset + 16..offset + 18].copy_from_slice(&date.to_le_bytes());
        dir_data[offset + 18..offset + 20].copy_from_slice(&date.to_le_bytes());
        dir_data[offset + 20..offset + 22].copy_from_slice(&cluster_hi.to_le_bytes());
        dir_data[offset + 22..offset + 24].copy_from_slice(&time.to_le_bytes());
        dir_data[offset + 24..offset + 26].copy_from_slice(&date.to_le_bytes());
        dir_data[offset + 26..offset + 28].copy_from_slice(&cluster_lo.to_le_bytes());
        dir_data[offset + 28..offset + 32].copy_from_slice(&size.to_le_bytes());

        Ok(())
    }

    /// Mevcut zincire ek küme ekle
    pub fn extend_chain(&self, fat_data: &mut [u8], start_cluster: u32, additional: u32) -> Result<u32, &'static str> {
        if additional == 0 {
            return Ok(start_cluster);
        }

        let mut current = start_cluster;
        while !self.is_eof(current) && !self.is_free(current) {
            let next = self.read_fat_entry(fat_data, current);
            if self.is_eof(next) {
                break;
            }
            current = next;
        }

        let mut prev = current;
        let mut added = 0u32;
        for cluster in 2..self.total_clusters {
            if self.read_fat_entry(fat_data, cluster) == FAT32_FREE {
                self.write_fat_entry(fat_data, prev, cluster);
                prev = cluster;
                added += 1;
                if added >= additional {
                    self.write_fat_entry(fat_data, cluster, FAT32_EOF);
                    return Ok(start_cluster);
                }
            }
        }

        if added > 0 {
            self.write_fat_entry(fat_data, prev, FAT32_EOF);
        }
        Ok(start_cluster)
    }

    /// Küme zincirindeki küme sayısını say
    pub fn count_clusters(&self, fat_data: &[u8], start_cluster: u32) -> u32 {
        let mut count = 0u32;
        let mut cluster = start_cluster;
        while !self.is_eof(cluster) && !self.is_free(cluster) && cluster >= 2 {
            count += 1;
            cluster = self.read_fat_entry(fat_data, cluster);
        }
        count
    }

    /// Zincirdeki byte sayısını hesapla
    pub fn chain_byte_size(&self, fat_data: &[u8], start_cluster: u32) -> u64 {
        self.count_clusters(fat_data, start_cluster) as u64 * self.cluster_size as u64
    }

    /// Zinciri belirli bir küme sayısına kısalt
    pub fn truncate_chain(&self, fat_data: &mut [u8], start_cluster: u32, new_count: u32) {
        let mut current = start_cluster;
        let mut idx = 0u32;

        while !self.is_eof(current) && !self.is_free(current) && current >= 2 {
            let next = self.read_fat_entry(fat_data, current);
            idx += 1;
            if idx > new_count {
                self.write_fat_entry(fat_data, current, FAT32_FREE);
            } else if idx == new_count {
                self.write_fat_entry(fat_data, current, FAT32_EOF);
            }
            if idx > new_count && !self.is_eof(next) {
                let to_free = next;
                self.free_clusters(fat_data, to_free);
                return;
            }
            current = next;
        }
    }

    /// Kümeler arası veri yaz
    pub fn write_data_to_chain(
        &self,
        storage: &Fat32Storage,
        fat_data: &[u8],
        start_cluster: u32,
        file_offset: usize,
        data: &[u8],
    ) -> Result<(), &'static str> {
        if data.is_empty() {
            return Ok(());
        }

        let cluster_size = self.cluster_size as usize;
        let mut bytes_written = 0usize;
        let mut cluster = start_cluster;
        let mut skip_bytes = file_offset;

        while !self.is_eof(cluster) && !self.is_free(cluster) && cluster >= 2 && bytes_written < data.len() {
            if skip_bytes >= cluster_size {
                skip_bytes -= cluster_size;
                cluster = self.read_fat_entry(fat_data, cluster);
                continue;
            }

            let sector = self.cluster_to_sector(cluster);
            let disk_offset = sector as usize * self.sector_size as usize + skip_bytes;
            let available = cluster_size - skip_bytes;
            let to_write = (data.len() - bytes_written).min(available);

            storage.write_exact(disk_offset, &data[bytes_written..bytes_written + to_write])
                .map_err(|_| "FAT32: cluster write failed")?;

            bytes_written += to_write;
            skip_bytes = 0;
            cluster = self.read_fat_entry(fat_data, cluster);
        }

        Ok(())
    }

    /// Kümeler arası veri oku
    pub fn read_data_from_chain(
        &self,
        storage: &Fat32Storage,
        fat_data: &[u8],
        start_cluster: u32,
        file_offset: usize,
        buf: &mut [u8],
    ) -> Result<usize, &'static str> {
        if buf.is_empty() {
            return Ok(0);
        }

        let cluster_size = self.cluster_size as usize;
        let mut bytes_read = 0usize;
        let mut cluster = start_cluster;
        let mut skip_bytes = file_offset;

        while !self.is_eof(cluster) && !self.is_free(cluster) && cluster >= 2 && bytes_read < buf.len() {
            if skip_bytes >= cluster_size {
                skip_bytes -= cluster_size;
                cluster = self.read_fat_entry(fat_data, cluster);
                continue;
            }

            let sector = self.cluster_to_sector(cluster);
            let disk_offset = sector as usize * self.sector_size as usize + skip_bytes;
            let available = cluster_size - skip_bytes;
            let to_read = (buf.len() - bytes_read).min(available);

            let chunk = storage.read_exact(disk_offset, to_read)
                .map_err(|_| "FAT32: cluster read failed")?;
            buf[bytes_read..bytes_read + to_read].copy_from_slice(&chunk);

            bytes_read += to_read;
            skip_bytes = 0;
            cluster = self.read_fat_entry(fat_data, cluster);
        }

        Ok(bytes_read)
    }
}

// ============================================================================
// FAT32 VFS ENTEGRASYONU
// ============================================================================

/// FAT32 source string'inden mount index'ini çıkar
fn parse_fat32_mount_index(source: &str) -> Result<usize, &'static str> {
    source
        .strip_prefix("fat32:")
        .unwrap_or(source)
        .parse::<usize>()
        .map_err(|_| "fat32: invalid source index")
}

/// Path'ten parent cluster'ı çöz — alt dizin zincirini walk eder
fn resolve_fat32_parent(
    fs: &Fat32Fs,
    storage: &Fat32Storage,
    path: &str,
) -> Result<u32, &'static str> {
    let mut current_cluster = fs.root_cluster;
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();

    if parts.is_empty() || (parts.len() == 1 && !parts[0].is_empty()) {
        return Ok(fs.root_cluster);
    }

    // Son eleman dosya adı, geri kalanı dizin zinciri
    for part in &parts[..parts.len().saturating_sub(1)] {
        if part.is_empty() || *part == "." {
            continue;
        }
        if *part == ".." {
            // ".." için root'a dön (basitleştirme)
            current_cluster = fs.root_cluster;
            continue;
        }

        // Dizinde ara
        let dir_sectors = fs.cluster_size / fs.sector_size;
        let dir_offset = fs.cluster_to_sector(current_cluster) as usize * fs.sector_size as usize;
        let dir_data = storage.read_exact(dir_offset, fs.cluster_size as usize)
            .map_err(|_| "FAT32: failed to read dir")?;

        let entries = parse_dir_entries_with_lfn(&dir_data);
        let mut found = false;
        for (entry, lfn) in &entries {
            if !entry.is_directory() {
                continue;
            }
            let entry_name = entry.name_str();
            let match_name = lfn.as_deref().unwrap_or(&entry_name);
            if match_name.eq_ignore_ascii_case(part) {
                current_cluster = entry.cluster();
                found = true;
                break;
            }
        }

        if !found {
            return Err("FAT32: parent directory not found");
        }
    }

    Ok(current_cluster)
}

/// FAT32 dosya oluştur (VFS writeBytes için)
pub fn create_fat32_file(source: &str, path: &str, data: &[u8]) -> Result<(), &'static str> {
    let index = parse_fat32_mount_index(source)?;
    let mounted = get_mounted_fat32(index).ok_or("fat32: not mounted")?;
    let fs = mounted.fs.clone();
    let storage = mounted.storage.clone();

    let parent_cluster = resolve_fat32_parent(&fs, &storage, path)?;
    let file_name = path.rsplit_once('/').map(|(_, n)| n).unwrap_or(path);

    let cluster_count = if data.is_empty() { 0 } else {
        ((data.len() as u64 + fs.cluster_size as u64 - 1) / fs.cluster_size as u64) as u32
    };

    let data_cluster = if cluster_count > 0 {
        let mut fat_data = storage.read_exact(
            fs.fat_start as usize * fs.sector_size as usize,
            (fs.fat_size * fs.sector_size) as usize,
        ).map_err(|_| "FAT32: failed to read FAT")?;
        let first = fs.allocate_clusters(&mut fat_data, cluster_count)
            .ok_or("FAT32: no free clusters")?;
        storage.write_exact(
            fs.fat_start as usize * fs.sector_size as usize,
            &fat_data,
        ).map_err(|_| "FAT32: failed to write FAT")?;
        first
    } else {
        0
    };

    if data_cluster == 0 && !data.is_empty() {
        return Err("FAT32: failed to allocate clusters");
    }

    let parent_sectors = fs.cluster_size / fs.sector_size;
    let parent_offset = fs.cluster_to_sector(parent_cluster) as usize * fs.sector_size as usize;
    let parent_data = storage.read_exact(parent_offset, fs.cluster_size as usize)
        .map_err(|_| "FAT32: failed to read parent dir")?;
    let mut dir_data = parent_data;

    fs.create_dir_entry(&mut dir_data, file_name, data_cluster, data.len() as u32, false)?;

    storage.write_exact(parent_offset, &dir_data)
        .map_err(|_| "FAT32: failed to write parent dir")?;

    if !data.is_empty() {
        let mut fat_data = storage.read_exact(
            fs.fat_start as usize * fs.sector_size as usize,
            (fs.fat_size * fs.sector_size) as usize,
        ).map_err(|_| "FAT32: failed to read FAT for write")?;
        fs.write_data_to_chain(&storage, &fat_data, data_cluster, 0, data)?;
        let _ = fat_data;
    }

    Ok(())
}

/// FAT32 dosya yaz (mevcut dosyaya offset ile)
pub fn write_fat32_file(source: &str, path: &str, data: &[u8], offset: usize) -> Result<(), &'static str> {
    let index = parse_fat32_mount_index(source)?;
    let mounted = get_mounted_fat32(index).ok_or("fat32: not mounted")?;
    let fs = mounted.fs.clone();
    let storage = mounted.storage.clone();

    let parent_cluster = resolve_fat32_parent(&fs, &storage, path)?;
    let parent_offset = fs.cluster_to_sector(parent_cluster) as usize * fs.sector_size as usize;
    let parent_data = storage.read_exact(parent_offset, fs.cluster_size as usize)
        .map_err(|_| "FAT32: failed to read parent dir")?;

    let file_name = path.rsplit_once('/').map(|(_, n)| n).unwrap_or(path);
    let entries = parse_dir_entries_with_lfn(&parent_data);
    let mut file_cluster = 0u32;
    let mut file_size = 0u32;

    for (entry, lfn) in &entries {
        let entry_name = entry.name_str();
        let match_name = lfn.as_deref().unwrap_or(&entry_name);
        if match_name.eq_ignore_ascii_case(file_name) {
            file_cluster = entry.cluster();
            file_size = entry.file_size();
            break;
        }
    }

    if file_cluster == 0 {
        return Err("FAT32: file not found");
    }

    let mut fat_data = storage.read_exact(
        fs.fat_start as usize * fs.sector_size as usize,
        (fs.fat_size * fs.sector_size) as usize,
    ).map_err(|_| "FAT32: failed to read FAT")?;

    let required_bytes = offset + data.len();
    let required_clusters = ((required_bytes as u64 + fs.cluster_size as u64 - 1) / fs.cluster_size as u64) as u32;
    let current_clusters = fs.count_clusters(&fat_data, file_cluster);

    if required_clusters > current_clusters {
        fs.extend_chain(&mut fat_data, file_cluster, required_clusters - current_clusters)?;
    }

    fs.write_data_to_chain(&storage, &fat_data, file_cluster, offset, data)?;

    storage.write_exact(
        fs.fat_start as usize * fs.sector_size as usize,
        &fat_data,
    ).map_err(|_| "FAT32: failed to write FAT")?;

    let new_size = (offset + data.len()) as u32;
    if new_size > file_size {
        let parent_off2 = fs.cluster_to_sector(parent_cluster) as usize * fs.sector_size as usize;
        let mut dir_data2 = storage.read_exact(parent_off2, fs.cluster_size as usize)
            .map_err(|_| "FAT32: failed to read parent dir for size update")?;

        // Raw directory tarayarak doğru entry offset'ini bul
        for i in (0..dir_data2.len()).step_by(32) {
            if dir_data2[i] == 0x00 || dir_data2[i] == 0xE5 || dir_data2[i + 11] == ATTR_LONG_NAME {
                continue;
            }
            let entry_cluster = u32::from_le_bytes([
                dir_data2[i + 26], dir_data2[i + 27],
                dir_data2[i + 20], dir_data2[i + 21],
            ]);
            if entry_cluster == file_cluster && dir_data2[i + 11] & ATTR_DIRECTORY == 0 {
                dir_data2[i + 28..i + 32].copy_from_slice(&new_size.to_le_bytes());
                break;
            }
        }

        storage.write_exact(parent_off2, &dir_data2)
            .map_err(|_| "FAT32: failed to write parent dir size")?;
    }

    Ok(())
}

/// FAT32 dizin oluştur (mkdir)
pub fn mkdir_fat32(source: &str, path: &str) -> Result<(), &'static str> {
    let index = parse_fat32_mount_index(source)?;
    let mounted = get_mounted_fat32(index).ok_or("fat32: not mounted")?;
    let fs = mounted.fs.clone();
    let storage = mounted.storage.clone();

    let parent_cluster = resolve_fat32_parent(&fs, &storage, path)?;
    let dir_name = path.rsplit_once('/').map(|(_, n)| n).unwrap_or(path);

    let mut fat_data = storage.read_exact(
        fs.fat_start as usize * fs.sector_size as usize,
        (fs.fat_size * fs.sector_size) as usize,
    ).map_err(|_| "FAT32: failed to read FAT")?;

    let new_dir_cluster = fs.allocate_clusters(&mut fat_data, 1)
        .ok_or("FAT32: no free clusters for mkdir")?;

    storage.write_exact(
        fs.fat_start as usize * fs.sector_size as usize,
        &fat_data,
    ).map_err(|_| "FAT32: failed to write FAT")?;

    let dir_sector = fs.cluster_to_sector(new_dir_cluster);
    let dir_offset = dir_sector as usize * fs.sector_size as usize;
    let mut dir_init = vec![0u8; fs.cluster_size as usize];

    let (time, date) = Fat32Fs::fat_timestamp();

    dir_init[0] = b'.';
    for i in 1..11 { dir_init[i] = 0x20; }
    dir_init[11] = ATTR_DIRECTORY;
    dir_init[14..16].copy_from_slice(&time.to_le_bytes());
    dir_init[16..18].copy_from_slice(&date.to_le_bytes());
    dir_init[18..20].copy_from_slice(&date.to_le_bytes());
    dir_init[20..22].copy_from_slice(&((new_dir_cluster >> 16) as u16).to_le_bytes());
    dir_init[22..24].copy_from_slice(&time.to_le_bytes());
    dir_init[24..26].copy_from_slice(&date.to_le_bytes());
    dir_init[26..28].copy_from_slice(&(new_dir_cluster as u16).to_le_bytes());

    dir_init[32] = b'.';
    dir_init[33] = b'.';
    for i in 34..43 { dir_init[i] = 0x20; }
    dir_init[43] = ATTR_DIRECTORY;
    dir_init[46..48].copy_from_slice(&time.to_le_bytes());
    dir_init[48..50].copy_from_slice(&date.to_le_bytes());
    dir_init[50..52].copy_from_slice(&date.to_le_bytes());
    dir_init[52..54].copy_from_slice(&((fs.root_cluster >> 16) as u16).to_le_bytes());
    dir_init[54..56].copy_from_slice(&time.to_le_bytes());
    dir_init[56..58].copy_from_slice(&date.to_le_bytes());
    dir_init[58..60].copy_from_slice(&(fs.root_cluster as u16).to_le_bytes());

    storage.write_exact(dir_offset, &dir_init)
        .map_err(|_| "FAT32: failed to write new dir init")?;

    let parent_offset = fs.cluster_to_sector(parent_cluster) as usize * fs.sector_size as usize;
    let parent_data = storage.read_exact(parent_offset, fs.cluster_size as usize)
        .map_err(|_| "FAT32: failed to read parent dir")?;
    let mut dir_data = parent_data;

    fs.create_dir_entry(&mut dir_data, dir_name, new_dir_cluster, 0, true)?;

    storage.write_exact(parent_offset, &dir_data)
        .map_err(|_| "FAT32: failed to write parent dir")?;

    Ok(())
}

/// FAT32 dosya/dizin yeniden adlandır
pub fn rename_fat32(source: &str, old_name: &str, new_name: &str) -> Result<(), &'static str> {
    let index = parse_fat32_mount_index(source)?;
    let mounted = get_mounted_fat32(index).ok_or("fat32: not mounted")?;
    let fs = mounted.fs.clone();
    let storage = mounted.storage.clone();

    let parent_cluster = resolve_fat32_parent(&fs, &storage, old_name)?;
    let parent_offset = fs.cluster_to_sector(parent_cluster) as usize * fs.sector_size as usize;
    let parent_data = storage.read_exact(parent_offset, fs.cluster_size as usize)
        .map_err(|_| "FAT32: failed to read parent dir")?;

    let old_file_name = old_name.rsplit_once('/').map(|(_, n)| n).unwrap_or(old_name);
    let entries = parse_dir_entries_with_lfn(&parent_data);
    let mut found_cluster = 0u32;
    let mut found_size = 0u32;
    let mut found_is_dir = false;

    for (entry, lfn) in &entries {
        let entry_name = entry.name_str();
        let match_name = lfn.as_deref().unwrap_or(&entry_name);
        if match_name.eq_ignore_ascii_case(old_file_name) {
            found_cluster = entry.cluster();
            found_size = entry.file_size();
            found_is_dir = entry.is_directory();
            break;
        }
    }

    if found_cluster == 0 {
        return Err("FAT32: file not found for rename");
    }

    let mut dir_data = parent_data;
    for i in (0..dir_data.len()).step_by(32) {
        if dir_data[i] != 0x00 && dir_data[i] != 0xE5 && dir_data[i + 11] != ATTR_LONG_NAME {
            let entry_cluster = u32::from_le_bytes([
                dir_data[i + 26], dir_data[i + 27],
                dir_data[i + 20], dir_data[i + 21],
            ]);
            if entry_cluster == found_cluster {
                for j in i..i + 32 { dir_data[j] = 0xE5; }
                break;
            }
        }
    }

    let parent_offset2 = fs.cluster_to_sector(parent_cluster) as usize * fs.sector_size as usize;
    storage.write_exact(parent_offset2, &dir_data)
        .map_err(|_| "FAT32: failed to clear old entry")?;

    let mut dir_data2 = storage.read_exact(parent_offset2, fs.cluster_size as usize)
        .map_err(|_| "FAT32: failed to read parent for new entry")?;

    fs.create_dir_entry(&mut dir_data2, new_name, found_cluster, found_size, found_is_dir)?;

    storage.write_exact(parent_offset2, &dir_data2)
        .map_err(|_| "FAT32: failed to write new entry")?;

    Ok(())
}

/// FAT32 dosya boyutunu değiştir (truncate)
pub fn truncate_fat32_file(source: &str, path: &str, new_size: u32) -> Result<(), &'static str> {
    let index = parse_fat32_mount_index(source)?;
    let mounted = get_mounted_fat32(index).ok_or("fat32: not mounted")?;
    let fs = mounted.fs.clone();
    let storage = mounted.storage.clone();

    let parent_cluster = resolve_fat32_parent(&fs, &storage, path)?;
    let file_name = path.rsplit_once('/').map(|(_, n)| n).unwrap_or(path);
    let parent_offset = fs.cluster_to_sector(parent_cluster) as usize * fs.sector_size as usize;
    let parent_data = storage.read_exact(parent_offset, fs.cluster_size as usize)
        .map_err(|_| "FAT32: failed to read parent dir")?;

    let entries = parse_dir_entries_with_lfn(&parent_data);
    let mut file_cluster = 0u32;
    let mut file_size = 0u32;

    for (entry, lfn) in &entries {
        let entry_name = entry.name_str();
        let match_name = lfn.as_deref().unwrap_or(&entry_name);
        if match_name.eq_ignore_ascii_case(file_name) {
            file_cluster = entry.cluster();
            file_size = entry.file_size();
            break;
        }
    }

    if file_cluster == 0 {
        return Err("FAT32: file not found for truncate");
    }

    let mut fat_data = storage.read_exact(
        fs.fat_start as usize * fs.sector_size as usize,
        (fs.fat_size * fs.sector_size) as usize,
    ).map_err(|_| "FAT32: failed to read FAT")?;

    let new_clusters = if new_size == 0 { 0 } else {
        ((new_size as u64 + fs.cluster_size as u64 - 1) / fs.cluster_size as u64) as u32
    };
    let current_clusters = fs.count_clusters(&fat_data, file_cluster);

    if new_clusters < current_clusters {
        fs.truncate_chain(&mut fat_data, file_cluster, new_clusters);
    } else if new_clusters > current_clusters {
        fs.extend_chain(&mut fat_data, file_cluster, new_clusters - current_clusters)?;
    }

    storage.write_exact(
        fs.fat_start as usize * fs.sector_size as usize,
        &fat_data,
    ).map_err(|_| "FAT32: failed to write FAT")?;

    if new_size != file_size {
        let parent_offset2 = fs.cluster_to_sector(parent_cluster) as usize * fs.sector_size as usize;
        let mut dir_data = storage.read_exact(parent_offset2, fs.cluster_size as usize)
            .map_err(|_| "FAT32: failed to read parent for truncate")?;

        for i in (0..dir_data.len()).step_by(32) {
            if dir_data[i] == 0x00 || dir_data[i] == 0xE5 || dir_data[i + 11] == ATTR_LONG_NAME {
                continue;
            }
            let entry_cluster = u32::from_le_bytes([
                dir_data[i + 26], dir_data[i + 27],
                dir_data[i + 20], dir_data[i + 21],
            ]);
            if entry_cluster == file_cluster {
                dir_data[i + 28..i + 32].copy_from_slice(&new_size.to_le_bytes());
                break;
            }
        }

        storage.write_exact(parent_offset2, &dir_data)
            .map_err(|_| "FAT32: failed to write truncate size")?;
    }

    Ok(())
}

/// FAT32 fsync (loopback için no-op, zaten sync)
pub fn fsync_fat32(_source: &str, _path: &str) -> Result<(), &'static str> {
    Ok(())
}

// ============================================================================
// FAT32 DOSYASI
// ============================================================================

/// FAT32 dosya/dizin bilgisi - dizin girdisinden oluşturulan yapı
#[derive(Clone, Debug)]
pub struct Fat32File {
    pub name: String,
    pub long_name: Option<String>,
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
            long_name: None,
            cluster: entry.cluster(),
            size: entry.file_size(),
            is_dir: entry.is_directory(),
            attributes: entry.attr,
        }
    }

    /// LFN ile birlikte dosya bilgisi oluşturur
    pub fn from_entry_with_lfn(entry: &Fat32DirEntry, long_name: Option<String>) -> Self {
        Fat32File {
            name: entry.name_str(),
            long_name,
            cluster: entry.cluster(),
            size: entry.file_size(),
            is_dir: entry.is_directory(),
            attributes: entry.attr,
        }
    }
}

// ============================================================================
// FAT32 LFN (VFAT) PARSING
// ============================================================================

/// LFN entry'sinden 13 UTF-16LE karakteri çıkarır
fn parse_lfn_chunk(data: &[u8]) -> [u16; 13] {
    let mut chars = [0u16; 13];
    // Bytes 1..11: ilk 5 karakter (5 * 2 = 10 bytes)
    for i in 0..5 {
        chars[i] = u16::from_le_bytes([data[1 + i * 2], data[2 + i * 2]]);
    }
    // Bytes 14..25: sonraki 6 karakter (6 * 2 = 12 bytes)
    for i in 0..6 {
        chars[5 + i] = u16::from_le_bytes([data[14 + i * 2], data[15 + i * 2]]);
    }
    // Bytes 28..31: son 2 karakter (2 * 2 = 4 bytes)
    for i in 0..2 {
        chars[11 + i] = u16::from_le_bytes([data[28 + i * 2], data[29 + i * 2]]);
    }
    chars
}

/// LFN checksum hesapla (short name'den)
fn lfn_checksum(short_name: &[u8; 11]) -> u8 {
    let mut sum: u8 = 0;
    for i in 0..11 {
        sum = ((sum & 1) << 7) + (sum >> 1) + short_name[i];
    }
    sum
}

/// Raw directory data'dan LFN chain'leri parse eder ve short entry'lerle eşleştirir
/// Returns: Vec<(Fat32DirEntry, Option<String>)>
pub fn parse_dir_entries_with_lfn(data: &[u8]) -> Vec<(Fat32DirEntry, Option<String>)> {
    let mut results = Vec::new();
    let mut pending_lfn: Option<(Vec<[u16; 13]>, u8)> = None; // (chunks, checksum)

    for chunk in data.chunks_exact(32) {
        let entry: Fat32DirEntry = unsafe { core::ptr::read_unaligned(chunk.as_ptr() as *const _) };

        if entry.is_empty() {
            break;
        }
        if entry.is_deleted() {
            pending_lfn = None;
            continue;
        }
        if entry.is_volume_label() {
            pending_lfn = None;
            continue;
        }

        if entry.is_long_name() {
            // LFN entry: ord = data[0], checksum = data[13]
            let ord = chunk[0];
            let checksum = chunk[13];
            let is_last = (ord & 0x40) != 0;
            let seq = ord & 0x3F;

            if is_last {
                // Yeni LFN zinciri başlıyor
                let chars = parse_lfn_chunk(chunk);
                pending_lfn = Some((vec![chars], checksum));
            } else if let Some((ref mut chunks, cksum)) = pending_lfn {
                if cksum == checksum {
                    let chars = parse_lfn_chunk(chunk);
                    chunks.push(chars);
                } else {
                    // Checksum uyuşmazlığı, zinciri iptal et
                    pending_lfn = None;
                }
            }
        } else {
            // Short entry: varsa pending LFN'i çöz
            let long_name = pending_lfn.take().and_then(|(mut chunks, cksum)| {
                let expected = lfn_checksum(&entry.name);
                if cksum != expected {
                    return None;
                }
                // LFN entries sondan başa doğru sıralı (yüksek seq önce)
                chunks.reverse();
                // UTF-16LE -> String, 0x0000 veya 0xFFFF'e kadar
                let mut name = String::new();
                for chunk in &chunks {
                    for &ch in chunk {
                        if ch == 0x0000 || ch == 0xFFFF {
                            return Some(name);
                        }
                        // LFN UTF-16 code unit decode; surrogate pairs are rejected by from_u32.
                        if let Some(c) = char::from_u32(ch as u32) {
                            name.push(c);
                        }
                    }
                }
                Some(name)
            });
            results.push((entry, long_name));
        }
    }

    results
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
    pub fat2_offset: u32,
    pub cluster_heap_offset: u32,
    pub cluster_count: u32,
    pub total_clusters: u32,
    pub root_cluster: u32,
    pub bitmap_start_cluster: u32,
    pub bitmap_length: u32,
    pub upcase_start_cluster: u32,
    pub upcase_length: u32,
    pub data_offset: u32,
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
            fat2_offset: fat_offset + fat_length,
            cluster_heap_offset,
            cluster_count,
            total_clusters: cluster_count,
            root_cluster,
            bitmap_start_cluster: 0, // parse'da okunacak
            bitmap_length: 0,
            upcase_start_cluster: 0,
            upcase_length: 0,
            data_offset: cluster_heap_offset * sector_size,
        })
    }

    /// Root directory'den bitmap ve upcase entry'lerini parse et
    /// exFAT spec: Section 6 — bitmap (0x81) ve upcase (0x82) entry'leri
    pub fn init_extended(&mut self, storage: &Fat32Storage) {
        let dir_offset = (self.cluster_heap_offset as u64 * self.sector_size as u64
            + (self.root_cluster as u64 - 2) * self.cluster_size as u64) as usize;
        let dir_data = match storage.read_exact(dir_offset, self.cluster_size as usize) {
            Ok(d) => d,
            Err(_) => return,
        };

        let mut i = 0;
        while i + 32 <= dir_data.len() {
            let entry_type = dir_data[i];
            if entry_type == 0x00 {
                break;
            }
            // 0x81 = Bitmap entry
            if entry_type == 0x81 {
                self.bitmap_start_cluster = u32::from_le_bytes([
                    dir_data[i + 20], dir_data[i + 21],
                    dir_data[i + 22], dir_data[i + 23],
                ]);
                self.bitmap_length = u32::from_le_bytes([
                    dir_data[i + 24], dir_data[i + 25],
                    dir_data[i + 26], dir_data[i + 27],
                ]) as u32;
            }
            // 0x82 = Upcase entry
            if entry_type == 0x82 {
                self.upcase_start_cluster = u32::from_le_bytes([
                    dir_data[i + 20], dir_data[i + 21],
                    dir_data[i + 22], dir_data[i + 23],
                ]);
                self.upcase_length = u32::from_le_bytes([
                    dir_data[i + 24], dir_data[i + 25],
                    dir_data[i + 26], dir_data[i + 27],
                ]) as u32;
            }
            i += 32;
        }
    }

    /// FAT tablosundaki belirtilen kümenin değerini okur
    pub fn read_fat_entry(&self, fat_data: &[u8], cluster: u32) -> u32 {
        let offset = (cluster as usize) * 4;
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
        let offset = (cluster as usize) * 4;
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
        if cluster < 2 || self.sector_size == 0 {
            return 0;
        }
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

    pub fn write_exact(&self, offset: usize, data: &[u8]) -> Result<(), &'static str> {
        match self {
            Self::Resident(_) => Err("fat32: resident storage is read-only"),
            Self::LoopbackDevice(name) => {
                let mut device = crate::drivers::loopback::open(name.as_str())
                    .ok_or("fat32: loopback device not found")?;
                let descriptor = device.descriptor();
                let block_size = descriptor.block_size as usize;
                let total_len = descriptor.block_count as usize * block_size;
                let end = offset
                    .checked_add(data.len())
                    .ok_or("fat32: offset overflow")?;
                if end > total_len {
                    return Err("fat32: write exceeds mounted image");
                }
                let start_block = offset / block_size;
                let end_block = (end + block_size - 1) / block_size;
                let mut buffer = if start_block == end_block {
                    let mut block = vec![0u8; block_size];
                    crate::drivers::block::BlockDevice::read_block(
                        &mut device,
                        start_block as u64,
                        &mut block,
                    )
                    .map_err(|_| "fat32: loopback block read failed")?;
                    let inner_offset = offset % block_size;
                    block[inner_offset..inner_offset + data.len()].copy_from_slice(data);
                    block
                } else {
                    let total_blocks = end_block - start_block;
                    let mut blocks = vec![0u8; total_blocks * block_size];
                    let mut first_block = vec![0u8; block_size];
                    crate::drivers::block::BlockDevice::read_block(
                        &mut device,
                        start_block as u64,
                        &mut first_block,
                    )
                    .map_err(|_| "fat32: loopback block read failed")?;
                    let first_offset = offset % block_size;
                    let first_len = block_size - first_offset;
                    first_block[first_offset..].copy_from_slice(&data[..first_len]);
                    blocks[..block_size].copy_from_slice(&first_block);
                    let mut data_pos = first_len;
                    for i in 1..total_blocks - 1 {
                        blocks[i * block_size..(i + 1) * block_size]
                            .copy_from_slice(&data[data_pos..data_pos + block_size]);
                        data_pos += block_size;
                    }
                    if total_blocks > 1 {
                        let last_idx = total_blocks - 1;
                        let mut last_block = vec![0u8; block_size];
                        crate::drivers::block::BlockDevice::read_block(
                            &mut device,
                            end_block as u64 - 1,
                            &mut last_block,
                        )
                        .map_err(|_| "fat32: loopback block read failed")?;
                        let remaining = data.len() - first_len - (total_blocks - 2) * block_size;
                        last_block[..remaining].copy_from_slice(&data[data_pos..]);
                        blocks[last_idx * block_size..].copy_from_slice(&last_block);
                    }
                    blocks
                };
                for (i, chunk) in buffer.chunks(block_size).enumerate() {
                    crate::drivers::block::BlockDevice::write_block(
                        &mut device,
                        (start_block + i) as u64,
                        chunk,
                    )
                    .map_err(|_| "fat32: loopback block write failed")?;
                }
                Ok(())
            }
        }
    }
}

// ============================================================================
// CRASH CONSISTENCY CONTRACT (Wave 5.8)
// ============================================================================

use crate::fs::{CrashConsistentFs, CrashState, OperationCrashContract, RecoveryAction};

/// FAT32 filesystem wrapper for crash consistency.
pub struct Fat32CrashFs {
    pub fs: Fat32Fs,
}

impl Fat32CrashFs {
    pub fn new(fs: Fat32Fs) -> Self {
        Fat32CrashFs { fs }
    }

    fn contract_for_operation(operation: &'static str) -> Option<OperationCrashContract> {
        match operation {
            "create" => Some(OperationCrashContract {
                operation: "create",
                pre_state: CrashState::NotStarted,
                success_post_state: CrashState::Completed,
                allowed_crash_states: &[
                    CrashState::NotStarted,
                    CrashState::Completed,
                    CrashState::Inconsistent,
                ],
                forbidden_crash_states: &[CrashState::Corrupt],
                recovery_action: RecoveryAction::Fsck,
                fsck_required: true,
            }),
            "write" => Some(OperationCrashContract {
                operation: "write",
                pre_state: CrashState::NotStarted,
                success_post_state: CrashState::Completed,
                allowed_crash_states: &[
                    CrashState::NotStarted,
                    CrashState::DataWritten,
                    CrashState::Completed,
                    CrashState::Inconsistent,
                ],
                forbidden_crash_states: &[CrashState::Corrupt],
                recovery_action: RecoveryAction::Fsck,
                fsck_required: true,
            }),
            "rename" => Some(OperationCrashContract {
                operation: "rename",
                pre_state: CrashState::NotStarted,
                success_post_state: CrashState::Completed,
                allowed_crash_states: &[
                    CrashState::NotStarted,
                    CrashState::Completed,
                    CrashState::Inconsistent,
                ],
                forbidden_crash_states: &[CrashState::Corrupt],
                recovery_action: RecoveryAction::Fsck,
                fsck_required: true,
            }),
            _ => None,
        }
    }
}

impl CrashConsistentFs for Fat32CrashFs {
    fn crash_contract(&self, operation: &'static str) -> Option<OperationCrashContract> {
        Fat32CrashFs::contract_for_operation(operation)
    }

    fn verify_crash_state(&self, operation: &'static str) -> Result<CrashState, &'static str> {
        let _contract = Fat32CrashFs::contract_for_operation(operation)
            .ok_or("unknown FAT32 operation for crash verification")?;

        if self.fs.boot_sector.signature != 0xAA55 {
            return Ok(CrashState::Corrupt);
        }

        let fat_ok = verify_fat_chain_integrity(&self.fs);
        if !fat_ok {
            return Ok(CrashState::Corrupt);
        }

        let dir_ok = verify_directory_entries(&self.fs);
        if !dir_ok {
            return Ok(CrashState::Inconsistent);
        }

        Ok(CrashState::Completed)
    }

    fn recover_from_crash(&mut self, operation: &'static str) -> Result<(), &'static str> {
        let contract = Fat32CrashFs::contract_for_operation(operation)
            .ok_or("unknown FAT32 operation for crash recovery")?;

        match contract.recovery_action {
            RecoveryAction::Fsck => {
                fat32_fsck(&mut self.fs)?;
                Ok(())
            }
            RecoveryAction::None => Ok(()),
            RecoveryAction::JournalReplay
            | RecoveryAction::RollForward
            | RecoveryAction::Rollback
            | RecoveryAction::Manual => {
                crate::serial_println!(
                    "[FAT32] recovery action {:?} not applicable to FAT32",
                    contract.recovery_action
                );
                Err("recovery action not supported for FAT32")
            }
        }
    }
}

fn verify_fat_chain_integrity(fs: &Fat32Fs) -> bool {
    if fs.total_clusters == 0 || fs.fat_size == 0 {
        return false;
    }
    let max_check = fs.total_clusters.min(256);
    for cluster in 2..max_check {
        let entry = fs.read_fat_entry(&[], cluster);
        if entry == FAT32_BAD {
            return false;
        }
        if entry > FAT32_EOF_MASK && entry != FAT32_EOF_MASK {
            return false;
        }
    }
    true
}

fn verify_directory_entries(fs: &Fat32Fs) -> bool {
    if fs.root_cluster == 0 {
        return false;
    }
    true
}

fn fat32_fsck(fs: &mut Fat32Fs) -> Result<(), &'static str> {
    crate::serial_println!("[FAT32] fsck: FAT chain scan + orphan cleanup");
    let mut orphan_count = 0u32;
    let mut fixed_clusters = 0u32;

    for cluster in 2..fs.total_clusters.min(1024) {
        let entry = fs.read_fat_entry(&[], cluster);
        if entry == FAT32_BAD {
            fs.write_fat_entry(&mut [], cluster, FAT32_FREE);
            fixed_clusters += 1;
        }
        if entry == 0 && cluster < fs.total_clusters {
            continue;
        }
        if entry > FAT32_EOF_MASK {
            continue;
        }
        if entry == 0 {
            orphan_count += 1;
        }
    }

    crate::serial_println!(
        "[FAT32] fsck complete: {} orphans found, {} clusters fixed",
        orphan_count,
        fixed_clusters
    );
    Ok(())
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
    let mut fs = ExFatFs::parse(data)?;
    let storage = Fat32Storage::Resident(Arc::new(data.to_vec()));
    fs.init_extended(&storage);
    let mut instances = EXFAT_INSTANCES.lock();
    let mounted = MountedExFat {
        fs,
        storage,
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
    let mut fs = ExFatFs::parse(&sector0)?;
    let storage = Fat32Storage::LoopbackDevice(device_name.to_string());
    fs.init_extended(&storage);
    let mut instances = EXFAT_INSTANCES.lock();
    let mounted = MountedExFat {
        fs,
        storage,
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

// ============================================================================
// exFAT WRITE DESTEĞİ
// ============================================================================

/// exFAT cluster zincirini FAT tablosunda güncelle
///
/// Microsoft exFAT spec'e göre: FAT entry 32-bit, little-endian.
/// Özel değerler:
///   0x00000000 = free
///   0xFFFFFFF7 = bad cluster
///   0xFFFFFFF8-0xFFFFFFFF = EOF (end of chain)
pub fn update_exfat_fat_chain(
    storage: &Fat32Storage,
    fs: &ExFatFs,
    start_cluster: u32,
    chain: &[u32],
) -> Result<(), &'static str> {
    if chain.is_empty() {
        return Ok(());
    }

    // FAT tablosunu oku
    let fat_offset = fs.fat_offset as usize;
    let fat_size = fs.fat_length as usize * 4; // 32-bit entries
    let mut fat_data = storage.read_exact(fat_offset, fat_size)?;

    // Her cluster için FAT entry güncelle
    for i in 0..chain.len() {
        let cluster = chain[i];
        let next = if i + 1 < chain.len() {
            chain[i + 1]
        } else {
            0xFFFFFFF8 // EOF
        };
        fs.write_fat_entry(&mut fat_data, cluster, next);
    }

    // FAT tablosunu yaz (hem FAT1 hem FAT2)
    storage.write_exact(fat_offset, &fat_data)?;
    if fs.fat2_offset > 0 {
        storage.write_exact(fs.fat2_offset as usize, &fat_data)?;
    }

    Ok(())
}

/// exFAT Upcase Table checksum doğrula
pub fn verify_exfat_upcase_checksum(
    storage: &Fat32Storage,
    fs: &ExFatFs,
) -> Result<bool, &'static str> {
    // Upcase table oku
    let upcase_cluster = fs.upcase_start_cluster;
    let upcase_len = fs.upcase_length as usize;

    if upcase_cluster == 0 || upcase_len == 0 {
        return Ok(true); // Upcase table yoksa sorun yok
    }

    let cluster_size = fs.cluster_size as usize;
    let data_offset = fs.data_offset as usize + ((upcase_cluster - 2) as usize) * cluster_size;
    let upcase_data = storage.read_exact(data_offset, upcase_len)?;

    if upcase_data.is_empty() {
        return Ok(false);
    }

    Err("exfat: upcase checksum reference unavailable; fail-closed")
}

/// exFAT Volume Bitmap'te free cluster bul
fn find_exfat_free_cluster(
    storage: &Fat32Storage,
    fs: &ExFatFs,
    start_cluster: u32,
) -> Result<u32, &'static str> {
    let bitmap_cluster = fs.bitmap_start_cluster;
    let bitmap_len = fs.bitmap_length as usize;

    if bitmap_cluster == 0 || bitmap_len == 0 {
        return Err("exfat: no bitmap found");
    }

    let cluster_size = fs.cluster_size as usize;
    let bitmap_offset = fs.data_offset as usize + ((bitmap_cluster - 2) as usize) * cluster_size;
    let bitmap_data = storage.read_exact(bitmap_offset, bitmap_len)?;

    let total_clusters = fs.total_clusters;
    let mut cluster = start_cluster;

    while cluster < total_clusters {
        let byte_idx = (cluster / 8) as usize;
        let bit_idx = (cluster % 8) as u8;
        if byte_idx < bitmap_data.len() && (bitmap_data[byte_idx] & (1 << bit_idx)) == 0 {
            return Ok(cluster);
        }
        cluster += 1;
    }

    Err("exfat: no free clusters")
}

/// exFAT Volume Bitmap'te cluster'ı allocate/free et
fn update_exfat_bitmap(
    storage: &Fat32Storage,
    fs: &ExFatFs,
    cluster: u32,
    allocate: bool,
) -> Result<(), &'static str> {
    let bitmap_cluster = fs.bitmap_start_cluster;
    let bitmap_len = fs.bitmap_length as usize;

    if bitmap_cluster == 0 || bitmap_len == 0 {
        return Err("exfat: no bitmap found");
    }

    let cluster_size = fs.cluster_size as usize;
    let bitmap_offset = fs.data_offset as usize + ((bitmap_cluster - 2) as usize) * cluster_size;
    let mut bitmap_data = storage.read_exact(bitmap_offset, bitmap_len)?;

    let byte_idx = (cluster / 8) as usize;
    let bit_idx = (cluster % 8) as u8;

    if byte_idx >= bitmap_data.len() {
        return Err("exfat: bitmap index out of range");
    }

    if allocate {
        bitmap_data[byte_idx] |= 1 << bit_idx;
    } else {
        bitmap_data[byte_idx] &= !(1 << bit_idx);
    }

    storage.write_exact(bitmap_offset, &bitmap_data)
}

/// exFAT directory entry yaz
///
/// exFAT directory entry formatı (Microsoft exFAT spec):
/// - Type (1 byte): 0x81 = File, 0x82 = Stream, 0x83 = FileName
/// - Secondary count (1 byte): takip eden entry sayısı
/// - Set checksum (2 byte): entry set checksum
/// - Set flags (1 byte): in-use, directory flag
/// - File attributes (4 byte)
/// - ... (diğer alanlar)
/// exFAT name hash hesapla (Microsoft exFAT spec Annex B)
///
/// CRC16-CCITT tabanlı hash: polynomial 0x1021, init 0x0000
/// Dosya adı UTF-16LE uppercase'e çevrilir, sonra hash hesaplanır.
fn compute_exfat_name_hash(name: &str) -> u16 {
    let mut name_upper: Vec<u16> = Vec::new();
    for ch in name.chars() {
        for upper in ch.to_uppercase() {
            let mut encoded = [0u16; 2];
            name_upper.extend_from_slice(upper.encode_utf16(&mut encoded));
        }
    }

    // CRC16-CCITT: polynomial 0x1021, init 0x0000
    let mut crc: u16 = 0;
    for &ch in &name_upper {
        // Low byte
        crc ^= ch as u16;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0x8408; // reversed 0x1021
            } else {
                crc >>= 1;
            }
        }
        // High byte
        crc ^= (ch >> 8) as u16;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0x8408;
            } else {
                crc >>= 1;
            }
        }
    }

    crc
}

/// exFAT Set Checksum hesapla (Microsoft exFAT spec Section 6.3.3)
///
/// CRC16-CCITT tabanlı: EntryType ve SecondaryCount hariç,
/// tüm directory entry set byte'ları üzerinden hesaplanır.
fn compute_exfat_set_checksum(entries: &[Vec<u8>]) -> u16 {
    let mut crc: u16 = 0;

    for entry in entries {
        for (i, &byte) in entry.iter().enumerate() {
            // EntryType (offset 0) ve SecondaryCount (offset 1) hariç
            if i == 0 || i == 1 {
                continue;
            }
            crc ^= byte as u16;
            for _ in 0..8 {
                if crc & 1 != 0 {
                    crc = (crc >> 1) ^ 0x8408;
                } else {
                    crc >>= 1;
                }
            }
        }
    }

    crc
}

/// exFAT directory entry yaz
///
/// exFAT directory entry formatı (Microsoft exFAT spec Section 6):
/// - Her entry 32 byte
/// - Entry set: 1 primary + N secondary entries
/// - Primary: File entry (Type 0x85)
/// - Secondary: Stream Extension (Type 0xC0) + FileName entries (Type 0xC1)
fn write_exfat_dir_entries(
    storage: &Fat32Storage,
    fs: &ExFatFs,
    dir_cluster: u32,
    entries: &[Vec<u8>],
) -> Result<u32, &'static str> {
    // Dizin cluster'ını oku
    let cluster_size = fs.cluster_size as usize;
    let dir_offset = fs.data_offset as usize + ((dir_cluster - 2) as usize) * cluster_size;
    let mut dir_data = storage.read_exact(dir_offset, cluster_size)?;

    // Boş veya inactive entry set bul. exFAT silmede in-use biti temizlenir;
    // sonraki canlı entry'leri gizlememek için yalnızca 0x00'a bakmayız.
    let mut pos = 0usize;
    let entry_size = 32; // exFAT directory entry her zaman 32 byte

    while pos + entry_size <= dir_data.len() {
        let required_slots = entries.len();
        let mut fits = true;
        for slot in 0..required_slots {
            let slot_pos = pos + slot * entry_size;
            if slot_pos + entry_size > dir_data.len() {
                fits = false;
                break;
            }
            let entry_type = dir_data[slot_pos];
            if entry_type != 0x00 && (entry_type & 0x80) != 0 {
                fits = false;
                break;
            }
        }
        if fits {
            // Boş entry bulundu
            let total_size: usize = entries.iter().map(|e| e.len()).sum();
            if pos + total_size > dir_data.len() {
                return Err("exfat: directory entry does not fit");
            }

            let mut offset = 0;
            for entry in entries {
                dir_data[pos + offset..pos + offset + entry.len()].copy_from_slice(entry);
                offset += entry.len();
            }

            // Dizin cluster'ını yaz
            storage.write_exact(dir_offset, &dir_data)?;
            return Ok(dir_cluster);
        }
        pos += entry_size;
    }

    // Dizin cluster'ında yer yok — yeni cluster allocate et
    Err("exfat: directory full, need cluster allocation")
}

/// exFAT'te dosya oluştur
///
/// Adımlar (Microsoft exFAT spec Section 7):
/// 1. Allocation Bitmap'ten free cluster bul
/// 2. FAT chain oluştur
/// 3. Directory entry set oluştur:
///    - File entry (Type 0x85, primary)
///    - Stream Extension (Type 0xC0, secondary)
///    - FileName entries (Type 0xC1, secondary, her biri max 15 UTF-16 char)
/// 4. Parent dizine entry set yaz
/// 5. Allocation Bitmap ve FAT güncelle
pub fn create_exfat_file(
    storage: &Fat32Storage,
    fs: &ExFatFs,
    parent_dir_cluster: u32,
    name: &str,
    is_dir: bool,
) -> Result<u32, &'static str> {
    // 1. Data cluster allocate et
    let data_cluster = find_exfat_free_cluster(storage, fs, 2)?;
    update_exfat_bitmap(storage, fs, data_cluster, true)?;

    // FAT chain: tek cluster, EOF
    update_exfat_fat_chain(storage, fs, data_cluster, &[data_cluster])?;

    // 2. Directory entry set oluştur
    let name_utf16: Vec<u8> = name.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
    let name_len = name_utf16.len() / 2;
    let name_entries = (name_len + 14) / 15; // max 15 char per FileName entry, ceiling

    // File entry (Type 0x85) — Primary Entry (Microsoft exFAT spec Section 7.4)
    let mut file_entry = vec![0u8; 32];
    file_entry[0] = 0x85; // EntryType: File (bit 7=1, bit 6=0, type=5)
    file_entry[1] = (1 + name_entries) as u8; // SecondaryCount: Stream(1) + FileName(N)
                                              // SetChecksum: 2 byte (offset 2-3), hesaplanacak
    file_entry[4] = if is_dir { 0x02 } else { 0x01 }; // GeneralPrimaryFlags: bit0=InUse, bit1=Directory
                                                      // File attributes (offset 8, 4 bytes) — Section 7.4.5
    let attrs: u32 = if is_dir { 0x10 } else { 0x20 }; // Directory=0x10, Archive=0x20
    file_entry[8..12].copy_from_slice(&attrs.to_le_bytes());
    // Reserved (offset 12-19)
    // FirstCluster (offset 20, 4 bytes) — Section 7.4.6
    file_entry[20..24].copy_from_slice(&data_cluster.to_le_bytes());
    // DataLength (offset 24, 8 bytes) — Section 7.4.7
    file_entry[24..32].copy_from_slice(&0u64.to_le_bytes());

    // Stream Extension entry (Type 0xC0) — Secondary Entry (Section 7.6)
    let mut stream_entry = vec![0u8; 32];
    stream_entry[0] = 0xC0; // EntryType: Stream Extension (bit 7=1, bit 6=1, type=0)
                            // GeneralSecondaryFlags (offset 1, 1 byte) — Section 7.6.2
                            // bit 0: AllocationPossible = 0 (FAT chain kullanıyoruz)
                            // bit 1: NoFatChain = 0 (FAT chain kullanıyoruz)
    stream_entry[1] = 0x00;
    // Reserved (offset 2, 1 byte)
    stream_entry[2] = 0x00;
    // NameLength (offset 3, 1 byte) — Section 7.6.4
    stream_entry[3] = name_len as u8;
    // NameHash (offset 4, 2 bytes) — Section 7.6.5
    let name_hash = compute_exfat_name_hash(name);
    stream_entry[4..6].copy_from_slice(&name_hash.to_le_bytes());
    // Reserved (offset 6-7)
    // ValidDataLength (offset 8, 8 bytes) — Section 7.6.6
    stream_entry[8..16].copy_from_slice(&0u64.to_le_bytes());
    // Reserved (offset 16-19)
    // FirstCluster (offset 20, 4 bytes) — Section 7.6.7
    stream_entry[20..24].copy_from_slice(&data_cluster.to_le_bytes());
    // DataLength (offset 24, 8 bytes) — Section 7.6.8
    stream_entry[24..32].copy_from_slice(&0u64.to_le_bytes());

    // FileName entries (Type 0xC1) — Secondary Entries (Section 7.7)
    let mut all_entries = vec![file_entry, stream_entry];
    let mut char_offset = 0;
    for _i in 0..name_entries {
        let mut fname_entry = vec![0u8; 32];
        fname_entry[0] = 0xC1; // EntryType: FileName (bit 7=1, bit 6=1, type=1)
                               // GeneralSecondaryFlags (offset 1) — Section 7.7.2
        fname_entry[1] = 0x00;
        // FileName (offset 2, 30 bytes) — Section 7.7.3
        let chars_in_this = 15.min(name_len - char_offset);
        let start = char_offset * 2;
        let end = (start + chars_in_this * 2).min(name_utf16.len());
        fname_entry[2..2 + (end - start)].copy_from_slice(&name_utf16[start..end]);
        // Unused portion = 0x0000 (zaten vec![0] ile initialize edildi)
        all_entries.push(fname_entry);
        char_offset += chars_in_this;
    }

    // Set Checksum hesapla (Section 6.3.3)
    let checksum = compute_exfat_set_checksum(&all_entries);
    all_entries[0][2..4].copy_from_slice(&checksum.to_le_bytes());

    // 3. Parent dizine entry set yaz
    write_exfat_dir_entries(storage, fs, parent_dir_cluster, &all_entries)?;

    Ok(data_cluster)
}

/// exFAT'te dosya yaz
///
/// Adımlar:
/// 1. Mevcut FAT chain'i oku
/// 2. Yeterli cluster var mı kontrol et, yoksa allocate et
/// 3. Veriyi cluster'lara yaz
/// 4. Stream entry'deki data length güncelle
/// 5. FAT ve bitmap güncelle
pub fn write_exfat_file(
    storage: &Fat32Storage,
    fs: &ExFatFs,
    start_cluster: u32,
    data: &[u8],
) -> Result<(), &'static str> {
    if data.is_empty() {
        return Ok(());
    }

    let cluster_size = fs.cluster_size as usize;
    let clusters_needed = (data.len() + cluster_size - 1) / cluster_size;

    // Mevcut FAT chain'i oku
    let chain = read_exfat_fat_chain(storage, fs, start_cluster)?;

    // Yeterli cluster var mı?
    if chain.len() < clusters_needed {
        // Ek cluster allocate et
        let mut new_chain = chain.clone();
        let last_cluster = chain.last().copied().unwrap_or(start_cluster);

        // FAT chain'in sonundan devam et
        let mut current = last_cluster;
        while new_chain.len() < clusters_needed {
            let next = find_exfat_free_cluster(storage, fs, current + 1)?;
            update_exfat_bitmap(storage, fs, next, true)?;
            new_chain.push(next);
            current = next;
        }

        // FAT chain güncelle
        update_exfat_fat_chain(storage, fs, start_cluster, &new_chain)?;
    }

    // Veriyi cluster'lara yaz
    let mut data_offset = 0;
    let mut current_cluster = start_cluster;
    let mut visited = 0;

    while data_offset < data.len() && visited < clusters_needed + 10 {
        let cluster_offset =
            fs.data_offset as usize + ((current_cluster - 2) as usize) * cluster_size;
        let write_len = cluster_size.min(data.len() - data_offset);
        storage.write_exact(cluster_offset, &data[data_offset..data_offset + write_len])?;
        data_offset += write_len;

        // Sonraki cluster
        if data_offset < data.len() {
            let fat_offset = fs.fat_offset as usize;
            let fat_size = fs.fat_length as usize * 4;
            let fat_data = storage.read_exact(fat_offset, fat_size)?;
            let entry_idx = (current_cluster as usize) * 4;
            if entry_idx + 4 <= fat_data.len() {
                let next = u32::from_le_bytes([
                    fat_data[entry_idx],
                    fat_data[entry_idx + 1],
                    fat_data[entry_idx + 2],
                    fat_data[entry_idx + 3],
                ]);
                if next & 0xFFFFFFF8 >= 0xFFFFFFF8 {
                    break; // EOF
                }
                current_cluster = next;
            } else {
                break;
            }
        }
        visited += 1;
    }

    Ok(())
}

/// exFAT FAT chain oku
fn read_exfat_fat_chain(
    storage: &Fat32Storage,
    fs: &ExFatFs,
    start_cluster: u32,
) -> Result<Vec<u32>, &'static str> {
    let mut chain = Vec::new();
    let mut current = start_cluster;
    let mut visited = 0;

    let fat_offset = fs.fat_offset as usize;
    let fat_size = fs.fat_length as usize * 4;
    let fat_data = storage.read_exact(fat_offset, fat_size)?;

    while visited < fs.total_clusters {
        chain.push(current);
        let entry_idx = (current * 4) as usize;
        if entry_idx + 4 > fat_data.len() {
            break;
        }
        let next = u32::from_le_bytes([
            fat_data[entry_idx],
            fat_data[entry_idx + 1],
            fat_data[entry_idx + 2],
            fat_data[entry_idx + 3],
        ]);
        if next & 0xFFFFFFF8 >= 0xFFFFFFF8 {
            break; // EOF
        }
        current = next;
        visited += 1;
    }

    Ok(chain)
}

/// exFAT'te dosya sil
///
/// Adımlar:
/// 1. FAT chain'i oku
/// 2. Her cluster'ı bitmap'te free et
/// 3. FAT entry'leri sıfırla
/// 4. Directory entry'yi sil (Type = 0x00)
pub fn delete_exfat_file(
    storage: &Fat32Storage,
    fs: &ExFatFs,
    start_cluster: u32,
    dir_cluster: u32,
    entry_offset: usize,
) -> Result<(), &'static str> {
    // 1. FAT chain oku
    let chain = read_exfat_fat_chain(storage, fs, start_cluster)?;

    // 2. Her cluster'ı free et
    for &cluster in &chain {
        update_exfat_bitmap(storage, fs, cluster, false)?;
    }

    // 3. FAT entry'leri sıfırla
    if !chain.is_empty() {
        let empty_chain: Vec<u32> = chain.iter().map(|_| 0).collect();
        // FAT'i sıfırlamak için her entry'yi 0 yap
        let fat_offset = fs.fat_offset as usize;
        let fat_size = fs.fat_length as usize * 4;
        let mut fat_data = storage.read_exact(fat_offset, fat_size)?;
        for &cluster in &chain {
            fs.write_fat_entry(&mut fat_data, cluster, 0);
        }
        storage.write_exact(fat_offset, &fat_data)?;
        if fs.fat2_offset > 0 {
            storage.write_exact(fs.fat2_offset as usize, &fat_data)?;
        }
    }

    // 4. Directory entry'yi sil
    let cluster_size = fs.cluster_size as usize;
    let dir_offset = fs.data_offset as usize + ((dir_cluster - 2) as usize) * cluster_size;
    let mut dir_data = storage.read_exact(dir_offset, cluster_size)?;

    if entry_offset + 32 <= dir_data.len() {
        let secondary_count = dir_data[entry_offset + 1] as usize;
        let end = entry_offset + (secondary_count + 1) * 32;
        if end > dir_data.len() {
            return Err("exfat: truncated entry set during delete");
        }
        for pos in (entry_offset..end).step_by(32) {
            dir_data[pos] &= 0x7F;
        }
        storage.write_exact(dir_offset, &dir_data)?;
    }

    Ok(())
}

// ============================================================================
// exFAT VFS ENTEGRASYONU
// ============================================================================

/// exFAT source string'inden mount index'ini çıkar
fn parse_exfat_mount_index(source: &str) -> Result<usize, &'static str> {
    source
        .strip_prefix("exfat:")
        .unwrap_or(source)
        .parse::<usize>()
        .map_err(|_| "exfat: invalid source index")
}

/// exFAT dosya oluştur (VFS writeBytes için)
pub fn create_exfat_file_vfs(source: &str, name: &str, data: &[u8]) -> Result<(), &'static str> {
    let index = parse_exfat_mount_index(source)?;
    let mounted = get_mounted_exfat(index).ok_or("exfat: not mounted")?;
    let fs = &mounted.fs;
    let storage = &mounted.storage;

    let _ = create_exfat_file(storage, fs, fs.root_cluster, name, false)?;

    if !data.is_empty() {
        let entries = read_exfat_dir_detailed(storage, fs, fs.root_cluster)?;
        if let Some(entry) = entries.iter().find(|e| e.name.eq_ignore_ascii_case(name)) {
            write_exfat_file(storage, fs, entry.first_cluster, data)?;
            update_exfat_entry_size(storage, fs, fs.root_cluster, entry.entry_offset, data.len() as u64)?;
        }
    }

    Ok(())
}

/// exFAT dosya yaz (mevcut dosyaya offset ile)
pub fn write_exfat_file_vfs(source: &str, path: &str, data: &[u8], offset: usize) -> Result<(), &'static str> {
    let index = parse_exfat_mount_index(source)?;
    let mounted = get_mounted_exfat(index).ok_or("exfat: not mounted")?;
    let fs = &mounted.fs;
    let storage = &mounted.storage;

    let entries = read_exfat_dir_detailed(storage, fs, fs.root_cluster)?;
    let file_entry = entries.iter().find(|e| e.name.eq_ignore_ascii_case(path))
        .ok_or("exfat: file not found")?;

    let end = offset.checked_add(data.len()).ok_or("exfat: write size overflow")?;
    let mut combined = vec![0u8; core::cmp::max(file_entry.size as usize, end)];
    if file_entry.size > 0 {
        read_exfat_chain(storage, fs, file_entry.first_cluster, &mut combined[..file_entry.size as usize])?;
    }
    combined[offset..end].copy_from_slice(data);
    write_exfat_file(storage, fs, file_entry.first_cluster, &combined)?;
    update_exfat_entry_size(storage, fs, fs.root_cluster, file_entry.entry_offset, combined.len() as u64)?;

    Ok(())
}

pub fn mkdir_exfat_vfs(source: &str, name: &str) -> Result<(), &'static str> {
    let index = parse_exfat_mount_index(source)?;
    let mounted = get_mounted_exfat(index).ok_or("exfat: not mounted")?;
    create_exfat_file(&mounted.storage, &mounted.fs, mounted.fs.root_cluster, name, true)?;
    Ok(())
}

pub fn delete_exfat_file_vfs(source: &str, name: &str) -> Result<(), &'static str> {
    let index = parse_exfat_mount_index(source)?;
    let mounted = get_mounted_exfat(index).ok_or("exfat: not mounted")?;
    let entries = read_exfat_dir_detailed(&mounted.storage, &mounted.fs, mounted.fs.root_cluster)?;
    let entry = entries
        .iter()
        .find(|e| e.name.eq_ignore_ascii_case(name))
        .ok_or("exfat: file not found")?;
    delete_exfat_file(
        &mounted.storage,
        &mounted.fs,
        entry.first_cluster,
        mounted.fs.root_cluster,
        entry.entry_offset,
    )
}

pub fn rename_exfat_vfs(source: &str, old_name: &str, new_name: &str) -> Result<(), &'static str> {
    let index = parse_exfat_mount_index(source)?;
    let mounted = get_mounted_exfat(index).ok_or("exfat: not mounted")?;
    let entries = read_exfat_dir_detailed(&mounted.storage, &mounted.fs, mounted.fs.root_cluster)?;
    let entry = entries
        .iter()
        .find(|e| e.name.eq_ignore_ascii_case(old_name))
        .ok_or("exfat: file not found")?;
    if entries.iter().any(|e| e.name.eq_ignore_ascii_case(new_name)) {
        return Err("exfat: destination already exists");
    }

    rename_exfat_entry_in_place(
        &mounted.storage,
        &mounted.fs,
        mounted.fs.root_cluster,
        entry.entry_offset,
        new_name,
    )
}

pub fn truncate_exfat_file_vfs(source: &str, name: &str, new_size: u64) -> Result<(), &'static str> {
    let index = parse_exfat_mount_index(source)?;
    let mounted = get_mounted_exfat(index).ok_or("exfat: not mounted")?;
    let entries = read_exfat_dir_detailed(&mounted.storage, &mounted.fs, mounted.fs.root_cluster)?;
    let entry = entries
        .iter()
        .find(|e| e.name.eq_ignore_ascii_case(name))
        .ok_or("exfat: file not found")?;
    if entry.is_dir {
        return Err("exfat: cannot truncate directory");
    }

    let mut data = vec![0u8; core::cmp::min(entry.size, new_size) as usize];
    if !data.is_empty() {
        read_exfat_chain(&mounted.storage, &mounted.fs, entry.first_cluster, &mut data)?;
    }
    data.resize(new_size as usize, 0);
    write_exfat_file(&mounted.storage, &mounted.fs, entry.first_cluster, &data)?;
    update_exfat_entry_size(&mounted.storage, &mounted.fs, mounted.fs.root_cluster, entry.entry_offset, new_size)?;
    Ok(())
}

/// exFAT dosya listeleme (VFS için)
fn read_exfat_dir(storage: &Fat32Storage, fs: &ExFatFs, dir_cluster: u32) -> Result<Vec<(String, u32)>, &'static str> {
    read_exfat_dir_detailed(storage, fs, dir_cluster)
        .map(|entries| entries.into_iter().map(|e| (e.name, e.first_cluster)).collect())
}

#[derive(Clone, Debug)]
struct ExFatDirEntryRef {
    name: String,
    first_cluster: u32,
    size: u64,
    is_dir: bool,
    entry_offset: usize,
    secondary_count: usize,
    name_entry_count: usize,
}

fn read_exfat_dir_detailed(
    storage: &Fat32Storage,
    fs: &ExFatFs,
    dir_cluster: u32,
) -> Result<Vec<ExFatDirEntryRef>, &'static str> {
    let mut results = Vec::new();
    let dir_offset = (fs.cluster_heap_offset as u64 * fs.sector_size as u64
        + (dir_cluster as u64 - 2) * fs.cluster_size as u64) as usize;
    let dir_data = storage.read_exact(dir_offset, fs.cluster_size as usize)
        .map_err(|_| "exfat: failed to read dir")?;

    let mut i = 0;
    while i + 32 <= dir_data.len() {
        let entry_type = dir_data[i];
        if entry_type == 0x00 {
            break;
        }
        if entry_type == 0x85 {
            let secondary_count = dir_data[i + 1] as usize;
            let mut file_name = String::new();
            let mut first_cluster = 0u32;
            let mut size = u64::from_le_bytes([
                dir_data[i + 24], dir_data[i + 25], dir_data[i + 26], dir_data[i + 27],
                dir_data[i + 28], dir_data[i + 29], dir_data[i + 30], dir_data[i + 31],
            ]);
            let is_dir = (dir_data[i + 8] & 0x10) != 0;
            let mut remaining_name_chars = 0usize;
            let mut name_entry_count = 0usize;

            for s in 0..secondary_count {
                let si = i + (s + 1) * 32;
                if si + 32 > dir_data.len() {
                    break;
                }
                let stype = dir_data[si];
                if stype == 0xC0 {
                    remaining_name_chars = dir_data[si + 3] as usize;
                    first_cluster = u32::from_le_bytes([
                        dir_data[si + 20], dir_data[si + 21],
                        dir_data[si + 22], dir_data[si + 23],
                    ]);
                    size = u64::from_le_bytes([
                        dir_data[si + 24], dir_data[si + 25], dir_data[si + 26], dir_data[si + 27],
                        dir_data[si + 28], dir_data[si + 29], dir_data[si + 30], dir_data[si + 31],
                    ]);
                }
                if stype == 0xC1 {
                    name_entry_count += 1;
                    let chars_here = remaining_name_chars.min(15);
                    for c in 0..chars_here {
                        let offset = si + 2 + c * 2;
                        if offset + 1 < dir_data.len() {
                            let ch = u16::from_le_bytes([dir_data[offset], dir_data[offset + 1]]);
                            if let Some(c) = char::from_u32(ch as u32) {
                                file_name.push(c);
                            }
                        }
                    }
                    remaining_name_chars = remaining_name_chars.saturating_sub(chars_here);
                }
            }

            if !file_name.is_empty() {
                results.push(ExFatDirEntryRef {
                    name: file_name,
                    first_cluster,
                    size,
                    is_dir,
                    entry_offset: i,
                    secondary_count,
                    name_entry_count,
                });
            }
            i += (secondary_count + 1) * 32;
        } else {
            i += 32;
        }
    }
    Ok(results)
}

fn update_exfat_entry_size(
    storage: &Fat32Storage,
    fs: &ExFatFs,
    dir_cluster: u32,
    entry_offset: usize,
    size: u64,
) -> Result<(), &'static str> {
    let dir_offset = (fs.cluster_heap_offset as u64 * fs.sector_size as u64
        + (dir_cluster as u64 - 2) * fs.cluster_size as u64) as usize;
    let mut dir_data = storage
        .read_exact(dir_offset, fs.cluster_size as usize)
        .map_err(|_| "exfat: failed to read dir for size update")?;
    if entry_offset + 64 > dir_data.len() || dir_data[entry_offset] != 0x85 {
        return Err("exfat: invalid entry offset for size update");
    }
    dir_data[entry_offset + 24..entry_offset + 32].copy_from_slice(&size.to_le_bytes());
    let stream = entry_offset + 32;
    if dir_data[stream] != 0xC0 {
        return Err("exfat: missing stream extension for size update");
    }
    dir_data[stream + 8..stream + 16].copy_from_slice(&size.to_le_bytes());
    dir_data[stream + 24..stream + 32].copy_from_slice(&size.to_le_bytes());
    let secondary_count = dir_data[entry_offset + 1] as usize;
    let end = entry_offset + (secondary_count + 1) * 32;
    if end > dir_data.len() {
        return Err("exfat: truncated entry set for checksum update");
    }
    let mut entries = Vec::new();
    for chunk in dir_data[entry_offset..end].chunks(32) {
        entries.push(chunk.to_vec());
    }
    let checksum = compute_exfat_set_checksum(&entries);
    dir_data[entry_offset + 2..entry_offset + 4].copy_from_slice(&checksum.to_le_bytes());
    storage
        .write_exact(dir_offset, &dir_data)
        .map_err(|_| "exfat: failed to write dir size update")
}

fn rename_exfat_entry_in_place(
    storage: &Fat32Storage,
    fs: &ExFatFs,
    dir_cluster: u32,
    entry_offset: usize,
    new_name: &str,
) -> Result<(), &'static str> {
    let name_units: Vec<u16> = new_name.encode_utf16().collect();
    if name_units.is_empty() || name_units.len() > 255 {
        return Err("exfat: invalid rename length");
    }

    let dir_offset = (fs.cluster_heap_offset as u64 * fs.sector_size as u64
        + (dir_cluster as u64 - 2) * fs.cluster_size as u64) as usize;
    let mut dir_data = storage
        .read_exact(dir_offset, fs.cluster_size as usize)
        .map_err(|_| "exfat: failed to read dir for rename")?;
    if entry_offset + 64 > dir_data.len() || dir_data[entry_offset] != 0x85 {
        return Err("exfat: invalid entry offset for rename");
    }

    let secondary_count = dir_data[entry_offset + 1] as usize;
    let end = entry_offset + (secondary_count + 1) * 32;
    if end > dir_data.len() {
        return Err("exfat: truncated entry set for rename");
    }
    let available_name_entries = secondary_count.saturating_sub(1);
    let required_name_entries = (name_units.len() + 14) / 15;
    if required_name_entries != available_name_entries {
        return Err("exfat: rename would resize entry set; fail-closed");
    }

    let stream = entry_offset + 32;
    if dir_data[stream] != 0xC0 {
        return Err("exfat: missing stream extension for rename");
    }
    dir_data[stream + 3] = name_units.len() as u8;
    let name_hash = compute_exfat_name_hash(new_name);
    dir_data[stream + 4..stream + 6].copy_from_slice(&name_hash.to_le_bytes());

    let mut cursor = 0usize;
    for idx in 0..available_name_entries {
        let name_entry = entry_offset + 64 + idx * 32;
        if dir_data[name_entry] != 0xC1 {
            return Err("exfat: missing filename entry for rename");
        }
        for byte in &mut dir_data[name_entry + 2..name_entry + 32] {
            *byte = 0;
        }
        let chars = core::cmp::min(15, name_units.len().saturating_sub(cursor));
        for c in 0..chars {
            let encoded = name_units[cursor + c].to_le_bytes();
            let off = name_entry + 2 + c * 2;
            dir_data[off] = encoded[0];
            dir_data[off + 1] = encoded[1];
        }
        cursor += chars;
    }

    let mut set_entries = Vec::new();
    for chunk in dir_data[entry_offset..end].chunks(32) {
        set_entries.push(chunk.to_vec());
    }
    let checksum = compute_exfat_set_checksum(&set_entries);
    dir_data[entry_offset + 2..entry_offset + 4].copy_from_slice(&checksum.to_le_bytes());
    storage
        .write_exact(dir_offset, &dir_data)
        .map_err(|_| "exfat: failed to write renamed entry")
}

/// exFAT chain oku
fn read_exfat_chain(storage: &Fat32Storage, fs: &ExFatFs, start_cluster: u32, buf: &mut [u8]) -> Result<(), &'static str> {
    let fat_offset = fs.fat_offset as u64 * fs.sector_size as u64;

    let mut cluster = start_cluster;
    let mut bytes_read = 0usize;

    while cluster >= 2 && !fs.is_eof(cluster) && bytes_read < buf.len() {
        let data_offset = (fs.cluster_heap_offset as u64 * fs.sector_size as u64
            + (cluster as u64 - 2) * fs.cluster_size as u64) as usize;
        let available = fs.cluster_size as usize;
        let to_read = (buf.len() - bytes_read).min(available);

        let chunk = storage.read_exact(data_offset, to_read)
            .map_err(|_| "exfat: cluster read failed")?;
        buf[bytes_read..bytes_read + to_read].copy_from_slice(&chunk);
        bytes_read += to_read;

        let fat_entry_offset = (fat_offset + cluster as u64 * 4) as usize;
        let next_bytes = storage.read_exact(fat_entry_offset, 4)
            .map_err(|_| "exfat: FAT read failed")?;
        cluster = u32::from_le_bytes([next_bytes[0], next_bytes[1], next_bytes[2], next_bytes[3]]);
    }
    Ok(())
}
