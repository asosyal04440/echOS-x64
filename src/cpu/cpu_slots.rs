//! Cacheline-aligned CPU publication slots for lock-free SMP coordination.

use crate::cpu::smp_state::CpuHotplugState;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

pub const MAX_CPU_SLOTS: usize = 256;

#[repr(align(64))]
pub struct CpuSlot {
    online: AtomicBool,
    apic_id: AtomicU32,
    hotplug_state: AtomicU32,
    load: AtomicU32,
    package_id: AtomicU32,
    core_id: AtomicU32,
    numa_node: AtomicU32,
    tlb_request_seq: AtomicU64,
    tlb_ack_seq: AtomicU64,
}

impl CpuSlot {
    const fn new() -> Self {
        Self {
            online: AtomicBool::new(false),
            apic_id: AtomicU32::new(0),
            hotplug_state: AtomicU32::new(CpuHotplugState::Offline as u32),
            load: AtomicU32::new(0),
            package_id: AtomicU32::new(0),
            core_id: AtomicU32::new(0),
            numa_node: AtomicU32::new(0),
            tlb_request_seq: AtomicU64::new(0),
            tlb_ack_seq: AtomicU64::new(0),
        }
    }
}

static CPU_COUNT: AtomicU32 = AtomicU32::new(1);
static ONLINE_COUNT: AtomicU32 = AtomicU32::new(1);
static CPU_SLOTS: [CpuSlot; MAX_CPU_SLOTS] = [const { CpuSlot::new() }; MAX_CPU_SLOTS];

#[inline]
fn slot(cpu_id: u32) -> Option<&'static CpuSlot> {
    CPU_SLOTS.get(cpu_id as usize)
}

pub fn set_cpu_count(cpu_count: u32) {
    CPU_COUNT.store(
        cpu_count.min(MAX_CPU_SLOTS as u32).max(1),
        Ordering::Release,
    );
}

pub fn cpu_count() -> u32 {
    CPU_COUNT.load(Ordering::Acquire)
}

pub fn online_cpu_count() -> u32 {
    ONLINE_COUNT.load(Ordering::Acquire)
}

pub fn publish_presence(cpu_id: u32, apic_id: u32, online: bool) {
    if let Some(slot) = slot(cpu_id) {
        slot.apic_id.store(apic_id, Ordering::Release);
        let was_online = slot.online.swap(online, Ordering::AcqRel);
        match (was_online, online) {
            (false, true) => {
                ONLINE_COUNT.fetch_add(1, Ordering::AcqRel);
            }
            (true, false) => {
                ONLINE_COUNT.fetch_sub(1, Ordering::AcqRel);
            }
            _ => {}
        }
    }
}

pub fn publish_state(cpu_id: u32, state: CpuHotplugState) {
    if let Some(slot) = slot(cpu_id) {
        slot.hotplug_state.store(state as u32, Ordering::Release);
        if state.is_online() {
            slot.online.store(true, Ordering::Release);
        }
    }
}

pub fn publish_topology(cpu_id: u32, package_id: u32, core_id: u32, numa_node: u32) {
    if let Some(slot) = slot(cpu_id) {
        slot.package_id.store(package_id, Ordering::Release);
        slot.core_id.store(core_id, Ordering::Release);
        slot.numa_node.store(numa_node, Ordering::Release);
    }
}

pub fn set_load(cpu_id: u32, load: u32) {
    if let Some(slot) = slot(cpu_id) {
        slot.load.store(load, Ordering::Release);
    }
}

pub fn load(cpu_id: u32) -> u32 {
    slot(cpu_id)
        .map(|s| s.load.load(Ordering::Acquire))
        .unwrap_or(0)
}

pub fn is_online(cpu_id: u32) -> bool {
    slot(cpu_id)
        .map(|s| s.online.load(Ordering::Acquire))
        .unwrap_or(false)
}

pub fn apic_id(cpu_id: u32) -> u32 {
    slot(cpu_id)
        .map(|s| s.apic_id.load(Ordering::Acquire))
        .unwrap_or(cpu_id)
}

pub fn package_id(cpu_id: u32) -> u32 {
    slot(cpu_id)
        .map(|s| s.package_id.load(Ordering::Acquire))
        .unwrap_or(0)
}

pub fn core_id(cpu_id: u32) -> u32 {
    slot(cpu_id)
        .map(|s| s.core_id.load(Ordering::Acquire))
        .unwrap_or(0)
}

pub fn numa_node(cpu_id: u32) -> u32 {
    slot(cpu_id)
        .map(|s| s.numa_node.load(Ordering::Acquire))
        .unwrap_or(0)
}

pub fn publish_tlb_request(cpu_id: u32, seq: u64) {
    if let Some(slot) = slot(cpu_id) {
        slot.tlb_request_seq.store(seq, Ordering::Release);
    }
}

pub fn publish_tlb_ack(cpu_id: u32, seq: u64) {
    if let Some(slot) = slot(cpu_id) {
        slot.tlb_ack_seq.store(seq, Ordering::Release);
    }
}

pub fn tlb_ack(cpu_id: u32) -> u64 {
    slot(cpu_id)
        .map(|s| s.tlb_ack_seq.load(Ordering::Acquire))
        .unwrap_or(0)
}

pub fn online_apic_targets(exclude_apic_id: u32) -> Vec<u32> {
    let mut targets = Vec::new();
    let limit = cpu_count().min(MAX_CPU_SLOTS as u32);
    for cpu_id in 0..limit {
        if is_online(cpu_id) {
            let apic = apic_id(cpu_id);
            if apic != exclude_apic_id {
                targets.push(apic);
            }
        }
    }
    targets
}
