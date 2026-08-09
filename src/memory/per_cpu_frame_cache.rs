//! # Per-CPU Frame Cache
//!
//! Each CPU holds a small LIFO stack of free order-0 (4 KiB) physical frame
//! addresses.  Allocation from the cache is O(1) and avoids the global PMM
//! lock.  When empty, the cache is refilled in a batch from the global PMM.
//! When full on free, half the cache is drained back to the global PMM.
//!
//! Huge-page or contiguous (> 1) allocations bypass the cache entirely.
//!
//! ## Batch protocol
//!
//! ```text
//! alloc_frame(order=0):
//!   cache.pop() → hit?  return
//!   refill_batch()      // allocs up to BATCH frames from global PMM
//!   cache.pop() → hit?  return
//!   return None
//!
//! free_frame(frame):
//!   cache.push(addr) → not full?  return
//!   drain_batch()             // frees BATCH/2 frames to global PMM
//!   cache.push(addr)          // guaranteed space now
//! ```

use x86_64::{PhysAddr, structures::paging::PhysFrame};

use super::fibonacci_pmm::FibonacciPmm;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Number of frames per batch (refill or drain).
const BATCH: usize = 32;

/// Maximum number of CPUs we support.  Must match the value used during
/// `PerCpuFrameCache::init()`.
const MAX_CPU_COUNT: usize = 64;

// ---------------------------------------------------------------------------
// Per-CPU cache
// ---------------------------------------------------------------------------

/// A thread-local cache of free order-0 frame addresses.
///
/// **Alignment 64** (cache-line) prevents false sharing between adjacent CPU
/// slots in the global array.
#[derive(Copy, Clone)]
#[repr(C, align(64))]
pub(crate) struct PerCpuFrameCache {
    /// LIFO stack of physical frame addresses.
    frames: [u64; BATCH],
    /// Number of valid entries in `frames`.
    count: u32,
}

impl PerCpuFrameCache {
    const fn new() -> Self {
        PerCpuFrameCache {
            frames: [0; BATCH],
            count: 0,
        }
    }

    #[inline]
    fn pop(&mut self) -> Option<u64> {
        if self.count == 0 {
            return None;
        }
        self.count -= 1;
        Some(self.frames[self.count as usize])
    }

    #[inline]
    fn push(&mut self, addr: u64) -> bool {
        if (self.count as usize) >= BATCH {
            return false;
        }
        self.frames[self.count as usize] = addr;
        self.count += 1;
        true
    }

    fn is_empty(&self) -> bool {
        self.count == 0
    }

    #[inline]
    fn count(&self) -> u32 {
        self.count
    }

    #[inline]
    fn set_count(&mut self, n: u32) {
        self.count = n;
    }
}

// ---------------------------------------------------------------------------
// Global array
// ---------------------------------------------------------------------------

/// Statically sized array of per-CPU caches.  Initialised at compile time —
/// every slot starts empty.  CPU `id` addresses its slot with `id as usize`.
static mut PER_CPU_FRAME_CACHES: [PerCpuFrameCache; MAX_CPU_COUNT] =
    [PerCpuFrameCache::new(); MAX_CPU_COUNT];

// ---------------------------------------------------------------------------
// Public helpers (crate‑visible to memory::MemoryManager)
// ---------------------------------------------------------------------------

/// Try to pop one frame from the current CPU's cache.
/// Returns `Some(PhysFrame)` on hit, `None` if empty.
#[inline]
pub(crate) fn try_alloc() -> Option<PhysFrame> {
    let cpu = crate::cpu::local::current_cpu_id() as usize;
    if cpu >= MAX_CPU_COUNT {
        return None;
    }
    // SAFETY: each CPU only accesses its own slot; no concurrent mutation.
    let cache = unsafe {
        &mut *core::ptr::addr_of_mut!(PER_CPU_FRAME_CACHES)
            .cast::<PerCpuFrameCache>()
            .add(cpu)
    };
    let result = cache.pop().map(|addr| PhysFrame::containing_address(PhysAddr::new(addr)));
    if result.is_none() {
        crate::serial_println!(
            "[TRYALLOC-NONE] cpu={} count={} cache={:p}",
            cpu,
            cache.count(),
            cache as *const _ as *const u8
        );
    }
    result
}

/// Try to push one frame into the current CPU's cache.
/// Returns `true` on success, `false` if the cache is full.
#[inline]
pub(crate) fn try_free(addr: u64) -> bool {
    let cpu = crate::cpu::local::current_cpu_id() as usize;
    if cpu >= MAX_CPU_COUNT {
        return false;
    }
    let cache = unsafe {
        &mut *core::ptr::addr_of_mut!(PER_CPU_FRAME_CACHES)
            .cast::<PerCpuFrameCache>()
            .add(cpu)
    };
    cache.push(addr)
}

/// Refill the current CPU's cache from the global PMM.
///
/// Allocates up to `BATCH` order‑0 frames from `pmm` and fills the cache.
/// Called when `try_alloc()` returns `None`.
pub(crate) fn refill(pmm: &mut FibonacciPmm) {
    let cpu = crate::cpu::local::current_cpu_id() as usize;
    if cpu >= MAX_CPU_COUNT {
        crate::serial_println!("[REFILL-EARLY] cpu={} >= MAX_CPU_COUNT", cpu);
        return;
    }
    let cache = unsafe {
        &mut *core::ptr::addr_of_mut!(PER_CPU_FRAME_CACHES)
            .cast::<PerCpuFrameCache>()
            .add(cpu)
    };

    let mut filled = 0u32;
    while (filled as usize) < BATCH {
        match pmm.allocate_frame() {
            Some(frame) => {
                cache.frames[filled as usize] = frame.start_address().as_u64();
                filled += 1;
            }
            None => break,
        }
    }
    cache.set_count(filled);
    crate::serial_println!(
        "[REFILL-DONE] cpu={} filled={} cache={:p}",
        cpu,
        filled,
        cache as *const _ as *const u8
    );
}

/// Drain half the current CPU's cache back to the global PMM.
///
/// Called when `try_free()` returns `false` (cache full).
pub(crate) fn drain(pmm: &mut FibonacciPmm) {
    let cpu = crate::cpu::local::current_cpu_id() as usize;
    if cpu >= MAX_CPU_COUNT {
        return;
    }
    let cache = unsafe {
        &mut *core::ptr::addr_of_mut!(PER_CPU_FRAME_CACHES)
            .cast::<PerCpuFrameCache>()
            .add(cpu)
    };

    let keep = cache.count() / 2; // keep the hottest half
    while cache.count() > keep {
        if let Some(addr) = cache.pop() {
            pmm.deallocate_contiguous(
                PhysFrame::containing_address(PhysAddr::new(addr)),
                1,
            );
        }
    }
}

/// Drain /all/ per‑CPU caches back to the global PMM (used during OOM /
/// reclaim pressure where we need every free page accounted).
///
/// Returns the total number of frames drained.
pub(crate) fn drain_all(pmm: &mut FibonacciPmm) -> usize {
    let mut total = 0;
    for cpu in 0..MAX_CPU_COUNT {
        let cache = unsafe {
        &mut *core::ptr::addr_of_mut!(PER_CPU_FRAME_CACHES)
            .cast::<PerCpuFrameCache>()
            .add(cpu)
    };
        while let Some(addr) = cache.pop() {
            pmm.deallocate_contiguous(
                PhysFrame::containing_address(PhysAddr::new(addr)),
                1,
            );
            total += 1;
        }
    }
    total
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::fibonacci_pmm::FibonacciPmm;

    /// Verify basic pop/push round‑trip.
    #[test]
    fn test_cache_push_pop() {
        let mut cache = PerCpuFrameCache::new();
        assert!(cache.is_empty());
        assert!(cache.pop().is_none());

        assert!(cache.push(0x1000));
        assert!(cache.push(0x2000));
        assert!(!cache.is_empty());
        assert_eq!(cache.count(), 2);

        // LIFO order.
        assert_eq!(cache.pop(), Some(0x2000));
        assert_eq!(cache.pop(), Some(0x1000));
        assert!(cache.is_empty());
    }

    /// Verify that a full cache rejects pushes.
    #[test]
    fn test_cache_full() {
        let mut cache = PerCpuFrameCache::new();
        for i in 0..BATCH {
            assert!(cache.push((i as u64 + 1) * 0x1000));
        }
        // Next push should fail.
        assert!(!cache.push(0xDEAD_0000));
        assert_eq!(cache.count(), BATCH as u32);
    }

    /// Verify drain keeps half.
    #[test]
    fn test_cache_drain_keeps_half() {
        let mut cache = PerCpuFrameCache::new();
        for i in 0..BATCH {
            cache.push((i as u64 + 1) * 0x1000);
        }
        let before = cache.count();
        let keep = before / 2;
        while cache.count() > keep {
            let _ = cache.pop();
        }
        assert_eq!(cache.count(), keep);
    }
}
