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
//! │  ShardedLru (16 shard): active/inactive listeleri                    │
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
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, AtomicU64, AtomicUsize, Ordering};
use vma::{Vma, VmaKind, VmaMap};
use lazy_static::lazy_static;
#[cfg(not(target_os = "uefi"))]
use multiboot2::BootInformation;
use rcore_fs::vfs::INode;
use spin::{Mutex, RwLock};
use uefi::table::boot::{MemoryAttribute, MemoryDescriptor, MemoryMap, MemoryMapIter, MemoryType};
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
#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
use crate::boot::context::{MemoryRegionKind, NormalizedMemoryMap};

/// cgroups v2 bellek denetleyicisi — limit, soft limit, swap limit
pub mod cgroup;
pub mod compaction;
pub use compaction::compact_contiguous;
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
pub mod migration;
/// Multi-Gen LRU (MGLRU) — sıcak/soğuk nesil tabanlı reclaim sinyali
pub mod mglru;
pub mod oom;
pub mod paging;
pub mod rmap;
pub mod pmm;
/// Pressure Stall Information (PSI) — bellek baskısı telemetrisi
pub mod psi;
pub mod shared_region;
/// Şeffaf büyük sayfa (Transparent Huge Pages) — 4K→2M collapse/split
pub mod thp;
/// Bellek sıkıştırma ve swap: ZSwap/ZRam, LZ4/ZSTD
pub mod folio;
pub mod frame_ownership;
/// Per-CPU order-0 frame cache — batch refill/drain, lock-free fast path.
pub(crate) mod per_cpu_frame_cache;
pub mod vma;
pub(crate) mod cils;
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
    let active_pages = LRU.active_pages();
    let inactive_pages = LRU.inactive_pages();

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

/// MemoryManager'ın bellek haritası kaynağı.
enum MemoryManagerMap<'a> {
    /// UEFI BootServices memory map (UEFI boot yolu)
    Uefi(MemoryMap<'a>),
    /// Multiboot2 bellek haritasından üretilen statik descriptor dilimi (BIOS yolu)
    Multiboot2(&'a [MemoryDescriptor]),
}

/// Ana bellek yöneticisi.
/// UEFI veya Multiboot2 bellek haritası ve PMM kullanır.
pub struct MemoryManager {
    /// Boot kaynağına göre bellek haritası
    memory_map: MemoryManagerMap<'static>,
    /// Fiziksel bellek yöneticisi (Fibonacci tabanlı)
    pmm: fibonacci_pmm::FibonacciPmm,
}

impl MemoryManager {
    /// Yeni bir MemoryManager oluşturur (UEFI yolu).
    ///
    /// # Parametreler
    /// - `memory_map`: UEFI'den alınan bellek haritası
    pub fn new(memory_map: MemoryMap<'static>) -> Self {
        let mut pmm = fibonacci_pmm::FibonacciPmm::empty();
        unsafe {
            pmm.init(memory_map.entries());
        }

        MemoryManager {
            memory_map: MemoryManagerMap::Uefi(memory_map),
            pmm,
        }
    }

    /// Yeni bir MemoryManager oluşturur (Multiboot2 yolu).
    ///
    /// PMM, verilen statik descriptor diliminden beslenir; dilim
    /// `init_multiboot2` tarafından MB2 bellek haritasından üretilir.
    pub fn from_multiboot2(descriptors: &'static [MemoryDescriptor]) -> Self {
        let mut pmm = fibonacci_pmm::FibonacciPmm::empty();
        unsafe {
            pmm.init(descriptors.iter());
        }

        MemoryManager {
            memory_map: MemoryManagerMap::Multiboot2(descriptors),
            pmm,
        }
    }

    /// UEFI bellek haritası üzerinde iterator döndürür.
    #[cfg(target_os = "uefi")]
    #[allow(dead_code)]
    pub fn get_memory_map(&self) -> MemoryMapIter<'_> {
        match &self.memory_map {
            MemoryManagerMap::Uefi(map) => map.entries(),
            MemoryManagerMap::Multiboot2(_) => {
                unreachable!("MB2 memory map is not available on the UEFI target")
            }
        }
    }

    pub fn allocate_contiguous_frames(&mut self, pages: usize) -> Option<PhysFrame> {
        if pages == 1 {
            // Fast path: try per-CPU cache first.
            if let Some(frame) = per_cpu_frame_cache::try_alloc() {
                return Some(frame);
            }
            // Cache empty — batch refill from PMM, then serve.
            per_cpu_frame_cache::refill(&mut self.pmm);
            let r = per_cpu_frame_cache::try_alloc();
            if r.is_none() {
                crate::serial_println!(
                    "[ACF-FAIL] pages=1 pmm total={} free={}",
                    self.pmm.total_frames(),
                    self.pmm.free_frames()
                );
            }
            return r;
        }
        // Huge-page / multi-page — bypass cache, go directly to PMM.
        self.pmm.allocate_contiguous(pages)
    }

    pub fn deallocate_contiguous_frames(&mut self, start: PhysFrame, pages: usize) {
        if pages == 1 {
            // Fast path: try per-CPU cache first.
            let addr = start.start_address().as_u64();
            if per_cpu_frame_cache::try_free(addr) {
                // Cgroup uncharge still applies.
                let pid = crate::task::scheduler::current_task_id() as u64;
                if let Some(cg_id) = cgroup::CGROUP_MANAGER.get_cgroup_for_process(pid) {
                    if let Some(cg) = cgroup::CGROUP_MANAGER.get_cgroup(cg_id) {
                        cg.uncharge(PAGE_SIZE as u64);
                    }
                }
                return;
            }
            // Cache full — drain half to PMM, then retry push.
            per_cpu_frame_cache::drain(&mut self.pmm);
            let _ = per_cpu_frame_cache::try_free(addr);
            // Cgroup uncharge (cache async, but visual correctness).
            let pid = crate::task::scheduler::current_task_id() as u64;
            if let Some(cg_id) = cgroup::CGROUP_MANAGER.get_cgroup_for_process(pid) {
                if let Some(cg) = cgroup::CGROUP_MANAGER.get_cgroup(cg_id) {
                    cg.uncharge(PAGE_SIZE as u64);
                }
            }
            return;
        }
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

    pub fn allocate_user_frame(&mut self) -> Option<crate::memory::frame_ownership::UniqueFrame> {
        crate::memory::frame_ownership::UniqueFrame::from_phys_alloc(
            self.allocate_frame_with_context(FrameAllocationContext::UserFault),
        )
    }

    pub fn allocate_kernel_frame(&mut self) -> Option<PhysFrame> {
        self.allocate_frame_with_context(FrameAllocationContext::KernelCritical)
    }

    fn allocate_frame_with_context(
        &mut self,
        context: FrameAllocationContext,
    ) -> Option<PhysFrame> {
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
        // Heap bootstrap maps 25,600 pages.  Emitting two serial records for
        // every frame turns the bounded map into an I/O-bound boot loop under
        // QEMU/TCG.  Keep the first few records for diagnostics and then take
        // a bounded periodic sample; allocation semantics remain unchanged.
        let trace_allocation = {
            let sample = AFC_TRACE_COUNT.fetch_add(1, Ordering::Relaxed);
            sample < AFC_TRACE_WARMUP || (sample & (AFC_TRACE_INTERVAL - 1)) == 0
        };
        if trace_allocation {
            crate::serial_println!("[AFC] allocate_frame_with_context calling pmm");
        }
        if let Some(frame) = self.pmm.allocate_frame() {
            self.account_allocated_frame(hooks_ready, context);
            if hooks_ready {
                let now = crate::task::scheduler::get_ticks() as u64;
                let elapsed = now.saturating_sub(stall_start);
                if elapsed > 0 {
                    psi::record_memory_stall(0, elapsed, false);
                }
            }
            if trace_allocation {
                crate::serial_println!(
                    "[AFC] pmm returned frame {:#x}",
                    frame.start_address().as_u64()
                );
            }
            return Some(frame);
        }
        crate::serial_println!("[AFC] pmm returned None");
        // Geri kazanım denemesi
        if hooks_ready {
            psi::record_memory_stall(1, 1, false);
        }
        if hooks_ready && reclaim_pages(16) > 0 {
            if let Some(frame) = self.pmm.allocate_frame() {
                self.account_allocated_frame(hooks_ready, context);
                let now = crate::task::scheduler::get_ticks() as u64;
                let elapsed = now.saturating_sub(stall_start).max(1);
                psi::record_memory_stall(elapsed.min(4), elapsed, false);
                return Some(frame);
            }
        }

        // OOM Killer yalnızca user-atfedilebilir tahsislerde devreye girer.
        if hooks_ready
            && context.allows_oom_kill()
            && oom::should_trigger_oom(self.free_frames(), self.total_frames())
        {
            crate::serial_println!(
                "[MEM] OOM triggered for {:?} allocation - free: {} / total: {}",
                context,
                self.free_frames(),
                self.total_frames()
            );

            // ZSwap writeback dene — OOM öncesi son kurtarma
            let _ = zswap::ZSWAP_MANAGER.writeback_lru();
            if let Some(frame) = self.pmm.allocate_frame() {
                self.account_allocated_frame(hooks_ready, context);
                let now = crate::task::scheduler::get_ticks() as u64;
                let elapsed = now.saturating_sub(stall_start).max(1);
                psi::record_memory_stall(elapsed.min(8), elapsed, false);
                return Some(frame);
            }
            let now = crate::task::scheduler::get_ticks() as u64;
            let elapsed = now.saturating_sub(stall_start).max(1);
            psi::record_memory_stall(elapsed, elapsed, true);

            let tasks = crate::task::scheduler::list_tasks();
            let oom_infos: alloc::vec::Vec<oom::OomProcessInfo> =
                tasks.iter().map(oom::process_info_from_task).collect();
            if let Some(estimated_freed) = oom::oom_kill(&oom_infos) {
                let reclaim_target = estimated_freed.clamp(16, 256);
                let reclaimed = reclaim_pages_global(reclaim_target);
                process_writeback_budget(WRITEBACK_BUDGET_FAST);
                crate::serial_println!(
                    "[MEM] OOM post-kill reclaim target={} reclaimed={}",
                    reclaim_target,
                    reclaimed
                );
                if let Some(frame) = self.pmm.allocate_frame() {
                    self.account_allocated_frame(hooks_ready, context);
                    let now = crate::task::scheduler::get_ticks() as u64;
                    let elapsed = now.saturating_sub(stall_start).max(1);
                    psi::record_memory_stall(elapsed, elapsed, true);
                    return Some(frame);
                }
            }
        }

        None
    }

    fn account_allocated_frame(&self, hooks_ready: bool, context: FrameAllocationContext) {
        if !hooks_ready || !context.charges_current_cgroup() {
            return;
        }
        let pid = crate::task::scheduler::current_task_id() as u64;
        if let Some(cg_id) = cgroup::CGROUP_MANAGER.get_cgroup_for_process(pid) {
            if let Some(cg) = cgroup::CGROUP_MANAGER.get_cgroup(cg_id) {
                let _ = cg.charge(PAGE_SIZE as u64);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameAllocationContext {
    KernelCritical,
    UserFault,
}

impl FrameAllocationContext {
    #[inline]
    const fn allows_oom_kill(self) -> bool {
        matches!(self, Self::UserFault)
    }

    #[inline]
    const fn charges_current_cgroup(self) -> bool {
        matches!(self, Self::UserFault)
    }
}

/// x86_64 FrameAllocator trait implementasyonu.
/// Scheduler ve paging sistemi için gerekli.
unsafe impl FrameAllocator<Size4KiB> for MemoryManager {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        self.allocate_kernel_frame()
    }
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// Bellek yöneticisini başlatır.
pub fn init_uefi(memory_map: MemoryMap<'static>) -> MemoryManager {
    MemoryManager::new(memory_map)
}

/// Multiboot2 bellek haritasından PMM descriptor'ları üreten maksimum alan sayısı.
#[cfg(target_os = "none")]
const MB2_MAX_MEMORY_DESCRIPTORS: usize = 192;

#[cfg(target_os = "none")]
const EMPTY_MB2_MEMORY_DESCRIPTOR: MemoryDescriptor = MemoryDescriptor {
    ty: MemoryType::RESERVED,
    phys_start: 0,
    virt_start: 0,
    page_count: 0,
    att: MemoryAttribute::empty(),
};

#[cfg(target_os = "none")]
static mut MB2_MEMORY_DESCRIPTORS: [MemoryDescriptor; MB2_MAX_MEMORY_DESCRIPTORS] =
    [EMPTY_MB2_MEMORY_DESCRIPTOR; MB2_MAX_MEMORY_DESCRIPTORS];

#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
const CANONICAL_MEMORY_DESCRIPTORS: usize = 256;

#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
static mut NORMALIZED_MEMORY_DESCRIPTORS: [MemoryDescriptor; CANONICAL_MEMORY_DESCRIPTORS] =
    [EMPTY_MB2_MEMORY_DESCRIPTOR; CANONICAL_MEMORY_DESCRIPTORS];

#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
fn init_from_normalized_map(
    map: &NormalizedMemoryMap,
    kernel_start_phys: u64,
    kernel_end_phys: u64,
) -> MemoryManager {
    let page_mask = PAGE_SIZE as u64 - 1;
    let kstart = kernel_start_phys & !page_mask;
    let kend = kernel_end_phys.saturating_add(page_mask) & !page_mask;
    let mut count = 0usize;
    let mut push_descriptor = |start: u64, end: u64| {
        if count >= CANONICAL_MEMORY_DESCRIPTORS {
            return false;
        }
        let start = (start + page_mask) & !page_mask;
        let end = end & !page_mask;
        let pages = end.saturating_sub(start) / PAGE_SIZE as u64;
        if pages == 0 {
            return true;
        }
        unsafe {
            NORMALIZED_MEMORY_DESCRIPTORS[count] = MemoryDescriptor {
                ty: MemoryType::CONVENTIONAL,
                phys_start: start,
                virt_start: start,
                page_count: pages,
                att: MemoryAttribute::empty(),
            };
        }
        crate::serial_println!(
            "[MEM] canonical descriptor[{}] base={:#x} pages={:#x}",
            count,
            start,
            pages
        );
        count += 1;
        true
    };

    for region in map.as_slice() {
        if !matches!(
            region.kind,
            MemoryRegionKind::Usable
                | MemoryRegionKind::ACPIReclaim
                | MemoryRegionKind::BootloaderReclaimable
        ) {
            continue;
        }
        let start = region.base;
        let end = region.base.saturating_add(region.len);
        if end <= kstart || start >= kend {
            if !push_descriptor(start, end) {
                break;
            }
        } else {
            if start < kstart && !push_descriptor(start, kstart) {
                break;
            }
            if end > kend && !push_descriptor(kend, end) {
                break;
            }
        }
    }

    let descriptors = unsafe {
        core::slice::from_raw_parts(NORMALIZED_MEMORY_DESCRIPTORS.as_ptr(), count)
    };
    crate::serial_println!(
        "[MEM] canonical PMM descriptors: {} (kernel {:#x}-{:#x} excluded)",
        count,
        kstart,
        kend
    );
    MemoryManager::from_multiboot2(descriptors)
}

#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
pub fn init_limine_normalized(map: &NormalizedMemoryMap, physical_base: u64) -> MemoryManager {
    extern "C" {
        static kernel_phys_start: u8;
        static kernel_phys_end: u8;
    }
    let link_start = unsafe { &kernel_phys_start as *const u8 as u64 };
    let link_end = unsafe { &kernel_phys_end as *const u8 as u64 };
    let span = link_end.wrapping_sub(link_start);
    init_from_normalized_map(map, physical_base, physical_base.saturating_add(span))
}

#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
pub fn init_multiboot2_normalized(
    map: &NormalizedMemoryMap,
    kaslr_offset: u64,
) -> MemoryManager {
    extern "C" {
        static kernel_phys_start: u8;
        static kernel_phys_end: u8;
    }
    let start = unsafe { (&kernel_phys_start as *const u8 as u64).wrapping_add(kaslr_offset) };
    let end = unsafe { (&kernel_phys_end as *const u8 as u64).wrapping_add(kaslr_offset) };
    init_from_normalized_map(map, start, end)
}

/// Multiboot2 yolu için bellek yöneticisini başlatır.
///
/// MB2 bellek haritası tag'indeki Available alanlarını UEFI
/// `MemoryDescriptor`'larına çevirir; kernel görüntü aralığını (linker
/// `kernel_phys_start/end` sembolleri) bölerek PMM'den ayırır ve statik
/// descriptor havuzunda saklar. Yalnızca BIOS (none) hedefinde çalışır;
/// UEFI yolu `init_uefi` kullanır.
#[cfg(target_os = "none")]
pub fn init_multiboot2(boot_info: &BootInformation, kaslr_offset: u64) -> MemoryManager {
    extern "C" {
        static kernel_phys_start: u8;
        static kernel_phys_end: u8;
    }

    let (kernel_start_phys, kernel_end_phys) = unsafe {
        (
            (&kernel_phys_start as *const u8 as u64).wrapping_add(kaslr_offset),
            (&kernel_phys_end as *const u8 as u64).wrapping_add(kaslr_offset),
        )
    };
    let page_mask = PAGE_SIZE as u64 - 1;
    let kstart = kernel_start_phys & !page_mask;
    let kend = (kernel_end_phys + page_mask) & !page_mask;

    let mut count: usize = 0;
    let mut push_descriptor = |start: u64, end: u64| {
        if count >= MB2_MAX_MEMORY_DESCRIPTORS {
            return;
        }
        let size = end.saturating_sub(start);
        let pages = size / PAGE_SIZE as u64;
        if pages == 0 {
            return;
        }
        unsafe {
            MB2_MEMORY_DESCRIPTORS[count] = MemoryDescriptor {
                ty: MemoryType::CONVENTIONAL,
                phys_start: start,
                virt_start: start,
                page_count: pages,
                att: MemoryAttribute::empty(),
            };
        }
        count += 1;
    };

    if let Some(tag) = boot_info.memory_map_tag() {
        for area in tag.memory_areas() {
            let start = area.start_address();
            let end = area.end_address();
            if end <= kstart || start >= kend {
                push_descriptor(start, end);
                continue;
            }
            if start < kstart {
                push_descriptor(start, kstart);
            }
            if end > kend {
                push_descriptor(kend, end);
            }
        }
    }

    let descriptors: &'static [MemoryDescriptor] =
        unsafe { core::slice::from_raw_parts(MB2_MEMORY_DESCRIPTORS.as_ptr(), count) };
    crate::serial_println!(
        "[MEM] MB2 PMM descriptors: {} (kernel {:#x}-{:#x} excluded)",
        count,
        kstart,
        kend
    );
    MemoryManager::from_multiboot2(descriptors)
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

    // Initialize the shared zero page — a single permanently-pinned zeroed
    // 4 KiB frame used for read-only anonymous private mappings.
    init_shared_zero_page();

    crate::serial_println!(
        "[MEM] Memory subsystems initialized (total: {} MB)",
        total_mem / (1024 * 1024)
    );
}

fn init_shared_zero_page() {
    let frame = match allocate_contiguous_frames(1) {
        Some(f) => f,
        None => {
            crate::serial_println!("[MEM] WARNING: cannot allocate shared zero page");
            return;
        }
    };
    let phys = frame.start_address().as_u64();
    let zero_va = active_physical_offset().saturating_add(phys);
    unsafe {
        core::ptr::write_bytes(zero_va as *mut u8, 0, PAGE_SIZE);
    }
    frame_ownership::pin_frame(phys);
    ZERO_PAGE_PFN.store(phys, Ordering::Release);
    crate::serial_println!("[MEM] Shared zero page at PFN {:#x}", phys);
}

/// Global bellek yöneticisi için ham pointer.
/// Main fonksiyonu hiç dönmediği için ömür boyunca geçerli kalır.
static mut GLOBAL_MEMORY_MANAGER: *mut MemoryManager = ptr::null_mut();
#[cfg(not(target_os = "uefi"))]
static mut GLOBAL_MB2_FRAME_ALLOCATOR: *mut frame_allocator::Multiboot2FrameAllocator<'static> =
    ptr::null_mut();

/// Global bellek yöneticisi pointer'ını ayarlar.
///
/// # Güvenlik
/// Verilen pointer geçerli ve yaşam süresi tüm kernel ömrü olmalıdır.
pub unsafe fn set_global_memory_manager(manager: *mut MemoryManager) {
    GLOBAL_MEMORY_MANAGER = manager;
    crate::serial_println!("[MEM] set_global_memory_manager ptr={:p}", manager);
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
    allocator: *mut frame_allocator::Multiboot2FrameAllocator<'static>,
) {
    GLOBAL_MB2_FRAME_ALLOCATOR = allocator;
}

#[cfg(not(target_os = "uefi"))]
unsafe fn global_mb2_frame_allocator_mut(
) -> Option<&'static mut frame_allocator::Multiboot2FrameAllocator<'static>> {
    if GLOBAL_MB2_FRAME_ALLOCATOR.is_null() {
        None
    } else {
        Some(&mut *GLOBAL_MB2_FRAME_ALLOCATOR)
    }
}

pub const PAGE_SIZE: usize = 4096;
pub const PHYSICAL_MEMORY_OFFSET: u64 = 0xFFFF_8000_0000_0000;
pub const KERNEL_SPACE_START: u64 = 0xFFFF_FFFF_8000_0000;
pub const KERNEL_STACK_VIRT_BASE: u64 = 0xFFFF_FE00_0000_0000;
pub const KERNEL_STACK_VIRT_LIMIT: u64 = 0xFFFF_FE80_0000_0000;
pub const USER_SPACE_START: u64 = 0x0000_0000_0000_0000;
pub const USER_SPACE_END: u64 = 0x0000_7fff_ffff_ffff;
pub const USER_STACK_TOP: u64 = 0x0000_7fff_ffff_f000;
pub const USER_STACK_PAGES: usize = 256;
pub const USER_STACK_GUARD_PAGES: usize = 1;
pub const USER_STACK_BYTES: u64 = (USER_STACK_PAGES as u64) * (PAGE_SIZE as u64);
pub const USER_STACK_USABLE_BYTES: u64 =
    ((USER_STACK_PAGES - USER_STACK_GUARD_PAGES) as u64) * (PAGE_SIZE as u64);
pub const USER_HEAP_BASE: u64 = 0x0000_1000_0000;
pub const USER_MMAP_BASE: u64 = 0x0000_4000_0000;
pub const USER_MMAP_RANDOM_RANGE: u64 = 1024 * 1024 * 1024 * 1024;
pub const USER_STACK_RANDOM_RANGE: u64 = 256 * 1024 * 1024 * 1024;
pub const USER_HEAP_RANDOM_RANGE: u64 = 1024 * 1024 * 1024;
static mut ACTIVE_PHYSICAL_MEMORY_OFFSET: u64 = PHYSICAL_MEMORY_OFFSET;
static KASLR_OFFSET: AtomicUsize = AtomicUsize::new(0);
const AFC_TRACE_WARMUP: usize = 8;
const AFC_TRACE_INTERVAL: usize = 4096;
static AFC_TRACE_COUNT: AtomicUsize = AtomicUsize::new(0);
static NEXT_KERNEL_STACK_VIRT: AtomicU64 = AtomicU64::new(KERNEL_STACK_VIRT_BASE);

/// Shared zero page physical frame number.  Set once during
/// `init_memory_subsystems` and never changed.  Zero (uninitialised) means
/// the zero page is not yet available.
static ZERO_PAGE_PFN: AtomicU64 = AtomicU64::new(0);

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

// Vma and VmaKind are defined in vma.rs

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

use frame_ownership::SharedAtomicFrame;

struct SharedAnonPages {
    pages: BTreeMap<(u64, u64), SharedAtomicFrame<[u8; 4096]>>,
}

struct SharedFilePages {
    pages: BTreeMap<(usize, u64), SharedAtomicFrame<[u8; 4096]>>,
}

struct PageCache {
    entries: BTreeMap<(usize, u64), PageCacheEntry>,
    max_pages: usize,
}

const FAULT_AROUND_PAGES: u64 = 2;
const RA_INITIAL_SIZE: u64 = 4;
const RA_MAX_SIZE: u64 = 32;
const MMAP_LOTSAMISS: u32 = 5;

/// Per-inode readahead state for sequential-access detection.
/// Modelled after Linux `struct file_ra_state` in `linux/fs.h`.
struct ReadaheadState {
    start: u64,
    size: u64,
    prev_fault: u64,
    mmap_miss: u32,
}

impl ReadaheadState {
    fn new(page_index: u64) -> Self {
        Self {
            start: page_index,
            size: RA_INITIAL_SIZE,
            prev_fault: page_index,
            mmap_miss: 0,
        }
    }
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
    Unevictable,
}

#[derive(Clone, Copy, Default)]
struct SpaceLruCounts {
    anon: usize,
    file: usize,
}

const MAX_EFAULT_ENTRIES: usize = 65536;

const LRU_GEN_COUNT: usize = 4;
const MAX_EFAULT_PER_GEN: usize = MAX_EFAULT_ENTRIES / LRU_GEN_COUNT;

// ============================================================================
// SWAPPINESS
// ============================================================================

/// Swappiness range: 0–100.  Default is 60.
/// - Low swappiness → prefer to drop file-backed pages (avoid swap I/O).
/// - High swappiness → more willing to swap anonymous pages.
const SWAPPINESS_DEFAULT: usize = 60;
/// Bias threshold hysteresis: how many percentage points above/below the
/// swappiness-derived target the anon ratio must drift before reclaim
/// selects a specific class.
const SWAPPINESS_HYSTERESIS: usize = 10;

/// Global swappiness value.  Set via `set_swappiness()`.
static SWAPPINESS: AtomicUsize = AtomicUsize::new(SWAPPINESS_DEFAULT);

/// Set swappiness (0–100).  Values outside the valid range are clamped.
pub fn set_swappiness(value: usize) {
    let clamped = value.clamp(0, 100);
    SWAPPINESS.store(clamped, Ordering::Release);
}

/// Current swappiness value.
pub fn get_swappiness() -> usize {
    SWAPPINESS.load(Ordering::Acquire)
}

fn class_of(entry: &LruEntry) -> LruClass {
    match entry.backing {
        PageBacking::Anonymous { .. } => LruClass::Anonymous,
        PageBacking::File { .. } | PageBacking::Image { .. } => LruClass::File,
    }
}

/// Per-generation LRU data.
///
/// Each generation contains its own ordered page list (by_seq),
/// reverse lookup (by_page), and counters.  No active/inactive split
/// inside a generation — the generation number itself encodes the
/// hotness gradient.
struct LruGenData {
    next_seq: u64,
    by_seq: BTreeMap<u64, LruEntry>,
    by_page: BTreeMap<(u64, u64), u64>,
    anon_pages: usize,
    file_pages: usize,
    space_counts: BTreeMap<u64, SpaceLruCounts>,
    node_counts: BTreeMap<u16, SpaceLruCounts>,
}

impl LruGenData {
    const fn new() -> Self {
        Self {
            next_seq: 1,
            by_seq: BTreeMap::new(),
            by_page: BTreeMap::new(),
            anon_pages: 0,
            file_pages: 0,
            space_counts: BTreeMap::new(),
            node_counts: BTreeMap::new(),
        }
    }

    fn insert(&mut self, entry: LruEntry) {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        self.adjust_counts(&entry, true);
        self.by_page
            .insert((entry.space_id, entry.page_index), seq);
        self.by_seq.insert(seq, entry);
    }

    fn remove(&mut self, space_id: u64, page_index: u64) -> Option<LruEntry> {
        let seq = self.by_page.remove(&(space_id, page_index))?;
        let entry = self.by_seq.remove(&seq)?;
        self.adjust_counts(&entry, false);
        Some(entry)
    }

    /// Pop the oldest entry (smallest seq) that matches the optional filters.
    fn pop_first_matching(
        &mut self,
        class: Option<LruClass>,
        space_hint: Option<u64>,
        node_hint: Option<u16>,
    ) -> Option<LruEntry> {
        if self.by_seq.is_empty() {
            return None;
        }
        let mut selected = None;
        for (seq, entry) in self.by_seq.iter() {
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
                if class_of(entry) != cls {
                    continue;
                }
            }
            selected = Some(*seq);
            break;
        }
        if selected.is_none() && (space_hint.is_some() || node_hint.is_some()) {
            for (seq, entry) in self.by_seq.iter() {
                if let Some(cls) = class {
                    if class_of(entry) != cls {
                        continue;
                    }
                }
                selected = Some(*seq);
                break;
            }
        }
        let seq = selected?;
        let entry = self.by_seq.remove(&seq)?;
        self.by_page.remove(&(entry.space_id, entry.page_index));
        self.adjust_counts(&entry, false);
        Some(entry)
    }

    fn adjust_counts(&mut self, entry: &LruEntry, add: bool) {
        let cls = class_of(entry);
        match cls {
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
            LruClass::Unevictable => unreachable!(),
        }
        let ec = self.space_counts.entry(entry.space_id).or_default();
        match cls {
            LruClass::Anonymous => {
                if add {
                    ec.anon = ec.anon.saturating_add(1);
                } else {
                    ec.anon = ec.anon.saturating_sub(1);
                }
            }
            LruClass::File => {
                if add {
                    ec.file = ec.file.saturating_add(1);
                } else {
                    ec.file = ec.file.saturating_sub(1);
                }
            }
            LruClass::Unevictable => unreachable!(),
        }
        let nc = self.node_counts.entry(entry.node_id).or_default();
        match cls {
            LruClass::Anonymous => {
                if add {
                    nc.anon = nc.anon.saturating_add(1);
                } else {
                    nc.anon = nc.anon.saturating_sub(1);
                }
            }
            LruClass::File => {
                if add {
                    nc.file = nc.file.saturating_add(1);
                } else {
                    nc.file = nc.file.saturating_sub(1);
                }
            }
            LruClass::Unevictable => unreachable!(),
        }
    }
}

/// Generation-based LRU with per-generation spinlocks.
///
/// Hierarchy (top → bottom):
///   NUMA node (static, single node for now)
///     └── memory cgroup (root-cgroup for now)
///           └── generation 0 … LRU_GEN_COUNT-1
///
/// Generation 0 = coldest/oldest → first eviction target.
/// Generation N-1 = hottest/newest → most recently accessed.
/// On touch: promote by 1 generation.
/// On eviction: start scanning from gen 0 upward.
struct GenerationLru {
    page_gen: spin::Mutex<BTreeMap<(u64, u64), u8>>,
    gens: Vec<spin::Mutex<LruGenData>>,
    refaults: spin::Mutex<BTreeMap<(u64, u64), u64>>,
    next_seq: core::sync::atomic::AtomicU64,
    unevictable: spin::Mutex<BTreeMap<(u64, u64), LruEntry>>,
}

impl GenerationLru {
    fn new() -> Self {
        let mut gens = Vec::with_capacity(LRU_GEN_COUNT);
        for _ in 0..LRU_GEN_COUNT {
            gens.push(spin::Mutex::new(LruGenData::new()));
        }
        Self {
            page_gen: spin::Mutex::new(BTreeMap::new()),
            gens,
            refaults: spin::Mutex::new(BTreeMap::new()),
            next_seq: core::sync::atomic::AtomicU64::new(1),
            unevictable: spin::Mutex::new(BTreeMap::new()),
        }
    }

    /// Insert or promote a page.
    ///
    /// - New page  → gen 2 (warm start).
    /// - Existing  → promote one step toward the hottest gen (N-1).
    /// - Refaulted → promote straight to the hottest gen.
    fn touch(&self, entry: LruEntry) {
        let key = (entry.space_id, entry.page_index);
        let mut pg = self.page_gen.lock();
        let old_gen = pg.get(&key).copied();
        let refaulted = self.refaults.lock().remove(&key).is_some();

        let new_gen: u8 = if refaulted {
            (LRU_GEN_COUNT - 1) as u8
        } else if let Some(gen) = old_gen {
            (gen + 1).min((LRU_GEN_COUNT - 1) as u8)
        } else {
            2u8.min((LRU_GEN_COUNT - 1) as u8)
        };

        if let Some(old) = old_gen {
            if old == new_gen {
                // Same generation — fast path: update in-place.
                let mut g = self.gens[old as usize].lock();
                let seq = g.next_seq;
                g.next_seq = seq.saturating_add(1);
                // Remove old seq entry, insert new one.
                if let Some(&old_seq) = g.by_page.get(&key) {
                    g.by_seq.remove(&old_seq);
                }
                g.by_page.insert(key, seq);
                g.by_seq.insert(seq, entry);
                return;
            }
        }

        // Cross-generation move (or new page).
        if let Some(old) = old_gen {
            let lo = old.min(new_gen) as usize;
            let hi = old.max(new_gen) as usize;
            let mut lo_g = self.gens[lo].lock();
            let mut hi_g = self.gens[hi].lock();
            // The generation that was old might be lo or hi.
            let (from, to) = if old < new_gen {
                (&mut *lo_g, &mut *hi_g)
            } else {
                (&mut *hi_g, &mut *lo_g)
            };
            from.remove(key.0, key.1);
            to.insert(entry);
        } else {
            let mut g = self.gens[new_gen as usize].lock();
            g.insert(entry);
        }
        pg.insert(key, new_gen);
    }

    fn remove_page(&self, space_id: u64, page_index: u64) {
        let key = (space_id, page_index);
        let mut pg = self.page_gen.lock();
        if let Some(gen) = pg.remove(&key) {
            let mut g = self.gens[gen as usize].lock();
            g.remove(space_id, page_index);
        }
        let mut ue = self.unevictable.lock();
        ue.remove(&key);
    }

    fn remove_page_and_refault(&self, space_id: u64, page_index: u64) {
        let key = (space_id, page_index);
        let mut pg = self.page_gen.lock();
        if let Some(gen) = pg.remove(&key) {
            let mut g = self.gens[gen as usize].lock();
            g.remove(space_id, page_index);
        }
        self.refaults.lock().remove(&key);
        self.unevictable.lock().remove(&key);
    }

    fn record_refault(&self, space_id: u64, page_index: u64) {
        let seq = self
            .next_seq
            .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let mut r = self.refaults.lock();
        r.insert((space_id, page_index), seq);
        if r.len() > MAX_EFAULT_ENTRIES {
            let threshold = seq.saturating_sub(MAX_EFAULT_ENTRIES as u64 / 2);
            r.retain(|_, v| *v >= threshold);
        }
        let now_tick = crate::task::scheduler::get_ticks() as u64;
        mglru::record_refault(space_id, page_index, now_tick);
        damon::record_refault(space_id, page_index, now_tick);
    }

    fn remove_refault(&self, space_id: u64, page_index: u64) {
        self.refaults.lock().remove(&(space_id, page_index));
    }

    fn update_phys(&self, space_id: u64, page_index: u64, new_phys: u64) {
        let key = (space_id, page_index);
        let mut pg = self.page_gen.lock();
        if let Some(&gen) = pg.get(&key) {
            let mut g = self.gens[gen as usize].lock();
            if let Some(mut entry) = g.remove(space_id, page_index) {
                entry.phys = new_phys;
                g.insert(entry);
            }
        }
    }

    fn cleanup_space(&self, space_id: u64) {
        self.refaults.lock().retain(|(sid, _), _| *sid != space_id);
        let mut pg = self.page_gen.lock();
        let stale: Vec<(u64, u64)> = pg
            .keys()
            .filter(|(sid, _)| *sid == space_id)
            .copied()
            .collect();
        for key in &stale {
            if let Some(gen) = pg.remove(key) {
                self.gens[gen as usize].lock().remove(key.0, key.1);
            }
        }
        for gen in &self.gens {
            gen.lock().space_counts.remove(&space_id);
        }
        self.unevictable.lock().retain(|(sid, _), _| *sid != space_id);
    }

    /// Pop the oldest matching page across all generations.
    ///
    /// Walks generations from coldest (0) to hottest (N-1) so that
    /// older pages are always preferred.  Tries with class hint first,
    /// then without.
    fn pop_oldest_balanced(
        &self,
        class: Option<LruClass>,
        space_hint: Option<u64>,
        node_hint: Option<u16>,
    ) -> Option<LruEntry> {
        for gen_idx in 0..LRU_GEN_COUNT {
            let mut g = self.gens[gen_idx].lock();
            if let Some(entry) = g.pop_first_matching(class, space_hint, node_hint) {
                let key = (entry.space_id, entry.page_index);
                self.page_gen.lock().remove(&key);
                return Some(entry);
            }
        }
        if class.is_some() {
            for gen_idx in 0..LRU_GEN_COUNT {
                let mut g = self.gens[gen_idx].lock();
                if let Some(entry) = g.pop_first_matching(None, space_hint, node_hint) {
                    let key = (entry.space_id, entry.page_index);
                    self.page_gen.lock().remove(&key);
                    return Some(entry);
                }
            }
        }
        None
    }

    fn active_pages(&self) -> usize {
        // Generations 2 and 3 are considered "active" (hot half).
        let mut total = 0usize;
        for i in (LRU_GEN_COUNT / 2)..LRU_GEN_COUNT {
            total = total.saturating_add(self.gens[i].lock().by_seq.len());
        }
        total
    }

    fn inactive_pages(&self) -> usize {
        // Generations 0 and 1 are "inactive" (cold half).
        let mut total = 0usize;
        for i in 0..(LRU_GEN_COUNT / 2) {
            total = total.saturating_add(self.gens[i].lock().by_seq.len());
        }
        total
    }

    fn anon_file_counts(&self) -> (usize, usize) {
        let mut anon = 0usize;
        let mut file = 0usize;
        for gen in &self.gens {
            let g = gen.lock();
            anon = anon.saturating_add(g.anon_pages);
            file = file.saturating_add(g.file_pages);
        }
        (anon, file)
    }

    fn space_counts_for(&self, space_id: u64) -> (usize, usize) {
        let mut anon = 0usize;
        let mut file = 0usize;
        for gen in &self.gens {
            let g = gen.lock();
            if let Some(counts) = g.space_counts.get(&space_id) {
                anon = anon.saturating_add(counts.anon);
                file = file.saturating_add(counts.file);
            }
        }
        (anon, file)
    }

    fn register_unevictable(&self, entry: LruEntry) {
        let key = (entry.space_id, entry.page_index);
        let mut ue = self.unevictable.lock();
        if !ue.contains_key(&key) {
            self.page_gen.lock().remove(&key);
        }
        ue.insert(key, entry);
    }

    fn remove_unevictable(&self, space_id: u64, page_index: u64) -> Option<LruEntry> {
        self.unevictable.lock().remove(&(space_id, page_index))
    }

    fn is_unevictable(&self, space_id: u64, page_index: u64) -> bool {
        self.unevictable.lock().contains_key(&(space_id, page_index))
    }

    /// Pop the oldest unevictable page (by seq).  Used for statistics /
    /// debugging only — never called by the reclaim path.
    #[allow(dead_code)]
    fn pop_oldest_unevictable(&self) -> Option<LruEntry> {
        let mut ue = self.unevictable.lock();
        let key = ue.first_key_value().map(|(k, _)| *k)?;
        ue.remove(&key)
    }

    fn unevictable_pages(&self) -> usize {
        self.unevictable.lock().len()
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
    folio: Option<folio::Folio>,
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

    fn cleanup_space(&mut self, space_id: u64) {
        let keys: Vec<(u64, u64)> = self
            .slots
            .keys()
            .filter(|(sid, _)| *sid == space_id)
            .copied()
            .collect();
        for key in keys {
            self.slots.remove(&key);
        }
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

    fn cleanup_space(&mut self, space_id: u64) {
        let keys: Vec<(u64, u64)> = self
            .slots
            .keys()
            .filter(|(sid, _)| *sid == space_id)
            .copied()
            .collect();
        for key in keys {
            self.slots.remove(&key);
        }
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
    static ref LRU: GenerationLru = GenerationLru::new();
    static ref SWAP: Mutex<SwapState> = Mutex::new(SwapState::new());
    static ref SWAP_DEVICE: Mutex<Option<SwapDeviceState>> = Mutex::new(None);
    static ref WRITEBACK_QUEUE: Mutex<WritebackQueue> = Mutex::new(WritebackQueue::new());
    static ref DIRTY_STATE: Mutex<DirtyThrottleState> = Mutex::new(DirtyThrottleState::new());
    static ref SHARED_ANON_PAGES: Mutex<SharedAnonPages> = Mutex::new(SharedAnonPages {
        pages: BTreeMap::new(),
    });
    static ref SHARED_FILE_PAGES: Mutex<SharedFilePages> = Mutex::new(SharedFilePages {
        pages: BTreeMap::new(),
    });
    static ref READAHEAD_STATES: Mutex<BTreeMap<usize, ReadaheadState>> = Mutex::new(BTreeMap::new());
}

fn dec_frame_ref(phys: u64) -> u32 {
    let flags = frame_ownership::frame_flags(phys);
    if flags.contains(frame_ownership::FrameFlags::REFCACHE) {
        frame_ownership::refcache_dec(phys);
        1
    } else {
        frame_ownership::dec_frame_ref(phys)
    }
}

fn current_space_id() -> u64 {
    with_address_space_ref(|space| space.id)
}

/// Lock or unlock all VMAs overlapping `[addr, addr+len)` and move
/// their present pages between the evictable and unevictable LRU lists.
///
/// * `lock = true`  → mlock: mark VMAs locked, pages → unevictable.
/// * `lock = false` → munlock: mark VMAs unlocked, pages → evictable.
///
/// Returns 0 on success, or a negative errno on failure.
/// Lock or unlock every VMA in the current address space.
pub fn user_mlock_all(lock: bool) {
    with_address_space_mut(|space| {
        space.mlockall_mode = if lock {
            (1 | 2) as u8 // MCL_CURRENT | MCL_FUTURE
        } else {
            0u8
        };
        unsafe {
            let mut cur = space.vmas.head.next_ptr(0);
            while !cur.is_null() {
                (*cur).vma.locked = lock;
                cur = (*cur).next_ptr(0);
            }
        }
    });
    let (min_a, max_a) = with_address_space_ref(|space| {
        let mut min_a = u64::MAX;
        let mut max_a = 0u64;
        unsafe {
            let mut cur = space.vmas.head.next_ptr(0);
            while !cur.is_null() {
                if (*cur).start < min_a { min_a = (*cur).start; }
                if (*cur).end > max_a { max_a = (*cur).end; }
                cur = (*cur).next_ptr(0);
            }
        }
        (min_a, max_a)
    });
    if max_a > min_a && max_a != 0 && min_a != u64::MAX {
        let _ = user_mlock_range(min_a as usize, (max_a - min_a) as usize, lock);
    }
}

/// Unlock every VMA in the current address space.
pub fn user_munlock_all() {
    with_address_space_mut(|space| {
        space.mlockall_mode = 0;
        unsafe {
            let mut cur = space.vmas.head.next_ptr(0);
            while !cur.is_null() {
                (*cur).vma.locked = false;
                cur = (*cur).next_ptr(0);
            }
        }
    });
    let (min_a, max_a) = with_address_space_ref(|space| {
        let mut min_a = u64::MAX;
        let mut max_a = 0u64;
        unsafe {
            let mut cur = space.vmas.head.next_ptr(0);
            while !cur.is_null() {
                if (*cur).start < min_a { min_a = (*cur).start; }
                if (*cur).end > max_a { max_a = (*cur).end; }
                cur = (*cur).next_ptr(0);
            }
        }
        (min_a, max_a)
    });
    if max_a > min_a && max_a != 0 && min_a != u64::MAX {
        let _ = user_mlock_range(min_a as usize, (max_a - min_a) as usize, false);
    }
}

/// Mark VMAs in [addr, addr+len) as locked without moving pages.
/// Used by MLOCK_ONFAULT.
pub fn mark_vmas_locked(addr: usize, len: usize) {
    let page_mask = !(PAGE_SIZE as u64 - 1);
    let start = (addr as u64) & page_mask;
    let end = (addr as u64).saturating_add(len as u64) & page_mask;
    with_address_space_mut(|space| {
        unsafe {
            let mut cur = space.vmas.head.next_ptr(0);
            while !cur.is_null() {
                if (*cur).end > start && (*cur).start < end {
                    (*cur).vma.locked = true;
                }
                cur = (*cur).next_ptr(0);
            }
        }
    });
}

pub fn user_mlock_range(addr: usize, len: usize, lock: bool) -> isize {
    let page_mask = !(PAGE_SIZE as u64 - 1);
    let start = (addr as u64) & page_mask;
    let end = (addr as u64)
        .saturating_add(len as u64)
        .saturating_add(PAGE_SIZE as u64 - 1)
        & page_mask;
    if end <= start || !is_user_range(start, end.saturating_sub(start)) {
        return -22; // -EINVAL
    }

    let space_id = current_space_id();

    // Phase 1 — mark every overlapping VMA as locked / unlocked
    with_address_space_mut(|space| {
        // SAFETY: we hold the address-space write lock so no other
        // thread can mutate the skip list concurrently.
        unsafe {
            let mut cur = space.vmas.head.next_ptr(0);
            while !cur.is_null() {
                let node = &mut *cur;
                if node.end > start && node.start < end {
                    node.vma.locked = lock;
                }
                cur = node.next_ptr(0);
            }
        }
    });

    // Phase 2 — walk present pages in the range and move each between
    // the evictable and unevictable LRU lists.
    let vmas = with_address_space_ref(|space| space.vmas.find_overlapping(start, end));
    if vmas.is_empty() {
        return -12; // -ENOMEM
    }

    for region in &vmas {
        let overlap_start = max(start, region.start);
        let overlap_end = min(end, region.end);
        if overlap_start >= overlap_end {
            continue;
        }

        let start_page =
            Page::<Size4KiB>::containing_address(VirtAddr::new(overlap_start));
        let end_page =
            Page::<Size4KiB>::containing_address(VirtAddr::new(overlap_end.saturating_sub(1)));

        for page in Page::range_inclusive(start_page, end_page) {
            let vaddr = page.start_address().as_u64();
            let page_index = vaddr / PAGE_SIZE as u64;

            if let Some(phys) = paging::translate_addr(page.start_address()) {
                let phys = phys.as_u64();

                if lock {
                    // mlock: evictable → unevictable
                    remove_lru_mapping(space_id, page_index);
                    let mut r = region.clone();
                    r.locked = true;
                    register_lru_mapping(vaddr, phys, &r);
                } else {
                    // munlock: unevictable → evictable
                    LRU.remove_unevictable(space_id, page_index);
                    let mut r = region.clone();
                    r.locked = false;
                    register_lru_mapping(vaddr, phys, &r);
                }
            }
        }
    }

    0
}

fn register_lru_mapping(addr: u64, phys: u64, region: &Vma) {
    let page_mask = !(PAGE_SIZE as u64 - 1);
    let page_start = addr & page_mask;
    let page_index = page_start / PAGE_SIZE as u64;
    let zero_pfn = ZERO_PAGE_PFN.load(Ordering::Acquire);
    if phys != zero_pfn {
        let (level_4_frame, _) = Cr3::read();
        let pml4_phys = level_4_frame.start_address().as_u64();
        rmap::rmap_insert(phys, current_space_id(), page_start, pml4_phys);
    }
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
    if region.locked {
        LRU.register_unevictable(entry);
    } else {
        LRU.touch(entry);
    }
}

fn remove_lru_mapping(space_id: u64, page_index: u64) {
    mglru::remove_page(space_id, page_index);
    damon::remove_page(space_id, page_index);
    LRU.remove_page_and_refault(space_id, page_index);
}

pub(crate) fn lru_update_phys(space_id: u64, page_index: u64, new_phys: u64) {
    LRU.update_phys(space_id, page_index, new_phys);
}

pub fn cleanup_address_space(space_id: u64) {
    cils::unregister(space_id);
    PAGE_TABLE_LOCKS.lock().remove(&space_id);
    AS_LIFECYCLE.lock().remove(&space_id);
    mglru::cleanup_space(space_id);
    damon::cleanup_space(space_id);
    LRU.cleanup_space(space_id);
    rmap::rmap_cleanup_space(space_id);
    if let Some(device) = SWAP_DEVICE.lock().as_mut() {
        device.cleanup_space(space_id);
    }
    SWAP.lock().cleanup_space(space_id);
}

// ============================================================================
// TRY_TO_UNMAP — reverse-map based page unmapping (Linux rmap.c semantics)
// ============================================================================

/// Check whether the ACCESSED flag is set in every PTE mapping a physical
/// page.  Returns the number of mappings that have ACCESSED = 1.  Clears
/// the flag after reading so the ageing cycle can restart.
///
/// Modelled after Linux `folio_referenced()` — the return value is a
/// coarse working-set signal used by the reclaim path to rank pages.
pub fn page_referenced(phys: u64) -> usize {
    let entries = rmap::rmap_lookup(phys);
    if entries.is_empty() {
        return 0;
    }
    let hhdm = active_physical_offset();
    let mut count = 0;
    for entry in entries {
        let lock_arc = get_pt_lock(entry.space_id);
        let _guard = lock_arc.lock();

        // Walk remote page table via HHDM to reach the leaf PTE.
        let pml4_v = VirtAddr::new(hhdm + entry.pml4);
        let pml4 = unsafe { &*pml4_v.as_ptr::<PageTable>() };
        let pml4e = &pml4[(entry.virt >> 39) as usize & 0x1FF];
        if pml4e.is_unused() {
            continue;
        }
        let pdpt_v = VirtAddr::new(hhdm + pml4e.addr().as_u64());
        let pdpt = unsafe { &*pdpt_v.as_ptr::<PageTable>() };
        let pdpte = &pdpt[(entry.virt >> 30) as usize & 0x1FF];
        if pdpte.is_unused() || pdpte.flags().contains(PageTableFlags::HUGE_PAGE) {
            continue;
        }
        let pd_v = VirtAddr::new(hhdm + pdpte.addr().as_u64());
        let pd = unsafe { &*pd_v.as_ptr::<PageTable>() };
        let pde = &pd[(entry.virt >> 21) as usize & 0x1FF];
        if pde.is_unused() || pde.flags().contains(PageTableFlags::HUGE_PAGE) {
            continue;
        }
        let pt_v = VirtAddr::new(hhdm + pde.addr().as_u64());
        let pt = unsafe { &mut *pt_v.as_mut_ptr::<PageTable>() };
        let pte = &mut pt[(entry.virt >> 12) as usize & 0x1FF];
        if pte.is_unused() {
            continue;
        }

        if pte.flags().contains(PageTableFlags::ACCESSED) {
            let mut f = pte.flags();
            f.remove(PageTableFlags::ACCESSED);
            pte.set_flags(f);
            count += 1;
        }
    }
    count
}

/// Unmap a physical page from remote address spaces that map it,
/// skipping the current space when requested.  Returns the number of
/// PTEs unmapped.
///
/// This is the echOS analogue of `try_to_unmap` in Linux `mm/rmap.c`.
/// It does **not** call `dec_frame_ref` — the caller owns refcount
/// management (the reclaim path decrements once when the last mapping
/// is removed via `flush_tlb_batch`).
pub fn try_to_unmap(phys: u64, flags: rmap::TtuFlags) -> usize {
    let entries = rmap::rmap_lookup(phys);
    if entries.is_empty() {
        return 0;
    }

    let current = current_space_id();
    let hhdm = active_physical_offset();
    let mut unmapped = 0;

    for entry in entries {
        if flags.contains(rmap::TtuFlags::SKIP_CURRENT) && entry.space_id == current {
            continue;
        }

        let lock_arc = get_pt_lock(entry.space_id);
        let _guard = lock_arc.lock();

        let remote_pml4_virt = VirtAddr::new(hhdm + entry.pml4);
        let table = unsafe { &mut *(remote_pml4_virt.as_mut_ptr::<PageTable>()) };
        // Safety: HHDM mapping is permanent; the lock protects against
        // concurrent modification of this address space.
        let mut remote_mapper = unsafe {
            OffsetPageTable::new(table, VirtAddr::new(hhdm))
        };

        let vaddr = VirtAddr::new(entry.virt);
        match paging::unmap_page(&mut remote_mapper, vaddr) {
            Ok(_frame) => {
                rmap::rmap_remove(phys, entry.space_id, entry.virt);
                crate::cpu::smp::tlb_defer_shootdown();
                unmapped += 1;
            }
            Err(UnmapError::PageNotMapped) | Err(UnmapError::InvalidFrameAddress(_)) => {
                // PTE already gone — clean up the stale rmap entry.
                rmap::rmap_remove(phys, entry.space_id, entry.virt);
            }
            Err(UnmapError::ParentEntryHugePage) => {
                // Huge page — skip for now; echOS does not split
                // remote huge pages during reclaim.
            }
        }
    }

    unmapped
}

/// Reap a dying task's address space — Phases 1, 3, 4, 5 from `scheduler::exit()`.
/// Phase 2 (VMA clear) is the caller's responsibility (needs the Arc).
/// Phase 6 (Arc drop) happens when the caller drops the last reference.
pub fn reap_address_space(sid: u64, pml4_phys: Option<PhysAddr>) {
    start_as_teardown(sid);
    crate::cpu::smp::tlb_defer_shootdown();
    crate::cpu::smp::tlb_flush_pending();
    if let Some(pml4) = pml4_phys {
        free_user_page_tables(pml4, sid);
    }
    cleanup_address_space(sid);
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
    if let Some(_frame) = compact_contiguous(THP_PAGES) {
    }
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
            let folio_order = 9;
            for i in 0..THP_PAGES {
                let offset = (i as u64).saturating_mul(PAGE_SIZE as u64);
                let page_addr = huge_start.saturating_add(offset);
                let phys_addr = phys.saturating_add(offset);
                frame_ownership::SharedAtomicFrame::<[u8; 4096]>::incref(phys_addr);
            }
            folio::folio_register(phys, folio_order);
            for i in 0..THP_PAGES {
                let offset = (i as u64).saturating_mul(PAGE_SIZE as u64);
                let page_addr = huge_start.saturating_add(offset);
                let phys_addr = phys.saturating_add(offset);
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
    let swp = SWAPPINESS.load(Ordering::Acquire);

    // threshold maps swappiness onto the anon-ratio axis.
    // swappiness=0:   threshold=80 (almost never pick Anonymous → avoids swap)
    // swappiness=50:  threshold=50 (neutral)
    // swappiness=100: threshold=20 (aggressively pick Anonymous → swap freely)
    let threshold = if swp < 50 {
        50 + (50 - swp) * 30 / 50
    } else {
        50 - (swp - 50) * 30 / 50
    };

    if anon_ratio > threshold + SWAPPINESS_HYSTERESIS {
        Some(LruClass::Anonymous)
    } else if anon_ratio < threshold - SWAPPINESS_HYSTERESIS {
        Some(LruClass::File)
    } else {
        None
    }
}

fn reclaim_class_for_space(space_id: u64) -> Option<LruClass> {
    let (anon, file) = LRU.space_counts_for(space_id);
    select_reclaim_class(anon, file)
}

fn reclaim_class_global() -> Option<LruClass> {
    let (anon, file) = LRU.anon_file_counts();
    select_reclaim_class(anon, file)
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
    pub(crate) id: u64,
    pub(crate) lifecycle: Arc<AsLifecycle>,
    pub(crate) vmas: VmaMap,
    image: Option<ImageRef>,
    heap_base: u64,
    heap_break: u64,
    mmap_base: u64,
    mmap_next: u64,
    stack_base: u64,
    stack_top: u64,
    mlockall_mode: u8,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AddressSpacePageCounts {
    pub resident_anon_pages: usize,
    pub resident_file_pages: usize,
    pub swapped_pages: usize,
    pub committed_pages: usize,
}

impl AddressSpacePageCounts {
    pub fn resident_pages(&self) -> usize {
        self.resident_anon_pages
            .saturating_add(self.resident_file_pages)
    }
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

lazy_static! {
    static ref PAGE_TABLE_LOCKS: spin::Mutex<BTreeMap<u64, Arc<spin::Mutex<()>>>> =
        spin::Mutex::new(BTreeMap::new());
}

fn get_pt_lock(space_id: u64) -> Arc<spin::Mutex<()>> {
    PAGE_TABLE_LOCKS
        .lock()
        .entry(space_id)
        .or_insert_with(|| Arc::new(spin::Mutex::new(())))
        .clone()
}

pub const AS_ALIVE: u8 = 0;
pub const AS_DYING: u8 = 1;
pub const AS_DEAD: u8 = 2;

pub struct AsLifecycle {
    pub state: AtomicU8,
    pub active_faults: AtomicU32,
}

impl AsLifecycle {
    pub fn new() -> Self {
        AsLifecycle {
            state: AtomicU8::new(AS_ALIVE),
            active_faults: AtomicU32::new(0),
        }
    }

    pub fn enter_fault(&self) -> bool {
        self.active_faults.fetch_add(1, Ordering::AcqRel);
        if self.state.load(Ordering::Acquire) != AS_ALIVE {
            self.active_faults.fetch_sub(1, Ordering::AcqRel);
            false
        } else {
            true
        }
    }

    pub fn exit_fault(&self) {
        self.active_faults.fetch_sub(1, Ordering::Release);
    }

    pub fn set_dying(&self) {
        self.state.store(AS_DYING, Ordering::Release);
    }

    pub fn set_dead(&self) {
        self.state.store(AS_DEAD, Ordering::Release);
    }

    pub fn is_alive(&self) -> bool {
        self.state.load(Ordering::Acquire) == AS_ALIVE
    }

    pub fn drain_faults(&self) {
        while self.active_faults.load(Ordering::Acquire) > 0 {
            core::hint::spin_loop();
        }
    }
}

lazy_static! {
    static ref AS_LIFECYCLE: spin::Mutex<BTreeMap<u64, Arc<AsLifecycle>>> =
        spin::Mutex::new(BTreeMap::new());
}

pub fn get_as_lifecycle(space_id: u64) -> Arc<AsLifecycle> {
    AS_LIFECYCLE
        .lock()
        .entry(space_id)
        .or_insert_with(|| Arc::new(AsLifecycle::new()))
        .clone()
}

pub fn start_as_teardown(space_id: u64) {
    let lc = get_as_lifecycle(space_id);
    lc.set_dying();
    lc.drain_faults();
    lc.set_dead();
}

pub fn remove_lifecycle(space_id: u64) {
    AS_LIFECYCLE.lock().remove(&space_id);
}

pub fn as_lifecycle(space: &Arc<RwLock<AddressSpace>>) -> Arc<AsLifecycle> {
    space.read().lifecycle.clone()
}

lazy_static! {
    static ref DEFAULT_ADDRESS_SPACE: RwLock<AddressSpace> = RwLock::new(AddressSpace {
        id: 0,
        lifecycle: get_as_lifecycle(0),
        vmas: VmaMap::new(),
        image: None,
        heap_base: 0,
        heap_break: 0,
        mmap_base: 0,
        mmap_next: 0,
        stack_base: 0,
        stack_top: 0,
        mlockall_mode: 0,
    });
}
static ACTIVE_ADDRESS_SPACE: Mutex<Option<Arc<RwLock<AddressSpace>>>> = Mutex::new(None);

pub fn address_space_page_counts(space: &Arc<RwLock<AddressSpace>>) -> AddressSpacePageCounts {
    let (space_id, committed_pages) = {
        let guard = space.read();
        let committed = guard.vmas.committed_pages();
        (guard.id, committed)
    };

    let (anon, file) = LRU.space_counts_for(space_id);
    let resident = SpaceLruCounts { anon, file };
    let swapped_pages = {
        let memory_swap = SWAP
            .lock()
            .slots
            .keys()
            .filter(|(id, _)| *id == space_id)
            .count();
        let device_swap = SWAP_DEVICE
            .lock()
            .as_ref()
            .map(|device| {
                device
                    .slots
                    .keys()
                    .filter(|(id, _)| *id == space_id)
                    .count()
            })
            .unwrap_or(0);
        memory_swap.saturating_add(device_swap)
    };

    AddressSpacePageCounts {
        resident_anon_pages: resident.anon,
        resident_file_pages: resident.file,
        swapped_pages,
        committed_pages,
    }
}

pub fn with_address_space_ref<F, R>(f: F) -> R
where
    F: FnOnce(&AddressSpace) -> R,
{
    let active = { ACTIVE_ADDRESS_SPACE.lock().clone() };
    if let Some(space) = active {
        let guard = space.read();
        f(&*guard)
    } else {
        let guard = DEFAULT_ADDRESS_SPACE.read();
        f(&*guard)
    }
}

pub fn with_address_space_mut<F, R>(f: F) -> R
where
    F: FnOnce(&mut AddressSpace) -> R,
{
    let active = { ACTIVE_ADDRESS_SPACE.lock().clone() };
    if let Some(space) = active {
        let mut guard = space.write();
        let r = f(&mut *guard);
        let mut retired = guard.vmas.drain_retired();
        drop(guard);
        cils::reclaim_retired(&mut retired);
        r
    } else {
        let mut guard = DEFAULT_ADDRESS_SPACE.write();
        let r = f(&mut *guard);
        let mut retired = guard.vmas.drain_retired();
        drop(guard);
        cils::reclaim_retired(&mut retired);
        r
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
    addr != 0 && addr <= USER_SPACE_END
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

pub fn create_address_space(image: &[u8]) -> Arc<RwLock<AddressSpace>> {
    let sid = next_address_space_id();
    let space = Arc::new(RwLock::new(AddressSpace {
        id: sid,
        lifecycle: get_as_lifecycle(sid),
        vmas: VmaMap::new(),
        image: Some(image_ref_from_slice(image)),
        heap_base: 0,
        heap_break: 0,
        mmap_base: 0,
        mmap_next: 0,
        stack_base: 0,
        stack_top: 0,
        mlockall_mode: 0,
    }));
    // Register CILS sentinel for concurrent readers.
    cils::register(sid, &space.read().vmas);
    space
}

pub fn create_address_space_owned(image: Arc<[u8]>) -> Arc<RwLock<AddressSpace>> {
    let sid = next_address_space_id();
    let space = Arc::new(RwLock::new(AddressSpace {
        id: sid,
        lifecycle: get_as_lifecycle(sid),
        vmas: VmaMap::new(),
        image: Some(image_ref_from_owned(image)),
        heap_base: 0,
        heap_break: 0,
        mmap_base: 0,
        mmap_next: 0,
        stack_base: 0,
        stack_top: 0,
        mlockall_mode: 0,
    }));
    cils::register(sid, &space.read().vmas);
    space
}

pub fn create_empty_address_space() -> Arc<RwLock<AddressSpace>> {
    create_address_space(&[])
}

pub fn address_space_id(space: &Arc<RwLock<AddressSpace>>) -> u64 {
    space.read().id
}

pub fn allocate_user_mmap_in(space: &Arc<RwLock<AddressSpace>>, size: u64) -> Option<u64> {
    if size == 0 {
        return None;
    }
    let page_mask = !(PAGE_SIZE as u64 - 1);
    let aligned = size.saturating_add(PAGE_SIZE as u64 - 1) & page_mask;
    let mut guard = space.write();
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
    space: &Arc<RwLock<AddressSpace>>,
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
        let mut guard = space.write();
        let ok = guard.vmas.insert(Vma {
            start,
            end,
            flags,
            kind: VmaKind::Anonymous { id: shared_id },
            cow: false,
            shared: true,
            locked: false,
        });
        let mut retired = guard.vmas.drain_retired();
        drop(guard);
        cils::reclaim_retired(&mut retired);
        ok
    };
    inserted.then_some(shared_id)
}

pub fn clone_address_space_for_cow(
    space: &Arc<RwLock<AddressSpace>>,
) -> Option<Arc<RwLock<AddressSpace>>> {
    let original = space.read();
    let mut cloned = original.clone();
    cloned.id = next_address_space_id();
    cloned.vmas.mark_cow();
    drop(original);
    if !apply_cow_write_protect_current() {
        return None;
    }
    Some(Arc::new(RwLock::new(cloned)))
}

pub fn clone_user_pml4_for_cow() -> Option<PhysFrame> {
    let regions = with_address_space_ref(|space| {
        space.vmas.collect_filtered(|region| region.cow || region.shared)
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
                    frame_ownership::SharedAtomicFrame::<[u8; 4096]>::incref(phys.as_u64());
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

pub fn set_active_address_space(space: Option<Arc<RwLock<AddressSpace>>>) {
    *ACTIVE_ADDRESS_SPACE.lock() = space;
}

pub fn apply_cow_write_protect_current() -> bool {
    let regions = with_address_space_ref(|space| {
        space.vmas.collect_filtered(|region| region.cow)
    });
    if regions.is_empty() {
        return true;
    }
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
    let lock_arc__pt_guard = get_pt_lock(space_id);
let _pt_guard = lock_arc__pt_guard.lock();
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
        space.vmas.insert(Vma {
            start,
            end,
            flags,
            kind: VmaKind::Anonymous { id: 0 },
            cow: false,
            shared: false,
            locked: false,
        })
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
        space.vmas.insert(Vma {
            start,
            end,
            flags,
            kind: VmaKind::Anonymous { id },
            cow: false,
            shared: true,
            locked: false,
        })
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
        let old_len = space.vmas.len();
        space.vmas.update_flags(start, end, flags);
        space.vmas.len() != old_len
    })
}

fn initial_heap_base(space: &AddressSpace) -> u64 {
    let limit = space.mmap_base.saturating_sub(PAGE_SIZE as u64);
    let mut base = USER_HEAP_BASE;
    for region in space.vmas.iter() {
        if matches!(region.kind, VmaKind::File { .. } | VmaKind::Image { .. }) {
            base = base.max(region.end);
        }
    }
    let page_mask = !(PAGE_SIZE as u64 - 1);
    let aligned = (base.saturating_add(PAGE_SIZE as u64 - 1)) & page_mask;
    let max_offset = limit.saturating_sub(aligned).min(USER_HEAP_RANDOM_RANGE);
    let max_pages = (max_offset / PAGE_SIZE as u64) as u32;
    let offset_pages = if max_pages == 0 {
        0
    } else {
        random::next_range(max_pages + 1) as u64
    };
    let offset = offset_pages.saturating_mul(PAGE_SIZE as u64);
    aligned.saturating_add(offset).min(limit)
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
    let mut data = alloc::vec![0u8; PAGE_SIZE];
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

fn fill_folio_from_cache(
    inode: &Arc<dyn INode>,
    file_offset: u64,
    file_end: u64,
    folio: &folio::Folio,
) -> bool {
    let nr = folio.nr_pages();
    let hhdm = active_physical_offset();
    for i in 0..nr {
        let page_file_off = file_offset + (i as u64) * 4096;
        if page_file_off >= file_end {
            break;
        }
        if let Some(data) = read_cached_file_page(inode, page_file_off, file_end) {
            let page_phys = folio.page_phys(i);
            let dst = (hhdm + page_phys) as *mut u8;
            let len = core::cmp::min(4096, data.len());
            unsafe {
                core::ptr::copy_nonoverlapping(data.as_ptr(), dst, len);
            }
        } else {
            return false;
        }
    }
    true
}

fn prefetch_file_pages(inode: &Arc<dyn INode>, start_offset: u64, count: u64, file_end: u64) {
    for i in 1..=count {
        let offset = start_offset.saturating_add(i.saturating_mul(PAGE_SIZE as u64));
        if offset >= file_end {
            break;
        }
        let page_index = offset / PAGE_SIZE as u64;
        let key = (inode_key(inode), page_index);
        {
            let cache = PAGE_CACHE.lock();
            if cache.entries.contains_key(&key) {
                continue;
            }
        }
        if let Some(data) = read_file_page(inode, offset, file_end) {
            PAGE_CACHE.lock().insert(
                key,
                PageCacheEntry {
                    data,
                    dirty: false,
                },
            );
        }
    }
}

fn do_file_fault_around(
    region: &Vma,
    space_id: u64,
    fault_page_file_offset: u64,
    file_end: u64,
    map_flags: PageTableFlags,
    table_flags: PageTableFlags,
) -> u64 {
    let VmaKind::File {
        inode,
        file_offset,
        ..
    } = &region.kind
    else {
        return 0;
    };
    let key_inode = inode_key(inode);
    let (level_4_frame, _) = Cr3::read();
    let phys_base = level_4_frame.start_address();
    let virt_base = VirtAddr::new(active_physical_offset() + phys_base.as_u64());
    let table = unsafe { &mut *(virt_base.as_mut_ptr()) };
    let mut mapper =
        unsafe { OffsetPageTable::new(table, VirtAddr::new(active_physical_offset())) };
    let frame_allocator = unsafe { global_memory_manager_mut() };
    let Some(frame_allocator) = frame_allocator else {
        return 0;
    };
    let mut mapped = 0u64;
    for i in 1..=FAULT_AROUND_PAGES {
        let step = i.saturating_mul(PAGE_SIZE as u64);
        for direction in [0u64, 1u64] {
            let (neighbor_offset, too_far) = if direction == 0 {
                let off = fault_page_file_offset.saturating_add(step);
                (off, off >= file_end)
            } else {
                if step > fault_page_file_offset {
                    continue;
                }
                let off = fault_page_file_offset - step;
                (off, off < *file_offset)
            };
            if too_far {
                continue;
            }
            let vaddr = region.start.saturating_add(neighbor_offset.saturating_sub(*file_offset));
            if vaddr >= region.end {
                continue;
            }
            if paging::translate_addr(VirtAddr::new(vaddr)).is_some() {
                continue;
            }
            let page_index = neighbor_offset / PAGE_SIZE as u64;
            let key = (key_inode, page_index);
            if region.shared {
                let shared_pages = SHARED_FILE_PAGES.lock();
                if let Some(shared) = shared_pages.pages.get(&key) {
                    let phys = shared.phys();
                    let frame = PhysFrame::<Size4KiB>::containing_address(PhysAddr::new(phys));
                    let neighbor_page = Page::containing_address(VirtAddr::new(vaddr));
                    let lock_arc__pt_guard = get_pt_lock(space_id);
let _pt_guard = lock_arc__pt_guard.lock();
                    if paging::translate_addr(VirtAddr::new(vaddr)).is_none() {
                        let map_result = paging::with_wp_disabled(|| unsafe {
                            mapper.map_to_with_table_flags(
                                neighbor_page,
                                frame,
                                map_flags,
                                table_flags,
                                frame_allocator,
                            )
                        });
                        if let Ok(flush) = map_result {
                            flush.flush();
                            drop(_pt_guard);
                            core::mem::forget(shared.clone());
                            if region.flags.contains(PageTableFlags::WRITABLE) {
                                mark_cache_dirty(key_inode, key);
                            }
                            register_lru_mapping(vaddr, phys, region);
                            mapped += 1;
                        } else {
                            drop(_pt_guard);
                        }
                    } else {
                        drop(_pt_guard);
                    }
                }
            } else {
                let cache_entry = {
                    let cache = PAGE_CACHE.lock();
                    cache.entries.get(&key).map(|e| e.data.clone())
                };
                if let Some(data) = cache_entry {
                    let mut frame = match frame_allocator.allocate_user_frame() {
                        Some(f) => f,
                        None => continue,
                    };
                    let phys = frame.start_address().as_u64();
                    let copy_len = min(
                        PAGE_SIZE as u64,
                        file_end.saturating_sub(neighbor_offset),
                    ) as usize;
                    if copy_len > 0 {
                        let dst: &mut [u8] = &mut *frame;
                        dst[..copy_len].copy_from_slice(&data[..copy_len]);
                    }
                    let neighbor_page = Page::containing_address(VirtAddr::new(vaddr));
                    let lock_arc__pt_guard = get_pt_lock(space_id);
let _pt_guard = lock_arc__pt_guard.lock();
                    if paging::translate_addr(VirtAddr::new(vaddr)).is_none() {
                        let map_result = paging::with_wp_disabled(|| unsafe {
                            mapper.map_to_with_table_flags(
                                neighbor_page,
                                frame.as_phys_frame(),
                                map_flags,
                                table_flags,
                                frame_allocator,
                            )
                        });
                        if let Ok(flush) = map_result {
                            flush.flush();
                            drop(_pt_guard);
                            frame.leak_as_shared();
                            if region.flags.contains(PageTableFlags::WRITABLE) {
                                mark_cache_dirty(key_inode, key);
                            }
                            register_lru_mapping(vaddr, phys, region);
                            mapped += 1;
                        } else {
                            drop(_pt_guard);
                        }
                    } else {
                        drop(_pt_guard);
                    }
                }
            }
        }
    }
    mapped
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
            let mut buf = alloc::vec![0u8; max_len];
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
    let mut buf = alloc::vec![0u8; max_len];
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
            folio: None,
        },
        urgent,
    );
}

fn schedule_writeback_folio(
    inode: Arc<dyn INode>,
    file_offset: u64,
    file_end: u64,
    phys: u64,
    urgent: bool,
    folio: folio::Folio,
) {
    WRITEBACK_QUEUE.lock().push(
        WritebackEntry {
            inode,
            file_offset,
            file_end,
            phys,
            urgent,
            folio: Some(folio),
        },
        urgent,
    );
}

fn writeback_file_folio(inode: &Arc<dyn INode>, file_offset: u64, file_end: u64, folio: &folio::Folio) -> bool {
    let nr = folio.nr_pages();
    let base_phys = folio.head_phys();
    let hhdm = active_physical_offset();
    for i in 0..nr {
        let page_phys = base_phys + (i as u64) * 4096;
        let page_file_offset = file_offset + (i as u64) * 4096;
        if page_file_offset >= file_end {
            break;
        }
        let max_len = core::cmp::min(4096u64, file_end.saturating_sub(page_file_offset)) as usize;
        if max_len == 0 {
            continue;
        }
        let virt = hhdm + page_phys;
        let mut buf = alloc::vec![0u8; max_len];
        unsafe {
            core::ptr::copy_nonoverlapping(virt as *const u8, buf.as_mut_ptr(), max_len);
        }
        let page_idx = page_file_offset / 4096;
        let key = (inode_key(inode), page_idx);
        if vfs_write_at(inode, page_file_offset as usize, &buf).is_err() {
            return false;
        }
        mark_cache_clean(key.0, key);
        let mut cache = PAGE_CACHE.lock();
        if let Some(entry) = cache.entries.get_mut(&key) {
            let copy_len = core::cmp::min(4096, max_len);
            entry.data[..copy_len].copy_from_slice(&buf[..copy_len]);
        }
    }
    true
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
        let ok = if let Some(ref folio) = entry.folio {
            writeback_file_folio(&entry.inode, entry.file_offset, entry.file_end, folio)
        } else {
            writeback_file_page(&entry.inode, entry.file_offset, entry.file_end, entry.phys)
        };
        if !ok {
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

/// Batch size for deferred TLB shootdown frames.
const TLB_BATCH_SIZE: usize = 64;

/// Flush any deferred TLB shootdowns (via [`crate::cpu::smp::tlb_flush_pending`])
/// and process the collected frames — decrement refcounts, clean up shared-page
/// caches, and free frames whose refcount reaches zero.
///
/// Must be called before any frame in `pending` is touched by another CPU.
fn flush_tlb_batch(
    pending: &mut Vec<(Frame4K, LruEntry, u64)>,
    freed: &mut usize,
) {
    if pending.is_empty() {
        return;
    }
    crate::cpu::smp::tlb_flush_pending();
    for (frame, entry, virt) in pending.drain(..) {
        let phys_unmapped = frame.start_address().as_u64();
        let new_count = dec_frame_ref(phys_unmapped);
        rmap::rmap_remove(phys_unmapped, entry.space_id, virt);
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
        *freed = freed.saturating_add(1);
    }
}

type Frame4K = x86_64::structures::paging::PhysFrame<x86_64::structures::paging::Size4KiB>;

/// Flush deferred TLB shootdowns for unmap batches, then drain refcount/free
/// frames. Separate from [`flush_tlb_batch`] because unmap batches carry
/// shared-page metadata instead of a full `LruEntry`.
fn flush_unmap_batch(
    pending: &mut Vec<(Frame4K, Option<(u64, u64)>, Option<(usize, u64)>)>,
) {
    if pending.is_empty() {
        return;
    }
    crate::cpu::smp::tlb_flush_pending();
    for (frame, shared_anon_key, shared_file_key) in pending.drain(..) {
        let phys = frame.start_address().as_u64();
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

fn reclaim_pages_scoped(target: usize, global: bool) -> usize {
    let mut freed = 0;
    let mut scan_budget = target.saturating_mul(6).max(8);
    let space_id = current_space_id();
    let node_hint = Some(current_numa_node());
    let mut pending: Vec<(Frame4K, LruEntry, u64)> = Vec::new();
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
        let entry = LRU.pop_oldest_balanced(class_hint, space_hint, node_select);
        let Some(entry) = entry else {
            flush_tlb_batch(&mut pending, &mut freed);
            break;
        };
        if !global && entry.space_id != space_id {
            flush_tlb_batch(&mut pending, &mut freed);
            LRU.touch(entry);
            break;
        }
        if let Some(hint) = damon::hint_for_page(entry.space_id, entry.page_index, now_tick) {
            let preserve_hot =
                matches!(hint.temperature, damon::DamonTemperature::Hot) && !pressure_critical;
            let preserve_warm = matches!(hint.temperature, damon::DamonTemperature::Warm)
                && pressure.full_avg10 < 200
                && scan_budget > 0;
            if preserve_hot || preserve_warm {
                LRU.touch(entry);
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
                    let rc = frame_ownership::frame_refcount(phys);
                    if rc <= 1 {
                        should_swap = true;
                    } else if global {
                        let unmapped = try_to_unmap(phys, rmap::TtuFlags::SKIP_CURRENT);
                        if unmapped > 0 {
                            should_swap = true;
                        }
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
            if frame_ownership::is_folio_head(phys) || frame_ownership::is_folio_tail(phys) {
                if let Some(folio) = folio::Folio::from_phys(phys) {
                    let folio_file_offset = file_offset & !((folio.nr_pages() as u64).saturating_mul(4096).saturating_sub(1));
                    schedule_writeback_folio(inode, folio_file_offset, file_end, phys, should_reclaim_now(), folio);
                } else {
                    schedule_writeback(inode, file_offset, file_end, phys, should_reclaim_now());
                }
            } else {
                schedule_writeback(inode, file_offset, file_end, phys, should_reclaim_now());
            }
        }
        if should_swap {
            let mut data = alloc::vec![0u8; PAGE_SIZE];
            let virt_phys = active_physical_offset().saturating_add(phys);
            unsafe {
                core::ptr::copy_nonoverlapping(
                    virt_phys as *const u8,
                    data.as_mut_ptr(),
                    PAGE_SIZE,
                );
            }
            if !swap_store_page(entry.space_id, entry.page_index, data) {
                flush_tlb_batch(&mut pending, &mut freed);
                LRU.touch(entry);
                break;
            }
        }
        LRU.record_refault(entry.space_id, entry.page_index);
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
        let frame = {
            let lock_arc__pt_guard = get_pt_lock(entry.space_id);
let _pt_guard = lock_arc__pt_guard.lock();
            let unmap_result = paging::unmap_page(&mut mapper, VirtAddr::new(virt));
            match unmap_result {
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
            }
        };
        crate::cpu::smp::tlb_defer_shootdown();
        pending.push((frame, entry, virt));
        if pending.len() >= TLB_BATCH_SIZE {
            flush_tlb_batch(&mut pending, &mut freed);
        }
    }
    flush_tlb_batch(&mut pending, &mut freed);
    freed
}

pub fn reclaim_pages(target: usize) -> usize {
    reclaim_pages_scoped(target, false)
}

pub fn reclaim_pages_global(target: usize) -> usize {
    reclaim_pages_scoped(target, true)
}

pub fn shrink_vma(old_start: u64, old_size: u64, new_size: u64) {
    let old_end = old_start + old_size as u64;
    let new_end = old_start + new_size as u64;
    with_address_space_mut(|space| {
        let regions = space.vmas.find_overlapping(old_start, old_end);
        for region in &regions {
            if old_start >= region.start && old_end <= region.end {
                space.vmas.remove(old_start, old_end);
                if region.start < old_start {
                    let mut left = region.clone();
                    left.end = old_start;
                    space.vmas.insert(left);
                }
                if new_end < old_end {
                    let mut trim = region.clone();
                    trim.start = new_end;
                    trim.end = old_end;
                    space.vmas.insert(trim);
                }
                if region.end > old_end {
                    let mut right = region.clone();
                    right.start = old_end;
                    space.vmas.insert(right);
                }
                break;
            }
        }
    });
}

pub fn extend_vma(old_start: u64, old_size: u64, new_size: u64) {
    let old_end = old_start + old_size as u64;
    let new_end = old_start + new_size as u64;
    with_address_space_mut(|space| {
        let regions = space.vmas.find_overlapping(old_start, old_end);
        for region in &regions {
            if region.start == old_start && region.end == old_end {
                let mut upd = region.clone();
                upd.end = new_end;
                space.vmas.remove(old_start, old_end);
                space.vmas.insert(upd);
                return;
            }
        }
        space.vmas.insert(Vma {
            start: old_start,
            end: new_end,
            flags: PageTableFlags::USER_ACCESSIBLE | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE,
            kind: VmaKind::Anonymous { id: 0 },
            cow: false,
            shared: false,
            locked: false,
        });
    });
}

pub fn clone_vma_to(old_start: u64, old_size: u64, new_start: u64, new_size: u64) {
    let old_end = old_start + old_size as u64;
    let new_end = new_start + new_size as u64;
    with_address_space_mut(|space| {
        let regions = space.vmas.find_overlapping(old_start, old_end);
        for region in &regions {
            if region.start == old_start && region.end == old_end {
                let mut new_vma = region.clone();
                new_vma.start = new_start;
                new_vma.end = new_end;
                space.vmas.insert(new_vma);
                return;
            }
        }
        space.vmas.insert(Vma {
            start: new_start,
            end: new_end,
            flags: PageTableFlags::USER_ACCESSIBLE | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE,
            kind: VmaKind::Anonymous { id: 0 },
            cow: false,
            shared: false,
            locked: false,
        });
    });
}

pub fn remove_vma(old_start: u64, old_size: u64) {
    let old_end = old_start + old_size as u64;
    with_address_space_mut(|space| {
        space.vmas.remove(old_start, old_end);
    });
}

pub fn copy_page_data(dst_page: u64, src_data: &[u8]) {
    let page_mask = !(PAGE_SIZE as u64 - 1);
    let aligned = dst_page & page_mask;
    if !is_user_address(aligned) {
        return;
    }
    if let Some(phys) = translate_addr(aligned) {
        let virt = active_physical_offset() + phys;
        let copy_len = src_data.len().min(PAGE_SIZE);
        unsafe {
            core::ptr::copy_nonoverlapping(src_data.as_ptr(), virt as *mut u8, copy_len);
        }
    }
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
        space.vmas.find_overlapping(start, end)
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
    let mut pending_unmap: Vec<(Frame4K, Option<(u64, u64)>, Option<(usize, u64)>)> = Vec::new();
    for page in Page::range_inclusive(start_page, end_page) {
        let addr = page.start_address().as_u64();
        let page_index = addr / PAGE_SIZE as u64;
        let space_id = current_space_id();
        remove_lru_mapping(space_id, page_index);
        swap_remove_page(space_id, page_index);
        if paging::translate_addr(page.start_address()).is_some() {
            let lock_arc__pt_guard = get_pt_lock(space_id);
let _pt_guard = lock_arc__pt_guard.lock();
            let unmap_result =
                paging::with_wp_disabled(|| paging::unmap_page(&mut mapper, page.start_address()));
            drop(_pt_guard);
            if let Ok(frame) = unmap_result {
                crate::cpu::smp::tlb_defer_shootdown();
                let phys = frame.start_address().as_u64();
                rmap::rmap_remove(phys, current_space_id(), addr);
                let current = frame_ownership::frame_refcount(phys);
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
                pending_unmap.push((frame, shared_anon_key, shared_file_key));
                if pending_unmap.len() >= TLB_BATCH_SIZE {
                    flush_unmap_batch(&mut pending_unmap);
                }
            }
        }
    }
    flush_unmap_batch(&mut pending_unmap);
    with_address_space_mut(|space| {
        space.vmas.remove_overlapping(start, end);
    });
    true
}

pub fn free_user_page_tables(pml4_phys: PhysAddr, space_id: u64) {
    let hhdm = active_physical_offset();
    let pml4_virt = VirtAddr::new(hhdm + pml4_phys.as_u64());
    let pml4 = unsafe { &*pml4_virt.as_ptr::<PageTable>() };

    let pml4_entry = &pml4[0];
    if pml4_entry.is_unused() {
        return;
    }
    let pdpt_phys = pml4_entry.addr();
    let pdpt_virt = VirtAddr::new(hhdm + pdpt_phys.as_u64());
    let pdpt = unsafe { &*pdpt_virt.as_ptr::<PageTable>() };

    let pdpt_entry = &pdpt[0];
    if !pdpt_entry.is_unused() && !pdpt_entry.flags().contains(PageTableFlags::HUGE_PAGE) {
        let pd_phys = pdpt_entry.addr();
        let pd_virt = VirtAddr::new(hhdm + pd_phys.as_u64());
        let pd = unsafe { &*pd_virt.as_ptr::<PageTable>() };

        for i in 0..512 {
            let pd_entry = &pd[i];
            if pd_entry.is_unused() {
                continue;
            }
            if pd_entry.flags().contains(PageTableFlags::HUGE_PAGE) {
                let huge_start = pd_entry.addr();
                let base_phys = huge_start.as_u64();
                let huge_virt_base = (i as u64) * (Size2MiB::SIZE as u64);
                for k in 0..512 {
                    let sub_phys = base_phys + k * PAGE_SIZE as u64;
                    let virt = huge_virt_base + k * PAGE_SIZE as u64;
                    let remaining = dec_frame_ref(sub_phys);
                    rmap::rmap_remove(sub_phys, space_id, virt);
                    if remaining == 0 {
                        deallocate_contiguous_frames(
                            PhysFrame::containing_address(PhysAddr::new(sub_phys)),
                            1,
                        );
                    }
                }
                continue;
            }
            let pt_phys = pd_entry.addr();
            let pt_virt = VirtAddr::new(hhdm + pt_phys.as_u64());
            let pt = unsafe { &*pt_virt.as_ptr::<PageTable>() };
            let pd_virt_base = (i as u64) * (Size2MiB::SIZE as u64);

            for j in 0..512 {
                let pte = &pt[j];
                if pte.is_unused() || !pte.flags().contains(PageTableFlags::PRESENT) {
                    continue;
                }
                let leaf_phys = pte.addr().as_u64();
                let virt = pd_virt_base + (j as u64) * PAGE_SIZE as u64;
                let new_count = dec_frame_ref(leaf_phys);
                rmap::rmap_remove(leaf_phys, space_id, virt);
                if new_count == 0 {
                    deallocate_contiguous_frames(
                        PhysFrame::containing_address(PhysAddr::new(leaf_phys)),
                        1,
                    );
                }
            }

            deallocate_contiguous_frames(PhysFrame::containing_address(pt_phys), 1);
        }
        deallocate_contiguous_frames(PhysFrame::containing_address(pd_phys), 1);
    }
    deallocate_contiguous_frames(PhysFrame::containing_address(pdpt_phys), 1);
    deallocate_contiguous_frames(PhysFrame::containing_address(pml4_phys), 1);
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
        let old_len = space.vmas.len();
        space.vmas.update_flags(start, end, flags);
        let regions = space.vmas.find_overlapping(start, end);
        for r in &regions {
            let r_start = r.start.max(start);
            let r_end = r.end.min(end);
            if r_start < r_end {
                space.vmas.remove(r_start, r_end);
                let mut cow_vma = r.clone();
                cow_vma.start = r_start;
                cow_vma.end = r_end;
                cow_vma.flags = flags;
                cow_vma.cow = true;
                cow_vma.shared = false;
                space.vmas.insert(cow_vma);
            }
        }
        space.vmas.len() != old_len
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
        space.vmas.insert(Vma {
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
            locked: false,
        })
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
        space.vmas.insert(Vma {
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
            locked: false,
        })
    })
}

pub fn user_region_overlaps(start: u64, size: u64) -> bool {
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
    with_address_space_ref(|space| {
        space.vmas.overlaps(region_start, region_end)
    })
}

pub fn user_stack_guards_region(start: u64, size: u64) -> bool {
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
    with_address_space_mut(|space| {
        let (_, stack_top) = ensure_stack_bounds(space, USER_STACK_PAGES);
        let guard_start = stack_top;
        let guard_end = USER_STACK_TOP;
        region_end > guard_start && region_start < guard_end
    })
}

pub fn user_heap_guards_region(start: u64, size: u64) -> bool {
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
    with_address_space_mut(|space| {
        let mmap_base = ensure_mmap_base(space);
        let guard_start = mmap_base.saturating_sub(PAGE_SIZE as u64);
        let guard_end = mmap_base;
        region_end > guard_start && region_start < guard_end
    })
}

pub fn handle_user_page_fault(addr: u64, error: PageFaultErrorCode) -> bool {
    let page_mask = !(PAGE_SIZE as u64 - 1);
    let aligned = addr & page_mask;
    static mut PF_LOG_COUNT: u32 = 0;
    let should_log = unsafe {
        PF_LOG_COUNT += 1;
        PF_LOG_COUNT <= 20
    };
    if should_log {
        crate::debug_diag!("[LAZY_PF] addr={:#x} aligned={:#x} err={:?}", addr, aligned, error);
    }
    if !is_user_address(aligned) {
        if should_log {
            crate::debug_diag!("[LAZY_PF] not user address");
        }
        return false;
    }

    // Lifecycle gate: increment active_faults, reject if DYING/DEAD
    let lifecycle = {
        let active = ACTIVE_ADDRESS_SPACE.lock();
        active.as_ref().and_then(|arc| {
            let guard = arc.read();
            if guard.id == 0 { None } else { Some(guard.lifecycle.clone()) }
        })
    };
    let Some(lc) = lifecycle else { return false; };
    if !lc.enter_fault() {
        return false;
    }
    let result = {
        if error.contains(PageFaultErrorCode::PROTECTION_VIOLATION) {
            if error.contains(PageFaultErrorCode::CAUSED_BY_WRITE) {
                if handle_cow_fault(aligned) {
                    true
                } else if handle_protection_fault(aligned, error) {
                    true
                } else {
                    false
                }
            } else {
                handle_protection_fault(aligned, error)
            }
        } else {
            handle_lazy_fault(aligned, should_log)
        }
    };
    lc.exit_fault();
    result
}

fn handle_lazy_fault(addr: u64, should_log: bool) -> bool {
    let vma = cils::find_vma_cils(addr).or_else(|| {
        with_address_space_ref(|space| {
            if should_log {
                crate::debug_diag!("[LAZY_FAULT] addr={:#x} vmas={}", addr, space.vmas.len());
            }
            let found = space.vmas.find(addr);
            if should_log {
                crate::debug_diag!("[LAZY_FAULT] result={}", found.is_some());
            }
            found
        })
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

fn handle_protection_fault(addr: u64, error: PageFaultErrorCode) -> bool {
    let vma = cils::find_vma_cils(addr).or_else(|| {
        with_address_space_ref(|space| space.vmas.find(addr))
    });
    let Some(vma) = vma else {
        crate::serial_println!("[PROT_FAULT] no VMA for {:#x}", addr);
        return false;
    };
    let space_id = current_space_id();
    let desired = vma_map_flags(&vma);
    let (level_4_frame, _) = Cr3::read();
    let phys_base = level_4_frame.start_address();
    let virt_base = VirtAddr::new(active_physical_offset() + phys_base.as_u64());
    let table = unsafe { &mut *(virt_base.as_mut_ptr()) };
    let mut mapper =
        unsafe { OffsetPageTable::new(table, VirtAddr::new(active_physical_offset())) };
    let frame_allocator = unsafe { global_memory_manager_mut() };
    let Some(frame_allocator) = frame_allocator else {
        return false;
    };
    let page = Page::containing_address(VirtAddr::new(addr));
    crate::serial_println!("[PROT_FAULT] upgrading flags for {:#x} vma_flags={:?}", addr, vma.flags);
    let lock_arc__pt_guard = get_pt_lock(space_id);
let _pt_guard = lock_arc__pt_guard.lock();
    update_page_flags_with_split(&mut mapper, frame_allocator, page, desired)
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
    mapper: &mut OffsetPageTable<'static>,
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
    let vmas = with_address_space_ref(|space| space.vmas.collect_all());
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
                if let Some(shared) = shared_pages.pages.get(&(*id, page_index)) {
                    let phys = shared.phys();
                    let frame = PhysFrame::containing_address(PhysAddr::new(phys));
                    let lock_arc__pt_guard = get_pt_lock(space_id);
let _pt_guard = lock_arc__pt_guard.lock();
                    let map_result = paging::with_wp_disabled(|| unsafe {
                        mapper.map_to_with_table_flags(
                            page,
                            frame,
                            map_flags,
                            table_flags,
                            frame_allocator,
                        )
                    });
                    match map_result {
                        Ok(flush) => {
                            flush.flush();
                            drop(_pt_guard);
                            core::mem::forget(shared.clone());
                            register_lru_mapping(addr, phys, region);
                            return true;
                        }
                        Err(_) => {
                            drop(_pt_guard);
                            return false;
                        }
                    };
                }
                let mut frame = match frame_allocator.allocate_user_frame() {
                    Some(frame) => frame,
                    None => return false,
                };
                let phys = frame.start_address().as_u64();
                frame.fill(0);
                    let lock_arc__pt_guard = get_pt_lock(space_id);
let _pt_guard = lock_arc__pt_guard.lock();
                    let map_result = paging::with_wp_disabled(|| unsafe {
                        mapper.map_to_with_table_flags(
                            page,
                            frame.as_phys_frame(),
                            map_flags,
                            table_flags,
                            frame_allocator,
                        )
                    });
                    match map_result {
                        Ok(flush) => {
                            flush.flush();
                            drop(_pt_guard);
                            let shared = frame.into_shared();
                            shared_pages.pages.insert((*id, page_index), shared);
                            register_lru_mapping(addr, phys, region);
                            return true;
                        }
                        Err(_) => {
                            drop(_pt_guard);
                            return false;
                        }
                    };
                }
            }
        }
    if let Some(data) = swap_take_page(space_id, page_start / PAGE_SIZE as u64) {
        let mut frame = match frame_allocator.allocate_user_frame() {
            Some(frame) => frame,
            None => {
                if !swap_store_page(space_id, page_start / PAGE_SIZE as u64, data) {
                    return false;
                }
                return false;
            }
        };
        let phys = frame.start_address().as_u64();
        let dst: &mut [u8] = &mut *frame;
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr(), dst.as_mut_ptr(), PAGE_SIZE);
        }
        let lock_arc__pt_guard = get_pt_lock(space_id);
let _pt_guard = lock_arc__pt_guard.lock();
        let map_result = paging::with_wp_disabled(|| unsafe {
            mapper.map_to_with_table_flags(page, frame.as_phys_frame(), map_flags, table_flags, frame_allocator)
        });
        match map_result {
            Ok(flush) => {
                flush.flush();
                drop(_pt_guard);
                frame.leak_as_shared();
                register_lru_mapping(addr, phys, region);
                return true;
            }
            Err(_) => {
                drop(_pt_guard);
                if !swap_store_page(space_id, page_start / PAGE_SIZE as u64, data) {
                    return false;
                }
                return false;
            }
        };
    }
    let lock_arc__thp_guard = get_pt_lock(space_id);
let _thp_guard = lock_arc__thp_guard.lock();
    let thp_ok = try_map_thp_anon(&mut mapper, frame_allocator, addr, region);
    drop(_thp_guard);
    if thp_ok {
        return true;
    }
    let zero_pfn = ZERO_PAGE_PFN.load(Ordering::Acquire);
    if zero_pfn != 0 && region.cow && !region.shared {
        let zero_frame = PhysFrame::containing_address(PhysAddr::new(zero_pfn));
        let lock_arc__pt_guard = get_pt_lock(space_id);
let _pt_guard = lock_arc__pt_guard.lock();
        let map_result = paging::with_wp_disabled(|| unsafe {
            mapper.map_to_with_table_flags(
                page,
                zero_frame,
                map_flags,
                table_flags,
                frame_allocator,
            )
        });
        match map_result {
            Ok(flush) => {
                flush.flush();
                drop(_pt_guard);
                register_lru_mapping(addr, zero_pfn, region);
                return true;
            }
            Err(_) => {
                drop(_pt_guard);
                return false;
            }
        }
    }
    let mut frame = match frame_allocator.allocate_user_frame() {
        Some(frame) => frame,
        None => return false,
    };
    let phys = frame.start_address().as_u64();
    frame.fill(0);
    let lock_arc__pt_guard = get_pt_lock(space_id);
let _pt_guard = lock_arc__pt_guard.lock();
    let map_result = paging::with_wp_disabled(|| unsafe {
        mapper.map_to_with_table_flags(page, frame.as_phys_frame(), map_flags, table_flags, frame_allocator)
    });
    match map_result {
        Ok(flush) => {
            flush.flush();
            drop(_pt_guard);
            frame.leak_as_shared();
            register_lru_mapping(addr, phys, region);
            true
        }
        Err(_) => {
            drop(_pt_guard);
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
    let mut frame = match frame_allocator.allocate_user_frame() {
        Some(frame) => frame,
        None => return false,
    };
    let phys = frame.start_address().as_u64();
    frame.fill(0);
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
        let dst: &mut [u8] = &mut *frame;
        let offset_in_page = (copy_start - page_start) as usize;
        unsafe {
            core::ptr::copy_nonoverlapping(src, dst.as_mut_ptr().add(offset_in_page), copy_len);
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
    let space_id = current_space_id();
    let lock_arc__pt_guard = get_pt_lock(space_id);
let _pt_guard = lock_arc__pt_guard.lock();
    let map_result = paging::with_wp_disabled(|| unsafe {
        mapper.map_to_with_table_flags(page, frame.as_phys_frame(), map_flags, table_flags, frame_allocator)
    });
    match map_result {
        Ok(flush) => {
            flush.flush();
            drop(_pt_guard);
            frame.leak_as_shared();
            register_lru_mapping(addr, phys, region);
            true
        }
        Err(e) => {
            drop(_pt_guard);
            crate::debug_diag!("[LAZY_FAULT] map_to failed for addr={:#x}: {:?}", addr, e);
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
    let key_inode = inode_key(inode);
    let ra_window = {
        let mut states = READAHEAD_STATES.lock();
        let ra = states.entry(key_inode).or_insert_with(|| ReadaheadState::new(page_index));
        if page_index == ra.prev_fault + 1 {
            ra.size = min(ra.size.saturating_mul(2), RA_MAX_SIZE);
        } else {
            ra.size = RA_INITIAL_SIZE;
        }
        ra.prev_fault = page_index;
        ra.size
    };
    if !region.shared {
        if let Some(data) = swap_take_page(space_id, page_start / PAGE_SIZE as u64) {
            let mut frame = match frame_allocator.allocate_user_frame() {
                Some(frame) => frame,
                None => {
                    if !swap_store_page(space_id, page_start / PAGE_SIZE as u64, data) {
                        return false;
                    }
                    return false;
                }
            };
            let phys = frame.start_address().as_u64();
            let dst: &mut [u8] = &mut *frame;
            unsafe {
                core::ptr::copy_nonoverlapping(data.as_ptr(), dst.as_mut_ptr(), PAGE_SIZE);
            }
            let lock_arc__pt_guard = get_pt_lock(space_id);
let _pt_guard = lock_arc__pt_guard.lock();
            let map_result = paging::with_wp_disabled(|| unsafe {
                mapper.map_to_with_table_flags(page, frame.as_phys_frame(), map_flags, table_flags, frame_allocator)
            });
            match map_result {
                Ok(flush) => {
                    flush.flush();
                    drop(_pt_guard);
                    frame.leak_as_shared();
                    do_file_fault_around(region, space_id, page_file_offset, file_end, map_flags, table_flags);
                    register_lru_mapping(addr, phys, region);
                    return true;
                }
                Err(_) => {
                    drop(_pt_guard);
                    if !swap_store_page(space_id, page_start / PAGE_SIZE as u64, data) {
                        return false;
                    }
                    return false;
                }
            };
        }
    }
    if region.shared {
        let key = (inode_key(inode), page_index);
        let mut shared_pages = SHARED_FILE_PAGES.lock();
        if let Some(shared) = shared_pages.pages.get(&key) {
            let phys = shared.phys();
            let frame = PhysFrame::containing_address(PhysAddr::new(phys));
            let lock_arc__pt_guard = get_pt_lock(space_id);
let _pt_guard = lock_arc__pt_guard.lock();
            let map_result = paging::with_wp_disabled(|| unsafe {
                mapper.map_to_with_table_flags(page, frame, map_flags, table_flags, frame_allocator)
            });
            match map_result {
                Ok(flush) => {
                    flush.flush();
                    drop(_pt_guard);
                    core::mem::forget(shared.clone());
                    if region.flags.contains(PageTableFlags::WRITABLE) {
                        mark_cache_dirty(key.0, key);
                    }
                    {
                        let mut s = READAHEAD_STATES.lock();
                        if let Some(ra) = s.get_mut(&key_inode) {
                            ra.mmap_miss = ra.mmap_miss.saturating_sub(1);
                        }
                    }
                    do_file_fault_around(region, space_id, page_file_offset, file_end, map_flags, table_flags);
                    register_lru_mapping(addr, phys, region);
                    return true;
                }
                Err(_) => {
                    drop(_pt_guard);
                    return false;
                }
            };
        }
        let mut frame = match frame_allocator.allocate_user_frame() {
            Some(frame) => frame,
            None => return false,
        };
        let phys = frame.start_address().as_u64();
        frame.fill(0);
        if page_file_offset < file_end {
            let data = match read_cached_file_page(inode, page_file_offset, file_end) {
                Some(value) => value,
                None => return false,
            };
            {
                let mut s = READAHEAD_STATES.lock();
                if let Some(ra) = s.get_mut(&key_inode) {
                    ra.mmap_miss = ra.mmap_miss.saturating_add(1);
                }
            }
            prefetch_file_pages(inode, page_file_offset, ra_window, file_end);
            let copy_len =
                min(PAGE_SIZE as u64, file_end.saturating_sub(page_file_offset)) as usize;
            if copy_len > 0 {
                let dst: &mut [u8] = &mut *frame;
                dst[..copy_len].copy_from_slice(&data[..copy_len]);
            }
        }
        let lock_arc__pt_guard = get_pt_lock(space_id);
let _pt_guard = lock_arc__pt_guard.lock();
        let map_result = paging::with_wp_disabled(|| unsafe {
            mapper.map_to_with_table_flags(page, frame.as_phys_frame(), map_flags, table_flags, frame_allocator)
        });
        match map_result {
            Ok(flush) => {
                flush.flush();
                drop(_pt_guard);
                let shared = frame.into_shared();
                shared_pages.pages.insert(key, shared);
                if region.flags.contains(PageTableFlags::WRITABLE) {
                    mark_cache_dirty(key.0, key);
                }
                do_file_fault_around(region, space_id, page_file_offset, file_end, map_flags, table_flags);
                register_lru_mapping(addr, phys, region);
                return true;
            }
            Err(_) => {
                drop(_pt_guard);
                return false;
            }
        };
    }
    let mut frame = match frame_allocator.allocate_user_frame() {
        Some(frame) => frame,
        None => return false,
    };
    let phys = frame.start_address().as_u64();
    frame.fill(0);
    if page_file_offset < file_end {
        let data = match read_cached_file_page(inode, page_file_offset, file_end) {
            Some(value) => value,
            None => return false,
        };
        {
            let mut s = READAHEAD_STATES.lock();
            if let Some(ra) = s.get_mut(&key_inode) {
                ra.mmap_miss = ra.mmap_miss.saturating_add(1);
            }
        }
        prefetch_file_pages(inode, page_file_offset, ra_window, file_end);
        let copy_len =
            min(PAGE_SIZE as u64, file_end.saturating_sub(page_file_offset)) as usize;
        if copy_len > 0 {
            let dst: &mut [u8] = &mut *frame;
            dst[..copy_len].copy_from_slice(&data[..copy_len]);
        }
    }
    let lock_arc__pt_guard = get_pt_lock(space_id);
let _pt_guard = lock_arc__pt_guard.lock();
    let map_result = paging::with_wp_disabled(|| unsafe {
        mapper.map_to_with_table_flags(page, frame.as_phys_frame(), map_flags, table_flags, frame_allocator)
    });
    match map_result {
        Ok(flush) => {
            flush.flush();
            drop(_pt_guard);
            if region.shared && region.flags.contains(PageTableFlags::WRITABLE) {
                let key = (inode_key(inode), page_index);
                mark_cache_dirty(key.0, key);
            }
            frame.leak_as_shared();
            do_file_fault_around(region, space_id, page_file_offset, file_end, map_flags, table_flags);
            register_lru_mapping(addr, phys, region);
            true
        }
        Err(_) => {
            drop(_pt_guard);
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
    let space_id = current_space_id();
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
    if !force_copy && frame_ownership::try_upgrade_unique(old_phys) {
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
        let lock_arc__pt_guard = get_pt_lock(space_id);
let _pt_guard = lock_arc__pt_guard.lock();
        if update_page_flags_with_split(&mut mapper, frame_allocator, page, map_flags) {
            drop(_pt_guard);
            register_lru_mapping(addr, old_phys, &region);
            return true;
        }
        drop(_pt_guard);
        return false;
    }
    let mut new_frame = match frame_allocator.allocate_user_frame() {
        Some(frame) => frame,
        None => return false,
    };
    let new_phys = new_frame.start_address().as_u64();
    let src = unsafe { core::ptr::addr_of!(*new_frame) as *const u8 };
    let src_vaddr = active_physical_offset().saturating_add(old_phys) as *const u8;
    unsafe {
        core::ptr::copy_nonoverlapping(src_vaddr, src as *mut u8, PAGE_SIZE);
    }
    let (level_4_frame, _) = Cr3::read();
    let phys_base = level_4_frame.start_address();
    let virt_base = VirtAddr::new(active_physical_offset() + phys_base.as_u64());
    let table = unsafe { &mut *(virt_base.as_mut_ptr()) };
    let mut mapper =
        unsafe { OffsetPageTable::new(table, VirtAddr::new(active_physical_offset())) };
    let page = Page::containing_address(VirtAddr::new(addr));
    let lock_arc__pt_guard = get_pt_lock(space_id);
let _pt_guard = lock_arc__pt_guard.lock();
    let unmap_result = paging::with_wp_disabled(|| mapper.unmap(page));
    if unmap_result.is_err() {
        drop(_pt_guard);
        return false;
    }
    let map_flags = flags
        | PageTableFlags::PRESENT
        | PageTableFlags::USER_ACCESSIBLE
        | PageTableFlags::WRITABLE;
    let table_flags =
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
    let map_result = paging::with_wp_disabled(|| unsafe {
        mapper.map_to_with_table_flags(page, new_frame.as_phys_frame(), map_flags, table_flags, frame_allocator)
    });
    match map_result {
        Ok(flush) => {
            flush.flush();
            drop(_pt_guard);
            crate::cpu::smp::tlb_defer_shootdown();
            crate::cpu::smp::tlb_flush_pending();
            let old_flags = frame_ownership::frame_flags(old_phys);
            if !old_flags.contains(frame_ownership::FrameFlags::REFCACHE) {
                drop(frame_ownership::SharedAtomicFrame::<[u8; 4096]>::from_phys_inner(old_phys));
            }
            register_lru_mapping(addr, new_phys, &region);
            core::mem::forget(new_frame.into_shared_refcache());
            true
        }
        Err(_) => {
            drop(_pt_guard);
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
    let hhdm = active_physical_offset();
    let in_kernel_text = start >= KERNEL_SPACE_START && end >= KERNEL_SPACE_START;
    let in_hhdm = hhdm != 0 && start >= hhdm && end >= hhdm;
    if in_kernel_text || in_hhdm {
        return true;
    }
    let start_page = Page::<Size4KiB>::containing_address(VirtAddr::new(start));
    let end_page = Page::<Size4KiB>::containing_address(VirtAddr::new(end));
    for page in Page::range_inclusive(start_page, end_page) {
        let Some(flags) = paging::translate_effective_flags(page.start_address()) else {
            return false;
        };
        if !flags.contains(PageTableFlags::PRESENT)
            || flags.contains(PageTableFlags::USER_ACCESSIBLE)
        {
            return false;
        }
    }
    true
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
    let mut base_phys: Option<u64> = None;
    let mut expected_next: Option<u64> = None;
    for page in Page::range_inclusive(start_page, end_page) {
        let phys = translate_addr(page.start_address().as_u64())?;
        match expected_next {
            None => {
                base_phys = Some(phys);
                expected_next = Some(phys.saturating_add(PAGE_SIZE as u64));
            }
            Some(expected) => {
                if phys != expected {
                    return None;
                }
                expected_next = Some(phys.saturating_add(PAGE_SIZE as u64));
            }
        }
    }
    let base = base_phys?;
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
                    devices: alloc::vec![dev],
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
            devices: alloc::vec![dev],
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

pub fn try_virt_to_phys(vaddr: usize) -> Option<usize> {
    translate_addr(vaddr as u64).map(|paddr| paddr as usize)
}

pub fn try_virt_to_phys_u64(vaddr: u64) -> Option<u64> {
    translate_addr(vaddr)
}

pub fn virt_to_phys(vaddr: usize) -> usize {
    match try_virt_to_phys(vaddr) {
        Some(paddr) => paddr as usize,
        None => {
            crate::serial_println!("[MEMORY] virt_to_phys failed for vaddr={:#x}", vaddr);
            0
        }
    }
}

/// u64 sanal adres için aşırı yükleme
pub fn virt_to_phys_u64(vaddr: u64) -> u64 {
    match try_virt_to_phys_u64(vaddr) {
        Some(paddr) => paddr,
        None => {
            crate::serial_println!("[MEMORY] virt_to_phys failed for vaddr={:#x}", vaddr);
            0
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
    let space_id = current_space_id();
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
    let lock_arc__pt_guard = get_pt_lock(space_id);
let _pt_guard = lock_arc__pt_guard.lock();
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

pub fn is_page_present(virt_addr: u64) -> bool {
    let page_mask = !(PAGE_SIZE as u64 - 1);
    let aligned = virt_addr & page_mask;
    if !is_user_address(aligned) {
        return false;
    }
    paging::translate_addr(VirtAddr::new(aligned)).is_some()
}

#[cfg(any(target_os = "none", target_os = "uefi"))]
fn resolve_user_phys_page_nofault(page_base: u64) -> Option<u64> {
    let page_mask = !(PAGE_SIZE as u64 - 1);
    let page_base = page_base & page_mask;
    if !is_user_address(page_base) {
        return None;
    }
    if let Some(phys) = translate_addr(page_base) {
        return Some(phys & page_mask);
    }
    if !handle_user_page_fault(page_base, PageFaultErrorCode::empty()) {
        return None;
    }
    translate_addr(page_base).map(|phys| phys & page_mask)
}

#[cfg(any(target_os = "none", target_os = "uefi"))]
pub fn copy_from_user_nofault(dst: &mut [u8], src_ptr: u64) -> bool {
    if dst.is_empty() {
        return true;
    }
    if !is_user_range(src_ptr, dst.len() as u64) {
        return false;
    }
    let page_mask = !(PAGE_SIZE as u64 - 1);
    let mut copied = 0usize;
    while copied < dst.len() {
        let virt = src_ptr.saturating_add(copied as u64);
        let page_base = virt & page_mask;
        let page_offset = (virt & !page_mask) as usize;
        let chunk = min(PAGE_SIZE.saturating_sub(page_offset), dst.len() - copied);
        let Some(phys_page_base) = resolve_user_phys_page_nofault(page_base) else {
            return false;
        };
        let src_phys = active_physical_offset()
            .saturating_add(phys_page_base)
            .saturating_add(page_offset as u64);
        unsafe {
            core::ptr::copy_nonoverlapping(
                src_phys as *const u8,
                dst[copied..].as_mut_ptr(),
                chunk,
            );
        }
        let Some(current_phys_page) = translate_addr(page_base).map(|phys| phys & page_mask) else {
            return false;
        };
        if current_phys_page != phys_page_base {
            return false;
        }
        copied = copied.saturating_add(chunk);
    }
    true
}

#[cfg(not(any(target_os = "none", target_os = "uefi")))]
pub fn copy_from_user_nofault(_dst: &mut [u8], _src_ptr: u64) -> bool {
    false
}

#[inline]
pub fn is_kernel_stack_virt_addr(addr: u64) -> bool {
    addr >= KERNEL_STACK_VIRT_BASE && addr < KERNEL_STACK_VIRT_LIMIT
}

pub fn map_kernel_stack_pages(phys_addr: u64, pages: usize) -> Option<u64> {
    if pages == 0 {
        return None;
    }
    let bytes = (pages as u64).checked_mul(PAGE_SIZE as u64)?;
    let stride = bytes.checked_add(PAGE_SIZE as u64)?;
    let virt_base = NEXT_KERNEL_STACK_VIRT.fetch_add(stride, Ordering::Relaxed);
    let virt_end = virt_base.checked_add(bytes)?;
    if virt_base < KERNEL_STACK_VIRT_BASE || virt_end > KERNEL_STACK_VIRT_LIMIT {
        crate::serial_println!(
            "[MEMORY] kernel stack VA exhausted base={:#x} pages={}",
            virt_base,
            pages
        );
        return None;
    }

    let frame_allocator = unsafe { global_memory_manager_mut()? };
    let (level_4_frame, _) = Cr3::read();
    let table_virt =
        VirtAddr::new(active_physical_offset() + level_4_frame.start_address().as_u64());
    let table = unsafe { &mut *(table_virt.as_mut_ptr::<PageTable>()) };
    let mut mapper =
        unsafe { OffsetPageTable::new(table, VirtAddr::new(active_physical_offset())) };
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;

    for page_idx in 0..pages {
        let virt = VirtAddr::new(virt_base + (page_idx as u64 * PAGE_SIZE as u64));
        let phys = PhysAddr::new(phys_addr + (page_idx as u64 * PAGE_SIZE as u64));
        let page = Page::<Size4KiB>::containing_address(virt);
        let frame = PhysFrame::<Size4KiB>::containing_address(phys);
        let result = paging::with_wp_disabled(|| unsafe {
            mapper.map_to(page, frame, flags, frame_allocator)
        });
        match result {
            Ok(flush) => flush.flush(),
            Err(err) => {
                crate::serial_println!(
                    "[MEMORY] map_kernel_stack_pages failed virt={:#x} phys={:#x} err={:?}",
                    virt.as_u64(),
                    phys.as_u64(),
                    err
                );
                let _ = unmap_kernel_stack_pages(virt_base, page_idx);
                return None;
            }
        }
    }
    Some(virt_base)
}

pub fn unmap_kernel_stack_pages(virt_addr: u64, pages: usize) -> bool {
    if pages == 0 {
        return true;
    }
    let (level_4_frame, _) = Cr3::read();
    let table_virt =
        VirtAddr::new(active_physical_offset() + level_4_frame.start_address().as_u64());
    let table = unsafe { &mut *(table_virt.as_mut_ptr::<PageTable>()) };
    let mut mapper =
        unsafe { OffsetPageTable::new(table, VirtAddr::new(active_physical_offset())) };
    let mut ok = true;
    for page_idx in 0..pages {
        let virt = VirtAddr::new(virt_addr + (page_idx as u64 * PAGE_SIZE as u64));
        if paging::translate_addr(virt).is_none() {
            continue;
        }
        let page = Page::<Size4KiB>::containing_address(virt);
        match paging::with_wp_disabled(|| mapper.unmap(page)) {
            Ok((_frame, flush)) => flush.flush(),
            Err(err) => {
                crate::serial_println!(
                    "[MEMORY] unmap_kernel_stack_pages failed virt={:#x} err={:?}",
                    virt.as_u64(),
                    err
                );
                ok = false;
            }
        }
    }
    ok
}

pub fn unmap_kernel_guard_page(virt_addr: u64) -> bool {
    let page = Page::<Size4KiB>::containing_address(VirtAddr::new(virt_addr));
    if paging::translate_addr(page.start_address()).is_none() {
        return true;
    }

    let frame_allocator = unsafe { global_memory_manager_mut() };
    let Some(frame_allocator) = frame_allocator else {
        crate::serial_println!(
            "[MEMORY] unmap_kernel_guard_page missing allocator virt={:#x}",
            virt_addr
        );
        return false;
    };

    let (level_4_frame, _) = Cr3::read();
    let table_virt =
        VirtAddr::new(active_physical_offset() + level_4_frame.start_address().as_u64());
    let table = unsafe { &mut *(table_virt.as_mut_ptr::<PageTable>()) };
    let mut mapper =
        unsafe { OffsetPageTable::new(table, VirtAddr::new(active_physical_offset())) };

    let result = paging::with_wp_disabled(|| paging::unmap_page(&mut mapper, page.start_address()));
    match result {
        Ok(_) => true,
        Err(UnmapError::ParentEntryHugePage) => {
            let split_flags =
                PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;
            if !split_huge_page(&mut mapper, frame_allocator, page, split_flags) {
                return false;
            }
            paging::with_wp_disabled(|| paging::unmap_page(&mut mapper, page.start_address()))
                .is_ok()
        }
        Err(err) => {
            crate::serial_println!(
                "[MEMORY] unmap_kernel_guard_page failed virt={:#x} err={:?}",
                virt_addr,
                err
            );
            false
        }
    }
}

pub fn remap_kernel_guard_page(virt_addr: u64, phys_addr: u64) -> bool {
    let page = Page::<Size4KiB>::containing_address(VirtAddr::new(virt_addr));
    let frame = PhysFrame::<Size4KiB>::containing_address(PhysAddr::new(phys_addr));
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;

    if paging::translate_addr(page.start_address()) == Some(frame.start_address()) {
        return paging::with_wp_disabled(|| unsafe {
            mapper_update_guard_flags(page, frame, flags)
        });
    }

    let frame_allocator = unsafe { global_memory_manager_mut() };
    let Some(frame_allocator) = frame_allocator else {
        crate::serial_println!(
            "[MEMORY] remap_kernel_guard_page missing allocator virt={:#x} phys={:#x}",
            virt_addr,
            phys_addr
        );
        return false;
    };

    let (level_4_frame, _) = Cr3::read();
    let table_virt =
        VirtAddr::new(active_physical_offset() + level_4_frame.start_address().as_u64());
    let table = unsafe { &mut *(table_virt.as_mut_ptr::<PageTable>()) };
    let mut mapper =
        unsafe { OffsetPageTable::new(table, VirtAddr::new(active_physical_offset())) };

    let map_result =
        paging::with_wp_disabled(|| unsafe { mapper.map_to(page, frame, flags, frame_allocator) });
    match map_result {
        Ok(flush) => {
            flush.flush();
            true
        }
        Err(MapToError::PageAlreadyMapped(_)) => {
            paging::with_wp_disabled(|| unsafe { mapper.update_flags(page, flags) })
                .map(|flush| {
                    flush.flush();
                    true
                })
                .unwrap_or(false)
        }
        Err(MapToError::ParentEntryHugePage) => {
            if !split_huge_page(&mut mapper, frame_allocator, page, flags) {
                return false;
            }
            let retry = paging::with_wp_disabled(|| unsafe {
                mapper.map_to(page, frame, flags, frame_allocator)
            });
            match retry {
                Ok(flush) => {
                    flush.flush();
                    true
                }
                Err(MapToError::PageAlreadyMapped(_)) => {
                    paging::translate_addr(page.start_address()) == Some(frame.start_address())
                }
                Err(err) => {
                    crate::serial_println!(
                        "[MEMORY] remap_kernel_guard_page retry failed virt={:#x} phys={:#x} err={:?}",
                        virt_addr,
                        phys_addr,
                        err
                    );
                    false
                }
            }
        }
        Err(err) => {
            crate::serial_println!(
                "[MEMORY] remap_kernel_guard_page failed virt={:#x} phys={:#x} err={:?}",
                virt_addr,
                phys_addr,
                err
            );
            false
        }
    }
}

fn mapper_update_guard_flags(
    page: Page<Size4KiB>,
    frame: PhysFrame<Size4KiB>,
    flags: PageTableFlags,
) -> bool {
    let (level_4_frame, _) = Cr3::read();
    let table_virt =
        VirtAddr::new(active_physical_offset() + level_4_frame.start_address().as_u64());
    let table = unsafe { &mut *(table_virt.as_mut_ptr::<PageTable>()) };
    let mut mapper =
        unsafe { OffsetPageTable::new(table, VirtAddr::new(active_physical_offset())) };
    match paging::translate_addr(page.start_address()) {
        Some(current) if current == frame.start_address() => {
            unsafe { mapper.update_flags(page, flags) }
                .map(|flush| {
                    flush.flush();
                    true
                })
                .unwrap_or(false)
        }
        _ => false,
    }
}

pub fn create_user_pml4() -> Option<PhysFrame> {
    let frame_allocator = unsafe { global_memory_manager_mut()? };
    let frame = frame_allocator.allocate_user_frame()?;
    let phys_offset = active_physical_offset();
    let frame_phys = frame.phys();
    let new_virt = VirtAddr::new(phys_offset + frame_phys);
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
    for index in 0..512 {
        if !new_table[index].is_unused() {
            continue;
        }
        if kernel_table[index].is_unused() {
            continue;
        }

        if index == 0 {
            // Allocate process-private Level 3 (PDPT)
            let user_pdpt_frame = match frame_allocator.allocate_user_frame() {
                Some(f) => f,
                None => return None,
            };
            let user_pdpt_phys = user_pdpt_frame.phys();
            let user_pdpt_virt = VirtAddr::new(phys_offset + user_pdpt_phys);
            let user_pdpt = unsafe { &mut *(user_pdpt_virt.as_mut_ptr::<PageTable>()) };
            unsafe {
                core::ptr::write_bytes(
                    user_pdpt as *mut PageTable as *mut u8,
                    0,
                    core::mem::size_of::<PageTable>(),
                );
            }
            core::mem::forget(user_pdpt_frame);

            // Find kernel PDPT
            let kernel_pdpt_frame: PhysFrame<Size4KiB> = PhysFrame::containing_address(kernel_table[0].addr());
            let kernel_pdpt_virt = VirtAddr::new(phys_offset + kernel_pdpt_frame.start_address().as_u64());
            let kernel_pdpt = unsafe { &*(kernel_pdpt_virt.as_ptr::<PageTable>()) };

            for i in 0..512 {
                if kernel_pdpt[i].is_unused() {
                    continue;
                }
                if i == 0 {
                    // Allocate process-private Level 2 (PD)
                    let user_pd_frame = match frame_allocator.allocate_user_frame() {
                        Some(f) => f,
                        None => return None,
                    };
                    let user_pd_phys = user_pd_frame.phys();
                    let user_pd_virt = VirtAddr::new(phys_offset + user_pd_phys);
                    let user_pd = unsafe { &mut *(user_pd_virt.as_mut_ptr::<PageTable>()) };
                    unsafe {
                        core::ptr::write_bytes(
                            user_pd as *mut PageTable as *mut u8,
                            0,
                            core::mem::size_of::<PageTable>(),
                        );
                    }
                    core::mem::forget(user_pd_frame);

                    // DO NOT clone kernel PD entries into the process-private PD.
                    // The kernel's identity mapping (PDPT[0] → PD[0..511]) maps
                    // physical memory at virtual 0-1GB. User-space ELF segments
                    // also live at virtual 0x400000+. If we clone the kernel's PD
                    // entries, user pages appear PRESENT but without USER_ACCESSIBLE,
                    // causing PROTECTION_VIOLATION on every CPL=3 access.
                    // Instead, leave the PD empty — the lazy page fault handler
                    // will create fresh page table entries with correct flags.

                    let mut pd_entry = user_pdpt[0].clone();
                    let orig_flags = kernel_pdpt[0].flags();
                    pd_entry.set_addr(
                        PhysAddr::new(user_pd_phys),
                        orig_flags | PageTableFlags::USER_ACCESSIBLE,
                    );
                    user_pdpt[0] = pd_entry;
                } else {
                    // Clone higher PDPT entries WITHOUT USER_ACCESSIBLE.
                    // The kernel code (loaded at ~0x7C500000 by UEFI) lives in
                    // PDPT[1+]. Without these entries, mov cr3 to the user PML4
                    // makes the kernel code inaccessible, so the CPU triple-faults
                    // immediately after the CR3 switch in enter_user_mode_with_ret.
                    let mut higher_entry = kernel_pdpt[i].clone();
                    higher_entry.set_flags(higher_entry.flags() & !PageTableFlags::USER_ACCESSIBLE);
                    user_pdpt[i] = higher_entry;
                }
            }

            let mut pml4_entry = new_table[0].clone();
            let orig_flags = kernel_table[0].flags();
            pml4_entry.set_addr(
                PhysAddr::new(user_pdpt_phys),
                orig_flags | PageTableFlags::USER_ACCESSIBLE,
            );
            new_table[0] = pml4_entry;
        } else {
            let mut entry = kernel_table[index].clone();
            let flags = entry.flags();
            if index >= 256 {
                entry.set_flags(flags & !PageTableFlags::USER_ACCESSIBLE);
            } else {
                entry.set_flags(flags | PageTableFlags::USER_ACCESSIBLE);
            }
            new_table[index] = entry;
        }
    }
    Some(frame.into_phys_frame())
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
        Mb2(&'a mut frame_allocator::Multiboot2FrameAllocator<'static>),
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
        Mb2(&'a mut frame_allocator::Multiboot2FrameAllocator<'static>),
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
            crate::serial_println!("[MI] using MemoryManager allocator");
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
    crate::serial_println!(
        "[MI] mapping phys={:#x} size={} via OffsetPageTable hhdm={:#x}",
        phys_addr,
        size,
        active_physical_offset()
    );
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
        crate::serial_println!(
            "[MI] page {:#x} mapped",
            page.start_address().as_u64()
        );
    }
    crate::serial_println!("[MI] map_identity done");
    true
}

fn split_huge_page(
    mapper: &mut OffsetPageTable<'static>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    page: Page<Size4KiB>,
    flags: PageTableFlags,
) -> bool {
    let huge_page = Page::<Size2MiB>::containing_address(page.start_address());
    let hhdm = active_physical_offset();
    let p4 = mapper.level_4_table();
    let p3_entry = &mut p4[huge_page.p4_index()];
    if !p3_entry.flags().contains(PageTableFlags::PRESENT) {
        return false;
    }
    let p3_ptr: *mut PageTable = VirtAddr::new(hhdm + p3_entry.addr().as_u64()).as_mut_ptr();
    let p3 = unsafe { &mut *p3_ptr };
    let p2_entry = &mut p3[huge_page.p3_index()];
    if !p2_entry.flags().contains(PageTableFlags::PRESENT)
        || !p2_entry.flags().contains(PageTableFlags::HUGE_PAGE)
    {
        return false;
    }
    let base = p2_entry.addr().as_u64();
    let Some(pt_frame) = frame_allocator.allocate_frame() else {
        return false;
    };
    let pt_ptr: *mut PageTable =
        VirtAddr::new(hhdm + pt_frame.start_address().as_u64()).as_mut_ptr();
    let pt = unsafe { &mut *pt_ptr };
    for (i, entry) in pt.iter_mut().enumerate() {
        let phys = PhysAddr::new(base + (i as u64) * PAGE_SIZE as u64);
        entry.set_frame(PhysFrame::containing_address(phys), flags);
    }
    let mut pd_flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
    if flags.contains(PageTableFlags::USER_ACCESSIBLE) {
        pd_flags |= PageTableFlags::USER_ACCESSIBLE;
    }
    p2_entry.set_frame(pt_frame, pd_flags);
    x86_64::instructions::tlb::flush(huge_page.start_address());
    if frame_ownership::is_folio_head(base) {
        folio::folio_unregister(base);
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

pub fn msync_user_range(start: u64, end: u64, _invalidate: bool) {
    let regions = with_address_space_ref(|space| {
        space
            .vmas
            .iter()
            .filter(|r| end > r.start && start < r.end && r.shared)
            .cloned()
            .collect::<Vec<_>>()
    });
    for region in &regions {
        let overlap_start = start.max(region.start);
        let overlap_end = end.min(region.end);
        if overlap_start < overlap_end {
            writeback_file_range(region, overlap_start, overlap_end);
        }
    }
}

pub fn flush_dirty_file_pages() {
    let dirty_keys: Vec<(usize, u64)> = {
        let cache = PAGE_CACHE.lock();
        cache
            .entries
            .iter()
            .filter(|(_, entry)| entry.dirty)
            .map(|(key, _)| *key)
            .collect()
    };
    for (inode_key_val, page_index) in dirty_keys {
        let file_offset = page_index * PAGE_SIZE as u64;
        let max_len = PAGE_SIZE;
        let buf = {
            let cache = PAGE_CACHE.lock();
            match cache.entries.get(&(inode_key_val, page_index)) {
                Some(entry) => entry.data[..max_len].to_vec(),
                None => continue,
            }
        };
        let regions = with_address_space_ref(|space| {
            space
                .vmas
                .iter()
                .filter(|r| matches!(&r.kind, VmaKind::File { .. } if r.shared))
                .cloned()
                .collect::<Vec<_>>()
        });
        for region in &regions {
            if let VmaKind::File { inode, .. } = &region.kind {
                if inode_key(inode) == inode_key_val {
                    let _ = vfs_write_at(inode, file_offset as usize, &buf);
                    mark_cache_clean(inode_key_val, (inode_key_val, page_index));
                    break;
                }
            }
        }
    }
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
fn uefi_enable_nxe_if_supported() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        use core::arch::x86_64::__cpuid;
        use x86_64::registers::model_specific::Msr;

        const CPUID_EXTENDED_MAX: u32 = 0x8000_0000;
        const CPUID_EXTENDED_FEATURES: u32 = 0x8000_0001;
        const CPUID_NX_EDX_BIT: u32 = 1 << 20;
        const MSR_EFER: u32 = 0xC000_0080;
        const EFER_NXE: u64 = 1 << 11;

        let max_extended = unsafe { __cpuid(CPUID_EXTENDED_MAX).eax };
        if max_extended < CPUID_EXTENDED_FEATURES {
            crate::serial_println!("[HHDM] NXE unavailable: extended CPUID leaf missing");
            return false;
        }
        let features = unsafe { __cpuid(CPUID_EXTENDED_FEATURES) };
        if (features.edx & CPUID_NX_EDX_BIT) == 0 {
            crate::serial_println!("[HHDM] NXE unavailable: CPUID NX bit clear");
            return false;
        }

        unsafe {
            let mut efer = Msr::new(MSR_EFER);
            let current = efer.read();
            if (current & EFER_NXE) == 0 {
                efer.write(current | EFER_NXE);
                crate::serial_println!("[HHDM] EFER.NXE enabled before NX page-table entries");
            }
        }
        true
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
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
    mapper: &mut OffsetPageTable<'static>,
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
    let nxe_enabled = uefi_enable_nxe_if_supported();
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
        if !exec_allowed && nxe_enabled {
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
    let mut mmio_flags = PageTableFlags::PRESENT
        | PageTableFlags::WRITABLE
        | PageTableFlags::NO_CACHE
        | PageTableFlags::WRITE_THROUGH;
    if nxe_enabled {
        mmio_flags |= PageTableFlags::NO_EXECUTE;
    }
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

/// Boot bellek haritasındaki bölgeleri `offset` üzerinde eşler (HHDM).
///
/// Limine ve MB2 yollarında çekirdek giriş kodu (entry.S) kendi sayfa
/// tablolarını yükler; bu, Limine'nin sağladığı HHDM eşlemesini yok eder ve
/// MB2'de hiçbir ofset eşlemesi var olmaz. `phys_to_virt` ile ACPI handler'ları
/// (RSDP/RSDT/XSDT/MADT okumaları) bu ofset üzerinden eriştiği için eşleme
/// boot sırasında boot bellek haritasından yeniden kurulur.
///
/// 4 KiB'lık başlık/kuyruk ve 2 MiB'lık gövde eşlemesi kullanır; zaten eşlenmiş
/// sayfalar hata sayılmaz. Başarıyla eşlenen toplam bayt sayısını döner.
pub fn map_physical_regions_hhdm<A: FrameAllocator<Size4KiB>>(
    mapper: &mut (impl MapperAllSizes + Translate),
    frame_allocator: &mut A,
    offset: u64,
    regions: &[(u64, u64)],
    flags: PageTableFlags,
) -> u64 {
    // Güvenlik: MB2 girişinde PML4[0] (identity) ve PML4[256] (HHDM penceresi)
    // AYNI pdpt_table/pd_table'ı paylaşır. Pencere üzerinde huge page
    // split/unmap yapmak, üzerinde çalışılan identity eşlemesini de kaldırır
    // (triple fault). Bu yüzden huge page zaten mevcutsa o aralık
    // "kapsanmış" sayılır: pencere eşlemesi offset+phys olduğu için mevcut
    // huge page, istenen fiziksel aralığı birebir karşılar.
    let huge_size = Size2MiB::SIZE;
    let page_size = Size4KiB::SIZE;
    let mut mapped = 0u64;
    for (base, size) in regions {
        if *size == 0 {
            continue;
        }
        let mut current = *base;
        let end = base.saturating_add(*size);

        while current < end && (current % huge_size) != 0 {
            let virt = VirtAddr::new(offset + current);
            let phys = PhysAddr::new(current);
            let map_result: Result<MapperFlush<Size4KiB>, MapToError<Size4KiB>> =
                paging::map_page(mapper, frame_allocator, virt, phys, flags);
            match map_result {
                Ok(flush) => flush.flush(),
                Err(MapToError::ParentEntryHugePage) => {}
                Err(MapToError::PageAlreadyMapped(_)) => {}
                Err(err) => {
                    crate::serial_println!(
                        "[HHDM] map 4K failed virt=0x{:x} err={:?}",
                        virt.as_u64(),
                        err
                    );
                    return mapped;
                }
            }
            current = current.saturating_add(page_size);
            mapped = mapped.saturating_add(page_size);
        }

        while current + huge_size <= end {
            let virt = VirtAddr::new(offset + current);
            let phys = PhysAddr::new(current);
            let page = Page::<Size2MiB>::containing_address(virt);
            let frame = PhysFrame::<Size2MiB>::containing_address(phys);
            match unsafe { mapper.map_to(page, frame, flags, frame_allocator) } {
                Ok(flush) => flush.flush(),
                Err(MapToError::PageAlreadyMapped(_)) => {}
                Err(MapToError::ParentEntryHugePage) => {}
                Err(err) => {
                    crate::serial_println!(
                        "[HHDM] map 2M failed virt=0x{:x} err={:?}",
                        virt.as_u64(),
                        err
                    );
                    return mapped;
                }
            }
            current = current.saturating_add(huge_size);
            mapped = mapped.saturating_add(huge_size);
        }

        while current < end {
            let virt = VirtAddr::new(offset + current);
            let phys = PhysAddr::new(current);
            let map_result: Result<MapperFlush<Size4KiB>, MapToError<Size4KiB>> =
                paging::map_page(mapper, frame_allocator, virt, phys, flags);
            match map_result {
                Ok(flush) => flush.flush(),
                Err(MapToError::ParentEntryHugePage) => {}
                Err(MapToError::PageAlreadyMapped(_)) => {}
                Err(err) => {
                    crate::serial_println!(
                        "[HHDM] map 4K tail failed virt=0x{:x} err={:?}",
                        virt.as_u64(),
                        err
                    );
                    return mapped;
                }
            }
            current = current.saturating_add(page_size);
            mapped = mapped.saturating_add(page_size);
        }
    }
    mapped
}

/// Map a BIOS adapter's reserved low-memory window into the HHDM, splitting a
/// pre-existing 2 MiB parent when the requested page is not actually present.
/// Limine's entry tables can leave a sparse low-memory parent; treating
/// `ParentEntryHugePage` as already mapped (the MB2-safe behavior above) would
/// otherwise hide an RSDP page fault.
#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
pub fn map_low_physical_hhdm<A: FrameAllocator<Size4KiB>>(
    mapper: &mut OffsetPageTable<'static>,
    frame_allocator: &mut A,
    offset: u64,
    start: u64,
    size: u64,
    flags: PageTableFlags,
) -> u64 {
    let end = start.saturating_add(size);
    let mut current = start & !(PAGE_SIZE as u64 - 1);
    let mut mapped = 0u64;
    while current < end {
        let virt = VirtAddr::new(offset.saturating_add(current));
        let page = Page::<Size4KiB>::containing_address(virt);
        let result = paging::map_page(
            mapper,
            frame_allocator,
            virt,
            PhysAddr::new(current),
            flags,
        );
        match result {
            Ok(flush) => {
                flush.flush();
                mapped = mapped.saturating_add(PAGE_SIZE as u64);
            }
            Err(MapToError::PageAlreadyMapped(_)) => {}
            Err(MapToError::ParentEntryHugePage) => {
                if split_huge_page(mapper, frame_allocator, page, flags) {
                    if let Ok(flush) = paging::map_page(
                        mapper,
                        frame_allocator,
                        virt,
                        PhysAddr::new(current),
                        flags,
                    ) {
                        flush.flush();
                        mapped = mapped.saturating_add(PAGE_SIZE as u64);
                    }
                } else {
                    crate::serial_println!(
                        "[HHDM] low-memory split failed virt={:#x}",
                        virt.as_u64()
                    );
                    break;
                }
            }
            Err(err) => {
                crate::serial_println!(
                    "[HHDM] low-memory map failed virt={:#x} err={:?}",
                    virt.as_u64(),
                    err
                );
                break;
            }
        }
        current = current.saturating_add(PAGE_SIZE as u64);
    }
    mapped
}

#[cfg(target_os = "uefi")]
fn map_hhdm_range_uefi<A>(
    mapper: &mut OffsetPageTable<'static>,
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

#[cfg(test)]
mod tests {
    use super::{
        PAGE_SIZE, USER_STACK_BYTES, USER_STACK_GUARD_PAGES, USER_STACK_PAGES,
        USER_STACK_USABLE_BYTES,
    };

    #[test]
    fn user_stack_contract_has_guard_and_interactive_shell_headroom() {
        assert_eq!(USER_STACK_BYTES, 1024 * 1024);
        assert_eq!(USER_STACK_GUARD_PAGES, 1);
        assert_eq!(USER_STACK_USABLE_BYTES, 1020 * 1024);
        assert_eq!(
            USER_STACK_BYTES,
            (USER_STACK_PAGES as u64) * (PAGE_SIZE as u64)
        );
        assert_eq!(
            USER_STACK_USABLE_BYTES,
            ((USER_STACK_PAGES - USER_STACK_GUARD_PAGES) as u64) * (PAGE_SIZE as u64)
        );
        assert!(USER_STACK_USABLE_BYTES >= 4 * 214 * 1024);
    }
}

#[cfg(test)]
mod cils_tests {
    use alloc::sync::Arc;
    use alloc::vec::Vec;
    use core::sync::atomic::AtomicU32;
use spin::Mutex;
use spin::RwLock;

use super::cils;
use super::vma::{Vma, VmaKind, VmaMap, Node};
use super::{create_address_space, set_active_address_space, PAGE_SIZE as TEST_PAGE_SIZE};

    /// Helper: build a simple Vma.
    fn vma(start: u64, end: u64, kind: VmaKind) -> Vma {
        Vma {
            start,
            end,
            flags: crate::memory::enforce_wx(
                crate::memory::PageTableFlags::USER_ACCESSIBLE
            ),
            kind,
            cow: false,
            shared: false,
            locked: false,
        }
    }

    #[test]
    fn cils_basic_lookup() {
        let space = create_address_space(&[]);
        let sid = space.read().id;

        // Insert VMAs.
        {
            let mut guard = space.write();
            guard.vmas.insert(vma(0x10000, 0x20000, VmaKind::Anonymous { id: 1 }));
            guard.vmas.insert(vma(0x30000, 0x40000, VmaKind::Anonymous { id: 2 }));
            guard.vmas.insert(vma(0x50000, 0x60000, VmaKind::Anonymous { id: 3 }));
            let mut retired = guard.vmas.drain_retired();
            drop(guard);
            // No RCU needed in test — we own exclusive access.
            for &addr in &retired {
                if addr != 0 {
                    VmaMap::free_retired(addr as *mut Node);
                }
            }
        }

        // CILS register + activate.
        cils::register(sid, &space.read().vmas);
        set_active_address_space(Some(space.clone()));

        // Lookup existing.
        let found = cils::find_vma_cils(0x15000);
        assert!(found.is_some());
        assert_eq!(found.unwrap().start, 0x10000);

        let found = cils::find_vma_cils(0x35000);
        assert!(found.is_some());
        assert_eq!(found.unwrap().start, 0x30000);

        // Lookup gap — should miss.
        assert!(cils::find_vma_cils(0x25000).is_none());
        assert!(cils::find_vma_cils(0x00000).is_none());
        assert!(cils::find_vma_cils(u64::MAX).is_none());

        // Cleanup.
        set_active_address_space(None);
        cils::unregister(sid);
    }

    #[test]
    fn cils_sees_new_insertions() {
        let space = create_address_space(&[]);
        let sid = space.read().id;

        // Register first (empty map).
        cils::register(sid, &space.read().vmas);
        set_active_address_space(Some(space.clone()));

        // Insert after registration.
        {
            let mut guard = space.write();
            guard.vmas.insert(vma(0x10000, 0x20000, VmaKind::Anonymous { id: 1 }));
            let mut retired = guard.vmas.drain_retired();
            drop(guard);
            for &addr in &retired {
                if addr != 0 {
                    VmaMap::free_retired(addr as *mut Node);
                }
            }
        }

        // CILS reader must see the newly inserted VMA.
        let found = cils::find_vma_cils(0x18000);
        assert!(found.is_some(), "CILS should see post-registration insert");
        assert_eq!(found.unwrap().start, 0x10000);

        set_active_address_space(None);
        cils::unregister(sid);
    }

    #[test]
    fn cils_after_unregister_returns_none() {
        let space = create_address_space(&[]);
        let sid = space.read().id;

        {
            let mut guard = space.write();
            guard.vmas.insert(vma(0x10000, 0x20000, VmaKind::Anonymous { id: 1 }));
            let mut retired = guard.vmas.drain_retired();
            drop(guard);
            for &addr in &retired {
                if addr != 0 {
                    VmaMap::free_retired(addr as *mut Node);
                }
            }
        }

        cils::register(sid, &space.read().vmas);
        set_active_address_space(Some(space.clone()));

        // Verify it works.
        assert!(cils::find_vma_cils(0x15000).is_some());

        // Unregister.
        cils::unregister(sid);

        // After unregister, lookup should return None.
        assert!(
            cils::find_vma_cils(0x15000).is_none(),
            "find_vma_cils should return None after unregister"
        );

        set_active_address_space(None);
    }

    #[test]
    fn cils_retired_collection() {
        // Test that unlink_node pushes to retired and drain_retired collects them.
        let mut map = VmaMap::new();

        // Insert several VMAs.
        map.insert(vma(0x10000, 0x20000, VmaKind::Anonymous { id: 1 }));
        map.insert(vma(0x20000, 0x30000, VmaKind::Anonymous { id: 2 }));
        map.insert(vma(0x30000, 0x40000, VmaKind::Anonymous { id: 3 }));
        assert_eq!(map.len(), 3);

        // Remove middle VMA.  This creates a non‑mergeable gap so only one
        // node is unlinked (the middle one).
        map.remove(0x20000, 0x30000);
        assert_eq!(map.len(), 2);

        let retired = map.drain_retired();
        // The removed node address must be non-zero.
        assert!(!retired.is_empty(), "retired list should not be empty after remove");
        assert!(retired.iter().all(|&a| a != 0), "retired addresses must be non-zero");

        // Clean up the retired nodes (in real code reclaim_retired does this
        // after an RCU grace period).
        for &addr in &retired {
            VmaMap::free_retired(addr as *mut Node);
        }

        // The retired list is now drained.
        assert!(map.drain_retired().is_empty());
    }

    #[test]
    fn cils_remove_accumulates_multiple_retired() {
        let mut map = VmaMap::new();
        map.insert(vma(0x10000, 0x20000, VmaKind::Anonymous { id: 1 }));
        map.insert(vma(0x30000, 0x40000, VmaKind::Anonymous { id: 2 }));
        map.insert(vma(0x50000, 0x60000, VmaKind::Anonymous { id: 3 }));
        assert_eq!(map.len(), 3);

        // Remove two non-overlapping ranges → two retired nodes.
        map.remove(0x10000, 0x20000);
        map.remove(0x50000, 0x60000);
        assert_eq!(map.len(), 1);

        let retired = map.drain_retired();
        assert_eq!(retired.len(), 2, "two removes should produce two retired entries");

        for &addr in &retired {
            VmaMap::free_retired(addr as *mut Node);
        }
    }

    #[test]
    fn cils_merge_generates_retired() {
        let mut map = VmaMap::new();

        // Insert two adjacent VMAs with same properties → they merge at insert time.
        map.insert(vma(0x10000, 0x20000, VmaKind::Anonymous { id: 1 }));
        map.insert(vma(0x20000, 0x30000, VmaKind::Anonymous { id: 1 }));
        // The second insert triggers maybe_merge_around, which can unlink
        // one of the two nodes (the adjacent one gets merged into the other).
        // After merge there should be 1 VMA covering 0x10000..0x30000.
        assert_eq!(map.len(), 1);

        let retired = map.drain_retired();
        // Either the first or second insert caused a merge (≥ 1 retired).
        assert!(!retired.is_empty(), "merge should generate retired nodes");

        for &addr in &retired {
            VmaMap::free_retired(addr as *mut Node);
        }
    }

    #[test]
    fn cils_clear_frees_retired() {
        // clear() must free both linked and retired nodes without double-free.
        let mut map = VmaMap::new();
        map.insert(vma(0x10000, 0x20000, VmaKind::Anonymous { id: 1 }));
        map.insert(vma(0x20000, 0x30000, VmaKind::Anonymous { id: 2 }));

        map.remove(0x10000, 0x20000);
        // Now 1 linked node, 1 retired node.

        // clear() frees everything.
        map.clear();
        assert_eq!(map.len(), 0);
        // drain_retired after clear must be empty (retired list cleared).
        assert!(map.drain_retired().is_empty());
    }

    // ── Concurrent Lock / Swap tests ──────────────────────────────

    #[test]
    fn cils_lock_swap_basic() {
        // Basic Lock + Swap: insert [0x100,0x200), then lock and swap in
        // a sub‑range [0x120,0x180) as two pieces.
        let mut map = VmaMap::new();
        map.insert(vma(0x100, 0x200, VmaKind::Anonymous { id: 1 }));
        let retired = map.drain_retired();
        for &addr in &retired { if addr != 0 { VmaMap::free_retired(addr as *mut Node); } }

        let guard = map.lock_interval(0x120, 0x180);

        // Build new nodes: [0x100,0x120) + [0x180,0x200)
        let left = Node::new(
            vma(0x100, 0x120, VmaKind::Anonymous { id: 1 }),
            1,
        );
        let right = Node::new(
            vma(0x180, 0x200, VmaKind::Anonymous { id: 1 }),
            1,
        );
        let new_nodes = alloc::vec![left, right];

        map.swap_interval(guard, new_nodes);

        // After swap, we should have 2 VMAs
        assert_eq!(map.len(), 2);

        // CILS reader should see gaps correctly
        let _retired = map.drain_retired();
        for &addr in &_retired {
            if addr != 0 {
                VmaMap::free_retired(addr as *mut Node);
            }
        }
    }

    #[test]
    fn cils_non_overlapping_parallel_lock() {
        // Two non‑overlapping intervals should be lockable concurrently.
        // Interval A: [0x100,0x200)
        // Interval B: [0x300,0x400)
        // We spawn two threads that each lock+swap their interval.
        let mut inner = VmaMap::new();
        inner.insert(vma(0x100, 0x200, VmaKind::Anonymous { id: 1 }));
        inner.insert(vma(0x300, 0x400, VmaKind::Anonymous { id: 2 }));
        let map = Arc::new(inner);

        let map_a = map.clone();
        let map_b = map.clone();

        // Add VMICheck-style sleep-free parallel dispatch:
        // Both threads try to lock+swap their intervals simultaneously.
        let t1 = std::thread::spawn(move || {
            let guard = map_a.lock_interval(0x100, 0x200);
            // Swap: remove [0x100,0x200) entirely → empty replacement
            let new_nodes = alloc::vec![];
            map_a.swap_interval(guard, new_nodes);
        });

        let t2 = std::thread::spawn(move || {
            let guard = map_b.lock_interval(0x300, 0x400);
            let new_nodes = alloc::vec![];
            map_b.swap_interval(guard, new_nodes);
        });

        t1.join().expect("thread 1 panicked");
        t2.join().expect("thread 2 panicked");

        // Both intervals removed → map should be empty
        assert_eq!(map.len(), 0);
        let _retired = map.drain_retired();
    }

    #[test]
    fn cils_overlapping_lock_serialises() {
        // Two overlapping Lock attempts on the same interval must be
        // serialised (one completes before the other starts, or they
        // don't both succeed on the same data).
        let mut inner = VmaMap::new();
        inner.insert(vma(0x100, 0x300, VmaKind::Anonymous { id: 1 }));
        let map = Arc::new(inner);

        let map_a = map.clone();
        let map_b = map.clone();

        let done1 = Arc::new(core::sync::atomic::AtomicBool::new(false));
        let done2 = Arc::new(core::sync::atomic::AtomicBool::new(false));
        let d1 = done1.clone();
        let d2 = done2.clone();

        let t1 = std::thread::spawn(move || {
            // Lock the full [0x100,0x300)
            let guard = map_a.lock_interval(0x100, 0x300);
            // Signal that T1 got the lock
            d1.store(true, core::sync::atomic::Ordering::Release);
            // Busy-wait a bit to stress concurrency
            for _ in 0..1000 { core::hint::spin_loop(); }
            // Swap in a split: [0x100,0x200) + [0x200,0x300)
            let left = Node::new(
                vma(0x100, 0x200, VmaKind::Anonymous { id: 1 }),
                1,
            );
            let right = Node::new(
                vma(0x200, 0x300, VmaKind::Anonymous { id: 1 }),
                1,
            );
            let new_nodes = alloc::vec![left, right];
            map_a.swap_interval(guard, new_nodes);
        });

        let t2 = std::thread::spawn(move || {
            // Thread 2 tries to lock a sub‑interval [0x140,0x260)
            // which overlaps with T1's lock.  This must block until T1
            // releases the lock (via swap_interval → unlocks pred).
            let guard = map_b.lock_interval(0x140, 0x260);
            // Swap: remove the sub‑interval from the map
            let new_nodes = alloc::vec![];
            map_b.swap_interval(guard, new_nodes);
            d2.store(true, core::sync::atomic::Ordering::Release);
        });

        t1.join().expect("thread 1 panicked");
        t2.join().expect("thread 2 panicked");

        // Both threads completed
        assert!(done1.load(core::sync::atomic::Ordering::Acquire));
        assert!(done2.load(core::sync::atomic::Ordering::Acquire));
        // CILS lock_interval locks from preds[0].next forward.
        // Depending on which thread wins the race:
        //   T1 first → pred=[0x100,0x200), old=[0x200,0x300)] → len=1
        //   T2 first → map empty after T2, then T1 inserts 2 → len=2
        // Both are correct — the key assertion is that both threads
        // completed without deadlock.
        assert!(map.len() == 1 || map.len() == 2,
            "len should be 1 (T1 won) or 2 (T2 won), got {}", map.len());
        let _retired = map.drain_retired();
    }

    #[test]
    fn cils_concurrent_read_write_stress() {
        // Stress-test concurrent CILS readers (RCU path) vs VmaMap writers
        // (AddressSpace RwLock write path).  Readers hold RCU read locks;
        // writers hold the AddressSpace write lock + Lock/Swap protocol.
        use std::sync::Barrier;

        const NUM_VMAS: u64 = 100;
        const NUM_READERS: u64 = 4;
        const NUM_WRITERS: u64 = 2;
        const ITERS_PER_THREAD: u64 = 500;

        let space = create_address_space(&[]);
        let sid = space.read().id;

        // Insert initial VMAs.
        {
            let mut guard = space.write();
            for i in 0..NUM_VMAS {
                let start = 0x1000 * (i as u64) + 0x1000;
                let end = start + 0x800;
                guard.vmas.insert(vma(start, end, VmaKind::Anonymous { id: i as u64 }));
            }
            let mut retired = guard.vmas.drain_retired();
            drop(guard);
            for &addr in &retired {
                if addr != 0 {
                    VmaMap::free_retired(addr as *mut Node);
                }
            }
        }

        cils::register(sid, &space.read().vmas);
        set_active_address_space(Some(space.clone()));

        let barrier = Arc::new(Barrier::new((NUM_READERS + NUM_WRITERS) as usize));
        let mut handles = Vec::new();

        // Reader threads — each does find_vma_cils in a loop.
        for _ in 0..NUM_READERS {
            let bar = barrier.clone();
            handles.push(std::thread::spawn(move || {
                bar.wait();
                let mut hits = 0u64;
                for _ in 0..ITERS_PER_THREAD {
                    let addr = (rand::random::<u64>() % (NUM_VMAS * 0x1000)) + 0x1000;
                    if cils::find_vma_cils(addr).is_some() {
                        hits += 1;
                    }
                }
                hits
            }));
        }

        // Writer threads — each does remove + re-insert in a loop.
        for w in 0..NUM_WRITERS {
            let bar = barrier.clone();
            let space_w = space.clone();
            handles.push(std::thread::spawn(move || {
                bar.wait();
                for i in 0..ITERS_PER_THREAD {
                    let idx = (i as u64 + w as u64) % NUM_VMAS;
                    let start = 0x1000 * idx + 0x1000;
                    let end = start + 0x800;

                    // Remove overlapping intervals.
                    {
                        let mut guard = space_w.write();
                        guard.vmas.remove_overlapping(start, end);
                    }
                    // Re-insert.
                    {
                        let mut guard = space_w.write();
                        guard.vmas
                            .insert(vma(start, end, VmaKind::Anonymous { id: idx as u64 }));
                    }
                    // NOTE: retired nodes accumulate until the concurrent
                    // phase ends — we must NOT free them here because readers
                    // may still reference them via RCU (test-only, the normal
                    // production path waits for a grace period via reclaim_retired).
                }
                0u64
            }));
        }

        let total_hits: u64 = handles
            .into_iter()
            .take(NUM_READERS as usize)
            .filter_map(|h| h.join().ok())
            .sum();

        set_active_address_space(None);
        cils::unregister(sid);

        assert!(
            total_hits > 0,
            "cils_concurrent_read_write_stress: readers found zero VMAs \
             under concurrent write load — CILS reader or RCU path broken"
        );
    }
}
