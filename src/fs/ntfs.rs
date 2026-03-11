//! # NTFS Dosya Sistemi (Salt Okunur Destek)
//!
//! New Technology File System - Yalnızca okuma modunda uygulama.
//! Windows'un yerel dosya sistemi olan NTFS'in temel yapılarını çözümler.
//!
//! ## NTFS Disk Yapısı (ASCII Diyagram)
//! ```text
//! NTFS Bölüm Düzeni:
//! ┌─────────────────────────────────────────────────────────────┐
//! │  Sektör 0       │  Önyükleme Sektörü - BPB + NTFS sihiri   │
//! ├─────────────────────────────────────────────────────────────┤
//! │  MFT (Ana Dosya Tablosu) - her dosya için 1024 baytlık giriş│
//! │   MFT #0  = $MFT (MFT'nin kendisi)                         │
//! │   MFT #1  = $MFTMirr (MFT aynası - yedek)                  │
//! │   MFT #2  = $LogFile (işlem günlüğü)                       │
//! │   MFT #3  = $Volume (birim bilgisi)                         │
//! │   MFT #4  = $AttrDef (öznitelik tanımları)                  │
//! │   MFT #5  = . (kök dizin)                                   │
//! │   MFT #6  = $Bitmap (disk bitmap'i)                         │
//! │   MFT #7  = $Boot (önyükleme dosyası)                       │
//! │   MFT #8+ = Kullanıcı dosyaları                             │
//! ├─────────────────────────────────────────────────────────────┤
//! │  Veri Bölgesi - kümeler, öznitelik tipleri ile erişilir     │
//! └─────────────────────────────────────────────────────────────┘
//!
//! MFT Girişi Öznitelik Tipleri:
//!   0x10 = $STANDARD_INFORMATION (zaman, bayraklar)
//!   0x30 = $FILE_NAME (UTF-16LE dosya adı, üst dizin)
//!   0x80 = $DATA (dosya içeriği - yerleşik veya data run'larla)
//!   0x90 = $INDEX_ROOT (dizin b-ağacı kökü)
//! ```

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use spin::Mutex;

// ============================================================================
// NTFS SABİTLERİ
// ============================================================================

/// NTFS sihiri: "NTFS    " - önyükleme sektörünü doğrular
const NTFS_MAGIC: [u8; 8] = [0x4E, 0x54, 0x46, 0x53, 0x20, 0x20, 0x20, 0x20];

/// Sektör boyutu (genellikle 512 bayt)
const SECTOR_SIZE: u64 = 512;

/// MFT girişi boyutu (genellikle 1024 bayt)
const MFT_ENTRY_SIZE: u64 = 1024;

/// Öznitelik türleri - MFT girdisindeki öznitelikleri tanımlar
const ATTR_STANDARD_INFORMATION: u32 = 0x10;
const ATTR_ATTRIBUTE_LIST: u32 = 0x20;
const ATTR_FILE_NAME: u32 = 0x30;
const ATTR_OBJECT_ID: u32 = 0x40;
const ATTR_SECURITY_DESCRIPTOR: u32 = 0x50;
const ATTR_VOLUME_NAME: u32 = 0x60;
const ATTR_VOLUME_INFORMATION: u32 = 0x70;
const ATTR_DATA: u32 = 0x80;
const ATTR_INDEX_ROOT: u32 = 0x90;
const ATTR_INDEX_ALLOCATION: u32 = 0xA0;
const ATTR_BITMAP: u32 = 0xB0;
const ATTR_REPARSE_POINT: u32 = 0xC0;
const ATTR_EA_INFORMATION: u32 = 0xD0;
const ATTR_EA: u32 = 0xE0;
const ATTR_LOGGED_UTILITY_STREAM: u32 = 0x100;

/// Sistem MFT giriş numaraları - rezerve edilmiş sistem dosyaları
const MFT_MFT: u64 = 0;
const MFT_MFTMIRR: u64 = 1;
const MFT_LOGFILE: u64 = 2;
const MFT_VOLUME: u64 = 3;
const MFT_ATTRDEF: u64 = 4;
const MFT_ROOTDIR: u64 = 5;
const MFT_BITMAP: u64 = 6;
const MFT_BOOT: u64 = 7;

/// Dosya adı ad alanları - aynı dosyanın farklı ad biçimleri
const FILE_NAME_POSIX: u8 = 0;
const FILE_NAME_WIN32: u8 = 1;
const FILE_NAME_DOS: u8 = 2;
const FILE_NAME_WIN32_DOS: u8 = 3;

// ============================================================================
// NTFS HATASI
// ============================================================================

/// NTFS işlemlerinde oluşabilecek hata türleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NtfsError {
    InvalidFormat,
    ReadError,
    NotFound,
    NotSupported,
    Corrupted,
    OutOfMemory,
}

// ============================================================================
// DOSYA TÜRLERİ
// ============================================================================

/// NTFS dosya türü numaralandırması
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NtfsFileType {
    File,
    Directory,
    System,
    Hidden,
    Archive,
    Compressed,
    Encrypted,
    Sparse,
    Unknown,
}

/// NTFS dizin girdisi - dosya adı, inode numarası, tür ve boyut bilgisi
#[derive(Clone, Debug)]
pub struct NtfsDirEntry {
    pub name: String,
    pub inode: u64,
    pub file_type: NtfsFileType,
    pub size: u64,
}

/// NTFS dosya meta verisi - boyut, zaman damgaları ve bayraklar
#[derive(Clone, Debug)]
pub struct NtfsMetadata {
    pub size: u64,
    pub allocated_size: u64,
    pub file_type: NtfsFileType,
    pub flags: u32,
    pub created: u64,
    pub modified: u64,
    pub accessed: u64,
}

// ============================================================================
// ÖNYÜKLEME SEKTÖRÜ
// ============================================================================

/// NTFS Önyükleme Sektörü - BPB parametreleri ve MFT konumu
#[derive(Clone, Debug)]
pub struct NtfsBootSector {
    pub oem_id: [u8; 8],
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub total_sectors: u64,
    pub mft_cluster: u64,
    pub mftmirr_cluster: u64,
    pub clusters_per_mft_record: i8,
    pub clusters_per_index_buffer: i8,
    pub serial_number: u64,
}

impl NtfsBootSector {
    /// Önyükleme sektörünü ham baytlardan çözümler ve NTFS sihirini doğrular
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 512 {
            return None;
        }

        // Sihiri kontrol et
        let oem_id: [u8; 8] = [
            data[3], data[4], data[5], data[6], data[7], data[8], data[9], data[10],
        ];
        if oem_id != NTFS_MAGIC {
            return None;
        }

        let bytes_per_sector = u16::from_le_bytes([data[11], data[12]]);
        let sectors_per_cluster = data[13];

        // Toplam sektör sayısı (64 bit, ofset 40)
        let total_sectors = u64::from_le_bytes([
            data[40], data[41], data[42], data[43], data[44], data[45], data[46], data[47],
        ]);

        // MFT kümesi (64 bit, ofset 48)
        let mft_cluster = u64::from_le_bytes([
            data[48], data[49], data[50], data[51], data[52], data[53], data[54], data[55],
        ]);

        // MFT aynası kümesi (64 bit, ofset 56)
        let mftmirr_cluster = u64::from_le_bytes([
            data[56], data[57], data[58], data[59], data[60], data[61], data[62], data[63],
        ]);

        let clusters_per_mft_record = data[64] as i8;
        let clusters_per_index_buffer = data[68] as i8;

        // Seri numarası (64 bit, ofset 72)
        let serial_number = u64::from_le_bytes([
            data[72], data[73], data[74], data[75], data[76], data[77], data[78], data[79],
        ]);

        Some(NtfsBootSector {
            oem_id,
            bytes_per_sector,
            sectors_per_cluster,
            total_sectors,
            mft_cluster,
            mftmirr_cluster,
            clusters_per_mft_record,
            clusters_per_index_buffer,
            serial_number,
        })
    }

    /// Küme boyutunu bayt olarak hesaplar
    pub fn cluster_size(&self) -> u64 {
        self.bytes_per_sector as u64 * self.sectors_per_cluster as u64
    }

    /// MFT giriş boyutunu hesaplar (negatif değer 2^|değer| anlamına gelir)
    pub fn mft_entry_size(&self) -> u64 {
        if self.clusters_per_mft_record > 0 {
            self.cluster_size() * self.clusters_per_mft_record as u64
        } else {
            // Negatif değer: 2^|değer| bayt
            1u64 << (-self.clusters_per_mft_record as u64)
        }
    }

    /// MFT'nin disk üzerindeki bayt ofsetini döndürür
    pub fn mft_offset(&self) -> u64 {
        self.mft_cluster * self.cluster_size()
    }
}

// ============================================================================
// MFT GİRİŞİ
// ============================================================================

/// MFT Girişi - bir dosya veya dizin için tüm meta veriyi öznitelik listesi olarak tutar
#[derive(Clone, Debug)]
pub struct MftEntry {
    pub signature: [u8; 4],
    pub sequence: u16,
    pub link_count: u16,
    pub attributes: Vec<NtfsAttribute>,
    pub entry_number: u64,
}

impl MftEntry {
    const SIGNATURE_FILE: [u8; 4] = [b'F', b'I', b'L', b'E'];
    const SIGNATURE_BAAD: [u8; 4] = [b'B', b'A', b'A', b'D'];
    const SIGNATURE_HOLE: [u8; 4] = [b'H', b'O', b'L', b'E'];
    const SIGNATURE_CHKD: [u8; 4] = [b'C', b'H', b'K', b'D'];

    /// MFT girişini ham baytlardan çözümler; imzayı ve öznitelikleri ayrıştırır
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 48 {
            return None;
        }

        let signature: [u8; 4] = [data[0], data[1], data[2], data[3]];

        // Geçerli giriş kontrolü
        if signature != Self::SIGNATURE_FILE
            && signature != Self::SIGNATURE_BAAD
            && signature != Self::SIGNATURE_HOLE
        {
            return None;
        }

        let sequence = u16::from_le_bytes([data[16], data[17]]);
        let link_count = u16::from_le_bytes([data[18], data[19]]);

        // Öznitelikleri çözümle
        let mut attributes = Vec::new();
        let mut offset = u16::from_le_bytes([data[20], data[21]]) as usize;

        while offset + 16 <= data.len() {
            let attr = NtfsAttribute::parse(&data[offset..])?;

            if attr.attr_type == 0xFFFFFFFF {
                break;
            }

            let attr_len = attr.total_length as usize;
            if attr_len == 0 {
                break;
            }

            attributes.push(attr);
            offset += attr_len;
        }

        Some(MftEntry {
            signature,
            sequence,
            link_count,
            attributes,
            entry_number: 0,
        })
    }

    /// Girişin geçerli bir FILE girişi olup olmadığını kontrol eder
    pub fn is_valid(&self) -> bool {
        self.signature == Self::SIGNATURE_FILE
    }

    /// Belirtilen türdeki özniteliği döndürür
    pub fn get_attribute(&self, attr_type: u32) -> Option<&NtfsAttribute> {
        self.attributes.iter().find(|a| a.attr_type == attr_type)
    }

    /// $DATA özniteliğini döndürür (dosya içeriği)
    pub fn get_data_attribute(&self) -> Option<&NtfsAttribute> {
        self.get_attribute(ATTR_DATA)
    }

    /// $FILE_NAME özniteliğini döndürür (dosya adı ve üst dizin)
    pub fn get_filename_attribute(&self) -> Option<&NtfsAttribute> {
        self.get_attribute(ATTR_FILE_NAME)
    }
}

// ============================================================================
// NTFS ÖZNİTELİĞİ
// ============================================================================

/// NTFS Özniteliği - yerleşik (resident) veya dışsal (non-resident) veri içerir
#[derive(Clone, Debug)]
pub struct NtfsAttribute {
    pub attr_type: u32,
    pub total_length: u32,
    pub non_resident: bool,
    pub name_length: u8,
    pub name_offset: u16,
    pub flags: u16,
    pub instance: u16,
    pub content: AttributeContent,
}

/// Öznitelik içeriği - yerleşik veri veya dışsal data run'ları
#[derive(Clone, Debug)]
pub enum AttributeContent {
    Resident {
        data_offset: u16,
        data_length: u32,
    },
    NonResident {
        start_vcn: u64,
        last_vcn: u64,
        data_runs_offset: u16,
        data_runs: Vec<DataRun>,
    },
}

impl NtfsAttribute {
    /// Özniteliği ham baytlardan çözümler; yerleşik veya dışsal içeriği ayrıştırır
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 16 {
            return None;
        }

        let attr_type = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);

        // Son işaretçi
        if attr_type == 0xFFFFFFFF {
            return Some(NtfsAttribute {
                attr_type,
                total_length: 0,
                non_resident: false,
                name_length: 0,
                name_offset: 0,
                flags: 0,
                instance: 0,
                content: AttributeContent::Resident {
                    data_offset: 0,
                    data_length: 0,
                },
            });
        }

        let total_length = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let non_resident = data[8] != 0;
        let name_length = data[9];
        let name_offset = u16::from_le_bytes([data[10], data[11]]);
        let flags = u16::from_le_bytes([data[12], data[13]]);
        let instance = u16::from_le_bytes([data[14], data[15]]);

        let content = if non_resident {
            // Dışsal öznitelik - data run'larla blok eşlemesi
            let start_vcn = u64::from_le_bytes([
                data[16], data[17], data[18], data[19], data[20], data[21], data[22], data[23],
            ]);
            let last_vcn = u64::from_le_bytes([
                data[24], data[25], data[26], data[27], data[28], data[29], data[30], data[31],
            ]);
            let data_runs_offset = u16::from_le_bytes([data[32], data[33]]);

            // Data run'larını çözümle
            let data_runs = Self::parse_data_runs(&data[data_runs_offset as usize..])?;

            AttributeContent::NonResident {
                start_vcn,
                last_vcn,
                data_runs_offset,
                data_runs,
            }
        } else {
            // Yerleşik öznitelik - doğrudan MFT girişi içinde
            let data_length = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
            let data_offset = u16::from_le_bytes([data[20], data[21]]);

            AttributeContent::Resident {
                data_offset,
                data_length,
            }
        };

        Some(NtfsAttribute {
            attr_type,
            total_length,
            non_resident,
            name_length,
            name_offset,
            flags,
            instance,
            content,
        })
    }

    /// Data run'larını çözümler (LCN eşlemeleri, sıkıştırılmış format)
    fn parse_data_runs(data: &[u8]) -> Option<Vec<DataRun>> {
        let mut runs = Vec::new();
        let mut offset = 0;

        loop {
            if offset >= data.len() {
                break;
            }

            let header = data[offset];
            if header == 0 {
                break;
            }

            let len_bytes = (header & 0x0F) as usize;
            let lcn_bytes = (header >> 4) as usize;

            offset += 1;

            if offset + len_bytes + lcn_bytes > data.len() {
                break;
            }

            // Uzunluğu çözümle
            let mut length = 0u64;
            for i in 0..len_bytes {
                length |= (data[offset + i] as u64) << (i * 8);
            }
            offset += len_bytes;

            // LCN'yi çözümle (işaretli - önceki değere göreli)
            let mut lcn = 0i64;
            for i in 0..lcn_bytes {
                lcn |= (data[offset + i] as i64) << (i * 8);
            }
            // İşaret uzatma
            if lcn_bytes > 0 && (data[offset + lcn_bytes - 1] & 0x80) != 0 {
                lcn |= !0 << (lcn_bytes * 8);
            }
            offset += lcn_bytes;

            runs.push(DataRun { length, lcn });
        }

        Some(runs)
    }

    /// Yerleşik özniteliğin ham verisini döndürür
    pub fn get_resident_data<'a>(&self, entry_data: &'a [u8]) -> Option<&'a [u8]> {
        if let AttributeContent::Resident {
            data_offset,
            data_length,
        } = self.content
        {
            let offset = data_offset as usize;
            let length = data_length as usize;
            if offset + length <= entry_data.len() {
                Some(&entry_data[offset..offset + length])
            } else {
                None
            }
        } else {
            None
        }
    }
}

/// Data run - LCN eşlemesi; dışsal özniteliklerin disk konumunu belirtir
#[derive(Clone, Copy, Debug)]
pub struct DataRun {
    pub length: u64,
    pub lcn: i64, // İşaretli, önceki değere göreli
}

// ============================================================================
// DOSYA ADI ÖZNİTELİĞİ
// ============================================================================

/// $FILE_NAME öznitelik içeriği - dosya adı, üst dizin ve zaman damgaları
#[derive(Clone, Debug)]
pub struct FileNameAttr {
    pub parent_directory: u64,
    pub created: u64,
    pub modified: u64,
    pub accessed: u64,
    pub file_size: u64,
    pub flags: u32,
    pub name: String,
}

impl FileNameAttr {
    /// $FILE_NAME özniteliğini ham baytlardan çözümler (UTF-16LE adı dönüştürür)
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 66 {
            return None;
        }

        let parent_directory = u64::from_le_bytes([
            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
        ]);

        let created = u64::from_le_bytes([
            data[8], data[9], data[10], data[11], data[12], data[13], data[14], data[15],
        ]);

        let modified = u64::from_le_bytes([
            data[16], data[17], data[18], data[19], data[20], data[21], data[22], data[23],
        ]);

        let accessed = u64::from_le_bytes([
            data[24], data[25], data[26], data[27], data[28], data[29], data[30], data[31],
        ]);

        let file_size = u64::from_le_bytes([
            data[56], data[57], data[58], data[59], data[60], data[61], data[62], data[63],
        ]);

        let flags = u32::from_le_bytes([data[52], data[53], data[54], data[55]]);
        let name_length = data[64] as usize;
        let namespace = data[65];

        // Ad UTF-16LE kodlamalıdır
        let name_offset = 66;
        if name_offset + name_length * 2 > data.len() {
            return None;
        }

        let mut name = String::new();
        for i in 0..name_length {
            let char_offset = name_offset + i * 2;
            let c = u16::from_le_bytes([data[char_offset], data[char_offset + 1]]);
            if c == 0 {
                break;
            }
            if let Some(c) = char::from_u32(c as u32) {
                name.push(c);
            }
        }

        Some(FileNameAttr {
            parent_directory,
            created,
            modified,
            accessed,
            file_size,
            flags,
            name,
        })
    }

    /// Dosyanın dizin olup olmadığını kontrol eder
    pub fn is_directory(&self) -> bool {
        (self.flags & 0x10000000) != 0
    }
}

// ============================================================================
// NTFS DOSYA SİSTEMİ
// ============================================================================

/// NTFS Dosya Sistemi örneği - bölüm parametrelerini ve MFT konumunu yönetir
#[derive(Clone, Debug)]
pub struct NtfsFileSystem {
    pub boot_sector: NtfsBootSector,
    pub cluster_size: u64,
    pub mft_offset: u64,
    pub mft_entry_size: u64,
}

impl NtfsFileSystem {
    /// Varsayılan değerlerle yeni bir NTFS dosya sistemi örneği oluşturur
    pub fn new() -> Self {
        NtfsFileSystem {
            boot_sector: unsafe { core::mem::zeroed() },
            cluster_size: 4096,
            mft_offset: 0,
            mft_entry_size: 1024,
        }
    }

    /// Aygıt verisinden NTFS'yi başlatır: önyükleme sektörünü okur ve MFT konumunu hesaplar
    pub fn init(&mut self, device_data: &[u8]) -> Result<(), NtfsError> {
        if device_data.len() < 512 {
            return Err(NtfsError::ReadError);
        }

        let boot = NtfsBootSector::parse(&device_data[0..512]).ok_or(NtfsError::InvalidFormat)?;

        let total_sectors = boot.total_sectors;
        let cluster_size = boot.cluster_size();
        let mft_offset = boot.mft_offset();
        let mft_entry_size = boot.mft_entry_size();

        self.boot_sector = boot;
        self.cluster_size = cluster_size;
        self.mft_offset = mft_offset;
        self.mft_entry_size = mft_entry_size;

        crate::serial_println!(
            "[NTFS] Başlatıldı: {} sektör, {} bayt/küme, MFT'nin konumu {}",
            total_sectors,
            cluster_size,
            mft_offset
        );

        Ok(())
    }

    /// Belirtilen numaralı MFT girişini aygıt verisinden okur
    pub fn read_mft_entry(
        &self,
        entry_num: u64,
        device_data: &[u8],
    ) -> Result<MftEntry, NtfsError> {
        let offset = self.mft_offset + entry_num * self.mft_entry_size;
        let offset = offset as usize;

        if offset + self.mft_entry_size as usize > device_data.len() {
            return Err(NtfsError::ReadError);
        }

        let entry_data = &device_data[offset..offset + self.mft_entry_size as usize];
        let mut entry = MftEntry::parse(entry_data).ok_or(NtfsError::Corrupted)?;
        entry.entry_number = entry_num;

        Ok(entry)
    }

    /// MFT girişindeki $DATA özniteliğinden dosya içeriğini okur
    pub fn read_file(&self, entry: &MftEntry, device_data: &[u8]) -> Result<Vec<u8>, NtfsError> {
        let data_attr = entry.get_data_attribute().ok_or(NtfsError::NotFound)?;

        match &data_attr.content {
            AttributeContent::Resident {
                data_offset,
                data_length,
            } => {
                let offset = *data_offset as usize;
                let length = *data_length as usize;
                let entry_offset = self.mft_offset as usize
                    + entry.entry_number as usize * self.mft_entry_size as usize;

                if entry_offset + offset + length <= device_data.len() {
                    Ok(device_data[entry_offset + offset..entry_offset + offset + length].to_vec())
                } else {
                    Err(NtfsError::ReadError)
                }
            }
            AttributeContent::NonResident { data_runs, .. } => {
                let mut data = Vec::new();
                let mut current_lcn: i64 = 0;

                for run in data_runs {
                    current_lcn += run.lcn;

                    if current_lcn < 0 {
                        // Seyrek çalıştır (sparse run) - sıfırlarla doldur
                        data.extend(
                            core::iter::repeat(0u8).take((run.length * self.cluster_size) as usize),
                        );
                        continue;
                    }

                    let offset = current_lcn as u64 * self.cluster_size;
                    let length = (run.length * self.cluster_size) as usize;

                    if offset as usize + length <= device_data.len() {
                        data.extend_from_slice(
                            &device_data[offset as usize..offset as usize + length],
                        );
                    }
                }

                Ok(data)
            }
        }
    }

    /// MFT girişinden dosya adını alır
    pub fn get_file_name(&self, entry: &MftEntry) -> Option<String> {
        let attr = entry.get_filename_attribute()?;
        let data = attr.get_resident_data(&[0u8; 0])?;

        // Asıl giriş verisinden alınması gerekir
        None // Şimdilik basitleştirildi
    }

    /// Kök dizin MFT girişini okur ve dizin içeriğini döndürür
    pub fn read_root_dir(&self, device_data: &[u8]) -> Result<Vec<NtfsDirEntry>, NtfsError> {
        let root_entry = self.read_mft_entry(MFT_ROOTDIR, device_data)?;

        // Kök dizin INDEX_ROOT ve INDEX_ALLOCATION kullanır
        // Basitleştirildi: şimdilik boş döndür
        Ok(Vec::new())
    }

    /// Verilen MFT girişinin meta verisini döndürür
    pub fn get_metadata(&self, entry: &MftEntry) -> Option<NtfsMetadata> {
        let filename_attr = entry.get_filename_attribute()?;
        // Dosya adı özniteliğinden meta veri oluştur
        None // Basitleştirildi
    }
}

impl Default for NtfsFileSystem {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// GLOBAL ÖRNEK
// ============================================================================

lazy_static::lazy_static! {
    static ref NTFS_INSTANCES: Mutex<BTreeMap<String, NtfsFileSystem>> = Mutex::new(BTreeMap::new());
}

/// NTFS dosya sistemini bağlar (mount)
pub fn mount_ntfs(name: &str, device_data: &[u8]) -> Result<(), NtfsError> {
    let mut fs = NtfsFileSystem::new();
    fs.init(device_data)?;

    NTFS_INSTANCES.lock().insert(name.to_string(), fs);
    Ok(())
}

/// İsme göre NTFS dosya sistemi örneğini döndürür
pub fn get_ntfs(name: &str) -> Option<NtfsFileSystem> {
    NTFS_INSTANCES.lock().get(name).cloned()
}

/// NTFS dosya sistemini ayırır (unmount)
pub fn unmount_ntfs(name: &str) -> bool {
    NTFS_INSTANCES.lock().remove(name).is_some()
}

/// NTFS modülünü başlatır
pub fn init() {
    crate::serial_println!("[NTFS] Modül başlatıldı");
}
