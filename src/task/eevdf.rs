//! EEVDF scheduler çekirdeği (Earliest Eligible Virtual Deadline First).
//!
//! Bu modül Faz-I scheduler backlog'u için:
//! - lag tracking
//! - runqueue başına sanal zaman (vtime)
//! - deadline tabanlı preemption
//! sağlar.

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cmp::Ordering as CmpOrdering;
use core::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use spin::Mutex;

#[derive(Debug)]
pub struct EevdfTask {
    pub task_id: u64,
    pub weight: u64,
    pub vruntime: AtomicU64,
    pub lag: AtomicI64,
    pub slice_ns: AtomicU64,
    pub eligible_vtime: AtomicU64,
    pub virtual_deadline: AtomicU64,
    pub on_rq: AtomicBool,
}

impl EevdfTask {
    pub fn new(task_id: u64, weight: u64, slice_ns: u64) -> Self {
        let safe_weight = weight.max(1);
        let safe_slice = slice_ns.max(1);
        Self {
            task_id,
            weight: safe_weight,
            vruntime: AtomicU64::new(0),
            lag: AtomicI64::new(0),
            slice_ns: AtomicU64::new(safe_slice),
            eligible_vtime: AtomicU64::new(0),
            virtual_deadline: AtomicU64::new(safe_slice),
            on_rq: AtomicBool::new(false),
        }
    }

    pub fn update_runtime(&self, delta_ns: u64, rq_vtime: u64) {
        let delta_v = delta_ns.saturating_mul(1024) / self.weight.max(1);
        let vr = self.vruntime.fetch_add(delta_v, Ordering::SeqCst) + delta_v;
        let lag = rq_vtime as i64 - vr as i64;
        self.lag.store(lag, Ordering::SeqCst);
        let slice = self.slice_ns.load(Ordering::Relaxed).max(1);
        let eligible = if lag >= 0 { rq_vtime } else { vr };
        self.eligible_vtime.store(eligible, Ordering::SeqCst);
        self.virtual_deadline
            .store(eligible.saturating_add(slice), Ordering::SeqCst);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct DeadlineKey {
    vd: u64,
    task_id: u64,
}

#[derive(Debug, Default, Clone)]
pub struct EevdfStats {
    pub tasks: usize,
    pub vtime: u64,
    pub min_deadline: u64,
}

pub struct EevdfRunQueue {
    vtime: AtomicU64,
    tasks: Mutex<BTreeMap<u64, Arc<EevdfTask>>>,
    by_deadline: Mutex<BTreeMap<DeadlineKey, Arc<EevdfTask>>>,
}

impl EevdfRunQueue {
    pub fn new() -> Self {
        Self {
            vtime: AtomicU64::new(0),
            tasks: Mutex::new(BTreeMap::new()),
            by_deadline: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn vtime(&self) -> u64 {
        self.vtime.load(Ordering::Acquire)
    }

    pub fn enqueue(&self, task: Arc<EevdfTask>) {
        let rq_vtime = self.vtime();
        task.update_runtime(0, rq_vtime);
        task.on_rq.store(true, Ordering::Release);
        let key = DeadlineKey {
            vd: task.virtual_deadline.load(Ordering::Acquire),
            task_id: task.task_id,
        };
        self.tasks.lock().insert(task.task_id, task.clone());
        self.by_deadline.lock().insert(key, task);
    }

    pub fn dequeue(&self, task_id: u64) -> Option<Arc<EevdfTask>> {
        let task = self.tasks.lock().remove(&task_id)?;
        let key = DeadlineKey {
            vd: task.virtual_deadline.load(Ordering::Acquire),
            task_id,
        };
        self.by_deadline.lock().remove(&key);
        task.on_rq.store(false, Ordering::Release);
        Some(task)
    }

    pub fn account_runtime(&self, task_id: u64, delta_ns: u64) {
        let task = self.tasks.lock().get(&task_id).cloned();
        let Some(task) = task else {
            return;
        };

        let old_key = DeadlineKey {
            vd: task.virtual_deadline.load(Ordering::Acquire),
            task_id,
        };
        self.by_deadline.lock().remove(&old_key);

        let delta_vtime = delta_ns.saturating_mul(1024) / task.weight.max(1);
        let next_vtime = self.vtime().saturating_add(delta_vtime);
        self.vtime.store(next_vtime, Ordering::Release);
        task.update_runtime(delta_ns, next_vtime);

        let new_key = DeadlineKey {
            vd: task.virtual_deadline.load(Ordering::Acquire),
            task_id,
        };
        self.by_deadline.lock().insert(new_key, task);
    }

    pub fn pick_next(&self) -> Option<Arc<EevdfTask>> {
        let rq_vtime = self.vtime();
        for (_, task) in self.by_deadline.lock().iter() {
            if task.eligible_vtime.load(Ordering::Acquire) <= rq_vtime {
                return Some(task.clone());
            }
        }
        None
    }

    pub fn should_preempt(&self, current_task_id: u64, wakee_task_id: u64) -> bool {
        let tasks = self.tasks.lock();
        let current = match tasks.get(&current_task_id) {
            Some(t) => t,
            None => return false,
        };
        let wakee = match tasks.get(&wakee_task_id) {
            Some(t) => t,
            None => return false,
        };

        let current_vd = current.virtual_deadline.load(Ordering::Acquire);
        let wakee_vd = wakee.virtual_deadline.load(Ordering::Acquire);
        let wakee_eligible = wakee.eligible_vtime.load(Ordering::Acquire) <= self.vtime();
        wakee_eligible && wakee_vd < current_vd
    }

    pub fn stats(&self) -> EevdfStats {
        let by_deadline = self.by_deadline.lock();
        let min_deadline = by_deadline.iter().next().map(|(k, _)| k.vd).unwrap_or(0);
        EevdfStats {
            tasks: by_deadline.len(),
            vtime: self.vtime(),
            min_deadline,
        }
    }

    pub fn ordered_task_ids(&self) -> Vec<u64> {
        self.by_deadline
            .lock()
            .iter()
            .map(|(_, task)| task.task_id)
            .collect()
    }
}
