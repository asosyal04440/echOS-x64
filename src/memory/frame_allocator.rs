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

use alloc::vec::Vec;
use multiboot2::{BootInformation, MemoryAreaType};
use x86_64::structures::paging::{FrameAllocator, PhysFrame, Size4KiB};
use x86_64::PhysAddr;

const FRAME_SIZE: u64 = 4096;
const KERNEL_VMA: u64 = 0xFFFF_FFFF_8000_0000;

#[derive(Clone, Copy)]
struct Region {
    start: u64,
    end: u64,
    typ: MemoryAreaType,
}

pub struct Multiboot2FrameAllocator {
    regions: Vec<Region>,
    region_index: usize,
    next_frame: u64,
    kernel_start_phys: u64,
    kernel_end_phys: u64,
    total_usable_bytes: u64,
}

impl Multiboot2FrameAllocator {
    pub fn new(boot_info: &BootInformation, kaslr_offset: u64) -> Option<Self> {
        let memory_map = boot_info.memory_map_tag()?;
        let mut regions = Vec::new();
        let mut total_usable_bytes = 0u64;

        for area in memory_map.memory_areas() {
            let start = area.start_address();
            let end = area.end_address();
            let typ = area.typ();
            if matches!(
                typ,
                MemoryAreaType::Available | MemoryAreaType::AcpiAvailable
            ) {
                total_usable_bytes = total_usable_bytes.saturating_add(end.saturating_sub(start));
            }
            regions.push(Region { start, end, typ });
        }

        let (kernel_start_phys, kernel_end_phys) = unsafe { kernel_phys_range(kaslr_offset) };
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

            if !Self::is_region_usable(region.typ) {
                self.advance_region();
                continue;
            }

            let start = align_up(self.next_frame, FRAME_SIZE);
            let end = start.saturating_add(size);

            if end > region.end {
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

            if !Self::is_region_usable(region.typ) {
                self.advance_region();
                continue;
            }

            let frame_start = align_up(self.next_frame, FRAME_SIZE);
            let frame_end = frame_start.saturating_add(FRAME_SIZE);

            if frame_end > region.end {
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
                let aligned = align_up(region.start, FRAME_SIZE);
                if aligned < region.end {
                    self.next_frame = aligned;
                    return;
                }
            }
            self.region_index = self.region_index.saturating_add(1);
        }
    }

    fn current_region(&self) -> Option<Region> {
        self.regions.get(self.region_index).copied()
    }

    fn overlaps_kernel(&self, start: u64, end: u64) -> bool {
        start < self.kernel_end_phys && end > self.kernel_start_phys
    }

    fn is_region_usable(typ: MemoryAreaType) -> bool {
        matches!(
            typ,
            MemoryAreaType::Available | MemoryAreaType::AcpiAvailable
        )
    }
}

unsafe impl FrameAllocator<Size4KiB> for Multiboot2FrameAllocator {
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

pub struct LimineFrameAllocator {
    regions: Vec<LimineMemmapEntry>,
    region_index: usize,
    next_frame: u64,
    kernel_start_phys: u64,
    kernel_end_phys: u64,
    total_usable_bytes: u64,
}

impl LimineFrameAllocator {
    pub fn new(entries: &[LimineMemmapEntry], kaslr_offset: u64) -> Option<Self> {
        let mut regions = Vec::new();
        let mut total_usable_bytes = 0u64;

        for entry in entries {
            let start = entry.base;
            let end = entry.base.saturating_add(entry.length);
            if Self::is_region_usable(entry.typ) {
                total_usable_bytes = total_usable_bytes.saturating_add(end.saturating_sub(start));
            }
            regions.push(*entry);
        }

        let (kernel_start_phys, kernel_end_phys) = unsafe { kernel_phys_range(kaslr_offset) };

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

unsafe impl FrameAllocator<Size4KiB> for LimineFrameAllocator {
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
unsafe fn kernel_phys_range(kaslr_offset: u64) -> (u64, u64) {
    extern "C" {
        static kernel_start: u8;
        static kernel_end: u8;
        static boot_lma_end: u8;
    }

    let kernel_start_virt = &kernel_start as *const u8 as u64;
    let kernel_end_virt = &kernel_end as *const u8 as u64;
    let boot_lma_end_phys = &boot_lma_end as *const u8 as u64;

    let kernel_start_phys = kernel_start_virt
        .wrapping_sub(KERNEL_VMA)
        .wrapping_sub(kaslr_offset)
        .wrapping_add(boot_lma_end_phys);

    let kernel_end_phys = kernel_end_virt
        .wrapping_sub(KERNEL_VMA)
        .wrapping_sub(kaslr_offset)
        .wrapping_add(boot_lma_end_phys);

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
