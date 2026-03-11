//! # Jail SPSC Ring Buffer — TIER 2 Sürücü İletişim Kanalı
//!
//! Lock-free Single-Producer Single-Consumer (SPSC) ring buffer.
//! TIER 2 jail worker thread'ler ile echOS core arasındaki iletişimi sağlar.
//!
//! ## Mimari
//!
//! ```text
//! ┌──────────────────┐                          ┌──────────────────┐
//! │ echOS Core       │                          │ Jail Worker      │
//! │ (Consumer)       │                          │ (Producer)       │
//! │                  │    Lock-Free SPSC Ring    │                  │
//! │  poll_event() ◄──┼──────────────────────────┼── push_event()   │
//! │                  │  AtomicU32 head/tail      │                  │
//! │                  │  smp_wmb / smp_rmb        │                  │
//! │  push_request()──┼──────────────────────────►┼── poll_request() │
//! │                  │                          │                  │
//! └──────────────────┘                          └──────────────────┘
//! ```
//!
//! ## Performans
//!
//! - Mutex: **SIFIR**
//! - Cache line: head ve tail ayrı cache line'da (64-byte hizalı)
//! - Batch desteği: tek smp_rmb ile N adet olay okunabilir
//! - Hedef: >10M event/sec
//!
//! ## Güvenlik Garantisi
//!
//! SPSC kısıtı:
//! - Tam olarak 1 producer (tail'e yazar)
//! - Tam olarak 1 consumer (head'den okur)
//! - CAS gerekmez — basit atomic store/load yeterli

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicU32, Ordering};

// ============================================================================
// Ring Buffer Sabitleri
// ============================================================================

/// Varsayılan ring boyutu: 1024 giriş (2'nin kuvveti olmalı)
pub const JAIL_RING_SIZE: usize = 1024;

/// Ring mask = SIZE - 1 (hızlı modülo)
const RING_MASK: u32 = (JAIL_RING_SIZE - 1) as u32;

/// Cache line boyutu (false sharing önleme)
const CACHE_LINE: usize = 64;

// ============================================================================
// Jail İletişim Yapıları
// ============================================================================

/// Jail'den core'a gönderilen tamamlama olayı.
///
/// TIER 2 sürücüsü bir I/O işlemini bitirdiğinde bu yapıyı ring'e yazar.
/// Core tarafı poll ile okur — Mutex SIFIR.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct JailEvent {
    /// Orijinal istek kimliği (request_id ile eşleştirilir)
    pub request_id: u64,
    /// İşlem sonucu: >=0 başarı (byte sayısı), <0 hata kodu
    pub result: i64,
    /// Transfer edilen veri uzunluğu
    pub data_len: u32,
    /// Kaynak jail kimliği
    pub jail_id: u16,
    /// Olay bayrakları (gelecek kullanım)
    pub flags: u16,
}

impl Default for JailEvent {
    fn default() -> Self {
        Self {
            request_id: 0,
            result: 0,
            data_len: 0,
            jail_id: 0,
            flags: 0,
        }
    }
}

/// Core'dan jail'e gönderilen I/O isteği.
///
/// Core, TIER 2 sürücüsüne iş vermek istediğinde bu yapıyı request ring'e yazar.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct JailRequest {
    /// Benzersiz istek kimliği (monoton artan)
    pub request_id: u64,
    /// İşlem kodu
    pub opcode: JailOpcode,
    /// Hedef blok/paket ofseti
    pub offset: u64,
    /// Transfer uzunluğu (byte)
    pub length: u32,
    /// DMA buffer fiziksel adresi (jail'in IOMMU kapsamında)
    pub buffer_paddr: u64,
    /// Hedef cihaz tanımlayıcı (jail-internal)
    pub device_id: u16,
    /// İstek bayrakları
    pub flags: u16,
}

impl Default for JailRequest {
    fn default() -> Self {
        Self {
            request_id: 0,
            opcode: JailOpcode::Nop,
            offset: 0,
            length: 0,
            buffer_paddr: 0,
            device_id: 0,
            flags: 0,
        }
    }
}

/// Jail I/O işlem kodları
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JailOpcode {
    /// Hiçbir işlem yapma (test/heartbeat)
    Nop = 0,
    /// Blok okuma
    Read = 1,
    /// Blok yazma
    Write = 2,
    /// Önbellek temizleme (flush/sync)
    Flush = 3,
    /// Cihaz kontrol komutu (ioctl benzeri)
    Control = 4,
    /// Cihaz sıfırlama
    Reset = 5,
    /// Cihaz durumu sorgulama
    Status = 6,
}

// ============================================================================
// Generic SPSC Ring Buffer
// ============================================================================

/// Lock-free SPSC Ring Buffer.
///
/// Generic: herhangi bir `Copy` türü için kullanılabilir.
/// `N` sabiti ring boyutunu belirler (2'nin kuvveti olmalı).
///
/// ## Cache Line Optimizasyonu
///
/// `head` ve `tail` ayrı cache line'lara yerleştirilir:
/// - Producer yalnızca tail'e yazar → consumer'ın head cache line'ını kirletmez
/// - Consumer yalnızca head'e yazar → producer'ın tail cache line'ını kirletmez
///
/// Bu, "false sharing" problemini ortadan kaldırır.
pub struct SpscRing<T: Copy, const N: usize> {
    /// Consumer tarafından ilerletilir (okuduktan sonra)
    head: CacheAligned<AtomicU32>,
    /// Producer tarafından ilerletilir (yazdıktan sonra)
    tail: CacheAligned<AtomicU32>,
    /// Veri dizisi — UnsafeCell ile interior mutability
    buffer: UnsafeCell<[MaybeUninit<T>; N]>,
}

/// Cache line hizalı sarmalayıcı (false sharing önleme)
#[repr(align(64))]
pub struct CacheAligned<T>(pub T);

// SAFETY: SPSC ring buffer tek producer + tek consumer garantisi ile güvenlidir.
// Atomic head/tail + memory barrier tüm senkronizasyonu sağlar.
unsafe impl<T: Copy + Send, const N: usize> Send for SpscRing<T, N> {}
unsafe impl<T: Copy + Send, const N: usize> Sync for SpscRing<T, N> {}

impl<T: Copy, const N: usize> SpscRing<T, N> {
    /// Yeni bir boş SPSC ring buffer oluşturur.
    ///
    /// # Panics
    /// N, 2'nin kuvveti değilse compile-time'da kontrol edemediğimiz için
    /// runtime assert eklenmiştir.
    pub fn new() -> Self {
        assert!(N.is_power_of_two(), "Ring boyutu 2'nin kuvveti olmalı");
        Self {
            head: CacheAligned(AtomicU32::new(0)),
            tail: CacheAligned(AtomicU32::new(0)),
            // SAFETY: MaybeUninit hiçbir zaman gerçekten okunmaz (head/tail sınırları kontrol eder)
            buffer: UnsafeCell::new(unsafe { MaybeUninit::uninit().assume_init() }),
        }
    }

    /// Ring mask (hızlı modülo: index & mask)
    #[inline(always)]
    const fn mask() -> u32 {
        (N - 1) as u32
    }

    /// Ring'deki bekleyen eleman sayısı
    #[inline]
    pub fn len(&self) -> u32 {
        let tail = self.tail.0.load(Ordering::Acquire);
        let head = self.head.0.load(Ordering::Acquire);
        tail.wrapping_sub(head)
    }

    /// Ring boş mu?
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Ring dolu mu?
    #[inline]
    pub fn is_full(&self) -> bool {
        self.len() >= N as u32
    }

    /// Ring kapasitesi
    #[inline]
    pub const fn capacity(&self) -> usize {
        N
    }

    // ========================================================================
    // PRODUCER API (Jail Worker Thread tarafı)
    // ========================================================================

    /// Bir eleman ekler (producer tarafı).
    ///
    /// ## Sıralama Garantisi
    /// 1. Veri yazılır (write_volatile)
    /// 2. smp_wmb() — yazma bariyeri
    /// 3. tail atomik artırılır (Release)
    ///
    /// ## Dönüş
    /// - `Ok(())`: Başarıyla eklendi
    /// - `Err(item)`: Ring dolu, eleman geri verilir
    pub fn push(&self, item: T) -> Result<(), T> {
        let tail = self.tail.0.load(Ordering::Relaxed);
        let head = self.head.0.load(Ordering::Acquire);

        // Dolu mu?
        if tail.wrapping_sub(head) >= N as u32 {
            return Err(item);
        }

        let index = (tail & Self::mask()) as usize;

        // 1. Veri yaz
        unsafe {
            let buf = self.buffer.get();
            (*buf)[index] = MaybeUninit::new(item);
        }

        // 2. Yazma bariyeri
        crate::memory_barriers::smp_wmb();

        // 3. Tail artır
        self.tail.0.store(tail.wrapping_add(1), Ordering::Release);

        Ok(())
    }

    /// Birden fazla eleman toplu ekler (batch push).
    ///
    /// Tek smp_wmb() ile N eleman yazılır — bariyer maliyeti amortisman.
    pub fn push_batch(&self, items: &[T]) -> usize {
        let tail = self.tail.0.load(Ordering::Relaxed);
        let head = self.head.0.load(Ordering::Acquire);

        let available = (N as u32).wrapping_sub(tail.wrapping_sub(head)) as usize;
        let count = items.len().min(available);

        if count == 0 {
            return 0;
        }

        unsafe {
            let buf = self.buffer.get();
            for i in 0..count {
                let index = ((tail.wrapping_add(i as u32)) & Self::mask()) as usize;
                (*buf)[index] = MaybeUninit::new(items[i]);
            }
        }

        crate::memory_barriers::smp_wmb();
        self.tail
            .0
            .store(tail.wrapping_add(count as u32), Ordering::Release);

        count
    }

    // ========================================================================
    // CONSUMER API (echOS Core tarafı)
    // ========================================================================

    /// Bir eleman okur (consumer tarafı).
    ///
    /// ## Sıralama Garantisi
    /// 1. tail atomik okunur (Acquire)
    /// 2. smp_rmb() — okuma bariyeri
    /// 3. Veri okunur (read_volatile)
    /// 4. head atomik artırılır (Release)
    pub fn pop(&self) -> Option<T> {
        let head = self.head.0.load(Ordering::Relaxed);
        let tail = self.tail.0.load(Ordering::Acquire);

        // Boş mu?
        if head == tail {
            return None;
        }

        // 1. Okuma bariyeri
        crate::memory_barriers::smp_rmb();

        let index = (head & Self::mask()) as usize;

        // 2. Veri oku
        let item = unsafe {
            let buf = self.buffer.get();
            (*buf)[index].assume_init()
        };

        // 3. Head artır
        self.head.0.store(head.wrapping_add(1), Ordering::Release);

        Some(item)
    }

    /// Birden fazla eleman toplu okur (batch pop).
    ///
    /// Tek smp_rmb() ile N eleman okunur.
    pub fn pop_batch(&self, out: &mut [T]) -> usize {
        let head = self.head.0.load(Ordering::Relaxed);
        let tail = self.tail.0.load(Ordering::Acquire);

        let available = tail.wrapping_sub(head) as usize;
        let count = available.min(out.len());

        if count == 0 {
            return 0;
        }

        crate::memory_barriers::smp_rmb();

        unsafe {
            let buf = self.buffer.get();
            for i in 0..count {
                let index = ((head.wrapping_add(i as u32)) & Self::mask()) as usize;
                out[i] = (*buf)[index].assume_init();
            }
        }

        self.head
            .0
            .store(head.wrapping_add(count as u32), Ordering::Release);

        count
    }

    /// Peek: elemanı okur ama çıkarmaz.
    ///
    /// Consumer tarafında kullanılır; head ilerletilmez.
    pub fn peek(&self) -> Option<T> {
        let head = self.head.0.load(Ordering::Relaxed);
        let tail = self.tail.0.load(Ordering::Acquire);

        if head == tail {
            return None;
        }

        crate::memory_barriers::smp_rmb();

        let index = (head & Self::mask()) as usize;
        let item = unsafe {
            let buf = self.buffer.get();
            (*buf)[index].assume_init()
        };

        Some(item)
    }
}

// ============================================================================
// Jail İletişim Ring'leri (Tip Takma Adları)
// ============================================================================

/// Jail → Core yönünde tamamlama olayları ring'i
pub type EventRing = SpscRing<JailEvent, JAIL_RING_SIZE>;

/// Core → Jail yönünde istek ring'i
pub type RequestRing = SpscRing<JailRequest, JAIL_RING_SIZE>;

/// Çift yönlü jail iletişim kanalı.
///
/// Bir TIER 2 sürücüsü ile echOS core arasındaki tam iletişim yapısı:
/// - `requests`: Core → Jail (I/O istekleri)
/// - `events`: Jail → Core (tamamlama olayları)
pub struct JailChannel {
    /// Core'dan jail'e gönderilen istekler (Core = producer, Jail = consumer)
    pub requests: RequestRing,
    /// Jail'den core'a gönderilen olaylar (Jail = producer, Core = consumer)
    pub events: EventRing,
    /// Kanal kimliği (jail_id ile eşleşir)
    pub channel_id: u16,
    /// İstatistik: toplam gönderilen istek
    pub total_requests: core::sync::atomic::AtomicU64,
    /// İstatistik: toplam alınan olay
    pub total_events: core::sync::atomic::AtomicU64,
    /// İstatistik: düşürülen istek (ring dolu)
    pub dropped_requests: core::sync::atomic::AtomicU64,
    /// İstatistik: düşürülen olay (ring dolu)
    pub dropped_events: core::sync::atomic::AtomicU64,
}

impl JailChannel {
    /// Yeni bir jail iletişim kanalı oluşturur.
    pub fn new(channel_id: u16) -> Self {
        Self {
            requests: RequestRing::new(),
            events: EventRing::new(),
            channel_id,
            total_requests: core::sync::atomic::AtomicU64::new(0),
            total_events: core::sync::atomic::AtomicU64::new(0),
            dropped_requests: core::sync::atomic::AtomicU64::new(0),
            dropped_events: core::sync::atomic::AtomicU64::new(0),
        }
    }

    // ======================== Core API ========================

    /// Core: Jail'e I/O isteği gönderir.
    pub fn submit_request(&self, req: JailRequest) -> Result<(), JailRequest> {
        match self.requests.push(req) {
            Ok(()) => {
                self.total_requests.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            Err(req) => {
                self.dropped_requests.fetch_add(1, Ordering::Relaxed);
                Err(req)
            }
        }
    }

    /// Core: Jail'den tamamlama olayı okur.
    pub fn poll_event(&self) -> Option<JailEvent> {
        let event = self.events.pop()?;
        self.total_events.fetch_add(1, Ordering::Relaxed);
        Some(event)
    }

    /// Core: Birden fazla tamamlama olayı toplu okur.
    pub fn poll_events(&self, out: &mut [JailEvent]) -> usize {
        let count = self.events.pop_batch(out);
        self.total_events.fetch_add(count as u64, Ordering::Relaxed);
        count
    }

    // ======================== Jail API ========================

    /// Jail: İstek okur (I/O komutu alır).
    pub fn poll_request(&self) -> Option<JailRequest> {
        self.requests.pop()
    }

    /// Jail: Tamamlama olayı gönderir.
    pub fn submit_event(&self, event: JailEvent) -> Result<(), JailEvent> {
        match self.events.push(event) {
            Ok(()) => Ok(()),
            Err(event) => {
                self.dropped_events.fetch_add(1, Ordering::Relaxed);
                Err(event)
            }
        }
    }

    // ======================== İstatistik ========================

    /// Kanal istatistiklerini seri porta yazdırır.
    pub fn print_stats(&self) {
        crate::serial_println!(
            "[JailChannel {}] requests={} events={} dropped_req={} dropped_evt={}",
            self.channel_id,
            self.total_requests.load(Ordering::Relaxed),
            self.total_events.load(Ordering::Relaxed),
            self.dropped_requests.load(Ordering::Relaxed),
            self.dropped_events.load(Ordering::Relaxed),
        );
    }

    /// Bekleyen istek sayısını döner.
    pub fn pending_requests(&self) -> u32 {
        self.requests.len()
    }

    /// Bekleyen olay sayısını döner.
    pub fn pending_events(&self) -> u32 {
        self.events.len()
    }
}
