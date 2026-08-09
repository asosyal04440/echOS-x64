use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicU16, Ordering};

/// ── Lock ranks ──────────────────────────────────────────────
pub const RANK_MM_LIFECYCLE:   u8 = 1;
pub const RANK_VMA_INTERVAL:   u8 = 2;
pub const RANK_PAGE_TABLE:     u8 = 3;
pub const RANK_FRAME_METADATA: u8 = 4;
pub const RANK_PAGE_CACHE:     u8 = 5;
pub const RANK_LRU:            u8 = 6;
pub const RANK_SWAP:           u8 = 7;

/// Maximum number of CPU slots tracked for lockdep.
const MAX_CPU_SLOTS: usize = 64;

/// Per-CPU bitmask of currently held lock ranks.
/// Bit N (0-indexed) corresponds to rank N+1.
#[cfg(debug_assertions)]
static CPU_RANK_MASK: [AtomicU16; MAX_CPU_SLOTS] =
    [const { AtomicU16::new(0) }; MAX_CPU_SLOTS];

#[cfg(debug_assertions)]
fn current_mask() -> &'static AtomicU16 {
    let cpu = crate::cpu::smp::current_cpu_id() as usize;
    let idx = cpu.min(MAX_CPU_SLOTS - 1);
    &CPU_RANK_MASK[idx]
}

/// Record the acquisition of `rank`.  Panics if a higher-level
/// (lower-numbered) lock is already held on this CPU.
///
/// # Panics
/// In debug builds, panics on ordering violation.
#[cfg(debug_assertions)]
pub fn acquire_rank(rank: u8) {
    let mask = current_mask();
    let held = mask.load(Ordering::Relaxed);
    let higher = (1u16 << (rank.saturating_sub(1))) - 1;
    if held & higher != 0 {
        panic!(
            "LOCKDEP: rank {} acquired but higher-level locks ({:016b}) held on CPU {}",
            rank,
            held & higher,
            crate::cpu::smp::current_cpu_id(),
        );
    }
    mask.fetch_or(1u16 << (rank.saturating_sub(1)), Ordering::Relaxed);
}

/// Record the release of `rank`.
#[cfg(debug_assertions)]
pub fn release_rank(rank: u8) {
    let mask = current_mask();
    let bit = 1u16 << (rank.saturating_sub(1));
    mask.fetch_and(!bit, Ordering::Relaxed);
}

/// Release‑build stubs — zero overhead.
#[cfg(not(debug_assertions))]
pub fn acquire_rank(_rank: u8) {}
#[cfg(not(debug_assertions))]
pub fn release_rank(_rank: u8) {}

/// A `spin::Mutex` wrapper that checks lock ordering in debug builds.
///
/// `RANK` is the compile‑time constant rank (1 = outermost, 7 = innermost
/// for memory‑subsystem locks).
pub struct RankedMutex<T, const RANK: u8> {
    inner: spin::Mutex<T>,
}

impl<T, const RANK: u8> RankedMutex<T, RANK> {
    pub const fn new(val: T) -> Self {
        RankedMutex {
            inner: spin::Mutex::new(val),
        }
    }

    pub fn lock(&self) -> RankedMutexGuard<'_, T, RANK> {
        acquire_rank(RANK);
        RankedMutexGuard {
            inner: self.inner.lock(),
        }
    }
}

/// Guard for [`RankedMutex`].
pub struct RankedMutexGuard<'a, T, const RANK: u8> {
    inner: spin::MutexGuard<'a, T>,
}

impl<T, const RANK: u8> Drop for RankedMutexGuard<'_, T, RANK> {
    fn drop(&mut self) {
        release_rank(RANK);
    }
}

impl<T, const RANK: u8> core::ops::Deref for RankedMutexGuard<'_, T, RANK> {
    type Target = T;
    fn deref(&self) -> &T {
        self.inner.deref()
    }
}

impl<T, const RANK: u8> core::ops::DerefMut for RankedMutexGuard<'_, T, RANK> {
    fn deref_mut(&mut self) -> &mut T {
        self.inner.deref_mut()
    }
}
