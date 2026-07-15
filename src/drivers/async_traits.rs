//! # echOS TIER 1 Asenkron Sürücü Trait'leri
//!
//! TIER 1 (Şah Damarı) donanımlar için lock-free asenkron I/O arayüzleri.
//!
//! Bu trait'ler io_uring submission/completion modeline göre tasarlanmıştır:
//!
//! ```text
//!  Kullanıcı Alanı          echOS Core             TIER 1 Sürücü
//!  ┌───────────┐     ┌─────────────────┐     ┌──────────────────────┐
//!  │ io_uring  │     │ Lock-Free       │     │ AsyncBlockDevice     │
//!  │ SQE       │────►│ Worker Pool     │────►│ submit() → Token     │
//!  │           │     │ (Treiber Stack) │     │                      │
//!  │           │     │                 │     │ poll() → Completion  │
//!  │ CQE ◄────│─────│ Completion Ring │◄────│                      │
//!  └───────────┘     └─────────────────┘     └──────────────────────┘
//! ```
//!
//! ## Kural: Mutex YASAK
//!
//! TIER 1 trait implementasyonları `Mutex`, `SpinLock`, `RwLock` veya
//! herhangi bir blocking primitive içeremez. Tüm senkronizasyon:
//! - Atomik operasyonlar (CAS, fetch_add, load/store)
//! - Memory barriers (smp_mb, smp_wmb, smp_rmb)
//! - Lock-free veri yapıları (ring buffer, Chase-Lev deque)
//!
//! ile yapılmalıdır.

use core::sync::atomic::AtomicU64;

// ────────────────────────────────────────────────────────────
// Token & Completion
// ────────────────────────────────────────────────────────────

/// Submission Token — her async I/O isteğinin benzersiz kimliği
///
/// `submit_*()` çağrısı bir `SubmissionToken` döndürür.
/// `poll_completion()` ile bu token'a karşılık gelen sonuç alınır.
///
/// Token değerleri monoton artan bir atomic counter'dan üretilir.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SubmissionToken(pub u64);

/// Global token sayacı (monoton artan)
static TOKEN_COUNTER: AtomicU64 = AtomicU64::new(1);

impl SubmissionToken {
    /// Yeni benzersiz bir submission token üret
    pub fn next() -> Self {
        SubmissionToken(TOKEN_COUNTER.fetch_add(1, core::sync::atomic::Ordering::Relaxed))
    }
}

/// Async I/O tamamlanma olayı
///
/// Bir async operasyon tamamlandığında sürücü bu struct'ı döndürür.
/// `result` negatifse hata kodu (Linux errno), pozitifse başarılı byte sayısı.
#[derive(Clone, Copy, Debug)]
pub struct CompletionEvent {
    /// Hangi submission'a ait olduğunu belirleyen token
    pub token: SubmissionToken,
    /// Sonuç: >= 0 başarılı (byte sayısı), < 0 hata kodu
    pub result: i64,
    /// Transfer edilen veri uzunluğu (byte)
    pub data_len: usize,
    /// Ek bayraklar (sürücüye özgü)
    pub flags: u32,
}

/// Async I/O hata türleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsyncIoError {
    /// Cihaz meşgul — tüm submission queue'lar dolu
    QueueFull,
    /// Geçersiz parametre (alignment, boyut vb.)
    InvalidParam,
    /// Cihaz hatası (timeout, controller reset)
    DeviceError,
    /// DMA buffer tahsis edilemedi
    DmaAllocFailed,
    /// Desteklenmeyen operasyon
    NotSupported,
    /// Cihaz bağlı değil / kaldırıldı
    DeviceGone,
}

// ────────────────────────────────────────────────────────────
// DMA Buffer
// ────────────────────────────────────────────────────────────

/// DMA buffer handle — fiziksel olarak contiguous bellek bölgesi
///
/// TIER 1 sürücüler zero-copy I/O yapar: kullanıcı buffer'ı doğrudan
/// DMA ile cihaza gönderilir. Bu struct fiziksel adresi ve boyutu tutar.
#[derive(Clone, Copy, Debug)]
pub struct DmaBuffer {
    /// Sanal adres (kernel adresi)
    pub vaddr: usize,
    /// Fiziksel adres (DMA için donanıma verilen)
    pub paddr: u64,
    /// Buffer boyutu (byte)
    pub size: usize,
}

/// Scatter/gather DMA fragment view.
///
/// Her parça fiziksel olarak contiguous olmalıdır; parça dizisi tek ağ paketi
/// olarak yayınlanır ve TX completion gelene kadar caller sahipliği korur.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DmaSlice {
    /// Sanal adres (opsiyonel kernel mapping; 0 olabilir)
    pub vaddr: usize,
    /// Fiziksel/DMA adresi
    pub paddr: u64,
    /// Fragment uzunluğu
    pub len: usize,
}

impl DmaSlice {
    pub const fn new(vaddr: usize, paddr: u64, len: usize) -> Self {
        Self { vaddr, paddr, len }
    }

    pub const fn from_buffer(buffer: &DmaBuffer, len: usize) -> Self {
        Self {
            vaddr: buffer.vaddr,
            paddr: buffer.paddr,
            len,
        }
    }
}

// ────────────────────────────────────────────────────────────
// TIER 1 Async Trait: Block Device (NVMe)
// ────────────────────────────────────────────────────────────

/// Asenkron blok aygıtı arayüzü — NVMe SSD ve benzeri cihazlar için.
///
/// **Mutex YASAK**: Tüm operasyonlar non-blocking ve lock-free olmalı.
///
/// ```text
/// let token = nvme.submit_read(0, 8, &dma_buf)?;
/// // ... başka işler yap ...
/// if let Some(event) = nvme.poll_completion() {
///     assert_eq!(event.token, token);
///     // Veri dma_buf'ta hazır
/// }
/// ```
pub trait AsyncBlockDevice: Send + Sync {
    /// Cihaz adı (ör: "nvme0n1")
    fn name(&self) -> &str;

    /// Sektör boyutu (genellikle 512 veya 4096 byte)
    fn sector_size(&self) -> u32;

    /// Toplam sektör sayısı
    fn total_sectors(&self) -> u64;

    /// I/O queue sayısı (per-CPU olabilir)
    fn queue_count(&self) -> u32;

    /// Asenkron okuma isteği gönder
    ///
    /// - `start_sector`: Başlangıç sektörü
    /// - `sector_count`: Okunacak sektör sayısı
    /// - `dma_buf`: Hedef DMA buffer (fiziksel adres donanıma verilir)
    ///
    /// Döndürülen `SubmissionToken` ile sonucu `poll_completion()` ile al.
    fn submit_read(
        &self,
        start_sector: u64,
        sector_count: u32,
        dma_buf: &DmaBuffer,
    ) -> Result<SubmissionToken, AsyncIoError>;

    /// Asenkron yazma isteği gönder
    ///
    /// - `start_sector`: Başlangıç sektörü
    /// - `sector_count`: Yazılacak sektör sayısı
    /// - `dma_buf`: Kaynak DMA buffer
    fn submit_write(
        &self,
        start_sector: u64,
        sector_count: u32,
        dma_buf: &DmaBuffer,
    ) -> Result<SubmissionToken, AsyncIoError>;

    /// Asenkron flush (cache → disk) isteği gönder
    fn submit_flush(&self) -> Result<SubmissionToken, AsyncIoError>;

    /// Tamamlanan bir I/O olayını al (non-blocking)
    ///
    /// Tamamlanan iş varsa `Some(CompletionEvent)` döner.
    /// Yoksa `None` döner — busy-wait YAPMA, poll döngüsünde çağır.
    fn poll_completion(&self) -> Option<CompletionEvent>;

    /// Belirli bir queue'daki tamamlanan olayları al
    fn poll_completion_queue(&self, queue_id: u32) -> Option<CompletionEvent>;
}

// ────────────────────────────────────────────────────────────
// TIER 1 Async Trait: Network Device (100G NIC)
// ────────────────────────────────────────────────────────────

/// Asenkron ağ aygıtı arayüzü — 100G NIC ve benzeri cihazlar için.
///
/// **Mutex YASAK**: TX/RX descriptor ring'leri lock-free olmalı.
///
/// ```text
/// let token = nic.submit_tx(&dma_buf, pkt_len)?;
/// if let Some(event) = nic.poll_rx() {
///     // Gelen paket event.data_len byte
/// }
/// ```
pub trait AsyncNetDevice: Send + Sync {
    /// Cihaz adı (ör: "eth0", "ens1")
    fn name(&self) -> &str;

    /// MAC adresi
    fn mac_address(&self) -> [u8; 6];

    /// MTU (Maximum Transmission Unit)
    fn mtu(&self) -> u32;

    /// Link hızı (Mbps, 0 = link down)
    fn link_speed(&self) -> u64;

    /// Asenkron paket gönderimi
    ///
    /// - `dma_buf`: Gönderilecek paket verisi (Ethernet frame dahil)
    /// - `len`: Paket uzunluğu (byte)
    fn submit_tx(&self, dma_buf: &DmaBuffer, len: usize) -> Result<SubmissionToken, AsyncIoError>;

    /// Scatter/gather asenkron paket gönderimi.
    ///
    /// Fragment'lar tek Ethernet frame'e aittir. SG desteklemeyen sürücüler tek
    /// fragment için `submit_tx`, çoklu fragment için `NotSupported` döndürür.
    fn submit_tx_sg(&self, fragments: &[DmaSlice]) -> Result<SubmissionToken, AsyncIoError> {
        if fragments.len() != 1 {
            return Err(AsyncIoError::NotSupported);
        }
        let frag = fragments[0];
        let dma_buf = DmaBuffer {
            vaddr: frag.vaddr,
            paddr: frag.paddr,
            size: frag.len,
        };
        self.submit_tx(&dma_buf, frag.len)
    }

    /// RX (alım) descriptor ring'inden paket al (non-blocking)
    ///
    /// Gelen paket varsa `Some(CompletionEvent)` döner.
    /// `data_len` alanı paket uzunluğunu belirtir.
    fn poll_rx(&self) -> Option<CompletionEvent>;

    /// TX tamamlanma olaylarını al (buffer geri kazanımı için)
    fn poll_tx_completion(&self) -> Option<CompletionEvent>;

    /// Promiscuous mode aç/kapat
    fn set_promiscuous(&self, enable: bool);

    /// RSS (Receive Side Scaling) queue sayısını ayarla
    fn set_rss_queues(&self, count: u32) -> Result<(), AsyncIoError>;
}

// ────────────────────────────────────────────────────────────
// TIER 1 Async Trait: GPU Device
// ────────────────────────────────────────────────────────────

/// Asenkron GPU aygıtı arayüzü — Display Controller ve 3D/Compute için.
///
/// **Mutex YASAK**: Komut ring buffer lock-free olmalı.
pub trait AsyncGpuDevice: Send + Sync {
    /// Cihaz adı (ör: "gpu0")
    fn name(&self) -> &str;

    /// VRAM boyutu (byte)
    fn vram_size(&self) -> u64;

    /// Mevcut çözünürlük (genişlik, yükseklik)
    fn resolution(&self) -> (u32, u32);

    /// Asenkron framebuffer blit (2D)
    ///
    /// Kaynak DMA buffer'daki BGRA piksel verilerini ekrana kopyala.
    fn submit_blit(
        &self,
        src_buf: &DmaBuffer,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<SubmissionToken, AsyncIoError>;

    /// Asenkron cursor güncelleme
    fn submit_cursor_update(
        &self,
        x: u32,
        y: u32,
        visible: bool,
    ) -> Result<SubmissionToken, AsyncIoError>;

    /// Display page flip (vsync)
    fn submit_page_flip(&self, framebuffer: &DmaBuffer) -> Result<SubmissionToken, AsyncIoError>;

    /// GPU komut tamamlanma olayı al
    fn poll_completion(&self) -> Option<CompletionEvent>;
}

// ────────────────────────────────────────────────────────────
// TIER 2 Jail Completion Event (TIER 2 sürücülerden gelen sonuçlar)
// ────────────────────────────────────────────────────────────

/// TIER 2 (jail) sürücünün SPSC ring buffer üzerinden gönderdiği tamamlanma olayı
///
/// Bu struct, jail worker thread'den echOS core'a lock-free SPSC ring ile iletilir.
/// Jail sürücüsü blocking call yaptıktan sonra sonucu bu formatta yayınlar.
#[derive(Clone, Copy, Debug)]
pub struct JailCompletionEvent {
    /// İstek ID (echOS tarafından atanan)
    pub request_id: u64,
    /// Sonuç: >= 0 başarılı, < 0 hata
    pub result: i64,
    /// Veri uzunluğu
    pub data_len: usize,
    /// Jail ID (hangi jail'den geldiği)
    pub jail_id: u32,
}

/// TIER 2 jail'e gönderilen I/O isteği
#[derive(Clone, Copy, Debug)]
pub struct JailRequest {
    /// İstek ID
    pub request_id: u64,
    /// Operasyon tipi
    pub opcode: JailOpcode,
    /// Hedef adres/offset
    pub offset: u64,
    /// Veri uzunluğu
    pub length: usize,
    /// DMA buffer (fiziksel adres)
    pub buffer_paddr: u64,
}

/// Jail operasyon tipleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JailOpcode {
    Read,
    Write,
    Flush,
    Control,
}
