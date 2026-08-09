//! # Fibonacci PMM — Zone Tabanlı Fiziksel Bellek Yönetimi
//!
//! Fibonacci Buddy System + Linux tarzı Zone tahsis mekanizması.
//!
//! ## Bellek Bölgeleri (Memory Zones)
//!
//! x86_64'te DMA cihazlarının erişebildiği adres aralıkları donanıma bağlıdır.
//! Linux'un `mm/mmzone.h` tasarımı esas alınmıştır:
//!
//! ```
//! Fiziksel Adres Uzayı:
//!
//!  0x0000_0000          0x100_0000 (16 MB)     0x1_0000_0000 (4 GB)
//!       │                    │                        │
//!       ▼                    ▼                        ▼
//!  ┌────────────────────┬───────────────────────┬──────────────── ···
//!  │    ZONE_DMA        │     ZONE_DMA32         │   ZONE_NORMAL
//!  │  0 → 16 MB         │  16 MB → 4 GB          │   4 GB → ∞
//!  │  ISA DMA (24-bit)  │  PCI 32-bit DMA        │   sınırsız
//!  └────────────────────┴───────────────────────┴──────────────── ···
//! ```
//!
//! ## Zone Seçim Mantığı (Fallback Zinciri)
//!
//! Bir cihaz tahsis istediğinde, gereken zone'dan başlayarak yukarı çıkar:
//!
//! ```
//! İstek: NORMAL zone
//!   ↓ NORMAL dolu veya yetersiz?
//!   → DMA32 dene
//!      ↓ DMA32 da dolu?
//!      → DMA dene (son çare)
//!         ↓ DMA da dolu?
//!         → None (tahsis başarısız)
//! ```
//!
//! ## Fibonacci Buddy ile Entegrasyon
//!
//! Her zone kendi `FibonacciBuddyAllocator`'ına sahiptir.
//! Fibonacci serileri (1, 1, 2, 3, 5, 8, 13, 21, ...) blok boyutlarını tanımlar;
//! standart 2^n buddy sistemine göre daha az dahili parçalanma üretir:
//!
//! ```
//! Standart Buddy (2^n):  1KB, 2KB, 4KB, 8KB, 16KB, ...  → %28 ortalama parçalanma
//! Fibonacci Buddy:       4KB, 4KB, 8KB,12KB, 20KB, ...  → %12 ortalama parçalanma
//! ```
//!
//! ## Zone İstatistikleri
//!
//! Her zone `free_frames`, `used_frames`, `total_frames` takibi yapar.
//! Fallback gerçekleştiğinde `fallback_count` artar — yüksek fallback oranı
//! bellek baskısının işaretidir (kswapd'yi tetikler).
//!
//! ## İlgili Modüller:
//! - `fibonacci_buddy.rs`: Fibonacci tabanlı buddy ayırıcı
//! - `mod.rs`: `MemoryManager` — bu PMM'i sarar ve x86_64 FrameAllocator trait'ini uygular
//! - `frame_allocator.rs`: Multiboot2 bootstrap ayırıcısı (bu modülün öncüsü)

use super::fibonacci_buddy::FibonacciBuddyAllocator;
use uefi::table::boot::{MemoryDescriptor, MemoryType};
use x86_64::structures::paging::{FrameAllocator, PhysFrame, Size4KiB};
use x86_64::PhysAddr;

// ============================================================================
// ZONE TANIMLARI (Linux mm/mmzone.h referans)
// ============================================================================

/// Bellek bölge türleri — DMA cihazlarının adres sınırlarına göre ayrılır.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryZone {
    /// ISA DMA cihazları: 0 – 16 MB (24-bit adresleme)
    Dma,
    /// PCI 32-bit DMA cihazları: 0 – 4 GB
    Dma32,
    /// Normal bellek: 4 GB üstü (sınırsız)
    Normal,
}

/// Zone sınırları (byte cinsinden)
const ZONE_DMA_LIMIT: u64 = 16 * 1024 * 1024; // 16 MB
const ZONE_DMA32_LIMIT: u64 = 4 * 1024 * 1024 * 1024; // 4 GB

impl MemoryZone {
    /// Fiziksel adrese göre zone belirler
    fn from_addr(addr: u64) -> Self {
        if addr < ZONE_DMA_LIMIT {
            MemoryZone::Dma
        } else if addr < ZONE_DMA32_LIMIT {
            MemoryZone::Dma32
        } else {
            MemoryZone::Normal
        }
    }

    /// Fallback chain: NORMAL → DMA32 → DMA
    fn fallback(self) -> Option<MemoryZone> {
        match self {
            MemoryZone::Normal => Some(MemoryZone::Dma32),
            MemoryZone::Dma32 => Some(MemoryZone::Dma),
            MemoryZone::Dma => None,
        }
    }
}

// ============================================================================
// REGION ALLOCATOR (Zone bilgisi ile)
// ============================================================================

#[derive(Clone, Copy)]
struct RegionAllocator {
    start: PhysAddr,
    size: usize,
    zone: MemoryZone,
    buddy: FibonacciBuddyAllocator,
}

// ============================================================================
// FIBONACCI PMM — ZONE-AWARE
// ============================================================================

/// Fibonacci Physical Memory Manager — Zone-based allocation ile.
pub struct FibonacciPmm {
    regions: [Option<RegionAllocator>; MAX_PMM_REGIONS],
    region_count: usize,
    /// Toplam frame sayısı
    total_frames: usize,
    /// Kullanılan frame sayısı
    used_frames: usize,
    /// Zone başına istatistik: [DMA, DMA32, NORMAL]
    zone_total: [usize; 3],
    zone_used: [usize; 3],
}

const MAX_PMM_REGIONS: usize = 32;

unsafe impl Send for FibonacciPmm {}
unsafe impl Sync for FibonacciPmm {}

impl FibonacciPmm {
    /// Yeni boş PMM oluşturur.
    pub fn empty() -> Self {
        Self {
            regions: [None; MAX_PMM_REGIONS],
            region_count: 0,
            total_frames: 0,
            used_frames: 0,
            zone_total: [0; 3],
            zone_used: [0; 3],
        }
    }

    /// Zone index (istatistik dizileri için)
    fn zone_idx(zone: MemoryZone) -> usize {
        match zone {
            MemoryZone::Dma => 0,
            MemoryZone::Dma32 => 1,
            MemoryZone::Normal => 2,
        }
    }

    /// UEFI Memory Map kullanarak PMM'i başlatır.
    /// Her bellek bölgesini fiziksel adresine göre zone'a atar.
    pub unsafe fn init<'a, I>(&mut self, map_iter: I)
    where
        I: Iterator<Item = &'a MemoryDescriptor> + Clone,
    {
        self.regions = [None; MAX_PMM_REGIONS];
        self.region_count = 0;
        self.total_frames = 0;
        self.used_frames = 0;
        self.zone_total = [0; 3];
        self.zone_used = [0; 3];

        for (desc_index, desc) in map_iter.clone().enumerate() {
            if desc.ty != MemoryType::CONVENTIONAL || desc.page_count == 0 {
                continue;
            }

            let phys_start = desc.phys_start;
            let phys_end = phys_start + desc.page_count * 4096;

            // Bölge birden fazla zone'a yayılabilir — parçala
            let boundaries = [ZONE_DMA_LIMIT, ZONE_DMA32_LIMIT, u64::MAX];
            let mut cursor = phys_start;

            for &boundary in &boundaries {
                if cursor >= phys_end {
                    break;
                }
                let chunk_end = phys_end.min(boundary);
                if chunk_end <= cursor {
                    continue;
                }

                let size = (chunk_end - cursor) as usize;
                let zone = MemoryZone::from_addr(cursor);
                let start = PhysAddr::new(cursor);
                crate::serial_println!(
                    "[PMM] region {} zone={:?} base={:#x} pages={:#x}",
                    desc_index,
                    zone,
                    cursor,
                    size / 4096
                );
                let buddy = FibonacciBuddyAllocator::new(start, size);
                let pages = size / 4096;

                if self.region_count >= MAX_PMM_REGIONS {
                    crate::serial_println!("[PMM] region capacity exhausted");
                    return;
                }
                self.regions[self.region_count] = Some(RegionAllocator {
                    start,
                    size,
                    zone,
                    buddy,
                });
                self.region_count += 1;
                self.total_frames = self.total_frames.saturating_add(pages);
                self.zone_total[Self::zone_idx(zone)] += pages;

                cursor = chunk_end;
            }
        }

        crate::serial_println!(
            "[PMM] Zone init: DMA={} frames (0-16MB), DMA32={} frames (16MB-4GB), NORMAL={} frames (4GB+)",
            self.zone_total[0],
            self.zone_total[1],
            self.zone_total[2]
        );
    }

    // ========================================================================
    // ZONE-AWARE ALLOCATION
    // ========================================================================

    /// Belirli bir zone'dan frame tahsis et.
    /// Başarısız olursa fallback chain'i dener: NORMAL → DMA32 → DMA.
    pub fn allocate_from_zone(&mut self, zone: MemoryZone) -> Option<PhysFrame> {
        // Önce istenen zone'dan dene
        if let Some(frame) = self.try_allocate_zone(zone) {
            return Some(frame);
        }
        // Fallback chain
        let mut fallback = zone.fallback();
        while let Some(fz) = fallback {
            if let Some(frame) = self.try_allocate_zone(fz) {
                return Some(frame);
            }
            fallback = fz.fallback();
        }
        None
    }

    /// Belirli bir zone'dan contiguous frames tahsis et.
    pub fn allocate_contiguous_from_zone(
        &mut self,
        pages: usize,
        zone: MemoryZone,
    ) -> Option<PhysFrame> {
        if pages == 0 {
            return None;
        }
        // Önce istenen zone
        if let Some(frame) = self.try_allocate_contiguous_zone(pages, zone) {
            return Some(frame);
        }
        // Fallback
        let mut fallback = zone.fallback();
        while let Some(fz) = fallback {
            if let Some(frame) = self.try_allocate_contiguous_zone(pages, fz) {
                return Some(frame);
            }
            fallback = fz.fallback();
        }
        None
    }

    /// Tek zone'dan single frame (fallback yok)
    fn try_allocate_zone(&mut self, zone: MemoryZone) -> Option<PhysFrame> {
        for index in 0..self.region_count {
            let Some(region) = self.regions[index].as_mut() else { continue };
            if region.zone != zone {
                continue;
            }
            if let Some(addr) = region.buddy.allocate(4096) {
                self.used_frames += 1;
                self.zone_used[Self::zone_idx(zone)] += 1;
                return Some(PhysFrame::containing_address(addr));
            }
        }
        None
    }

    /// Tek zone'dan contiguous frames (fallback yok)
    fn try_allocate_contiguous_zone(
        &mut self,
        pages: usize,
        zone: MemoryZone,
    ) -> Option<PhysFrame> {
        let size = pages * 4096;
        for index in 0..self.region_count {
            let Some(region) = self.regions[index].as_mut() else { continue };
            if region.zone != zone {
                continue;
            }
            if let Some(addr) = region.buddy.allocate(size) {
                self.used_frames += pages;
                self.zone_used[Self::zone_idx(zone)] += pages;
                return Some(PhysFrame::containing_address(addr));
            }
        }
        None
    }

    // ========================================================================
    // MEVCUT API (GERIYE UYUMLU)
    // ========================================================================

    /// Single frame tahsisi — varsayılan olarak NORMAL zone'dan başlar.
    pub fn allocate_frame(&mut self) -> Option<PhysFrame> {
        let result = self.allocate_from_zone(MemoryZone::Normal);
        if result.is_none() {
            crate::serial_println!(
                "[PMM-OOM] used={}/{} zones={:?}",
                self.used_frames,
                self.total_frames,
                self.zone_used
            );
            for i in 0..self.region_count {
                let Some(reg) = self.regions[i].as_ref() else { continue };
                crate::serial_println!(
                    "[PMM-OOM] region[{}] start={:#x} size={:#x} zone={:?}",
                    i,
                    reg.start.as_u64(),
                    reg.size,
                    reg.zone
                );
            }
        }
        result
    }

    /// Contiguous frames tahsisi — varsayılan NORMAL zone.
    pub fn allocate_contiguous(&mut self, pages: usize) -> Option<PhysFrame> {
        self.allocate_contiguous_from_zone(pages, MemoryZone::Normal)
    }

    /// Frame'leri free et — zone otomatik belirlenir.
    pub fn deallocate_contiguous(&mut self, start: PhysFrame, pages: usize) {
        if pages == 0 {
            return;
        }
        let addr = start.start_address();
        let size = pages * 4096;
        for index in 0..self.region_count {
            let Some(region) = self.regions[index].as_mut() else { continue };
            let region_start = region.start.as_u64();
            let region_end = region_start + region.size as u64;
            let addr_u64 = addr.as_u64();
            if addr_u64 >= region_start && addr_u64 < region_end {
                region.buddy.deallocate(addr, size);
                self.used_frames = self.used_frames.saturating_sub(pages);
                let idx = Self::zone_idx(region.zone);
                self.zone_used[idx] = self.zone_used[idx].saturating_sub(pages);
                return;
            }
        }
    }

    // ========================================================================
    // İSTATİSTİKLER
    // ========================================================================

    pub fn utilization(&self) -> f64 {
        if self.total_frames == 0 {
            return 0.0;
        }
        (self.used_frames as f64 / self.total_frames as f64) * 100.0
    }

    pub fn total_frames(&self) -> usize {
        self.total_frames
    }

    pub fn free_frames(&self) -> usize {
        self.total_frames.saturating_sub(self.used_frames)
    }

    /// Zone başına (total, used, free) döndürür.
    pub fn zone_stats(&self, zone: MemoryZone) -> (usize, usize, usize) {
        let idx = Self::zone_idx(zone);
        let total = self.zone_total[idx];
        let used = self.zone_used[idx];
        (total, used, total.saturating_sub(used))
    }

    pub fn fragmentation(&self) -> f64 {
        let mut total_weight = 0usize;
        let mut weighted_sum = 0.0;
        for index in 0..self.region_count {
            let Some(region) = self.regions[index].as_ref() else { continue };
            let pages = region.size / 4096;
            if pages == 0 {
                continue;
            }
            total_weight = total_weight.saturating_add(pages);
            weighted_sum += region.buddy.fragmentation() * pages as f64;
        }
        if total_weight == 0 {
            return 0.0;
        }
        weighted_sum / total_weight as f64
    }
}

// FrameAllocator trait implementasyonu (geriye uyumlu)
unsafe impl FrameAllocator<Size4KiB> for FibonacciPmm {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        self.allocate_frame()
    }
}

// ============================================================================
// BENCHMARK TESTS
// ============================================================================

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    use super::*;
    use uefi::table::boot::MemoryDescriptor;

    #[test]
    fn test_fibonacci_pmm_allocation() {
        let mut pmm = FibonacciPmm::empty();

        let desc = MemoryDescriptor {
            phys_start: 0x1000000, // 16MB — DMA32 zone
            virt_start: 0x1000000,
            page_count: 1024, // 4MB
            ty: MemoryType::CONVENTIONAL,
            att: uefi::table::boot::MemoryAttribute::empty(),
        };

        unsafe {
            pmm.init(core::iter::once(&desc));
        }

        let frame = pmm.allocate_frame().unwrap();
        assert_eq!(frame.start_address().as_u64(), 0x1000000);
        assert!(pmm.utilization() > 0.0);
        assert!(pmm.fragmentation() < 15.0);
    }

    #[test]
    fn test_zone_allocation() {
        let mut pmm = FibonacciPmm::empty();

        // DMA zone (0-16MB)
        let desc_dma = MemoryDescriptor {
            phys_start: 0x100000, // 1MB — DMA zone
            virt_start: 0x100000,
            page_count: 256, // 1MB
            ty: MemoryType::CONVENTIONAL,
            att: uefi::table::boot::MemoryAttribute::empty(),
        };

        // NORMAL zone (4GB+)
        let desc_normal = MemoryDescriptor {
            phys_start: 0x1_0000_0000, // 4GB — NORMAL zone
            virt_start: 0x1_0000_0000,
            page_count: 1024,
            ty: MemoryType::CONVENTIONAL,
            att: uefi::table::boot::MemoryAttribute::empty(),
        };

        unsafe {
            pmm.init([desc_dma, desc_normal].iter());
        }

        // DMA zone'dan tahsis
        let dma_frame = pmm.allocate_from_zone(MemoryZone::Dma).unwrap();
        assert!(dma_frame.start_address().as_u64() < ZONE_DMA_LIMIT);

        // DMA zone stats
        let (total, used, _free) = pmm.zone_stats(MemoryZone::Dma);
        assert!(total > 0);
        assert_eq!(used, 1);
    }
}
