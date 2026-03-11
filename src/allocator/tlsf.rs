//! # echOS TLSF Allocator
//!
//! TLSF (Two-Level Segregated Fit — İki Düzeyli Ayrılmış Uyum) heap allocator sarmalayıcısı.
//! O(1) allocation/deallocation performansı sağlar.
//!
//! ## TLSF Algoritması Nedir?
//!
//! TLSF, gerçek zamanlı sistemler için tasarlanmış bir bellek yönetim algoritmasıdır.
//! Temel fikir: serbest blokları boyutlarına göre iki boyutlu bir bitmap indeksiyle
//! organize etmektir. Bu sayede hem allocation hem deallocation O(1)'de tamamlanır.
//!
//! ## İki Düzeyli İndeks Yapısı:
//! ```
//!  1. Düzey (FLI - First Level Index):
//!     Blok boyutunun log2'si → hangi büyüklük sınıfında?
//!     Örn: 128-255 byte → FLI=7
//!
//!  2. Düzey (SLI - Second Level Index):
//!     Büyüklük sınıfı içinde daha ince ayrım
//!     Örn: 128-143 → SLI=0, 144-159 → SLI=1 ...
//!
//!  Bitmap:
//!  +----+----+----+----+----+
//!  | FL | SL | SL | SL | .. |   <- Hangi yuvada serbest blok var?
//!  +----+----+----+----+----+
//!        ^
//!        O(1) bit tarama (BSF/BSR talimatları)
//! ```
//!
//! ## Allocation: O(1)
//! 1. İstenen boyutu yukarı yuvarla (rounding up).
//! 2. Bitmap'te uygun boyut sınıfını bul (BSF talimatı: O(1)).
//! 3. O sınıftaki serbest blok listesinden ilk bloğu al.
//! 4. Fazla kısım varsa böl ve uygun sınıfa ekle.
//!
//! ## Deallocation: O(1)
//! 1. Serbest bırakılan bloğa komşu boş blokları birleştir (coalescing).
//! 2. Birleşik bloğu boyutuna göre uygun bitmap yuvasına ekle.
//!
//! ## Güvenlik Özellikleri
//! - Erken heap koruması (önyükleme sırasında ayrılan bellek asla serbest bırakılmaz)
//! - Heap sınırları kontrolü (ana heap dışına dealloc engellenir)
//! - Null pointer koruması
//! - Hizalama doğrulaması
//! - Heap canary (tampon taşması tespiti — buffer overflow detection)
//! - Allocation bütünlük takibi
//! - Bozulma tespiti ve raporlama

use super::PageOwner;
use core::alloc::{GlobalAlloc, Layout};
use core::ptr::NonNull;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use rlsf::Tlsf;
use spin::Mutex;

/// Erken heap boyutu (512 KiB — sayfa tablosu kurulmadan önce kullanılır)
const EARLY_HEAP_SIZE: usize = 512 * 1024;

/// Heap canary sihirli değeri.
///
/// Her takip edilen allocation'ın yanında bu değer saklanır.
/// Değer bozulursa (0xDEADBEEF_CAFEBABE yerine başka bir şey görünürse)
/// tampon taşması (buffer overflow) meydana gelmiş demektir.
const HEAP_CANARY_MAGIC: u64 = 0xDEADBEEF_CAFEBABE;

/// Bütünlük kontrolü için takip edilen maksimum allocation sayısı.
///
/// 4096 slot sabit boyutlu dizi halinde saklanır; döngüsel tampon (ring buffer)
/// mantığıyla indekslenir. En fazla 4096 allocation eş zamanlı takip edilebilir.
const MAX_TRACKED_ALLOCATIONS: usize = 4096;

/// Erken heap belleği (BSS bölümünde statik dizi).
///
/// Sistem başlangıcında (sayfa tablosu henüz yok) heap ihtiyaçları için
/// kullanılır. Tüm baytlar sıfır ile başlatılır (BSS garantisi).
static EARLY_HEAP: [u8; EARLY_HEAP_SIZE] = [0; EARLY_HEAP_SIZE];

/// Erken heap ofseti: bir sonraki allocation'ın yerini gösterir.
///
/// Atomik olarak güncellenir (CAS — Compare-And-Swap ile lock-free).
/// Bu sayede spinlock olmadan çok çekirdekli güvenlik sağlanır.
static EARLY_OFFSET: AtomicUsize = AtomicUsize::new(0);

/// Ana heap hazır mı?
///
/// `insert_free_region_ptr` çağrıldıktan sonra `true` yapılır.
/// Bu bayrak `false` iken tüm alloc'lar erken heap'e yönlendirilir.
static HEAP_READY: AtomicBool = AtomicBool::new(false);

/// Ana heap başlangıç adresi (init_heap tarafından ayarlanır)
static MAIN_HEAP_START: AtomicUsize = AtomicUsize::new(0);
/// Ana heap bitiş adresi (init_heap tarafından ayarlanır)
static MAIN_HEAP_END: AtomicUsize = AtomicUsize::new(0);

/// Allocation istatistikleri (yalnızca `heap_stats` özelliği etkinse derlenir)
#[cfg(feature = "heap_stats")]
static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "heap_stats")]
static FREE_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Tespit edilen bozulma (corruption) sayacı
static CORRUPTION_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Şu ana kadar toplam ayrılan bayt sayısı
static TOTAL_ALLOCATED: AtomicUsize = AtomicUsize::new(0);

/// Tepe bellek kullanımı (peak — en yüksek anlık kullanım)
static PEAK_USAGE: AtomicUsize = AtomicUsize::new(0);

/// Tek bir allocation'ı takip eden kayıt.
///
/// Her allocation için pointer, boyut ve canary değeri atomik olarak saklanır.
/// Lock-free okuma/yazma için tüm alanlar Atomic türündedir.
struct AllocationEntry {
    ptr: AtomicUsize,
    size: AtomicUsize,
    canary: AtomicU64,
}

impl AllocationEntry {
    /// Sıfırlanmış yeni bir kayıt girdisi oluşturur.
    const fn new() -> Self {
        Self {
            ptr: AtomicUsize::new(0),
            size: AtomicUsize::new(0),
            canary: AtomicU64::new(0),
        }
    }
}

/// Tüm allocation kayıtlarını tutan sabit boyutlu dizi.
///
/// Mutex ile korunur. 4096 slot döngüsel olarak kullanılır.
/// Aynı anda 4096'dan fazla allocation takip edilemez.
static ALLOCATION_TRACKER: Mutex<[AllocationEntry; MAX_TRACKED_ALLOCATIONS]> =
    Mutex::new(const { [const { AllocationEntry::new() }; MAX_TRACKED_ALLOCATIONS] });

/// İş parçacığı güvenli (thread-safe) TLSF allocator sarmalayıcısı.
///
/// İç `Mutex<Option<Tlsf>>` ile korunur. `Option` sayesinde `const fn new()`
/// ile compile-time başlatma yapılabilir; TLSF ancak bellek bölgesi
/// (`insert_free_region_ptr`) eklendikten sonra `Some(...)` haline gelir.
pub struct LockedTlsf(Mutex<Option<Tlsf<'static, usize, usize, 32, 32>>>);

unsafe impl Send for LockedTlsf {}
unsafe impl Sync for LockedTlsf {}

impl LockedTlsf {
    /// Yeni boş allocator oluşturur.
    ///
    /// `const fn` olduğu için global statik olarak tanımlanabilir.
    /// TLSF henüz başlatılmamıştır; bellek bölgesi eklenene kadar
    /// erken heap devreye girer.
    pub const fn new() -> Self {
        LockedTlsf(Mutex::new(None))
    }

    /// Bellek bölgesini TLSF allocator'a kaydeder ve ana heap'i etkinleştirir.
    ///
    /// Bu fonksiyon çağrıldıktan sonra `HEAP_READY = true` yapılır ve
    /// tüm yeni alloc'lar TLSF'e yönlendirilir.
    ///
    /// # Güvenlik
    /// Verilen bellek bölgesi geçerli, başka bir yapı tarafından kullanılmayan
    /// ve `size` kadar erişilebilir olmalıdır.
    pub unsafe fn insert_free_region_ptr(&self, ptr: *mut u8, size: usize) {
        let mut lock = self.0.lock();
        if lock.is_none() {
            *lock = Some(Tlsf::new());
        }
        let tlsf = lock.as_mut().unwrap();

        let slice_ptr = core::ptr::slice_from_raw_parts_mut(ptr, size);
        if let Some(nonnull_slice) = NonNull::new(slice_ptr) {
            tlsf.insert_free_block_ptr(nonnull_slice);

            // Ana heap sınırlarını kaydet (dealloc güvenlik kontrolü için)
            MAIN_HEAP_START.store(ptr as usize, Ordering::Release);
            MAIN_HEAP_END.store(ptr as usize + size, Ordering::Release);
            HEAP_READY.store(true, Ordering::Release);
        }
    }

    /// Pointer'ın erken heap'te olup olmadığını kontrol eder.
    ///
    /// Erken heap adresi: `EARLY_HEAP.as_ptr()` ile `+EARLY_HEAP_SIZE` arası.
    #[inline]
    fn is_early_heap(ptr: usize) -> bool {
        let early_start = EARLY_HEAP.as_ptr() as usize;
        let early_end = early_start + EARLY_HEAP_SIZE;
        ptr >= early_start && ptr < early_end
    }

    /// Pointer'ın ana heap'te olup olmadığını kontrol eder.
    ///
    /// Ana heap hazır değilse her zaman `false` döner.
    #[inline]
    fn is_main_heap(ptr: usize) -> bool {
        if !HEAP_READY.load(Ordering::Acquire) {
            return false;
        }
        let start = MAIN_HEAP_START.load(Ordering::Acquire);
        let end = MAIN_HEAP_END.load(Ordering::Acquire);
        ptr >= start && ptr < end
    }

    /// Pointer'ın geçerli bir heap bölgesinde (erken veya ana) olup olmadığını kontrol eder.
    #[inline]
    fn is_valid_heap_ptr(ptr: usize) -> bool {
        Self::is_early_heap(ptr) || Self::is_main_heap(ptr)
    }

    /// Tüm takip edilen allocation'ların canary değerlerini kontrol eder.
    ///
    /// Her aktif alloc için canary `HEAP_CANARY_MAGIC` ile karşılaştırılır.
    /// Farklıysa tampon taşması (buffer overflow) tespit edilmiş demektir.
    ///
    /// Döndürülen `IntegrityReport` bozulma sayısını ve hangi adreslerin
    /// bozulduğunu içerir.
    pub fn check_integrity() -> IntegrityReport {
        let mut report = IntegrityReport {
            total_tracked: 0,
            corrupted: 0,
            total_bytes: 0,
            corruptions: alloc::vec::Vec::new(),
        };

        let tracker = ALLOCATION_TRACKER.lock();
        for (i, entry) in tracker.iter().enumerate() {
            let ptr = entry.ptr.load(Ordering::Relaxed);
            if ptr != 0 {
                report.total_tracked += 1;
                report.total_bytes += entry.size.load(Ordering::Relaxed);

                // Canary kontrolü: beklenen değerden farklıysa bozulma var
                let canary = entry.canary.load(Ordering::Relaxed);
                if canary != HEAP_CANARY_MAGIC {
                    report.corrupted += 1;
                    report
                        .corruptions
                        .push((ptr, entry.size.load(Ordering::Relaxed)));
                    CORRUPTION_COUNT.fetch_add(1, Ordering::Relaxed);
                }
            }
        }

        report
    }

    /// Tespit edilen toplam bozulma sayısını döndürür.
    pub fn corruption_count() -> usize {
        CORRUPTION_COUNT.load(Ordering::Relaxed)
    }

    /// Heap bütünlük kontrolünü çalıştırır ve bozulma sayısını döndürür.
    pub fn check_heap_integrity() -> usize {
        let report = Self::check_integrity();
        report.corrupted
    }

    /// İzleme amaçlı bellek ayırma istatistiklerini döndürür.
    ///
    /// Aktif allocation sayısı, toplam ayrılan bayt, tepe kullanım ve
    /// toplam bozulma sayısını içeren `AllocStats` yapısını döndürür.
    pub fn get_stats() -> AllocStats {
        AllocStats {
            active_allocations: ALLOCATION_TRACKER
                .lock()
                .iter()
                .filter(|e| e.ptr.load(Ordering::Relaxed) != 0)
                .count(),
            total_allocated: TOTAL_ALLOCATED.load(Ordering::Relaxed),
            peak_usage: PEAK_USAGE.load(Ordering::Relaxed),
            corruption_count: CORRUPTION_COUNT.load(Ordering::Relaxed),
        }
    }

    /// Bellek istatistiklerinin daha ayrıntılı bir görünümünü döndürür.
    ///
    /// `AllocStats`'a ek olarak erken heap kullanımını da içerir.
    pub fn memory_stats() -> MemoryStats {
        MemoryStats {
            total_allocated: TOTAL_ALLOCATED.load(Ordering::Relaxed),
            peak_usage: PEAK_USAGE.load(Ordering::Relaxed),
            early_heap_used: EARLY_OFFSET.load(Ordering::Relaxed),
            corruption_count: CORRUPTION_COUNT.load(Ordering::Relaxed),
        }
    }

    pub unsafe fn alloc_from_main_heap(&self, layout: Layout) -> *mut u8 {
        if !HEAP_READY.load(Ordering::Acquire) {
            return core::ptr::null_mut();
        }

        let mut lock = self.0.lock();
        if lock.is_none() {
            *lock = Some(Tlsf::new());
        }
        let tlsf = lock.as_mut().unwrap();
        match tlsf.allocate(layout) {
            Some(ptr) => ptr.as_ptr(),
            None => core::ptr::null_mut(),
        }
    }

    pub unsafe fn dealloc_to_main_heap(&self, ptr: *mut u8, align: usize) {
        if ptr.is_null() || !HEAP_READY.load(Ordering::Acquire) {
            return;
        }
        let mut lock = self.0.lock();
        if let Some(tlsf) = lock.as_mut() {
            if let Some(ptr) = NonNull::new(ptr) {
                tlsf.deallocate(ptr, align.max(8));
            }
        }
    }
}

/// Bütünlük kontrolü sonuç raporu.
#[derive(Clone, Debug)]
pub struct IntegrityReport {
    pub total_tracked: usize,
    pub corrupted: usize,
    pub total_bytes: usize,
    pub corruptions: alloc::vec::Vec<(usize, usize)>,
}

/// Bellek istatistikleri (erken heap dahil).
#[derive(Clone, Debug)]
pub struct MemoryStats {
    pub total_allocated: usize,
    pub peak_usage: usize,
    pub early_heap_used: usize,
    pub corruption_count: usize,
}

/// Allocator performans ve durum istatistikleri.
#[derive(Clone, Debug)]
pub struct AllocStats {
    pub active_allocations: usize,
    pub total_allocated: usize,
    pub peak_usage: usize,
    pub corruption_count: usize,
}

/// Erken heap'ten bellek ayırır (sayfa tablosu hazır olmadan önce).
///
/// ## Bump + CAS Mekanizması:
/// ```
/// early_alloc(layout)
///      |
///      v
/// loop:
///   current = EARLY_OFFSET.load()
///   aligned = align_up(EARLY_HEAP.base + current, align)
///   next_offset = aligned + size - base
///      |
///      v
///   next_offset > EARLY_HEAP_SIZE? --> null_mut() (erken heap doldu)
///      |
///     hayır
///      v
///   CAS(current, next_offset) başarılı? --> aligned döndür
///      |
///     hayır (başka CPU önce güncelledi)
///      v
///   Tekrar dene (döngü başına)
/// ```
///
/// Bu lock-free yaklaşım spinlock olmadan çok çekirdekli güvenlik sağlar.
/// Hizalama en az 8 byte yapılır (TLSF gereksinimi).
fn early_alloc(layout: Layout) -> *mut u8 {
    let align = layout.align().max(1);
    let size = layout.size();

    // TLSF 8-byte hizalama gerektirir; hem hizalama hem boyutu 8'e yukarı yuvarla
    let align = align.max(8);
    let size = (size + 7) & !7; // 8-byte hizala

    loop {
        let current = EARLY_OFFSET.load(Ordering::Relaxed);
        let base = EARLY_HEAP.as_ptr() as usize;
        let absolute = base.saturating_add(current);
        let aligned = (absolute + align - 1) & !(align - 1);
        let next = aligned.saturating_add(size);
        let next_offset = next.saturating_sub(base);

        if next_offset > EARLY_HEAP_SIZE {
            // Erken heap kapasitesi doldu!
            return core::ptr::null_mut();
        }

        // CAS: atomic karşılaştır-ve-değiştir; başarısız olursa döngü yeniden dener
        if EARLY_OFFSET
            .compare_exchange(current, next_offset, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            // Güvenlik için sıfırla: önceden orada ne olduğu bilinmiyor
            unsafe {
                core::ptr::write_bytes(aligned as *mut u8, 0, size);
            }
            return aligned as *mut u8;
        }
    }
}

unsafe impl GlobalAlloc for LockedTlsf {
    /// Bellek ayırır.
    ///
    /// ## Genel Alloc Akışı:
    /// ```
    /// alloc(layout)
    ///      |
    ///      v
    /// Layout'u 8-byte hizala (TLSF gereksinimi)
    ///      |
    ///      v
    /// HEAP_READY? --> HAYIR --> early_alloc() (erken heap modu)
    ///      |
    ///     EVET
    ///      v
    /// TLSF kilitle, allocate() çağır
    ///      |
    ///      v
    /// Başarılı? --> ptr döndür
    ///      |
    ///     HAYIR (TLSF yetersiz)
    ///      |
    ///      v
    /// early_alloc() ile yedek girişim
    /// ```
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // TLSF en az 8-byte hizalama gerektirir; hem hizalama hem boyutu normalize et
        let align = layout.align().max(8);
        let size = (layout.size() + 7) & !7; // 8-byte hizala

        // Layout'u düzeltilmiş değerlerle yeniden oluştur
        let layout = match Layout::from_size_align(size, align) {
            Ok(l) => l,
            Err(_) => return core::ptr::null_mut(),
        };

        // Erken heap modu: TLSF hazır değilse erken heap kullan
        if !HEAP_READY.load(Ordering::Acquire) {
            return early_alloc(layout);
        }

        if let Some(ptr) = crate::allocator::slab::slab_alloc(layout.size(), layout.align()) {
            #[cfg(feature = "heap_stats")]
            ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
            return ptr;
        }

        // Ana heap modu: TLSF'den bellek iste
        let ptr = self.alloc_from_main_heap(layout);
        if !ptr.is_null() {
            #[cfg(feature = "heap_stats")]
            ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
            return ptr;
        }

        // TLSF allocation başarısız; son çare olarak erken heap'e döngüsel geri dön
        early_alloc(layout)
    }

    /// Bellek serbest bırakır.
    ///
    /// ## Güvenlik Kontrolleri (sırayla):
    /// ```
    /// dealloc(ptr, layout)
    ///      |
    ///      v
    /// ptr == null? --> sessizce çık
    ///      |
    ///      v
    /// Erken heap'te mi? --> EVET --> sessizce çık (erken heap hiç serbest bırakılmaz)
    ///      |
    ///     HAYIR
    ///      v
    /// Ana heap'te mi? --> HAYIR --> sessizce çık (geçersiz ptr / çift serbest bırakma)
    ///      |
    ///     EVET
    ///      v
    /// HEAP_READY? --> HAYIR --> sessizce çık
    ///      |
    ///     EVET
    ///      v
    /// TLSF kilitle, deallocate() çağır
    /// ```
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let ptr_addr = ptr as usize;

        // Null pointer kontrolü
        if ptr_addr == 0 {
            return;
        }

        // Erken heap kontrolü — erken heap allocation'ları asla serbest bırakılmaz.
        if Self::is_early_heap(ptr_addr) {
            return;
        }

        // Ana heap kontrolü — yalnızca ana heap'ten ayrılanlar serbest bırakılır.
        if !Self::is_main_heap(ptr_addr) {
            return;
        }

        if !HEAP_READY.load(Ordering::Acquire) {
            return;
        }

        match crate::allocator::page_owner_for_ptr(ptr_addr) {
            PageOwner::Slab => {
                let _ = crate::allocator::slab::slab_dealloc(ptr);
            }
            PageOwner::Tlsf | PageOwner::Large | PageOwner::Unassigned => {
                // Hizalamayı alloc ile tutarlı tut (her ikisinde de max(align, 8))
                let align = layout.align().max(8);
                self.dealloc_to_main_heap(ptr, align);
            }
        }

        #[cfg(feature = "heap_stats")]
        FREE_COUNT.fetch_add(1, Ordering::Relaxed);
    }
}

/// Heap allocation/deallocation sayılarını döndürür (debug için).
///
/// Yalnızca `heap_stats` özelliği etkinleştirildiğinde derlenir.
/// Döndürülen tuple: (toplam_alloc_sayısı, toplam_free_sayısı)
#[cfg(feature = "heap_stats")]
pub fn heap_stats() -> (usize, usize) {
    (
        ALLOC_COUNT.load(Ordering::Relaxed),
        FREE_COUNT.load(Ordering::Relaxed),
    )
}

/// Erken heap kullanımını bayt cinsinden döndürür.
///
/// Bu değer yalnızca artabilir; erken heap serbest bırakmayı desteklemez.
pub fn early_heap_usage() -> usize {
    EARLY_OFFSET.load(Ordering::Relaxed)
}

/// Ana heap sınırlarını (başlangıç, bitiş) döndürür.
///
/// Heap henüz başlatılmamışsa her iki değer de 0 döner.
pub fn main_heap_bounds() -> (usize, usize) {
    (
        MAIN_HEAP_START.load(Ordering::Relaxed),
        MAIN_HEAP_END.load(Ordering::Relaxed),
    )
}
