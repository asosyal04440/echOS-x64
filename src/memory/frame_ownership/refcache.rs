use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;
use x86_64::structures::paging::PhysFrame;
use x86_64::PhysAddr;

use super::FRAME_TABLE;
use crate::cpu::cpu_slots::MAX_CPU_SLOTS;

const REFCACHE_LOG2_SIZE: usize = 6;
const REFCACHE_SIZE: usize = 1 << REFCACHE_LOG2_SIZE;
const REFCACHE_MASK: usize = REFCACHE_SIZE - 1;
const REVIEW_QUEUE_SIZE: usize = 32;

#[derive(Clone, Copy)]
struct DeltaEntry {
    phys: u64,
    delta: i32,
}

#[derive(Clone, Copy)]
struct ReviewEntry {
    phys: u64,
    added_epoch: u64,
    dirty: bool,
}

pub(crate) struct RefCacheCpuState {
    cache: [DeltaEntry; REFCACHE_SIZE],
    review: [Option<ReviewEntry>; REVIEW_QUEUE_SIZE],
    review_count: usize,
    epoch: u64,
}

impl RefCacheCpuState {
    const fn new() -> Self {
        const EMPTY: DeltaEntry = DeltaEntry { phys: 0, delta: 0 };
        const NONE: Option<ReviewEntry> = None;
        Self {
            cache: [EMPTY; REFCACHE_SIZE],
            review: [NONE; REVIEW_QUEUE_SIZE],
            review_count: 0,
            epoch: 0,
        }
    }
}

static mut REFCACHE_CPU: [RefCacheCpuState; MAX_CPU_SLOTS] =
    [const { RefCacheCpuState::new() }; MAX_CPU_SLOTS];

static REFCACHE_GLOBAL_EPOCH: AtomicU64 = AtomicU64::new(0);

/// Review queue'dan çıkmış, global refcount'u sıfırlanmış fiziksel sayfalar.
/// Scheduler tick (timer interrupt context) içinde toplanır, idle loop'ta boşaltılır.
static DRAIN_LIST: Mutex<Vec<u64>> = Mutex::new(Vec::new());

#[inline]
fn current_cpu_slot() -> usize {
    let cpu = crate::cpu::smp::current_cpu_id() as usize;
    if cpu >= MAX_CPU_SLOTS {
        MAX_CPU_SLOTS - 1
    } else {
        cpu
    }
}

#[inline]
fn cache_slot(phys: u64) -> usize {
    ((phys >> 12) as usize) & REFCACHE_MASK
}

fn enqueue_review(cpu_id: usize, phys: u64, current_epoch: u64) {
    let cpu = unsafe { &mut REFCACHE_CPU[cpu_id] };
    if cpu.review_count < REVIEW_QUEUE_SIZE {
        cpu.review[cpu.review_count] = Some(ReviewEntry {
            phys,
            added_epoch: current_epoch,
            dirty: false,
        });
        cpu.review_count += 1;
    }
}

fn remove_review(cpu_id: usize, idx: usize) {
    let cpu = unsafe { &mut REFCACHE_CPU[cpu_id] };
    let last = cpu.review_count - 1;
    if idx != last {
        cpu.review[idx] = cpu.review[last].take();
    }
    cpu.review_count -= 1;
}

pub(crate) fn inc(phys: u64) {
    let cpu_id = current_cpu_slot();
    let slot = cache_slot(phys);
    let cpu = unsafe { &mut REFCACHE_CPU[cpu_id] };
    let entry = &mut cpu.cache[slot];

    if entry.phys == (phys & !(0xFFF)) {
        entry.delta = entry.delta.saturating_add(1);
    } else {
        let cpu = unsafe { &mut REFCACHE_CPU[cpu_id] };
        let old = cpu.cache[slot];
        if old.phys != 0 && old.delta != 0 {
            let new_global = super::refcache_flush_delta(old.phys, old.delta);
            if new_global == 0 {
                enqueue_review(cpu_id, old.phys, cpu.epoch);
            }
        }
        cpu.cache[slot] = DeltaEntry {
            phys: phys & !(0xFFF),
            delta: 1,
        };
    }
}

pub(crate) fn dec(phys: u64) {
    let cpu_id = current_cpu_slot();
    let slot = cache_slot(phys);
    let cpu = unsafe { &mut REFCACHE_CPU[cpu_id] };
    let entry = &mut cpu.cache[slot];

    if entry.phys == (phys & !(0xFFF)) {
        entry.delta = entry.delta.saturating_sub(1);
    } else {
        let cpu = unsafe { &mut REFCACHE_CPU[cpu_id] };
        let old = cpu.cache[slot];
        if old.phys != 0 && old.delta != 0 {
            let new_global = super::refcache_flush_delta(old.phys, old.delta);
            if new_global == 0 {
                enqueue_review(cpu_id, old.phys, cpu.epoch);
            }
        }
        cpu.cache[slot] = DeltaEntry {
            phys: phys & !(0xFFF),
            delta: -1,
        };
    }
}

pub(crate) fn flush() {
    let cpu_id = current_cpu_slot();
    let cpu = unsafe { &mut REFCACHE_CPU[cpu_id] };
    for slot in 0..REFCACHE_SIZE {
        let entry = cpu.cache[slot];
        if entry.phys == 0 {
            continue;
        }
        if entry.delta != 0 {
            let new_global = super::refcache_flush_delta(entry.phys, entry.delta);
            if new_global == 0 {
                enqueue_review(cpu_id, entry.phys, cpu.epoch);
            }
        }
        cpu.cache[slot].delta = 0;
    }
}

pub(crate) fn tick() {
    let cpu_id = current_cpu_slot();
    flush();

    let cpu = unsafe { &mut REFCACHE_CPU[cpu_id] };
    let global_epoch = REFCACHE_GLOBAL_EPOCH.load(Ordering::Acquire);
    cpu.epoch = global_epoch;
    REFCACHE_GLOBAL_EPOCH.store(global_epoch.wrapping_add(1), Ordering::Release);

    let mut i = 0;
    while i < cpu.review_count {
        let review_entry = match &cpu.review[i] {
            Some(e) => *e,
            None => {
                i += 1;
                continue;
            }
        };
        let epochs_passed = global_epoch.wrapping_sub(review_entry.added_epoch);
        if epochs_passed < 2 {
            i += 1;
            continue;
        }
        let freed = super::refcache_try_free(review_entry.phys);
        if freed {
            DRAIN_LIST.lock().push(review_entry.phys);
            remove_review(cpu_id, i);
        } else if review_entry.dirty {
            cpu.review[i] = Some(ReviewEntry {
                phys: review_entry.phys,
                added_epoch: global_epoch,
                dirty: false,
            });
            i += 1;
        } else {
            remove_review(cpu_id, i);
        }
    }
}

pub(crate) fn drain() {
    let to_free = {
        let mut list = DRAIN_LIST.lock();
        core::mem::take(&mut *list)
    };
    for phys in to_free {
        let frame = PhysFrame::containing_address(PhysAddr::new(phys));
        crate::memory::deallocate_contiguous_frames(frame, 1);
    }
}

pub(crate) fn init_frame(phys: u64) {
    super::refcache_init_global(phys);
}

pub(crate) fn refcount(phys: u64) -> u32 {
    let cpu_id = current_cpu_slot();
    let slot = cache_slot(phys);
    let cpu = unsafe { &REFCACHE_CPU[cpu_id] };
    let cached = cpu.cache[slot];
    let local_delta = if cached.phys == (phys & !(0xFFF)) {
        cached.delta
    } else {
        0
    };
    let global = FRAME_TABLE.lock().refcount(phys);
    if local_delta >= 0 {
        global + local_delta as u32
    } else {
        global.saturating_sub((-local_delta) as u32)
    }
}
