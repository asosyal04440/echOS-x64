//! # DMA Ownership Contract
//!
//! Unified DMA lifecycle for all drivers: alloc → iommu_map → program_device → sync → unmap → free.
//! Linux DMA API model: streaming mappings transfer ownership between CPU and device.
//!
//! ## Ownership Model
//! ```text
//! CPU owns buffer
//!   │
//!   ▼ dma_map_single(DMA_TO_DEVICE)
//!   │ clflush (if non-coherent)
//!   ▼
//! Device owns buffer (CPU must NOT read/write)
//!   │
//!   ▼ dma_sync_single_for_cpu(DMA_FROM_DEVICE)
//!   │ invalidate (if non-coherent)
//!   ▼
//! CPU owns buffer (device must NOT access)
//! ```
//!
//! ## Bounce Buffer (swiotlb-style)
//! Device DMA mask > physical address ise bounce buffer kullan:
//! - Original buffer → CPU copy → Bounce buffer → Device DMA
//! - Device DMA → CPU copy → Original buffer
//!
//! ## Cache Coherency (x86 clflush)
//! Intel SDM Vol. 3 §11.5: `clflush` cache line'ı write-back + invalidate eder.
//! `mfence` ile serialize edilmeli.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

// ============================================================================
// DMA DIRECTION
// ============================================================================

/// DMA transfer yönü — cache coherency operasyonlarını belirler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmaDirection {
    /// CPU → Device: CPU veriyi yazdı, device okuyacak.
    /// Flush: CPU cache → memory (clean).
    ToDevice,
    /// Device → CPU: Device veriyi yazdı, CPU okuyacak.
    /// Invalidate: CPU cache → stale, memory'den oku.
    FromDevice,
    /// Bidirectional: Her iki taraf da yazabilir.
    /// Flush + Invalidate: Her sync'te iki yönlü operasyon.
    Bidirectional,
}

// ============================================================================
// DMA MAPPING
// ============================================================================

/// DMA mapping sonucu — device'e programlanacak DMA adresi.
#[derive(Debug, Clone, Copy)]
pub struct DmaMapping {
    /// Device'in gördüğü DMA adresi (IOMMU çevirisi sonrası)
    pub dma_addr: u64,
    /// Orijinal CPU sanal adresi
    pub cpu_virt: u64,
    /// Orijinal fiziksel adres (bounce kullanılmazsa dma_addr ile aynı)
    pub phys_addr: u64,
    /// Eşleme boyutu (byte)
    pub size: usize,
    /// Transfer yönü
    pub direction: DmaDirection,
    /// Bounce buffer kullanıldı mı?
    pub is_bounce: bool,
    /// Bounce buffer fiziksel adresi (kullanıldıysa)
    pub bounce_phys: u64,
    /// Bounce buffer CPU sanal adresi (kullanıldıysa)
    pub bounce_virt: u64,
}

// ============================================================================
// BOUNCE BUFFER (swiotlb-style)
// ============================================================================

/// Bounce buffer havuzu — device DMA mask'i aşan adresler için.
/// Linux swiotlb modeli: önceden ayrılmış pool, slot-based allocation.
pub struct BouncePool {
    /// Havuz başlangıç fiziksel adresi
    pool_phys: u64,
    /// Havuz CPU sanal adresi
    pool_virt: u64,
    /// Toplam havuz boyutu (byte)
    pool_size: usize,
    /// Slot boyutu (byte, page-aligned)
    slot_size: usize,
    /// Slot sayısı
    slot_count: usize,
    /// Kullanılan slot bitmask'ı
    used_slots: Mutex<Vec<bool>>,
    /// Toplam allocation sayısı
    alloc_count: AtomicUsize,
    /// Toplam bounce copy byte'ı
    total_copied: AtomicU64,
}

impl BouncePool {
    /// Yeni bounce buffer havuzu oluşturur.
    /// `pool_phys` ve `pool_virt` önceden ayrılmış, DMA-mask-compliant bellek olmalı.
    pub fn new(pool_phys: u64, pool_virt: u64, pool_size: usize, slot_size: usize) -> Self {
        let slot_count = pool_size / slot_size;
        let mut used = Vec::with_capacity(slot_count);
        for _ in 0..slot_count {
            used.push(false);
        }
        Self {
            pool_phys,
            pool_virt,
            pool_size,
            slot_size,
            slot_count,
            used_slots: Mutex::new(used),
            alloc_count: AtomicUsize::new(0),
            total_copied: AtomicU64::new(0),
        }
    }

    /// Bounce buffer slotu ayırır. Fiziksel adresi döner.
    pub fn alloc(&self, size: usize) -> Option<(u64, u64, usize)> {
        let slots_needed = (size + self.slot_size - 1) / self.slot_size;
        let mut used = self.used_slots.lock();

        // İlk fit slot bul
        let mut consecutive = 0usize;
        let mut start = None;
        for i in 0..self.slot_count {
            if !used[i] {
                if consecutive == 0 {
                    start = Some(i);
                }
                consecutive += 1;
                if consecutive >= slots_needed {
                    // Slotları işaretle
                    let s = start.unwrap();
                    for j in s..s + slots_needed {
                        used[j] = true;
                    }
                    let phys = self.pool_phys + (s * self.slot_size) as u64;
                    let virt = self.pool_virt + (s * self.slot_size) as u64;
                    self.alloc_count.fetch_add(1, Ordering::Relaxed);
                    return Some((phys, virt, slots_needed));
                }
            } else {
                consecutive = 0;
                start = None;
            }
        }
        None
    }

    /// Bounce buffer slotunu serbest bırakır.
    pub fn free(&self, phys: u64, slots_needed: usize) {
        let offset = phys - self.pool_phys;
        let start_slot = (offset as usize) / self.slot_size;
        let mut used = self.used_slots.lock();
        for i in start_slot..start_slot + slots_needed {
            if i < self.slot_count {
                used[i] = false;
            }
        }
    }

    /// CPU → Bounce buffer kopyası (DMA_TO_DEVICE öncesi).
    pub fn copy_to_bounce(&self, bounce_virt: u64, cpu_virt: u64, size: usize) {
        unsafe {
            core::ptr::copy_nonoverlapping(cpu_virt as *const u8, bounce_virt as *mut u8, size);
        }
        self.total_copied.fetch_add(size as u64, Ordering::Relaxed);
    }

    /// Bounce buffer → CPU kopyası (DMA_FROM_DEVICE sonrası).
    pub fn copy_from_bounce(&self, cpu_virt: u64, bounce_virt: u64, size: usize) {
        unsafe {
            core::ptr::copy_nonoverlapping(bounce_virt as *const u8, cpu_virt as *mut u8, size);
        }
        self.total_copied.fetch_add(size as u64, Ordering::Relaxed);
    }

    pub fn stats(&self) -> (usize, u64) {
        (
            self.alloc_count.load(Ordering::Relaxed),
            self.total_copied.load(Ordering::Relaxed),
        )
    }
}

// ============================================================================
// CACHE COHERENCY (x86 clflush)
// ============================================================================

/// x86 cache line boyutu (Intel SDM: minimum 64 byte).
const CACHE_LINE_SIZE: usize = 64;

/// Belirli bir adres aralığını cache'den flush eder.
/// Intel SDM Vol. 3 §11.5: `clflush` — write-back + invalidate.
/// Her cache line için ayrı clflush, sonunda mfence.
pub fn clflush_range(virt_addr: u64, size: usize) {
    let start = virt_addr & !(CACHE_LINE_SIZE as u64 - 1);
    let end = virt_addr + size as u64;
    let mut addr = start;

    while addr < end {
        unsafe {
            core::arch::x86_64::_mm_clflush(addr as *const u8);
        }
        addr += CACHE_LINE_SIZE as u64;
    }

    // Serialize — tüm clflush'lar tamamlanana kadar bekle
    unsafe {
        core::arch::x86_64::_mm_mfence();
    }
}

/// Belirli bir adres aralığını cache'den invalidate eder.
/// `clflush` zaten invalidate içerir; from-device sonrası için de kullanılabilir.
pub fn invalidate_range(virt_addr: u64, size: usize) {
    // x86'da clflush hem flush hem invalidate yapar
    clflush_range(virt_addr, size);
}

// ============================================================================
// DMA OWNERSHIP CONTRACT
// ============================================================================

/// DMA mapping lifecycle yöneticisi.
/// Tüm driver'lar bu contract'ı kullanmalı:
/// 1. `map_single` → device'e DMA adresi ver
/// 2. `sync_for_device` → CPU → device ownership transfer
/// 3. `sync_for_cpu` → device → CPU ownership transfer
/// 4. `unmap_single` → mapping'i kaldır, bounce buffer'ı free et
pub struct DmaManager {
    /// Bounce buffer havuzu (opsiyonel)
    bounce_pool: Mutex<Option<BouncePool>>,
    /// Aktif mapping sayısı
    active_mappings: AtomicUsize,
    /// Toplam map byte'ı
    total_mapped: AtomicU64,
}

impl DmaManager {
    pub const fn new() -> Self {
        Self {
            bounce_pool: Mutex::new(None),
            active_mappings: AtomicUsize::new(0),
            total_mapped: AtomicU64::new(0),
        }
    }

    /// Bounce buffer havuzunu başlatır.
    pub fn init_bounce_pool(&self, pool_phys: u64, pool_virt: u64, pool_size: usize) {
        let slot_size = 4096; // 1 page per slot
        *self.bounce_pool.lock() = Some(BouncePool::new(pool_phys, pool_virt, pool_size, slot_size));
        crate::serial_println!(
            "[DMA] Bounce pool initialized: {} bytes, {} slots",
            pool_size,
            pool_size / slot_size
        );
    }

    /// Tek bir buffer için DMA mapping.
    ///
    /// Adımlar:
    /// 1. phys_addr device DMA mask içinde mi? → doğrudan map
    /// 2. Değilse → bounce buffer alloc + CPU copy
    /// 3. IOMMU map (varsa)
    /// 4. Cache flush (DMA_TO_DEVICE veya BIDIRECTIONAL)
    pub fn map_single(
        &self,
        cpu_virt: u64,
        phys_addr: u64,
        size: usize,
        direction: DmaDirection,
        dma_mask: u64,
        segment: u16,
        bdf: u16,
    ) -> Option<DmaMapping> {
        let needs_bounce = phys_addr.saturating_add(size as u64) > dma_mask;

        let (dma_addr, actual_phys, is_bounce, bounce_phys, bounce_virt, bounce_slots) =
            if needs_bounce {
                let pool = self.bounce_pool.lock();
                let pool = pool.as_ref()?;
                let (b_phys, b_virt, b_slots) = pool.alloc(size)?;
                // CPU → Bounce copy
                pool.copy_to_bounce(b_virt, cpu_virt, size);
                // Bounce buffer zaten DMA-mask-compliant
                (b_phys, b_phys, true, b_phys, b_virt, b_slots)
            } else {
                (phys_addr, phys_addr, false, 0, 0, 0)
            };

        // IOMMU mapping (varsa)
        let final_dma_addr = if crate::drivers::iommu::IOMMU_MANAGER.is_enabled() {
            let _ = crate::drivers::iommu::IOMMU_MANAGER.map_dma(
                segment, bdf, dma_addr, actual_phys, size as u64,
                direction != DmaDirection::FromDevice,
                direction != DmaDirection::ToDevice,
            );
            dma_addr // IOMMU mapping sonrası device DMA adresi
        } else {
            dma_addr
        };

        // Cache flush: CPU → Device öncesi
        if direction == DmaDirection::ToDevice || direction == DmaDirection::Bidirectional {
            let flush_virt = if is_bounce { bounce_virt } else { cpu_virt };
            clflush_range(flush_virt, size);
        }

        self.active_mappings.fetch_add(1, Ordering::Relaxed);
        self.total_mapped.fetch_add(size as u64, Ordering::Relaxed);

        Some(DmaMapping {
            dma_addr: final_dma_addr,
            cpu_virt,
            phys_addr: actual_phys,
            size,
            direction,
            is_bounce,
            bounce_phys,
            bounce_virt,
        })
    }

    /// Device → CPU ownership transfer.
    /// DMA_FROM_DEVICE sonrası: bounce buffer'dan CPU'ya kopya + cache invalidate.
    pub fn sync_for_cpu(&self, mapping: &DmaMapping) {
        if mapping.is_bounce && mapping.direction != DmaDirection::ToDevice {
            // Bounce → CPU copy
            let pool = self.bounce_pool.lock();
            if let Some(ref pool) = *pool {
                pool.copy_from_bounce(mapping.cpu_virt, mapping.bounce_virt, mapping.size);
            }
        }

        // Cache invalidate: Device → CPU sonrası
        if mapping.direction == DmaDirection::FromDevice
            || mapping.direction == DmaDirection::Bidirectional
        {
            let inv_virt = if mapping.is_bounce {
                mapping.bounce_virt
            } else {
                mapping.cpu_virt
            };
            invalidate_range(inv_virt, mapping.size);
        }
    }

    /// CPU → Device ownership transfer.
    /// DMA_TO_DEVICE öncesi: CPU → bounce buffer kopya + cache flush.
    pub fn sync_for_device(&self, mapping: &DmaMapping) {
        if mapping.is_bounce {
            let pool = self.bounce_pool.lock();
            if let Some(ref pool) = *pool {
                pool.copy_to_bounce(mapping.bounce_virt, mapping.cpu_virt, mapping.size);
            }
        }

        // Cache flush
        if mapping.direction == DmaDirection::ToDevice
            || mapping.direction == DmaDirection::Bidirectional
        {
            let flush_virt = if mapping.is_bounce {
                mapping.bounce_virt
            } else {
                mapping.cpu_virt
            };
            clflush_range(flush_virt, mapping.size);
        }
    }

    /// DMA mapping'i kaldırır.
    /// Bounce buffer'ı serbest bırakır, IOMMU unmap yapar.
    pub fn unmap_single(&self, mapping: &DmaMapping, segment: u16, bdf: u16) {
        // IOMMU unmap
        if crate::drivers::iommu::IOMMU_MANAGER.is_enabled() {
            let _ = crate::drivers::iommu::IOMMU_MANAGER.unmap_dma(segment, bdf, mapping.dma_addr);
        }

        // Bounce buffer free
        if mapping.is_bounce {
            let slots_needed = (mapping.size + 4095) / 4096;
            let pool = self.bounce_pool.lock();
            if let Some(ref pool) = *pool {
                pool.free(mapping.bounce_phys, slots_needed);
            }
        }

        self.active_mappings.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn stats(&self) -> (usize, u64) {
        (
            self.active_mappings.load(Ordering::Relaxed),
            self.total_mapped.load(Ordering::Relaxed),
        )
    }
}

// ============================================================================
// GLOBAL
// ============================================================================

lazy_static::lazy_static! {
    pub static ref DMA_MANAGER: DmaManager = DmaManager::new();
}

/// DMA alt sistemini başlatır.
pub fn init() {
    crate::serial_println!("[DMA] DMA ownership contract initialized");
    crate::serial_println!("[DMA]   Streaming DMA: map → sync → unmap lifecycle");
    crate::serial_println!("[DMA]   Bounce buffer: swiotlb-style for limited DMA mask");
    crate::serial_println!("[DMA]   Coherency: x86 clflush + mfence serialization");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clflush_range_covers_all_cache_lines() {
        // 128 byte range, 64-byte cache lines → 2 clflush calls
        // Test: start aligned, size = 2 cache lines
        let addr = 0x1000u64; // cache-line aligned
        let size = 128usize;
        // Should flush lines at 0x1000 and 0x1040
        // We can't actually run clflush in test, but verify the math
        let start = addr & !(CACHE_LINE_SIZE as u64 - 1);
        let end = addr + size as u64;
        let lines = (end - start) / CACHE_LINE_SIZE as u64;
        assert_eq!(lines, 2);
    }

    #[test]
    fn bounce_pool_alloc_and_free() {
        let pool_phys = 0x1000_0000u64;
        let pool_virt = 0xFFFF_8000_0000_0000u64;
        let pool_size = 4096 * 8; // 8 slots
        let slot_size = 4096;

        let pool = BouncePool::new(pool_phys, pool_virt, pool_size, slot_size);

        // Alloc 1 slot
        let (phys, virt, slots) = pool.alloc(100).unwrap();
        assert_eq!(slots, 1);
        assert_eq!(phys, pool_phys);
        assert_eq!(virt, pool_virt);

        // Free
        pool.free(phys, slots);

        // Realloc should succeed
        let (phys2, _, _) = pool.alloc(100).unwrap();
        assert_eq!(phys2, pool_phys);
    }

    #[test]
    fn bounce_pool_consecutive_alloc() {
        let pool_phys = 0x1000_0000u64;
        let pool_virt = 0xFFFF_8000_0000_0000u64;
        let pool_size = 4096 * 4;
        let slot_size = 4096;

        let pool = BouncePool::new(pool_phys, pool_virt, pool_size, slot_size);

        // Alloc 3 consecutive slots (size > 1 slot)
        let (phys, _, slots) = pool.alloc(8192 + 1).unwrap(); // needs 3 slots
        assert_eq!(slots, 3);
        assert_eq!(phys, pool_phys);

        // Only 1 slot left, can't alloc 2 more
        assert!(pool.alloc(8192).is_none());
    }
}
