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
use alloc::sync::Arc;
use alloc::vec;
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

const INDEX_ENTRY_NODE: u8 = 0x01;
const INDEX_ENTRY_END: u8 = 0x02;

/// $INDEX_ROOT header — resident attribute içinde
#[derive(Clone, Copy, Debug)]
pub struct IndexRootHeader {
    pub attr_type: u32,
    pub collation_rule: u32,
    pub bytes_per_index_record: u32,
    pub clusters_per_index_record: u8,
}

impl IndexRootHeader {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 16 {
            return None;
        }
        Some(IndexRootHeader {
            attr_type: u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
            collation_rule: u32::from_le_bytes([data[4], data[5], data[6], data[7]]),
            bytes_per_index_record: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
            clusters_per_index_record: data[12],
        })
    }
}

/// Index node header — hem $INDEX_ROOT hem de index record'larda
#[derive(Clone, Copy, Debug)]
pub struct IndexNodeHeader {
    pub entries_offset: u32,
    pub entries_length: u32,
    pub allocated_size: u32,
    pub has_children: bool,
}

impl IndexNodeHeader {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 16 {
            return None;
        }
        Some(IndexNodeHeader {
            entries_offset: u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
            entries_length: u32::from_le_bytes([data[4], data[5], data[6], data[7]]),
            allocated_size: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
            has_children: data[12] != 0,
        })
    }
}

/// Index entry — $INDEX_ROOT veya index record içinde
#[derive(Clone, Debug)]
pub struct IndexEntry {
    pub file_reference: u64,
    pub entry_length: u16,
    pub stream_length: u16,
    pub flags: u8,
    pub filename_attr: Option<FileNameAttr>,
    pub sub_node_vcn: Option<u64>,
}

impl IndexEntry {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 16 {
            return None;
        }
        let file_reference = u64::from_le_bytes([
            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
        ]);
        let entry_length = u16::from_le_bytes([data[8], data[9]]);
        let stream_length = u16::from_le_bytes([data[10], data[11]]);
        let flags = data[12];

        if entry_length == 0 || entry_length as usize > data.len() {
            return None;
        }

        // $FILE_NAME stream parse (eğer END flag yoksa)
        let filename_attr = if (flags & INDEX_ENTRY_END) == 0 && stream_length >= 66 {
            FileNameAttr::parse(&data[16..])
        } else {
            None
        };

        // Sub-node VCN (eğer NODE flag varsa, entry sonundaki 8 byte)
        let sub_node_vcn = if (flags & INDEX_ENTRY_NODE) != 0 {
            let vcn_offset = entry_length as usize - 8;
            if vcn_offset + 8 <= data.len() && vcn_offset >= 16 {
                Some(u64::from_le_bytes([
                    data[vcn_offset],
                    data[vcn_offset + 1],
                    data[vcn_offset + 2],
                    data[vcn_offset + 3],
                    data[vcn_offset + 4],
                    data[vcn_offset + 5],
                    data[vcn_offset + 6],
                    data[vcn_offset + 7],
                ]))
            } else {
                None
            }
        } else {
            None
        };

        Some(IndexEntry {
            file_reference,
            entry_length,
            stream_length,
            flags,
            filename_attr,
            sub_node_vcn,
        })
    }

    pub fn is_end(&self) -> bool {
        (self.flags & INDEX_ENTRY_END) != 0
    }

    pub fn has_sub_node(&self) -> bool {
        (self.flags & INDEX_ENTRY_NODE) != 0
    }
}

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
    NoSpace,
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
    resident_data: Vec<u8>,
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
                resident_data: Vec::new(),
            });
        }

        let total_length = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let non_resident = data[8] != 0;
        let name_length = data[9];
        let name_offset = u16::from_le_bytes([data[10], data[11]]);
        let flags = u16::from_le_bytes([data[12], data[13]]);
        let instance = u16::from_le_bytes([data[14], data[15]]);

        let (content, resident_data) = if non_resident {
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

            (
                AttributeContent::NonResident {
                    start_vcn,
                    last_vcn,
                    data_runs_offset,
                    data_runs,
                },
                Vec::new(),
            )
        } else {
            // Yerleşik öznitelik - doğrudan MFT girişi içinde
            let data_length = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
            let data_offset = u16::from_le_bytes([data[20], data[21]]);
            let resident_end = data_offset as usize + data_length as usize;
            if resident_end > data.len() {
                return None;
            }

            (
                AttributeContent::Resident {
                    data_offset,
                    data_length,
                },
                data[data_offset as usize..resident_end].to_vec(),
            )
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
            resident_data,
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
    pub fn get_resident_data(&self) -> Option<&[u8]> {
        if self.non_resident {
            None
        } else {
            Some(&self.resident_data)
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

    pub fn init_from_storage(&mut self, storage: &NtfsStorage) -> Result<(), NtfsError> {
        let boot_bytes = storage.read_exact(0, 512)?;
        let boot = NtfsBootSector::parse(boot_bytes.as_slice()).ok_or(NtfsError::InvalidFormat)?;

        let total_sectors = boot.total_sectors;
        let cluster_size = boot.cluster_size();
        let mft_offset = boot.mft_offset();
        let mft_entry_size = boot.mft_entry_size();

        self.boot_sector = boot;
        self.cluster_size = cluster_size;
        self.mft_offset = mft_offset;
        self.mft_entry_size = mft_entry_size;

        crate::serial_println!(
            "[NTFS] BaÅŸlatÄ±ldÄ±: {} sektÃ¶r, {} bayt/kÃ¼me, MFT'nin konumu {}",
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

    pub fn read_mft_entry_from_storage(
        &self,
        entry_num: u64,
        storage: &NtfsStorage,
    ) -> Result<MftEntry, NtfsError> {
        let offset = self.mft_offset + entry_num * self.mft_entry_size;
        let entry_bytes = storage.read_exact(offset as usize, self.mft_entry_size as usize)?;
        let mut entry = MftEntry::parse(entry_bytes.as_slice()).ok_or(NtfsError::Corrupted)?;
        entry.entry_number = entry_num;
        Ok(entry)
    }

    /// MFT girişindeki $DATA özniteliğinden dosya içeriğini okur
    pub fn read_file(&self, entry: &MftEntry, device_data: &[u8]) -> Result<Vec<u8>, NtfsError> {
        let data_attr = entry.get_data_attribute().ok_or(NtfsError::NotFound)?;

        match &data_attr.content {
            AttributeContent::Resident { .. } => data_attr
                .get_resident_data()
                .map(|data| data.to_vec())
                .ok_or(NtfsError::ReadError),
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

    pub fn read_file_from_storage(
        &self,
        entry: &MftEntry,
        storage: &NtfsStorage,
    ) -> Result<Vec<u8>, NtfsError> {
        let data_attr = entry.get_data_attribute().ok_or(NtfsError::NotFound)?;

        match &data_attr.content {
            AttributeContent::Resident { .. } => data_attr
                .get_resident_data()
                .map(|data| data.to_vec())
                .ok_or(NtfsError::ReadError),
            AttributeContent::NonResident { data_runs, .. } => {
                let mut data = Vec::new();
                let mut current_lcn: i64 = 0;

                for run in data_runs {
                    current_lcn += run.lcn;

                    if current_lcn < 0 {
                        data.extend(
                            core::iter::repeat(0u8).take((run.length * self.cluster_size) as usize),
                        );
                        continue;
                    }

                    let offset = current_lcn as u64 * self.cluster_size;
                    let length = (run.length * self.cluster_size) as usize;
                    let chunk = storage.read_exact(offset as usize, length)?;
                    data.extend_from_slice(chunk.as_slice());
                }

                Ok(data)
            }
        }
    }

    /// MFT girişinden dosya adını alır
    pub fn get_file_name(&self, entry: &MftEntry) -> Option<String> {
        let attr = entry.get_filename_attribute()?;
        let data = attr.get_resident_data()?;
        FileNameAttr::parse(data).map(|info| info.name)
    }

    /// Kök dizin MFT girişini okur ve dizin içeriğini döndürür
    pub fn read_root_dir(&self, device_data: &[u8]) -> Result<Vec<NtfsDirEntry>, NtfsError> {
        self.list_directory(MFT_ROOTDIR, device_data)
    }

    pub fn read_root_dir_from_storage(
        &self,
        storage: &NtfsStorage,
    ) -> Result<Vec<NtfsDirEntry>, NtfsError> {
        self.list_directory_from_storage(MFT_ROOTDIR, storage)
    }

    /// Verilen MFT girişinin meta verisini döndürür
    pub fn get_metadata(&self, entry: &MftEntry) -> Option<NtfsMetadata> {
        let filename_attr = self.read_filename_attr(entry)?;
        let file_type = file_type_from_filename_attr(&filename_attr);
        let allocated_size = match entry.get_data_attribute() {
            Some(data_attr) => match data_attr.content {
                AttributeContent::Resident { data_length, .. } => data_length as u64,
                AttributeContent::NonResident {
                    last_vcn,
                    start_vcn,
                    ..
                } => last_vcn.saturating_sub(start_vcn).saturating_add(1) * self.cluster_size,
            },
            None if matches!(file_type, NtfsFileType::Directory) => 0,
            None => return None,
        };
        Some(NtfsMetadata {
            size: filename_attr.file_size,
            allocated_size,
            file_type,
            flags: filename_attr.flags,
            created: filename_attr.created,
            modified: filename_attr.modified,
            accessed: filename_attr.accessed,
        })
    }

    pub fn resolve_path(&self, path: &str, device_data: &[u8]) -> Result<MftEntry, NtfsError> {
        if path.trim_matches('/').is_empty() {
            return self.read_mft_entry(MFT_ROOTDIR, device_data);
        }

        let mut current = MFT_ROOTDIR;
        for component in path
            .trim_matches('/')
            .split('/')
            .filter(|part| !part.is_empty() && *part != ".")
        {
            let child = self.find_child_entry(current, component, device_data)?;
            current = child.entry_number;
        }
        self.read_mft_entry(current, device_data)
    }

    pub fn resolve_path_from_storage(
        &self,
        path: &str,
        storage: &NtfsStorage,
    ) -> Result<MftEntry, NtfsError> {
        if path.trim_matches('/').is_empty() {
            return self.read_mft_entry_from_storage(MFT_ROOTDIR, storage);
        }

        let mut current = MFT_ROOTDIR;
        for component in path
            .trim_matches('/')
            .split('/')
            .filter(|part| !part.is_empty() && *part != ".")
        {
            let child = self.find_child_entry_from_storage(current, component, storage)?;
            current = child.entry_number;
        }
        self.read_mft_entry_from_storage(current, storage)
    }

    pub fn list_directory(
        &self,
        parent_entry: u64,
        device_data: &[u8],
    ) -> Result<Vec<NtfsDirEntry>, NtfsError> {
        let mut result = Vec::new();
        for entry_num in 0..self.max_mft_entries(device_data) {
            if entry_num == parent_entry {
                continue;
            }
            let Ok(entry) = self.read_mft_entry(entry_num, device_data) else {
                continue;
            };
            if !entry.is_valid() {
                continue;
            }
            let Some(filename_attr) = self.read_filename_attr(&entry) else {
                continue;
            };
            if normalize_file_reference(filename_attr.parent_directory) != parent_entry {
                continue;
            }
            let metadata = self.get_metadata(&entry);
            result.push(NtfsDirEntry {
                name: filename_attr.name,
                inode: entry.entry_number,
                file_type: metadata
                    .as_ref()
                    .map(|meta| meta.file_type)
                    .unwrap_or(NtfsFileType::Unknown),
                size: metadata.map(|meta| meta.size).unwrap_or(0),
            });
        }
        Ok(result)
    }

    pub fn list_directory_from_storage(
        &self,
        parent_entry: u64,
        storage: &NtfsStorage,
    ) -> Result<Vec<NtfsDirEntry>, NtfsError> {
        let mut result = Vec::new();
        for entry_num in 0..self.max_mft_entries_from_storage(storage)? {
            if entry_num == parent_entry {
                continue;
            }
            let Ok(entry) = self.read_mft_entry_from_storage(entry_num, storage) else {
                continue;
            };
            if !entry.is_valid() {
                continue;
            }
            let Some(filename_attr) = self.read_filename_attr(&entry) else {
                continue;
            };
            if normalize_file_reference(filename_attr.parent_directory) != parent_entry {
                continue;
            }
            let metadata = self.get_metadata(&entry);
            result.push(NtfsDirEntry {
                name: filename_attr.name,
                inode: entry.entry_number,
                file_type: metadata
                    .as_ref()
                    .map(|meta| meta.file_type)
                    .unwrap_or(NtfsFileType::Unknown),
                size: metadata.map(|meta| meta.size).unwrap_or(0),
            });
        }
        Ok(result)
    }

    pub fn bitmap_usage(&self, device_data: &[u8]) -> Result<(u64, u64, u64), NtfsError> {
        let bitmap_entry = self.read_mft_entry(MFT_BITMAP, device_data)?;
        let bitmap = self.read_file(&bitmap_entry, device_data)?;
        let total_clusters =
            self.boot_sector.total_sectors / self.boot_sector.sectors_per_cluster as u64;
        let mut used_clusters = 0u64;
        for (byte_index, byte) in bitmap.iter().enumerate() {
            for bit in 0..8 {
                let cluster = byte_index as u64 * 8 + bit;
                if cluster >= total_clusters {
                    break;
                }
                if (byte >> bit) & 1 == 1 {
                    used_clusters += 1;
                }
            }
        }
        let total = total_clusters.saturating_mul(self.cluster_size);
        let used = used_clusters.saturating_mul(self.cluster_size);
        let free = total.saturating_sub(used);
        Ok((total, used, free))
    }

    pub fn bitmap_usage_from_storage(
        &self,
        storage: &NtfsStorage,
    ) -> Result<(u64, u64, u64), NtfsError> {
        let bitmap_entry = self.read_mft_entry_from_storage(MFT_BITMAP, storage)?;
        let bitmap = self.read_file_from_storage(&bitmap_entry, storage)?;
        let total_clusters =
            self.boot_sector.total_sectors / self.boot_sector.sectors_per_cluster as u64;
        let mut used_clusters = 0u64;
        for (byte_index, byte) in bitmap.iter().enumerate() {
            for bit in 0..8 {
                let cluster = byte_index as u64 * 8 + bit;
                if cluster >= total_clusters {
                    break;
                }
                if (byte >> bit) & 1 == 1 {
                    used_clusters += 1;
                }
            }
        }
        let total = total_clusters.saturating_mul(self.cluster_size);
        let used = used_clusters.saturating_mul(self.cluster_size);
        let free = total.saturating_sub(used);
        Ok((total, used, free))
    }

    fn max_mft_entries(&self, device_data: &[u8]) -> u64 {
        if self.mft_entry_size == 0 || self.mft_offset as usize >= device_data.len() {
            return 0;
        }
        ((device_data.len() - self.mft_offset as usize) / self.mft_entry_size as usize) as u64
    }

    fn max_mft_entries_from_storage(&self, storage: &NtfsStorage) -> Result<u64, NtfsError> {
        let image_len = storage.image_len()?;
        if self.mft_entry_size == 0 || self.mft_offset as usize >= image_len {
            return Ok(0);
        }
        Ok(((image_len - self.mft_offset as usize) / self.mft_entry_size as usize) as u64)
    }

    fn read_filename_attr(&self, entry: &MftEntry) -> Option<FileNameAttr> {
        let attr = entry.get_filename_attribute()?;
        let data = attr.get_resident_data()?;
        FileNameAttr::parse(data)
    }

    fn find_child_entry(
        &self,
        parent_entry: u64,
        name: &str,
        device_data: &[u8],
    ) -> Result<MftEntry, NtfsError> {
        // Önce $INDEX_ROOT üzerinden dene
        let parent = self.read_mft_entry(parent_entry, device_data)?;
        if let Some(index_attr) = parent.get_attribute(ATTR_INDEX_ROOT) {
            if let Some(entry) = self.find_via_index_root(index_attr, name, device_data)? {
                return Ok(entry);
            }
        }
        // Fallback: MFT scan
        self.find_via_mft_scan(parent_entry, name, device_data)
    }

    fn find_child_entry_from_storage(
        &self,
        parent_entry: u64,
        name: &str,
        storage: &NtfsStorage,
    ) -> Result<MftEntry, NtfsError> {
        let parent = self.read_mft_entry_from_storage(parent_entry, storage)?;
        if let Some(index_attr) = parent.get_attribute(ATTR_INDEX_ROOT) {
            if let Some(entry) =
                self.find_via_index_root_from_storage(index_attr, name, &parent, storage)?
            {
                return Ok(entry);
            }
        }
        self.find_via_mft_scan_from_storage(parent_entry, name, storage)
    }

    /// $INDEX_ROOT üzerinden child bul (resident attribute)
    fn find_via_index_root(
        &self,
        index_attr: &NtfsAttribute,
        name: &str,
        device_data: &[u8],
    ) -> Result<Option<MftEntry>, NtfsError> {
        if index_attr.non_resident {
            return Ok(None);
        }
        let data = &index_attr.resident_data;
        if data.len() < 32 {
            return Ok(None);
        }

        let _root_header = IndexRootHeader::parse(data).ok_or(NtfsError::InvalidFormat)?;
        let node_header = IndexNodeHeader::parse(&data[16..]).ok_or(NtfsError::InvalidFormat)?;

        let entries_start = 32usize;
        let entries_end = entries_start + node_header.entries_length as usize;
        if entries_end > data.len() {
            return Ok(None);
        }

        let mut offset = entries_start;
        while offset + 16 <= data.len() {
            if let Some(entry) = IndexEntry::parse(&data[offset..]) {
                if entry.is_end() {
                    break;
                }
                if let Some(ref fname) = entry.filename_attr {
                    if ntfs_name_matches(&fname.name, name) {
                        let entry_num = entry.file_reference & 0xFFFFFFFFFFFF;
                        return self
                            .read_mft_entry(entry_num, device_data)
                            .map(Some)
                            .or(Ok(None));
                    }
                }
                offset += entry.entry_length as usize;
            } else {
                break;
            }
        }
        Ok(None)
    }

    fn find_via_index_root_from_storage(
        &self,
        index_attr: &NtfsAttribute,
        name: &str,
        parent: &MftEntry,
        storage: &NtfsStorage,
    ) -> Result<Option<MftEntry>, NtfsError> {
        if index_attr.non_resident {
            return Ok(None);
        }
        let data = &index_attr.resident_data;
        if data.len() < 32 {
            return Ok(None);
        }

        let _root_header = IndexRootHeader::parse(data).ok_or(NtfsError::InvalidFormat)?;
        let node_header = IndexNodeHeader::parse(&data[16..]).ok_or(NtfsError::InvalidFormat)?;

        let entries_start = 32usize;
        let entries_end = entries_start + node_header.entries_length as usize;
        if entries_end > data.len() {
            return Ok(None);
        }

        let mut offset = entries_start;
        while offset + 16 <= data.len() {
            if let Some(entry) = IndexEntry::parse(&data[offset..]) {
                if entry.is_end() {
                    if entry.has_sub_node() {
                        // Sub-node VCN bulundu, $INDEX_ALLOCATION'a geç
                        if let Some(sub_node_vcn) = entry.sub_node_vcn {
                            if let Some(index_alloc_attr) =
                                parent.get_attribute(ATTR_INDEX_ALLOCATION)
                            {
                                return self.find_via_index_allocation_from_storage(
                                    Some(index_alloc_attr),
                                    name,
                                    storage,
                                    sub_node_vcn,
                                );
                            }
                        }
                    }
                    break;
                }
                if let Some(ref fname) = entry.filename_attr {
                    if ntfs_name_matches(&fname.name, name) {
                        return self
                            .read_mft_entry_from_storage(
                                entry.file_reference & 0xFFFFFFFFFFFF,
                                storage,
                            )
                            .map(Some)
                            .or(Ok(None));
                    }
                }
                offset += entry.entry_length as usize;
            } else {
                break;
            }
        }
        Ok(None)
    }

    /// $INDEX_ALLOCATION üzerinden child bul (storage-aware, sub_node VCN ile)
    fn find_via_index_allocation_from_storage(
        &self,
        index_alloc_attr: Option<&NtfsAttribute>,
        name: &str,
        storage: &NtfsStorage,
        sub_node_vcn: u64,
    ) -> Result<Option<MftEntry>, NtfsError> {
        let attr = match index_alloc_attr {
            Some(a) => a,
            None => return Ok(None),
        };
        if !attr.non_resident {
            return Ok(None);
        }

        let data = self.read_non_resident_data_from_storage(attr, storage)?;
        if data.is_empty() {
            return Ok(None);
        }

        // Index record'ları parse et (her record INDX header ile başlar)
        // sub_node_vcn, index record'un VCN'sini belirtir
        let record_size = 4096usize; // Tipik index record boyutu
        let record_offset = (sub_node_vcn as usize) * record_size;

        if record_offset + 32 > data.len() {
            return Ok(None);
        }

        // INDX magic kontrolü: "INDX" = 0x58444E49
        if data[record_offset] != 0x49
            || data[record_offset + 1] != 0x4E
            || data[record_offset + 2] != 0x44
            || data[record_offset + 3] != 0x58
        {
            return Ok(None);
        }

        let node_header = IndexNodeHeader::parse(&data[record_offset + 24..]);
        if let Some(nh) = node_header {
            let entries_start = record_offset + 24 + 16;
            let entries_end = entries_start + nh.entries_length as usize;
            if entries_end <= data.len() {
                let mut eoffset = entries_start;
                while eoffset + 16 <= data.len() {
                    if let Some(entry) = IndexEntry::parse(&data[eoffset..]) {
                        if entry.is_end() {
                            // Bu record'da bulunamadı, sub_node varsa devam et
                            if entry.has_sub_node() {
                                if let Some(next_vcn) = entry.sub_node_vcn {
                                    return self.find_via_index_allocation_from_storage(
                                        Some(attr),
                                        name,
                                        storage,
                                        next_vcn,
                                    );
                                }
                            }
                            break;
                        }
                        if let Some(ref fname) = entry.filename_attr {
                            if ntfs_name_matches(&fname.name, name) {
                                return self
                                    .read_mft_entry_from_storage(
                                        entry.file_reference & 0xFFFFFFFFFFFF,
                                        storage,
                                    )
                                    .map(Some)
                                    .or(Ok(None));
                            }
                        }
                        eoffset += entry.entry_length as usize;
                    } else {
                        break;
                    }
                }
            }
        }
        Ok(None)
    }

    /// Non-resident attribute verisini storage'dan oku (data runs ile)
    fn read_non_resident_data_from_storage(
        &self,
        attr: &NtfsAttribute,
        storage: &NtfsStorage,
    ) -> Result<Vec<u8>, NtfsError> {
        if let AttributeContent::NonResident { data_runs, .. } = &attr.content {
            let mut result = Vec::new();
            let mut current_lcn = 0i64;
            for run in data_runs {
                if run.length == 0 {
                    current_lcn = 0;
                    continue;
                }
                current_lcn += run.lcn;
                if current_lcn < 0 {
                    continue;
                }
                let offset = current_lcn as u64 * self.cluster_size;
                let length = (run.length * self.cluster_size) as usize;
                let data = storage.read_exact(offset as usize, length)?;
                result.extend_from_slice(&data);
            }
            Ok(result)
        } else {
            Ok(Vec::new())
        }
    }

    /// $INDEX_ALLOCATION üzerinden child bul (non-resident, data runs)
    fn find_via_index_allocation(
        &self,
        index_alloc_attr: Option<&NtfsAttribute>,
        name: &str,
        device_data: &[u8],
    ) -> Result<Option<MftEntry>, NtfsError> {
        let attr = match index_alloc_attr {
            Some(a) => a,
            None => return Ok(None),
        };
        if !attr.non_resident {
            return Ok(None);
        }

        let data = self.read_non_resident_data(attr, device_data)?;
        if data.is_empty() {
            return Ok(None);
        }

        // Index record'ları parse et (her record INDX header ile başlar)
        let mut offset = 0;
        while offset + 32 <= data.len() {
            // INDX magic kontrolü (opsiyonel, bazı implementasyonlarda yok)
            let node_header = IndexNodeHeader::parse(&data[offset + 24..]);
            if let Some(nh) = node_header {
                let entries_start = offset + 24 + 16;
                let entries_end = entries_start + nh.entries_length as usize;
                if entries_end <= data.len() {
                    let mut eoffset = entries_start;
                    while eoffset + 16 <= data.len() {
                        if let Some(entry) = IndexEntry::parse(&data[eoffset..]) {
                            if entry.is_end() {
                                break;
                            }
                            if let Some(ref fname) = entry.filename_attr {
                                if ntfs_name_matches(&fname.name, name) {
                                    return self
                                        .read_mft_entry(
                                            entry.file_reference & 0xFFFFFFFFFFFF,
                                            device_data,
                                        )
                                        .map(Some)
                                        .or(Ok(None));
                                }
                            }
                            eoffset += entry.entry_length as usize;
                        } else {
                            break;
                        }
                    }
                }
            }
            // Sonraki index record'a atla (genellikle 4KB)
            offset += 4096;
        }
        Ok(None)
    }

    /// Fallback: tüm MFT'yi scan et (mevcut O(n) davranış)
    fn find_via_mft_scan(
        &self,
        parent_entry: u64,
        name: &str,
        device_data: &[u8],
    ) -> Result<MftEntry, NtfsError> {
        for entry_num in 0..self.max_mft_entries(device_data) {
            let Ok(entry) = self.read_mft_entry(entry_num, device_data) else {
                continue;
            };
            if !entry.is_valid() {
                continue;
            }
            let Some(filename_attr) = self.read_filename_attr(&entry) else {
                continue;
            };
            if normalize_file_reference(filename_attr.parent_directory) != parent_entry {
                continue;
            }
            if ntfs_name_matches(&filename_attr.name, name) {
                return Ok(entry);
            }
        }
        Err(NtfsError::NotFound)
    }

    fn find_via_mft_scan_from_storage(
        &self,
        parent_entry: u64,
        name: &str,
        storage: &NtfsStorage,
    ) -> Result<MftEntry, NtfsError> {
        for entry_num in 0..self.max_mft_entries_from_storage(storage)? {
            let Ok(entry) = self.read_mft_entry_from_storage(entry_num, storage) else {
                continue;
            };
            if !entry.is_valid() {
                continue;
            }
            let Some(filename_attr) = self.read_filename_attr(&entry) else {
                continue;
            };
            if normalize_file_reference(filename_attr.parent_directory) != parent_entry {
                continue;
            }
            if ntfs_name_matches(&filename_attr.name, name) {
                return Ok(entry);
            }
        }
        Err(NtfsError::NotFound)
    }

    fn read_non_resident_data(
        &self,
        attr: &NtfsAttribute,
        device_data: &[u8],
    ) -> Result<Vec<u8>, NtfsError> {
        if let AttributeContent::NonResident { data_runs, .. } = &attr.content {
            let mut result = Vec::new();
            let mut current_lcn = 0i64;
            for run in data_runs {
                if run.length == 0 {
                    current_lcn = 0;
                    continue;
                }
                current_lcn += run.lcn;
                if current_lcn < 0 {
                    continue;
                }
                let offset = current_lcn as u64 * self.cluster_size;
                let length = (run.length * self.cluster_size) as usize;
                if offset as usize + length <= device_data.len() {
                    result
                        .extend_from_slice(&device_data[offset as usize..offset as usize + length]);
                }
            }
            Ok(result)
        } else {
            Ok(Vec::new())
        }
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
    static ref NTFS_INSTANCES: Mutex<BTreeMap<String, MountedNtfs>> = Mutex::new(BTreeMap::new());
}

#[derive(Clone, Debug)]
pub enum NtfsStorage {
    Resident(Arc<Vec<u8>>),
    LoopbackDevice(String),
}

impl NtfsStorage {
    pub fn image_len(&self) -> Result<usize, NtfsError> {
        match self {
            Self::Resident(image) => Ok(image.len()),
            Self::LoopbackDevice(name) => {
                let device =
                    crate::drivers::loopback::open(name.as_str()).ok_or(NtfsError::ReadError)?;
                let descriptor = device.descriptor();
                Ok(descriptor.block_count as usize * descriptor.block_size as usize)
            }
        }
    }

    pub fn read_exact(&self, offset: usize, len: usize) -> Result<Vec<u8>, NtfsError> {
        match self {
            Self::Resident(image) => {
                let end = offset.checked_add(len).ok_or(NtfsError::ReadError)?;
                if end > image.len() {
                    return Err(NtfsError::ReadError);
                }
                Ok(image[offset..end].to_vec())
            }
            Self::LoopbackDevice(name) => {
                let mut device =
                    crate::drivers::loopback::open(name.as_str()).ok_or(NtfsError::ReadError)?;
                let descriptor = device.descriptor();
                let block_size = descriptor.block_size as usize;
                let total_len = descriptor.block_count as usize * block_size;
                let end = offset.checked_add(len).ok_or(NtfsError::ReadError)?;
                if end > total_len {
                    return Err(NtfsError::ReadError);
                }
                let start_block = offset / block_size;
                let end_block = (end + block_size - 1) / block_size;
                let mut blocks = Vec::with_capacity((end_block - start_block) * block_size);
                for lba in start_block..end_block {
                    let mut block = vec![0u8; block_size];
                    crate::drivers::block::BlockDevice::read_block(
                        &mut device,
                        lba as u64,
                        block.as_mut_slice(),
                    )
                    .map_err(|_| NtfsError::ReadError)?;
                    blocks.extend_from_slice(block.as_slice());
                }
                let inner_offset = offset % block_size;
                Ok(blocks[inner_offset..inner_offset + len].to_vec())
            }
        }
    }

    pub fn write_exact(&self, offset: usize, data: &[u8]) -> Result<(), NtfsError> {
        match self {
            Self::Resident(image) => {
                let image_len = image.len();
                let end = offset.checked_add(data.len()).ok_or(NtfsError::ReadError)?;
                if end > image_len {
                    return Err(NtfsError::ReadError);
                }
                // Arc<Vec<u8>> mutable değil — write için yeni Arc oluştur
                // Bu yöntem resident storage'da write için uygun değil,
                // loopback device kullanılmalı
                Err(NtfsError::NotSupported)
            }
            Self::LoopbackDevice(name) => {
                let mut device =
                    crate::drivers::loopback::open(name.as_str()).ok_or(NtfsError::ReadError)?;
                let descriptor = device.descriptor();
                let block_size = descriptor.block_size as usize;
                let total_len = descriptor.block_count as usize * block_size;
                let end = offset.checked_add(data.len()).ok_or(NtfsError::ReadError)?;
                if end > total_len {
                    return Err(NtfsError::ReadError);
                }
                let start_block = offset / block_size;
                let end_block = (end + block_size - 1) / block_size;
                let mut buffer = if start_block == end_block {
                    // Tek blok içinde kısmi yazma
                    let mut block = vec![0u8; block_size];
                    crate::drivers::block::BlockDevice::read_block(
                        &mut device,
                        start_block as u64,
                        block.as_mut_slice(),
                    )
                    .map_err(|_| NtfsError::ReadError)?;
                    let inner_offset = offset % block_size;
                    block[inner_offset..inner_offset + data.len()].copy_from_slice(data);
                    block
                } else {
                    // Çoklu blok yazma
                    let mut blocks = vec![0u8; (end_block - start_block) * block_size];
                    // İlk blok: oku + kısmi yaz
                    let mut first_block = vec![0u8; block_size];
                    crate::drivers::block::BlockDevice::read_block(
                        &mut device,
                        start_block as u64,
                        first_block.as_mut_slice(),
                    )
                    .map_err(|_| NtfsError::ReadError)?;
                    let first_offset = offset % block_size;
                    let first_len = block_size - first_offset;
                    first_block[first_offset..].copy_from_slice(&data[..first_len]);
                    blocks[..block_size].copy_from_slice(&first_block);
                    // Orta bloklar: tam yaz
                    let mut data_pos = first_len;
                    for i in 1..(end_block - start_block - 1) {
                        blocks[i * block_size..(i + 1) * block_size]
                            .copy_from_slice(&data[data_pos..data_pos + block_size]);
                        data_pos += block_size;
                    }
                    // Son blok: oku + kısmi yaz
                    if end_block > start_block + 1 {
                        let last_idx = end_block - start_block - 1;
                        let mut last_block = vec![0u8; block_size];
                        crate::drivers::block::BlockDevice::read_block(
                            &mut device,
                            end_block as u64 - 1,
                            last_block.as_mut_slice(),
                        )
                        .map_err(|_| NtfsError::ReadError)?;
                        let remaining =
                            data.len() - first_len - (end_block - start_block - 2) * block_size;
                        last_block[..remaining].copy_from_slice(&data[data_pos..]);
                        blocks[last_idx * block_size..].copy_from_slice(&last_block);
                    }
                    blocks
                };
                // Blokları yaz
                for (i, chunk) in buffer.chunks(block_size).enumerate() {
                    crate::drivers::block::BlockDevice::write_block(
                        &mut device,
                        (start_block + i) as u64,
                        chunk,
                    )
                    .map_err(|_| NtfsError::ReadError)?;
                }
                Ok(())
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct MountedNtfs {
    pub fs: NtfsFileSystem,
    pub storage: NtfsStorage,
}

/// NTFS dosya sistemini bağlar (mount)
pub fn mount_ntfs(name: &str, device_data: &[u8]) -> Result<(), NtfsError> {
    let mut fs = NtfsFileSystem::new();
    fs.init(device_data)?;

    NTFS_INSTANCES.lock().insert(
        name.to_string(),
        MountedNtfs {
            fs,
            storage: NtfsStorage::Resident(Arc::new(device_data.to_vec())),
        },
    );
    Ok(())
}

pub fn mount_ntfs_loopback(name: &str, device_name: &str) -> Result<(), NtfsError> {
    let mut fs = NtfsFileSystem::new();
    let storage = NtfsStorage::LoopbackDevice(device_name.to_string());
    fs.init_from_storage(&storage)?;

    NTFS_INSTANCES
        .lock()
        .insert(name.to_string(), MountedNtfs { fs, storage });
    Ok(())
}

/// İsme göre NTFS dosya sistemi örneğini döndürür
pub fn get_ntfs(name: &str) -> Option<NtfsFileSystem> {
    NTFS_INSTANCES
        .lock()
        .get(name)
        .map(|mounted| mounted.fs.clone())
}

pub fn get_mounted_ntfs(name: &str) -> Option<MountedNtfs> {
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

// ============================================================================
// NTFS WRITE DESTEĞİ
// ============================================================================

/// MFT entry'yi ham baytlara serialize eder (parse'ın tersi)
///
/// Format: FILE header + attributes + USA array
/// MST fixup uygulanmamış ham data döner
pub fn serialize_mft_entry(entry: &MftEntry, mft_entry_size: usize) -> Vec<u8> {
    let mut data = vec![0u8; mft_entry_size];

    // FILE signature
    data[0..4].copy_from_slice(&entry.signature);
    // Sequence number (offset 2)
    data[2..4].copy_from_slice(&entry.sequence.to_le_bytes());
    // Link count (offset 18)
    data[18..20].copy_from_slice(&entry.link_count.to_le_bytes());

    // Öznitelikleri serialize et
    let mut attr_offset: usize = 56; // MFT entry header boyutu
    let mut attr_data = Vec::new();

    for attr in &entry.attributes {
        let attr_bytes = serialize_attribute(attr);
        attr_data.extend_from_slice(&attr_bytes);
    }

    // Attribute end marker (0xFFFFFFFF)
    attr_data.extend_from_slice(&0xFFFFFFFFu32.to_le_bytes());

    // USA offset ve count hesapla
    let sector_size = 512;
    let num_sectors = (mft_entry_size + sector_size - 1) / sector_size;
    let usa_offset = mft_entry_size - (num_sectors * 2);
    // USA offset word 4'te
    data[4..6].copy_from_slice(&(usa_offset as u16).to_le_bytes());
    // USA count word 6'da
    data[6..8].copy_from_slice(&((num_sectors + 1) as u16).to_le_bytes());

    // Attribute offset word 20'de
    data[20..22].copy_from_slice(&(attr_offset as u16).to_le_bytes());

    // Öznitelikleri kopyala
    if attr_offset + attr_data.len() <= mft_entry_size {
        data[attr_offset..attr_offset + attr_data.len()].copy_from_slice(&attr_data);
    }

    // MST fixup uygula
    apply_mst_fixup(&mut data, sector_size);

    data
}

/// NtfsAttribute'ü ham baytlara serialize eder
fn serialize_attribute(attr: &NtfsAttribute) -> Vec<u8> {
    let mut data = Vec::new();

    // Common header (16 byte)
    data.extend_from_slice(&attr.attr_type.to_le_bytes());
    data.extend_from_slice(&attr.total_length.to_le_bytes());
    data.push(if attr.non_resident { 1 } else { 0 });
    data.push(attr.name_length);
    data.extend_from_slice(&attr.name_offset.to_le_bytes());
    data.extend_from_slice(&attr.flags.to_le_bytes());
    data.extend_from_slice(&attr.instance.to_le_bytes());

    if attr.non_resident {
        // Non-resident header
        if let AttributeContent::NonResident {
            start_vcn,
            last_vcn,
            data_runs_offset,
            data_runs,
        } = &attr.content
        {
            data.extend_from_slice(&start_vcn.to_le_bytes());
            data.extend_from_slice(&last_vcn.to_le_bytes());
            data.extend_from_slice(&data_runs_offset.to_le_bytes());
            // Compression unit size (2 byte) — 0 = compression yok
            data.extend_from_slice(&0u16.to_le_bytes());
            // Padding (5 byte)
            data.extend_from_slice(&[0u8; 5]);

            // Data run'ları encode et
            let runs_start = *data_runs_offset as usize;
            let mut encoded_runs = encode_data_runs(data_runs);
            // Runs offset'e padding ekle
            while data.len() < runs_start {
                data.push(0);
            }
            data.extend_from_slice(&encoded_runs);

            // total_length güncelle
            let total_len = data.len() as u32;
            data[4..8].copy_from_slice(&total_len.to_le_bytes());
        }
    } else {
        // Resident header
        if let AttributeContent::Resident {
            data_offset,
            data_length,
        } = &attr.content
        {
            data.extend_from_slice(&data_length.to_le_bytes());
            data.extend_from_slice(&data_offset.to_le_bytes());

            // Resident data'yı kopyala
            let resident_start = *data_offset as usize;
            let resident_len = *data_length as usize;
            while data.len() < resident_start {
                data.push(0);
            }
            if let Some(rdata) = attr.get_resident_data() {
                let copy_len = resident_len.min(rdata.len());
                data.extend_from_slice(&rdata[..copy_len]);
            }
            // Padding
            while data.len() < resident_start + resident_len {
                data.push(0);
            }

            // total_length güncelle
            let total_len = data.len() as u32;
            data[4..8].copy_from_slice(&total_len.to_le_bytes());
        }
    }

    data
}

/// DataRun listesini NTFS data run formatına encode eder
fn encode_data_runs(runs: &[DataRun]) -> Vec<u8> {
    let mut data = Vec::new();
    let mut prev_lcn: i64 = 0;

    for run in runs {
        let length = run.length;
        let lcn = run.lcn;

        // Length encoding — minimal byte sayısı
        let mut len_bytes = Vec::new();
        let mut len = length;
        while len > 0 {
            len_bytes.push((len & 0xFF) as u8);
            len >>= 8;
        }
        if len_bytes.is_empty() {
            len_bytes.push(0);
        }

        // LCN encoding — relative to previous, minimal byte sayısı
        let relative_lcn = lcn - prev_lcn;
        let mut lcn_bytes = Vec::new();
        let mut lcn_val = relative_lcn;
        let is_negative = lcn_val < 0;

        while lcn_val != 0 {
            lcn_bytes.push((lcn_val & 0xFF) as u8);
            lcn_val >>= 8;
        }
        if lcn_bytes.is_empty() {
            lcn_bytes.push(0);
        }

        // Sign extension: negative ise son byte 0xFF olmalı
        if is_negative && (lcn_bytes.last().unwrap() & 0x80) == 0 {
            lcn_bytes.push(0xFF);
        }
        // Positive ise son byte 0x80 olmamalı
        if !is_negative && (lcn_bytes.last().unwrap() & 0x80) != 0 {
            lcn_bytes.push(0);
        }

        // Header: low nibble = len size, high nibble = lcn size
        let header = (len_bytes.len() as u8) | ((lcn_bytes.len() as u8) << 4);
        data.push(header);
        data.extend_from_slice(&len_bytes);
        data.extend_from_slice(&lcn_bytes);

        prev_lcn = lcn;
    }

    // Runlist terminator
    data.push(0);

    data
}

/// MFT entry'ye MST (Multi-Sector Transfer) fixup uygula
///
/// Her sektörün son 2 byte'ına update sequence number yazılır.
/// Orijinal sector son byte'ları USA array'e kaydedilir.
fn apply_mst_fixup(data: &mut [u8], sector_size: usize) {
    if data.len() < 48 {
        return;
    }

    let usa_offset = u16::from_le_bytes([data[4], data[5]]) as usize;
    let usa_count = u16::from_le_bytes([data[6], data[7]]) as usize;

    // Sequence number'ı 1 artır
    let seq_num = u16::from_le_bytes([data[2], data[3]]).wrapping_add(1);
    data[2..4].copy_from_slice(&seq_num.to_le_bytes());

    // Her sektörün son 2 byte'ını USA array'e kaydet ve seq_num ile değiştir
    let num_sectors = data.len() / sector_size;
    for i in 0..num_sectors.min(usa_count - 1) {
        let sector_end = (i + 1) * sector_size - 2;
        let orig_bytes = [data[sector_end], data[sector_end + 1]];
        if usa_offset + i * 2 + 2 <= data.len() {
            data[usa_offset + i * 2..usa_offset + i * 2 + 2].copy_from_slice(&orig_bytes);
        }
        data[sector_end..sector_end + 2].copy_from_slice(&seq_num.to_le_bytes());
    }
}

/// MFT entry'den MST fixup doğrula ve kaldır
fn verify_and_remove_mst_fixup(data: &mut [u8], sector_size: usize) -> Result<(), NtfsError> {
    if data.len() < 48 {
        return Err(NtfsError::Corrupted);
    }

    let usa_offset = u16::from_le_bytes([data[4], data[5]]) as usize;
    let usa_count = u16::from_le_bytes([data[6], data[7]]) as usize;

    if usa_offset == 0 || usa_count == 0 {
        return Ok(()); // USA yok
    }

    if usa_offset + usa_count * 2 > data.len() {
        return Err(NtfsError::Corrupted);
    }

    // USA array'den orijinal sector son byte'larını geri yükle
    let num_sectors = data.len() / sector_size;
    for i in 0..num_sectors.min(usa_count - 1) {
        let sector_end = (i + 1) * sector_size - 2;
        if usa_offset + i * 2 + 2 <= data.len() {
            let orig_bytes = [data[usa_offset + i * 2], data[usa_offset + i * 2 + 1]];
            data[sector_end..sector_end + 2].copy_from_slice(&orig_bytes);
        }
    }

    Ok(())
}

/// MFT entry'yi diske yaz
fn write_mft_entry_raw(
    storage: &NtfsStorage,
    fs: &NtfsFileSystem,
    entry_num: u64,
    entry_data: &[u8],
) -> Result<(), NtfsError> {
    let offset = fs.mft_offset + entry_num * fs.mft_entry_size;
    storage.write_exact(offset as usize, entry_data)
}

/// $MFTMirr'e sync et — MFT'nin ilk N entry'sinin kopyası
///
/// Spec: layout.h FILE_MFTMirr — "copy of first four mft records.
/// If cluster size > 4kiB, copy of first N mft records with
/// N = cluster_size / mft_record_size."
/// mft.c ntfs_mftmirr_sync.
fn sync_mft_mirror(
    storage: &NtfsStorage,
    fs: &NtfsFileSystem,
    entry_num: u64,
    entry_data: &[u8],
) -> Result<(), NtfsError> {
    // MFTMirr'de kaç entry var?
    let mirror_count = if fs.cluster_size > 4096 {
        fs.cluster_size / fs.mft_entry_size
    } else {
        4
    };

    // Sadece ilk N entry mirror'da. Bu entry o aralıktaysa sync et.
    if entry_num >= mirror_count {
        return Ok(());
    }

    let mirror_offset = fs.boot_sector.mftmirr_cluster * fs.cluster_size
        + entry_num * fs.mft_entry_size;
    storage.write_exact(mirror_offset as usize, entry_data)
}

/// MFT entry'yi oku, MST fixup'u kaldır, parse et
fn read_mft_entry_raw(
    storage: &NtfsStorage,
    fs: &NtfsFileSystem,
    entry_num: u64,
) -> Result<(MftEntry, Vec<u8>), NtfsError> {
    let entry_bytes = fs.read_mft_entry_from_storage(entry_num, storage)?;
    let mut raw_data = serialize_mft_entry(&entry_bytes, fs.mft_entry_size as usize);

    // Bu zaten parse edilmiş MftEntry, raw_data için ayrı okuma yapalım
    let offset = fs.mft_offset + entry_num * fs.mft_entry_size;
    let mut raw = storage.read_exact(offset as usize, fs.mft_entry_size as usize)?;
    let sector_size = fs.boot_sector.bytes_per_sector as usize;
    verify_and_remove_mst_fixup(&mut raw, sector_size)?;
    let entry = MftEntry::parse(&raw).ok_or(NtfsError::Corrupted)?;

    Ok((entry, raw))
}

/// $BITMAP attribute verisini oku
///
/// $Bitmap (MFT #6) non-resident attribute'tur. Data run'ları absolute LCN
/// içerir — MFT offset ile karıştırılmamalıdır. Spec: layout.h FILE_Bitmap,
/// attrib.c ntfs_attr_read.
fn read_bitmap_data(
    storage: &NtfsStorage,
    fs: &NtfsFileSystem,
    bitmap_mft_num: u64,
) -> Result<Vec<u8>, NtfsError> {
    let (entry, _raw) = read_mft_entry_raw(storage, fs, bitmap_mft_num)?;

    let bitmap_attr = entry
        .attributes
        .iter()
        .find(|a| a.attr_type == ATTR_BITMAP)
        .ok_or(NtfsError::NotFound)?;

    if !bitmap_attr.non_resident {
        bitmap_attr
            .get_resident_data()
            .map(|d| d.to_vec())
            .ok_or(NtfsError::Corrupted)
    } else {
        if let AttributeContent::NonResident { data_runs, .. } = &bitmap_attr.content {
            let mut data = Vec::new();
            let mut prev_lcn: i64 = 0;

            for run in data_runs {
                let lcn = prev_lcn + run.lcn;
                prev_lcn = lcn;

                if lcn >= 0 {
                    let cluster_offset = (lcn as u64) * fs.cluster_size;
                    let read_len = (run.length * fs.cluster_size) as usize;
                    let bytes = storage.read_exact(cluster_offset as usize, read_len)?;
                    data.extend_from_slice(&bytes);
                }
            }

            Ok(data)
        } else {
            Err(NtfsError::Corrupted)
        }
    }
}

/// $BITMAP attribute verisini yaz
///
/// Data run'ları absolute LCN içerir. Spec: attrib.c ntfs_cluster_write.
fn write_bitmap_data(
    storage: &NtfsStorage,
    fs: &NtfsFileSystem,
    bitmap_mft_num: u64,
    data: &[u8],
) -> Result<(), NtfsError> {
    let (entry, _raw) = read_mft_entry_raw(storage, fs, bitmap_mft_num)?;

    let bitmap_attr = entry
        .attributes
        .iter()
        .find(|a| a.attr_type == ATTR_BITMAP)
        .ok_or(NtfsError::NotFound)?;

    if !bitmap_attr.non_resident {
        Err(NtfsError::NotSupported)
    } else {
        if let AttributeContent::NonResident { data_runs, .. } = &bitmap_attr.content {
            let mut data_offset = 0usize;
            let mut prev_lcn: i64 = 0;

            for run in data_runs {
                let lcn = prev_lcn + run.lcn;
                prev_lcn = lcn;

                if lcn >= 0 {
                    let cluster_offset = (lcn as u64) * fs.cluster_size;
                    let write_len = (run.length * fs.cluster_size) as usize;
                    let write_len = write_len.min(data.len() - data_offset);
                    storage.write_exact(
                        cluster_offset as usize,
                        &data[data_offset..data_offset + write_len],
                    )?;
                    data_offset += write_len;
                }
            }

            Ok(())
        } else {
            Err(NtfsError::Corrupted)
        }
    }
}

/// $BITMAP'te bir bit'i set veya clear et
fn update_bitmap_bit(
    storage: &NtfsStorage,
    fs: &NtfsFileSystem,
    bitmap_mft_num: u64,
    bit: u64,
    set: bool,
) -> Result<(), NtfsError> {
    let mut bitmap_data = read_bitmap_data(storage, fs, bitmap_mft_num)?;

    let byte_idx = (bit / 8) as usize;
    let bit_idx = (bit % 8) as u8;

    if byte_idx >= bitmap_data.len() {
        return Err(NtfsError::NoSpace);
    }

    if set {
        bitmap_data[byte_idx] |= 1 << bit_idx;
    } else {
        bitmap_data[byte_idx] &= !(1 << bit_idx);
    }

    write_bitmap_data(storage, fs, bitmap_mft_num, &bitmap_data)
}

/// $BITMAP'te ilk sıfır bit'i bul
fn find_free_bit_in_bitmap(
    storage: &NtfsStorage,
    fs: &NtfsFileSystem,
    bitmap_mft_num: u64,
    start_bit: u64,
) -> Result<u64, NtfsError> {
    let bitmap_data = read_bitmap_data(storage, fs, bitmap_mft_num)?;
    let total_bits = (bitmap_data.len() as u64) * 8;

    let mut bit = start_bit;
    while bit < total_bits {
        let byte_idx = (bit / 8) as usize;
        let bit_idx = (bit % 8) as u8;
        if byte_idx < bitmap_data.len() && (bitmap_data[byte_idx] & (1 << bit_idx)) == 0 {
            return Ok(bit);
        }
        bit += 1;
    }

    Err(NtfsError::NoSpace)
}

/// NTFS attribute'tan VCN->LCN mapping ile veri oku
///
/// Data run'ları delta-encoded LCN içerir. İlk run absolute LCN,
/// sonraki run'lar önceki LCN'ye göreli. Spec: layout.h ATTR_RECORD,
/// attrib.c ntfs_attr_read.
fn read_non_resident_attr(
    storage: &NtfsStorage,
    fs: &NtfsFileSystem,
    attr: &NtfsAttribute,
) -> Result<Vec<u8>, NtfsError> {
    if let AttributeContent::NonResident {
        data_runs,
        last_vcn,
        start_vcn,
        ..
    } = &attr.content
    {
        let total_size = ((*last_vcn + 1).saturating_sub(*start_vcn) * fs.cluster_size) as usize;
        if total_size == 0 {
            return Ok(Vec::new());
        }
        let mut data = vec![0u8; total_size];
        let mut prev_lcn: i64 = 0;
        let mut current_vcn: u64 = *start_vcn;

        for run in data_runs {
            let lcn = prev_lcn + run.lcn;
            prev_lcn = lcn;

            if lcn >= 0 {
                let cluster_offset = (lcn as u64) * fs.cluster_size;
                let read_len = (run.length * fs.cluster_size) as usize;
                let data_offset = ((current_vcn - *start_vcn) * fs.cluster_size) as usize;
                let copy_len = read_len.min(total_size.saturating_sub(data_offset));
                if data_offset < total_size {
                    let bytes = storage.read_exact(cluster_offset as usize, read_len)?;
                    data[data_offset..data_offset + copy_len]
                        .copy_from_slice(&bytes[..copy_len]);
                }
            }

            current_vcn += run.length;
        }

        Ok(data)
    } else {
        Err(NtfsError::NotSupported)
    }
}

/// NTFS attribute'a VCN->LCN mapping ile veri yaz
///
/// Data run'ları delta-encoded LCN içerir. İlk run absolute LCN,
/// sonraki run'lar önceki LCN'ye göreli. Spec: attrib.c ntfs_attr_write.
fn write_non_resident_attr(
    storage: &NtfsStorage,
    fs: &NtfsFileSystem,
    attr: &NtfsAttribute,
    data: &[u8],
) -> Result<(), NtfsError> {
    if let AttributeContent::NonResident {
        data_runs,
        start_vcn,
        ..
    } = &attr.content
    {
        let mut data_offset = 0usize;
        let mut prev_lcn: i64 = 0;
        let mut current_vcn: u64 = *start_vcn;

        for run in data_runs {
            let lcn = prev_lcn + run.lcn;
            prev_lcn = lcn;

            if lcn >= 0 && data_offset < data.len() {
                let cluster_offset = (lcn as u64) * fs.cluster_size;
                let write_len = (run.length * fs.cluster_size) as usize;
                let write_len = write_len.min(data.len() - data_offset);
                storage.write_exact(
                    cluster_offset as usize,
                    &data[data_offset..data_offset + write_len],
                )?;
                data_offset += write_len;
            }

            current_vcn += run.length;
        }

        Ok(())
    } else {
        Err(NtfsError::NotSupported)
    }
}

/// NTFS'te dosya yaz — resident ve non-resident tam destek
///
/// Küçük dosyalar (~700 byte altı) resident olarak MFT'ye yazılır.
/// Büyük dosyalar non-resident olarak cluster'lara yazılır.
/// data=ordered semantics: önce veri yazılır, sonra metadata güncellenir.
pub fn write_ntfs_file(
    storage: &NtfsStorage,
    fs: &NtfsFileSystem,
    mft_entry_num: u64,
    data: &[u8],
) -> Result<(), NtfsError> {
    let (mut entry, mut raw_data) = read_mft_entry_raw(storage, fs, mft_entry_num)?;

    // $DATA attribute bul (isimsiz — default data stream)
    let data_attr_idx = entry
        .attributes
        .iter()
        .position(|a| a.attr_type == ATTR_DATA && a.name_length == 0)
        .ok_or(NtfsError::NotFound)?;

    let data_attr = &entry.attributes[data_attr_idx];
    let mft_entry_size = fs.mft_entry_size as usize;

    if !data_attr.non_resident && data.len() <= 700 {
        // Resident write — MFT entry içinde
        if let AttributeContent::Resident {
            data_offset,
            data_length,
        } = &data_attr.content
        {
            let offset = *data_offset as usize;
            let len = *data_length as usize;

            if offset + data.len() > mft_entry_size {
                // Veri MFT'ye sığmıyor, non-resident'e geç
                return write_ntfs_file_non_resident(storage, fs, mft_entry_num, data);
            }

            // raw_data'da resident veriyi güncelle
            raw_data[offset..offset + data.len()].copy_from_slice(data);

            // Attribute header güncelle: content length (offset 16)
            let attr_header_offset = find_attr_header_offset(&raw_data, data_attr_idx)?;
            let len_bytes = (data.len() as u32).to_le_bytes();
            raw_data[attr_header_offset + 16..attr_header_offset + 20].copy_from_slice(&len_bytes);

            // MFT entry'yi geri yaz
            write_mft_entry_raw(storage, fs, mft_entry_num, &raw_data)?;
            return Ok(());
        }
    }

    // Non-resident write
    if data_attr.non_resident {
        // Zaten non-resident — mevcut cluster'lara yaz
        write_non_resident_attr(storage, fs, data_attr, data)?;

        // MFT entry'deki data size güncelle
        let attr_header_offset = find_attr_header_offset(&raw_data, data_attr_idx)?;
        let size_bytes = (data.len() as u64).to_le_bytes();
        // Initialized size (offset 48)
        raw_data[attr_header_offset + 48..attr_header_offset + 56].copy_from_slice(&size_bytes);
        // Data size (offset 56)
        raw_data[attr_header_offset + 56..attr_header_offset + 64].copy_from_slice(&size_bytes);

        write_mft_entry_raw(storage, fs, mft_entry_num, &raw_data)?;
    } else {
        // Resident'tan non-resident'a geçiş
        return write_ntfs_file_non_resident(storage, fs, mft_entry_num, data);
    }

    Ok(())
}

/// Dosyayı non-resident olarak yaz — cluster allocation + runlist + data write
///
/// Spec: attrib.c ntfs_cluster_alloc + ntfs_non_resident_attr_write.
/// 1. $Bitmap'den cluster allocate et (zone-aware, contiguous tercih)
/// 2. Bitişik cluster'ları run'lara birleştir (delta-encoded LCN)
/// 3. Veriyi allocated cluster'lara yaz (data=ordered: önce veri)
/// 4. MFT entry'yi güncelle (sonra metadata)
/// 5. $MFTMirr'e sync et
fn write_ntfs_file_non_resident(
    storage: &NtfsStorage,
    fs: &NtfsFileSystem,
    mft_entry_num: u64,
    data: &[u8],
) -> Result<(), NtfsError> {
    let clusters_needed = ((data.len() as u64 + fs.cluster_size - 1) / fs.cluster_size) as u64;
    if clusters_needed == 0 {
        return Err(NtfsError::InvalidFormat);
    }

    // $Bitmap'den free cluster'ları bul ve allocate et
    // Contiguous allocation tercih et (runlist merging için)
    let mut allocated_runs: Vec<(u64, u64)> = Vec::new(); // (start_cluster, count)
    let mut search_start = 0u64;
    let mut remaining = clusters_needed;

    while remaining > 0 {
        let cluster = find_free_bit_in_bitmap(storage, fs, MFT_BITMAP, search_start)?;
        let run_start = cluster;
        let mut run_len = 0u64;

        // Contiguous free cluster'ları tara
        while cluster + run_len < (fs.boot_sector.total_sectors / fs.boot_sector.sectors_per_cluster as u64)
            && run_len < remaining
        {
            let byte_idx = ((cluster + run_len) / 8) as usize;
            let bit_idx = ((cluster + run_len) % 8) as u8;
            let bitmap_data = read_bitmap_data(storage, fs, MFT_BITMAP)?;
            if byte_idx >= bitmap_data.len()
                || (bitmap_data[byte_idx] & (1 << bit_idx)) != 0
            {
                break;
            }
            run_len += 1;
        }

        // Bu run'u allocate et
        for i in 0..run_len {
            update_bitmap_bit(storage, fs, MFT_BITMAP, run_start + i, true)?;
        }
        allocated_runs.push((run_start, run_len));
        remaining -= run_len;
        search_start = run_start + run_len;
    }

    // DataRun'lara dönüştür (delta-encoded LCN)
    let mut runs: Vec<DataRun> = Vec::new();
    let mut prev_lcn: i64 = 0;

    for &(start, count) in &allocated_runs {
        let lcn = start as i64;
        let delta = lcn - prev_lcn;
        runs.push(DataRun {
            length: count,
            lcn: delta,
        });
        prev_lcn = lcn;
    }

    let last_vcn = clusters_needed - 1;
    let data_runs_offset: u16 = 64;

    let new_data_attr = NtfsAttribute {
        attr_type: ATTR_DATA,
        total_length: 0,
        non_resident: true,
        name_length: 0,
        name_offset: 0,
        flags: 0,
        instance: 0,
        content: AttributeContent::NonResident {
            start_vcn: 0,
            last_vcn,
            data_runs_offset,
            data_runs: runs.clone(),
        },
        resident_data: Vec::new(),
    };

    let (mut entry, _raw_data) = read_mft_entry_raw(storage, fs, mft_entry_num)?;

    let data_attr_idx = entry
        .attributes
        .iter()
        .position(|a| a.attr_type == ATTR_DATA && a.name_length == 0)
        .ok_or(NtfsError::NotFound)?;
    entry.attributes[data_attr_idx] = new_data_attr;

    let serialized = serialize_mft_entry(&entry, fs.mft_entry_size as usize);
    write_mft_entry_raw(storage, fs, mft_entry_num, &serialized)?;

    // Sync $MFTMirr (MFT'nin ilk 4 entry'sinin kopyası)
    sync_mft_mirror(storage, fs, mft_entry_num, &serialized)?;

    // Veriyi allocated cluster'lara yaz (data=ordered: önce veri, sonra metadata commit)
    let mut data_offset = 0usize;
    let mut prev_lcn_write: i64 = 0;

    for run in &runs {
        let lcn = prev_lcn_write + run.lcn;
        prev_lcn_write = lcn;

        if lcn >= 0 {
            let cluster_offset = (lcn as u64) * fs.cluster_size;
            let write_len = (run.length as usize) * fs.cluster_size as usize;
            let write_len = write_len.min(data.len() - data_offset);
            storage.write_exact(
                cluster_offset as usize,
                &data[data_offset..data_offset + write_len],
            )?;
            data_offset += write_len;
        }
    }

    Ok(())
}

/// Raw data içinde bir attribute'un header offset'ini bul
fn find_attr_header_offset(raw_data: &[u8], attr_idx: usize) -> Result<usize, NtfsError> {
    let mut offset: usize = u16::from_le_bytes([raw_data[20], raw_data[21]]) as usize;
    let mut idx = 0;

    while offset + 16 <= raw_data.len() {
        let attr_type = u32::from_le_bytes([
            raw_data[offset],
            raw_data[offset + 1],
            raw_data[offset + 2],
            raw_data[offset + 3],
        ]);

        if attr_type == 0xFFFFFFFF {
            break;
        }

        if idx == attr_idx {
            return Ok(offset);
        }

        let attr_len = u32::from_le_bytes([
            raw_data[offset + 4],
            raw_data[offset + 5],
            raw_data[offset + 6],
            raw_data[offset + 7],
        ]) as usize;

        if attr_len == 0 {
            break;
        }

        offset += attr_len;
        idx += 1;
    }

    Err(NtfsError::NotFound)
}

/// NTFS'te yeni dosya oluştur
///
/// Adımlar:
/// 1. $MFT Bitmap'ten free MFT entry bul ve allocate et
/// 2. Yeni MFT entry oluştur (FILE header + $SI + $FN + $DATA)
/// 3. Parent dizinin index'ine entry ekle ($INDEX_ROOT veya $INDEX_ALLOCATION)
/// 4. MFT entry'yi diske yaz
pub fn create_ntfs_file(
    storage: &NtfsStorage,
    fs: &NtfsFileSystem,
    parent_mft: u64,
    name: &str,
    is_dir: bool,
) -> Result<u64, NtfsError> {
    // 1. Free MFT entry bul
    let free_entry = find_free_bit_in_bitmap(storage, fs, MFT_BITMAP, 24)?;
    update_bitmap_bit(storage, fs, MFT_BITMAP, free_entry, true)?;

    // 2. Yeni MFT entry oluştur
    let mft_entry_size = fs.mft_entry_size as usize;
    let sector_size = fs.boot_sector.bytes_per_sector as usize;
    let num_sectors = (mft_entry_size + sector_size - 1) / sector_size;

    // $FILE_NAME attribute için name verisi
    let name_utf16: Vec<u8> = name.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
    let name_len_bytes = name_utf16.len() as u8;

    // $STANDARD_INFORMATION attribute: 72 byte (resident)
    let si_attr = NtfsAttribute {
        attr_type: ATTR_STANDARD_INFORMATION,
        total_length: 72,
        non_resident: false,
        name_length: 0,
        name_offset: 0,
        flags: 0,
        instance: 0,
        content: AttributeContent::Resident {
            data_offset: 24,
            data_length: 64,
        },
        resident_data: vec![0u8; 64], // Timestamps — şimdilik sıfır
    };

    // $FILE_NAME attribute: 66 + name_len (resident)
    // Spec: layout.h FILE_NAME_ATTR — parent_directory is leMFT_REF (48-bit index + 16-bit seq)
    let fname_content_len = 66 + name_utf16.len();
    let mut fname_data = vec![0u8; fname_content_len];
    // Parent directory reference (8 byte) — MFT_REF format: low 48 bits = index, high 16 bits = sequence
    let parent_ref = parent_mft & 0x0000_FFFF_FFFF_FFFF; // 48-bit index, sequence = 0
    fname_data[0..8].copy_from_slice(&parent_ref.to_le_bytes());
    // Timestamps (32 byte) — sıfır
    // File size (8 byte) — sıfır
    // Allocated size (8 byte) — sıfır
    // Flags (4 byte)
    let flags: u32 = if is_dir { 0x00000010 } else { 0x00000020 };
    fname_data[56..60].copy_from_slice(&flags.to_le_bytes());
    // Reparse value (4 byte)
    // Name length (1 byte)
    fname_data[64] = name_len_bytes;
    // Name space (1 byte) — POSIX
    fname_data[65] = FILE_NAME_POSIX;
    // Name
    fname_data[66..].copy_from_slice(&name_utf16);

    let fname_attr = NtfsAttribute {
        attr_type: ATTR_FILE_NAME,
        total_length: (16 + fname_content_len) as u32,
        non_resident: false,
        name_length: 0,
        name_offset: 0,
        flags: 0,
        instance: 0,
        content: AttributeContent::Resident {
            data_offset: 24,
            data_length: fname_content_len as u32,
        },
        resident_data: fname_data,
    };

    // $DATA attribute: resident, boş
    let data_attr = NtfsAttribute {
        attr_type: ATTR_DATA,
        total_length: 24,
        non_resident: false,
        name_length: 0,
        name_offset: 0,
        flags: 0,
        instance: 0,
        content: AttributeContent::Resident {
            data_offset: 24,
            data_length: 0,
        },
        resident_data: Vec::new(),
    };

    // MFT entry oluştur
    let mut entry = MftEntry {
        signature: MftEntry::SIGNATURE_FILE,
        sequence: 1,
        link_count: 1,
        attributes: vec![si_attr, fname_attr, data_attr],
        entry_number: free_entry,
    };

    // Flags: in-use (0x01), directory (0x02)
    // Serialize sırasında raw data'ya yazılacak

    // 3. MFT entry'yi yaz
    let serialized = serialize_mft_entry(&entry, mft_entry_size);
    write_mft_entry_raw(storage, fs, free_entry, &serialized)?;

    // 4. Parent dizine index entry ekle
    add_index_entry(storage, fs, parent_mft, free_entry, name, is_dir)?;

    Ok(free_entry)
}

/// Parent dizinin index'ine yeni entry ekle
///
/// $INDEX_ROOT resident ise: içinde yer varsa ekle, yoksa $INDEX_ALLOCATION'a geç
/// $INDEX_ALLOCATION non-resident ise: index record'a ekle
fn add_index_entry(
    storage: &NtfsStorage,
    fs: &NtfsFileSystem,
    parent_mft: u64,
    child_mft: u64,
    name: &str,
    is_dir: bool,
) -> Result<(), NtfsError> {
    let (mut entry, mut raw_data) = read_mft_entry_raw(storage, fs, parent_mft)?;

    // $INDEX_ROOT attribute bul
    let idx_root_attr = entry
        .attributes
        .iter()
        .find(|a| a.attr_type == ATTR_INDEX_ROOT)
        .ok_or(NtfsError::NotFound)?;

    if !idx_root_attr.non_resident {
        // Resident $INDEX_ROOT — içinde yer varsa ekle
        if let AttributeContent::Resident {
            data_offset,
            data_length,
        } = &idx_root_attr.content
        {
            let idx_data = idx_root_attr
                .get_resident_data()
                .ok_or(NtfsError::Corrupted)?;
            let mut idx_data = idx_data.to_vec();

            // Index node header parse
            if idx_data.len() < 16 {
                return Err(NtfsError::Corrupted);
            }
            let entries_offset =
                u32::from_le_bytes([idx_data[0], idx_data[1], idx_data[2], idx_data[3]]) as usize;
            let entries_length =
                u32::from_le_bytes([idx_data[4], idx_data[5], idx_data[6], idx_data[7]]) as usize;
            let allocated_size =
                u32::from_le_bytes([idx_data[8], idx_data[9], idx_data[10], idx_data[11]]) as usize;

            // Yeni index entry oluştur
            let name_utf16: Vec<u8> = name.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
            let entry_size = 16 + 66 + name_utf16.len() + 8; // header + $FN + sub-node VCN (align)
            let entry_size_aligned = (entry_size + 7) & !7;

            // Yer var mı kontrol et
            if entries_length + entry_size_aligned > allocated_size {
                // Yer yok — $INDEX_ALLOCATION'a geç (şimdilik hata)
                return Err(NtfsError::NoSpace);
            }

            // Index entry oluştur
            let mut new_entry = vec![0u8; entry_size_aligned];
            // File reference (8 byte)
            new_entry[0..8].copy_from_slice(&child_mft.to_le_bytes());
            // Entry length (2 byte)
            new_entry[8..10].copy_from_slice(&(entry_size as u16).to_le_bytes());
            // Stream length (2 byte) — $FILE_NAME boyutu
            new_entry[10..12].copy_from_slice(&((66 + name_utf16.len()) as u16).to_le_bytes());
            // Flags: NODE (0x01) — child var
            if is_dir {
                new_entry[12] = INDEX_ENTRY_NODE;
            }
            // $FILE_NAME stream
            let mut fname_data = vec![0u8; 66 + name_utf16.len()];
            // Parent directory = child_mft'nin parent'ı (parent_mft), MFT_REF format
            let parent_ref = parent_mft & 0x0000_FFFF_FFFF_FFFF;
            fname_data[0..8].copy_from_slice(&parent_ref.to_le_bytes());
            let flags: u32 = if is_dir { 0x00000010 } else { 0x00000020 };
            fname_data[56..60].copy_from_slice(&flags.to_le_bytes());
            fname_data[64] = name_utf16.len() as u8 / 2;
            fname_data[65] = FILE_NAME_POSIX;
            fname_data[66..].copy_from_slice(&name_utf16);
            new_entry[16..16 + fname_data.len()].copy_from_slice(&fname_data);

            // Sub-node VCN (eğer NODE flag varsa)
            if is_dir {
                let vcn_offset = entry_size - 8;
                new_entry[vcn_offset..vcn_offset + 8].copy_from_slice(&0u64.to_le_bytes());
            }

            // END marker oluştur
            let end_entry_size = 16;
            let mut end_entry = vec![0u8; end_entry_size];
            end_entry[8..10].copy_from_slice(&(end_entry_size as u16).to_le_bytes());
            end_entry[12] = INDEX_ENTRY_END;

            // Yeni entry'yi idx_data'ya ekle
            let insert_pos = entries_offset + entries_length - end_entry_size;
            idx_data.splice(insert_pos..insert_pos, new_entry.iter().cloned());

            // Index node header güncelle
            let new_entries_length = entries_length + entry_size_aligned;
            idx_data[4..8].copy_from_slice(&(new_entries_length as u32).to_le_bytes());

            // Attribute content güncelle
            let attr_idx = entry
                .attributes
                .iter()
                .position(|a| a.attr_type == ATTR_INDEX_ROOT)
                .unwrap();

            if let AttributeContent::Resident { data_length, .. } =
                &mut entry.attributes[attr_idx].content
            {
                *data_length = idx_data.len() as u32;
            }
            entry.attributes[attr_idx].resident_data = idx_data;

            // MFT entry'yi serialize ve yaz
            let serialized = serialize_mft_entry(&entry, fs.mft_entry_size as usize);
            write_mft_entry_raw(storage, fs, parent_mft, &serialized)?;

            return Ok(());
        }
    }

    // Non-resident $INDEX_ROOT veya $INDEX_ALLOCATION — şimdilik unsupported
    Err(NtfsError::NotSupported)
}

/// NTFS'te dosya sil
///
/// Adımlar:
/// 1. MFT entry'den link count'u azalt
/// 2. Link count 0 ise: MFT bitmap'te entry'yi free et, data cluster'larını free et
/// 3. Parent dizinden index entry'yi kaldır
pub fn delete_ntfs_file(
    storage: &NtfsStorage,
    fs: &NtfsFileSystem,
    parent_mft: u64,
    mft_entry_num: u64,
    name: &str,
) -> Result<(), NtfsError> {
    // 1. MFT entry oku
    let (mut entry, raw_data) = read_mft_entry_raw(storage, fs, mft_entry_num)?;

    // Link count azalt
    if entry.link_count <= 1 {
        // 2. Link count 0 — entry'yi ve data'yı sil
        entry.link_count = 0;
        entry.signature = MftEntry::SIGNATURE_BAAD; // Silinmiş işareti

        // $DATA attribute cluster'larını free et
        for attr in &entry.attributes {
            if attr.attr_type == ATTR_DATA && attr.non_resident {
                if let AttributeContent::NonResident { data_runs, .. } = &attr.content {
                    let mut prev_lcn: i64 = 0;
                    for run in data_runs {
                        let lcn = prev_lcn + run.lcn;
                        prev_lcn = lcn;

                        if lcn >= 0 {
                            for i in 0..run.length {
                                let cluster = (lcn as u64) + i;
                                update_bitmap_bit(storage, fs, MFT_BITMAP, cluster, false)?;
                            }
                        }
                    }
                }
            }
        }

        // MFT entry'yi free et
        update_bitmap_bit(storage, fs, MFT_BITMAP, mft_entry_num, false)?;

        // MFT entry'yi yaz
        let serialized = serialize_mft_entry(&entry, fs.mft_entry_size as usize);
        write_mft_entry_raw(storage, fs, mft_entry_num, &serialized)?;
    } else {
        // Link count > 1 — sadece count azalt
        entry.link_count -= 1;
        let serialized = serialize_mft_entry(&entry, fs.mft_entry_size as usize);
        write_mft_entry_raw(storage, fs, mft_entry_num, &serialized)?;
    }

    // 3. Parent dizinden index entry'yi kaldır
    remove_index_entry(storage, fs, parent_mft, name)?;

    Ok(())
}

/// Parent dizinden index entry'yi kaldır
fn remove_index_entry(
    storage: &NtfsStorage,
    fs: &NtfsFileSystem,
    parent_mft: u64,
    name: &str,
) -> Result<(), NtfsError> {
    let (mut entry, _raw_data) = read_mft_entry_raw(storage, fs, parent_mft)?;

    // $INDEX_ROOT attribute bul
    let idx_root_idx = entry
        .attributes
        .iter()
        .position(|a| a.attr_type == ATTR_INDEX_ROOT)
        .ok_or(NtfsError::NotFound)?;

    let idx_root_attr = &entry.attributes[idx_root_idx];

    if !idx_root_attr.non_resident {
        if let AttributeContent::Resident { .. } = &idx_root_attr.content {
            let mut idx_data = idx_root_attr
                .get_resident_data()
                .ok_or(NtfsError::Corrupted)?
                .to_vec();

            // Index entry'leri tara ve silinecek olanı bul
            if idx_data.len() < 16 {
                return Err(NtfsError::Corrupted);
            }
            let entries_offset =
                u32::from_le_bytes([idx_data[0], idx_data[1], idx_data[2], idx_data[3]]) as usize;

            let mut pos = entries_offset;
            let mut found = false;
            let mut entry_len = 0usize;

            while pos + 16 <= idx_data.len() {
                let elen = u16::from_le_bytes([idx_data[pos + 8], idx_data[pos + 9]]) as usize;
                if elen == 0 || elen > idx_data.len() - pos {
                    break;
                }

                let flags = idx_data[pos + 12];
                if (flags & INDEX_ENTRY_END) != 0 {
                    break;
                }

                // $FILE_NAME parse
                let stream_len =
                    u16::from_le_bytes([idx_data[pos + 10], idx_data[pos + 11]]) as usize;
                if stream_len >= 66 && pos + 16 + 64 + 1 <= idx_data.len() {
                    let name_len = idx_data[pos + 16 + 64] as usize;
                    let name_start = pos + 16 + 66;
                    if name_start + name_len * 2 <= idx_data.len() {
                        let entry_name_bytes = &idx_data[name_start..name_start + name_len * 2];
                        if let Ok(entry_name) = String::from_utf16(
                            &entry_name_bytes
                                .chunks_exact(2)
                                .map(|w| u16::from_le_bytes([w[0], w[1]]))
                                .collect::<Vec<u16>>(),
                        ) {
                            if ntfs_name_matches(&entry_name, name) {
                                found = true;
                                entry_len = elen;
                                break;
                            }
                        }
                    }
                }

                pos += elen;
            }

            if found {
                // Entry'yi sil (sonraki entry'leri kaydır)
                idx_data.drain(pos..pos + entry_len);

                // Index node header güncelle
                let new_entries_length = idx_data.len() - entries_offset;
                idx_data[4..8].copy_from_slice(&(new_entries_length as u32).to_le_bytes());

                // Attribute güncelle
                if let AttributeContent::Resident { data_length, .. } =
                    &mut entry.attributes[idx_root_idx].content
                {
                    *data_length = idx_data.len() as u32;
                }
                entry.attributes[idx_root_idx].resident_data = idx_data;

                // MFT entry'yi yaz
                let serialized = serialize_mft_entry(&entry, fs.mft_entry_size as usize);
                write_mft_entry_raw(storage, fs, parent_mft, &serialized)?;
            }
        }
    }

    Ok(())
}

/// NTFS reparse point detection — UnsupportedFeature olarak reddet
///
/// $REPARSE_POINT attribute (0xC0) varsa, bu bir symlink/junction/ mount point.
/// echOS bunları implement etmediği için explicit UnsupportedFeature döner.
pub fn check_reparse_point(
    storage: &NtfsStorage,
    fs: &NtfsFileSystem,
    mft_entry_num: u64,
) -> Result<bool, NtfsError> {
    let (entry, _raw) = read_mft_entry_raw(storage, fs, mft_entry_num)?;

    let has_reparse = entry
        .attributes
        .iter()
        .any(|a| a.attr_type == ATTR_REPARSE_POINT);

    Ok(has_reparse)
}

fn normalize_file_reference(reference: u64) -> u64 {
    reference & 0x0000_FFFF_FFFF_FFFF
}

fn ntfs_name_matches(left: &str, right: &str) -> bool {
    left == right || left.eq_ignore_ascii_case(right)
}

fn file_type_from_filename_attr(attr: &FileNameAttr) -> NtfsFileType {
    if (attr.flags & 0x10000000) != 0 {
        NtfsFileType::Directory
    } else if (attr.flags & 0x00000004) != 0 {
        NtfsFileType::System
    } else if (attr.flags & 0x00000002) != 0 {
        NtfsFileType::Hidden
    } else if (attr.flags & 0x00000800) != 0 {
        NtfsFileType::Compressed
    } else if (attr.flags & 0x00004000) != 0 {
        NtfsFileType::Encrypted
    } else if (attr.flags & 0x00000200) != 0 {
        NtfsFileType::Sparse
    } else if (attr.flags & 0x00000020) != 0 {
        NtfsFileType::Archive
    } else {
        NtfsFileType::File
    }
}
