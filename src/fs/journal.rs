//! # Dosya Sistemi Günlükleme (Journaling)
//!
//! Çökme tutarlılığı için işlem tabanlı günlükleme (journaling) sistemi.
//!
//! ## Write-Ahead Log (WAL) — Önceden Yazma Günlüğü Mekanizması
//!
//! ```
//! Uygulama
//!    │
//!    ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │  1. İŞLEM BAŞLA  (start_transaction)                           │
//! │     ┌──────────┐                                               │
//! │     │   TXN    │  bir işlem kimliği (TID) tahsis edilir        │
//! │     │  tid=N   │                                               │
//! │     └──────────┘                                               │
//! │         │                                                       │
//! │  2. GÜNLÜĞE YAZ  (write_descriptor + write_data_blocks)        │
//! │         │                                                       │
//! │         ▼                                                       │
//! │  ┌─────────────────────────────────────────────────────────┐   │
//! │  │          JOURNAL (Günlük Bölgesi — Disk)                │   │
//! │  │  ┌────────────┐  ┌────────────┐  ┌────────────────┐    │   │
//! │  │  │ Tanımlayıcı │  │ Veri Blok  │  │ Teslim Bloğu   │    │   │
//! │  │  │  (DESC)    │  │  (DATA)    │  │   (COMMIT)     │    │   │
//! │  │  └────────────┘  └────────────┘  └────────────────┘    │   │
//! │  └─────────────────────────────────────────────────────────┘   │
//! │         │                                                       │
//! │  3. TESLIM ET (commit_transaction)                              │
//! │         │   teslim bloğu yazılır → işlem kalıcıdır             │
//! │         ▼                                                       │
//! │  ┌─────────────────────────────────────────────────────────┐   │
//! │  │         GERÇEK DOSYA SİSTEMİ (Asıl Disk Konumları)      │   │
//! │  │  ┌──────────┐  ┌──────────┐  ┌──────────┐              │   │
//! │  │  │  Blok A  │  │  Blok B  │  │  Blok C  │  ← checkpoint│   │
//! │  │  └──────────┘  └──────────┘  └──────────┘              │   │
//! │  └─────────────────────────────────────────────────────────┘   │
//! │         │                                                       │
//! │  4. DENETLEME NOKTASI (checkpoint)                              │
//! │         │   günlük girdileri asıl konumlarına yazılır          │
//! │         ▼                                                       │
//! │  5. GÜNLÜK TEMİZLE  — günlük alanı geri kazanılır              │
//! └─────────────────────────────────────────────────────────────────┘
//!
//! ## Çökme Sonrası Kurtarma
//!
//!  Çökme anı:
//!  ┌──────────────────────────────────────────────────────────────┐
//!  │  [DESC][DATA]  ← teslim YOK → geri al (rollback)            │
//!  │  [DESC][DATA][COMMIT] ← teslim VAR → yeniden uygula (replay)│
//!  └──────────────────────────────────────────────────────────────┘
//!
//! Bu modül Linux'un JBD2 (Journaling Block Device 2) tasarımına dayanır
//! ve ext4 dosya sistemi tarafından kullanılır.

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::vec;
use alloc::boxed::Box;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU32, Ordering};
use spin::Mutex;

use crate::drivers::linux::BlockDevice;
use crate::drivers::linux::LinuxDriverError;

// ============================================================================
// GÜNLÜK SABİTLERİ
// ============================================================================

/// Günlük sihirli sayısı — geçerli bir JBD2 günlüğünü tanımlar
pub const JBD2_MAGIC_NUMBER: u32 = 0xC03B3998;
/// Günlük süper bloğu sürüm 1
pub const JBD2_SUPERBLOCK_V1: u32 = 1;
/// Günlük süper bloğu sürüm 2
pub const JBD2_SUPERBLOCK_V2: u32 = 2;

/// Günlük blok türleri — her blok bu türlerden birini taşır
pub const JBD2_DESCRIPTOR_BLOCK: u32 = 1;   // Tanımlayıcı blok: hangi fs bloklarının olduğunu listeler
pub const JBD2_COMMIT_BLOCK: u32 = 2;       // Teslim bloğu: işlemin başarıyla yazıldığını onaylar
pub const JBD2_SUPERBLOCK_V1_BLK: u32 = 3; // Süper blok v1
pub const JBD2_SUPERBLOCK_V2_BLK: u32 = 4; // Süper blok v2
pub const JBD2_REVOKE_BLOCK: u32 = 5;      // İptal bloğu: belirli blokların yazılmamasını sağlar

/// Günlük bayrakları — durum bitleri
pub const JBD2_FLAG_UNMOUNT: u32 = 0x001;     // Dosya sistemi söküldü (unmount)
pub const JBD2_FLAG_ABORT: u32 = 0x002;       // Günlük iptal edildi (hata durumu)
pub const JBD2_FLAG_ACK_ERR: u32 = 0x004;     // Hata onaylandı
pub const JBD2_FLAG_FLUSHED: u32 = 0x008;     // Veriler diske aktarıldı
pub const JBD2_FLAG_RECOVERY: u32 = 0x010;    // Kurtarma modunda
pub const JBD2_FLAG_SEQUENTIAL: u32 = 0x020;  // Sıralı yazma modu

/// İşlem başına maksimum boyut (1 GiB)
pub const JBD2_MAX_TRANSACTION_SIZE: u64 = 1024 * 1024 * 1024;

// ============================================================================
// GÜNLÜK SÜPER BLOĞU
// ============================================================================

/// JBD2 günlük süper bloğu — günlüğün tamamını tanımlayan meta veri yapısı.
///
/// Disk üzerinde günlüğün baş kısmında saklanır ve kurtarma sırasında
/// ilk okunan yapıdır.
///
/// Bellek düzeni (C uyumlu, repr(C)):
/// ```
/// Ofset  Alan
/// 0x00   header_magic   — 0xC03B3998 geçerliyse bu bir JBD2 günlüğüdür
/// 0x04   block_type     — süper blok türü
/// 0x08   sequence       — güncel sıra numarası
/// ...
/// ```
#[repr(C)]
pub struct JournalSuperblock {
    /// Sihirli sayı — günlüğün geçerliliğini doğrular
    pub header_magic: u32,
    /// Blok türü
    pub block_type: u32,
    /// Sıra numarası
    pub sequence: u32,
    /// Günlükteki toplam blok sayısı
    pub total_blocks: u32,
    /// Günlüğün ilk bloğu
    pub first_block: u32,
    /// Günlük blok boyutu (bayt)
    pub block_size: u32,
    /// Dolgu alanı (hizalama için)
    pub padding: [u32; 2],
    /// Maksimum eş zamanlı işlem sayısı
    pub max_trans: u32,
    /// İşlem başına maksimum veri bloğu
    pub max_trans_data: u32,
    /// Uyumlu özellik bayrakları (mount koşulu değil)
    pub feature_compat: u32,
    /// Uyumsuz özellik bayrakları (bunlar eksikse mount edilemez)
    pub feature_incompat: u32,
    /// Salt okunur uyumlu özellik bayrakları
    pub feature_ro_compat: u32,
    /// Günlük UUID (evrensel benzersiz kimlik, 128 bit)
    pub uuid: [u8; 16],
    /// Dosya sistemi blok boyutu
    pub fs_block_size: u32,
    /// Günlük bloğu başına dosya sistemi blok sayısı
    pub fs_blocks_per_journal: u32,
    /// Kullanıcı tanımlı başlangıç sırası
    pub start_sequence: u32,
    /// Kullanıcı tanımlı başlangıç bloğu
    pub start_block: u32,
    /// Hata numarası (son hata kodu)
    pub errno: u32,
    /// Hata kaynağı bilgisi
    pub feature_compat2: u32,
    /// Dolgu
    pub padding2: [u32; 44],
    /// Sağlama toplamı türü (crc32c gibi)
    pub checksum_type: u32,
    /// Dolgu
    pub padding3: [u32; 3],
    /// Günlükteki toplam log bloğu sayısı (64-bit)
    pub total_log_blocks: u64,
    /// Dolgu
    pub padding4: [u32; 46],
}

// ============================================================================
// GÜNLÜK BAŞLIĞI
// ============================================================================

/// Her günlük bloğunun başında yer alan ortak başlık yapısı.
/// block_type alanı bu bloğun tanımlayıcı mı, teslim mi, vb. olduğunu belirtir.
#[repr(C)]
pub struct JournalHeader {
    /// Sihirli sayı — geçerli günlük bloğunu tanımlar
    pub magic: u32,
    /// Blok türü (DESC / COMMIT / REVOKE / ...)
    pub block_type: u32,
    /// Bu bloğun ait olduğu işlemin sıra numarası
    pub sequence: u32,
}

// ============================================================================
// GÜNLÜK İŞLEMİ (TRANSACTION)
// ============================================================================

/// İşlem durumu — bir işlem yaşam döngüsü boyunca bu durumları geçer.
///
/// ```
/// Running → Locked → FlushSuspended → Committing → CommitRecord → Finished
///   │                                                                  │
///   └──────────────── kurtarma / hata durumunda ───────────────────────┘
/// ```
///
/// - Running      : Yeni bloklar eklenebilir
/// - Locked       : Yeni blok kabul edilmiyor, teslim hazırlığı başladı
/// - Committing   : Günlüğe yazılıyor
/// - CommitRecord : Teslim bloğu yazılıyor
/// - Finished     : Denetleme noktası (checkpoint) bekliyor
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransactionState {
    Running,
    Locked,
    FlushSuspended,
    Committing,
    CommitRecord,
    Finished,
}

/// Bir günlük işlemini temsil eder.
/// Birden fazla blok değişikliği atomik olarak gruplandırılır.
pub struct Transaction {
    /// İşlem kimliği — her işlem benzersiz bir TID alır
    pub tid: u64,
    /// Durum — dikkatli kilitlenme (Mutex) ile korunur
    pub state: Mutex<TransactionState>,
    /// Günlük sıra numarası
    pub sequence: AtomicU64,
    /// Bu işleme ait bloklar listesi
    pub blocks: Mutex<Vec<JournalBlock>>,
    /// Tampun kredisi — kaç blok daha eklenebileceğini sınırlar
    pub credits: AtomicU32,
    /// İşlemin başlangıç zamanı (tik sayısı)
    pub start_time: u64,
    /// Veri blok sayacı
    pub data_blocks: AtomicU64,
    /// İptal edilen bloklar — checkpoint sırasında yazılmaması gereken bloklar
    pub revoked: Mutex<Vec<u64>>,
}

impl Transaction {
    pub fn new(tid: u64) -> Self {
        Self {
            tid,
            state: Mutex::new(TransactionState::Running),
            sequence: AtomicU64::new(0),
            blocks: Mutex::new(Vec::new()),
            credits: AtomicU32::new(0),
            start_time: 0,
            data_blocks: AtomicU64::new(0),
            revoked: Mutex::new(Vec::new()),
        }
    }

    /// İşleme bir blok ekler — blok günlüğe yazılmak üzere kuyruğa alınır
    pub fn add_block(&self, block: JournalBlock) {
        self.blocks.lock().push(block);
        self.data_blocks.fetch_add(1, Ordering::Relaxed);
    }

    /// Bloğu iptal eder — bu blok checkpoint sırasında asıl konumuna yazılmaz
    pub fn revoke_block(&self, block_nr: u64) {
        self.revoked.lock().push(block_nr);
    }

    /// Bu işlemdeki blok sayısını döndürür
    pub fn block_count(&self) -> usize {
        self.blocks.lock().len()
    }
}

/// Günlükte saklanan tek bir blok kaydı.
/// Hem günlük konumu hem de asıl dosya sistemi konumu tutulur.
#[derive(Clone, Debug)]
pub struct JournalBlock {
    /// Dosya sistemindeki orijinal blok numarası
    pub fs_block: u64,
    /// Günlükteki karşılık blok numarası
    pub journal_block: u64,
    /// Blok verisi (genellikle 4096 bayt)
    pub data: Vec<u8>,
    /// Bu bir iptal (revoke) girdisi mi?
    pub is_revoke: bool,
    /// CRC sağlama toplamı — bütünlük doğrulaması için
    pub checksum: u32,
}

// ============================================================================
// CRC32C SAĞLAMA TOPLAMI
// ============================================================================

/// CRC32C (Castagnoli) — JBD2'nin kullandığı checksum algoritması.
/// Intel makinelerde PCLMULQDQ ile hızlandırılabilir; burada tablo tabanlı
/// yazılım implementasyonu kullanılıyor.
const CRC32C_TABLE: [u32; 256] = generate_crc32c_table();

const fn generate_crc32c_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0u32;
    while i < 256 {
        let mut crc = i;
        let mut j = 0;
        while j < 8 {
            let mask = if crc & 1 != 0 { 0x82F63B78 } else { 0 };
            crc = (crc >> 1) ^ mask;
            j += 1;
        }
        table[i as usize] = crc;
        i += 1;
    }
    table
}

/// Bir veri tamponunun CRC32C checksum'ını hesaplar
pub fn crc32c(data: &[u8]) -> u32 {
    crc32c_with_seed(data, 0xFFFFFFFF) ^ 0xFFFFFFFFu32
}

/// CRC32C raw (no final XOR) — ext4 metadata checksum chaining için
/// seed: başlangıç CRC değeri (~0 yani 0xFFFFFFFF ilk çağrı için)
/// Linux kernel'in crc32c_le(seed, data, len) fonksiyonu ile aynı davranış
pub fn crc32c_with_seed(data: &[u8], seed: u32) -> u32 {
    let mut crc = seed;
    for &byte in data {
        let idx = ((crc ^ byte as u32) & 0xFF) as usize;
        crc = (crc >> 8) ^ CRC32C_TABLE[idx];
    }
    crc
}

// ============================================================================
// DİSK ÜZERİ YAPILAR (JBD2 ON-DISK FORMAT)
// ============================================================================

/// JBD2 blok başlığı — tüm günlük blokları bu başlıkla başlar.
/// 12 bayt: magic(4) + blocktype(4) + sequence(4) — big-endian
#[repr(C, packed)]
struct DiskJournalHeader {
    h_magic: u32,
    h_blocktype: u32,
    h_sequence: u32,
}

/// JBD2 teslim (commit) blok başlığı
/// 48 bayt: header(12) + chksum_type(1) + chksum_size(1) + padding(2)
///           + chksum[8](32) + commit_sec(8) + commit_nsec(4)
#[repr(C, packed)]
struct DiskCommitHeader {
    h_magic: u32,
    h_blocktype: u32,
    h_sequence: u32,
    h_chksum_type: u8,
    h_chksum_size: u8,
    h_padding: [u8; 2],
    h_chksum: [u32; 8],
    h_commit_sec: u64,
    h_commit_nsec: u32,
}

/// JBD2 blok etiketi (tag3) — tanımlayıcı blokta her veri bloğu için
/// 16 bayt: blocknr(4) + flags(4) + blocknr_high(4) + checksum(4)
#[repr(C, packed)]
struct DiskBlockTag3 {
    t_blocknr: u32,
    t_flags: u32,
    t_blocknr_high: u32,
    t_checksum: u32,
}

/// JBD2 günlük süper blok kuyruk yapısı
#[repr(C, packed)]
struct DiskJournalTail {
    t_checksum: u32,
}

/// Big-endian u32 yaz
fn write_be32(buf: &mut [u8], offset: usize, val: u32) {
    buf[offset..offset + 4].copy_from_slice(&val.to_be_bytes());
}

/// Big-endian u64 yaz
fn write_be64(buf: &mut [u8], offset: usize, val: u64) {
    buf[offset..offset + 8].copy_from_slice(&val.to_be_bytes());
}

// ============================================================================
// GÜNLÜK (JOURNAL)
// ============================================================================

/// Ana günlük yapısı — bir blok cihazı üzerindeki günlüğü yönetir.
///
/// ```
/// Disk Üzerinde Günlük Dairesel Tampon:
///
///   start_block
///       │
///       ▼
///  ┌────┬────┬────┬────┬────┬────┬────┬────┐
///  │ S  │ T1 │ T1 │ T1 │ T2 │ T2 │    │    │
///  │up  │DESC│DATA│COM │DESC│COM │FREE│FREE│
///  └────┴────┴────┴────┴────┴────┴────┴────┘
///   [0]  [1]  [2]  [3]  [4]  [5]  [6]  [7]
///              ▲                        ▲
///          tail_seq               head_seq
///         (checkpoint            (en son
///          noktası)               yazılan)
/// ```
pub struct Journal {
    /// Günlük kimliği
    pub id: u64,
    /// Ait olduğu blok cihazının adresi
    pub device: u64,
    /// Günlüğün diskteki başlangıç bloğu
    pub start_block: u64,
    /// Toplam günlük bloğu sayısı
    pub total_blocks: AtomicU64,
    /// Blok boyutu (bayt)
    pub block_size: u32,
    /// Aktif işlem — aynı anda yalnızca bir işlem aktif olabilir
    pub current_transaction: Mutex<Option<Arc<Transaction>>>,
    /// Teslim edilmiş ama henüz checkpoint yapılmamış işlem kuyruğu
    pub transaction_queue: Mutex<VecDeque<Arc<Transaction>>>,
    /// Günlük başı sıra numarası (en son yazılan)
    pub head_sequence: AtomicU64,
    /// Günlük kuyruğu sıra numarası (checkpoint için bekleyen)
    pub tail_sequence: AtomicU64,
    /// Sonraki işlem kimliği sayacı
    pub next_tid: AtomicU64,
    /// Bayraklar (JBD2_FLAG_* sabitleri)
    pub flags: AtomicU32,
    /// Günlük iptal edildi mi? (kurtarılamaz hata)
    pub aborted: AtomicBool,
    /// Performans istatistikleri
    pub stats: Mutex<JournalStats>,
}

/// Günlük istatistikleri — performans izleme için
#[derive(Clone, Debug, Default)]
pub struct JournalStats {
    pub transactions: u64,   // Toplam başlatılan işlem sayısı
    pub blocks_written: u64, // Günlüğe yazılan toplam blok
    pub blocks_revoked: u64, // İptal edilen blok sayısı
    pub commits: u64,        // Başarılı teslim sayısı
    pub rollbacks: u64,      // Geri alınan işlem sayısı
}

impl Journal {
    pub fn new(id: u64, device: u64, start_block: u64, total_blocks: u64, block_size: u32) -> Self {
        Self {
            id,
            device,
            start_block,
            total_blocks: AtomicU64::new(total_blocks),
            block_size,
            current_transaction: Mutex::new(None),
            transaction_queue: Mutex::new(VecDeque::new()),
            head_sequence: AtomicU64::new(0),
            tail_sequence: AtomicU64::new(0),
            next_tid: AtomicU64::new(1),
            flags: AtomicU32::new(0),
            aborted: AtomicBool::new(false),
            stats: Mutex::new(JournalStats::default()),
        }
    }

    /// Yeni bir işlem başlatır.
    ///
    /// WAL protokolünde ilk adım: değişiklikler yapılmadan önce
    /// bir işlem açılır ve tüm blok değişiklikleri bu işlem altında toplanır.
    pub fn start_transaction(&self) -> Arc<Transaction> {
        let tid = self.next_tid.fetch_add(1, Ordering::SeqCst);
        let trans = Arc::new(Transaction::new(tid));
        trans.sequence.store(self.head_sequence.load(Ordering::SeqCst) + 1, Ordering::SeqCst);

        *self.current_transaction.lock() = Some(trans.clone());

        crate::serial_println!("[JOURNAL] Started transaction {}", tid);
        trans
    }

    /// Mevcut işlemi teslim eder (commit).
    ///
    /// Teslim adımları:
    /// 1. Durum → Committing
    /// 2. Tanımlayıcı bloğu yaz (hangi fs blokları var?)
    /// 3. Veri bloklarını yaz
    /// 4. Teslim bloğunu yaz (işlem artık kalıcıdır)
    /// 5. Sıra sayacını artır
    /// 6. Cihazı flush et (veriler kalıcı ortama ulaşsın)
    pub fn commit_transaction(&self, drive: &mut dyn BlockDevice) -> Result<(), JournalError> {
        let trans_opt = self.current_transaction.lock().take();

        if let Some(trans) = trans_opt {
            if self.aborted.load(Ordering::SeqCst) {
                return Err(JournalError::Aborted);
            }

            // Durum → teslim ediliyor
            *trans.state.lock() = TransactionState::Committing;

            // Tanımlayıcı bloğu yaz
            self.write_descriptor(drive, &trans)?;

            // Veri bloklarını yaz
            self.write_data_blocks(drive, &trans)?;

            // Teslim bloğunu yaz
            self.write_commit(drive, &trans)?;

            // Sıra sayacını ilerlet
            self.head_sequence.fetch_add(1, Ordering::SeqCst);

            // İşlemi kuyruğa ekle (checkpoint için)
            self.transaction_queue.lock().push_back(trans.clone());

            let mut stats = self.stats.lock();
            stats.transactions += 1;
            stats.commits += 1;
            stats.blocks_written += trans.block_count() as u64;

            *trans.state.lock() = TransactionState::Finished;

            crate::serial_println!("[JOURNAL] Committed transaction {} ({} blocks)",
                trans.tid, trans.block_count());

            return Ok(());
        }

        Err(JournalError::NoTransaction)
    }

    /// Tanımlayıcı bloğunu yazar.
    /// Bu blok, işlemdeki fs bloklarının listesini içerir.
    /// Kurtarma sırasında hangi blokların yeniden oynatılacağını belirler.
    ///
    /// Disk formatı (JBD2 descriptor block):
    ///   [journal_header_s] [journal_block_tag3_t]... [journal_block_tail]
    fn write_descriptor(&self, drive: &mut dyn BlockDevice, trans: &Transaction) -> Result<(), JournalError> {
        let blocks = trans.blocks.lock();
        let seq = trans.sequence.load(Ordering::SeqCst) as u32;
        let bs = self.block_size as usize;

        // Tanımlayıcı bloğunu oluştur
        let mut desc_buf = vec![0u8; bs];

        // Başlık: magic + blocktype + sequence (big-endian)
        write_be32(&mut desc_buf, 0, JBD2_MAGIC_NUMBER);
        write_be32(&mut desc_buf, 4, JBD2_DESCRIPTOR_BLOCK);
        write_be32(&mut desc_buf, 8, seq);

        // Her blok için bir journal_block_tag3_t ekle
        let tag_start = 12; // journal_header_s sonrası
        let mut tag_offset = tag_start;
        let tags_per_block = (bs - 12 - 4) / 16; // 4 = journal_block_tail boyutu

        let mut journal_offset = 0u64; // Günlük içindeki başlangıç ofseti (blok cinsinden)
        let mut desc_blocks_needed = 1;

        // Kaç tanımlayıcı bloğu gerektiğini hesapla
        let total_blocks = blocks.len();
        if total_blocks > 0 {
            desc_blocks_needed = (total_blocks + tags_per_block - 1) / tags_per_block;
        }

        let mut desc_block_idx = 0usize;
        let mut tag_in_desc = 0usize;

        for (i, block) in blocks.iter().enumerate() {
            if tag_in_desc == 0 && tag_offset > tag_start {
                // Önceki tanımlayıcı bloğunu yaz
                let tail_offset = bs - 4;
                let desc_crc = crc32c(&desc_buf[..tail_offset]);
                write_be32(&mut desc_buf, tail_offset, desc_crc);

                let journal_lba = (self.start_block + desc_block_idx as u64) as u32;
                drive.write_sectors(journal_lba, &desc_buf).map_err(|_| JournalError::IoError)?;

                desc_block_idx += 1;
                // Yeni tanımlayıcı bloğu
                desc_buf.fill(0);
                write_be32(&mut desc_buf, 0, JBD2_MAGIC_NUMBER);
                write_be32(&mut desc_buf, 4, JBD2_DESCRIPTOR_BLOCK);
                write_be32(&mut desc_buf, 8, seq);
                tag_offset = tag_start;
            }

            let is_last = (i == total_blocks - 1) && (desc_blocks_needed == 1 || tag_in_desc == tags_per_block - 1);

            // journal_block_tag3_t yaz
            let tag_buf = &mut desc_buf[tag_offset..tag_offset + 16];
            write_be32(tag_buf, 0, block.fs_block as u32);
            let mut flags = 0u32;
            if is_last {
                flags |= 8; // JBD2_FLAG_LAST_TAG
            }
            write_be32(tag_buf, 4, flags);
            write_be32(tag_buf, 8, (block.fs_block >> 32) as u32);
            // Veri bloğunun checksum'ı (seq + data üzerinden)
            let mut crc_input = Vec::with_capacity(4 + block.data.len());
            crc_input.extend_from_slice(&seq.to_be_bytes());
            crc_input.extend_from_slice(&block.data);
            let data_crc = crc32c(&crc_input);
            write_be32(tag_buf, 12, data_crc);

            tag_offset += 16;
            tag_in_desc += 1;
            journal_offset += 1;
        }

        // Son tanımlayıcı bloğu yaz (eğer veri varsa)
        if !blocks.is_empty() {
            let tail_offset = bs - 4;
            let desc_crc = crc32c(&desc_buf[..tail_offset]);
            write_be32(&mut desc_buf, tail_offset, desc_crc);

            let journal_lba = (self.start_block + desc_block_idx as u64) as u32;
            drive.write_sectors(journal_lba, &desc_buf).map_err(|_| JournalError::IoError)?;
        }

        Ok(())
    }

    /// Veri bloklarını günlüğe yazar.
    /// Her blok, checksum ile birlikte günlük bölgesine kopyalanır.
    fn write_data_blocks(&self, drive: &mut dyn BlockDevice, trans: &Transaction) -> Result<(), JournalError> {
        let blocks = trans.blocks.lock();
        let bs = self.block_size as usize;

        // Tanımlayıcı bloklarını hesapla
        let tags_per_block = (bs - 12 - 4) / 16;
        let total_blocks = blocks.len();
        let desc_blocks = if total_blocks > 0 {
            (total_blocks + tags_per_block - 1) / tags_per_block
        } else {
            0
        };

        // Veri blokları tanımlayıcılardan sonra başlar
        let mut data_offset = desc_blocks as u64;

        for block in blocks.iter() {
            let journal_lba = (self.start_block + data_offset) as u32;

            // Blok verisini doğrudan günlüğe yaz
            if block.data.len() == bs {
                drive.write_sectors(journal_lba, &block.data).map_err(|_| JournalError::IoError)?;
            } else if block.data.len() < bs {
                // Kısa blok: sıfır dolgulu tam blok yaz
                let mut padded = vec![0u8; bs];
                padded[..block.data.len()].copy_from_slice(&block.data);
                drive.write_sectors(journal_lba, &padded).map_err(|_| JournalError::IoError)?;
            } else {
                // Blok boyutundan büyük veri — hata
                return Err(JournalError::IoError);
            }

            data_offset += 1;
        }

        Ok(())
    }

    /// Teslim bloğunu yazar.
    /// Bu blok yazıldıktan sonra işlem KALICIDIR —
    /// sistem çökse bile kurtarma sırasında bu işlem yeniden uygulanır.
    ///
    /// Disk formatı (JBD2 commit block):
    ///   [commit_header_s] — 48 bayt
    fn write_commit(&self, drive: &mut dyn BlockDevice, trans: &Transaction) -> Result<(), JournalError> {
        let bs = self.block_size as usize;
        let mut commit_buf = vec![0u8; bs];
        let seq = trans.sequence.load(Ordering::SeqCst) as u32;

        // commit_header_s doldur
        write_be32(&mut commit_buf, 0, JBD2_MAGIC_NUMBER);
        write_be32(&mut commit_buf, 4, JBD2_COMMIT_BLOCK);
        write_be32(&mut commit_buf, 8, seq);
        commit_buf[12] = 4; // JBD2_CRC32C_CHKSUM
        commit_buf[13] = 4; // chksum_size
        // h_padding[2] zaten 0

        // Tüm işlem checksum'ı (descriptor + data blokları üzerinden)
        let blocks = trans.blocks.lock();
        let mut crc_input = Vec::with_capacity(4 * blocks.len());
        for block in blocks.iter() {
            crc_input.extend_from_slice(&block.checksum.to_be_bytes());
        }
        let commit_crc = crc32c(&crc_input);
        write_be32(&mut commit_buf, 16, commit_crc);
        // h_chksum[1..8] zaten 0

        // commit_sec ve commit_nsec (şu an 0 — gerçek zaman için RTC okunmalı)
        write_be64(&mut commit_buf, 40, 0);
        write_be32(&mut commit_buf, 48, 0);

        // Teslim bloğunu günlüğe yaz
        let tags_per_block = (bs - 12 - 4) / 16;
        let total_blocks = blocks.len();
        let desc_blocks = if total_blocks > 0 {
            (total_blocks + tags_per_block - 1) / tags_per_block
        } else {
            0
        };
        let data_blocks = total_blocks as u64;
        let commit_offset = desc_blocks as u64 + data_blocks;
        let journal_lba = (self.start_block + commit_offset) as u32;

        drive.write_sectors(journal_lba, &commit_buf).map_err(|_| JournalError::IoError)?;

        Ok(())
    }

    /// Denetleme noktası (checkpoint) — teslim edilmiş verileri asıl fs konumlarına yazar.
    ///
    /// ```
    /// Checkpoint akışı:
    ///   Kuyruk    : [TXN-1(Finished)] [TXN-2(Finished)] [TXN-3(Committing)]
    ///                     │                   │                   │
    ///                     ▼                   ▼                   ▼
    ///   Asıl FS : blokları yaz         blokları yaz        bekle (henüz hazır değil)
    ///                     │
    ///                     ▼
    ///   Günlük alanı serbest bırakıldı → dairesel tampon ilerledi
    /// ```
    pub fn checkpoint(&self, drive: &mut dyn BlockDevice) -> Result<(), JournalError> {
        let mut queue = self.transaction_queue.lock();
        let mut checkpointed = 0u64;
        let bs = self.block_size as usize;

        while let Some(trans) = queue.pop_front() {
            if *trans.state.lock() == TransactionState::Finished {
                // Blokları asıl dosya sistemi konumlarına yaz
                let blocks = trans.blocks.lock();
                for block in blocks.iter() {
                    if block.is_revoke {
                        // İptal bloğu — asıl konuma yazma
                        continue;
                    }

                    let fs_lba = block.fs_block as u32;
                    if block.data.len() == bs {
                        drive.write_sectors(fs_lba, &block.data).map_err(|_| JournalError::IoError)?;
                    } else if block.data.len() < bs {
                        // Mevcut bloğu oku, kısmi güncelleme yap
                        let existing = drive.read_sectors(fs_lba, (bs / 512) as u8);
                        if !existing.is_empty() && existing.len() == bs {
                            let mut merged = existing;
                            merged[..block.data.len()].copy_from_slice(&block.data);
                            drive.write_sectors(fs_lba, &merged).map_err(|_| JournalError::IoError)?;
                        } else {
                            // Okuma başarısız — doğrudan yaz (sıfır dolgulu)
                            let mut padded = vec![0u8; bs];
                            padded[..block.data.len()].copy_from_slice(&block.data);
                            drive.write_sectors(fs_lba, &padded).map_err(|_| JournalError::IoError)?;
                        }
                    }
                }
                drop(blocks);

                checkpointed += 1;
                let mut stats = self.stats.lock();
                stats.blocks_revoked += trans.revoked.lock().len() as u64;
            } else {
                // Henüz hazır değil — kuyruğa geri koy
                queue.push_front(trans);
                break;
            }
        }

        if checkpointed > 0 {
            // Kuyruk ilerledi — tail_sequence güncelle
            self.tail_sequence.fetch_add(checkpointed, Ordering::SeqCst);

            crate::serial_println!("[JOURNAL] Checkpoint complete ({} transactions)", checkpointed);
        }

        Ok(())
    }

    /// Çökme sonrası günlüğü kurtarır.
    ///
    /// Kurtarma algoritması:
    /// 1. Günlük süper bloğunu oku
    /// 2. Tüm DESC+COMMIT çiftlerini tara (teslim edilmiş işlemler)
    /// 3. Teslim edilmemiş işlemleri atla (eksik COMMIT bloğu)
    /// 4. Teslim edilmiş işlemleri sırayla yeniden uygula
    pub fn recover(&self, drive: &mut dyn BlockDevice) -> Result<u64, JournalError> {
        crate::serial_println!("[JOURNAL] Starting recovery");

        self.flags.fetch_or(JBD2_FLAG_RECOVERY, Ordering::SeqCst);

        let bs = self.block_size as usize;
        let total = self.total_blocks.load(Ordering::SeqCst);
        let mut recovered = 0u64;
        let sectors_per_block = (bs / 512) as u8;

        // Süper bloğu oku
        let superblock_buf = drive.read_sectors(self.start_block as u32, sectors_per_block);
        if superblock_buf.is_empty() {
            crate::serial_println!("[JOURNAL] No superblock found, skipping recovery");
            return Ok(0);
        }

        // Süper blok sihirli sayısını doğrula
        let magic = u32::from_be_bytes([
            superblock_buf[0], superblock_buf[1],
            superblock_buf[2], superblock_buf[3],
        ]);
        if magic != JBD2_MAGIC_NUMBER {
            crate::serial_println!("[JOURNAL] Invalid journal magic (0x{:08X}), skipping recovery", magic);
            return Ok(0);
        }

        // Günlük bloklarını tara — DESC+COMMIT çiftlerini bul
        let mut offset = 1u64; // Süper bloktan sonra başla
        while offset < total {
            // Blok başlığını oku
            let header_buf = drive.read_sectors((self.start_block + offset) as u32, sectors_per_block);
            if header_buf.is_empty() || header_buf.len() < 12 {
                break;
            }

            let magic = u32::from_be_bytes([
                header_buf[0], header_buf[1],
                header_buf[2], header_buf[3],
            ]);
            let blocktype = u32::from_be_bytes([
                header_buf[4], header_buf[5],
                header_buf[6], header_buf[7],
            ]);

            if magic != JBD2_MAGIC_NUMBER {
                // Geçerli bir günlük bloğu değil — taramayı durdur
                break;
            }

            if blocktype == JBD2_DESCRIPTOR_BLOCK {
                // Tanımlayıcı blok bulundu — COMMIT bloğunu ara
                let seq = u32::from_be_bytes([
                    header_buf[8], header_buf[9],
                    header_buf[10], header_buf[11],
                ]);

                // Tanımlayıcı bloktaki etiketleri oku
                let tags_per_block = (bs - 12 - 4) / 16;
                let mut tag_offset = 12usize;
                let mut block_addrs: Vec<u64> = Vec::new();
                let mut found_last = false;

                while tag_offset + 16 <= bs - 4 && !found_last {
                    let flags = u32::from_be_bytes([
                        header_buf[tag_offset + 4], header_buf[tag_offset + 5],
                        header_buf[tag_offset + 6], header_buf[tag_offset + 7],
                    ]);
                    let blocknr = u32::from_be_bytes([
                        header_buf[tag_offset + 0], header_buf[tag_offset + 1],
                        header_buf[tag_offset + 2], header_buf[tag_offset + 3],
                    ]) as u64;
                    let blocknr_high = u32::from_be_bytes([
                        header_buf[tag_offset + 8], header_buf[tag_offset + 9],
                        header_buf[tag_offset + 10], header_buf[tag_offset + 11],
                    ]);
                    let full_blocknr = blocknr | ((blocknr_high as u64) << 32);

                    block_addrs.push(full_blocknr);

                    if flags & 8 != 0 {
                        found_last = true;
                    }
                    tag_offset += 16;
                }

                // Veri bloklarını hesapla
                let desc_blocks = if !block_addrs.is_empty() {
                    (block_addrs.len() + tags_per_block - 1) / tags_per_block
                } else {
                    0
                };
                let data_start = offset + desc_blocks as u64;
                let commit_offset = data_start + block_addrs.len() as u64;

                if commit_offset >= total {
                    break;
                }

                // COMMIT bloğunu kontrol et
                let commit_buf = drive.read_sectors((self.start_block + commit_offset) as u32, sectors_per_block);
                if commit_buf.is_empty() || commit_buf.len() < 12 {
                    break;
                }

                let commit_magic = u32::from_be_bytes([
                    commit_buf[0], commit_buf[1],
                    commit_buf[2], commit_buf[3],
                ]);
                let commit_type = u32::from_be_bytes([
                    commit_buf[4], commit_buf[5],
                    commit_buf[6], commit_buf[7],
                ]);
                let commit_seq = u32::from_be_bytes([
                    commit_buf[8], commit_buf[9],
                    commit_buf[10], commit_buf[11],
                ]);

                if commit_magic == JBD2_MAGIC_NUMBER && commit_type == JBD2_COMMIT_BLOCK && commit_seq == seq {
                    // DESC+COMMIT çifti doğrulandı — veri bloklarını yeniden uygula
                    for (i, fs_blocknr) in block_addrs.iter().enumerate() {
                        let journal_data_lba = self.start_block + data_start + i as u64;
                        let data_buf = drive.read_sectors(journal_data_lba as u32, sectors_per_block);
                        if !data_buf.is_empty() {
                            // Veriyi asıl fs konumuna yaz
                            drive.write_sectors(*fs_blocknr as u32, &data_buf).map_err(|_| JournalError::IoError)?;
                        }
                    }

                    recovered += 1;
                    crate::serial_println!("[JOURNAL] Recovered transaction seq={} ({} blocks)", seq, block_addrs.len());

                    // Bir sonraki bloğa atla (commit sonrası)
                    offset = commit_offset + 1;
                } else {
                    // COMMIT yok veya uyumsuz — teslim edilmemiş işlem, atla
                    crate::serial_println!("[JOURNAL] Skipping incomplete transaction seq={}", seq);
                    offset += 1;
                }
            } else if blocktype == JBD2_SUPERBLOCK_V1_BLK || blocktype == JBD2_SUPERBLOCK_V2_BLK {
                offset += 1;
            } else {
                // Bilinmeyen blok tipi — taramayı durdur
                break;
            }
        }

        crate::serial_println!("[JOURNAL] Recovery complete, {} transactions recovered", recovered);
        Ok(recovered)
    }

    /// Günlüğü iptal eder — kurtarılamaz bir I/O hatası durumunda çağrılır.
    /// İptal edilen günlük üzerinde başka işlem yapılamaz.
    pub fn abort(&self, errno: i32) {
        self.aborted.store(true, Ordering::SeqCst);
        self.flags.fetch_or(JBD2_FLAG_ABORT, Ordering::SeqCst);

        crate::serial_println!("[JOURNAL] Journal aborted (errno={})", errno);
    }

    /// Günlüğün iptal edilip edilmediğini sorgular
    pub fn is_aborted(&self) -> bool {
        self.aborted.load(Ordering::SeqCst)
    }

    /// Günlük istatistiklerini döndürür
    pub fn get_stats(&self) -> JournalStats {
        self.stats.lock().clone()
    }
}

// ============================================================================
// GÜNLÜK YÖNETİCİSİ
// ============================================================================

/// Sistem genelinde birden fazla günlüğü yöneten merkezi yapı.
/// Farklı blok cihazları farklı günlüklere sahip olabilir.
pub struct JournalManager {
    journals: Mutex<Vec<Arc<Journal>>>,
}

impl JournalManager {
    pub const fn new() -> Self {
        Self {
            journals: Mutex::new(Vec::new()),
        }
    }

    /// Yeni bir günlük oluşturur ve yöneticiye kaydeder
    pub fn create_journal(&self, device: u64, start: u64, size: u64, block_size: u32) -> Arc<Journal> {
        let id = self.journals.lock().len() as u64;
        let journal = Arc::new(Journal::new(id, device, start, size, block_size));
        self.journals.lock().push(journal.clone());
        journal
    }

    /// Kimliğe göre günlük getirir
    pub fn get_journal(&self, id: u64) -> Option<Arc<Journal>> {
        self.journals.lock().get(id as usize).cloned()
    }
}

lazy_static::lazy_static! {
    /// Sistem geneli statik günlük yöneticisi
    pub static ref JOURNAL_MANAGER: JournalManager = JournalManager::new();
}

// ============================================================================
// HATA TÜRÜ
// ============================================================================

/// Günlük işlemlerinde oluşabilecek hatalar
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalError {
    /// Mevcut aktif işlem yok
    NoTransaction,
    /// Günlük alanı doldu — checkpoint gerekiyor
    JournalFull,
    /// Disk I/O hatası
    IoError,
    /// Günlük verisi bozuk (sihirli sayı veya checksum uyumsuzluğu)
    CorruptJournal,
    /// Günlük iptal edildi
    Aborted,
}

impl From<LinuxDriverError> for JournalError {
    fn from(_: LinuxDriverError) -> Self {
        JournalError::IoError
    }
}

// ============================================================================
// BAŞLATMA
// ============================================================================

/// Günlük alt sistemini başlatır
pub fn init() {
    crate::serial_println!("[JOURNAL] Subsystem initialized");
}
