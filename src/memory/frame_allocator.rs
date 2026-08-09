#![cfg(not(target_os = "uefi"))]
//! # Multiboot2 / Limine Fiziksel Frame Ayırıcısı
//!
//! Kernel önyükleme aşamasına özel, ileri-doğru büyüyen (bump) ayırıcı.
//!
//! ## Neden Bump Ayırıcı?
//!
//! Kernel başlarken henüz yığın (heap) veya PMM hazır değildir.
//! Bu aşamada yalnızca sıralı frame tahsis yeterlidir:
//!
//! ```
//! Multiboot2 Bellek Haritası:
//!
//!  [Bölge 1: Available] → [Bölge 2: Reserved] → [Bölge 3: Available] → ...
//!
//!  next_frame işaretçisi ileri doğru ilerler:
//!
//!  │ frame[0] │ frame[1] │ frame[2] │ frame[3] │ ... │
//!  │ kernel   │ kernel   │ ← next_frame         │
//!
//!  Tahsis: next_frame alınır, next_frame += FRAME_SIZE
//!  Serbest bırakma: DESTEKLENMIYOR (bootstrap fazı için gereksiz)
//! ```
//!
//! ## Kernel Fiziksel Aralık Koruması
//!
//! Kernel'in kendi fiziksel adresleri tahsis edilmemelidir.
//! Linker sembolleri `kernel_start` ve `kernel_end` (KASLR offset düzeltmeli)
//! bu aralığı tanımlar; ayırıcı bu aralığı otomatik olarak atlar:
//!
//! ```
//! [0x0000 ... kernel_start_phys ... kernel_end_phys ... RAM_END]
//!                    │                     │
//!                    └─── bu aralığı atla──┘
//! ```
//!
//! ## Multiboot2 vs UEFI Karşılaştırması
//!
//! | Özellik              | Multiboot2FrameAllocator | MemoryManager (UEFI)        |
//! |----------------------|--------------------------|-----------------------------|
//! | Bellek haritası      | Multiboot2 tag            | UEFI MemoryMap              |
//! | Serbest bırakma      | Yok (bump)               | FibonacciPmm üzerinden var  |
//! | Zone desteği         | Yok                       | ZONE_DMA/DMA32/NORMAL       |
//! | Kullanım amacı       | Bootstrap / Multiboot2   | Ana kernel (UEFI)           |
//!
//! ## İlgili Modüller:
//! - `fibonacci_pmm.rs`: UEFI ortamlarında kullanılan zone tabanlı PMM
//! - `mod.rs`: Global memory manager; `set_global_mb2_frame_allocator()` ile kaydedilir

use crate::boot::context::{MemoryRegion, MemoryRegionKind};
use multiboot2::{BootInformation, MemoryAreaType};
use x86_64::structures::paging::{FrameAllocator, PhysFrame, Size4KiB};
use x86_64::PhysAddr;

const FRAME_SIZE: u64 = 4096;

const MAX_MB2_BOOTSTRAP_REGIONS: usize = 192;

static mut MB2_BOOTSTRAP_REGIONS: [MemoryRegion; MAX_MB2_BOOTSTRAP_REGIONS] =
    [MemoryRegion::EMPTY; MAX_MB2_BOOTSTRAP_REGIONS];

pub struct Multiboot2FrameAllocator<'a> {
    regions: &'a [MemoryRegion],
    region_index: usize,
    next_frame: u64,
    kernel_start_phys: u64,
    kernel_end_phys: u64,
    total_usable_bytes: u64,
}

impl<'a> Multiboot2FrameAllocator<'a> {
    pub fn new(boot_info: &BootInformation, kaslr_offset: u64) -> Option<Multiboot2FrameAllocator<'static>> {
        let memory_map = boot_info.memory_map_tag()?;
        let mut count = 0usize;
        for area in memory_map.memory_areas() {
            if count >= MAX_MB2_BOOTSTRAP_REGIONS {
                return None;
            }
            let kind = match area.typ() {
                MemoryAreaType::Available => MemoryRegionKind::Usable,
                MemoryAreaType::AcpiAvailable => MemoryRegionKind::ACPIReclaim,
                _ => MemoryRegionKind::Reserved,
            };
            unsafe {
                MB2_BOOTSTRAP_REGIONS[count] = MemoryRegion {
                    base: area.start_address(),
                    len: area.size(),
                    kind,
                };
            }
            count += 1;
        }

        let regions = unsafe {
            core::slice::from_raw_parts(MB2_BOOTSTRAP_REGIONS.as_ptr(), count)
        };
        Multiboot2FrameAllocator::<'static>::from_regions(regions, kaslr_offset)
    }

    pub fn from_regions(
        regions: &'a [MemoryRegion],
        kaslr_offset: u64,
    ) -> Option<Self> {
        let (kernel_start_phys, kernel_end_phys) = unsafe { kernel_phys_range(kaslr_offset) };
        Self::from_regions_with_kernel_range(regions, kernel_start_phys, kernel_end_phys)
    }

    /// Canonical-map bootstrap constructor for adapters whose image is placed
    /// at a bootloader-selected physical base (native Limine).  It keeps the
    /// allocator bounded and heap-free while using the exact executable span
    /// reported by the adapter.
    pub fn from_regions_with_kernel_range(
        regions: &'a [MemoryRegion],
        kernel_start_phys: u64,
        kernel_end_phys: u64,
    ) -> Option<Self> {
        let mut total_usable_bytes = 0u64;
        for region in regions {
            if Self::is_region_usable(region.kind) {
                total_usable_bytes = total_usable_bytes.saturating_add(region.len);
            }
        }

        crate::serial_println!(
            "[MEMORY] Kernel phys range: {:#x}-{:#x}",
            kernel_start_phys,
            kernel_end_phys
        );

        let mut allocator = Self {
            regions,
            region_index: 0,
            next_frame: 0,
            kernel_start_phys,
            kernel_end_phys,
            total_usable_bytes,
        };

        allocator.advance_to_next_usable_region();
        Some(allocator)
    }

    pub fn total_usable_bytes(&self) -> u64 {
        self.total_usable_bytes
    }

    pub fn allocate_contiguous(&mut self, pages: usize) -> Option<PhysFrame<Size4KiB>> {
        if pages == 0 {
            return None;
        }

        let size = (pages as u64).saturating_mul(FRAME_SIZE);

        loop {
            let region = match self.current_region() {
                Some(region) => region,
                None => return None,
            };

            if !Self::is_region_usable(region.kind) {
                self.advance_region();
                continue;
            }

            let start = align_up(self.next_frame, FRAME_SIZE);
            let end = start.saturating_add(size);

            if end > region.base.saturating_add(region.len) {
                self.advance_region();
                continue;
            }

            if self.overlaps_kernel(start, end) {
                self.next_frame = align_up(self.kernel_end_phys, FRAME_SIZE);
                continue;
            }

            self.next_frame = end;
            return Some(PhysFrame::containing_address(PhysAddr::new(start)));
        }
    }

    fn allocate_frame_internal(&mut self) -> Option<PhysFrame<Size4KiB>> {
        loop {
            let region = match self.current_region() {
                Some(region) => region,
                None => return None,
            };

            if !Self::is_region_usable(region.kind) {
                self.advance_region();
                continue;
            }

            let frame_start = align_up(self.next_frame, FRAME_SIZE);
            let frame_end = frame_start.saturating_add(FRAME_SIZE);

            if frame_end > region.base.saturating_add(region.len) {
                self.advance_region();
                continue;
            }

            self.next_frame = frame_end;

            if self.overlaps_kernel(frame_start, frame_end) {
                self.next_frame = align_up(self.kernel_end_phys, FRAME_SIZE);
                continue;
            }

            return Some(PhysFrame::containing_address(PhysAddr::new(frame_start)));
        }
    }

    fn advance_region(&mut self) {
        self.region_index = self.region_index.saturating_add(1);
        self.advance_to_next_usable_region();
    }

    fn advance_to_next_usable_region(&mut self) {
        while let Some(region) = self.current_region() {
            if Self::is_region_usable(region.kind) {
                let aligned = align_up(region.base, FRAME_SIZE);
                if aligned < region.base.saturating_add(region.len) {
                    self.next_frame = aligned;
                    return;
                }
            }
            self.region_index = self.region_index.saturating_add(1);
        }
    }

    fn current_region(&self) -> Option<MemoryRegion> {
        self.regions.get(self.region_index).copied()
    }

    fn overlaps_kernel(&self, start: u64, end: u64) -> bool {
        start < self.kernel_end_phys && end > self.kernel_start_phys
    }

    fn is_region_usable(kind: MemoryRegionKind) -> bool {
        matches!(
            kind,
            MemoryRegionKind::Usable
                | MemoryRegionKind::ACPIReclaim
                | MemoryRegionKind::BootloaderReclaimable
        )
    }
}

unsafe impl<'a> FrameAllocator<Size4KiB> for Multiboot2FrameAllocator<'a> {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        self.allocate_frame_internal()
    }
}

#[derive(Clone, Copy)]
pub struct LimineMemmapEntry {
    pub base: u64,
    pub length: u64,
    pub typ: u64,
}

pub const MAX_LIMINE_MEMORY_REGIONS: usize = 256;

pub struct LimineFrameAllocator<'a> {
    regions: &'a [LimineMemmapEntry],
    region_index: usize,
    next_frame: u64,
    kernel_start_phys: u64,
    kernel_end_phys: u64,
    total_usable_bytes: u64,
}

impl<'a> LimineFrameAllocator<'a> {
    pub fn new(entries: &'a [LimineMemmapEntry], physical_base: u64) -> Option<Self> {
        let mut total_usable_bytes = 0u64;

        for entry in entries {
            let start = entry.base;
            let end = entry.base.saturating_add(entry.length);
            if Self::is_region_usable(entry.typ) {
                total_usable_bytes = total_usable_bytes.saturating_add(end.saturating_sub(start));
            }
        }

        let (kernel_start_phys, kernel_end_phys) = unsafe { kernel_phys_range_limine(physical_base) };

        let mut allocator = Self {
            regions: entries,
            region_index: 0,
            next_frame: 0,
            kernel_start_phys,
            kernel_end_phys,
            total_usable_bytes,
        };

        allocator.advance_to_next_usable_region();
        Some(allocator)
    }

    pub fn total_usable_bytes(&self) -> u64 {
        self.total_usable_bytes
    }

    fn allocate_frame_internal(&mut self) -> Option<PhysFrame<Size4KiB>> {
        loop {
            let region = match self.current_region() {
                Some(region) => region,
                None => return None,
            };

            if !Self::is_region_usable(region.typ) {
                self.advance_region();
                continue;
            }

            let frame_start = align_up(self.next_frame, FRAME_SIZE);
            let frame_end = frame_start.saturating_add(FRAME_SIZE);

            let region_end = region.base.saturating_add(region.length);
            if frame_end > region_end {
                self.advance_region();
                continue;
            }

            self.next_frame = frame_end;

            if self.overlaps_kernel(frame_start, frame_end) {
                self.next_frame = align_up(self.kernel_end_phys, FRAME_SIZE);
                continue;
            }

            return Some(PhysFrame::containing_address(PhysAddr::new(frame_start)));
        }
    }

    fn advance_region(&mut self) {
        self.region_index = self.region_index.saturating_add(1);
        self.advance_to_next_usable_region();
    }

    fn advance_to_next_usable_region(&mut self) {
        while let Some(region) = self.current_region() {
            if Self::is_region_usable(region.typ) {
                let aligned = align_up(region.base, FRAME_SIZE);
                let region_end = region.base.saturating_add(region.length);
                if aligned < region_end {
                    self.next_frame = aligned;
                    return;
                }
            }
            self.region_index = self.region_index.saturating_add(1);
        }
    }

    fn current_region(&self) -> Option<LimineMemmapEntry> {
        self.regions.get(self.region_index).copied()
    }

    fn overlaps_kernel(&self, start: u64, end: u64) -> bool {
        start < self.kernel_end_phys && end > self.kernel_start_phys
    }

    fn is_region_usable(typ: u64) -> bool {
        matches!(typ, 0 | 2)
    }
}

unsafe impl<'a> FrameAllocator<Size4KiB> for LimineFrameAllocator<'a> {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        self.allocate_frame_internal()
    }
}

fn align_up(addr: u64, align: u64) -> u64 {
    if align == 0 {
        return addr;
    }
    (addr + align - 1) & !(align - 1)
}

fn align_down(addr: u64, align: u64) -> u64 {
    if align == 0 {
        return addr;
    }
    addr & !(align - 1)
}

#[cfg(target_os = "none")]
unsafe fn kernel_phys_range_limine(physical_base: u64) -> (u64, u64) {
    extern "C" {
        static kernel_phys_start: u8;
        static kernel_phys_end: u8;
    }

    // Limine path'i: görüntü tek bir slide ile higher-half'a kaydırılır ve
    // fiziksel yerleşim garantisizdir (spec). Link-time LMA sembolleri
    // (0x100000 tabanlı) yalnız görüntü-içi offset türetmek için kullanılır;
    // gerçek fiziksel taban Executable Address feature'dan gelir.
    let link_base = &kernel_phys_start as *const u8 as u64;
    let link_end = &kernel_phys_end as *const u8 as u64;

    (
        align_down(physical_base, FRAME_SIZE),
        align_up(
            physical_base.wrapping_add(link_end.wrapping_sub(link_base)),
            FRAME_SIZE,
        ),
    )
}

#[cfg(not(target_os = "none"))]
unsafe fn kernel_phys_range_limine(_physical_base: u64) -> (u64, u64) {
    // Host verification builds: stable local image span (bkz. kernel_phys_range).
    static HOST_IMAGE_START: u8 = 0;
    static HOST_IMAGE_END: u8 = 0;

    let start = &HOST_IMAGE_START as *const u8 as u64;
    let end = (&HOST_IMAGE_END as *const u8 as u64).saturating_add(FRAME_SIZE);

    (align_down(start, FRAME_SIZE), align_up(end, FRAME_SIZE))
}

#[cfg(target_os = "none")]
unsafe fn kernel_phys_range(kaslr_offset: u64) -> (u64, u64) {
    extern "C" {
        static kernel_phys_start: u8;
        static kernel_phys_end: u8;
    }

    // Linker LMA sembolleri (linker.ld): kernel görüntüsünün fiziksel adres
    // aralığı. Kernel 0x100000 tabanına VMA == LMA olarak bağlanır; KASLR
    // slide'ı varsa fiziksel aralığı kaydırır (VMA sabittir).
    let kernel_start_phys = (&kernel_phys_start as *const u8 as u64).wrapping_add(kaslr_offset);
    let kernel_end_phys = (&kernel_phys_end as *const u8 as u64).wrapping_add(kaslr_offset);

    (
        align_down(kernel_start_phys, FRAME_SIZE),
        align_up(kernel_end_phys, FRAME_SIZE),
    )
}

#[cfg(not(target_os = "none"))]
unsafe fn kernel_phys_range(_kaslr_offset: u64) -> (u64, u64) {
    // Host verification builds do not carry the bare-metal linker image symbols.
    // Derive a stable local image span so allocator code can still link and execute.
    static HOST_IMAGE_START: u8 = 0;
    static HOST_IMAGE_END: u8 = 0;

    let start = &HOST_IMAGE_START as *const u8 as u64;
    let end = (&HOST_IMAGE_END as *const u8 as u64).saturating_add(FRAME_SIZE);

    (align_down(start, FRAME_SIZE), align_up(end, FRAME_SIZE))
}
