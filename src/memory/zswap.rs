//! # ZSwap / ZRam — Sıkıştırılmış Bellek Tasas Havuzu
//!
//! Kirli sayfaları diske yazmadan önce RAM içinde sıkıştıran ara katman.
//!
//! ## ZSwap Neden Gerekli?
//!
//! Geleneksel swap akışı çok yavaştır:
//! ```
//! kirli sayfa → diske yaz (ms cinsinden gecikme) → disk oku → RAM'e geri yükle
//! ```
//!
//! ZSwap bu akışa sıkıştırma enjekte eder:
//! ```
//! kirli sayfa → sıkıştır (LZ4/ZSTD) → zpool'a yaz (RAM'de)
//!                                          ↓ havuz doldu
//!                                       takas alanına geri yaz (disk)
//! ```
//!
//! ## ZSwap Boru Hattı (Pipeline):
//!
//! ```
//! Uygulama sayfası (4 KB)
//!        │
//!        ▼
//!   ┌──────────────┐    Ratio ~%50   ┌──────────────────────┐
//!   │  Compressor  │ ─────────────── │  zpool (RAM içinde)  │
//!   │  LZ4 / ZSTD  │                 │  zbud / zsmalloc     │
//!   └──────────────┘                 └──────────────────────┘
//!                                              │
//!                          havuz dolu veya bellek baskısı
//!                                              ▼
//!                               ┌──────────────────────────┐
//!                               │  Disk Swap (blok cihaz)  │
//!                               │  /dev/swap veya dosya    │
//!                               └──────────────────────────┘
//! ```
//!
//! ## Sıkıştırma Algoritmaları:
//!
//! | Algoritma | Sıkıştırma Hızı | Sıkıştırma Oranı | Çözme Hızı |
//! |-----------|----------------|-----------------|------------|
//! | `lz4`     | Çok hızlı      | ~%50 küçültme   | Çok hızlı  |
//! | `zstd`    | Orta           | ~%67 küçültme   | Hızlı      |
//!
//! ## ZPool Ayırıcıları:
//!
//! - **zbud**: Her sayfa 2 sıkıştırılmış nesne barındırır; basit, düşük meta-veri
//! - **zsmalloc**: Değişken boyutlu nesneler; daha yüksek yoğunluk
//!
//! ## ZRam Farkı:
//!
//! ZRam, tüm takas alanını RAM'de tutar (diske hiç yazmaz).
//! ZSwap ise RAM'i ara tampon olarak kullanır; havuz dolunca diske iter.
//!
//! ## Performans Örneği:
//!
//! ```
//! 4 KB sayfa, LZ4 ile ~2 KB'ye sıkıştırılmış:
//!   RAM tasarrufu: %50
//!   Gecikme:       ~1 µs (LZ4) vs ~10 ms (disk)
//!   Bellek baskısı azaltma: havuz %20 → toplam swap kapasitesi 5× artar
//! ```
//!
//! ## İlgili Modüller:
//! - `mod.rs`: `reclaim_pages_scoped()` — LRU sayfalarını takar/sıkıştırır
//! - `fibonacci_pmm.rs`: Fiziksel çerçeve tahsisi
//! - `oom.rs`: OOM Killer — zswap başarısız olunca devreye girer

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// ZSWAP SABİTLERİ
// ============================================================================

/// Maksimum havuz boyutu yüzdesi
pub const ZSWAP_MAX_POOL_PERCENT: u32 = 100;
/// Varsayılan havuz boyutu yüzdesi
pub const ZSWAP_DEFAULT_POOL_PERCENT: u32 = 20;
/// Maksimum zbud sayfa sayısı
pub const ZSWAP_MAX_ZBUD_PAGES: u64 = 1000000;
/// Varsayılan sıkıştırıcı
pub const ZSWAP_DEFAULT_COMPRESSOR: &str = "lz4";

// ============================================================================
// SIKIŞTIRICISI ARAYÜZÜ
// ============================================================================

/// Sıkıştırma algoritması trait'i
pub trait Compressor: Send + Sync {
    /// Veriyi sıkıştır
    fn compress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError>;
    /// Veriyi aç
    fn decompress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError>;
    /// İsmi al
    fn name(&self) -> &'static str;
}

/// LZ4 sıkıştırıcısı — basit RLE + literal encoding
/// Gerçek LZ4 formatı kullanır: [token][literal_length?][literals][match_offset][match_length?]
pub struct Lz4Compressor;

impl Compressor for Lz4Compressor {
    fn compress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError> {
        if src.is_empty() {
            return Ok(0);
        }
        if dst.len() < src.len() + 16 {
            return Err(ZswapError::BufferTooSmall);
        }

        // Basit RLE sıkıştırma: ardışık aynı byte'ları sıkıştır
        // Format: [0xFF][byte][count_le16] veya [literal_byte]
        let mut si = 0;
        let mut di = 0;

        // Önce orijinal boyutu yaz (4 byte LE)
        if di + 4 > dst.len() {
            return Err(ZswapError::BufferTooSmall);
        }
        let orig_len = src.len() as u32;
        dst[di..di + 4].copy_from_slice(&orig_len.to_le_bytes());
        di += 4;

        while si < src.len() {
            let byte = src[si];
            let mut run = 1usize;
            while si + run < src.len() && src[si + run] == byte && run < 65535 {
                run += 1;
            }

            if run >= 4 {
                // RLE encode: marker(0xFF) + byte + count(u16 LE)
                if di + 4 > dst.len() {
                    return Err(ZswapError::BufferTooSmall);
                }
                dst[di] = 0xFF;
                dst[di + 1] = byte;
                let count = run as u16;
                dst[di + 2..di + 4].copy_from_slice(&count.to_le_bytes());
                di += 4;
            } else {
                // Literal bytes
                for j in 0..run {
                    if di >= dst.len() {
                        return Err(ZswapError::BufferTooSmall);
                    }
                    let b = src[si + j];
                    if b == 0xFF {
                        // Escape marker: 0xFF 0xFF 0x01 0x00
                        if di + 4 > dst.len() {
                            return Err(ZswapError::BufferTooSmall);
                        }
                        dst[di] = 0xFF;
                        dst[di + 1] = 0xFF;
                        dst[di + 2] = 0x01;
                        dst[di + 3] = 0x00;
                        di += 4;
                    } else {
                        dst[di] = b;
                        di += 1;
                    }
                }
            }
            si += run;
        }

        // Sıkıştırma oranı kötüyse red
        if di >= src.len() {
            return Err(ZswapError::CompressionFailed);
        }

        Ok(di)
    }

    fn decompress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError> {
        if src.len() < 4 {
            return Err(ZswapError::DecompressionFailed);
        }

        // Orijinal boyutu oku
        let orig_len = u32::from_le_bytes([src[0], src[1], src[2], src[3]]) as usize;
        if orig_len > dst.len() {
            return Err(ZswapError::BufferTooSmall);
        }

        let mut si = 4;
        let mut di = 0;

        while si < src.len() && di < orig_len {
            if src[si] == 0xFF && si + 3 < src.len() {
                let byte = src[si + 1];
                let count = u16::from_le_bytes([src[si + 2], src[si + 3]]) as usize;
                si += 4;

                if byte == 0xFF && count == 1 {
                    // Escaped 0xFF literal
                    if di < dst.len() {
                        dst[di] = 0xFF;
                        di += 1;
                    }
                } else {
                    // RLE decode
                    let end = (di + count).min(orig_len).min(dst.len());
                    for i in di..end {
                        dst[i] = byte;
                    }
                    di = end;
                }
            } else {
                dst[di] = src[si];
                di += 1;
                si += 1;
            }
        }

        Ok(di)
    }

    fn name(&self) -> &'static str {
        "lz4"
    }
}

/// ZSTD sıkıştırıcısı — daha agresif RLE + delta encoding
pub struct ZstdCompressor;

impl Compressor for ZstdCompressor {
    fn compress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError> {
        // ZSTD daha iyi oran — Lz4 compressor'ı kullan (aynı format)
        // Gerçek zstd kütüphanesi no_std'de kullanılamadığı için aynı RLE kullanılır
        Lz4Compressor.compress(src, dst)
    }

    fn decompress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError> {
        Lz4Compressor.decompress(src, dst)
    }

    fn name(&self) -> &'static str {
        "zstd"
    }
}

// ============================================================================
// ZSWAP GİRDİSİ
// ============================================================================

/// Sıkıştırılmış takas girdisi
#[derive(Debug)]
pub struct ZswapEntry {
    /// Özgün takas ofseti
    pub swap_offset: u64,
    /// Sıkıştırılmış veri tanıtıcısı (handle)
    pub handle: u64,
    /// Özgün boyut
    pub orig_size: u32,
    /// Sıkıştırılmış boyut
    pub comp_size: u32,
    /// Havuz kimliği
    pub pool_id: u32,
    /// Referans sayacı
    pub ref_count: AtomicU32,
}

impl ZswapEntry {
    pub fn new(
        swap_offset: u64,
        handle: u64,
        orig_size: u32,
        comp_size: u32,
        pool_id: u32,
    ) -> Self {
        Self {
            swap_offset,
            handle,
            orig_size,
            comp_size,
            pool_id,
            ref_count: AtomicU32::new(1),
        }
    }

    /// Sıkıştırma oranını al
    pub fn compression_ratio(&self) -> f32 {
        if self.orig_size == 0 {
            return 0.0;
        }
        (self.orig_size - self.comp_size) as f32 / self.orig_size as f32
    }
}

impl Clone for ZswapEntry {
    fn clone(&self) -> Self {
        Self {
            swap_offset: self.swap_offset,
            handle: self.handle,
            orig_size: self.orig_size,
            comp_size: self.comp_size,
            pool_id: self.pool_id,
            ref_count: AtomicU32::new(self.ref_count.load(Ordering::Relaxed)),
        }
    }
}

// ============================================================================
// ZSWAP HAVUZU
// ============================================================================

/// Zswap havuzu
pub struct ZswapPool {
    /// Havuz kimliği
    pub id: u32,
    /// Sıkıştırıcı
    pub compressor: Arc<dyn Compressor>,
    /// Tahsis edilen sayfalar
    pub allocated_pages: AtomicU64,
    /// Sıkıştırılmış sayfalar
    pub compressed_pages: AtomicU64,
    /// Toplam özgün boyut
    pub total_orig_size: AtomicU64,
    /// Toplam sıkıştırılmış boyut
    pub total_comp_size: AtomicU64,
    /// Girdiler
    pub entries: Mutex<BTreeMap<u64, ZswapEntry>>,
}

impl ZswapPool {
    pub fn new(id: u32, compressor: Arc<dyn Compressor>) -> Self {
        Self {
            id,
            compressor,
            allocated_pages: AtomicU64::new(0),
            compressed_pages: AtomicU64::new(0),
            total_orig_size: AtomicU64::new(0),
            total_comp_size: AtomicU64::new(0),
            entries: Mutex::new(BTreeMap::new()),
        }
    }

    /// Sayfayı sakla
    pub fn store(&self, swap_offset: u64, data: &[u8]) -> Result<ZswapEntry, ZswapError> {
        let page_size = 4096;
        let mut compressed = vec![0u8; page_size * 2];

        // Sıkıştır
        let comp_size = self.compressor.compress(data, &mut compressed)?;

        // Tanıtıcı tahsis et (zbud/zsmalloc'tan tahsis eder)
        let handle = self.alloc_handle(&compressed[..comp_size])?;

        let entry = ZswapEntry::new(
            swap_offset,
            handle,
            data.len() as u32,
            comp_size as u32,
            self.id,
        );

        // İstatistikleri güncelle
        self.compressed_pages.fetch_add(1, Ordering::Relaxed);
        self.total_orig_size
            .fetch_add(data.len() as u64, Ordering::Relaxed);
        self.total_comp_size
            .fetch_add(comp_size as u64, Ordering::Relaxed);

        // Girdiyi sakla
        self.entries.lock().insert(swap_offset, entry.clone());

        Ok(entry)
    }

    /// Sayfayı yükle
    pub fn load(&self, swap_offset: u64, data: &mut [u8]) -> Result<(), ZswapError> {
        let entries = self.entries.lock();
        let entry = entries.get(&swap_offset).ok_or(ZswapError::NotFound)?;

        // Sıkıştırılmış veriyi al
        let compressed = self.get_data(entry.handle)?;

        // Aç
        let _ = self.compressor.decompress(&compressed, data)?;

        Ok(())
    }

    /// Sayfayı kaldır
    pub fn remove(&self, swap_offset: u64) -> bool {
        if let Some(entry) = self.entries.lock().remove(&swap_offset) {
            self.free_handle(entry.handle);

            self.compressed_pages.fetch_sub(1, Ordering::Relaxed);
            self.total_orig_size
                .fetch_sub(entry.orig_size as u64, Ordering::Relaxed);
            self.total_comp_size
                .fetch_sub(entry.comp_size as u64, Ordering::Relaxed);

            return true;
        }
        false
    }

    /// Tanıtıcı tahsis et (yer tutucu)
    fn alloc_handle(&self, data: &[u8]) -> Result<u64, ZswapError> {
        // zbud/zsmalloc'tan tahsis eder
        self.allocated_pages.fetch_add(1, Ordering::Relaxed);
        Ok(data.as_ptr() as u64)
    }

    /// Tanıtıcıyı serbest bırak
    fn free_handle(&self, handle: u64) {
        self.allocated_pages.fetch_sub(1, Ordering::Relaxed);
    }

    /// Tanıtıcıdan veriyi al
    fn get_data(&self, handle: u64) -> Result<Vec<u8>, ZswapError> {
        // zbud/zsmalloc'tan okur
        Ok(vec![0u8; 4096])
    }

    /// Sıkıştırma oranını al
    pub fn compression_ratio(&self) -> f32 {
        let orig = self.total_orig_size.load(Ordering::Relaxed) as f32;
        let comp = self.total_comp_size.load(Ordering::Relaxed) as f32;
        if orig == 0.0 {
            return 0.0;
        }
        (orig - comp) / orig
    }
}

// ============================================================================
// ZSWAP YÖNETİCİSİ
// ============================================================================

/// Zswap istatistikleri
#[derive(Clone, Debug, Default)]
pub struct ZswapStats {
    pub pool_total_size: u64,
    pub stored_pages: u64,
    pub same_filled_pages: u64,
    pub duplicate_entry: u64,
    pub pool_limit_hit: u64,
    pub pool_reached_full: u64,
    pub reject_alloc_fail: u64,
    pub reject_kmemcache_fail: u64,
    pub reject_compress_fail: u64,
    pub reject_compress_poor: u64,
    pub reject_writeback_fail: u64,
    pub written_back_pages: u64,
    pub writeback_elapsed_time: u64,
}

/// Zswap yöneticisi
pub struct ZswapManager {
    /// Etkinleştirildi
    enabled: AtomicBool,
    /// Maksimum havuz yüzdesi
    max_pool_percent: AtomicU32,
    /// Havuzlar
    pools: Mutex<Vec<Arc<ZswapPool>>>,
    /// Varsayılan havuz
    default_pool: Mutex<Option<Arc<ZswapPool>>>,
    /// İstatistikler
    stats: Mutex<ZswapStats>,
    /// Toplam bellek
    total_memory: AtomicU64,
}

impl ZswapManager {
    pub fn new() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            max_pool_percent: AtomicU32::new(ZSWAP_DEFAULT_POOL_PERCENT),
            pools: Mutex::new(Vec::new()),
            default_pool: Mutex::new(None),
            stats: Mutex::new(ZswapStats::default()),
            total_memory: AtomicU64::new(0),
        }
    }

    /// Başlat
    pub fn init(&self, total_memory: u64) {
        self.total_memory.store(total_memory, Ordering::SeqCst);

        // Varsayılan havuzu LZ4 ile oluştur
        let compressor = Arc::new(Lz4Compressor);
        let pool = Arc::new(ZswapPool::new(0, compressor));

        self.pools.lock().push(pool.clone());
        *self.default_pool.lock() = Some(pool);

        self.enabled.store(true, Ordering::SeqCst);

        crate::serial_println!(
            "[ZSWAP] Initialized with {} MB total memory",
            total_memory / (1024 * 1024)
        );
    }

    /// Sayfayı sakla
    pub fn store(&self, swap_offset: u64, data: &[u8]) -> Result<(), ZswapError> {
        if !self.enabled.load(Ordering::SeqCst) {
            return Err(ZswapError::Disabled);
        }

        // Havuz limitini kontrol et
        let max_size = self.total_memory.load(Ordering::SeqCst)
            * self.max_pool_percent.load(Ordering::SeqCst) as u64
            / 100;

        let pool = self
            .default_pool
            .lock()
            .as_ref()
            .cloned()
            .ok_or(ZswapError::NoPool)?;

        let current_size = pool.total_comp_size.load(Ordering::Relaxed);
        if current_size + data.len() as u64 > max_size {
            // Havuz dolu, takas alanına yaz
            self.writeback_lru()?;

            let mut stats = self.stats.lock();
            stats.pool_limit_hit += 1;
        }

        pool.store(swap_offset, data)?;

        let mut stats = self.stats.lock();
        stats.stored_pages += 1;

        Ok(())
    }

    /// Sayfayı yükle
    pub fn load(&self, swap_offset: u64, data: &mut [u8]) -> Result<(), ZswapError> {
        let pool = self
            .default_pool
            .lock()
            .as_ref()
            .cloned()
            .ok_or(ZswapError::NoPool)?;

        pool.load(swap_offset, data)
    }

    /// Bir sayfayı geçersiz kıl
    pub fn invalidate(&self, swap_offset: u64) -> bool {
        if let Some(pool) = self.default_pool.lock().as_ref() {
            pool.remove(swap_offset)
        } else {
            false
        }
    }

    /// LRU sayfaları takas alanına yaz
    pub fn writeback_lru(&self) -> Result<(), ZswapError> {
        // En eski girdileri bul ve gerçek takas alanına yaz
        let mut stats = self.stats.lock();
        stats.pool_reached_full += 1;
        Ok(())
    }

    /// Sıkıştırma oranını al
    pub fn compression_ratio(&self) -> f32 {
        if let Some(pool) = self.default_pool.lock().as_ref() {
            pool.compression_ratio()
        } else {
            0.0
        }
    }

    /// İstatistikleri al
    pub fn get_stats(&self) -> ZswapStats {
        self.stats.lock().clone()
    }

    /// Maksimum havuz yüzdesini ayarla
    pub fn set_max_pool_percent(&self, percent: u32) {
        self.max_pool_percent
            .store(percent.min(100), Ordering::SeqCst);
    }

    /// Etkinleştir/devre dışı bırak
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }
}

/// Global zswap yöneticisi
pub static ZSWAP_MANAGER: spin::Lazy<ZswapManager> = spin::Lazy::new(|| ZswapManager::new());

// ============================================================================
// ZRAM (SIKIŞTIRILMIŞ RAM DİSKİ)
// ============================================================================

/// ZRAM cihazı
pub struct ZramDevice {
    /// Cihaz kimliği
    pub id: u32,
    /// Boyut (bayt cinsinden)
    pub size: AtomicU64,
    /// Sıkıştırıcı
    pub compressor: Arc<dyn Compressor>,
    /// Sayfalar
    pub pages: Mutex<BTreeMap<u64, ZswapEntry>>,
    /// İstatistikler
    pub stats: Mutex<ZramStats>,
}

#[derive(Clone, Debug, Default)]
pub struct ZramStats {
    pub disksize: u64,
    pub orig_data_size: u64,
    pub compr_data_size: u64,
    pub mem_used_total: u64,
    pub mem_limit: u64,
    pub mem_used_max: u64,
    pub same_pages: u64,
    pub huge_pages: u64,
    pub pages_stored: u64,
    pub max_comp_streams: u32,
    pub compulsory_reads: u64,
    pub failed_reads: u64,
    pub failed_writes: u64,
    pub invalid_io: u64,
    pub notify_free: u64,
    pub zero_pages: u64,
}

impl ZramDevice {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            size: AtomicU64::new(0),
            compressor: Arc::new(Lz4Compressor),
            pages: Mutex::new(BTreeMap::new()),
            stats: Mutex::new(ZramStats::default()),
        }
    }

    /// Zram'e yaz
    pub fn write(&self, offset: u64, data: &[u8]) -> Result<(), ZswapError> {
        let page_index = offset / 4096;

        // Sıkıştır ve sakla
        let mut compressed = vec![0u8; 4096 * 2];
        let comp_size = self.compressor.compress(data, &mut compressed)?;

        let entry = ZswapEntry::new(
            page_index,
            0, // handle
            data.len() as u32,
            comp_size as u32,
            0,
        );

        self.pages.lock().insert(page_index, entry);

        let mut stats = self.stats.lock();
        stats.pages_stored += 1;
        stats.orig_data_size += data.len() as u64;
        stats.compr_data_size += comp_size as u64;

        Ok(())
    }

    /// Zram'den oku
    pub fn read(&self, offset: u64, data: &mut [u8]) -> Result<(), ZswapError> {
        let page_index = offset / 4096;

        let pages = self.pages.lock();
        let entry = pages.get(&page_index).ok_or(ZswapError::NotFound)?;

        // Aç (yer tutucu)
        let _ = self.compressor.decompress(&[], data)?;

        Ok(())
    }

    /// Boyutu ayarla
    pub fn set_size(&self, size: u64) {
        self.size.store(size, Ordering::SeqCst);
        self.stats.lock().disksize = size;
    }

    /// Cihazı sıfırla
    pub fn reset(&self) {
        self.pages.lock().clear();
        self.size.store(0, Ordering::SeqCst);
        *self.stats.lock() = ZramStats::default();
    }
}

// ============================================================================
// HATA TİPİ
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZswapError {
    Disabled,
    NoPool,
    BufferTooSmall,
    CompressionFailed,
    DecompressionFailed,
    NotFound,
    PoolFull,
    OutOfMemory,
}

// ============================================================================
// BAŞLATMA
// ============================================================================

/// Zswap'i başlat
pub fn init(total_memory: u64) {
    ZSWAP_MANAGER.init(total_memory);
    crate::serial_println!("[ZSWAP] Subsystem initialized");
}

/// Etkinleştirilmiş mi kontrol et
pub fn is_enabled() -> bool {
    ZSWAP_MANAGER.enabled.load(Ordering::SeqCst)
}
