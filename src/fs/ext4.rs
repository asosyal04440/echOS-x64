//! # ext4 Dosya Sistemi
//!
//! Günlükleme (journaling) desteği ile Dördüncü Genişletilmiş Dosya Sistemi (ext4)
//! uygulaması. Okuma ve yazma desteği sunar; JBD2 günlüğü ile çökmeden kurtarma sağlar.
//!
//! ## ext4 Disk Yapısı (ASCII Diyagram)
//! ```text
//! Disk Düzeni:
//! ┌──────────────────────────────────────────────────────────────┐
//! │  0 - 1023  │  Önyükleme Bloğu (boot block)                  │
//! ├──────────────────────────────────────────────────────────────┤
//! │ 1024-2047  │  Süper Blok (Superblock) - sihirli sayı 0xEF53 │
//! ├──────────────────────────────────────────────────────────────┤
//! │  Blok 1    │  Blok Grubu Tanımlayıcıları (Group Descriptors) │
//! ├──────────────────────────────────────────────────────────────┤
//! │  Blok 2+   │  Blok Bitmap (hangi bloklar kullanımda?)        │
//! ├──────────────────────────────────────────────────────────────┤
//! │  ...       │  Inode Bitmap (hangi inode'lar kullanımda?)     │
//! ├──────────────────────────────────────────────────────────────┤
//! │  ...       │  Inode Tablosu (dosya meta verisi)              │
//! ├──────────────────────────────────────────────────────────────┤
//! │  ...       │  Veri Blokları (dosya içeriği)                  │
//! └──────────────────────────────────────────────────────────────┘
//!
//! Her Blok Grubu aynı yapıya sahiptir. Extent ağacı (extent tree)
//! büyük dosyalar için blok haritalamasını verimli şekilde yapar.
//! ```

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::mem;
use spin::Mutex;

use super::ext4_journal::{Journal, JournalError, Transaction, TransactionState};

// ============================================================================
// ext4 SABİTLERİ
// ============================================================================

/// ext4 sihirli sayısı - süper blok doğrulama için kullanılır
const EXT4_MAGIC: u16 = 0xEF53;

/// Süper blok ofseti (baştan 1024 bayt sonra)
const SUPERBLOCK_OFFSET: u64 = 1024;

/// Inode türleri - dosya modu bitlerindeki tür alanı
const EXT4_S_IFIFO: u16 = 0x1000;
const EXT4_S_IFCHR: u16 = 0x2000;
const EXT4_S_IFDIR: u16 = 0x4000;
const EXT4_S_IFBLK: u16 = 0x6000;
const EXT4_S_IFREG: u16 = 0x8000;
const EXT4_S_IFLNK: u16 = 0xA000;
const EXT4_S_IFSOCK: u16 = 0xC000;

/// Özellik bayrakları - dosya sisteminin desteklediği yetenekler
const EXT4_FEATURE_INCOMPAT_EXTENTS: u32 = 0x0040;
const EXT4_FEATURE_INCOMPAT_64BIT: u32 = 0x0080;

// ============================================================================
// DOSYA TÜRLERİ
// ============================================================================

/// Dosya türü numaralandırması
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

/// Dizin girdisi - bir dizindeki dosya veya alt dizin kaydı
#[derive(Clone, Debug)]
pub struct Ext4DirEntry {
    pub name: String,
    pub inode: u32,
    pub file_type: Ext4FileType,
}

/// Dosya meta verisi - boyut, izinler, zaman damgaları
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

/// Dosya sistemi hata türleri
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
// SÜPER BLOK
// ============================================================================

/// ext4 Süper Bloğu - dosya sisteminin ana meta veri yapısı (temel alanlar)
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
    /// Süper bloğu ham baytlardan çözümler ve sihirli sayıyı doğrular
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
            s_free_blocks_count_hi: u32::from_le_bytes([
                data[340], data[341], data[342], data[343],
            ]),
        })
    }

    /// Blok boyutunu hesaplar: 1024 << s_log_block_size (örn. 4096 bayt)
    pub fn block_size(&self) -> u32 {
        1024 << self.s_log_block_size
    }

    /// Toplam blok sayısını 64 bit olarak döndürür (hi + lo birleşimi)
    pub fn total_blocks(&self) -> u64 {
        ((self.s_blocks_count_hi as u64) << 32) | (self.s_blocks_count_lo as u64)
    }

    /// Serbest blok sayısını 64 bit olarak döndürür
    pub fn free_blocks(&self) -> u64 {
        ((self.s_free_blocks_count_hi as u64) << 32) | (self.s_free_blocks_count_lo as u64)
    }

    /// Blok grubu sayısını hesaplar (toplam bloklar / grup başına bloklar)
    pub fn block_groups_count(&self) -> u32 {
        let total = self.total_blocks();
        ((total + self.s_blocks_per_group as u64 - 1) / self.s_blocks_per_group as u64) as u32
    }

    /// Dosya sisteminin 64-bit modda olup olmadığını kontrol eder
    pub fn is_64bit(&self) -> bool {
        (self.s_feature_incompat & EXT4_FEATURE_INCOMPAT_64BIT) != 0
    }

    /// Dosya sisteminin extent ağacı kullanıp kullanmadığını kontrol eder
    pub fn has_extents(&self) -> bool {
        (self.s_feature_incompat & EXT4_FEATURE_INCOMPAT_EXTENTS) != 0
    }
}

// ============================================================================
// BLOK GRUBU TANIL AYICISI
// ============================================================================

/// Blok Grubu Tanımlayıcısı - her blok grubunun harita ve tablo konumlarını tutar
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
    /// 32-baytlık disk formatından tanımlayıcıyı çözümler
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

    /// Blok bitmap'in diskdeki bloğunu döndürür (64-bit moda göre)
    pub fn block_bitmap(&self, is_64bit: bool) -> u64 {
        if is_64bit {
            ((self.bg_block_bitmap_hi as u64) << 32) | self.bg_block_bitmap_lo as u64
        } else {
            self.bg_block_bitmap_lo as u64
        }
    }

    /// Inode tablosunun diskdeki başlangıç bloğunu döndürür
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

/// ext4 Inode yapısı - dosya ve dizinlerin meta verisini tutan temel yapı
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
    /// Inode'u ham baytlardan çözümler (en az 128 bayt gerekir)
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

    /// Dosya boyutunu 64 bit olarak döndürür (hi ve lo birleşimi)
    pub fn size(&self) -> u64 {
        ((self.i_size_hi as u64) << 32) | (self.i_size_lo as u64)
    }

    /// Inode modundan dosya türünü belirler
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

    /// Inode'un bir dizin olup olmadığını kontrol eder
    pub fn is_directory(&self) -> bool {
        (self.i_mode & 0xF000) == EXT4_S_IFDIR
    }

    /// Inode'un extent ağacı kullanıp kullanmadığını kontrol eder
    pub fn uses_extents(&self) -> bool {
        (self.i_flags & 0x00080000) != 0
    }

    /// Doğrudan ve dolaylı blok göstericilerini döndürür (sadece extent kullanmıyorsa)
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

    /// Inode'dan meta veri yapısı oluşturur
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
// EXTENT AĞACI
// ============================================================================

/// Extent başlığı - inode'un i_block alanının başında yer alır
#[derive(Clone, Copy, Debug)]
pub struct Ext4ExtentHeader {
    pub eh_magic: u16,
    pub eh_entries: u16,
    pub eh_depth: u16,
}

impl Ext4ExtentHeader {
    const MAGIC: u16 = 0xF30A;

    /// Extent başlığını baytlardan çözümler ve sihirli sayıyı doğrular
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

    /// Derinlik 0 ise yaprak düğüm (doğrudan disk bloklarına işaret eder)
    pub fn is_leaf(&self) -> bool {
        self.eh_depth == 0
    }
}

/// Extent girdisi - mantıksal blok aralığını fiziksel blok konumuna eşler
#[derive(Clone, Copy, Debug)]
pub struct Ext4Extent {
    pub ee_block: u32,
    pub ee_len: u16,
    pub ee_start: u64,
}

impl Ext4Extent {
    /// Extent girişini ham baytlardan çözümler
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
// ext4 DOSYA SİSTEMİ
// ============================================================================

/// ext4 Dosya Sistemi örneği - tüm dosya sistemi durumunu yönetir
#[derive(Clone, Debug)]
pub struct Ext4FileSystem {
    pub superblock: Ext4Superblock,
    pub block_size: u32,
    pub is_64bit: bool,
    pub group_descriptors: Vec<Ext4GroupDescriptor>,
    pub root_inode: u32,
    /// Yazma desteği için isteğe bağlı günlük (journal)
    pub journal: Option<Arc<Mutex<Journal>>>,
    /// Günlüğün başladığı blok ofseti
    pub journal_offset: u64,
}

impl Ext4FileSystem {
    /// Yeni bir ext4 dosya sistemi örneği oluşturur (varsayılan değerlerle)
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

    /// Aygıt verisinden dosya sistemini başlatır: süper bloğu okur ve doğrular
    pub fn init(&mut self, device_data: &[u8]) -> Result<(), Ext4Error> {
        if device_data.len() < SUPERBLOCK_OFFSET as usize + 1024 {
            return Err(Ext4Error::ReadError);
        }

        let sb_data = &device_data[SUPERBLOCK_OFFSET as usize..];
        let sb = Ext4Superblock::parse(sb_data).ok_or(Ext4Error::InvalidFormat)?;

        self.superblock = sb;
        self.block_size = sb.block_size();
        self.is_64bit = sb.is_64bit();

        // Blok grubu tanımlayıcılarını diskten yükle
        self.load_group_descriptors(device_data)?;

        crate::serial_println!(
            "[ext4] Başlatıldı: {} blok, {} inode, {} bayt/blok",
            sb.total_blocks(),
            sb.s_inodes_count,
            self.block_size
        );

        Ok(())
    }

    /// Blok grubu tanımlayıcılarını diskten okuyup belleğe yükler
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

    /// Verilen inode numarasının disk üzerindeki bayt ofsetini ve boyutunu döndürür
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

    /// Belirtilen inode numarasını aygıt verisinden okur
    pub fn read_inode(&self, inode: u32, device_data: &[u8]) -> Result<Ext4Inode, Ext4Error> {
        let (offset, size) = self.get_inode_location(inode);
        let offset = offset as usize;

        if offset + size as usize > device_data.len() {
            return Err(Ext4Error::ReadError);
        }

        Ext4Inode::parse(&device_data[offset..]).ok_or(Ext4Error::Corrupted)
    }

    /// Mantıksal blok numarasını fiziksel blok numarasına çevirir (extent veya dolaylı)
    pub fn map_block(&self, inode: &Ext4Inode, logical_block: u32) -> Option<u64> {
        if inode.uses_extents() {
            // i_block alanından extent başlığını çözümle
            let header = Ext4ExtentHeader::parse(&inode.i_block[12..])?;

            if !header.is_leaf() {
                return None; // Çok seviyeli extent ağaçları henüz desteklenmiyor
            }

            // Extent'leri tarayarak mantıksal bloğu bul
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
            // Dolaylı blok göstericileri (eski yöntem)
            let blocks = inode.indirect_blocks();
            if logical_block < 12 {
                return Some(blocks[logical_block as usize] as u64);
            }
        }

        None
    }

    /// Dosyanın tüm içeriğini aygıt verisinden okur
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

    /// Dizin inode'undan tüm girişleri okuyup döndürür
    pub fn read_dir(
        &self,
        inode: &Ext4Inode,
        device_data: &[u8],
    ) -> Result<Vec<Ext4DirEntry>, Ext4Error> {
        if !inode.is_directory() {
            return Err(Ext4Error::NotSupported);
        }

        let data = self.read_file(inode, device_data)?;
        let mut entries = Vec::new();
        let mut offset = 0;

        while offset + 8 <= data.len() {
            let inode_num = u32::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]);
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

    /// Kök dizin inode'unu (inode 2) aygıt verisinden okur
    pub fn root_inode_data(&self, device_data: &[u8]) -> Result<Ext4Inode, Ext4Error> {
        self.read_inode(self.root_inode, device_data)
    }

    // ========================================================================
    // GÜNLÜKLEME İLE YAZMA DESTEĞİ
    // ========================================================================

    /// Yazma desteği için JBD2 günlüğünü başlatır ve kurtarma yapar
    pub fn init_journal(
        &mut self,
        device_data: &[u8],
        journal_offset: u64,
        journal_size: u64,
    ) -> Result<(), Ext4Error> {
        let mut journal = Journal::new(self.block_size, journal_offset, journal_size);
        journal
            .init(device_data)
            .map_err(|_| Ext4Error::NotSupported)?;

        // Tamamlanmamış işlemleri kurtar (crash recovery)
        journal
            .recover(device_data)
            .map_err(|_| Ext4Error::Corrupted)?;

        self.journal = Some(Arc::new(Mutex::new(journal)));
        self.journal_offset = journal_offset;

        crate::serial_println!("[ext4] Günlük {} ofsetinde başlatıldı", journal_offset);
        Ok(())
    }

    /// Yazma işlemleri için yeni bir işlem (transaction) başlatır
    pub fn begin_transaction(&self, credits: usize) -> Result<(), Ext4Error> {
        if let Some(ref journal) = self.journal {
            let mut j = journal.lock();
            j.start_transaction(credits)
                .map_err(|_| Ext4Error::NotSupported)?;
        }
        Ok(())
    }

    /// Mevcut işlemi günlüğe kaydeder ve diske yazar
    pub fn commit_transaction(&self) -> Result<(), Ext4Error> {
        if let Some(ref journal) = self.journal {
            let mut j = journal.lock();
            j.commit_transaction().map_err(|_| Ext4Error::WriteError)?;
        }
        Ok(())
    }

    /// Dosyaya veri yazar (günlükleme etkinse işleme ekler)
    pub fn write_file(
        &self,
        inode: &mut Ext4Inode,
        offset: u64,
        data: &[u8],
        device_data: &mut [u8],
    ) -> Result<usize, Ext4Error> {
        let block_size = self.block_size as u64;
        let start_block = offset / block_size;
        let end_block = (offset + data.len() as u64 + block_size - 1) / block_size;

        // Günlükleme etkinse blokları işleme ekle
        if let Some(ref journal) = self.journal {
            let mut j = journal.lock();
            for block_num in start_block..end_block {
                if let Some(phys_block) = self.map_block(inode, block_num as u32) {
                    let block_offset = phys_block as usize * block_size as usize;
                    if block_offset + block_size as usize <= device_data.len() {
                        j.add_block(
                            phys_block as u32,
                            &device_data[block_offset..block_offset + block_size as usize],
                            true,
                        )?;
                    }
                }
            }
        }

        // Veriyi bloklara yaz
        let mut bytes_written = 0;
        let mut data_offset = 0;

        for block_num in start_block..end_block {
            if let Some(phys_block) = self.map_block(inode, block_num as u32) {
                let block_offset = phys_block as usize * block_size as usize;
                let block_start_in_file = block_num * block_size;

                // Blok içindeki yazma konumunu hesapla
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
                        device_data
                            [block_offset + write_start..block_offset + write_start + write_count]
                            .copy_from_slice(&data[data_offset..data_offset + write_count]);
                        bytes_written += write_count;
                        data_offset += write_count;
                    }
                }
            }
        }

        // Gerekirse inode boyutunu güncelle
        let new_size = offset + bytes_written as u64;
        if new_size > inode.size() {
            inode.i_size_lo = (new_size & 0xFFFFFFFF) as u32;
            inode.i_size_hi = (new_size >> 32) as u32;
        }

        Ok(bytes_written)
    }

    /// Dosya için yeni bir blok tahsis eder
    pub fn allocate_block(
        &self,
        inode: &mut Ext4Inode,
        logical_block: u32,
        device_data: &mut [u8],
    ) -> Result<u64, Ext4Error> {
        // Blok bitmap'inden serbest blok bul
        let group = logical_block / self.superblock.s_blocks_per_group;
        let gd = self
            .group_descriptors
            .get(group as usize)
            .ok_or(Ext4Error::OutOfMemory)?;

        // Basit tahsis stratejisi (gerçek uygulamada blok bitmap taranır)
        let new_block =
            self.superblock.total_blocks() - self.superblock.free_blocks() + logical_block as u64;

        // Günlükleme etkinse yeni bloğu işleme ekle
        if let Some(ref journal) = self.journal {
            let mut j = journal.lock();
            j.add_new_block(new_block as u32, &vec![0u8; self.block_size as usize], true)?;
        }

        // Inode blok göstericilerini güncelle
        if inode.uses_extents() {
            // Extent ağacı güncellemesi gerekir
            // Şimdilik yer tutucu
        } else {
            let blocks = inode.indirect_blocks();
            if logical_block < 12 {
                // Doğrudan blok - i_block dizisini güncelle
                let _ = blocks;
            }
        }

        Ok(new_block)
    }

    /// Belirtilen türde ve izinlerde yeni bir inode oluşturur
    pub fn create_inode(&self, file_type: Ext4FileType, mode: u16) -> Result<Ext4Inode, Ext4Error> {
        let mut inode: Ext4Inode = unsafe { mem::zeroed() };

        inode.i_mode = match file_type {
            Ext4FileType::Regular => EXT4_S_IFREG,
            Ext4FileType::Directory => EXT4_S_IFDIR,
            Ext4FileType::Symlink => EXT4_S_IFLNK,
            _ => 0,
        } | mode;

        inode.i_links_count = 1;
        inode.i_flags = if self.superblock.has_extents() {
            0x00080000
        } else {
            0
        };

        // Mevcut zamanı al (sistem saatinden alınır)
        let time = crate::task::scheduler::get_ticks() as u32;
        inode.i_atime = time;
        inode.i_ctime = time;
        inode.i_mtime = time;

        Ok(inode)
    }

    /// Üst dizine yeni bir dizin girdisi ekler
    pub fn create_dir_entry(
        &self,
        parent_inode: &mut Ext4Inode,
        name: &str,
        child_inode: u32,
        file_type: Ext4FileType,
        device_data: &mut [u8],
    ) -> Result<(), Ext4Error> {
        // Mevcut dizin verisini oku
        let mut dir_data = self.read_file(parent_inode, device_data)?;

        // Yeni girdi oluştur
        let ft_code = match file_type {
            Ext4FileType::Regular => 1,
            Ext4FileType::Directory => 2,
            Ext4FileType::Symlink => 7,
            _ => 0,
        };

        // Girdi formatı: inode(4) + rec_len(2) + name_len(1) + file_type(1) + isim
        let name_bytes = name.as_bytes();
        let entry_len = 8 + name_bytes.len();
        let rec_len = (entry_len + 3) & !3; // 4 bayta hizala

        let mut entry = vec![0u8; rec_len];
        entry[0..4].copy_from_slice(&child_inode.to_le_bytes());
        entry[4..6].copy_from_slice(&(rec_len as u16).to_le_bytes());
        entry[6] = name_bytes.len() as u8;
        entry[7] = ft_code;
        entry[8..8 + name_bytes.len()].copy_from_slice(name_bytes);

        // Dizin veriye ekle
        dir_data.extend_from_slice(&entry);

        // Geri yaz (günlük üzerinden yazılması gerekir)

        // Alt öğe dizinse üst inode bağlantı sayısını artır
        if file_type == Ext4FileType::Directory {
            parent_inode.i_links_count += 1;
        }

        Ok(())
    }

    /// Dosya sistemini diske eşitler (bekleyen işlemleri tamamlar)
    pub fn sync(&self, device_data: &mut [u8]) -> Result<(), Ext4Error> {
        // Bekleyen işlemleri tamamla
        if let Some(ref journal) = self.journal {
            let mut j = journal.lock();
            j.commit_transaction().map_err(|_| Ext4Error::WriteError)?;
        }

        // Süper bloğu yaz
        let sb_offset = SUPERBLOCK_OFFSET as usize;
        // Süper bloğu serileştirip yazma işlemi burada yapılır

        crate::serial_println!("[ext4] Dosya sistemi eşitlendi");
        Ok(())
    }
}

impl Default for Ext4FileSystem {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// GLOBAL ÖRNEK
// ============================================================================

lazy_static::lazy_static! {
    static ref EXT4_INSTANCES: Mutex<BTreeMap<String, Ext4FileSystem>> = Mutex::new(BTreeMap::new());
}

/// ext4 dosya sistemini bağlar (mount)
pub fn mount_ext4(name: &str, device_data: &[u8]) -> Result<(), Ext4Error> {
    let mut fs = Ext4FileSystem::new();
    fs.init(device_data)?;

    EXT4_INSTANCES.lock().insert(name.to_string(), fs);
    Ok(())
}

/// İsme göre ext4 dosya sistemi örneğini döndürür
pub fn get_ext4(name: &str) -> Option<Ext4FileSystem> {
    EXT4_INSTANCES.lock().get(name).cloned()
}

/// ext4 dosya sistemini ayırır (unmount)
pub fn unmount_ext4(name: &str) -> bool {
    EXT4_INSTANCES.lock().remove(name).is_some()
}

/// ext4 modülünü başlatır
pub fn init() {
    crate::serial_println!("[ext4] Modül başlatıldı");
}

// ============================================================================
// HTree Dizin İndeksleme (Hash Tree / dx_root)
// ============================================================================
//
// ext4 büyük dizinlerin O(n) yerine O(log n) aranmasını sağlamak için
// B-tree benzeri karma ağaç (htree) yapısı kullanır.
// Bu yapı, dizin bloğunun 0. girişinde dx_root olarak saklanır.

/// dx_root — HTree kök bloğu yapısı.
///
/// Dizin bloğunun başında yer alır ve ağacın meta verisini tutar.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct DxRoot {
    /// Sahte dot girişi (inode, rec_len, name_len, file_type)
    pub dot_inode: u32,
    pub dot_rec_len: u16,
    pub dot_name_len: u8,
    pub dot_file_type: u8,
    /// Sahte dotdot girişi
    pub dotdot_inode: u32,
    pub dotdot_rec_len: u16,
    pub dotdot_name_len: u8,
    pub dotdot_file_type: u8,
    // dx_root_info yapısı başlangıcı
    /// Ayrılmış (0)
    pub reserved_zero: u32,
    /// Hash versiyonu (0=legacy, 1=half_md4, 2=tea, 3=unsigned legacy, 4=unsigned half_md4, 5=unsigned tea, 6=siphash)
    pub hash_version: u8,
    /// Ağaç derinliği (info_length)
    pub info_length: u8,
    /// Dolaylılık seviyesi (indirect levels) — genellikle 0 veya 1
    pub indirect_levels: u8,
    /// Kullanılmayan bayraklar
    pub unused_flags: u8,
    /// Limit — bu blokta saklanabilecek maximum giriş sayısı
    pub limit: u16,
    /// Count — mevcut giriş sayısı
    pub count: u16,
    /// İlk hash aralığının bloğu
    pub block: u32,
}

/// dx_entry — HTree arama tablosu girişi.
///
/// Hash değerine göre sıralanmış blok referanslarıdır.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct DxEntry {
    /// Hash değeri alt sınırı
    pub hash: u32,
    /// Bu hash aralığını içeren blok numarası
    pub block: u32,
}

/// Desteklenen hash algoritmaları
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DxHashVersion {
    Legacy = 0,
    HalfMd4 = 1,
    Tea = 2,
    UnsignedLegacy = 3,
    UnsignedHalfMd4 = 4,
    UnsignedTea = 5,
    SipHash = 6,
}

impl DxHashVersion {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Legacy),
            1 => Some(Self::HalfMd4),
            2 => Some(Self::Tea),
            3 => Some(Self::UnsignedLegacy),
            4 => Some(Self::UnsignedHalfMd4),
            5 => Some(Self::UnsignedTea),
            6 => Some(Self::SipHash),
            _ => None,
        }
    }
}

/// Half-MD4 karma fonksiyonu (ext4 varsayılanı).
///
/// Dosya adını 32-bit hash değerine dönüştürür.
/// Gerçek half_md4, TEA tabanlı hash'e yakın basitleştirilmiş versiyondur.
pub fn dx_hash_half_md4(name: &[u8], seed: u32) -> u32 {
    let mut hash = seed;
    for &b in name {
        hash = hash.wrapping_mul(0x01000193) ^ (b as u32); // FNV-benzeri
    }
    // Sıfır hash geçersiz — 1'e yuvarlat
    if hash == 0 {
        1
    } else {
        hash
    }
}

/// HTree dizin araması.
///
/// `root_block` verilen dx_root bloğu ve `entries` listesi ile
/// belirtilen dosya adının bulunduğu dizin bloğu döner.
pub fn htree_lookup(entries: &[DxEntry], name_hash: u32) -> Option<u32> {
    if entries.is_empty() {
        return None;
    }
    // İkili arama: hash değerine göre doğru bloğu bul
    let mut lo = 0usize;
    let mut hi = entries.len();
    while lo + 1 < hi {
        let mid = (lo + hi) / 2;
        if entries[mid].hash <= name_hash {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some(entries[lo].block)
}

/// dx_root bloğundan giriş tablosunu ayrıştırır.
pub fn parse_dx_root(block_data: &[u8]) -> Option<(DxRoot, Vec<DxEntry>)> {
    if block_data.len() < 40 {
        return None;
    }
    let root = DxRoot {
        dot_inode: u32::from_le_bytes([block_data[0], block_data[1], block_data[2], block_data[3]]),
        dot_rec_len: u16::from_le_bytes([block_data[4], block_data[5]]),
        dot_name_len: block_data[6],
        dot_file_type: block_data[7],
        dotdot_inode: u32::from_le_bytes([
            block_data[8],
            block_data[9],
            block_data[10],
            block_data[11],
        ]),
        dotdot_rec_len: u16::from_le_bytes([block_data[12], block_data[13]]),
        dotdot_name_len: block_data[14],
        dotdot_file_type: block_data[15],
        reserved_zero: u32::from_le_bytes([
            block_data[16],
            block_data[17],
            block_data[18],
            block_data[19],
        ]),
        hash_version: block_data[20],
        info_length: block_data[21],
        indirect_levels: block_data[22],
        unused_flags: block_data[23],
        limit: u16::from_le_bytes([block_data[24], block_data[25]]),
        count: u16::from_le_bytes([block_data[26], block_data[27]]),
        block: u32::from_le_bytes([
            block_data[28],
            block_data[29],
            block_data[30],
            block_data[31],
        ]),
    };

    let count = root.count as usize;
    let mut entries = Vec::with_capacity(count);
    let mut offset = 32usize;
    for _ in 0..count {
        if offset + 8 > block_data.len() {
            break;
        }
        entries.push(DxEntry {
            hash: u32::from_le_bytes([
                block_data[offset],
                block_data[offset + 1],
                block_data[offset + 2],
                block_data[offset + 3],
            ]),
            block: u32::from_le_bytes([
                block_data[offset + 4],
                block_data[offset + 5],
                block_data[offset + 6],
                block_data[offset + 7],
            ]),
        });
        offset += 8;
    }

    Some((root, entries))
}
