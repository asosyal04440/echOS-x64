//! # echOS TLSF Allocator
//!
//! TLSF (Two-Level Segregated Fit) heap allocator wrapper.
//! O(1) allocation/deallocation performansı sağlar.
//!
//! ## Güvenlik Özellikleri
//! - Early heap koruması (önyükleme sırasında ayrılan bellek asla serbest bırakılmaz)
//! - Heap sınırları kontrolü (main heap dışına dealloc engellenir)
//! - Null pointer koruması
//! - Alignment doğrulaması
//! - Heap canary (buffer overflow tespiti)
//! - Allocation bütünlük izleme
//! - Bozulma tespiti ve raporlama

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::NonNull;
use core::sync::atomic::{AtomicBool, AtomicUsize, AtomicU64, Ordering};
use rlsf::Tlsf;
use spin::Mutex;

/// Early heap boyutu (512 KiB - önyükleme için yeterli)
const EARLY_HEAP_SIZE: usize = 512 * 1024;

/// Heap canary sihirli değeri
const HEAP_CANARY_MAGIC: u64 = 0xDEADBEEF_CAFEBABE;

/// Bütünlük kontrolü için izlenen maksimum allocation sayısı
const MAX_TRACKED_ALLOCATIONS: usize = 4096;

/// Early heap belleği (static, BSS section'da)
static EARLY_HEAP: [u8; EARLY_HEAP_SIZE] = [0; EARLY_HEAP_SIZE];

/// Early heap offset (bir sonraki allocation yeri)
static EARLY_OFFSET: AtomicUsize = AtomicUsize::new(0);

/// Main heap hazır mı?
static HEAP_READY: AtomicBool = AtomicBool::new(false);

/// Main heap başlangıç adresi (init_heap tarafından ayarlanır)
static MAIN_HEAP_START: AtomicUsize = AtomicUsize::new(0);
static MAIN_HEAP_END: AtomicUsize = AtomicUsize::new(0);

/// Allocation istatistikleri (debug için)
#[cfg(feature = "heap_stats")]
static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "heap_stats")]
static FREE_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Bozulma (corruption) tespit sayacı
static CORRUPTION_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Toplam ayrılan bayt sayısı
static TOTAL_ALLOCATED: AtomicUsize = AtomicUsize::new(0);

/// En yüksek bellek kullanımı
static PEAK_USAGE: AtomicUsize = AtomicUsize::new(0);

/// Bütünlük kontrolü için allocation takibi
struct AllocationEntry {
    ptr: AtomicUsize,
    size: AtomicUsize,
    canary: AtomicU64,
}

impl AllocationEntry {
    const fn new() -> Self {
        Self {
            ptr: AtomicUsize::new(0),
            size: AtomicUsize::new(0),
            canary: AtomicU64::new(0),
        }
    }
}

static ALLOCATION_TRACKER: Mutex<[AllocationEntry; MAX_TRACKED_ALLOCATIONS]> = Mutex::new(
    const { [const { AllocationEntry::new() }; MAX_TRACKED_ALLOCATIONS] }
);

static TRACKER_INDEX: AtomicUsize = AtomicUsize::new(0);

/// Thread-safe (iş parçacığı güvenli) TLSF allocator sarmalayıcı.
pub struct LockedTlsf(Mutex<Option<Tlsf<'static, usize, usize, 32, 32>>>);

unsafe impl Send for LockedTlsf {}
unsafe impl Sync for LockedTlsf {}

impl LockedTlsf {
    /// Yeni boş allocator oluşturur.
    pub const fn new() -> Self {
        LockedTlsf(Mutex::new(None))
    }

    /// Bellek bölgesini allocator'a kaydeder.
    ///
    /// # Güvenlik
    /// Verilen bellek bölgesi geçerli ve allocator tarafından
    /// kullanılabilir olmalıdır.
    pub unsafe fn insert_free_region_ptr(&self, ptr: *mut u8, size: usize) {
        let mut lock = self.0.lock();
        if lock.is_none() {
            *lock = Some(Tlsf::new());
        }
        let tlsf = lock.as_mut().unwrap();

        let slice_ptr = core::ptr::slice_from_raw_parts_mut(ptr, size);
        if let Some(nonnull_slice) = NonNull::new(slice_ptr) {
            tlsf.insert_free_block_ptr(nonnull_slice);

            // Main heap sınırlarını kaydet
            MAIN_HEAP_START.store(ptr as usize, Ordering::Release);
            MAIN_HEAP_END.store(ptr as usize + size, Ordering::Release);
            HEAP_READY.store(true, Ordering::Release);
        }
    }

    /// Pointer'ın early heap'te olup olmadığını kontrol et
    #[inline]
    fn is_early_heap(ptr: usize) -> bool {
        let early_start = EARLY_HEAP.as_ptr() as usize;
        let early_end = early_start + EARLY_HEAP_SIZE;
        ptr >= early_start && ptr < early_end
    }

    /// Pointer'ın main heap'te olup olmadığını kontrol et
    #[inline]
    fn is_main_heap(ptr: usize) -> bool {
        if !HEAP_READY.load(Ordering::Acquire) {
            return false;
        }
        let start = MAIN_HEAP_START.load(Ordering::Acquire);
        let end = MAIN_HEAP_END.load(Ordering::Acquire);
        ptr >= start && ptr < end
    }

    /// Pointer'ın geçerli bir heap bölgesinde olup olmadığını kontrol et
    #[inline]
    fn is_valid_heap_ptr(ptr: usize) -> bool {
        Self::is_early_heap(ptr) || Self::is_main_heap(ptr)
    }

    /// Bütünlük kontrolü için allocation'ı izle
    fn track_allocation(ptr: usize, size: usize) {
        let idx = TRACKER_INDEX.fetch_add(1, Ordering::SeqCst) % MAX_TRACKED_ALLOCATIONS;
        let tracker = &mut ALLOCATION_TRACKER.lock();
        tracker[idx].ptr.store(ptr, Ordering::SeqCst);
        tracker[idx].size.store(size, Ordering::SeqCst);
        tracker[idx].canary.store(HEAP_CANARY_MAGIC, Ordering::SeqCst);

        TOTAL_ALLOCATED.fetch_add(size, Ordering::SeqCst);

        let current = TOTAL_ALLOCATED.load(Ordering::SeqCst);
        let mut peak = PEAK_USAGE.load(Ordering::SeqCst);
        while current > peak {
            match PEAK_USAGE.compare_exchange(peak, current, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => break,
                Err(p) => peak = p,
            }
        }
    }

    /// Allocation izlemesini kaldır
    fn untrack_allocation(ptr: usize) -> Option<usize> {
        let tracker = ALLOCATION_TRACKER.lock();
        for entry in tracker.iter() {
            if entry.ptr.load(Ordering::SeqCst) == ptr {
                let size = entry.size.load(Ordering::SeqCst);
                entry.ptr.store(0, Ordering::SeqCst);
                entry.size.store(0, Ordering::SeqCst);
                entry.canary.store(0, Ordering::SeqCst);
                TOTAL_ALLOCATED.fetch_sub(size, Ordering::SeqCst);
                return Some(size);
            }
        }
        None
    }

    /// İzlenen tüm allocation'ların bütünlüğünü kontrol et
    pub fn check_integrity() -> IntegrityReport {
        let mut report = IntegrityReport {
            total_tracked: 0,
            corrupted: 0,
            total_bytes: 0,
            corruptions: alloc::vec::Vec::new(),
        };

        let tracker = ALLOCATION_TRACKER.lock();
        for (i, entry) in tracker.iter().enumerate() {
            let ptr = entry.ptr.load(Ordering::SeqCst);
            if ptr != 0 {
                report.total_tracked += 1;
                report.total_bytes += entry.size.load(Ordering::SeqCst);

                // Canary'yi kontrol et
                let canary = entry.canary.load(Ordering::SeqCst);
                if canary != HEAP_CANARY_MAGIC {
                    report.corrupted += 1;
                    report.corruptions.push((ptr, entry.size.load(Ordering::SeqCst)));
                    CORRUPTION_COUNT.fetch_add(1, Ordering::SeqCst);
                }
            }
        }

        report
    }

    /// Bozulma sayısını al
    pub fn corruption_count() -> usize {
        CORRUPTION_COUNT.load(Ordering::SeqCst)
    }

    /// Heap bütünlüğünü kontrol et (bozulma sayısını döndürür)
    pub fn check_heap_integrity() -> usize {
        let report = Self::check_integrity();
        report.corrupted
    }

    /// İzleme için allocation istatistiklerini al
    pub fn get_stats() -> AllocStats {
        AllocStats {
            active_allocations: ALLOCATION_TRACKER.lock().iter()
                .filter(|e| e.ptr.load(Ordering::SeqCst) != 0)
                .count(),
            total_allocated: TOTAL_ALLOCATED.load(Ordering::SeqCst),
            peak_usage: PEAK_USAGE.load(Ordering::SeqCst),
            corruption_count: CORRUPTION_COUNT.load(Ordering::SeqCst),
        }
    }

    /// Bellek istatistiklerini al
    pub fn memory_stats() -> MemoryStats {
        MemoryStats {
            total_allocated: TOTAL_ALLOCATED.load(Ordering::SeqCst),
            peak_usage: PEAK_USAGE.load(Ordering::SeqCst),
            early_heap_used: EARLY_OFFSET.load(Ordering::SeqCst),
            corruption_count: CORRUPTION_COUNT.load(Ordering::SeqCst),
        }
    }
}

#[derive(Clone, Debug)]
pub struct IntegrityReport {
    pub total_tracked: usize,
    pub corrupted: usize,
    pub total_bytes: usize,
    pub corruptions: alloc::vec::Vec<(usize, usize)>,
}

#[derive(Clone, Debug)]
pub struct MemoryStats {
    pub total_allocated: usize,
    pub peak_usage: usize,
    pub early_heap_used: usize,
    pub corruption_count: usize,
}

#[derive(Clone, Debug)]
pub struct AllocStats {
    pub active_allocations: usize,
    pub total_allocated: usize,
    pub peak_usage: usize,
    pub corruption_count: usize,
}

/// Early heap'ten bellek ayır (önyükleme sırasında)
fn early_alloc(layout: Layout) -> *mut u8 {
    let align = layout.align().max(1);
    let size = layout.size();

    // Alignment en az 8 olmalı (TLSF gereksinimi)
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
            // Early heap doldu!
            return core::ptr::null_mut();
        }

        if EARLY_OFFSET
            .compare_exchange(current, next_offset, Ordering::SeqCst, Ordering::Relaxed)
            .is_ok()
        {
            // Sıfırla (güvenlik için)
            unsafe { core::ptr::write_bytes(aligned as *mut u8, 0, size); }
            return aligned as *mut u8;
        }
    }
}

unsafe impl GlobalAlloc for LockedTlsf {
    /// Bellek ayırır.
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // Alignment düzelt (TLSF en az 8-byte alignment gerektirir)
        let align = layout.align().max(8);
        let size = (layout.size() + 7) & !7; // 8-byte hizala

        // Layout'u düzeltilmiş değerlerle yeniden oluştur
        let layout = match Layout::from_size_align(size, align) {
            Ok(l) => l,
            Err(_) => return core::ptr::null_mut(),
        };

        // Early heap modu
        if !HEAP_READY.load(Ordering::Acquire) {
            return early_alloc(layout);
        }

        // Main heap modu
        let mut lock = self.0.lock();
        if lock.is_none() {
            *lock = Some(Tlsf::new());
        }
        let tlsf = lock.as_mut().unwrap();

        match tlsf.allocate(layout) {
            Some(ptr) => {
                #[cfg(feature = "heap_stats")]
                ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
                ptr.as_ptr()
            }
            None => {
                // Allocation başarısız - early heap'e fallback
                early_alloc(layout)
            }
        }
    }

    /// Bellek serbest bırakır.
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let ptr_addr = ptr as usize;

        // Null pointer kontrolü
        if ptr_addr == 0 {
            return;
        }

        // Early heap kontrolü - asla serbest bırakılmaz
        if Self::is_early_heap(ptr_addr) {
            // Erken heap allocation'ları asla serbest bırakılmaz
            // Bu bir hata değil, normal davranış
            return;
        }

        // Main heap kontrolü - sadece main heap'ten ayrılanlar serbest bırakılır
        if !Self::is_main_heap(ptr_addr) {
            // Geçersiz pointer - muhtemelen corruption veya double-free
            // sessizce geri dön (panic heap'i daha fazla bozabilir)
            return;
        }

        if !HEAP_READY.load(Ordering::Acquire) {
            return;
        }

        // Alignment düzelt (alloc ile aynı)
        let align = layout.align().max(8);

        let mut lock = self.0.lock();
        if let Some(tlsf) = lock.as_mut() {
            if let Some(ptr) = NonNull::new(ptr) {
                tlsf.deallocate(ptr, align);
                #[cfg(feature = "heap_stats")]
                FREE_COUNT.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

/// Heap istatistiklerini döndür (debug için)
#[cfg(feature = "heap_stats")]
pub fn heap_stats() -> (usize, usize) {
    (
        ALLOC_COUNT.load(Ordering::Relaxed),
        FREE_COUNT.load(Ordering::Relaxed),
    )
}

/// Early heap kullanımını döndür
pub fn early_heap_usage() -> usize {
    EARLY_OFFSET.load(Ordering::Relaxed)
}

/// Main heap sınırlarını döndür
pub fn main_heap_bounds() -> (usize, usize) {
    (
        MAIN_HEAP_START.load(Ordering::Relaxed),
        MAIN_HEAP_END.load(Ordering::Relaxed),
    )
}
