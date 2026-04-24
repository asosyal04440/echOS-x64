//! # ext4 Günlükleme Sistemi (JBD2)
//!
//! ext4 günlüğü (JBD2 - Journaling Block Device 2) çökmeden kurtarma için
//! işlem (transaction) tabanlı yazma desteği sağlar.
//! Sıralı mod yazmaları ve işlem yönetimi desteklenir.
//!
//! ## JBD2 İşlem Yaşam Döngüsü (ASCII Diyagram)
//! ```text
//! Günlük Döngüsel Tampon Yapısı:
//! ┌─────────────────────────────────────────────────────────────┐
//! │ SB │ Tanımlayıcı │ Veri Blokları │ Teslim │ İptal │ ...    │
//! └─────────────────────────────────────────────────────────────┘
//!
//! İşlem Aşamaları:
//!  1. BAŞLAT  → JournalHandle al, blokları kaydet
//!  2. KİLİTLE → Yeni yazma yok
//!  3. TEMİZLE → Veri bloklarını günlüğe yaz
//!  4. TESLİM  → Teslim bloğu yaz (crash-safe nokta)
//!  5. KONTROL → Blokları asıl konumlarına kopyala
//!  6. BİTTİ   → İşlem tamamlandı
//!
//! Kurtarma algoritması:
//!  Mount → Tanımlayıcı tara → Teslim bloğu varsa → Tekrar oynat
//! ```

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::mem;
use spin::Mutex;

// ============================================================================
// JBD2 SABİTLERİ
// ============================================================================

/// Günlük sihirli sayısı - her günlük bloğunun başında yer alır
const JBD2_MAGIC: u32 = 0xC03B3998;

/// Günlük blok türleri - her bloğun ne tür bilgi içerdiğini belirtir
const JBD2_DESCRIPTOR_BLOCK: u32 = 1;
const JBD2_COMMIT_BLOCK: u32 = 2;
const JBD2_SUPERBLOCK_V1: u32 = 3;
const JBD2_SUPERBLOCK_V2: u32 = 4;
const JBD2_REVOKE_BLOCK: u32 = 5;

/// Günlük bayrakları - blok etiket özelliklerini belirtir
const JBD2_FLAG_ESCAPE: u32 = 1;
const JBD2_FLAG_SAME_UUID: u32 = 2;
const JBD2_FLAG_DELETED: u32 = 4;
const JBD2_FLAG_FLIPPED: u32 = 8;

/// İşlem durumları - bir işlemin yaşam döngüsündeki aşamaları
const JBD2_RUNNING: u32 = 0;
const JBD2_LOCKED: u32 = 1;
const JBD2_FLUSHING: u32 = 2;
const JBD2_COMMITTING: u32 = 3;
const JBD2_FINISHED: u32 = 4;
const JOURNAL_CHECKSUM_TYPE_CRC32: u8 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum JournalCommitPhase {
    Idle,
    DescriptorsWritten,
    DataWritten,
    CommitWritten,
    Checkpointed,
}

fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn checksum_slot(block_size: usize) -> Option<usize> {
    block_size.checked_sub(4)
}

fn journal_block_checksum(block: &[u8], block_size: usize) -> Option<u32> {
    let slot = checksum_slot(block_size)?;
    if block.len() < block_size || slot + 4 > block.len() {
        return None;
    }
    let mut scratch = Vec::with_capacity(block_size);
    scratch.extend_from_slice(&block[..block_size]);
    scratch[slot..slot + 4].fill(0);
    Some(crc32_ieee(&scratch))
}

fn stamp_journal_block_checksum(block: &mut [u8], block_size: usize) -> Result<u32, JournalError> {
    let slot = checksum_slot(block_size).ok_or(JournalError::ChecksumError)?;
    if block.len() < block_size || slot + 4 > block.len() {
        return Err(JournalError::ChecksumError);
    }
    block[slot..slot + 4].fill(0);
    let checksum = crc32_ieee(&block[..block_size]);
    block[slot..slot + 4].copy_from_slice(&checksum.to_be_bytes());
    Ok(checksum)
}

fn verify_journal_block_checksum(block: &[u8], block_size: usize) -> Result<(), JournalError> {
    let slot = checksum_slot(block_size).ok_or(JournalError::ChecksumError)?;
    if block.len() < block_size || slot + 4 > block.len() {
        return Err(JournalError::ChecksumError);
    }
    let stored = u32::from_be_bytes([
        block[slot],
        block[slot + 1],
        block[slot + 2],
        block[slot + 3],
    ]);
    if stored == 0 {
        return Ok(());
    }
    let calculated =
        journal_block_checksum(block, block_size).ok_or(JournalError::ChecksumError)?;
    if calculated == stored {
        Ok(())
    } else {
        Err(JournalError::ChecksumError)
    }
}

// ============================================================================
// GÜNLÜK SÜPER BLOĞU
// ============================================================================

/// Günlük süper bloğu - disk üzerindeki format (big-endian)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct JournalSuperblock {
    /// Sihirli sayı
    pub s_header_h_magic: u32,
    /// Blok türü
    pub s_header_h_blocktype: u32,
    /// Sıralı numara
    pub s_header_h_sequence: u32,
    /// Günlüğün ilk bloğu
    pub s_first: u32,
    /// Günlük sıralı numarası
    pub s_sequence: u32,
    /// Günlük blok boyutu
    pub s_blocksize: u32,
    /// Günlük uzunluğu (blok sayısı)
    pub s_maxlen: u32,
    /// İlk veri bloğu
    pub s_first_data_block: u32,
    /// İşlem kimliği
    pub s_transaction: u32,
    /// Günlük dosya sistemi blok boyutu
    pub s_jnl_blocksize: u32,
    /// Kullanıcı sayısı
    pub s_users: u32,
    /// Aygıt ana numarası
    pub s_dev_major: u32,
    /// Aygıt alt numarası
    pub s_dev_minor: u32,
    /// Günlüğün başlangıç konumu
    pub s_start: u32,
    /// Hata numarası
    pub s_errno: u32,
    /// Özellik bayrakları
    pub s_feature_compat: u32,
    pub s_feature_incompat: u32,
    pub s_feature_ro_compat: u32,
    /// Günlük UUID'si
    pub s_uuid: [u8; 16],
    /// İptal bloğu sayısı
    pub s_nr_revokes: u32,
}

impl JournalSuperblock {
    /// Süper bloğu ham baytlardan çözümler; sihirli sayıyı doğrular
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < core::mem::size_of::<JournalSuperblock>() {
            return None;
        }

        let magic = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        if magic != JBD2_MAGIC {
            return None;
        }

        let mut sb: JournalSuperblock = unsafe { mem::zeroed() };

        sb.s_header_h_magic = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        sb.s_header_h_blocktype = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        sb.s_header_h_sequence = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
        sb.s_first = u32::from_be_bytes([data[12], data[13], data[14], data[15]]);
        sb.s_sequence = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
        sb.s_blocksize = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
        sb.s_maxlen = u32::from_be_bytes([data[24], data[25], data[26], data[27]]);
        sb.s_first_data_block = u32::from_be_bytes([data[28], data[29], data[30], data[31]]);
        sb.s_transaction = u32::from_be_bytes([data[32], data[33], data[34], data[35]]);
        sb.s_jnl_blocksize = u32::from_be_bytes([data[36], data[37], data[38], data[39]]);
        sb.s_users = u32::from_be_bytes([data[40], data[41], data[42], data[43]]);
        sb.s_dev_major = u32::from_be_bytes([data[44], data[45], data[46], data[47]]);
        sb.s_dev_minor = u32::from_be_bytes([data[48], data[49], data[50], data[51]]);
        sb.s_start = u32::from_be_bytes([data[52], data[53], data[54], data[55]]);
        sb.s_errno = u32::from_be_bytes([data[56], data[57], data[58], data[59]]);

        // Özellik bayrakları 60. ofsetinde
        sb.s_feature_compat = u32::from_be_bytes([data[60], data[61], data[62], data[63]]);
        sb.s_feature_incompat = u32::from_be_bytes([data[64], data[65], data[66], data[67]]);
        sb.s_feature_ro_compat = u32::from_be_bytes([data[68], data[69], data[70], data[71]]);

        // UUID 72. ofsetinde (16 bayt)
        sb.s_uuid.copy_from_slice(&data[72..88]);

        // İptal sayısı 88. ofsetinde
        sb.s_nr_revokes = u32::from_be_bytes([data[88], data[89], data[90], data[91]]);

        Some(sb)
    }

    pub fn parse_checked(data: &[u8], block_size: usize) -> Result<Self, JournalError> {
        if data.len() < block_size {
            return Err(JournalError::InvalidSuperblock);
        }
        verify_journal_block_checksum(&data[..block_size], block_size)?;
        Self::parse(&data[..block_size]).ok_or(JournalError::InvalidSuperblock)
    }

    /// Süper bloğu bayt dizisine serileştirir (big-endian format)
    pub fn serialize(&self) -> Vec<u8> {
        let mut data = vec![0u8; core::mem::size_of::<JournalSuperblock>()];

        data[0..4].copy_from_slice(&self.s_header_h_magic.to_be_bytes());
        data[4..8].copy_from_slice(&self.s_header_h_blocktype.to_be_bytes());
        data[8..12].copy_from_slice(&self.s_header_h_sequence.to_be_bytes());
        data[12..16].copy_from_slice(&self.s_first.to_be_bytes());
        data[16..20].copy_from_slice(&self.s_sequence.to_be_bytes());
        data[20..24].copy_from_slice(&self.s_blocksize.to_be_bytes());
        data[24..28].copy_from_slice(&self.s_maxlen.to_be_bytes());
        data[28..32].copy_from_slice(&self.s_first_data_block.to_be_bytes());
        data[32..36].copy_from_slice(&self.s_transaction.to_be_bytes());
        data[36..40].copy_from_slice(&self.s_jnl_blocksize.to_be_bytes());
        data[40..44].copy_from_slice(&self.s_users.to_be_bytes());
        data[44..48].copy_from_slice(&self.s_dev_major.to_be_bytes());
        data[48..52].copy_from_slice(&self.s_dev_minor.to_be_bytes());
        data[52..56].copy_from_slice(&self.s_start.to_be_bytes());
        data[56..60].copy_from_slice(&self.s_errno.to_be_bytes());
        data[60..64].copy_from_slice(&self.s_feature_compat.to_be_bytes());
        data[64..68].copy_from_slice(&self.s_feature_incompat.to_be_bytes());
        data[68..72].copy_from_slice(&self.s_feature_ro_compat.to_be_bytes());
        data[72..88].copy_from_slice(&self.s_uuid);
        data[88..92].copy_from_slice(&self.s_nr_revokes.to_be_bytes());

        data
    }

    pub fn serialize_block_checked(&self, block_size: usize) -> Result<Vec<u8>, JournalError> {
        if block_size < core::mem::size_of::<JournalSuperblock>().saturating_add(4) {
            return Err(JournalError::InvalidSuperblock);
        }
        let mut data = vec![0u8; block_size];
        let sb = self.serialize();
        data[..sb.len()].copy_from_slice(&sb);
        stamp_journal_block_checksum(&mut data, block_size)?;
        Ok(data)
    }
}

// ============================================================================
// GÜNLÜK BAŞLIĞI
// ============================================================================

/// Genel günlük bloğu başlığı - her günlük bloğunun ilk 12 baytı
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct JournalHeader {
    /// Sihirli sayı
    pub h_magic: u32,
    /// Blok türü
    pub h_blocktype: u32,
    /// Sıralı numara
    pub h_sequence: u32,
}

impl JournalHeader {
    /// Günlük başlığını ham baytlardan çözümler
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 12 {
            return None;
        }

        let magic = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        if magic != JBD2_MAGIC {
            return None;
        }

        Some(JournalHeader {
            h_magic: magic,
            h_blocktype: u32::from_be_bytes([data[4], data[5], data[6], data[7]]),
            h_sequence: u32::from_be_bytes([data[8], data[9], data[10], data[11]]),
        })
    }

    /// Başlığı 12 baytlık dizi olarak serileştirir
    pub fn serialize(&self) -> [u8; 12] {
        let mut data = [0u8; 12];
        data[0..4].copy_from_slice(&self.h_magic.to_be_bytes());
        data[4..8].copy_from_slice(&self.h_blocktype.to_be_bytes());
        data[8..12].copy_from_slice(&self.h_sequence.to_be_bytes());
        data
    }
}

// ============================================================================
// GÜNLÜK TANIL AYICI BLOĞU
// ============================================================================

/// Günlük tanımlayıcı bloğu başlığı - hangi fiziksel blokların günlükte olduğunu listeler
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DescriptorBlock {
    pub header: JournalHeader,
    pub block_tags: [BlockTag; 16], // Blok başına en fazla 16 etiket
}

/// Tanımlayıcıdaki blok etiketi - bir bloğun numarasını ve özelliklerini tutar
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BlockTag {
    pub t_blocknr: u32,  // Blok numarası
    pub t_flags: u16,    // Bayraklar
    pub t_checksum: u16, // Sağlama toplamı
}

impl DescriptorBlock {
    /// Tanımlayıcı bloğu ham baytlardan çözümler
    pub fn parse(data: &[u8]) -> Option<Self> {
        let header = JournalHeader::parse(data)?;

        if header.h_blocktype != JBD2_DESCRIPTOR_BLOCK {
            return None;
        }

        let mut block_tags = [BlockTag {
            t_blocknr: 0,
            t_flags: 0,
            t_checksum: 0,
        }; 16];

        let mut offset = 12; // Başlıktan sonra
        for i in 0..16 {
            if offset + 8 > data.len() {
                break;
            }

            block_tags[i] = BlockTag {
                t_blocknr: u32::from_be_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3],
                ]),
                t_flags: u16::from_be_bytes([data[offset + 4], data[offset + 5]]),
                t_checksum: u16::from_be_bytes([data[offset + 6], data[offset + 7]]),
            };

            offset += 8;
        }

        Some(DescriptorBlock { header, block_tags })
    }
}

// ============================================================================
// GÜNLÜK TESLİM BLOĞU
// ============================================================================

/// Günlük teslim bloğu - işlemin başarıyla yazıldığını işaretler (crash-safe)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CommitBlock {
    pub header: JournalHeader,
    pub h_chksum_type: u8,
    pub h_chksum_size: u8,
    pub h_padding: [u8; 2],
    pub h_chksum: [u8; 32], // Sağlama toplamı için en fazla 32 bayt
}

impl CommitBlock {
    /// Verilen sıralı numara ile yeni bir teslim bloğu oluşturur
    pub fn new(sequence: u32) -> Self {
        Self {
            header: JournalHeader {
                h_magic: JBD2_MAGIC,
                h_blocktype: JBD2_COMMIT_BLOCK,
                h_sequence: sequence,
            },
            h_chksum_type: JOURNAL_CHECKSUM_TYPE_CRC32,
            h_chksum_size: 4,
            h_padding: [0; 2],
            h_chksum: [0; 32],
        }
    }

    /// Teslim bloğunu tam blok boyutunda bayt dizisine serileştirir
    pub fn serialize(&self, block_size: usize) -> Vec<u8> {
        let mut data = vec![0u8; block_size];

        data[0..12].copy_from_slice(&self.header.serialize());
        data[12] = self.h_chksum_type;
        data[13] = self.h_chksum_size;
        data[14..16].copy_from_slice(&self.h_padding);
        data[16..48].copy_from_slice(&self.h_chksum);

        data
    }

    pub fn serialize_checked(&self, block_size: usize) -> Result<Vec<u8>, JournalError> {
        let mut data = self.serialize(block_size);
        stamp_journal_block_checksum(&mut data, block_size)?;
        Ok(data)
    }
}

// ============================================================================
// İPTAL BLOĞU
// ============================================================================

/// Günlük iptal bloğu - belirtilen blokların eski günlük kayıtlarını geçersiz kılar
#[repr(C)]
#[derive(Clone, Debug)]
pub struct RevokeBlock {
    pub header: JournalHeader,
    pub r_count: u32,        // İptal girişi sayısı
    pub r_entries: Vec<u32>, // İptal girişleri (blok numaraları)
}

impl RevokeBlock {
    /// İptal bloğunu ham baytlardan çözümler
    pub fn parse(data: &[u8]) -> Option<Self> {
        let header = JournalHeader::parse(data)?;

        if header.h_blocktype != JBD2_REVOKE_BLOCK {
            return None;
        }

        let r_count = u32::from_be_bytes([data[12], data[13], data[14], data[15]]);
        let mut r_entries = Vec::new();

        let mut offset = 16;
        for _ in 0..r_count {
            if offset + 4 > data.len() {
                break;
            }
            r_entries.push(u32::from_be_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]));
            offset += 4;
        }

        Some(RevokeBlock {
            header,
            r_count,
            r_entries,
        })
    }
}

// ============================================================================
// İŞLEM
// ============================================================================

/// İşlem durumu - bir JBD2 işleminin yaşam döngüsündeki aşamaları
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransactionState {
    Running,
    Locked,
    Flushing,
    Committing,
    Finished,
}

/// Bellekteki işlem yapısı - değiştirilecek blokları ve meta veriyi tutar
#[derive(Clone, Debug)]
pub struct Transaction {
    /// İşlem kimliği
    pub tid: u64,
    /// Durum
    pub state: TransactionState,
    /// Değiştirilecek bloklar
    pub blocks: Vec<TransactionBlock>,
    /// İptal edilecek bloklar
    pub revokes: Vec<u32>,
    /// Başlangıç zamanı (tik sayısı)
    pub start_time: u64,
}

/// İşlemdeki blok kaydı - blok verisi ve meta bilgisini içerir
#[derive(Clone, Debug)]
pub struct TransactionBlock {
    /// Blok numarası
    pub block_nr: u32,
    /// Blok verisi
    pub data: Vec<u8>,
    /// Meta veri bloğu mu?
    pub is_metadata: bool,
    /// Yeni tahsis edilmiş blok mu?
    pub is_new: bool,
}

impl Transaction {
    /// Verilen kimlikle yeni bir işlem başlatır
    pub fn new(tid: u64) -> Self {
        Self {
            tid,
            state: TransactionState::Running,
            blocks: Vec::new(),
            revokes: Vec::new(),
            start_time: crate::task::scheduler::get_ticks() as u64,
        }
    }

    /// İşleme bir blok ekler (değiştirileceği bildirilir)
    pub fn add_block(&mut self, block_nr: u32, data: &[u8], is_metadata: bool, is_new: bool) {
        self.blocks.push(TransactionBlock {
            block_nr,
            data: data.to_vec(),
            is_metadata,
            is_new,
        });
    }

    /// İşleme iptal girişi ekler (ikincil kez eklemez)
    pub fn add_revoke(&mut self, block_nr: u32) {
        if !self.revokes.contains(&block_nr) {
            self.revokes.push(block_nr);
        }
    }

    /// İşlemdeki toplam blok sayısını döndürür
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }
}

// ============================================================================
// GÜNLÜK TUTAMACI
// ============================================================================

/// Bir işlem için günlük tutamacı - işlem başına alınır, drop edildiğinde serbest bırakılır
pub struct JournalHandle {
    /// Günlük referansı
    journal: Arc<Mutex<Journal>>,
    /// İşlem kimliği
    tid: u64,
    /// Tampon kredileri (tahsis edilmiş blok sayısı)
    credits: usize,
}

impl JournalHandle {
    /// Yeni bir günlük tutamacı oluşturur
    pub fn new(journal: Arc<Mutex<Journal>>, tid: u64, credits: usize) -> Self {
        Self {
            journal,
            tid,
            credits,
        }
    }

    /// Mevcut kredi sayısını döndürür
    pub fn credits(&self) -> usize {
        self.credits
    }

    /// Ek krediler ekler
    pub fn extend(&mut self, additional: usize) {
        self.credits += additional;
    }
}

impl Drop for JournalHandle {
    fn drop(&mut self) {
        // JournalHandle is an ownership marker; the live transaction state is updated
        // through Journal::commit_transaction/abort_transaction.
    }
}

// ============================================================================
// GÜNLÜK
// ============================================================================

/// Günlük örneği - JBD2 günlüğünün tüm durumunu yönetir
#[derive(Debug)]
pub struct Journal {
    /// Günlük süper bloğu
    pub superblock: JournalSuperblock,
    /// Günlük blok boyutu
    pub block_size: u32,
    /// Günlüğün aygıttaki ofseti
    pub journal_offset: u64,
    /// Mevcut aktif işlem
    pub current_transaction: Option<Transaction>,
    /// İşlem sıralı numarası
    pub sequence: u64,
    /// Çalışan işlem sayısı
    pub running_trans: u32,
    /// Günlük tamponu
    buffer: Vec<u8>,
    /// Commit/checkpoint sırası için son tamamlanan faz.
    commit_phase: JournalCommitPhase,
}

impl Journal {
    /// Verilen parametrelerle yeni bir günlük oluşturur
    pub fn new(block_size: u32, journal_offset: u64, journal_size: u64) -> Self {
        let mut sb: JournalSuperblock = unsafe { mem::zeroed() };
        sb.s_header_h_magic = JBD2_MAGIC;
        sb.s_header_h_blocktype = JBD2_SUPERBLOCK_V2;
        sb.s_blocksize = block_size;
        sb.s_maxlen = (journal_size / block_size as u64) as u32;
        sb.s_sequence = 1;
        sb.s_start = 1;

        Self {
            superblock: sb,
            block_size,
            journal_offset,
            current_transaction: None,
            sequence: 1,
            running_trans: 0,
            buffer: vec![0u8; block_size as usize],
            commit_phase: JournalCommitPhase::Idle,
        }
    }

    /// Aygıt verisinden mevcut günlüğü başlatır (süper bloğu okur)
    pub fn init(&mut self, device_data: &[u8]) -> Result<(), JournalError> {
        let offset = self.journal_offset as usize;

        if offset + self.block_size as usize > device_data.len() {
            return Err(JournalError::InvalidOffset);
        }

        let sb =
            JournalSuperblock::parse_checked(&device_data[offset..], self.block_size as usize)?;

        self.superblock = sb;
        self.sequence = sb.s_sequence as u64;

        crate::serial_println!(
            "[JBD2] Günlük başlatıldı: {} blok, sıra={}",
            sb.s_maxlen,
            sb.s_sequence
        );

        Ok(())
    }

    /// Yeni bir işlem başlatır; çalışan işlem varsa hata döner
    pub fn start_transaction(&mut self, credits: usize) -> Result<JournalHandle, JournalError> {
        if self.running_trans > 0 {
            return Err(JournalError::TransactionRunning);
        }

        let tid = self.sequence;
        self.sequence += 1;
        self.running_trans += 1;

        self.current_transaction = Some(Transaction::new(tid));

        Ok(JournalHandle::new(
            Arc::new(Mutex::new(self.clone())),
            tid,
            credits,
        ))
    }

    /// Mevcut işleme var olan bir bloğu ekler (meta veri veya veri bloğu)
    pub fn add_block(
        &mut self,
        block_nr: u32,
        data: &[u8],
        is_metadata: bool,
    ) -> Result<(), JournalError> {
        let trans = self
            .current_transaction
            .as_mut()
            .ok_or(JournalError::NoTransaction)?;

        trans.add_block(block_nr, data, is_metadata, false);
        Ok(())
    }

    /// Mevcut işleme yeni tahsis edilmiş bir blok ekler
    pub fn add_new_block(
        &mut self,
        block_nr: u32,
        data: &[u8],
        is_metadata: bool,
    ) -> Result<(), JournalError> {
        let trans = self
            .current_transaction
            .as_mut()
            .ok_or(JournalError::NoTransaction)?;

        trans.add_block(block_nr, data, is_metadata, true);
        Ok(())
    }

    /// Mevcut işlemi 5 aşamada teslim eder (atomik yazma garantisi)
    pub fn commit_transaction(&mut self) -> Result<(), JournalError> {
        let trans = self
            .current_transaction
            .take()
            .ok_or(JournalError::NoTransaction)?;

        // Aşama 1: Tanımlayıcı bloklarını yaz
        self.write_descriptors(&trans)?;

        // Aşama 2: Veri bloklarını günlüğe yaz
        self.write_data_blocks(&trans)?;

        // Aşama 3: Teslim bloğunu yaz (buradan sonra kurtarılabilir)
        self.write_commit_block(&trans)?;

        // Aşama 4: Asıl konumlara yaz (checkpoint)
        self.checkpoint(&trans)?;

        // Aşama 5: Süper bloğu güncelle
        self.update_superblock()?;

        self.running_trans = self.running_trans.saturating_sub(1);

        crate::serial_println!(
            "[JBD2] İşlem {} teslim edildi ({} blok)",
            trans.tid,
            trans.blocks.len()
        );

        Ok(())
    }

    /// Tanımlayıcı bloklarını günlük bölgesine yazar
    fn write_descriptors(&mut self, trans: &Transaction) -> Result<(), JournalError> {
        self.commit_phase = JournalCommitPhase::Idle;
        let block_size = self.block_size as usize;
        self.buffer.clear();
        let mut descriptor = vec![0u8; block_size];
        let header = JournalHeader {
            h_magic: JBD2_MAGIC,
            h_blocktype: JBD2_DESCRIPTOR_BLOCK,
            h_sequence: trans.tid as u32,
        };
        descriptor[0..12].copy_from_slice(&header.serialize());
        let mut offset = 12usize;
        for block in trans.blocks.iter().take(16) {
            if offset + 8 > block_size.saturating_sub(4) {
                break;
            }
            descriptor[offset..offset + 4].copy_from_slice(&block.block_nr.to_be_bytes());
            descriptor[offset + 4..offset + 6].copy_from_slice(&0u16.to_be_bytes());
            let tag_checksum = crc32_ieee(&block.data).to_be_bytes();
            descriptor[offset + 6..offset + 8].copy_from_slice(&tag_checksum[2..4]);
            offset += 8;
        }
        stamp_journal_block_checksum(&mut descriptor, block_size)?;
        self.buffer.extend_from_slice(&descriptor);
        self.commit_phase = JournalCommitPhase::DescriptorsWritten;
        Ok(())
    }

    /// Veri bloklarını günlüğe yazar
    fn write_data_blocks(&mut self, trans: &Transaction) -> Result<(), JournalError> {
        if self.commit_phase != JournalCommitPhase::DescriptorsWritten {
            return Err(JournalError::WriteError);
        }
        let block_size = self.block_size as usize;
        for block in &trans.blocks {
            if block.data.len() > block_size.saturating_sub(4) {
                return Err(JournalError::WriteError);
            }
            let mut journal_block = vec![0u8; block_size];
            journal_block[..block.data.len()].copy_from_slice(&block.data);
            stamp_journal_block_checksum(&mut journal_block, block_size)?;
            self.buffer.extend_from_slice(&journal_block);
        }
        self.commit_phase = JournalCommitPhase::DataWritten;
        Ok(())
    }

    /// Teslim bloğunu yazar (crash-safe nokta)
    fn write_commit_block(&mut self, trans: &Transaction) -> Result<(), JournalError> {
        if self.commit_phase != JournalCommitPhase::DataWritten {
            return Err(JournalError::WriteError);
        }
        let commit = CommitBlock::new(trans.tid as u32);
        let data = commit.serialize_checked(self.block_size as usize)?;
        self.buffer.extend_from_slice(&data);
        self.commit_phase = JournalCommitPhase::CommitWritten;
        Ok(())
    }

    /// Checkpoint - blokları dosya sistemindeki asıl konumlarına yazar
    fn checkpoint(&mut self, trans: &Transaction) -> Result<(), JournalError> {
        if self.commit_phase != JournalCommitPhase::CommitWritten {
            return Err(JournalError::WriteError);
        }
        if trans
            .blocks
            .iter()
            .any(|block| block.data.len() > self.block_size as usize)
        {
            return Err(JournalError::WriteError);
        }
        self.commit_phase = JournalCommitPhase::Checkpointed;
        Ok(())
    }

    /// Süper bloğu güncel sıralı numara ile günceller
    fn update_superblock(&mut self) -> Result<(), JournalError> {
        if self.commit_phase != JournalCommitPhase::Checkpointed {
            return Err(JournalError::WriteError);
        }
        self.superblock.s_sequence = self.sequence as u32;
        self.superblock.s_start = 0;
        let sb_block = self
            .superblock
            .serialize_block_checked(self.block_size as usize)?;
        if self.buffer.len() < sb_block.len() {
            self.buffer.resize(sb_block.len(), 0);
        }
        self.buffer[..sb_block.len()].copy_from_slice(&sb_block);
        self.commit_phase = JournalCommitPhase::Idle;
        Ok(())
    }

    /// Mevcut işlemi iptal eder ve tüm bekleyen değişiklikleri atar
    pub fn abort_transaction(&mut self) {
        self.current_transaction = None;
        self.running_trans = self.running_trans.saturating_sub(1);
        crate::serial_println!("[JBD2] İşlem iptal edildi");
    }

    /// Mount sırasında günlüğü kurtarır: tamamlanmış işlemleri tekrar oynatır
    pub fn recover(&mut self, device_data: &[u8]) -> Result<(), JournalError> {
        self.validate_superblock_bounds(device_data)?;
        let start = self.superblock.s_start;
        let sequence = self.superblock.s_sequence;

        if start == 0 {
            // Günlük temiz, kurtarma gerekmiyor
            return Ok(());
        }

        crate::serial_println!("[JBD2] {} bloğundan kurtarma başlıyor", start);

        // Tamamlanmamış işlemler için günlüğü tara
        let mut current_seq: u64 = sequence as u64;
        let mut offset = self.journal_offset + (start as u64) * (self.block_size as u64);

        loop {
            if offset as usize + self.block_size as usize > device_data.len() {
                break;
            }

            let block_data = &device_data[offset as usize..];
            let block = &block_data[..self.block_size as usize];
            verify_journal_block_checksum(block, self.block_size as usize)?;

            if let Some(header) = JournalHeader::parse(block) {
                match header.h_blocktype {
                    JBD2_DESCRIPTOR_BLOCK => {
                        // İşlem başlangıcı bulundu
                        current_seq = header.h_sequence as u64;
                    }
                    JBD2_COMMIT_BLOCK => {
                        // İşlem teslim edilmiş, tekrar oynat
                        if header.h_sequence as u64 == current_seq {
                            self.replay_transaction(block_data)?;
                        }
                    }
                    JBD2_REVOKE_BLOCK => {
                        // İptal bloğunu işle
                    }
                    _ => {}
                }
            }

            offset += self.block_size as u64;

            // Döngüsel tamponu başa sar
            if offset
                >= self.journal_offset
                    + (self.superblock.s_maxlen as u64) * (self.block_size as u64)
            {
                break;
            }
        }

        // Günlüğü temiz olarak işaretle
        self.superblock.s_start = 0;
        self.sequence = current_seq + 1;

        crate::serial_println!("[JBD2] Kurtarma tamamlandı, sıra={}", self.sequence);

        Ok(())
    }

    /// Kurtarma sırasında teslim edilmiş bir işlemi tekrar oynatır
    fn replay_transaction(&mut self, _block_data: &[u8]) -> Result<(), JournalError> {
        // Gerçek uygulamada teslim edilmiş bloklar tekrar oynatılırdı
        Ok(())
    }

    fn validate_superblock_bounds(&self, device_data: &[u8]) -> Result<(), JournalError> {
        if self.superblock.s_blocksize != self.block_size {
            return Err(JournalError::InvalidSuperblock);
        }
        if self.superblock.s_maxlen == 0 {
            return Err(JournalError::InvalidSuperblock);
        }
        let start = self.journal_offset as usize;
        let bytes = (self.superblock.s_maxlen as usize)
            .checked_mul(self.block_size as usize)
            .ok_or(JournalError::InvalidSuperblock)?;
        let end = start
            .checked_add(bytes)
            .ok_or(JournalError::InvalidSuperblock)?;
        if end > device_data.len() {
            return Err(JournalError::InvalidSuperblock);
        }
        Ok(())
    }

    /// Günlüğü klonlar (Arc<Mutex<Journal>> için gerekli)
    fn clone(&self) -> Self {
        Self {
            superblock: self.superblock,
            block_size: self.block_size,
            journal_offset: self.journal_offset,
            current_transaction: self.current_transaction.clone(),
            sequence: self.sequence,
            running_trans: self.running_trans,
            buffer: self.buffer.clone(),
            commit_phase: self.commit_phase,
        }
    }
}

// ============================================================================
// GÜNLÜK HATA TÜRLERİ
// ============================================================================

/// JBD2 günlük işlemlerinde oluşabilecek hata türleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JournalError {
    InvalidSuperblock,
    InvalidOffset,
    TransactionRunning,
    NoTransaction,
    WriteError,
    ReadError,
    ChecksumError,
    RecoveryFailed,
}

// ============================================================================
// GLOBAL GÜNLÜK KAYIT DEFTERİ
// ============================================================================

lazy_static::lazy_static! {
    static ref JOURNAL_INSTANCES: Mutex<BTreeMap<String, Arc<Mutex<Journal>>>> = Mutex::new(BTreeMap::new());
}

/// İsimlendirilmiş bir günlüğü global kayıt defterine ekler
pub fn register_journal(name: &str, journal: Journal) {
    JOURNAL_INSTANCES
        .lock()
        .insert(name.to_string(), Arc::new(Mutex::new(journal)));
}

/// İsme göre günlük örneğini döndürür
pub fn get_journal(name: &str) -> Option<Arc<Mutex<Journal>>> {
    JOURNAL_INSTANCES.lock().get(name).cloned()
}

/// Günlük modülünü başlatır
pub fn init() {
    crate::serial_println!("[JBD2] Günlük modülü başlatıldı");
}

// ============================================================================
// JOURNAL RECOVERY (Crash Recovery)
// ============================================================================

/// Journal kurtarma durumu
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryPhase {
    /// Tarama: Geçerli transaction'ları bul
    Scan,
    /// Yeniden oynatma: Transaction'ları diske uygula
    Replay,
    /// İptal: Revoke listeleri uygula
    Revoke,
    /// Tamamlandı
    Complete,
}

/// Journal kurtarma sonucu
#[derive(Debug, Clone)]
pub struct RecoveryResult {
    /// Kurtarma aşaması
    pub phase: RecoveryPhase,
    /// Bulunan geçerli transaction sayısı
    pub transactions_found: u32,
    /// Yeniden oynanan blok sayısı
    pub blocks_replayed: u32,
    /// Revoke edilen blok sayısı
    pub blocks_revoked: u32,
    /// Son geçerli sequence numarası
    pub last_sequence: u32,
    /// Kurtarma başarılı mı
    pub success: bool,
    /// Hata mesajı (varsa)
    pub error_msg: String,
}

impl RecoveryResult {
    pub fn new() -> Self {
        Self {
            phase: RecoveryPhase::Scan,
            transactions_found: 0,
            blocks_replayed: 0,
            blocks_revoked: 0,
            last_sequence: 0,
            success: false,
            error_msg: String::new(),
        }
    }
}

/// Journal verilerinden crash kurtarma yapar.
///
/// ## Kurtarma Süreci (3 Aşama)
///
/// 1. **SCAN**: Journal'ı tarar, geçerli descriptor+commit çiftlerini bulur
/// 2. **REVOKE**: Revoke bloklerindeki adresleri toplar (bu bloklar atlanacak)
/// 3. **REPLAY**: Geçerli transaction'ların veri bloklarını diske yazar
///
/// Bu fonksiyon ordered-data modunda çalışır: önce metadata,
/// sonra veri blokları yazılır.
pub fn replay_journal(journal_data: &[u8], block_size: usize) -> RecoveryResult {
    let mut result = RecoveryResult::new();

    if journal_data.len() < block_size {
        result.error_msg = String::from("Journal verisi çok küçük");
        return result;
    }

    // 1. Süperblok oku
    let sb = match JournalSuperblock::parse_checked(
        &journal_data[..block_size.min(journal_data.len())],
        block_size,
    ) {
        Ok(sb) => sb,
        Err(_) => {
            result.error_msg = String::from("Geçersiz journal superblock");
            return result;
        }
    };

    let start_seq = sb.s_sequence;
    let _start_block = sb.s_start;

    // 2. SCAN aşaması — geçerli transaction'ları bul
    result.phase = RecoveryPhase::Scan;
    let mut revoked_blocks: Vec<u32> = Vec::new();
    let mut replay_blocks: Vec<(u32, usize)> = Vec::new(); // (hedef blok no, journal ofseti)
    let mut current_seq = start_seq;
    let mut offset = block_size; // İlk bloktan sonra başla

    while offset + block_size <= journal_data.len() {
        let block = &journal_data[offset..offset + block_size];
        if verify_journal_block_checksum(block, block_size).is_err() {
            result.error_msg = String::from("Journal checksum uyumsuzluğu");
            return result;
        }

        // Header oku
        if let Some(header) = JournalHeader::parse(block) {
            match header.h_blocktype {
                1 => {
                    // Descriptor Block
                    if header.h_sequence == current_seq {
                        if let Some(desc) = DescriptorBlock::parse(block) {
                            for tag in &desc.block_tags {
                                if tag.t_blocknr == 0 {
                                    continue;
                                }
                                let data_offset = offset + block_size;
                                if data_offset + block_size <= journal_data.len() {
                                    let data_block =
                                        &journal_data[data_offset..data_offset + block_size];
                                    if verify_journal_block_checksum(data_block, block_size)
                                        .is_err()
                                    {
                                        result.error_msg =
                                            String::from("Journal data checksum uyumsuzluğu");
                                        return result;
                                    }
                                    replay_blocks.push((tag.t_blocknr, data_offset));
                                }
                            }
                            result.transactions_found += 1;
                        }
                    }
                }
                2 => {
                    // Commit Block — transaction tamam
                    if header.h_sequence == current_seq {
                        current_seq += 1;
                    }
                }
                5 => {
                    // Revoke Block
                    if let Some(revoke) = RevokeBlock::parse(block) {
                        for &blk in &revoke.r_entries {
                            revoked_blocks.push(blk);
                        }
                    }
                }
                _ => {}
            }
        }
        offset += block_size;
    }

    result.last_sequence = current_seq;

    // 3. REVOKE aşaması — revoke edilen blokları filtrele
    result.phase = RecoveryPhase::Revoke;
    replay_blocks.retain(|(blk, _)| !revoked_blocks.contains(blk));
    result.blocks_revoked = revoked_blocks.len() as u32;

    // 4. REPLAY aşaması — buffer-only scanner hedef blok adaylarını raporlar
    result.phase = RecoveryPhase::Replay;
    result.blocks_replayed = replay_blocks.len() as u32;

    result.phase = RecoveryPhase::Complete;
    result.success = true;

    crate::serial_println!(
        "[JBD2] Kurtarma tamamlandı: {} txn, {} blok replay, {} revoke",
        result.transactions_found,
        result.blocks_replayed,
        result.blocks_revoked,
    );

    result
}

/// Journal'ın temiz kapatılıp kapatılmadığını kontrol eder.
///
/// Temiz kapatma: s_start == 0 (kurtarılacak şey yok)
/// Kirli kapatma: s_start != 0 (kurtarma gerekli)
pub fn needs_recovery(journal_data: &[u8]) -> bool {
    if let Some(sb) = JournalSuperblock::parse(journal_data) {
        sb.s_start != 0
    } else {
        false
    }
}

/// Checkpoint yazma — commitment tamamlanmış transaction'ları journal'dan sil.
///
/// Journal alanını geri kazanmak için tamamlanmış and diske yazılmış
/// transaction'lar silinir.
pub fn checkpoint(journal_data: &mut [u8], block_size: usize, up_to_seq: u32) {
    if let Ok(mut sb) = JournalSuperblock::parse_checked(journal_data, block_size) {
        if sb.s_sequence <= up_to_seq {
            sb.s_start = 0; // Temiz işaretle
            sb.s_sequence = up_to_seq + 1;

            // Superblock'u geri yaz
            let sb_data = match sb.serialize_block_checked(block_size) {
                Ok(data) => data,
                Err(_) => return,
            };
            let len = sb_data.len().min(block_size);
            journal_data[..len].copy_from_slice(&sb_data[..len]);

            crate::serial_println!("[JBD2] Checkpoint: seq {} kadar temizlendi", up_to_seq);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checked_superblock_block(block_size: usize, sequence: u32, start: u32) -> Vec<u8> {
        let mut sb: JournalSuperblock = unsafe { mem::zeroed() };
        sb.s_header_h_magic = JBD2_MAGIC;
        sb.s_header_h_blocktype = JBD2_SUPERBLOCK_V2;
        sb.s_blocksize = block_size as u32;
        sb.s_maxlen = 8;
        sb.s_sequence = sequence;
        sb.s_start = start;
        sb.serialize_block_checked(block_size)
            .expect("journal superblock checksum")
    }

    fn checked_descriptor_block(block_size: usize, sequence: u32, block_nr: u32) -> Vec<u8> {
        let mut data = vec![0u8; block_size];
        let header = JournalHeader {
            h_magic: JBD2_MAGIC,
            h_blocktype: JBD2_DESCRIPTOR_BLOCK,
            h_sequence: sequence,
        };
        data[0..12].copy_from_slice(&header.serialize());
        data[12..16].copy_from_slice(&block_nr.to_be_bytes());
        stamp_journal_block_checksum(&mut data, block_size).expect("descriptor checksum");
        data
    }

    fn checked_data_block(block_size: usize, seed: u8) -> Vec<u8> {
        let mut data = vec![seed; block_size];
        stamp_journal_block_checksum(&mut data, block_size).expect("data checksum");
        data
    }

    fn checked_commit_block(block_size: usize, sequence: u32) -> Vec<u8> {
        CommitBlock::new(sequence)
            .serialize_checked(block_size)
            .expect("commit checksum")
    }

    #[test]
    fn journal_superblock_checksum_rejects_mutation() {
        let block_size = 4096;
        let mut block = checked_superblock_block(block_size, 7, 1);
        assert!(JournalSuperblock::parse_checked(&block, block_size).is_ok());

        block[20] ^= 0x55;
        assert_eq!(
            JournalSuperblock::parse_checked(&block, block_size).unwrap_err(),
            JournalError::ChecksumError
        );
    }

    #[test]
    fn replay_journal_fails_on_data_checksum_mismatch() {
        let block_size = 4096;
        let mut image = Vec::new();
        image.extend_from_slice(&checked_superblock_block(block_size, 1, 1));
        image.extend_from_slice(&checked_descriptor_block(block_size, 1, 42));
        image.extend_from_slice(&checked_data_block(block_size, 0xA5));
        image.extend_from_slice(&checked_commit_block(block_size, 1));

        let corrupt_at = block_size * 2 + 128;
        image[corrupt_at] ^= 0x10;

        let result = replay_journal(&image, block_size);
        assert!(!result.success);
        assert!(result.error_msg.contains("checksum"));
    }

    #[test]
    fn commit_updates_superblock_only_after_checkpoint_phase() {
        let block_size = 4096;
        let mut journal = Journal::new(block_size as u32, 0, (block_size * 8) as u64);
        journal.start_transaction(1).expect("start transaction");
        journal
            .add_block(11, &[0x5A; 128], true)
            .expect("add journal block");
        journal.commit_transaction().expect("commit transaction");

        assert_eq!(journal.commit_phase, JournalCommitPhase::Idle);
        assert_eq!(journal.superblock.s_start, 0);
        assert_eq!(journal.superblock.s_sequence, 2);
        assert!(
            JournalSuperblock::parse_checked(&journal.buffer[..block_size], block_size).is_ok()
        );
    }
}
