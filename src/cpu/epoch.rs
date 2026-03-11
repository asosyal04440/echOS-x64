//! Minimal no_std epoch tracking for SMP control-plane lock-free structures.

use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

pub const MAX_EPOCH_CPUS: usize = 256;

#[repr(align(64))]
struct EpochSlot {
    active: AtomicBool,
    observed_epoch: AtomicU64,
    retired_items: AtomicUsize,
}

impl EpochSlot {
    const fn new() -> Self {
        Self {
            active: AtomicBool::new(false),
            observed_epoch: AtomicU64::new(0),
            retired_items: AtomicUsize::new(0),
        }
    }
}

pub struct EpochDomain {
    global_epoch: AtomicU64,
    slots: [EpochSlot; MAX_EPOCH_CPUS],
}

impl EpochDomain {
    pub const fn new() -> Self {
        Self {
            global_epoch: AtomicU64::new(1),
            slots: [const { EpochSlot::new() }; MAX_EPOCH_CPUS],
        }
    }

    #[inline]
    pub fn enter(&self, cpu_id: u32) -> u64 {
        let idx = cpu_id as usize;
        if idx >= self.slots.len() {
            return self.global_epoch.load(Ordering::Acquire);
        }
        let epoch = self.global_epoch.load(Ordering::Acquire);
        let slot = &self.slots[idx];
        slot.observed_epoch.store(epoch, Ordering::Release);
        slot.active.store(true, Ordering::Release);
        epoch
    }

    #[inline]
    pub fn leave(&self, cpu_id: u32) {
        let idx = cpu_id as usize;
        if idx < self.slots.len() {
            self.slots[idx].active.store(false, Ordering::Release);
        }
    }

    #[inline]
    pub fn advance(&self) -> u64 {
        self.global_epoch
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1)
    }

    #[inline]
    pub fn retire(&self, cpu_id: u32, items: usize) -> u64 {
        let idx = cpu_id as usize;
        if idx < self.slots.len() && items != 0 {
            self.slots[idx]
                .retired_items
                .fetch_add(items, Ordering::AcqRel);
        }
        self.advance()
    }

    pub fn can_reclaim(&self, retire_epoch: u64, active_cpu_count: u32) -> bool {
        let active_limit = active_cpu_count.min(self.slots.len() as u32) as usize;
        for slot in self.slots.iter().take(active_limit) {
            if slot.active.load(Ordering::Acquire)
                && slot.observed_epoch.load(Ordering::Acquire) <= retire_epoch
            {
                return false;
            }
        }
        true
    }

    pub fn retired_items(&self, cpu_id: u32) -> usize {
        let idx = cpu_id as usize;
        if idx >= self.slots.len() {
            return 0;
        }
        self.slots[idx].retired_items.load(Ordering::Acquire)
    }
}

pub static SMP_EPOCH_DOMAIN: EpochDomain = EpochDomain::new();
