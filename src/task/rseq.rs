use crate::cpu::{cpu_slots, smp::current_cpu_id};
use crate::task::scheduler::PER_CPU_CURRENT_TASK;
use crate::task::task::RseqState;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RseqUserArea {
    pub cpu_id_start: u32,
    pub cpu_id: u32,
    pub rseq_cs: u64,
    pub flags: u32,
}

fn current_snapshot(state: &RseqState) -> RseqUserArea {
    RseqUserArea {
        cpu_id_start: state.cpu_id_start,
        cpu_id: state.cpu_id,
        rseq_cs: 0,
        flags: state.flags,
    }
}

pub fn sync_user_area(state: &RseqState) {
    if !state.registered || state.area_ptr == 0 || state.area_len < core::mem::size_of::<RseqUserArea>() as u32 {
        return;
    }

    let snapshot = current_snapshot(state);
    unsafe {
        core::ptr::write_volatile(state.area_ptr as *mut RseqUserArea, snapshot);
    }
}

fn with_current_rseq_mut<T>(f: impl FnOnce(&mut RseqState) -> T) -> Result<T, i64> {
    let cpu_id = current_cpu_id() as usize;
    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        let Some(current) = PER_CPU_CURRENT_TASK
            .get_mut(cpu_id)
            .and_then(|slot| slot.as_mut())
        else {
            return Err(-3);
        };
        Ok(f(&mut current.cold.rseq))
    })
}

pub fn refresh_current_cpu_state() {
    let cpu_id = current_cpu_id();
    let node_id = cpu_slots::numa_node(cpu_id);
    let _ = with_current_rseq_mut(|state| {
        if !state.registered {
            return;
        }
        state.cpu_id_start = cpu_id;
        state.cpu_id = cpu_id;
        state.numa_node = node_id;
        state.event_counter = state.event_counter.saturating_add(1);
        sync_user_area(state);
    });
}

pub fn note_migration(previous_cpu: u32, current_cpu: u32) {
    if previous_cpu == current_cpu {
        refresh_current_cpu_state();
        return;
    }

    let node_id = cpu_slots::numa_node(current_cpu);
    let _ = with_current_rseq_mut(|state| {
        if !state.registered {
            return;
        }
        state.abort_count = state.abort_count.saturating_add(1);
        state.event_counter = state.event_counter.saturating_add(1);
        state.cpu_id_start = current_cpu;
        state.cpu_id = current_cpu;
        state.numa_node = node_id;
        sync_user_area(state);
    });
}

pub fn sys_rseq(rseq_ptr: u64, rseq_len: u32, flags: u32, sig: u32) -> i64 {
    if flags != 0 {
        return -22;
    }

    let cpu_id = current_cpu_id();
    let node_id = cpu_slots::numa_node(cpu_id);

    with_current_rseq_mut(|state| {
        if rseq_ptr == 0 && rseq_len == 0 {
            *state = RseqState::default();
            return 0;
        }

        if rseq_ptr == 0 || rseq_len < core::mem::size_of::<RseqUserArea>() as u32 {
            return -22;
        }

        state.registered = true;
        state.area_ptr = rseq_ptr;
        state.area_len = rseq_len;
        state.signature = sig;
        state.flags = flags;
        state.cpu_id_start = cpu_id;
        state.cpu_id = cpu_id;
        state.numa_node = node_id;
        state.event_counter = state.event_counter.saturating_add(1);
        sync_user_area(state);
        0
    })
    .unwrap_or(-3)
}

pub fn registered_state(task_id: usize) -> Option<RseqState> {
    let mut snapshot = None;
    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        for slot in PER_CPU_CURRENT_TASK.iter() {
            if let Some(task) = slot.as_ref() {
                if task.id() == task_id && task.cold.rseq.registered {
                    snapshot = Some(task.cold.rseq);
                    break;
                }
            }
        }
    });
    snapshot
}
