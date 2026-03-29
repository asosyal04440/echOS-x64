//! # echOS Bellek Yönetimi — Ana Modül
//!
//! Fiziksel ve sanal bellek yönetiminin tüm katmanlarını barındıran ana modül.
//! UEFI/Multiboot2 bellek haritasından başlayarak kullanıcı alanı sayfa hatalarına
//! kadar tüm bellek yönetim akışını koordine eder.
//!
//! ## Modül Mimarisi
//!
//! ```
//! ┌─────────────────────────────────────────────────────────┐
//! │                    Kullanıcı Alanı                      │
//! │  mmap / munmap / mprotect / brk / madvise              │
//! └──────────────────────┬──────────────────────────────────┘
//!                        │ sistem çağrısı
//! ┌──────────────────────▼──────────────────────────────────┐
//! │              AddressSpace + VMA Yönetimi                │
//! │  Vma { start, end, flags, kind, cow, shared }           │
//! │  VmaKind::Anonymous | File | Image                      │
//! └──────────────────────┬──────────────────────────────────┘
//!                        │ sayfa hatası → handle_user_page_fault()
//! ┌──────────────────────▼──────────────────────────────────┐
//! │               Sayfa Hatası İşleyici                     │
//! │  handle_anon_lazy_fault()  → sıfır sayfa tahsis        │
//! │  handle_image_lazy_fault() → ELF segmentini yükle      │
//! │  handle_file_lazy_fault()  → dosya sayfasını yükle     │
//! │  handle_cow_fault()        → kopyala-yaz                │
//! └──────────────────────┬──────────────────────────────────┘
//!                        │
//! ┌──────────────────────▼──────────────────────────────────┐
//! │       THP (Transparent Huge Pages)                      │
//! │  try_map_thp_anon() → 512 × 4KB → 1 × 2MB              │
//! └──────────────────────┬──────────────────────────────────┘
//!                        │
//! ┌──────────────────────▼──────────────────────────────────┐
//! │         MemoryManager (FrameAllocator impl)             │
//! │  FibonacciPmm (zone: DMA / DMA32 / NORMAL)             │
//! └──────────────────────┬──────────────────────────────────┘
//!                        │ bellek baskısı
//! ┌──────────────────────▼──────────────────────────────────┐
//! │    kswapd (memory_reclaim_daemon) + LRU                 │
//! │  LruState: active/inactive listeleri                    │
//! │  reclaim_pages_scoped() → swap veya writeback           │
//! └──────────────────────┬──────────────────────────────────┘
//!                        │ son çare
//! ┌──────────────────────▼──────────────────────────────────┐
//! │              OOM Killer (oom.rs)                        │
//! │  En yüksek skorlu süreci öldür                         │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Sanal Adres Uzayı Düzeni
//!
//! ```
//! 0x0000_0000_0000_0000 ─── Kullanıcı alanı başlangıcı
//! 0x0000_1000_0000_0000 ─── USER_HEAP_BASE (yığın)
//! 0x0000_4000_0000_0000 ─── USER_MMAP_BASE (rastgele başlangıç)
//! 0x0000_7fff_ffff_f000 ─── USER_STACK_TOP (yığın üstü)
//! 0x0000_7fff_ffff_ffff ─── Kullanıcı alanı sonu
//! ─────────── Kanonik boşluk ──────────────
//! 0xFFFF_8000_0000_0000 ─── HHDM başlangıcı (fiziksel bellek doğrudan eşlem)
//! 0xFFFF_FFFF_8000_0000 ─── Çekirdek kodu/verisi (KASLR ile kaydırılabilir)
//! ```
//!
//! ## LRU Sayfa Geri Kazanımı
//!
//! `kswapd` arka plan görevi su eşiği tabanlı geri kazanım yapar:
//!
//! ```
//! Boş frame < yüksek eşik (toplam/5)?
//!   → kswapd uyanır → reclaim_pages_global(128) çalıştırır
//!
//! Boş frame < düşük eşik (toplam/10)?
//!   → allocate_frame() senkron geri kazanım yapar
//!
//! Geri kazanım kararı:
//!   Anonim sayfa → swap alanına yaz (disk/RAM)
//!   Dosya sayfası (paylaşımlı + kirli) → writeback kuyruğuna ekle
//!   Dosya sayfası (özel + kirli) → swap'a yaz
//!   Görüntü (image) sayfası → serbest bırak (yeniden yüklenebilir)
//! ```
//!
//! ## COW (Copy-on-Write) Mekanizması
//!
//! ```
//! fork() çağrıldı:
//!   1. AddressSpace klonlanır, tüm yazılabilir VMA'lar cow=true olur
//!   2. Sayfa tablosunda WRITABLE biti kaldırılır (write-protect)
//!   3. Çocuk süreç aynı fiziksel sayfalara read-only erişir
//!
//! Çocuk süreci bir sayfaya yazdı:
//!   4. Yazma hatası → handle_cow_fault(addr)
//!   5. frame_refcount > 1 → yeni fiziksel sayfa tahsis et
//!   6. Eski sayfayı kopyala → yeni çerçeveye yaz → WRITABLE ekle
//!   7. Eski sayfanın refcount'u azal → 0 ise serbest bırak
//! ```
//!
//! ## Sayfa Önbelleği (Page Cache)
//!
//! Dosya sayfaları `PageCache` yapısında önbelleğe alınır.
//! Dirty tracking ve writeback mekanizması ile senkronize edilir:
//!
//! ```
//! Dosya okundu → `read_cached_file_page()` → PageCache'e ekle
//! Dosya yazıldı → `mark_cache_dirty()` → WritebackQueue'ya koy
//! kswapd → `process_writeback_budget()` → diske geri yaz
//! ```
//!
//! ## IOMMU ve DMA Desteği
//!
//! `dma_alloc()`, `dma_share()`, `iommu_register_device()` ile DMA tamponları
//! IOMMU alanlarına kaydedilir; cihazların yalnızca kısıtlı fiziksel alanlara
//! erişmesi garanti edilir.
//!
//! ## İlgili Alt Modüller:
//! - `fibonacci_pmm.rs` — Zone tabanlı fiziksel bellek yöneticisi
//! - `paging.rs`        — Sayfa tablosu yardımcıları (HHDM, WP, PCID)
//! - `thp.rs`           — Şeffaf büyük sayfalar (2MB/1GB)
//! - `oom.rs`           — OOM Killer
//! - `zswap.rs`         — Sıkıştırılmış takas havuzu
//! - `memfd.rs`         — Anonim dosya tanımlayıcıları ve userfaultfd

use crate::drivers::ata::BLOCK_SIZE;
use crate::drivers::linux::{select_block_device, BlockDevice};
use crate::fs::{vfs_read_at, vfs_write_at};
use crate::random;
use alloc::boxed::Box;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::cmp::{max, min};
use core::ptr;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use lazy_static::lazy_static;
#[cfg(not(target_os = "uefi"))]
use multiboot2::BootInformation;
use rcore_fs::vfs::INode;
use spin::Mutex;
use uefi::table::boot::{MemoryDescriptor, MemoryMap, MemoryMapIter};
use x86_64::registers::control::{Cr0, Cr0Flags, Cr3};
use x86_64::structures::idt::PageFaultErrorCode;
use x86_64::structures::paging::mapper::{
    FlagUpdateError, MapToError, MapperAllSizes, MapperFlush, Translate, UnmapError,
};
use x86_64::structures::paging::{
    FrameAllocator, Mapper, OffsetPageTable, Page, PageSize, PageTable, PageTableFlags, PhysFrame,
    Size2MiB, Size4KiB,
};
use x86_64::{PhysAddr, VirtAddr};

/// cgroups v2 bellek denetleyicisi — limit, soft limit, swap limit
pub mod cgroup;
pub mod damon;
/// Erken önyükleme için doğrudan 2MB huge-page kurulumu (kaynak: pagging.rs)
#[path = "pagging.rs"]
pub mod early_paging;
pub mod fibonacci_buddy;
pub mod fibonacci_pmm;
pub mod frame_allocator;
/// Opsiyonel KASAN benzeri gölge bellek doğrulaması (debug)
pub mod kasan;
/// memfd_create — güvenli anonim bellek dosyaları
pub mod memfd;
/// Multi-Gen LRU (MGLRU) — sıcak/soğuk nesil tabanlı reclaim sinyali
pub mod mglru;
pub mod oom;
pub mod paging;
pub mod pmm;
/// Pressure Stall Information (PSI) — bellek baskısı telemetrisi
pub mod psi;
pub mod shared_region;
/// Şeffaf büyük sayfa (Transparent Huge Pages) — 4K→2M collapse/split
pub mod thp;
/// Bellek sıkıştırma ve swap: ZSwap/ZRam, LZ4/ZSTD
pub mod zswap;

// ============================================================================
// BELLEK İSTATİSTİKLERİ — procfs ve shell için
// ============================================================================

/// Bellek istatistik bilgisi yapısı — /proc/meminfo ve shell `info mem` komutu tarafından kullanılır
pub struct MemoryStats {
    pub total_kb: usize,
    pub free_kb: usize,
    pub available_kb: usize,
    pub buffers_kb: usize,
    pub cached_kb: usize,
    pub swap_cached_kb: usize,
    pub active_kb: usize,
    pub inactive_kb: usize,
    pub swap_total_kb: usize,
    pub swap_free_kb: usize,
    pub slab_kb: usize,
    pub page_tables_kb: usize,
}

/// Çekirdek heap boyutu (başlangıç adresi allocator'dan alınır)
pub const KERNEL_HEAP_BASE: u64 = crate::allocator::HEAP_START as u64;
/// Çekirdek heap boyutu (byte)
pub const KERNEL_HEAP_SIZE: usize = crate::allocator::HEAP_SIZE;

/// Bellek istatistikleri döndürür.
/// PMM'den gerçek fiziksel bellek istatistiklerini alır.
/// LRU, ZSwap ve heap verilerini birleştirir.
pub fn get_memory_stats() -> MemoryStats {
    let heap_kb = KERNEL_HEAP_SIZE / 1024;
    let page_size_kb = PAGE_SIZE / 1024;

    // PMM'den gerçek frame sayılarını al
    let total_frames = memory_total_frames();
    let free_frames = memory_free_frames();

    let total_kb = if total_frames > 0 {
        total_frames * page_size_kb
    } else {
        512 * 1024 // PMM henüz init olmadıysa fallback
    };
    let free_kb = free_frames * page_size_kb;

    // LRU istatistikleri
    let (active_pages, inactive_pages) = {
        let lru = LRU.lock();
        (lru.active_by_seq.len(), lru.inactive_by_seq.len())
    };

    // ZSwap istatistikleri
    let zswap_stats = zswap::ZSWAP_MANAGER.get_stats();
    let swap_cached_kb = zswap_stats.stored_pages as usize * page_size_kb;
    let zswap_pool_kb = (zswap_stats.pool_total_size as usize) / 1024;

    MemoryStats {
        total_kb,
        free_kb,
        available_kb: free_kb.saturating_add(inactive_pages * page_size_kb),
        buffers_kb: 0,
        cached_kb: (active_pages + inactive_pages) * page_size_kb,
        swap_cached_kb,
        active_kb: active_pages * page_size_kb,
        inactive_kb: inactive_pages * page_size_kb,
        swap_total_kb: zswap_pool_kb,
        swap_free_kb: zswap_pool_kb.saturating_sub(swap_cached_kb),
        slab_kb: heap_kb / 8,
        page_tables_kb: 1024,
    }
}

// ============================================================================
// MEMORY MANAGER
// ============================================================================

/// Ana bellek yöneticisi.
/// UEFI memory map ve PMM kullanır.
pub struct MemoryManager {
    /// UEFI'den alınan bellek haritası
    memory_map: MemoryMap<'static>,
    /// Fiziksel bellek yöneticisi (UEFI için Fibonacci tabanlı)
    pmm: fibonacci_pmm::FibonacciPmm,
}

impl MemoryManager {
    /// Yeni bir MemoryManager oluşturur.
    ///
    /// # Parametreler
    /// - `memory_map`: UEFI'den alınan bellek haritası
    pub fn new(memory_map: MemoryMap<'static>) -> Self {
        let mut pmm = fibonacci_pmm::FibonacciPmm::empty();
        unsafe {
            pmm.init(memory_map.entries());
        }

        MemoryManager { memory_map, pmm }
    }

    /// UEFI bellek haritası üzerinde iterator döndürür.
    #[allow(dead_code)]
    pub fn get_memory_map(&self) -> MemoryMapIter<'_> {
        self.memory_map.entries()
    }

    pub fn memory_map_mut(&mut self) -> &mut MemoryMap<'static> {
        &mut self.memory_map
    }

    pub fn allocate_contiguous_frames(&mut self, pages: usize) -> Option<PhysFrame> {
        self.pmm.allocate_contiguous(pages)
    }

    pub fn deallocate_contiguous_frames(&mut self, start: PhysFrame, pages: usize) {
        self.pmm.deallocate_contiguous(start, pages);
        // Cgroup bellek muhasebesi: serbest bırakılan frame'leri uncharge et
        let pid = crate::task::scheduler::current_task_id() as u64;
        if let Some(cg_id) = cgroup::CGROUP_MANAGER.get_cgroup_for_process(pid) {
            if let Some(cg) = cgroup::CGROUP_MANAGER.get_cgroup(cg_id) {
                cg.uncharge((pages * PAGE_SIZE) as u64);
            }
        }
    }

    pub fn total_frames(&self) -> usize {
        self.pmm.total_frames()
    }

    pub fn free_frames(&self) -> usize {
        self.pmm.free_frames()
    }
}

/// x86_64 FrameAllocator trait implementasyonu.
/// Scheduler ve paging sistemi için gerekli.
unsafe impl FrameAllocator<Size4KiB> for MemoryManager {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        // İleri düzey hook'lar (reclaim, cgroup, OOM) yalnızca alt sistemler hazır olduğunda çalışır.
        // Boot sırasında bu yollar UB (aliased &mut) ve hazır olmayan alt sistemlere erişim yapar.
        let hooks_ready = ALLOC_HOOKS_READY.load(Ordering::Relaxed);
        let stall_start = if hooks_ready {
            crate::task::scheduler::get_ticks() as u64
        } else {
            0
        };

        if hooks_ready && should_reclaim_now() {
            reclaim_pages_global(64);
            process_writeback_budget(8);
        }
        if let Some(frame) = self.pmm.allocate_frame() {
            // Cgroup bellek muhasebesi: alloc edilen frame'i mevcut task'ın cgroup'una yükle
            if hooks_ready {
                let pid = crate::task::scheduler::current_task_id() as u64;
                if let Some(cg_id) = cgroup::CGROUP_MANAGER.get_cgroup_for_process(pid) {
                    if let Some(cg) = cgroup::CGROUP_MANAGER.get_cgroup(cg_id) {
                        let _ = cg.charge(PAGE_SIZE as u64);
                    }
                }
            }
            if hooks_ready {
                let now = crate::task::scheduler::get_ticks() as u64;
                let elapsed = now.saturating_sub(stall_start);
                if elapsed > 0 {
                    psi::record_memory_stall(0, elapsed, false);
                }
            }
            return Some(frame);
        }
        // Geri kazanım denemesi
        if hooks_ready {
            psi::record_memory_stall(1, 1, false);
        }
        if hooks_ready && reclaim_pages(16) > 0 {
            if let Some(frame) = self.pmm.allocate_frame() {
                let now = crate::task::scheduler::get_ticks() as u64;
                let elapsed = now.saturating_sub(stall_start).max(1);
                psi::record_memory_stall(elapsed.min(4), elapsed, false);
                return Some(frame);
            }
        }

        // OOM Killer: Bellek hala yoksa process öldür
        if hooks_ready && oom::should_trigger_oom(self.free_frames(), self.total_frames()) {
            crate::serial_println!(
                "[MEM] OOM triggered - free: {} / total: {}",
                self.free_frames(),
                self.total_frames()
            );

            // ZSwap writeback dene — OOM öncesi son kurtarma
            let _ = zswap::ZSWAP_MANAGER.writeback_lru();
            if let Some(frame) = self.pmm.allocate_frame() {
                let now = crate::task::scheduler::get_ticks() as u64;
                let elapsed = now.saturating_sub(stall_start).max(1);
                psi::record_memory_stall(elapsed.min(8), elapsed, false);
                return Some(frame);
            }
            let now = crate::task::scheduler::get_ticks() as u64;
            let elapsed = now.saturating_sub(stall_start).max(1);
            psi::record_memory_stall(elapsed, elapsed, true);

            // Scheduler'dan gerçek task listesi al
            let tasks = crate::task::scheduler::list_tasks();
            let oom_infos: alloc::vec::Vec<oom::OomProcessInfo> = tasks
                .iter()
                .map(|t| {
                    oom::OomProcessInfo {
                        pid: t.pid,
                        name: alloc::string::String::from(t.name),
                        rss_pages: 256, // Tahmini — gerçek RSS için VMA tracking gerekir
                        swap_pages: 0,
                        oom_score_adj: 0,
                        nice: 0,
                        runtime_ticks: 0,
                        is_kernel: t.pid < 2,
                        is_root: false,
                        children: 0,
                        cpu_percent: 0,
                    }
                })
                .collect();
            oom::oom_kill(&oom_infos);
        }

        None
    }
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// Bellek yöneticisini başlatır.
pub fn init_uefi(memory_map: MemoryMap<'static>) -> MemoryManager {
    MemoryManager::new(memory_map)
}

/// Tüm bellek alt sistemlerini başlatır.
/// PMM init'ten sonra çağrılmalıdır.
/// OOM, THP, Cgroup, Memfd, ZSwap alt modüllerini devreye sokar.
pub fn init_memory_subsystems() {
    oom::init();
    psi::init(true);
    damon::init(true);
    mglru::init(true);
    #[cfg(debug_assertions)]
    kasan::init(true);
    #[cfg(not(debug_assertions))]
    kasan::init(false);
    thp::THP_MANAGER.compact_for_thp(); // THP yapısını zorla lazy_static init et
    cgroup::init();
    memfd::init();

    // ZSwap'ı toplam bellek bilgisi ile başlat
    let total_mem = memory_total_frames() as u64 * PAGE_SIZE as u64;
    zswap::ZSWAP_MANAGER.set_enabled(true);

    // Artık allocate_frame hook'ları güvenle çalışabilir
    ALLOC_HOOKS_READY.store(true, Ordering::Release);

    crate::serial_println!(
        "[MEM] Memory subsystems initialized (total: {} MB)",
        total_mem / (1024 * 1024)
    );
}

/// Global bellek yöneticisi için ham pointer.
/// Main fonksiyonu hiç dönmediği için ömür boyunca geçerli kalır.
static mut GLOBAL_MEMORY_MANAGER: *mut MemoryManager = ptr::null_mut();
#[cfg(not(target_os = "uefi"))]
static mut GLOBAL_MB2_FRAME_ALLOCATOR: *mut frame_allocator::Multiboot2FrameAllocator =
    ptr::null_mut();

/// Global bellek yöneticisi pointer'ını ayarlar.
///
/// # Güvenlik
/// Verilen pointer geçerli ve yaşam süresi tüm kernel ömrü olmalıdır.
pub unsafe fn set_global_memory_manager(manager: *mut MemoryManager) {
    GLOBAL_MEMORY_MANAGER = manager;
}

/// Global bellek yöneticisine mutable erişim sağlar.
///
/// # Güvenlik
/// Bu fonksiyon global pointer üzerinde ham erişim yapar.
pub unsafe fn global_memory_manager_mut() -> Option<&'static mut MemoryManager> {
    if GLOBAL_MEMORY_MANAGER.is_null() {
        None
    } else {
        Some(&mut *GLOBAL_MEMORY_MANAGER)
    }
}

/// Global bellek yöneticisine immutable erişim sağlar (güvenli wrapper).
pub fn global_memory_manager() -> Option<&'static MemoryManager> {
    unsafe {
        if GLOBAL_MEMORY_MANAGER.is_null() {
            None
        } else {
            Some(&*GLOBAL_MEMORY_MANAGER)
        }
    }
}

#[cfg(not(target_os = "uefi"))]
pub unsafe fn set_global_mb2_frame_allocator(
    allocator: *mut frame_allocator::Multiboot2FrameAllocator,
) {
    GLOBAL_MB2_FRAME_ALLOCATOR = allocator;
}

#[cfg(not(target_os = "uefi"))]
unsafe fn global_mb2_frame_allocator_mut(
) -> Option<&'static mut frame_allocator::Multiboot2FrameAllocator> {
    if GLOBAL_MB2_FRAME_ALLOCATOR.is_null() {
        None
    } else {
        Some(&mut *GLOBAL_MB2_FRAME_ALLOCATOR)
    }
}

pub const PAGE_SIZE: usize = 4096;
pub const PHYSICAL_MEMORY_OFFSET: u64 = 0xFFFF_8000_0000_0000;
pub const KERNEL_SPACE_START: u64 = 0xFFFF_FFFF_8000_0000;
pub const USER_SPACE_START: u64 = 0x0000_0000_0000_0000;
pub const USER_SPACE_END: u64 = 0x0000_7fff_ffff_ffff;
pub const USER_STACK_TOP: u64 = 0x0000_7fff_ffff_f000;
pub const USER_STACK_PAGES: usize = 16;
pub const USER_HEAP_BASE: u64 = 0x0000_1000_0000;
pub const USER_MMAP_BASE: u64 = 0x0000_4000_0000;
pub const USER_MMAP_RANDOM_RANGE: u64 = 256 * 1024 * 1024;
pub const USER_STACK_RANDOM_RANGE: u64 = 128 * 1024 * 1024;
static mut ACTIVE_PHYSICAL_MEMORY_OFFSET: u64 = PHYSICAL_MEMORY_OFFSET;
static KASLR_OFFSET: AtomicUsize = AtomicUsize::new(0);

/// Bellek alt sistemleri (reclaim, cgroup, OOM) hazır olduğunda true.
/// allocate_frame içindeki ileri düzey hook'lar bu bayrak true olmadan çalışmaz.
static ALLOC_HOOKS_READY: AtomicBool = AtomicBool::new(false);

const RECLAIM_HIGH_DIV: usize = 5;
const RECLAIM_LOW_DIV: usize = 10;
const RECLAIM_MIN_HIGH: usize = 128;
const RECLAIM_MIN_LOW: usize = 64;
const KSWAPD_SLEEP_TICKS: usize = 50;
const KSWAPD_RECLAIM_BATCH: usize = 128;
const WRITEBACK_BUDGET_FAST: usize = 16;
const WRITEBACK_BUDGET_IDLE: usize = 4;
const DIRTY_BG_DIV: usize = 20;
const DIRTY_LIMIT_DIV: usize = 10;
const DIRTY_INODE_LIMIT: usize = 128;
const WRITEBACK_TOKENS_PER_TICK: usize = 16;
const WRITEBACK_INODE_TOKENS_PER_TICK: usize = 4;
const WRITEBACK_TOKEN_CAP: usize = 256;
const WRITEBACK_INODE_TOKEN_CAP: usize = 64;
const THP_PAGES: usize = 512;
static KSWAPD_RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Clone)]
enum VmaKind {
    Anonymous {
        id: u64,
    },
    Image {
        seg_start: u64,
        file_offset: u64,
        file_size: u64,
    },
    File {
        inode: Arc<dyn INode>,
        file_offset: u64,
        file_size: u64,
    },
}

#[derive(Clone)]
struct Vma {
    start: u64,
    end: u64,
    flags: PageTableFlags,
    kind: VmaKind,
    cow: bool,
    shared: bool,
}

#[derive(Clone)]
struct ImageRef {
    base: usize,
    len: usize,
    owner: Option<Arc<[u8]>>,
}

struct PageCacheEntry {
    data: Vec<u8>,
    dirty: bool,
}

struct FrameRefCounts {
    counts: BTreeMap<u64, u32>,
}

struct SharedAnonPages {
    pages: BTreeMap<(u64, u64), u64>,
}

struct SharedFilePages {
    pages: BTreeMap<(usize, u64), u64>,
}

struct PageCache {
    entries: BTreeMap<(usize, u64), PageCacheEntry>,
    max_pages: usize,
}

#[derive(Clone)]
enum PageBacking {
    Anonymous {
        shared_id: u64,
    },
    File {
        inode: Arc<dyn INode>,
        file_offset: u64,
        file_size: u64,
        region_start: u64,
        shared: bool,
    },
    Image {
        seg_start: u64,
        file_offset: u64,
        file_size: u64,
        region_start: u64,
    },
}

#[derive(Clone)]
struct LruEntry {
    space_id: u64,
    page_index: u64,
    virt: u64,
    phys: u64,
    node_id: u16,
    backing: PageBacking,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LruClass {
    Anonymous,
    File,
}

#[derive(Clone, Copy, Default)]
struct SpaceLruCounts {
    anon: usize,
    file: usize,
}

struct LruState {
    next_seq: u64,
    active_by_seq: BTreeMap<u64, LruEntry>,
    inactive_by_seq: BTreeMap<u64, LruEntry>,
    by_page: BTreeMap<(u64, u64), (bool, u64)>,
    refaults: BTreeMap<(u64, u64), u64>,
    anon_pages: usize,
    file_pages: usize,
    space_counts: BTreeMap<u64, SpaceLruCounts>,
    node_counts: BTreeMap<u16, SpaceLruCounts>,
}

impl LruState {
    fn new() -> Self {
        Self {
            next_seq: 1,
            active_by_seq: BTreeMap::new(),
            inactive_by_seq: BTreeMap::new(),
            by_page: BTreeMap::new(),
            refaults: BTreeMap::new(),
            anon_pages: 0,
            file_pages: 0,
            space_counts: BTreeMap::new(),
            node_counts: BTreeMap::new(),
        }
    }

    fn touch(&mut self, entry: LruEntry) {
        let mut active = true;
        if let Some((was_active, seq)) = self.by_page.remove(&(entry.space_id, entry.page_index)) {
            let removed = if was_active {
                self.active_by_seq.remove(&seq)
            } else {
                self.inactive_by_seq.remove(&seq)
            };
            if let Some(old_entry) = removed {
                self.adjust_counts(&old_entry, false);
            }
            active = was_active;
        }
        if let Some(evicted_seq) = self.refaults.remove(&(entry.space_id, entry.page_index)) {
            let distance = self.next_seq.saturating_sub(evicted_seq);
            if distance <= 512 {
                active = true;
            } else if !active {
                active = false;
            }
        }
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        self.adjust_counts(&entry, true);
        self.by_page
            .insert((entry.space_id, entry.page_index), (active, seq));
        if active {
            self.active_by_seq.insert(seq, entry);
        } else {
            self.inactive_by_seq.insert(seq, entry);
        }
    }

    fn pop_oldest_balanced(
        &mut self,
        class: Option<LruClass>,
        space_hint: Option<u64>,
        node_hint: Option<u16>,
    ) -> Option<LruEntry> {
        if let Some(entry) = self.pop_matching(true, class, space_hint, node_hint) {
            return Some(entry);
        }
        if let Some(entry) = self.pop_matching(false, class, space_hint, node_hint) {
            return Some(entry);
        }
        if class.is_some() {
            if let Some(entry) = self.pop_matching(true, None, space_hint, node_hint) {
                return Some(entry);
            }
            if let Some(entry) = self.pop_matching(false, None, space_hint, node_hint) {
                return Some(entry);
            }
        }
        None
    }

    fn remove_page(&mut self, space_id: u64, page_index: u64) {
        if let Some((was_active, seq)) = self.by_page.remove(&(space_id, page_index)) {
            let removed = if was_active {
                self.active_by_seq.remove(&seq)
            } else {
                self.inactive_by_seq.remove(&seq)
            };
            if let Some(entry) = removed {
                self.adjust_counts(&entry, false);
            }
        }
    }

    fn record_refault(&mut self, space_id: u64, page_index: u64) {
        let seq = self.next_seq;
        self.refaults.insert((space_id, page_index), seq);
        let now_tick = crate::task::scheduler::get_ticks() as u64;
        mglru::record_refault(space_id, page_index, now_tick);
        damon::record_refault(space_id, page_index, now_tick);
    }

    fn pop_matching(
        &mut self,
        inactive: bool,
        class: Option<LruClass>,
        space_hint: Option<u64>,
        node_hint: Option<u16>,
    ) -> Option<LruEntry> {
        let list = if inactive {
            &self.inactive_by_seq
        } else {
            &self.active_by_seq
        };
        if list.is_empty() {
            return None;
        }
        let mut selected_seq = None;
        for (seq, entry) in list.iter() {
            if let Some(space) = space_hint {
                if entry.space_id != space {
                    continue;
                }
            }
            if let Some(node) = node_hint {
                if entry.node_id != node {
                    continue;
                }
            }
            if let Some(cls) = class {
                if Self::class_of(entry) != cls {
                    continue;
                }
            }
            selected_seq = Some(*seq);
            break;
        }
        if selected_seq.is_none() && (space_hint.is_some() || node_hint.is_some()) {
            for (seq, entry) in list.iter() {
                if let Some(cls) = class {
                    if Self::class_of(entry) != cls {
                        continue;
                    }
                }
                selected_seq = Some(*seq);
                break;
            }
        }
        let seq = selected_seq?;
        let entry = if inactive {
            self.inactive_by_seq.remove(&seq)?
        } else {
            self.active_by_seq.remove(&seq)?
        };
        self.by_page.remove(&(entry.space_id, entry.page_index));
        self.adjust_counts(&entry, false);
        Some(entry)
    }

    fn class_of(entry: &LruEntry) -> LruClass {
        match entry.backing {
            PageBacking::Anonymous { .. } => LruClass::Anonymous,
            PageBacking::File { .. } | PageBacking::Image { .. } => LruClass::File,
        }
    }

    fn adjust_counts(&mut self, entry: &LruEntry, add: bool) {
        let class = Self::class_of(entry);
        match class {
            LruClass::Anonymous => {
                if add {
                    self.anon_pages = self.anon_pages.saturating_add(1);
                } else {
                    self.anon_pages = self.anon_pages.saturating_sub(1);
                }
            }
            LruClass::File => {
                if add {
                    self.file_pages = self.file_pages.saturating_add(1);
                } else {
                    self.file_pages = self.file_pages.saturating_sub(1);
                }
            }
        }
        let entry_counts = self.space_counts.entry(entry.space_id).or_default();
        match class {
            LruClass::Anonymous => {
                if add {
                    entry_counts.anon = entry_counts.anon.saturating_add(1);
                } else {
                    entry_counts.anon = entry_counts.anon.saturating_sub(1);
                }
            }
            LruClass::File => {
                if add {
                    entry_counts.file = entry_counts.file.saturating_add(1);
                } else {
                    entry_counts.file = entry_counts.file.saturating_sub(1);
                }
            }
        }
        let node_counts = self.node_counts.entry(entry.node_id).or_default();
        match class {
            LruClass::Anonymous => {
                if add {
                    node_counts.anon = node_counts.anon.saturating_add(1);
                } else {
                    node_counts.anon = node_counts.anon.saturating_sub(1);
                }
            }
            LruClass::File => {
                if add {
                    node_counts.file = node_counts.file.saturating_add(1);
                } else {
                    node_counts.file = node_counts.file.saturating_sub(1);
                }
            }
        }
    }
}

struct SwapState {
    slots: BTreeMap<(u64, u64), Vec<u8>>,
}

struct SwapDeviceState {
    device: Box<dyn BlockDevice>,
    base_lba: u32,
    next_slot: u32,
    max_slots: u32,
    slots: BTreeMap<(u64, u64), u32>,
}

struct WritebackEntry {
    inode: Arc<dyn INode>,
    file_offset: u64,
    file_end: u64,
    phys: u64,
    urgent: bool,
}

struct WritebackQueue {
    urgent: VecDeque<WritebackEntry>,
    background: VecDeque<WritebackEntry>,
    urgent_quota: usize,
}

struct DirtyThrottleState {
    global_dirty: usize,
    per_inode_dirty: BTreeMap<usize, usize>,
    last_tick: usize,
    global_tokens: usize,
    per_inode_tokens: BTreeMap<usize, usize>,
}

impl SwapState {
    fn new() -> Self {
        Self {
            slots: BTreeMap::new(),
        }
    }

    fn insert(&mut self, space_id: u64, page_index: u64, data: Vec<u8>) {
        self.slots.insert((space_id, page_index), data);
    }

    fn take(&mut self, space_id: u64, page_index: u64) -> Option<Vec<u8>> {
        self.slots.remove(&(space_id, page_index))
    }

    fn remove(&mut self, space_id: u64, page_index: u64) {
        self.slots.remove(&(space_id, page_index));
    }
}

impl SwapDeviceState {
    fn new(device: Box<dyn BlockDevice>, base_lba: u32, max_slots: u32) -> Self {
        Self {
            device,
            base_lba,
            next_slot: 0,
            max_slots,
            slots: BTreeMap::new(),
        }
    }

    fn sector_per_page() -> u32 {
        (PAGE_SIZE / BLOCK_SIZE) as u32
    }

    fn store(&mut self, space_id: u64, page_index: u64, data: &[u8]) -> bool {
        if data.len() != PAGE_SIZE {
            return false;
        }
        let slot = if let Some(slot) = self.slots.get(&(space_id, page_index)).copied() {
            slot
        } else {
            if self.next_slot >= self.max_slots {
                return false;
            }
            let slot = self.next_slot;
            self.next_slot = self.next_slot.saturating_add(1);
            self.slots.insert((space_id, page_index), slot);
            slot
        };
        let sectors = Self::sector_per_page();
        let start_lba = self.base_lba.saturating_add(slot.saturating_mul(sectors));
        let mut offset = 0usize;
        for i in 0..sectors {
            let end = offset.saturating_add(BLOCK_SIZE);
            if end > data.len() {
                return false;
            }
            if self
                .device
                .write_sectors(start_lba.saturating_add(i), &data[offset..end])
                .is_err()
            {
                return false;
            }
            offset = end;
        }
        true
    }

    fn take(&mut self, space_id: u64, page_index: u64) -> Option<Vec<u8>> {
        let slot = self.slots.remove(&(space_id, page_index))?;
        let sectors = Self::sector_per_page();
        let start_lba = self.base_lba.saturating_add(slot.saturating_mul(sectors));
        let mut data = Vec::with_capacity(PAGE_SIZE);
        for i in 0..sectors {
            let sector = self.device.read_sectors(start_lba.saturating_add(i), 1);
            if sector.len() != BLOCK_SIZE {
                return None;
            }
            data.extend_from_slice(&sector);
        }
        if data.len() != PAGE_SIZE {
            None
        } else {
            Some(data)
        }
    }

    fn remove(&mut self, space_id: u64, page_index: u64) {
        self.slots.remove(&(space_id, page_index));
    }
}

impl WritebackQueue {
    fn new() -> Self {
        Self {
            urgent: VecDeque::new(),
            background: VecDeque::new(),
            urgent_quota: 4,
        }
    }

    fn push(&mut self, entry: WritebackEntry, urgent: bool) {
        if urgent {
            self.urgent.push_back(entry);
        } else {
            self.background.push_back(entry);
        }
    }

    fn pop(&mut self) -> Option<WritebackEntry> {
        if self.urgent_quota > 0 {
            if let Some(entry) = self.urgent.pop_front() {
                self.urgent_quota = self.urgent_quota.saturating_sub(1);
                return Some(entry);
            }
        }
        if let Some(entry) = self.background.pop_front() {
            self.urgent_quota = 4;
            return Some(entry);
        }
        if let Some(entry) = self.urgent.pop_front() {
            self.urgent_quota = self.urgent_quota.saturating_sub(1);
            return Some(entry);
        }
        None
    }
}

impl DirtyThrottleState {
    fn new() -> Self {
        Self {
            global_dirty: 0,
            per_inode_dirty: BTreeMap::new(),
            last_tick: 0,
            global_tokens: 0,
            per_inode_tokens: BTreeMap::new(),
        }
    }

    fn update_tokens(&mut self, now: usize) {
        if now <= self.last_tick {
            return;
        }
        let delta = now.saturating_sub(self.last_tick);
        self.last_tick = now;
        let add = delta.saturating_mul(WRITEBACK_TOKENS_PER_TICK);
        self.global_tokens = self
            .global_tokens
            .saturating_add(add)
            .min(WRITEBACK_TOKEN_CAP);
        let inode_add = delta.saturating_mul(WRITEBACK_INODE_TOKENS_PER_TICK);
        for value in self.per_inode_tokens.values_mut() {
            *value = value
                .saturating_add(inode_add)
                .min(WRITEBACK_INODE_TOKEN_CAP);
        }
    }

    fn consume_token(&mut self, inode_key: usize, now: usize) -> bool {
        self.update_tokens(now);
        if self.global_tokens == 0 {
            return false;
        }
        let entry = self
            .per_inode_tokens
            .entry(inode_key)
            .or_insert(WRITEBACK_INODE_TOKEN_CAP);
        if *entry == 0 {
            return false;
        }
        self.global_tokens = self.global_tokens.saturating_sub(1);
        *entry = entry.saturating_sub(1);
        true
    }

    fn mark_dirty(&mut self, inode_key: usize) {
        self.global_dirty = self.global_dirty.saturating_add(1);
        let entry = self.per_inode_dirty.entry(inode_key).or_insert(0);
        *entry = entry.saturating_add(1);
        self.per_inode_tokens
            .entry(inode_key)
            .or_insert(WRITEBACK_INODE_TOKEN_CAP);
    }

    fn mark_clean(&mut self, inode_key: usize) {
        self.global_dirty = self.global_dirty.saturating_sub(1);
        if let Some(entry) = self.per_inode_dirty.get_mut(&inode_key) {
            *entry = entry.saturating_sub(1);
            if *entry == 0 {
                self.per_inode_dirty.remove(&inode_key);
            }
        }
    }

    fn inode_dirty(&self, inode_key: usize) -> usize {
        self.per_inode_dirty.get(&inode_key).copied().unwrap_or(0)
    }
}

impl PageCache {
    fn new(max_pages: usize) -> Self {
        Self {
            entries: BTreeMap::new(),
            max_pages,
        }
    }

    fn insert(&mut self, key: (usize, u64), entry: PageCacheEntry) {
        if self.entries.len() >= self.max_pages {
            if let Some(first_key) = self.entries.keys().next().copied() {
                if let Some(removed) = self.entries.remove(&first_key) {
                    if removed.dirty {
                        DIRTY_STATE.lock().mark_clean(first_key.0);
                    }
                }
            }
        }
        if let Some(removed) = self.entries.remove(&key) {
            if removed.dirty {
                DIRTY_STATE.lock().mark_clean(key.0);
            }
        }
        self.entries.insert(key, entry);
    }
}

lazy_static! {
    static ref PAGE_CACHE: Mutex<PageCache> = Mutex::new(PageCache::new(4096));
    static ref LRU: Mutex<LruState> = Mutex::new(LruState::new());
    static ref SWAP: Mutex<SwapState> = Mutex::new(SwapState::new());
    static ref SWAP_DEVICE: Mutex<Option<SwapDeviceState>> = Mutex::new(None);
    static ref WRITEBACK_QUEUE: Mutex<WritebackQueue> = Mutex::new(WritebackQueue::new());
    static ref DIRTY_STATE: Mutex<DirtyThrottleState> = Mutex::new(DirtyThrottleState::new());
    static ref FRAME_REFCOUNTS: Mutex<FrameRefCounts> = Mutex::new(FrameRefCounts {
        counts: BTreeMap::new(),
    });
    static ref SHARED_ANON_PAGES: Mutex<SharedAnonPages> = Mutex::new(SharedAnonPages {
        pages: BTreeMap::new(),
    });
    static ref SHARED_FILE_PAGES: Mutex<SharedFilePages> = Mutex::new(SharedFilePages {
        pages: BTreeMap::new(),
    });
}

fn frame_key(phys: u64) -> u64 {
    phys & !(PAGE_SIZE as u64 - 1)
}

fn frame_refcount(phys: u64) -> u32 {
    let key = frame_key(phys);
    FRAME_REFCOUNTS
        .lock()
        .counts
        .get(&key)
        .copied()
        .unwrap_or(0)
}

fn inc_frame_ref(phys: u64) {
    let key = frame_key(phys);
    let mut counts = FRAME_REFCOUNTS.lock();
    let entry = counts.counts.entry(key).or_insert(0);
    *entry = entry.saturating_add(1);
}

fn dec_frame_ref(phys: u64) -> u32 {
    let key = frame_key(phys);
    let mut counts = FRAME_REFCOUNTS.lock();
    match counts.counts.get_mut(&key) {
        Some(count) => {
            if *count > 1 {
                *count -= 1;
                *count
            } else {
                counts.counts.remove(&key);
                0
            }
        }
        None => 0,
    }
}

fn free_frame_if_unused(phys: u64) {
    if dec_frame_ref(phys) == 0 {
        let frame = PhysFrame::containing_address(PhysAddr::new(phys));
        deallocate_contiguous_frames(frame, 1);
    }
}

fn current_space_id() -> u64 {
    with_address_space_ref(|space| space.id)
}

fn register_lru_mapping(addr: u64, phys: u64, region: &Vma) {
    let page_mask = !(PAGE_SIZE as u64 - 1);
    let page_start = addr & page_mask;
    let page_index = page_start / PAGE_SIZE as u64;
    let backing = match &region.kind {
        VmaKind::Anonymous { id } => PageBacking::Anonymous { shared_id: *id },
        VmaKind::File {
            inode,
            file_offset,
            file_size,
        } => PageBacking::File {
            inode: inode.clone(),
            file_offset: *file_offset,
            file_size: *file_size,
            region_start: region.start,
            shared: region.shared,
        },
        VmaKind::Image {
            seg_start,
            file_offset,
            file_size,
        } => PageBacking::Image {
            seg_start: *seg_start,
            file_offset: *file_offset,
            file_size: *file_size,
            region_start: region.start,
        },
    };
    let entry = LruEntry {
        space_id: current_space_id(),
        page_index,
        virt: page_start,
        phys,
        node_id: node_id_for_phys(phys),
        backing,
    };
    let accessed = page_table_flags(page_start)
        .map(|flags| flags.contains(PageTableFlags::ACCESSED))
        .unwrap_or(false);
    mglru::record_page_access(
        entry.space_id,
        entry.page_index,
        entry.node_id,
        accessed,
        crate::task::scheduler::get_ticks() as u64,
    );
    damon::record_page_access(
        entry.space_id,
        entry.page_index,
        entry.node_id,
        accessed,
        crate::task::scheduler::get_ticks() as u64,
    );
    LRU.lock().touch(entry);
}

fn remove_lru_mapping(space_id: u64, page_index: u64) {
    mglru::remove_page(space_id, page_index);
    damon::remove_page(space_id, page_index);
    LRU.lock().remove_page(space_id, page_index);
}

fn swap_take_page(space_id: u64, page_index: u64) -> Option<Vec<u8>> {
    if let Some(device) = SWAP_DEVICE.lock().as_mut() {
        if let Some(data) = device.take(space_id, page_index) {
            return Some(data);
        }
    }
    SWAP.lock().take(space_id, page_index)
}

fn swap_remove_page(space_id: u64, page_index: u64) {
    if let Some(device) = SWAP_DEVICE.lock().as_mut() {
        device.remove(space_id, page_index);
    }
    SWAP.lock().remove(space_id, page_index);
}

fn swap_store_page(space_id: u64, page_index: u64, data: Vec<u8>) -> bool {
    // Önce ZSwap'a yaz (sıkıştırılmış RAM önbelleği)
    let offset = (space_id << 32) | page_index;
    if zswap::ZSWAP_MANAGER.store(offset, &data).is_ok() {
        return true;
    }
    // ZSwap başarısız → disk swap dene
    if let Some(device) = SWAP_DEVICE.lock().as_mut() {
        if device.store(space_id, page_index, &data) {
            return true;
        }
    }
    // Son çare: bellek içi swap hashmap
    SWAP.lock().insert(space_id, page_index, data);
    true
}

fn memory_total_frames() -> usize {
    global_memory_manager()
        .map(|manager| manager.total_frames())
        .unwrap_or(0)
}

fn memory_free_frames() -> usize {
    global_memory_manager()
        .map(|manager| manager.free_frames())
        .unwrap_or(0)
}

fn memory_watermarks() -> (usize, usize) {
    let total = memory_total_frames();
    if total == 0 {
        return (0, 0);
    }
    let mut high = max(RECLAIM_MIN_HIGH, total / RECLAIM_HIGH_DIV);
    let mut low = max(RECLAIM_MIN_LOW, total / RECLAIM_LOW_DIV);
    if low >= high {
        high = high.saturating_add(1);
        low = high.saturating_sub(1);
    }
    (low, high)
}

fn dirty_limits() -> (usize, usize) {
    let total = memory_total_frames();
    if total == 0 {
        return (0, 0);
    }
    let mut background = max(1, total / DIRTY_BG_DIV);
    let mut limit = max(1, total / DIRTY_LIMIT_DIV);
    if background >= limit {
        limit = background.saturating_add(1);
    }
    if background == 0 {
        background = 1;
    }
    (background, limit)
}

fn mark_cache_dirty(inode_key: usize, key: (usize, u64)) {
    let mut cache = PAGE_CACHE.lock();
    if let Some(entry) = cache.entries.get_mut(&key) {
        if entry.dirty {
            return;
        }
        entry.dirty = true;
    } else {
        return;
    }
    DIRTY_STATE.lock().mark_dirty(inode_key);
    maybe_throttle_dirty(inode_key);
}

fn mark_cache_clean(inode_key: usize, key: (usize, u64)) {
    let mut cache = PAGE_CACHE.lock();
    if let Some(entry) = cache.entries.get_mut(&key) {
        if !entry.dirty {
            return;
        }
        entry.dirty = false;
    } else {
        return;
    }
    DIRTY_STATE.lock().mark_clean(inode_key);
}

fn maybe_throttle_dirty(inode_key: usize) {
    let now = crate::task::get_ticks();
    let (background, limit) = dirty_limits();
    if background == 0 || limit == 0 {
        return;
    }
    let (global_dirty, inode_dirty) = {
        let mut state = DIRTY_STATE.lock();
        state.update_tokens(now);
        (state.global_dirty, state.inode_dirty(inode_key))
    };
    if global_dirty >= background {
        process_writeback_budget(WRITEBACK_BUDGET_FAST);
    }
    if global_dirty >= limit || inode_dirty >= DIRTY_INODE_LIMIT {
        process_writeback_budget(WRITEBACK_BUDGET_FAST);
        crate::task::sleep(1);
    }
}

fn node_id_for_phys(_phys: u64) -> u16 {
    0
}

fn current_numa_node() -> u16 {
    0
}

fn compact_memory_for_thp() {
    reclaim_pages_global(THP_PAGES.saturating_mul(2));
    process_writeback_budget(WRITEBACK_BUDGET_FAST);
}

fn allocate_contiguous_huge_frame(
    frame_allocator: &mut MemoryManager,
) -> Option<PhysFrame<Size2MiB>> {
    for _ in 0..3 {
        if let Some(frame) = frame_allocator.allocate_contiguous_frames(THP_PAGES) {
            let phys = frame.start_address().as_u64();
            if phys % Size2MiB::SIZE == 0 {
                return Some(PhysFrame::<Size2MiB>::containing_address(
                    frame.start_address(),
                ));
            }
            deallocate_contiguous_frames(frame, THP_PAGES);
        }
        compact_memory_for_thp();
    }
    None
}

fn try_map_thp_anon(
    mapper: &mut (impl MapperAllSizes + Translate),
    frame_allocator: &mut MemoryManager,
    addr: u64,
    region: &Vma,
) -> bool {
    if region.shared || region.cow {
        return false;
    }
    let VmaKind::Anonymous { id } = &region.kind else {
        return false;
    };
    if *id != 0 {
        return false;
    }
    let huge_size = Size2MiB::SIZE;
    let huge_start = addr & !(huge_size - 1);
    if huge_start < region.start {
        return false;
    }
    let huge_end = huge_start.saturating_add(huge_size);
    if huge_end > region.end {
        return false;
    }
    let frame = match allocate_contiguous_huge_frame(frame_allocator) {
        Some(frame) => frame,
        None => return false,
    };
    let phys = frame.start_address().as_u64();
    let virt = active_physical_offset().saturating_add(phys);
    unsafe {
        core::ptr::write_bytes(virt as *mut u8, 0, huge_size as usize);
    }
    let page = Page::<Size2MiB>::containing_address(VirtAddr::new(huge_start));
    let map_flags = vma_map_flags(region) | PageTableFlags::HUGE_PAGE;
    let map_flags = match sanitize_user_map_flags(huge_start, huge_size, map_flags) {
        Some(value) => value,
        None => {
            let frame_4k = PhysFrame::<Size4KiB>::containing_address(frame.start_address());
            deallocate_contiguous_frames(frame_4k, THP_PAGES);
            return false;
        }
    };
    let map_result = paging::with_wp_disabled(|| unsafe {
        mapper.map_to(page, frame, map_flags, frame_allocator)
    });
    match map_result {
        Ok(flush) => {
            flush.flush();
            for i in 0..THP_PAGES {
                let offset = (i as u64).saturating_mul(PAGE_SIZE as u64);
                let page_addr = huge_start.saturating_add(offset);
                let phys_addr = phys.saturating_add(offset);
                inc_frame_ref(phys_addr);
                register_lru_mapping(page_addr, phys_addr, region);
            }
            true
        }
        Err(_) => {
            let frame_4k = PhysFrame::<Size4KiB>::containing_address(frame.start_address());
            deallocate_contiguous_frames(frame_4k, THP_PAGES);
            false
        }
    }
}

fn select_reclaim_class(anon: usize, file: usize) -> Option<LruClass> {
    let total = anon.saturating_add(file);
    if total == 0 {
        return None;
    }
    let anon_ratio = anon.saturating_mul(100) / total;
    if anon_ratio > 60 {
        Some(LruClass::Anonymous)
    } else if anon_ratio < 40 {
        Some(LruClass::File)
    } else {
        None
    }
}

fn reclaim_class_for_space(space_id: u64) -> Option<LruClass> {
    let lru = LRU.lock();
    if let Some(counts) = lru.space_counts.get(&space_id) {
        select_reclaim_class(counts.anon, counts.file)
    } else {
        None
    }
}

fn reclaim_class_global() -> Option<LruClass> {
    let lru = LRU.lock();
    select_reclaim_class(lru.anon_pages, lru.file_pages)
}

fn should_reclaim_now() -> bool {
    let (low, _) = memory_watermarks();
    low > 0 && memory_free_frames() < low
}

fn should_reclaim_background() -> bool {
    let (_, high) = memory_watermarks();
    high > 0 && memory_free_frames() < high
}

fn page_table_flags(virt: u64) -> Option<PageTableFlags> {
    let virt_addr = VirtAddr::new(virt);
    let (pml4_frame, _) = Cr3::read();
    let mut frame = pml4_frame;

    let p4_index = virt_addr.p4_index();
    let p3_index = virt_addr.p3_index();
    let p2_index = virt_addr.p2_index();
    let p1_index = virt_addr.p1_index();

    let p4_table = unsafe { phys_table(frame) };
    let p4_entry = &p4_table[p4_index];
    if !p4_entry.flags().contains(PageTableFlags::PRESENT) {
        return None;
    }
    frame = PhysFrame::containing_address(p4_entry.addr());

    let p3_table = unsafe { phys_table(frame) };
    let p3_entry = &p3_table[p3_index];
    if !p3_entry.flags().contains(PageTableFlags::PRESENT) {
        return None;
    }
    if p3_entry.flags().contains(PageTableFlags::HUGE_PAGE) {
        return Some(p3_entry.flags());
    }
    frame = PhysFrame::containing_address(p3_entry.addr());

    let p2_table = unsafe { phys_table(frame) };
    let p2_entry = &p2_table[p2_index];
    if !p2_entry.flags().contains(PageTableFlags::PRESENT) {
        return None;
    }
    if p2_entry.flags().contains(PageTableFlags::HUGE_PAGE) {
        return Some(p2_entry.flags());
    }
    frame = PhysFrame::containing_address(p2_entry.addr());

    let p1_table = unsafe { phys_table(frame) };
    let p1_entry = &p1_table[p1_index];
    if !p1_entry.flags().contains(PageTableFlags::PRESENT) {
        return None;
    }
    Some(p1_entry.flags())
}

fn page_is_dirty(virt: u64) -> bool {
    page_table_flags(virt)
        .map(|flags| flags.contains(PageTableFlags::DIRTY))
        .unwrap_or(false)
}

unsafe fn phys_table(frame: PhysFrame) -> &'static PageTable {
    let phys = frame.start_address().as_u64();
    let virt = VirtAddr::new(active_physical_offset() + phys);
    &*virt.as_ptr()
}

fn next_shared_anon_id() -> u64 {
    use core::sync::atomic::{AtomicU64, Ordering};
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

fn next_address_space_id() -> u64 {
    use core::sync::atomic::{AtomicU64, Ordering};
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone)]
pub struct AddressSpace {
    id: u64,
    vmas: Vec<Vma>,
    image: Option<ImageRef>,
    heap_base: u64,
    heap_break: u64,
    mmap_base: u64,
    mmap_next: u64,
    stack_base: u64,
    stack_top: u64,
}

fn image_ref_from_slice(image: &[u8]) -> ImageRef {
    ImageRef {
        base: image.as_ptr() as usize,
        len: image.len(),
        owner: None,
    }
}

fn image_ref_from_owned(image: Arc<[u8]>) -> ImageRef {
    let slice = image.as_ref();
    ImageRef {
        base: slice.as_ptr() as usize,
        len: slice.len(),
        owner: Some(image),
    }
}

static DEFAULT_ADDRESS_SPACE: Mutex<AddressSpace> = Mutex::new(AddressSpace {
    id: 0,
    vmas: Vec::new(),
    image: None,
    heap_base: 0,
    heap_break: 0,
    mmap_base: 0,
    mmap_next: 0,
    stack_base: 0,
    stack_top: 0,
});
static ACTIVE_ADDRESS_SPACE: Mutex<Option<Arc<Mutex<AddressSpace>>>> = Mutex::new(None);

fn try_merge_vma(left: &mut Vma, right: &Vma) -> bool {
    if left.end != right.start
        || left.flags != right.flags
        || left.cow != right.cow
        || left.shared != right.shared
    {
        return false;
    }
    match (&mut left.kind, &right.kind) {
        (VmaKind::Anonymous { id: left_id }, VmaKind::Anonymous { id: right_id }) => {
            if left_id == right_id {
                left.end = right.end;
                true
            } else {
                false
            }
        }
        (
            VmaKind::Image {
                seg_start: left_seg,
                file_offset: left_off,
                file_size: left_size,
            },
            VmaKind::Image {
                seg_start: right_seg,
                file_offset: right_off,
                file_size: right_size,
            },
        ) => {
            let expected_off = left_off.saturating_add(left.end.saturating_sub(left.start));
            if left_seg == right_seg && *right_off == expected_off {
                *left_size = left_size.saturating_add(*right_size);
                left.end = right.end;
                true
            } else {
                false
            }
        }
        (
            VmaKind::File {
                inode: left_inode,
                file_offset: left_off,
                file_size: left_size,
            },
            VmaKind::File {
                inode: right_inode,
                file_offset: right_off,
                file_size: right_size,
            },
        ) => {
            let expected_off = left_off.saturating_add(left.end.saturating_sub(left.start));
            if Arc::ptr_eq(left_inode, right_inode) && *right_off == expected_off {
                *left_size = left_size.saturating_add(*right_size);
                left.end = right.end;
                true
            } else {
                false
            }
        }
        _ => false,
    }
}

fn merge_adjacent(vmas: &mut Vec<Vma>) {
    if vmas.len() <= 1 {
        return;
    }
    vmas.sort_by_key(|v| v.start);
    let mut merged = Vec::with_capacity(vmas.len());
    for vma in vmas.iter().cloned() {
        if let Some(last) = merged.last_mut() {
            if try_merge_vma(last, &vma) {
                continue;
            }
        }
        merged.push(vma);
    }
    *vmas = merged;
}

fn insert_vma(space: &mut AddressSpace, vma: Vma) -> bool {
    if vma.end <= vma.start {
        return false;
    }
    let idx = space
        .vmas
        .iter()
        .position(|item| item.start > vma.start)
        .unwrap_or(space.vmas.len());
    if idx > 0 {
        let prev = &space.vmas[idx - 1];
        if vma.start < prev.end {
            return false;
        }
    }
    if idx < space.vmas.len() {
        let next = &space.vmas[idx];
        if vma.end > next.start {
            return false;
        }
    }
    space.vmas.insert(idx, vma);
    merge_adjacent(&mut space.vmas);
    true
}

fn with_address_space_ref<F, R>(f: F) -> R
where
    F: FnOnce(&AddressSpace) -> R,
{
    let active = { ACTIVE_ADDRESS_SPACE.lock().clone() };
    if let Some(space) = active {
        let guard = space.lock();
        f(&*guard)
    } else {
        let guard = DEFAULT_ADDRESS_SPACE.lock();
        f(&*guard)
    }
}

fn with_address_space_mut<F, R>(f: F) -> R
where
    F: FnOnce(&mut AddressSpace) -> R,
{
    let active = { ACTIVE_ADDRESS_SPACE.lock().clone() };
    if let Some(space) = active {
        let mut guard = space.lock();
        f(&mut *guard)
    } else {
        let mut guard = DEFAULT_ADDRESS_SPACE.lock();
        f(&mut *guard)
    }
}

pub fn active_physical_offset() -> u64 {
    unsafe { ACTIVE_PHYSICAL_MEMORY_OFFSET }
}

pub fn set_active_physical_offset(offset: u64) {
    unsafe {
        ACTIVE_PHYSICAL_MEMORY_OFFSET = offset;
    }
}

pub fn set_kaslr_offset(offset: u64) {
    KASLR_OFFSET.store(offset as usize, Ordering::SeqCst);
}

pub fn kaslr_offset() -> u64 {
    KASLR_OFFSET.load(Ordering::SeqCst) as u64
}

pub fn kernel_virtual_base() -> u64 {
    KERNEL_SPACE_START.saturating_add(kaslr_offset())
}

pub fn is_user_address(addr: u64) -> bool {
    addr <= USER_SPACE_END
}

pub fn is_user_range(start: u64, size: u64) -> bool {
    if size == 0 {
        return false;
    }
    let end = start.saturating_add(size.saturating_sub(1));
    is_user_address(start) && is_user_address(end)
}

pub fn is_kernel_address(addr: u64) -> bool {
    addr >= kernel_virtual_base()
}

pub fn create_address_space(image: &[u8]) -> Arc<Mutex<AddressSpace>> {
    Arc::new(Mutex::new(AddressSpace {
        id: next_address_space_id(),
        vmas: Vec::new(),
        image: Some(image_ref_from_slice(image)),
        heap_base: 0,
        heap_break: 0,
        mmap_base: 0,
        mmap_next: 0,
        stack_base: 0,
        stack_top: 0,
    }))
}

pub fn create_address_space_owned(image: Arc<[u8]>) -> Arc<Mutex<AddressSpace>> {
    Arc::new(Mutex::new(AddressSpace {
        id: next_address_space_id(),
        vmas: Vec::new(),
        image: Some(image_ref_from_owned(image)),
        heap_base: 0,
        heap_break: 0,
        mmap_base: 0,
        mmap_next: 0,
        stack_base: 0,
        stack_top: 0,
    }))
}

pub fn create_empty_address_space() -> Arc<Mutex<AddressSpace>> {
    create_address_space(&[])
}

pub fn address_space_id(space: &Arc<Mutex<AddressSpace>>) -> u64 {
    space.lock().id
}

pub fn allocate_user_mmap_in(space: &Arc<Mutex<AddressSpace>>, size: u64) -> Option<u64> {
    if size == 0 {
        return None;
    }
    let page_mask = !(PAGE_SIZE as u64 - 1);
    let aligned = size.saturating_add(PAGE_SIZE as u64 - 1) & page_mask;
    let mut guard = space.lock();
    let base = ensure_mmap_base(&mut guard);
    let (stack_base, _) = ensure_stack_bounds(&mut guard, USER_STACK_PAGES);
    if guard.mmap_next == 0 {
        guard.mmap_next = stack_base;
    }
    let next = guard.mmap_next.min(stack_base);
    let new_next = next.saturating_sub(aligned);
    let end = new_next.saturating_add(aligned);
    if new_next < base || end > stack_base || !is_user_range(new_next, aligned) {
        return None;
    }
    guard.mmap_next = new_next;
    Some(new_next)
}

pub fn register_shared_anon_region_in(
    space: &Arc<Mutex<AddressSpace>>,
    start: u64,
    size: u64,
    flags: PageTableFlags,
    shared_id: Option<u64>,
) -> Option<u64> {
    if size == 0 {
        return None;
    }
    let page_mask = !(PAGE_SIZE as u64 - 1);
    let start = start & page_mask;
    let end = start
        .saturating_add(size)
        .saturating_add(PAGE_SIZE as u64 - 1)
        & page_mask;
    if end <= start || !is_user_range(start, end.saturating_sub(start)) {
        return None;
    }
    let flags = enforce_wx(flags | PageTableFlags::USER_ACCESSIBLE);
    let shared_id = shared_id.unwrap_or_else(next_shared_anon_id);
    let inserted = {
        let mut guard = space.lock();
        insert_vma(
            &mut guard,
            Vma {
                start,
                end,
                flags,
                kind: VmaKind::Anonymous { id: shared_id },
                cow: false,
                shared: true,
            },
        )
    };
    inserted.then_some(shared_id)
}

pub fn clone_address_space_for_cow(
    space: &Arc<Mutex<AddressSpace>>,
) -> Option<Arc<Mutex<AddressSpace>>> {
    let original = space.lock();
    let mut cloned = original.clone();
    cloned.id = next_address_space_id();
    for vma in &mut cloned.vmas {
        if !vma.shared && vma.flags.contains(PageTableFlags::WRITABLE) {
            vma.cow = true;
        }
    }
    if !apply_cow_write_protect_current() {
        return None;
    }
    Some(Arc::new(Mutex::new(cloned)))
}

pub fn clone_user_pml4_for_cow() -> Option<PhysFrame> {
    let regions = with_address_space_ref(|space| {
        space
            .vmas
            .iter()
            .filter(|region| region.cow || region.shared)
            .cloned()
            .collect::<Vec<_>>()
    });
    let frame_allocator = unsafe { global_memory_manager_mut()? };
    let new_pml4 = create_user_pml4()?;
    if regions.is_empty() {
        return Some(new_pml4);
    }
    let phys_offset = active_physical_offset();
    let pml4_phys = new_pml4.start_address().as_u64();
    let pml4_virt = VirtAddr::new(phys_offset + pml4_phys);
    let table = unsafe { &mut *(pml4_virt.as_mut_ptr()) };
    let mut mapper = unsafe { OffsetPageTable::new(table, VirtAddr::new(phys_offset)) };
    let table_flags =
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
    for region in regions {
        if region.end <= region.start {
            continue;
        }
        let start_page = Page::containing_address(VirtAddr::new(region.start));
        let end_page = Page::containing_address(VirtAddr::new(region.end.saturating_sub(1)));
        let mut flags = region.flags | PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
        if region.cow {
            flags.remove(PageTableFlags::WRITABLE);
        }
        for page in Page::range_inclusive(start_page, end_page) {
            let phys = match paging::translate_addr(page.start_address()) {
                Some(value) => value,
                None => continue,
            };
            let frame = PhysFrame::containing_address(phys);
            let map_result = paging::with_wp_disabled(|| unsafe {
                mapper.map_to_with_table_flags(page, frame, flags, table_flags, frame_allocator)
            });
            match map_result {
                Ok(flush) => {
                    flush.flush();
                    inc_frame_ref(phys.as_u64());
                }
                Err(MapToError::PageAlreadyMapped(_)) => {
                    if !paging::verify_idempotent_mapping(&mut mapper, page, frame) {
                        return None;
                    }
                }
                Err(MapToError::ParentEntryHugePage) => {
                    if !split_huge_page(&mut mapper, frame_allocator, page, flags) {
                        return None;
                    }
                }
                Err(_) => return None,
            }
        }
    }
    Some(new_pml4)
}

pub fn set_active_address_space(space: Option<Arc<Mutex<AddressSpace>>>) {
    *ACTIVE_ADDRESS_SPACE.lock() = space;
}

pub fn apply_cow_write_protect_current() -> bool {
    let regions = with_address_space_ref(|space| {
        space
            .vmas
            .iter()
            .filter(|region| region.cow)
            .cloned()
            .collect::<Vec<_>>()
    });
    if regions.is_empty() {
        return true;
    }
    let frame_allocator = unsafe { global_memory_manager_mut() };
    let Some(frame_allocator) = frame_allocator else {
        return false;
    };
    let (level_4_frame, _) = Cr3::read();
    let phys_base = level_4_frame.start_address();
    let virt_base = VirtAddr::new(active_physical_offset() + phys_base.as_u64());
    let table = unsafe { &mut *(virt_base.as_mut_ptr()) };
    let mut mapper =
        unsafe { OffsetPageTable::new(table, VirtAddr::new(active_physical_offset())) };
    for region in regions {
        if region.end <= region.start {
            continue;
        }
        let start_page = Page::containing_address(VirtAddr::new(region.start));
        let end_page = Page::containing_address(VirtAddr::new(region.end.saturating_sub(1)));
        let flags = (region.flags | PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE)
            & !PageTableFlags::WRITABLE;
        for page in Page::range_inclusive(start_page, end_page) {
            if paging::translate_addr(page.start_address()).is_none() {
                continue;
            }
            if !update_page_flags_with_split(&mut mapper, frame_allocator, page, flags) {
                return false;
            }
        }
    }
    true
}

pub fn register_lazy_region(start: u64, size: u64, flags: PageTableFlags) -> bool {
    if size == 0 {
        return false;
    }
    let page_mask = !(PAGE_SIZE as u64 - 1);
    let start = start & page_mask;
    let end = start
        .saturating_add(size)
        .saturating_add(PAGE_SIZE as u64 - 1)
        & page_mask;
    if end <= start {
        return false;
    }
    if !is_user_range(start, end.saturating_sub(start)) {
        return false;
    }
    let flags = enforce_wx(flags | PageTableFlags::USER_ACCESSIBLE);
    with_address_space_mut(|space| {
        insert_vma(
            space,
            Vma {
                start,
                end,
                flags,
                kind: VmaKind::Anonymous { id: 0 },
                cow: false,
                shared: false,
            },
        )
    })
}

pub fn register_shared_anon_region(start: u64, size: u64, flags: PageTableFlags) -> bool {
    if size == 0 {
        return false;
    }
    let page_mask = !(PAGE_SIZE as u64 - 1);
    let start = start & page_mask;
    let end = start
        .saturating_add(size)
        .saturating_add(PAGE_SIZE as u64 - 1)
        & page_mask;
    if end <= start {
        return false;
    }
    if !is_user_range(start, end.saturating_sub(start)) {
        return false;
    }
    let flags = enforce_wx(flags | PageTableFlags::USER_ACCESSIBLE);
    let id = next_shared_anon_id();
    with_address_space_mut(|space| {
        insert_vma(
            space,
            Vma {
                start,
                end,
                flags,
                kind: VmaKind::Anonymous { id },
                cow: false,
                shared: true,
            },
        )
    })
}

fn ensure_mmap_base(space: &mut AddressSpace) -> u64 {
    if space.mmap_base != 0 {
        return space.mmap_base;
    }
    let page_mask = !(PAGE_SIZE as u64 - 1);
    let max_pages = (USER_MMAP_RANDOM_RANGE / PAGE_SIZE as u64) as u32;
    let offset_pages = random::next_range(max_pages.max(1)) as u64;
    let offset = offset_pages.saturating_mul(PAGE_SIZE as u64);
    let base = (USER_MMAP_BASE.saturating_add(offset)) & page_mask;
    space.mmap_base = base;
    base
}

fn ensure_stack_bounds(space: &mut AddressSpace, pages: usize) -> (u64, u64) {
    if space.stack_top != 0 {
        return (space.stack_base, space.stack_top);
    }
    let base = ensure_mmap_base(space);
    let size = (pages as u64).saturating_mul(PAGE_SIZE as u64);
    let min_stack_top = base.saturating_add(PAGE_SIZE as u64).saturating_add(size);
    let max_offset = USER_STACK_TOP.saturating_sub(min_stack_top);
    let max_offset = max_offset.min(USER_STACK_RANDOM_RANGE);
    let max_pages = (max_offset / PAGE_SIZE as u64) as u32;
    let offset_pages = if max_pages == 0 {
        0
    } else {
        random::next_range(max_pages + 1) as u64
    };
    let offset = offset_pages.saturating_mul(PAGE_SIZE as u64);
    let page_mask = !(PAGE_SIZE as u64 - 1);
    let stack_top = USER_STACK_TOP.saturating_sub(offset) & page_mask;
    let stack_base = stack_top.saturating_sub(size);
    space.stack_top = stack_top;
    space.stack_base = stack_base;
    if space.mmap_next == 0 {
        space.mmap_next = stack_base;
    }
    (stack_base, stack_top)
}

pub fn user_stack_bounds() -> (u64, u64) {
    with_address_space_mut(|space| {
        if space.mmap_base == 0 {
            ensure_mmap_base(space);
        }
        ensure_stack_bounds(space, USER_STACK_PAGES)
    })
}

pub fn user_heap_limit() -> u64 {
    with_address_space_mut(|space| {
        let base = ensure_mmap_base(space);
        base.saturating_sub(PAGE_SIZE as u64)
    })
}

pub fn allocate_user_mmap(size: u64) -> Option<u64> {
    if size == 0 {
        return None;
    }
    let page_mask = !(PAGE_SIZE as u64 - 1);
    let aligned = size.saturating_add(PAGE_SIZE as u64 - 1) & page_mask;
    with_address_space_mut(|space| {
        let base = ensure_mmap_base(space);
        let (stack_base, _) = ensure_stack_bounds(space, USER_STACK_PAGES);
        if space.mmap_next == 0 {
            space.mmap_next = stack_base;
        }
        let next = space.mmap_next.min(stack_base);
        let new_next = next.saturating_sub(aligned);
        let end = new_next.saturating_add(aligned);
        if new_next < base || end > stack_base || !is_user_range(new_next, aligned) {
            return None;
        }
        space.mmap_next = new_next;
        Some(new_next)
    })
}

pub fn update_user_region_flags(start: u64, size: u64, flags: PageTableFlags) -> bool {
    if size == 0 {
        return false;
    }
    let page_mask = !(PAGE_SIZE as u64 - 1);
    let start = start & page_mask;
    let end = start
        .saturating_add(size)
        .saturating_add(PAGE_SIZE as u64 - 1)
        & page_mask;
    if end <= start {
        return false;
    }
    if !is_user_range(start, end.saturating_sub(start)) {
        return false;
    }
    let flags = enforce_wx(flags | PageTableFlags::USER_ACCESSIBLE);
    with_address_space_mut(|space| {
        let mut updated = false;
        let mut next = Vec::with_capacity(space.vmas.len().saturating_add(2));
        for region in &space.vmas {
            if end <= region.start || start >= region.end {
                next.push(region.clone());
                continue;
            }
            if start > region.start {
                let mut left = region.clone();
                left.end = start;
                next.push(left);
            }
            let mid_start = region.start.max(start);
            let mid_end = region.end.min(end);
            if mid_start < mid_end {
                let mut mid = region.clone();
                mid.start = mid_start;
                mid.end = mid_end;
                mid.flags = flags;
                next.push(mid);
                updated = true;
            }
            if end < region.end {
                let mut right = region.clone();
                right.start = end;
                next.push(right);
            }
        }
        merge_adjacent(&mut next);
        space.vmas = next;
        updated
    })
}

fn initial_heap_base(space: &AddressSpace) -> u64 {
    let limit = space.mmap_base.saturating_sub(PAGE_SIZE as u64);
    let mut base = USER_HEAP_BASE;
    for region in &space.vmas {
        if matches!(region.kind, VmaKind::File { .. } | VmaKind::Image { .. }) {
            base = base.max(region.end);
        }
    }
    let page_mask = !(PAGE_SIZE as u64 - 1);
    let aligned = (base.saturating_add(PAGE_SIZE as u64 - 1)) & page_mask;
    aligned.min(limit)
}

pub fn user_heap_state() -> (u64, u64) {
    with_address_space_mut(|space| {
        ensure_mmap_base(space);
        if space.heap_base == 0 {
            let base = initial_heap_base(space);
            space.heap_base = base;
            space.heap_break = base;
        }
        (space.heap_base, space.heap_break)
    })
}

pub fn set_user_heap_break(new_break: u64) {
    with_address_space_mut(|space| {
        space.heap_break = new_break;
    });
}

fn inode_key(inode: &Arc<dyn INode>) -> usize {
    Arc::as_ptr(inode) as *const () as usize
}

fn read_file_page(inode: &Arc<dyn INode>, file_offset: u64, file_end: u64) -> Option<Vec<u8>> {
    let mut data = vec![0u8; PAGE_SIZE];
    if file_offset >= file_end {
        return Some(data);
    }
    let to_read = min(PAGE_SIZE as u64, file_end.saturating_sub(file_offset)) as usize;
    if to_read == 0 {
        return Some(data);
    }
    let read = match vfs_read_at(inode, file_offset as usize, &mut data[..to_read]) {
        Ok(value) => value,
        Err(_) => return None,
    };
    if read < to_read {
        for b in data[read..to_read].iter_mut() {
            *b = 0;
        }
    }
    Some(data)
}

fn read_cached_file_page(
    inode: &Arc<dyn INode>,
    file_offset: u64,
    file_end: u64,
) -> Option<Vec<u8>> {
    let page_index = file_offset / PAGE_SIZE as u64;
    let key = (inode_key(inode), page_index);
    if let Some(entry) = PAGE_CACHE.lock().entries.get(&key) {
        return Some(entry.data.clone());
    }
    let data = read_file_page(inode, file_offset, file_end)?;
    PAGE_CACHE.lock().insert(
        key,
        PageCacheEntry {
            data: data.clone(),
            dirty: false,
        },
    );
    Some(data)
}

fn writeback_file_range(vma: &Vma, start: u64, end: u64) -> bool {
    let VmaKind::File {
        inode,
        file_offset,
        file_size,
    } = &vma.kind
    else {
        return true;
    };
    if !vma.shared || !vma.flags.contains(PageTableFlags::WRITABLE) {
        return true;
    }
    let page_mask = !(PAGE_SIZE as u64 - 1);
    let mut page_start = start & page_mask;
    let end = end.saturating_add(PAGE_SIZE as u64 - 1) & page_mask;
    let file_end = file_offset.saturating_add(*file_size);
    while page_start < end {
        let page_file_offset = file_offset.saturating_add(page_start.saturating_sub(vma.start));
        if page_file_offset >= file_end {
            break;
        }
        let page_index = page_file_offset / PAGE_SIZE as u64;
        let key = (inode_key(inode), page_index);
        let dirty = PAGE_CACHE
            .lock()
            .entries
            .get(&key)
            .map(|entry| entry.dirty)
            .unwrap_or(false);
        if dirty {
            let phys = match paging::translate_addr(VirtAddr::new(page_start)) {
                Some(addr) => addr.as_u64() & page_mask,
                None => {
                    page_start = page_start.saturating_add(PAGE_SIZE as u64);
                    continue;
                }
            };
            let virt = active_physical_offset().saturating_add(phys);
            let max_len = min(PAGE_SIZE as u64, file_end.saturating_sub(page_file_offset)) as usize;
            let mut buf = vec![0u8; max_len];
            unsafe {
                core::ptr::copy_nonoverlapping(virt as *const u8, buf.as_mut_ptr(), max_len);
            }
            if vfs_write_at(inode, page_file_offset as usize, &buf).is_err() {
                return false;
            }
            mark_cache_clean(key.0, key);
            let mut cache = PAGE_CACHE.lock();
            if let Some(entry) = cache.entries.get_mut(&key) {
                let copy_len = min(PAGE_SIZE, max_len);
                entry.data[..copy_len].copy_from_slice(&buf[..copy_len]);
            }
        }
        page_start = page_start.saturating_add(PAGE_SIZE as u64);
    }
    true
}

fn writeback_file_page(inode: &Arc<dyn INode>, file_offset: u64, file_end: u64, phys: u64) -> bool {
    if file_offset >= file_end {
        return true;
    }
    let max_len = min(PAGE_SIZE as u64, file_end.saturating_sub(file_offset)) as usize;
    if max_len == 0 {
        return true;
    }
    let virt = active_physical_offset().saturating_add(phys);
    let mut buf = vec![0u8; max_len];
    unsafe {
        core::ptr::copy_nonoverlapping(virt as *const u8, buf.as_mut_ptr(), max_len);
    }
    if vfs_write_at(inode, file_offset as usize, &buf).is_err() {
        return false;
    }
    let page_index = file_offset / PAGE_SIZE as u64;
    let key = (inode_key(inode), page_index);
    mark_cache_clean(key.0, key);
    let mut cache = PAGE_CACHE.lock();
    if let Some(entry) = cache.entries.get_mut(&key) {
        let copy_len = min(PAGE_SIZE, max_len);
        entry.data[..copy_len].copy_from_slice(&buf[..copy_len]);
    }
    true
}

fn schedule_writeback(
    inode: Arc<dyn INode>,
    file_offset: u64,
    file_end: u64,
    phys: u64,
    urgent: bool,
) {
    WRITEBACK_QUEUE.lock().push(
        WritebackEntry {
            inode,
            file_offset,
            file_end,
            phys,
            urgent,
        },
        urgent,
    );
}

fn process_writeback_budget(budget: usize) -> usize {
    let mut done: usize = 0;
    for _ in 0..budget {
        let entry = WRITEBACK_QUEUE.lock().pop();
        let Some(entry) = entry else {
            break;
        };
        let inode_key = inode_key(&entry.inode);
        let now = crate::task::get_ticks();
        if !DIRTY_STATE.lock().consume_token(inode_key, now) {
            let urgent = entry.urgent;
            WRITEBACK_QUEUE.lock().push(entry, urgent);
            break;
        }
        if !writeback_file_page(&entry.inode, entry.file_offset, entry.file_end, entry.phys) {
            let urgent = entry.urgent;
            WRITEBACK_QUEUE.lock().push(entry, urgent);
            break;
        }
        done = done.saturating_add(1);
    }
    done
}

pub fn init_swap_device(base_lba: u32, max_slots: u32) -> bool {
    if max_slots == 0 {
        return false;
    }
    let mut guard = SWAP_DEVICE.lock();
    if guard.is_some() {
        return true;
    }
    let device = match select_block_device() {
        Ok(device) => device,
        Err(_) => return false,
    };
    *guard = Some(SwapDeviceState::new(device, base_lba, max_slots));
    true
}

pub fn start_reclaim_daemon() {
    if KSWAPD_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    crate::serial_println!("[DEBUG] Spawning kswapd task...");
    crate::task::spawn_with_priority(memory_reclaim_daemon, crate::task::Priority::Low, "kswapd");
    crate::serial_println!("[DEBUG] Successfully spawned kswapd task");
}

fn memory_reclaim_daemon() -> ! {
    loop {
        let now = crate::task::scheduler::get_ticks() as u64;
        damon::age(now);
        mglru::age_generations(now);
        let _ = thp::khugepaged_scan_once(8);
        if should_reclaim_background() {
            reclaim_pages_global(KSWAPD_RECLAIM_BATCH);
            process_writeback_budget(WRITEBACK_BUDGET_FAST);
            crate::task::sleep(2);
        } else {
            process_writeback_budget(WRITEBACK_BUDGET_IDLE);
            crate::task::sleep(KSWAPD_SLEEP_TICKS);
        }
    }
}

fn reclaim_pages_scoped(target: usize, global: bool) -> usize {
    let mut freed = 0;
    let mut scan_budget = target.saturating_mul(6).max(8);
    let space_id = current_space_id();
    let node_hint = Some(current_numa_node());
    while freed < target && scan_budget > 0 {
        scan_budget = scan_budget.saturating_sub(1);
        let now_tick = crate::task::scheduler::get_ticks() as u64;
        let pressure = psi::snapshot();
        let pressure_critical = pressure.some_avg10 >= 700 || pressure.full_avg10 >= 350;
        let class_hint = if global {
            reclaim_class_global()
        } else {
            reclaim_class_for_space(space_id)
        };
        let damon_victim = damon::pick_victim(
            if global { None } else { Some(space_id) },
            node_hint,
            now_tick,
        );
        let mg_victim = mglru::pick_victim(if global { None } else { Some(space_id) }, node_hint);
        let space_hint = if global {
            damon_victim
                .map(|v| v.key.space_id)
                .or_else(|| mg_victim.map(|v| v.key.space_id))
        } else {
            Some(space_id)
        };
        let node_select = damon_victim
            .map(|v| v.node_id)
            .or_else(|| mg_victim.map(|v| v.node_id))
            .or(node_hint);
        let entry = LRU
            .lock()
            .pop_oldest_balanced(class_hint, space_hint, node_select);
        let Some(entry) = entry else {
            break;
        };
        if !global && entry.space_id != space_id {
            LRU.lock().touch(entry);
            break;
        }
        if let Some(hint) = damon::hint_for_page(entry.space_id, entry.page_index, now_tick) {
            let preserve_hot =
                matches!(hint.temperature, damon::DamonTemperature::Hot) && !pressure_critical;
            let preserve_warm = matches!(hint.temperature, damon::DamonTemperature::Warm)
                && pressure.full_avg10 < 200
                && scan_budget > 0;
            if preserve_hot || preserve_warm {
                LRU.lock().touch(entry);
                continue;
            }
        }
        let page_mask = !(PAGE_SIZE as u64 - 1);
        let virt = entry.virt & page_mask;
        let phys = match paging::translate_addr(VirtAddr::new(virt)) {
            Some(addr) => addr.as_u64() & page_mask,
            None => {
                continue;
            }
        };
        let dirty = page_is_dirty(virt);
        let mut should_swap = false;
        let mut should_writeback = None;
        match &entry.backing {
            PageBacking::Anonymous { shared_id } => {
                if *shared_id != 0 {
                    if frame_refcount(phys) <= 1 {
                        should_swap = true;
                    }
                } else {
                    should_swap = true;
                }
            }
            PageBacking::File {
                inode,
                file_offset,
                file_size,
                region_start,
                shared,
            } => {
                let page_file_offset =
                    file_offset.saturating_add(virt.saturating_sub(*region_start));
                let file_end = file_offset.saturating_add(*file_size);
                if *shared {
                    if dirty {
                        should_writeback = Some((inode.clone(), page_file_offset, file_end, phys));
                    }
                } else if dirty {
                    should_swap = true;
                }
            }
            PageBacking::Image { .. } => {}
        }
        if let Some((inode, file_offset, file_end, phys)) = should_writeback {
            schedule_writeback(inode, file_offset, file_end, phys, should_reclaim_now());
        }
        if should_swap {
            let mut data = vec![0u8; PAGE_SIZE];
            let virt_phys = active_physical_offset().saturating_add(phys);
            unsafe {
                core::ptr::copy_nonoverlapping(
                    virt_phys as *const u8,
                    data.as_mut_ptr(),
                    PAGE_SIZE,
                );
            }
            if !swap_store_page(entry.space_id, entry.page_index, data) {
                LRU.lock().touch(entry);
                break;
            }
        }
        LRU.lock().record_refault(entry.space_id, entry.page_index);
        let (level_4_frame, _) = Cr3::read();
        let phys_base = level_4_frame.start_address();
        let virt_base = VirtAddr::new(active_physical_offset() + phys_base.as_u64());
        let table = unsafe { &mut *(virt_base.as_mut_ptr()) };
        let mut mapper =
            unsafe { OffsetPageTable::new(table, VirtAddr::new(active_physical_offset())) };
        let frame_allocator = unsafe { global_memory_manager_mut() };
        let Some(frame_allocator) = frame_allocator else {
            continue;
        };
        let unmap_result = paging::unmap_page(&mut mapper, VirtAddr::new(virt));
        let frame = match unmap_result {
            Ok(frame) => frame,
            Err(UnmapError::ParentEntryHugePage) => {
                let flags = page_table_flags(virt)
                    .unwrap_or(PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE);
                if !split_huge_page(
                    &mut mapper,
                    frame_allocator,
                    Page::containing_address(VirtAddr::new(virt)),
                    flags,
                ) {
                    continue;
                }
                let retry = paging::unmap_page(&mut mapper, VirtAddr::new(virt));
                match retry {
                    Ok(frame) => frame,
                    Err(_) => continue,
                }
            }
            Err(_) => continue,
        };
        let phys_unmapped = frame.start_address().as_u64();
        let new_count = dec_frame_ref(phys_unmapped);
        if new_count == 0 {
            match &entry.backing {
                PageBacking::Anonymous { shared_id } => {
                    if *shared_id != 0 {
                        SHARED_ANON_PAGES
                            .lock()
                            .pages
                            .remove(&(*shared_id, entry.page_index));
                    }
                }
                PageBacking::File {
                    inode,
                    file_offset,
                    region_start,
                    ..
                } => {
                    let page_file_offset =
                        file_offset.saturating_add(virt.saturating_sub(*region_start));
                    let page_index = page_file_offset / PAGE_SIZE as u64;
                    SHARED_FILE_PAGES
                        .lock()
                        .pages
                        .remove(&(inode_key(inode), page_index));
                }
                PageBacking::Image { .. } => {}
            }
            deallocate_contiguous_frames(frame, 1);
        }
        damon::record_eviction(entry.space_id, entry.page_index);
        mglru::record_eviction(entry.space_id, entry.page_index);
        freed = freed.saturating_add(1);
    }
    freed
}

pub fn reclaim_pages(target: usize) -> usize {
    reclaim_pages_scoped(target, false)
}

pub fn reclaim_pages_global(target: usize) -> usize {
    reclaim_pages_scoped(target, true)
}

pub fn unmap_user_range(start: u64, size: u64) -> bool {
    if size == 0 {
        return false;
    }
    let page_mask = !(PAGE_SIZE as u64 - 1);
    let start = start & page_mask;
    let end = start
        .saturating_add(size)
        .saturating_add(PAGE_SIZE as u64 - 1)
        & page_mask;
    if end <= start {
        return false;
    }
    if !is_user_range(start, end.saturating_sub(start)) {
        return false;
    }
    let overlaps = with_address_space_ref(|space| {
        space
            .vmas
            .iter()
            .filter(|region| end > region.start && start < region.end)
            .cloned()
            .collect::<Vec<_>>()
    });
    for region in &overlaps {
        let overlap_start = max(start, region.start);
        let overlap_end = min(end, region.end);
        if overlap_start < overlap_end {
            if !writeback_file_range(&region, overlap_start, overlap_end) {
                return false;
            }
        }
    }
    let (level_4_frame, _) = Cr3::read();
    let phys_base = level_4_frame.start_address();
    let virt_base = VirtAddr::new(active_physical_offset() + phys_base.as_u64());
    let table = unsafe { &mut *(virt_base.as_mut_ptr()) };
    let mut mapper =
        unsafe { OffsetPageTable::new(table, VirtAddr::new(active_physical_offset())) };
    let start_page = Page::<Size4KiB>::containing_address(VirtAddr::new(start));
    let end_page = Page::<Size4KiB>::containing_address(VirtAddr::new(end.saturating_sub(1)));
    for page in Page::range_inclusive(start_page, end_page) {
        let addr = page.start_address().as_u64();
        let page_index = addr / PAGE_SIZE as u64;
        let space_id = current_space_id();
        remove_lru_mapping(space_id, page_index);
        swap_remove_page(space_id, page_index);
        if paging::translate_addr(page.start_address()).is_some() {
            let unmap_result =
                paging::with_wp_disabled(|| paging::unmap_page(&mut mapper, page.start_address()));
            if let Ok(frame) = unmap_result {
                let phys = frame.start_address().as_u64();
                let current = frame_refcount(phys);
                if current == 0 {
                    continue;
                }
                let mut shared_anon_key: Option<(u64, u64)> = None;
                let mut shared_file_key: Option<(usize, u64)> = None;
                for region in &overlaps {
                    if addr < region.start || addr >= region.end {
                        continue;
                    }
                    match &region.kind {
                        VmaKind::Anonymous { id } => {
                            if region.shared && *id != 0 {
                                let page_index =
                                    addr.saturating_sub(region.start) / PAGE_SIZE as u64;
                                shared_anon_key = Some((*id, page_index));
                            }
                        }
                        VmaKind::File {
                            inode, file_offset, ..
                        } => {
                            if region.shared {
                                let page_offset =
                                    file_offset.saturating_add(addr.saturating_sub(region.start));
                                let page_index = page_offset / PAGE_SIZE as u64;
                                shared_file_key = Some((inode_key(inode), page_index));
                            }
                        }
                        _ => {}
                    }
                    break;
                }
                let new_count = dec_frame_ref(phys);
                if new_count == 0 {
                    if let Some(key) = shared_anon_key {
                        SHARED_ANON_PAGES.lock().pages.remove(&key);
                    }
                    if let Some(key) = shared_file_key {
                        SHARED_FILE_PAGES.lock().pages.remove(&key);
                    }
                    deallocate_contiguous_frames(frame, 1);
                }
            }
        }
    }
    with_address_space_mut(|space| {
        let mut next = Vec::with_capacity(space.vmas.len().saturating_add(2));
        for region in &space.vmas {
            if end <= region.start || start >= region.end {
                next.push(region.clone());
                continue;
            }
            if start > region.start {
                let mut left = region.clone();
                left.end = start;
                next.push(left);
            }
            if end < region.end {
                let mut right = region.clone();
                right.start = end;
                next.push(right);
            }
        }
        merge_adjacent(&mut next);
        space.vmas = next;
    });
    true
}

pub fn register_cow_region(start: u64, size: u64, flags: PageTableFlags) -> bool {
    if size == 0 {
        return false;
    }
    let page_mask = !(PAGE_SIZE as u64 - 1);
    let start = start & page_mask;
    let end = start
        .saturating_add(size)
        .saturating_add(PAGE_SIZE as u64 - 1)
        & page_mask;
    if end <= start {
        return false;
    }
    if !is_user_range(start, end.saturating_sub(start)) {
        return false;
    }
    let flags = enforce_wx(flags | PageTableFlags::USER_ACCESSIBLE);
    with_address_space_mut(|space| {
        let mut updated = false;
        let mut next = Vec::with_capacity(space.vmas.len().saturating_add(2));
        for region in &space.vmas {
            if end <= region.start || start >= region.end {
                next.push(region.clone());
                continue;
            }
            if start > region.start {
                let mut left = region.clone();
                left.end = start;
                next.push(left);
            }
            let mid_start = region.start.max(start);
            let mid_end = region.end.min(end);
            if mid_start < mid_end {
                let mut mid = region.clone();
                mid.start = mid_start;
                mid.end = mid_end;
                mid.flags = flags;
                mid.cow = true;
                mid.shared = false;
                next.push(mid);
                updated = true;
            }
            if end < region.end {
                let mut right = region.clone();
                right.start = end;
                next.push(right);
            }
        }
        merge_adjacent(&mut next);
        space.vmas = next;
        updated
    })
}

pub fn set_user_image(image: &[u8]) {
    with_address_space_mut(|space| {
        space.image = Some(image_ref_from_slice(image));
    });
}

pub fn set_user_image_owned(image: Arc<[u8]>) {
    with_address_space_mut(|space| {
        space.image = Some(image_ref_from_owned(image));
    });
}

pub fn register_file_lazy_region(
    seg_start: u64,
    seg_size: u64,
    flags: PageTableFlags,
    file_offset: u64,
    file_size: u64,
) -> bool {
    if seg_size == 0 {
        return false;
    }
    let page_mask = !(PAGE_SIZE as u64 - 1);
    let seg_end = seg_start.saturating_add(seg_size);
    let region_start = seg_start & page_mask;
    let region_end = seg_end.saturating_add(PAGE_SIZE as u64 - 1) & page_mask;
    if region_end <= region_start {
        return false;
    }
    if !is_user_range(region_start, region_end.saturating_sub(region_start)) {
        return false;
    }
    let flags = enforce_wx(flags | PageTableFlags::USER_ACCESSIBLE);
    with_address_space_mut(|space| {
        insert_vma(
            space,
            Vma {
                start: region_start,
                end: region_end,
                flags,
                kind: VmaKind::Image {
                    seg_start,
                    file_offset,
                    file_size,
                },
                cow: false,
                shared: false,
            },
        )
    })
}

pub fn register_file_backed_region(
    start: u64,
    size: u64,
    flags: PageTableFlags,
    inode: Arc<dyn INode>,
    file_offset: u64,
    file_size: u64,
    shared: bool,
    cow: bool,
) -> bool {
    if size == 0 {
        return false;
    }
    let page_mask = !(PAGE_SIZE as u64 - 1);
    let region_start = start & page_mask;
    let region_end = start
        .saturating_add(size)
        .saturating_add(PAGE_SIZE as u64 - 1)
        & page_mask;
    if region_end <= region_start {
        return false;
    }
    if !is_user_range(region_start, region_end.saturating_sub(region_start)) {
        return false;
    }
    let flags = enforce_wx(flags | PageTableFlags::USER_ACCESSIBLE);
    with_address_space_mut(|space| {
        insert_vma(
            space,
            Vma {
                start: region_start,
                end: region_end,
                flags,
                kind: VmaKind::File {
                    inode,
                    file_offset,
                    file_size,
                },
                cow,
                shared,
            },
        )
    })
}

pub fn handle_user_page_fault(addr: u64, error: PageFaultErrorCode) -> bool {
    let page_mask = !(PAGE_SIZE as u64 - 1);
    let aligned = addr & page_mask;
    if !is_user_address(aligned) {
        return false;
    }
    if error.contains(PageFaultErrorCode::PROTECTION_VIOLATION) {
        if error.contains(PageFaultErrorCode::CAUSED_BY_WRITE) {
            return handle_cow_fault(aligned);
        }
        return false;
    }
    handle_lazy_fault(aligned)
}

fn handle_lazy_fault(addr: u64) -> bool {
    let vma = with_address_space_ref(|space| {
        space
            .vmas
            .iter()
            .find(|region| addr >= region.start && addr < region.end)
            .cloned()
    });
    let Some(vma) = vma else {
        return false;
    };
    match &vma.kind {
        VmaKind::Anonymous { .. } => handle_anon_lazy_fault(addr, &vma),
        VmaKind::Image { .. } => handle_image_lazy_fault(addr, &vma),
        VmaKind::File { .. } => handle_file_lazy_fault(addr, &vma),
    }
}

fn enforce_wx(flags: PageTableFlags) -> PageTableFlags {
    if flags.contains(PageTableFlags::WRITABLE) && !flags.contains(PageTableFlags::NO_EXECUTE) {
        flags | PageTableFlags::NO_EXECUTE
    } else {
        flags
    }
}

fn sanitize_user_map_flags(addr: u64, size: u64, flags: PageTableFlags) -> Option<PageTableFlags> {
    let flags = enforce_wx(flags);
    if flags.contains(PageTableFlags::USER_ACCESSIBLE) && !is_user_range(addr, size) {
        return None;
    }
    Some(flags)
}

fn vma_map_flags(vma: &Vma) -> PageTableFlags {
    let mut flags = vma.flags | PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
    if vma.cow {
        flags.remove(PageTableFlags::WRITABLE);
    }
    enforce_wx(flags)
}

fn update_page_flags_with_split(
    mapper: &mut (impl MapperAllSizes + Translate),
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    page: Page<Size4KiB>,
    flags: PageTableFlags,
) -> bool {
    let flags =
        match sanitize_user_map_flags(page.start_address().as_u64(), PAGE_SIZE as u64, flags) {
            Some(value) => value,
            None => return false,
        };
    let update_result = paging::with_wp_disabled(|| unsafe { mapper.update_flags(page, flags) });
    match update_result {
        Ok(flush) => {
            flush.flush();
            true
        }
        Err(FlagUpdateError::ParentEntryHugePage) => {
            if !split_huge_page(mapper, frame_allocator, page, flags) {
                return false;
            }
            let update_result =
                paging::with_wp_disabled(|| unsafe { mapper.update_flags(page, flags) });
            match update_result {
                Ok(flush) => {
                    flush.flush();
                    true
                }
                Err(FlagUpdateError::PageNotMapped) => true,
                Err(_) => false,
            }
        }
        Err(FlagUpdateError::PageNotMapped) => true,
        Err(_) => false,
    }
}

pub fn audit_user_mappings() -> bool {
    let vmas = with_address_space_ref(|space| space.vmas.clone());
    for region in vmas {
        if !is_user_range(region.start, region.end.saturating_sub(region.start)) {
            return false;
        }
        let mut addr = region.start;
        while addr < region.end {
            let flags = paging::translate_effective_flags(VirtAddr::new(addr));
            if let Some(flags) = flags {
                if !flags.contains(PageTableFlags::USER_ACCESSIBLE) {
                    return false;
                }
                if flags.contains(PageTableFlags::WRITABLE)
                    && !flags.contains(PageTableFlags::NO_EXECUTE)
                {
                    return false;
                }
            }
            addr = addr.saturating_add(PAGE_SIZE as u64);
        }
    }
    true
}

pub fn audit_kernel_user_flags() -> bool {
    let (level_4_frame, _) = Cr3::read();
    let table = unsafe { phys_table(level_4_frame) };
    for idx in 256..512 {
        let entry = &table[idx];
        if entry.flags().contains(PageTableFlags::USER_ACCESSIBLE) {
            return false;
        }
    }
    true
}

pub fn audit_page_table_security() -> bool {
    audit_user_mappings() && audit_kernel_user_flags()
}

fn handle_anon_lazy_fault(addr: u64, region: &Vma) -> bool {
    let page_mask = !(PAGE_SIZE as u64 - 1);
    let page_start = addr & page_mask;
    let page_index = page_start.saturating_sub(region.start) / PAGE_SIZE as u64;
    let space_id = current_space_id();
    let frame_allocator = unsafe { global_memory_manager_mut() };
    let Some(frame_allocator) = frame_allocator else {
        return false;
    };
    let (level_4_frame, _) = Cr3::read();
    let phys_base = level_4_frame.start_address();
    let virt_base = VirtAddr::new(active_physical_offset() + phys_base.as_u64());
    let table = unsafe { &mut *(virt_base.as_mut_ptr()) };
    let mut mapper =
        unsafe { OffsetPageTable::new(table, VirtAddr::new(active_physical_offset())) };
    let page = Page::containing_address(VirtAddr::new(addr));
    let map_flags = vma_map_flags(region);
    let map_flags =
        match sanitize_user_map_flags(page.start_address().as_u64(), PAGE_SIZE as u64, map_flags) {
            Some(value) => value,
            None => return false,
        };
    let table_flags =
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
    if region.shared {
        if let VmaKind::Anonymous { id } = &region.kind {
            if *id != 0 {
                let mut shared_pages = SHARED_ANON_PAGES.lock();
                if let Some(phys) = shared_pages.pages.get(&(*id, page_index)).copied() {
                    let frame = PhysFrame::containing_address(PhysAddr::new(phys));
                    let map_result = paging::with_wp_disabled(|| unsafe {
                        mapper.map_to_with_table_flags(
                            page,
                            frame,
                            map_flags,
                            table_flags,
                            frame_allocator,
                        )
                    });
                    return match map_result {
                        Ok(flush) => {
                            flush.flush();
                            inc_frame_ref(phys);
                            register_lru_mapping(addr, phys, region);
                            true
                        }
                        Err(_) => false,
                    };
                }
                let frame = match frame_allocator.allocate_frame() {
                    Some(frame) => frame,
                    None => return false,
                };
                let phys = frame.start_address().as_u64();
                let virt = active_physical_offset().saturating_add(phys);
                unsafe {
                    core::ptr::write_bytes(virt as *mut u8, 0, PAGE_SIZE);
                }
                let map_result = paging::with_wp_disabled(|| unsafe {
                    mapper.map_to_with_table_flags(
                        page,
                        frame,
                        map_flags,
                        table_flags,
                        frame_allocator,
                    )
                });
                return match map_result {
                    Ok(flush) => {
                        flush.flush();
                        shared_pages.pages.insert((*id, page_index), phys);
                        inc_frame_ref(phys);
                        register_lru_mapping(addr, phys, region);
                        true
                    }
                    Err(_) => {
                        deallocate_contiguous_frames(frame, 1);
                        false
                    }
                };
            }
        }
    }
    if let Some(data) = swap_take_page(space_id, page_start / PAGE_SIZE as u64) {
        let frame = match frame_allocator.allocate_frame() {
            Some(frame) => frame,
            None => {
                if !swap_store_page(space_id, page_start / PAGE_SIZE as u64, data) {
                    return false;
                }
                return false;
            }
        };
        let phys = frame.start_address().as_u64();
        let virt = active_physical_offset().saturating_add(phys);
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr(), virt as *mut u8, PAGE_SIZE);
        }
        let map_result = paging::with_wp_disabled(|| unsafe {
            mapper.map_to_with_table_flags(page, frame, map_flags, table_flags, frame_allocator)
        });
        return match map_result {
            Ok(flush) => {
                flush.flush();
                inc_frame_ref(phys);
                register_lru_mapping(addr, phys, region);
                true
            }
            Err(_) => {
                deallocate_contiguous_frames(frame, 1);
                if !swap_store_page(space_id, page_start / PAGE_SIZE as u64, data) {
                    return false;
                }
                false
            }
        };
    }
    if try_map_thp_anon(&mut mapper, frame_allocator, addr, region) {
        return true;
    }
    let frame = match frame_allocator.allocate_frame() {
        Some(frame) => frame,
        None => return false,
    };
    let phys = frame.start_address().as_u64();
    let virt = active_physical_offset().saturating_add(phys);
    unsafe {
        core::ptr::write_bytes(virt as *mut u8, 0, PAGE_SIZE);
    }
    let map_result = paging::with_wp_disabled(|| unsafe {
        mapper.map_to_with_table_flags(page, frame, map_flags, table_flags, frame_allocator)
    });
    match map_result {
        Ok(flush) => {
            flush.flush();
            inc_frame_ref(phys);
            register_lru_mapping(addr, phys, region);
            true
        }
        Err(_) => {
            deallocate_contiguous_frames(frame, 1);
            false
        }
    }
}

fn handle_image_lazy_fault(addr: u64, region: &Vma) -> bool {
    let image = with_address_space_ref(|space| space.image.clone());
    let Some(image) = image else {
        return false;
    };
    let VmaKind::Image {
        seg_start,
        file_offset,
        file_size,
    } = &region.kind
    else {
        return false;
    };
    let frame_allocator = unsafe { global_memory_manager_mut() };
    let Some(frame_allocator) = frame_allocator else {
        return false;
    };
    let frame = match frame_allocator.allocate_frame() {
        Some(frame) => frame,
        None => return false,
    };
    let phys = frame.start_address().as_u64();
    let virt = active_physical_offset().saturating_add(phys);
    unsafe {
        core::ptr::write_bytes(virt as *mut u8, 0, PAGE_SIZE);
    }
    let page_mask = !(PAGE_SIZE as u64 - 1);
    let page_start = addr & page_mask;
    let page_end = page_start.saturating_add(PAGE_SIZE as u64);
    let file_end = seg_start.saturating_add(*file_size);
    let copy_start = max(page_start, *seg_start);
    let copy_end = min(page_end, file_end);
    if copy_start < copy_end {
        let file_offset = file_offset.saturating_add(copy_start.saturating_sub(*seg_start));
        let copy_len = copy_end.saturating_sub(copy_start) as usize;
        if file_offset.saturating_add(copy_len as u64) > image.len as u64 {
            return false;
        }
        let src = unsafe { (image.base as *const u8).add(file_offset as usize) };
        let dst = (virt as u64).saturating_add(copy_start.saturating_sub(page_start)) as *mut u8;
        unsafe {
            core::ptr::copy_nonoverlapping(src, dst, copy_len);
        }
    }
    let (level_4_frame, _) = Cr3::read();
    let phys_base = level_4_frame.start_address();
    let virt_base = VirtAddr::new(active_physical_offset() + phys_base.as_u64());
    let table = unsafe { &mut *(virt_base.as_mut_ptr()) };
    let mut mapper =
        unsafe { OffsetPageTable::new(table, VirtAddr::new(active_physical_offset())) };
    let page = Page::containing_address(VirtAddr::new(addr));
    let map_flags = vma_map_flags(region);
    let table_flags =
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
    let map_result = paging::with_wp_disabled(|| unsafe {
        mapper.map_to_with_table_flags(page, frame, map_flags, table_flags, frame_allocator)
    });
    match map_result {
        Ok(flush) => {
            flush.flush();
            inc_frame_ref(phys);
            register_lru_mapping(addr, phys, region);
            true
        }
        Err(_) => {
            deallocate_contiguous_frames(frame, 1);
            false
        }
    }
}

fn handle_file_lazy_fault(addr: u64, region: &Vma) -> bool {
    let VmaKind::File {
        inode,
        file_offset,
        file_size,
    } = &region.kind
    else {
        return false;
    };
    let frame_allocator = unsafe { global_memory_manager_mut() };
    let Some(frame_allocator) = frame_allocator else {
        return false;
    };
    let page_mask = !(PAGE_SIZE as u64 - 1);
    let page_start = addr & page_mask;
    let page_file_offset = file_offset.saturating_add(page_start.saturating_sub(region.start));
    let file_end = file_offset.saturating_add(*file_size);
    let page_index = page_file_offset / PAGE_SIZE as u64;
    let space_id = current_space_id();
    let (level_4_frame, _) = Cr3::read();
    let phys_base = level_4_frame.start_address();
    let virt_base = VirtAddr::new(active_physical_offset() + phys_base.as_u64());
    let table = unsafe { &mut *(virt_base.as_mut_ptr()) };
    let mut mapper =
        unsafe { OffsetPageTable::new(table, VirtAddr::new(active_physical_offset())) };
    let page = Page::containing_address(VirtAddr::new(addr));
    let map_flags = vma_map_flags(region);
    let table_flags =
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
    if !region.shared {
        if let Some(data) = swap_take_page(space_id, page_start / PAGE_SIZE as u64) {
            let frame = match frame_allocator.allocate_frame() {
                Some(frame) => frame,
                None => {
                    if !swap_store_page(space_id, page_start / PAGE_SIZE as u64, data) {
                        return false;
                    }
                    return false;
                }
            };
            let phys = frame.start_address().as_u64();
            let virt = active_physical_offset().saturating_add(phys);
            unsafe {
                core::ptr::copy_nonoverlapping(data.as_ptr(), virt as *mut u8, PAGE_SIZE);
            }
            let map_result = paging::with_wp_disabled(|| unsafe {
                mapper.map_to_with_table_flags(page, frame, map_flags, table_flags, frame_allocator)
            });
            return match map_result {
                Ok(flush) => {
                    flush.flush();
                    inc_frame_ref(phys);
                    register_lru_mapping(addr, phys, region);
                    true
                }
                Err(_) => {
                    deallocate_contiguous_frames(frame, 1);
                    if !swap_store_page(space_id, page_start / PAGE_SIZE as u64, data) {
                        return false;
                    }
                    false
                }
            };
        }
    }
    if region.shared {
        let key = (inode_key(inode), page_index);
        let mut shared_pages = SHARED_FILE_PAGES.lock();
        if let Some(phys) = shared_pages.pages.get(&key).copied() {
            let frame = PhysFrame::containing_address(PhysAddr::new(phys));
            let map_result = paging::with_wp_disabled(|| unsafe {
                mapper.map_to_with_table_flags(page, frame, map_flags, table_flags, frame_allocator)
            });
            return match map_result {
                Ok(flush) => {
                    flush.flush();
                    inc_frame_ref(phys);
                    if region.flags.contains(PageTableFlags::WRITABLE) {
                        mark_cache_dirty(key.0, key);
                    }
                    register_lru_mapping(addr, phys, region);
                    true
                }
                Err(_) => false,
            };
        }
        let frame = match frame_allocator.allocate_frame() {
            Some(frame) => frame,
            None => return false,
        };
        let phys = frame.start_address().as_u64();
        let virt = active_physical_offset().saturating_add(phys);
        unsafe {
            core::ptr::write_bytes(virt as *mut u8, 0, PAGE_SIZE);
        }
        if page_file_offset < file_end {
            let data = match read_cached_file_page(inode, page_file_offset, file_end) {
                Some(value) => value,
                None => {
                    deallocate_contiguous_frames(frame, 1);
                    return false;
                }
            };
            let copy_len =
                min(PAGE_SIZE as u64, file_end.saturating_sub(page_file_offset)) as usize;
            if copy_len > 0 {
                unsafe {
                    core::ptr::copy_nonoverlapping(data.as_ptr(), virt as *mut u8, copy_len);
                }
            }
        }
        let map_result = paging::with_wp_disabled(|| unsafe {
            mapper.map_to_with_table_flags(page, frame, map_flags, table_flags, frame_allocator)
        });
        return match map_result {
            Ok(flush) => {
                flush.flush();
                shared_pages.pages.insert(key, phys);
                inc_frame_ref(phys);
                if region.flags.contains(PageTableFlags::WRITABLE) {
                    mark_cache_dirty(key.0, key);
                }
                register_lru_mapping(addr, phys, region);
                true
            }
            Err(_) => {
                deallocate_contiguous_frames(frame, 1);
                false
            }
        };
    }
    let frame = match frame_allocator.allocate_frame() {
        Some(frame) => frame,
        None => return false,
    };
    let phys = frame.start_address().as_u64();
    let virt = active_physical_offset().saturating_add(phys);
    unsafe {
        core::ptr::write_bytes(virt as *mut u8, 0, PAGE_SIZE);
    }
    if page_file_offset < file_end {
        let data = match read_cached_file_page(inode, page_file_offset, file_end) {
            Some(value) => value,
            None => {
                deallocate_contiguous_frames(frame, 1);
                return false;
            }
        };
        let copy_len = min(PAGE_SIZE as u64, file_end.saturating_sub(page_file_offset)) as usize;
        if copy_len > 0 {
            unsafe {
                core::ptr::copy_nonoverlapping(data.as_ptr(), virt as *mut u8, copy_len);
            }
        }
    }
    let map_result = paging::with_wp_disabled(|| unsafe {
        mapper.map_to_with_table_flags(page, frame, map_flags, table_flags, frame_allocator)
    });
    match map_result {
        Ok(flush) => {
            flush.flush();
            if region.shared && region.flags.contains(PageTableFlags::WRITABLE) {
                let key = (inode_key(inode), page_index);
                mark_cache_dirty(key.0, key);
            }
            inc_frame_ref(phys);
            register_lru_mapping(addr, phys, region);
            true
        }
        Err(_) => {
            deallocate_contiguous_frames(frame, 1);
            false
        }
    }
}

fn handle_cow_fault(addr: u64) -> bool {
    let region = with_address_space_ref(|space| {
        space
            .vmas
            .iter()
            .find(|region| region.cow && addr >= region.start && addr < region.end)
            .cloned()
    });
    let Some(region) = region else {
        return false;
    };
    let flags = region.flags;
    let old_phys = match paging::translate_addr(VirtAddr::new(addr)) {
        Some(addr) => addr.as_u64() & !(PAGE_SIZE as u64 - 1),
        None => return false,
    };
    let frame_allocator = unsafe { global_memory_manager_mut() };
    let Some(frame_allocator) = frame_allocator else {
        return false;
    };
    let force_copy = matches!(region.kind, VmaKind::File { .. });
    if !force_copy && frame_refcount(old_phys) <= 1 {
        let (level_4_frame, _) = Cr3::read();
        let phys_base = level_4_frame.start_address();
        let virt_base = VirtAddr::new(active_physical_offset() + phys_base.as_u64());
        let table = unsafe { &mut *(virt_base.as_mut_ptr()) };
        let mut mapper =
            unsafe { OffsetPageTable::new(table, VirtAddr::new(active_physical_offset())) };
        let page = Page::containing_address(VirtAddr::new(addr));
        let map_flags = flags
            | PageTableFlags::PRESENT
            | PageTableFlags::USER_ACCESSIBLE
            | PageTableFlags::WRITABLE;
        let map_flags = match sanitize_user_map_flags(
            page.start_address().as_u64(),
            PAGE_SIZE as u64,
            map_flags,
        ) {
            Some(value) => value,
            None => return false,
        };
        if update_page_flags_with_split(&mut mapper, frame_allocator, page, map_flags) {
            register_lru_mapping(addr, old_phys, &region);
            return true;
        }
        return false;
    }
    let new_frame = match frame_allocator.allocate_frame() {
        Some(frame) => frame,
        None => return false,
    };
    let new_phys = new_frame.start_address().as_u64();
    let src = active_physical_offset().saturating_add(old_phys) as *const u8;
    let dst = active_physical_offset().saturating_add(new_phys) as *mut u8;
    unsafe {
        core::ptr::copy_nonoverlapping(src, dst, PAGE_SIZE);
    }
    let (level_4_frame, _) = Cr3::read();
    let phys_base = level_4_frame.start_address();
    let virt_base = VirtAddr::new(active_physical_offset() + phys_base.as_u64());
    let table = unsafe { &mut *(virt_base.as_mut_ptr()) };
    let mut mapper =
        unsafe { OffsetPageTable::new(table, VirtAddr::new(active_physical_offset())) };
    let page = Page::containing_address(VirtAddr::new(addr));
    let unmap_result = paging::with_wp_disabled(|| mapper.unmap(page));
    if unmap_result.is_err() {
        deallocate_contiguous_frames(new_frame, 1);
        return false;
    }
    let map_flags = flags
        | PageTableFlags::PRESENT
        | PageTableFlags::USER_ACCESSIBLE
        | PageTableFlags::WRITABLE;
    let table_flags =
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
    let map_result = paging::with_wp_disabled(|| unsafe {
        mapper.map_to_with_table_flags(page, new_frame, map_flags, table_flags, frame_allocator)
    });
    match map_result {
        Ok(flush) => {
            flush.flush();
            inc_frame_ref(new_phys);
            free_frame_if_unused(old_phys);
            register_lru_mapping(addr, new_phys, &region);
            true
        }
        Err(_) => {
            deallocate_contiguous_frames(new_frame, 1);
            false
        }
    }
}

pub fn allocate_contiguous_frames(pages: usize) -> Option<PhysFrame> {
    unsafe {
        global_memory_manager_mut().and_then(|manager| manager.allocate_contiguous_frames(pages))
    }
}

/// Ardışık fiziksel bellek tahsis eder ve fiziksel adresi döndürür
/// NVMe gibi sürücüler tarafından DMA tamponları için kullanılır
pub fn alloc_phys(size: usize) -> Option<u64> {
    let page_size = 4096;
    let pages = (size + page_size - 1) / page_size;

    allocate_contiguous_frames(pages).map(|frame| frame.start_address().as_u64())
}

/// alloc_phys tarafından tahsis edilen ardışık fiziksel belleği serbest bırakır
pub fn free_phys(phys_addr: u64, size: usize) {
    let page_size = 4096;
    let pages = (size + page_size - 1) / page_size;

    // Adresten PhysFrame oluştur
    let frame = PhysFrame::containing_address(x86_64::addr::PhysAddr::new(phys_addr));
    deallocate_contiguous_frames(frame, pages);
}

pub fn deallocate_contiguous_frames(start: PhysFrame, pages: usize) {
    unsafe {
        if let Some(manager) = global_memory_manager_mut() {
            manager.deallocate_contiguous_frames(start, pages);
        }
    }
}

#[derive(Clone, Copy)]
struct DmaMapping {
    vaddr: usize,
    paddr: usize,
    len: usize,
    owned: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DeviceId {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

#[derive(Clone, Copy, Debug)]
struct IommuMapping {
    vaddr: usize,
    paddr: usize,
    len: usize,
    owned: bool,
}

struct IommuDomain {
    id: u32,
    devices: Vec<DeviceId>,
    mappings: BTreeMap<usize, IommuMapping>,
}

struct IommuState {
    enabled: bool,
    initialized: bool,
    next_domain_id: u32,
    device_domains: BTreeMap<DeviceId, u32>,
    mapping_owner: BTreeMap<usize, u32>,
    domains: BTreeMap<u32, IommuDomain>,
}

lazy_static! {
    static ref DMA_MAPPINGS: Mutex<BTreeMap<usize, DmaMapping>> = Mutex::new(BTreeMap::new());
    static ref IOMMU_STATE: Mutex<IommuState> = Mutex::new(IommuState {
        enabled: false,
        initialized: false,
        next_domain_id: 1,
        device_domains: BTreeMap::new(),
        mapping_owner: BTreeMap::new(),
        domains: BTreeMap::new(),
    });
}

fn dma_mapping_contains(mapping: &DmaMapping, start: usize, len: usize) -> bool {
    let end = start.saturating_add(len);
    start >= mapping.vaddr && end <= mapping.vaddr.saturating_add(mapping.len)
}

fn find_dma_mapping(start: usize, len: usize) -> Option<usize> {
    let map = DMA_MAPPINGS.lock();
    let (base, mapping) = map.range(..=start).next_back()?;
    if !dma_mapping_contains(mapping, start, len) {
        return None;
    }
    let offset = start.saturating_sub(*base);
    Some(mapping.paddr.saturating_add(offset))
}

fn insert_dma_mapping(mapping: DmaMapping) -> bool {
    let mut map = DMA_MAPPINGS.lock();
    if let Some((_, prev)) = map.range(..=mapping.vaddr).next_back() {
        if mapping.vaddr < prev.vaddr.saturating_add(prev.len) {
            return false;
        }
    }
    if let Some((next_base, _)) = map.range(mapping.vaddr..).next() {
        if mapping.vaddr.saturating_add(mapping.len) > *next_base {
            return false;
        }
    }
    map.insert(mapping.vaddr, mapping);
    true
}

fn remove_dma_mapping(vaddr: usize) -> Option<DmaMapping> {
    DMA_MAPPINGS.lock().remove(&vaddr)
}

fn is_kernel_range(start: u64, len: u64) -> bool {
    if len == 0 {
        return false;
    }
    let end = start.saturating_add(len.saturating_sub(1));
    start >= KERNEL_SPACE_START && end >= KERNEL_SPACE_START && !is_user_range(start, len)
}

#[cfg(any(target_os = "none", target_os = "uefi"))]
pub fn dma_alloc(pages: usize) -> Option<(usize, NonNull<u8>)> {
    if pages == 0 {
        return None;
    }
    let frame = allocate_contiguous_frames(pages)?;
    let paddr = frame.start_address().as_u64() as usize;
    if paddr == 0 {
        deallocate_contiguous_frames(frame, pages);
        return None;
    }
    let vaddr_ptr = phys_to_virt(paddr) as *mut u8;
    let vaddr = NonNull::new(vaddr_ptr)?;
    unsafe { core::ptr::write_bytes(vaddr.as_ptr(), 0, pages.saturating_mul(PAGE_SIZE)) };
    let len = pages.saturating_mul(PAGE_SIZE);
    let mapping = DmaMapping {
        vaddr: vaddr.as_ptr() as usize,
        paddr,
        len,
        owned: true,
    };
    if !insert_dma_mapping(mapping) {
        deallocate_contiguous_frames(frame, pages);
        return None;
    }
    Some((paddr, vaddr))
}

#[cfg(any(target_os = "none", target_os = "uefi"))]
pub fn dma_dealloc(paddr: usize, pages: usize) {
    if paddr == 0 || pages == 0 {
        return;
    }
    let vaddr = phys_to_virt(paddr);
    if let Some(entry) = remove_dma_mapping(vaddr) {
        if !entry.owned {
            insert_dma_mapping(entry);
            return;
        }
    }
    let frame = PhysFrame::containing_address(PhysAddr::new(paddr as u64));
    deallocate_contiguous_frames(frame, pages);
}

#[cfg(all(not(target_os = "none"), not(target_os = "uefi")))]
pub fn dma_alloc(pages: usize) -> Option<(usize, NonNull<u8>)> {
    if pages == 0 {
        return None;
    }
    let len = pages.saturating_mul(PAGE_SIZE);
    let layout = core::alloc::Layout::from_size_align(len, PAGE_SIZE).ok()?;
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    let vaddr = NonNull::new(ptr)?;
    let mapping = DmaMapping {
        vaddr: vaddr.as_ptr() as usize,
        paddr: vaddr.as_ptr() as usize,
        len,
        owned: true,
    };
    if !insert_dma_mapping(mapping) {
        unsafe { std::alloc::dealloc(vaddr.as_ptr(), layout) };
        return None;
    }
    Some((vaddr.as_ptr() as usize, vaddr))
}

#[cfg(all(not(target_os = "none"), not(target_os = "uefi")))]
pub fn dma_dealloc(paddr: usize, pages: usize) {
    if paddr == 0 || pages == 0 {
        return;
    }
    let len = pages.saturating_mul(PAGE_SIZE);
    let Some(entry) = remove_dma_mapping(paddr) else {
        return;
    };
    if !entry.owned {
        let _ = insert_dma_mapping(entry);
        return;
    }
    if let Ok(layout) = core::alloc::Layout::from_size_align(len, PAGE_SIZE) {
        unsafe { std::alloc::dealloc(entry.vaddr as *mut u8, layout) };
    }
}

pub fn dma_share(buffer: NonNull<[u8]>) -> Option<usize> {
    let len = buffer.len();
    if len == 0 {
        return None;
    }
    let start = buffer.as_ptr() as *const u8 as u64;
    if !is_kernel_range(start, len as u64) {
        return None;
    }
    if let Some(paddr) = find_dma_mapping(start as usize, len) {
        return Some(paddr);
    }
    let end = start.saturating_add(len as u64 - 1);
    let start_page = Page::<Size4KiB>::containing_address(VirtAddr::new(start));
    let end_page = Page::<Size4KiB>::containing_address(VirtAddr::new(end));
    let mut expected: Option<u64> = None;
    for page in Page::range_inclusive(start_page, end_page) {
        let phys = translate_addr(page.start_address().as_u64())?;
        match expected {
            None => expected = Some(phys),
            Some(prev) => {
                if phys != prev.saturating_add(PAGE_SIZE as u64) {
                    return None;
                }
                expected = Some(phys);
            }
        }
    }
    let base = expected?;
    let offset = start & (PAGE_SIZE as u64 - 1);
    let dma_base = base.saturating_add(offset) as usize;
    let mapping = DmaMapping {
        vaddr: start as usize,
        paddr: dma_base,
        len,
        owned: false,
    };
    if !insert_dma_mapping(mapping) {
        return None;
    }
    Some(dma_base)
}

pub fn dma_unshare(buffer: NonNull<[u8]>) {
    let len = buffer.len();
    if len == 0 {
        return;
    }
    let start = buffer.as_ptr() as *const u8 as usize;
    if let Some(entry) = remove_dma_mapping(start) {
        if entry.owned {
            insert_dma_mapping(entry);
        }
    }
}

pub fn init_iommu() -> bool {
    let units = crate::cpu::acpi::get_dmar_units();
    let mut state = IOMMU_STATE.lock();
    state.initialized = true;
    state.enabled = !units.is_empty();
    state.next_domain_id = 1;
    state.device_domains.clear();
    state.mapping_owner.clear();
    state.domains.clear();
    state.domains.insert(
        0,
        IommuDomain {
            id: 0,
            devices: Vec::new(),
            mappings: BTreeMap::new(),
        },
    );
    for unit in units {
        for device in unit.devices {
            let dev = DeviceId {
                bus: device.bus,
                device: device.device,
                function: device.function,
            };
            if state.device_domains.contains_key(&dev) {
                continue;
            }
            let domain_id = state.next_domain_id;
            state.next_domain_id = state.next_domain_id.saturating_add(1);
            state.device_domains.insert(dev, domain_id);
            state.domains.insert(
                domain_id,
                IommuDomain {
                    id: domain_id,
                    devices: vec![dev],
                    mappings: BTreeMap::new(),
                },
            );
        }
    }
    state.enabled
}

pub fn iommu_enabled() -> bool {
    IOMMU_STATE.lock().enabled
}

pub fn iommu_register_device(bus: u8, device: u8, function: u8) -> u32 {
    let dev = DeviceId {
        bus,
        device,
        function,
    };
    let mut state = IOMMU_STATE.lock();
    if !state.initialized {
        state.initialized = true;
        state.domains.insert(
            0,
            IommuDomain {
                id: 0,
                devices: Vec::new(),
                mappings: BTreeMap::new(),
            },
        );
    }
    if let Some(domain) = state.device_domains.get(&dev).copied() {
        return domain;
    }
    let domain_id = state.next_domain_id;
    state.next_domain_id = state.next_domain_id.saturating_add(1);
    state.device_domains.insert(dev, domain_id);
    state.domains.insert(
        domain_id,
        IommuDomain {
            id: domain_id,
            devices: vec![dev],
            mappings: BTreeMap::new(),
        },
    );
    domain_id
}

pub fn iommu_domain_for_device(bus: u8, device: u8, function: u8) -> Option<u32> {
    let dev = DeviceId {
        bus,
        device,
        function,
    };
    let state = IOMMU_STATE.lock();
    state.device_domains.get(&dev).copied()
}

fn iommu_map_domain(domain_id: u32, vaddr: usize, paddr: usize, len: usize, owned: bool) -> bool {
    let mut state = IOMMU_STATE.lock();
    if let Some(owner) = state.mapping_owner.get(&vaddr).copied() {
        if owner != domain_id {
            return false;
        }
    }
    // Alan mevcut değilse otomatik oluştur
    if !state.domains.contains_key(&domain_id) {
        state.domains.insert(
            domain_id,
            IommuDomain {
                id: domain_id,
                devices: Vec::new(),
                mappings: BTreeMap::new(),
            },
        );
    }
    state.mapping_owner.insert(vaddr, domain_id);
    let Some(domain) = state.domains.get_mut(&domain_id) else {
        return false;
    };
    domain.mappings.insert(
        vaddr,
        IommuMapping {
            vaddr,
            paddr,
            len,
            owned,
        },
    );
    true
}

fn iommu_unmap_domain(domain_id: u32, vaddr: usize) {
    let mut state = IOMMU_STATE.lock();
    if let Some(domain) = state.domains.get_mut(&domain_id) {
        domain.mappings.remove(&vaddr);
    }
    if let Some(owner) = state.mapping_owner.get(&vaddr).copied() {
        if owner == domain_id {
            state.mapping_owner.remove(&vaddr);
        }
    }
}

pub fn dma_alloc_for_domain(domain_id: u32, pages: usize) -> Option<(usize, NonNull<u8>)> {
    let (paddr, vaddr) = dma_alloc(pages)?;
    let len = pages.saturating_mul(PAGE_SIZE);
    if !iommu_map_domain(domain_id, vaddr.as_ptr() as usize, paddr, len, true) {
        dma_dealloc(paddr, pages);
        return None;
    }
    Some((paddr, vaddr))
}

pub fn dma_dealloc_for_domain(domain_id: u32, paddr: usize, pages: usize) {
    if paddr == 0 || pages == 0 {
        return;
    }
    let vaddr = phys_to_virt(paddr);
    iommu_unmap_domain(domain_id, vaddr);
    dma_dealloc(paddr, pages);
}

pub fn dma_share_for_domain(domain_id: u32, buffer: NonNull<[u8]>) -> Option<usize> {
    let len = buffer.len();
    if len == 0 {
        return None;
    }
    let paddr = dma_share(buffer)?;
    let vaddr = buffer.as_ptr() as *const u8 as usize;
    if !iommu_map_domain(domain_id, vaddr, paddr, len, false) {
        dma_unshare(buffer);
        return None;
    }
    Some(paddr)
}

pub fn dma_unshare_for_domain(domain_id: u32, buffer: NonNull<[u8]>) {
    let len = buffer.len();
    if len == 0 {
        return;
    }
    let vaddr = buffer.as_ptr() as *const u8 as usize;
    iommu_unmap_domain(domain_id, vaddr);
    dma_unshare(buffer);
}

pub fn phys_to_virt(paddr: usize) -> usize {
    paddr.saturating_add(active_physical_offset() as usize)
}

pub fn virt_to_phys(vaddr: usize) -> usize {
    match translate_addr(vaddr as u64) {
        Some(paddr) => paddr as usize,
        None => {
            crate::serial_println!("[MEMORY] virt_to_phys failed for vaddr={:#x}", vaddr);
            panic!("virt_to_phys: unmapped virtual address");
        }
    }
}

/// u64 sanal adres için aşırı yükleme
pub fn virt_to_phys_u64(vaddr: u64) -> u64 {
    match translate_addr(vaddr) {
        Some(paddr) => paddr,
        None => {
            crate::serial_println!("[MEMORY] virt_to_phys failed for vaddr={:#x}", vaddr);
            panic!("virt_to_phys: unmapped virtual address");
        }
    }
}

/// x86_64 tipi sarmalayıcı: VirtAddr → Option<PhysAddr>.
/// Adres mevcut sayfa tablosunda eşlenmemişse None döndürür.
pub fn virt_to_phys_va(va: x86_64::VirtAddr) -> Option<x86_64::PhysAddr> {
    paging::translate_addr(va)
}

// ─── dma_bridge / vfio alt sistemleri için yardımcı fonksiyonlar ─────────────

/// HHDM (Higher-Half Direct Map) offset'ini döndürür.
///
/// ## HHDM Nedir?
/// Kernel başlangıçta tüm fiziksel RAM'ı sanal adres uzayının üst yarısına
/// "doğrudan eşleştirir" (Direct Map). Bu eşlemin başlangıç sabiti HHDM'dir.
/// Bootloader bunu bize bildirir, biz `PHYSICAL_MEMORY_OFFSET` atomic'ine kaydederiz.
///
/// Kullanımı:
///   fiziksel_adres + hhdm_offset() = kernel sanal adresi
///
/// Örnek (tipik x86_64):
///   Fiziksel 0x0008_0000 + hhdm = 0xFFFF_8000_0008_0000
#[inline]
pub fn hhdm_offset() -> u64 {
    active_physical_offset()
}

/// Tek bir sıfırlanmış 4 KB fiziksel sayfa tahsis eder.
///
/// ## Çalışma Mantığı:
/// 1. `alloc_phys(4096)` ile frame allocator'dan fiziksel frame al.
/// 2. `phys + hhdm_offset()` ile HHDM üzerinden sanal adresini hesapla.
/// 3. O sanal adrese 4096 byte sıfır yaz (güvenli başlangıç için zorunlu).
/// 4. HHDM sanal adresini döndür — çağırıcı buraya sayfa tablosu girdisi yazabilir.
///
/// ## Neden Sıfırlama Zorunlu?
/// IOMMU ve CPU sayfa tablolarında "sıfır = geçersiz/yok" anlamına gelir.
/// Eski frame'deki çöp veriler yanlış DMA yetkisi veya sayfa hatası üretir.
pub fn alloc_zeroed_page() -> Option<u64> {
    let phys = alloc_phys(4096)?;
    let hhdm_va = phys + active_physical_offset();
    // HHDM eşlemesi aracılığıyla sayfayı sıfırla
    unsafe { core::ptr::write_bytes(hhdm_va as *mut u8, 0, 4096) };
    Some(hhdm_va)
}

/// Güncel (CR3) adres uzayında `va` sanal adresine fiziksel bir sayfa ekler.
///
/// ## Kullanım Senaryosu — DMA Bridge (Tier 1):
/// GOP framebuffer fiziksel bir adreste bulunur (örn: 0xB000_0000).
/// Oyun process'inin Ring-3'ten buna yazabilmesi için bu fiziksel sayfa,
/// process'in PML4'ine `RING3_FB_VA` adresinden eklenir.
///
/// ## Çalışma Adımları:
/// 1. CR3 oku → aktif PML4'ün fiziksel adresi
/// 2. `phys_base + hhdm` ile PML4'ün sanal adresini bul
/// 3. `x86_64::OffsetPageTable` ile `map_to_with_table_flags` çağır
/// 4. `flush()` ile TLB invalidate yap (aksi halde eski çeviri kullanılır)
///
/// ## Bayraklar (flags):
/// - `PRESENT`          : sayfa tabloda, erişilebilir
/// - `USER_ACCESSIBLE`  : Ring-3 erişebilir (olmadan GPF üretir)
/// - `WRITABLE`         : yazma izni (GPU MMIO için gerekli)
pub fn map_physical_to_user_va(va: u64, phys: u64, flags: PageTableFlags) -> bool {
    let frame_allocator = unsafe {
        match global_memory_manager_mut() {
            Some(m) => m,
            None => return false,
        }
    };
    let (level_4_frame, _) = Cr3::read();
    let phys_base = level_4_frame.start_address();
    let virt_base = VirtAddr::new(active_physical_offset() + phys_base.as_u64());
    let table = unsafe { &mut *(virt_base.as_mut_ptr()) };
    let mut mapper =
        unsafe { OffsetPageTable::new(table, VirtAddr::new(active_physical_offset())) };
    let page = Page::<Size4KiB>::containing_address(VirtAddr::new(va));
    let frame = PhysFrame::containing_address(x86_64::addr::PhysAddr::new(phys));
    let map_flags = (flags | PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE)
        & !PageTableFlags::BIT_9; // clear software bit used for lazy
    let table_flags =
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
    let result = paging::with_wp_disabled(|| unsafe {
        mapper.map_to_with_table_flags(page, frame, map_flags, table_flags, frame_allocator)
    });
    match result {
        Ok(flush) => {
            flush.flush();
            true
        }
        Err(MapToError::PageAlreadyMapped(_)) => false,
        Err(_) => false,
    }
}

/// `va` adresindeki tek 4 KiB sayfayı adres uzayından kaldır.
///
/// ## Önemli: Fiziksel Sayfa Serbest Bırakılmaz
///
/// Bu fonksiyon yalnızca sayfa tablosu girdisini (PTE) siler ve TLB'yi temizler.
/// **Fiziksel çerçeveyi serbest bırakmaz** — bu kasıtlıdır çünkü:
///   - DMA Köprüsü: Framebuffer fiziksel belleği GPU'ya aittir, kernel ayırmadı.
///   - MMIO: IOMMU/GPU kaydedicileri fiziksel adres aralıkları; serbest bırakılamaz.
///   - Çift serbest bırakma riski: Yalnızca PTE silinir, allocator manipüle edilmez.
///
/// DMA Köprüsü'nün `revoke()` methodu tüm framebuffer sayfaları için bu fonksiyonu çağırır.
pub fn unmap_user_va(va: u64) {
    unmap_user_range(va, 4096);
}

/// Kernel'in PML4 frame'i (scheduler context switch için)
pub static mut KERNEL_PML4_FRAME: Option<PhysFrame> = None;
pub static mut KERNEL_PML4_PHYS: u64 = 0;

/// Sayfa tablosunu başlatır.
///
/// # Güvenlik
/// # Parametreler
/// - `physical_memory_offset`: Fiziksel-sanal adres farkı
pub unsafe fn init_paging(physical_memory_offset: u64) -> OffsetPageTable<'static> {
    let (level_4_table_frame, _) = Cr3::read();

    // Kernel PML4'ü scheduler için kaydet
    KERNEL_PML4_FRAME = Some(level_4_table_frame);
    KERNEL_PML4_PHYS = level_4_table_frame.start_address().as_u64();
    ACTIVE_PHYSICAL_MEMORY_OFFSET = physical_memory_offset;

    let phys = level_4_table_frame.start_address();
    let virt = VirtAddr::new(physical_memory_offset + phys.as_u64());
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();

    let level_4_table = &mut *page_table_ptr;
    OffsetPageTable::new(level_4_table, VirtAddr::new(physical_memory_offset))
}

pub fn translate_addr(virt_addr: u64) -> Option<u64> {
    paging::translate_addr(VirtAddr::new(virt_addr)).map(|addr| addr.as_u64())
}

pub fn create_user_pml4() -> Option<PhysFrame> {
    let frame_allocator = unsafe { global_memory_manager_mut()? };
    let frame = frame_allocator.allocate_frame()?;
    let phys_offset = active_physical_offset();
    let new_virt = VirtAddr::new(phys_offset + frame.start_address().as_u64());
    let new_table = unsafe { &mut *(new_virt.as_mut_ptr::<PageTable>()) };
    unsafe {
        core::ptr::write_bytes(
            new_table as *mut PageTable as *mut u8,
            0,
            core::mem::size_of::<PageTable>(),
        );
    }
    let kernel_phys = unsafe {
        if KERNEL_PML4_PHYS != 0 {
            KERNEL_PML4_PHYS
        } else {
            let (level_4_frame, _) = Cr3::read();
            level_4_frame.start_address().as_u64()
        }
    };
    let kernel_virt = VirtAddr::new(phys_offset + kernel_phys);
    let kernel_table = unsafe { &*(kernel_virt.as_ptr::<PageTable>()) };
    for index in 256..512 {
        let mut entry = kernel_table[index].clone();
        let flags = entry.flags();
        entry.set_flags(flags & !PageTableFlags::USER_ACCESSIBLE);
        new_table[index] = entry;
    }
    Some(frame)
}

pub fn map_mmio(phys_addr: u64, size: usize) -> *mut u8 {
    if size == 0 {
        crate::serial_println!("[MEMORY] map_mmio size=0 phys={:#x}", phys_addr);
        return core::ptr::null_mut();
    }
    let virt_base = active_physical_offset();
    let start = VirtAddr::new(virt_base + phys_addr);
    let end = VirtAddr::new(virt_base + phys_addr + size as u64 - 1);
    let start_page = Page::<Size4KiB>::containing_address(start);
    let end_page = Page::<Size4KiB>::containing_address(end);
    enum MmioAllocator<'a> {
        Manager(&'a mut MemoryManager),
        #[cfg(not(target_os = "uefi"))]
        Mb2(&'a mut frame_allocator::Multiboot2FrameAllocator),
    }
    unsafe impl FrameAllocator<Size4KiB> for MmioAllocator<'_> {
        fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
            match self {
                MmioAllocator::Manager(manager) => manager.allocate_frame(),
                #[cfg(not(target_os = "uefi"))]
                MmioAllocator::Mb2(allocator) => allocator.allocate_frame(),
            }
        }
    }
    let mut frame_allocator = unsafe {
        if let Some(manager) = global_memory_manager_mut() {
            MmioAllocator::Manager(manager)
        } else {
            #[cfg(not(target_os = "uefi"))]
            {
                if let Some(allocator) = global_mb2_frame_allocator_mut() {
                    MmioAllocator::Mb2(allocator)
                } else {
                    crate::serial_println!(
                        "[MEMORY] map_mmio missing global allocator phys={:#x}",
                        phys_addr
                    );
                    return core::ptr::null_mut();
                }
            }
            #[cfg(target_os = "uefi")]
            {
                crate::serial_println!(
                    "[MEMORY] map_mmio missing global allocator phys={:#x}",
                    phys_addr
                );
                return core::ptr::null_mut();
            }
        }
    };
    let (level_4_frame, _) = Cr3::read();
    let phys = level_4_frame.start_address();
    let virt = VirtAddr::new(active_physical_offset() + phys.as_u64());
    let table = unsafe { &mut *(virt.as_mut_ptr()) };
    let mut mapper =
        unsafe { OffsetPageTable::new(table, VirtAddr::new(active_physical_offset())) };
    let flags = PageTableFlags::PRESENT
        | PageTableFlags::WRITABLE
        | PageTableFlags::NO_EXECUTE
        | PageTableFlags::NO_CACHE
        | PageTableFlags::WRITE_THROUGH;
    for page in Page::range_inclusive(start_page, end_page) {
        let frame =
            PhysFrame::containing_address(PhysAddr::new(page.start_address().as_u64() - virt_base));
        let map_result = paging::with_wp_disabled(|| unsafe {
            mapper.map_to(page, frame, flags, &mut frame_allocator)
        });
        match map_result {
            Ok(flush) => flush.flush(),
            Err(MapToError::PageAlreadyMapped(_)) => {
                if let Ok(flush) =
                    paging::with_wp_disabled(|| unsafe { mapper.update_flags(page, flags) })
                {
                    flush.flush();
                } else if !split_huge_page(&mut mapper, &mut frame_allocator, page, flags) {
                    crate::serial_println!(
                        "[MEMORY] map_mmio split huge page failed virt={:#x}",
                        page.start_address().as_u64()
                    );
                    return core::ptr::null_mut();
                }
            }
            Err(MapToError::ParentEntryHugePage) => {
                if !split_huge_page(&mut mapper, &mut frame_allocator, page, flags) {
                    crate::serial_println!(
                        "[MEMORY] map_mmio split huge page failed virt={:#x}",
                        page.start_address().as_u64()
                    );
                    return core::ptr::null_mut();
                }
            }
            Err(err) => {
                crate::serial_println!(
                    "[MEMORY] map_mmio map failed virt={:#x} phys={:#x} err={:?}",
                    page.start_address().as_u64(),
                    frame.start_address().as_u64(),
                    err
                );
                return core::ptr::null_mut();
            }
        }
    }
    (virt_base + phys_addr) as *mut u8
}

pub fn map_identity(phys_addr: u64, size: usize) -> bool {
    if size == 0 {
        return true;
    }
    let start = VirtAddr::new(phys_addr);
    let end = VirtAddr::new(phys_addr + size as u64 - 1);
    let start_page = Page::<Size4KiB>::containing_address(start);
    let end_page = Page::<Size4KiB>::containing_address(end);
    enum IdentityAllocator<'a> {
        Manager(&'a mut MemoryManager),
        #[cfg(not(target_os = "uefi"))]
        Mb2(&'a mut frame_allocator::Multiboot2FrameAllocator),
    }
    unsafe impl FrameAllocator<Size4KiB> for IdentityAllocator<'_> {
        fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
            match self {
                IdentityAllocator::Manager(manager) => manager.allocate_frame(),
                #[cfg(not(target_os = "uefi"))]
                IdentityAllocator::Mb2(allocator) => allocator.allocate_frame(),
            }
        }
    }
    let mut frame_allocator = unsafe {
        if let Some(manager) = global_memory_manager_mut() {
            IdentityAllocator::Manager(manager)
        } else {
            #[cfg(not(target_os = "uefi"))]
            {
                if let Some(allocator) = global_mb2_frame_allocator_mut() {
                    IdentityAllocator::Mb2(allocator)
                } else {
                    crate::serial_println!(
                        "[MEMORY] map_identity missing global allocator phys={:#x}",
                        phys_addr
                    );
                    return false;
                }
            }
            #[cfg(target_os = "uefi")]
            {
                crate::serial_println!(
                    "[MEMORY] map_identity missing global allocator phys={:#x}",
                    phys_addr
                );
                return false;
            }
        }
    };
    let (level_4_frame, _) = Cr3::read();
    let phys = level_4_frame.start_address();
    let virt = VirtAddr::new(active_physical_offset() + phys.as_u64());
    let table = unsafe { &mut *(virt.as_mut_ptr()) };
    let mut mapper =
        unsafe { OffsetPageTable::new(table, VirtAddr::new(active_physical_offset())) };
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
    for page in Page::range_inclusive(start_page, end_page) {
        let frame = PhysFrame::containing_address(PhysAddr::new(page.start_address().as_u64()));
        let map_result = paging::with_wp_disabled(|| unsafe {
            mapper.map_to(page, frame, flags, &mut frame_allocator)
        });
        match map_result {
            Ok(flush) => flush.flush(),
            Err(MapToError::PageAlreadyMapped(_)) => {
                if let Ok(flush) =
                    paging::with_wp_disabled(|| unsafe { mapper.update_flags(page, flags) })
                {
                    flush.flush();
                } else if !split_huge_page(&mut mapper, &mut frame_allocator, page, flags) {
                    crate::serial_println!(
                        "[MEMORY] map_identity split huge page failed virt={:#x}",
                        page.start_address().as_u64()
                    );
                    return false;
                }
            }
            Err(MapToError::ParentEntryHugePage) => {
                if !split_huge_page(&mut mapper, &mut frame_allocator, page, flags) {
                    crate::serial_println!(
                        "[MEMORY] map_identity split huge page failed virt={:#x}",
                        page.start_address().as_u64()
                    );
                    return false;
                }
            }
            Err(err) => {
                crate::serial_println!(
                    "[MEMORY] map_identity map failed virt={:#x} phys={:#x} err={:?}",
                    page.start_address().as_u64(),
                    frame.start_address().as_u64(),
                    err
                );
                return false;
            }
        }
    }
    true
}

fn split_huge_page(
    mapper: &mut (impl MapperAllSizes + Translate),
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    page: Page<Size4KiB>,
    flags: PageTableFlags,
) -> bool {
    let huge_page = Page::<Size2MiB>::containing_address(page.start_address());
    let unmap_result = paging::with_wp_disabled(|| mapper.unmap(huge_page));
    let (frame, flush) = match unmap_result {
        Ok(value) => value,
        Err(err) => {
            crate::serial_println!(
                "[MEMORY] split_huge_page unmap failed virt={:#x} err={:?}",
                huge_page.start_address().as_u64(),
                err
            );
            return false;
        }
    };
    flush.flush();
    let base = frame.start_address().as_u64();
    let virt_base = huge_page.start_address().as_u64();
    for i in 0..512u64 {
        let virt = VirtAddr::new(virt_base + i * 4096);
        let phys = PhysAddr::new(base + i * 4096);
        let small_page: Page<Size4KiB> = Page::containing_address(virt);
        let small_frame: PhysFrame<Size4KiB> = PhysFrame::containing_address(phys);
        let map_result = paging::with_wp_disabled(|| unsafe {
            mapper.map_to(small_page, small_frame, flags, frame_allocator)
        });
        match map_result {
            Ok(flush) => flush.flush(),
            Err(MapToError::PageAlreadyMapped(_)) => {
                if !paging::verify_idempotent_mapping(mapper, small_page, small_frame) {
                    crate::serial_println!(
                        "[MEMORY] split_huge_page idempotent mismatch virt={:#x}",
                        small_page.start_address().as_u64()
                    );
                    return false;
                }
            }
            Err(err) => {
                crate::serial_println!(
                    "[MEMORY] split_huge_page map failed virt={:#x} phys={:#x} err={:?}",
                    small_page.start_address().as_u64(),
                    small_frame.start_address().as_u64(),
                    err
                );
                return false;
            }
        }
    }
    true
}

pub fn ensure_identity_mapped(phys_addr: usize, size: usize) -> Result<(), MapToError<Size4KiB>> {
    if size == 0 {
        return Ok(());
    }
    let base = active_physical_offset();
    let start = VirtAddr::new(base + phys_addr as u64);
    let end = VirtAddr::new(base + phys_addr as u64 + size as u64 - 1);
    let start_page = Page::containing_address(start);
    let end_page = Page::containing_address(end);
    let frame_allocator =
        unsafe { global_memory_manager_mut().ok_or(MapToError::FrameAllocationFailed)? };
    let (level_4_frame, _) = Cr3::read();
    let phys = level_4_frame.start_address();
    let virt = VirtAddr::new(base + phys.as_u64());
    let table = unsafe { &mut *(virt.as_mut_ptr()) };
    let mut mapper = unsafe { OffsetPageTable::new(table, VirtAddr::new(base)) };
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;
    for page in Page::range_inclusive(start_page, end_page) {
        if let Some(current) = mapper.translate_addr(page.start_address()) {
            let expected = PhysAddr::new(page.start_address().as_u64() - base);
            if current != expected {
                crate::serial_println!(
                    "[MEMORY] ensure_identity_mapped mismatch virt={:#x} current={:#x} expected={:#x}",
                    page.start_address().as_u64(),
                    current.as_u64(),
                    expected.as_u64()
                );
                return Err(MapToError::PageAlreadyMapped(
                    PhysFrame::containing_address(current),
                ));
            }
            continue;
        }
        let frame =
            PhysFrame::containing_address(PhysAddr::new(page.start_address().as_u64() - base));
        let map_result = paging::with_wp_disabled(|| unsafe {
            mapper.map_to(page, frame, flags, frame_allocator)
        });
        match map_result {
            Ok(flush) => flush.flush(),
            Err(MapToError::ParentEntryHugePage) => {}
            Err(MapToError::PageAlreadyMapped(_)) => {}
            Err(e) => {
                crate::serial_println!(
                    "[MEMORY] ensure_identity_mapped map failed virt={:#x} phys={:#x} err={:?}",
                    page.start_address().as_u64(),
                    frame.start_address().as_u64(),
                    e
                );
                return Err(e);
            }
        }
    }
    Ok(())
}

#[cfg(not(target_os = "uefi"))]
pub enum VmmInitError {
    MissingMemoryMap,
    InvalidPml4Alignment,
    Map4K(MapToError<Size4KiB>),
    Map2M(MapToError<Size2MiB>),
    UpdateFlags(FlagUpdateError),
}

#[cfg(not(target_os = "uefi"))]
pub unsafe fn init(
    boot_info: &BootInformation,
    pml4_phys: PhysAddr,
    kaslr_offset: u64,
) -> Result<OffsetPageTable<'static>, VmmInitError> {
    let memory_map = match boot_info.memory_map_tag() {
        Some(tag) => tag,
        None => {
            crate::serial_println!("[MEMORY] Missing multiboot memory map tag");
            return Err(VmmInitError::MissingMemoryMap);
        }
    };
    let mut frame_allocator =
        match frame_allocator::Multiboot2FrameAllocator::new(boot_info, kaslr_offset) {
            Some(allocator) => allocator,
            None => {
                crate::serial_println!("[MEMORY] Frame allocator init failed");
                return Err(VmmInitError::MissingMemoryMap);
            }
        };
    let total_mb = frame_allocator.total_usable_bytes() / (1024 * 1024);
    crate::serial_println!("[MEMORY] Total usable RAM detected: {} MB", total_mb);
    crate::serial_println!(
        "[MEMORY] HHDM initialized at offset: {:#x}",
        PHYSICAL_MEMORY_OFFSET
    );

    if (pml4_phys.as_u64() & 0xFFF) != 0 {
        crate::serial_println!("[MEMORY] Invalid PML4 alignment: {:#x}", pml4_phys.as_u64());
        return Err(VmmInitError::InvalidPml4Alignment);
    }
    let pml4_frame = PhysFrame::containing_address(pml4_phys);
    KERNEL_PML4_FRAME = Some(pml4_frame);

    let pml4_virt = VirtAddr::new(PHYSICAL_MEMORY_OFFSET + pml4_phys.as_u64());
    let pml4_ptr: *mut PageTable = pml4_virt.as_mut_ptr();
    let pml4_table = &mut *pml4_ptr;
    let mut mapper = OffsetPageTable::new(pml4_table, VirtAddr::new(PHYSICAL_MEMORY_OFFSET));

    let hhdm_flags =
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;
    for area in memory_map.memory_areas() {
        let typ = area.typ();
        if !matches!(
            typ,
            multiboot2::MemoryAreaType::Available | multiboot2::MemoryAreaType::AcpiAvailable
        ) {
            continue;
        }
        let start = area.start_address();
        let end = area.end_address();
        map_hhdm_range(&mut mapper, &mut frame_allocator, start, end, hhdm_flags)?;
    }

    let mut cr0 = Cr0::read();
    cr0.insert(Cr0Flags::WRITE_PROTECT);
    Cr0::write(cr0);

    if let Some(elf_sections) = boot_info.elf_sections_tag() {
        let base_flags =
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;
        for section in elf_sections.sections() {
            if !section.is_allocated() {
                continue;
            }
            let name = section.name();
            let flags = match name {
                ".text" => PageTableFlags::PRESENT,
                ".rodata" => PageTableFlags::PRESENT | PageTableFlags::NO_EXECUTE,
                ".data" | ".bss" => {
                    PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE
                }
                _ => base_flags,
            };
            let start = section.start_address().saturating_add(kaslr_offset);
            let end = section.end_address().saturating_add(kaslr_offset);
            if end <= start {
                continue;
            }
            let start_page = Page::containing_address(VirtAddr::new(start));
            let end_page = Page::containing_address(VirtAddr::new(end - 1));
            for page in Page::range_inclusive(start_page, end_page) {
                let update_result =
                    paging::with_wp_disabled(|| unsafe { mapper.update_flags(page, flags) });
                match update_result {
                    Ok(flush) => flush.flush(),
                    Err(FlagUpdateError::ParentEntryHugePage) => {
                        if !split_huge_page(&mut mapper, &mut frame_allocator, page, base_flags) {
                            crate::serial_println!(
                                "[MEMORY] Split huge page failed for section {:?} at {:#x}",
                                name,
                                page.start_address().as_u64()
                            );
                            return Err(VmmInitError::UpdateFlags(
                                FlagUpdateError::ParentEntryHugePage,
                            ));
                        }
                        let update_result = paging::with_wp_disabled(|| unsafe {
                            mapper.update_flags(page, flags)
                        });
                        match update_result {
                            Ok(flush) => flush.flush(),
                            Err(err) => {
                                crate::serial_println!(
                                    "[MEMORY] Update flags failed for section {:?} at {:#x}: {:?}",
                                    name,
                                    page.start_address().as_u64(),
                                    err
                                );
                                return Err(VmmInitError::UpdateFlags(err));
                            }
                        }
                    }
                    Err(FlagUpdateError::PageNotMapped) => {}
                }
            }
        }
    }

    Ok(mapper)
}

#[cfg(not(target_os = "uefi"))]
fn map_hhdm_range(
    mapper: &mut (impl MapperAllSizes + Translate),
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    start: u64,
    end: u64,
    flags: PageTableFlags,
) -> Result<(), VmmInitError> {
    let mut current = start;
    let huge_size = Size2MiB::SIZE;
    let page_size = Size4KiB::SIZE;

    while current < end && (current % huge_size) != 0 {
        let virt = VirtAddr::new(PHYSICAL_MEMORY_OFFSET + current);
        let phys = PhysAddr::new(current);
        let map_result: Result<MapperFlush<Size4KiB>, MapToError<Size4KiB>> =
            paging::map_page(mapper, frame_allocator, virt, phys, flags);
        match map_result {
            Ok(flush) => {
                let flush: MapperFlush<Size4KiB> = flush;
                flush.flush();
            }
            Err(MapToError::ParentEntryHugePage) => {
                let page = Page::containing_address(virt);
                if !split_huge_page(mapper, frame_allocator, page, flags) {
                    crate::serial_println!(
                        "[MEMORY] Split huge page failed at HHDM {:#x}",
                        virt.as_u64()
                    );
                    return Err(VmmInitError::Map4K(MapToError::ParentEntryHugePage));
                }
            }
            Err(MapToError::PageAlreadyMapped(_)) => {}
            Err(err) => {
                crate::serial_println!(
                    "[MEMORY] Map4K failed at HHDM {:#x}: {:?}",
                    virt.as_u64(),
                    err
                );
                return Err(VmmInitError::Map4K(err));
            }
        }
        current = current.saturating_add(page_size);
    }

    while current + huge_size <= end {
        let virt = VirtAddr::new(PHYSICAL_MEMORY_OFFSET + current);
        let phys = PhysAddr::new(current);
        let page = Page::<Size2MiB>::containing_address(virt);
        let frame = PhysFrame::<Size2MiB>::containing_address(phys);
        let map_result =
            unsafe { mapper.map_to(page, frame, flags, frame_allocator) }.map_err(|err| {
                crate::serial_println!(
                    "[MEMORY] Map2M failed at HHDM {:#x}: {:?}",
                    virt.as_u64(),
                    err
                );
                VmmInitError::Map2M(err)
            })?;
        map_result.flush();
        current = current.saturating_add(huge_size);
    }

    while current < end {
        let virt = VirtAddr::new(PHYSICAL_MEMORY_OFFSET + current);
        let phys = PhysAddr::new(current);
        let map_result: Result<MapperFlush<Size4KiB>, MapToError<Size4KiB>> =
            paging::map_page(mapper, frame_allocator, virt, phys, flags);
        match map_result {
            Ok(flush) => {
                let flush: MapperFlush<Size4KiB> = flush;
                flush.flush();
            }
            Err(MapToError::ParentEntryHugePage) => {
                let page = Page::containing_address(virt);
                if !split_huge_page(mapper, frame_allocator, page, flags) {
                    crate::serial_println!(
                        "[MEMORY] Split huge page failed at HHDM {:#x}",
                        virt.as_u64()
                    );
                    return Err(VmmInitError::Map4K(MapToError::ParentEntryHugePage));
                }
            }
            Err(MapToError::PageAlreadyMapped(_)) => {}
            Err(err) => {
                crate::serial_println!(
                    "[MEMORY] Map4K failed at HHDM {:#x}: {:?}",
                    virt.as_u64(),
                    err
                );
                return Err(VmmInitError::Map4K(err));
            }
        }
        current = current.saturating_add(page_size);
    }

    Ok(())
}

#[cfg(target_os = "uefi")]
#[derive(Debug)]
pub enum UefiHhdmError {
    Map4K(MapToError<Size4KiB>),
    Map2M(MapToError<Size2MiB>),
}

/// HHDM init sırasında identity-mapped bölgeden (< 4GB) frame tahsis eden wrapper.
/// OffsetPageTable offset=0 ile çalışırken, UEFI firmware yalnızca < 4GB'ı identity-map eder.
/// Page table frame'leri bu bölgeden alınmalıdır, aksi takdirde page fault oluşur.
#[cfg(target_os = "uefi")]
struct Dma32FrameAllocator<'a> {
    inner: &'a mut MemoryManager,
}

#[cfg(target_os = "uefi")]
unsafe impl FrameAllocator<Size4KiB> for Dma32FrameAllocator<'_> {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        self.inner
            .pmm
            .allocate_from_zone(fibonacci_pmm::MemoryZone::Dma32)
    }
}

#[cfg(target_os = "uefi")]
pub fn init_uefi_hhdm(
    mapper: &mut (impl MapperAllSizes + Translate),
    frame_allocator: &mut MemoryManager,
    hhdm_offset: u64,
) -> Result<(), UefiHhdmError> {
    use uefi::table::boot::{MemoryAttribute, MemoryType};
    let old_cr0 = Cr0::read();
    if old_cr0.contains(Cr0Flags::WRITE_PROTECT) {
        let mut cr0 = old_cr0;
        cr0.remove(Cr0Flags::WRITE_PROTECT);
        unsafe {
            Cr0::write(cr0);
        }
    }
    let descriptors: Vec<MemoryDescriptor> = frame_allocator.get_memory_map().map(|d| *d).collect();
    let mut map_count: usize = 0;
    for desc in descriptors.iter() {
        let ty = desc.ty;
        let is_runtime = desc.att.contains(MemoryAttribute::RUNTIME);
        let should_map = matches!(
            ty,
            MemoryType::CONVENTIONAL
                | MemoryType::LOADER_CODE
                | MemoryType::LOADER_DATA
                | MemoryType::BOOT_SERVICES_CODE
                | MemoryType::BOOT_SERVICES_DATA
                | MemoryType::ACPI_RECLAIM
                | MemoryType::ACPI_NON_VOLATILE
                | MemoryType::RUNTIME_SERVICES_CODE
                | MemoryType::RUNTIME_SERVICES_DATA
        ) || is_runtime;
        if !should_map {
            continue;
        }
        let mut flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        let exec_allowed = matches!(
            ty,
            MemoryType::RUNTIME_SERVICES_CODE
                | MemoryType::BOOT_SERVICES_CODE
                | MemoryType::LOADER_CODE
        );
        if !exec_allowed {
            flags |= PageTableFlags::NO_EXECUTE;
        }
        let start = desc.phys_start;
        let size = desc.page_count.saturating_mul(Size4KiB::SIZE);
        if size == 0 {
            continue;
        }
        let end = start.saturating_add(size);
        {
            let mut dma32_alloc = Dma32FrameAllocator {
                inner: frame_allocator,
            };
            map_hhdm_range_uefi(mapper, &mut dma32_alloc, start, end, hhdm_offset, flags)?;
        }
        map_count += 1;
    }
    crate::serial_println!("[HHDM] {} regions + device MMIO mapped", map_count);

    // PCI MMIO hole (DF800000-100000000) bölgesindeki cihaz MMIO alanlarını HHDM'e ekle.
    // UEFI memory map bu bölgeyi içermez ama Local APIC, IOAPIC, HPET gibi cihazlar
    // bu aralıkta MMIO register'lara sahiptir. HHDM offset aktif olduktan sonra
    // tüm fiziksel erişimler hhdm_offset + phys üzerinden yapılacağından
    // bu bölgelerin de haritalanması zorunludur.
    let device_mmio_regions: [(u64, u64, &str); 3] = [
        (0xFEC0_0000, 0x1000, "IOAPIC"), // I/O APIC
        (0xFED0_0000, 0x1000, "HPET"),   // HPET Timer
        (0xFEE0_0000, 0x1000, "LAPIC"),  // Local APIC MMIO
    ];
    let mmio_flags = PageTableFlags::PRESENT
        | PageTableFlags::WRITABLE
        | PageTableFlags::NO_EXECUTE
        | PageTableFlags::NO_CACHE
        | PageTableFlags::WRITE_THROUGH;
    for (phys_base, size, name) in &device_mmio_regions {
        let mut dma32_alloc = Dma32FrameAllocator {
            inner: frame_allocator,
        };
        let mut cur = *phys_base;
        let region_end = phys_base + size;
        while cur < region_end {
            let virt = VirtAddr::new(hhdm_offset + cur);
            let phys = PhysAddr::new(cur);
            let page = Page::<Size4KiB>::containing_address(virt);
            let frame = PhysFrame::<Size4KiB>::containing_address(phys);
            match unsafe { mapper.map_to(page, frame, mmio_flags, &mut dma32_alloc) } {
                Ok(flush) => flush.flush(),
                Err(MapToError::PageAlreadyMapped(_)) => {}
                Err(MapToError::ParentEntryHugePage) => {
                    // Huge page varsa split et
                    let p = Page::<Size4KiB>::containing_address(virt);
                    if !split_huge_page(mapper, &mut dma32_alloc, p, mmio_flags) {
                        crate::serial_println!("[HHDM] WARN: {} split failed at {:#x}", name, cur);
                    }
                }
                Err(e) => {
                    crate::serial_println!(
                        "[HHDM] WARN: {} map failed at {:#x}: {:?}",
                        name,
                        cur,
                        e
                    );
                }
            }
            cur += Size4KiB::SIZE;
        }
    }

    if old_cr0.contains(Cr0Flags::WRITE_PROTECT) {
        unsafe {
            Cr0::write(old_cr0);
        }
    }
    Ok(())
}

#[cfg(target_os = "uefi")]
fn map_hhdm_range_uefi<A>(
    mapper: &mut (impl MapperAllSizes + Translate),
    frame_allocator: &mut A,
    start: u64,
    end: u64,
    hhdm_offset: u64,
    flags: PageTableFlags,
) -> Result<(), UefiHhdmError>
where
    A: FrameAllocator<Size4KiB>,
{
    let mut current = start;
    let huge_size = Size2MiB::SIZE;
    let page_size = Size4KiB::SIZE;

    while current < end && (current % huge_size) != 0 {
        let virt = VirtAddr::new(hhdm_offset + current);
        let phys = PhysAddr::new(current);
        let map_result: Result<MapperFlush<Size4KiB>, MapToError<Size4KiB>> =
            paging::map_page(mapper, frame_allocator, virt, phys, flags);
        match map_result {
            Ok(flush) => flush.flush(),
            Err(MapToError::ParentEntryHugePage) => {
                let page = Page::containing_address(virt);
                if !split_huge_page(mapper, frame_allocator, page, flags) {
                    return Err(UefiHhdmError::Map4K(MapToError::ParentEntryHugePage));
                }
            }
            Err(MapToError::PageAlreadyMapped(_)) => {}
            Err(err) => return Err(UefiHhdmError::Map4K(err)),
        }
        current = current.saturating_add(page_size);
    }

    while current + huge_size <= end {
        let virt = VirtAddr::new(hhdm_offset + current);
        let phys = PhysAddr::new(current);
        let page = Page::<Size2MiB>::containing_address(virt);
        let frame = PhysFrame::<Size2MiB>::containing_address(phys);
        let map_result = unsafe { mapper.map_to(page, frame, flags, frame_allocator) }
            .map_err(UefiHhdmError::Map2M)?;
        map_result.flush();
        current = current.saturating_add(huge_size);
    }

    while current < end {
        let virt = VirtAddr::new(hhdm_offset + current);
        let phys = PhysAddr::new(current);
        let map_result: Result<MapperFlush<Size4KiB>, MapToError<Size4KiB>> =
            paging::map_page(mapper, frame_allocator, virt, phys, flags);
        match map_result {
            Ok(flush) => flush.flush(),
            Err(MapToError::ParentEntryHugePage) => {
                let page = Page::containing_address(virt);
                if !split_huge_page(mapper, frame_allocator, page, flags) {
                    return Err(UefiHhdmError::Map4K(MapToError::ParentEntryHugePage));
                }
            }
            Err(MapToError::PageAlreadyMapped(_)) => {}
            Err(err) => return Err(UefiHhdmError::Map4K(err)),
        }
        current = current.saturating_add(page_size);
    }

    Ok(())
}

#[cfg(target_os = "uefi")]
pub fn set_uefi_virtual_address_map(
    system_table_ptr: u64,
    memory_manager: &mut MemoryManager,
    hhdm_offset: u64,
) -> Result<usize, uefi::Status> {
    use uefi::table::boot::MemoryAttribute;
    use uefi::table::{Runtime, SystemTable};
    let system_table =
        unsafe { SystemTable::<Runtime>::from_ptr(system_table_ptr as *mut core::ffi::c_void) }
            .ok_or(uefi::Status::INVALID_PARAMETER)?;
    let mut descriptors: Vec<MemoryDescriptor> =
        memory_manager.get_memory_map().map(|d| *d).collect();
    for desc in descriptors.iter_mut() {
        if desc.att.contains(MemoryAttribute::RUNTIME) {
            desc.virt_start = desc.phys_start.saturating_add(hhdm_offset);
        }
    }
    let new_system_table_virtual_addr = system_table_ptr.saturating_add(hhdm_offset);
    let new_system_table = unsafe {
        system_table.set_virtual_address_map(&mut descriptors, new_system_table_virtual_addr)
    }
    .map_err(|err| err.status())?;
    let runtime_services = unsafe { new_system_table.runtime_services() } as *const _ as usize;
    Ok(runtime_services)
}
