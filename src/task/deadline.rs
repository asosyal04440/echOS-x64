//! # Deadline Scheduler (EDF)
//!
//! Earliest Deadline First real-time scheduling.

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use spin::Mutex;

// ============================================================================
// DEADLINE CONSTANTS
// ============================================================================

/// Default runtime (microseconds)
pub const DL_DEFAULT_RUNTIME: u64 = 100_000; // 100ms
/// Default period (microseconds)
pub const DL_DEFAULT_PERIOD: u64 = 1_000_000; // 1s
/// Default deadline equals period
pub const DL_DEFAULT_DEADLINE: u64 = DL_DEFAULT_PERIOD;

/// Deadline flags
pub const SCHED_DEADLINE: i32 = 6;

/// Flags for sched_attr
pub const SCHED_FLAG_DL_OVERRUN: u64 = 1 << 0;
pub const SCHED_FLAG_DL_RECLAIM: u64 = 1 << 1;
pub const SCHED_FLAG_DL_SPECIAL: u64 = 1 << 2;

// ============================================================================
// DEADLINE TASK
// ============================================================================

#[derive(Clone, Debug)]
pub struct DeadlineTask {
    /// Task ID
    pub task_id: u64,
    /// Runtime budget (nanoseconds)
    pub runtime: AtomicU64,
    /// Remaining runtime
    pub remaining_runtime: AtomicU64,
    /// Period (nanoseconds)
    pub period: u64,
    /// Relative deadline (nanoseconds)
    pub deadline: u64,
    /// Absolute deadline (monotonic clock)
    pub abs_deadline: AtomicU64,
    /// Next replenishment time
    pub next_replenish: AtomicU64,
    /// Is active
    pub active: AtomicBool,
    /// Is throttled
    pub throttled: AtomicBool,
    /// Flags
    pub flags: u64,
    /// Statistics
    pub stats: Mutex<DlStats>,
}

#[derive(Clone, Debug, Default)]
pub struct DlStats {
    pub migrations: u64,
    pub throttled_time: u64,
    pub runtime_time: u64,
    pub deadline_misses: u64,
}

impl DeadlineTask {
    pub fn new(task_id: u64, runtime: u64, period: u64, deadline: u64, flags: u64) -> Self {
        let now = crate::task::scheduler::get_ticks();
        
        Self {
            task_id,
            runtime: AtomicU64::new(runtime),
            remaining_runtime: AtomicU64::new(runtime),
            period,
            deadline,
            abs_deadline: AtomicU64::new(now + deadline),
            next_replenish: AtomicU64::new(now + period),
            active: AtomicBool::new(true),
            throttled: AtomicBool::new(false),
            flags,
            stats: Mutex::new(DlStats::default()),
        }
    }

    /// Check if deadline has passed
    pub fn deadline_passed(&self) -> bool {
        let now = crate::task::scheduler::get_ticks();
        now > self.abs_deadline.load(Ordering::Relaxed)
    }

    /// Check if runtime exhausted
    pub fn runtime_exhausted(&self) -> bool {
        self.remaining_runtime.load(Ordering::Relaxed) == 0
    }

    /// Consume runtime
    pub fn consume_runtime(&self, ns: u64) {
        let remaining = self.remaining_runtime.load(Ordering::Relaxed);
        let new_remaining = remaining.saturating_sub(ns);
        self.remaining_runtime.store(new_remaining, Ordering::Relaxed);
        
        if new_remaining == 0 {
            self.throttled.store(true, Ordering::SeqCst);
        }
    }

    /// Replenish budget
    pub fn replenish(&self) {
        let now = crate::task::scheduler::get_ticks();
        let runtime = self.runtime.load(Ordering::Relaxed);
        
        // Set new deadline
        let new_deadline = now + self.deadline;
        self.abs_deadline.store(new_deadline, Ordering::SeqCst);
        
        // Replenish runtime
        self.remaining_runtime.store(runtime, Ordering::SeqCst);
        
        // Set next replenishment
        self.next_replenish.store(now + self.period, Ordering::SeqCst);
        
        // Unthrottle
        self.throttled.store(false, Ordering::SeqCst);
        
        crate::serial_println!("[DL] Task {} replenished, deadline={}", 
            self.task_id, new_deadline);
    }

    /// Get laxity (time until deadline minus remaining runtime)
    pub fn laxity(&self) -> i64 {
        let now = crate::task::scheduler::get_ticks();
        let deadline = self.abs_deadline.load(Ordering::Relaxed) as i64;
        let remaining = self.remaining_runtime.load(Ordering::Relaxed) as i64;
        
        deadline - now as i64 - remaining
    }

    /// Compare deadlines for EDF ordering
    pub fn compare_deadline(&self, other: &DeadlineTask) -> core::cmp::Ordering {
        self.abs_deadline.load(Ordering::Relaxed)
            .cmp(&other.abs_deadline.load(Ordering::Relaxed))
    }
}

// ============================================================================
// DEADLINE RUN QUEUE
// ============================================================================

pub struct DeadlineRq {
    /// Tasks sorted by deadline
    pub tasks: Mutex<BTreeMap<u64, Arc<DeadlineTask>>>, // deadline -> task
    /// Running task
    pub running: Mutex<Option<Arc<DeadlineTask>>>,
    /// Total bandwidth
    pub total_bw: AtomicU64,
    /// Maximum bandwidth (percentage * 100)
    pub max_bw: u64, // 10000 = 100%
}

impl DeadlineRq {
    pub fn new() -> Self {
        Self {
            tasks: Mutex::new(BTreeMap::new()),
            running: Mutex::new(None),
            total_bw: AtomicU64::new(0),
            max_bw: 10000, // 100%
        }
    }

    /// Add task to run queue
    pub fn enqueue(&self, task: Arc<DeadlineTask>) -> Result<(), DlError> {
        // Check bandwidth
        let task_bw = self.compute_bandwidth(&task);
        let current_bw = self.total_bw.load(Ordering::Relaxed);
        
        if current_bw + task_bw > self.max_bw {
            return Err(DlError::BandwidthExceeded);
        }
        
        self.total_bw.fetch_add(task_bw, Ordering::Relaxed);
        
        let deadline = task.abs_deadline.load(Ordering::Relaxed);
        self.tasks.lock().insert(deadline, task);
        
        Ok(())
    }

    /// Remove task from run queue
    pub fn dequeue(&self, task: &DeadlineTask) {
        let deadline = task.abs_deadline.load(Ordering::Relaxed);
        self.tasks.lock().remove(&deadline);
        
        let task_bw = self.compute_bandwidth(task);
        self.total_bw.fetch_sub(task_bw, Ordering::Relaxed);
    }

    /// Get earliest deadline task
    pub fn pick_next(&self) -> Option<Arc<DeadlineTask>> {
        let tasks = self.tasks.lock();
        
        // Find earliest deadline that is not throttled
        for task in tasks.values() {
            if !task.throttled.load(Ordering::Relaxed) && 
               task.active.load(Ordering::Relaxed) {
                return Some(task.clone());
            }
        }
        
        None
    }

    /// Compute bandwidth (percentage * 100)
    fn compute_bandwidth(&self, task: &DeadlineTask) -> u64 {
        let runtime = task.runtime.load(Ordering::Relaxed);
        let period = task.period;
        
        if period == 0 {
            return 0;
        }
        
        // bw = (runtime / period) * 10000
        (runtime * 10000) / period
    }

    /// Check for replenishments
    pub fn check_replenishments(&self) {
        let now = crate::task::scheduler::get_ticks();
        
        for task in self.tasks.lock().values() {
            if task.next_replenish.load(Ordering::Relaxed) <= now {
                task.replenish();
            }
        }
    }

    /// Check for deadline misses
    pub fn check_deadline_misses(&self) {
        for task in self.tasks.lock().values() {
            if task.deadline_passed() && !task.throttled.load(Ordering::Relaxed) {
                let mut stats = task.stats.lock();
                stats.deadline_misses += 1;
                
                crate::serial_println!(
                    "[DL] Task {} missed deadline!",
                    task.task_id
                );
            }
        }
    }
}

// ============================================================================
// DEADLINE SCHEDULER
// ============================================================================

pub struct DeadlineScheduler {
    /// Per-CPU run queues
    pub run_queues: Mutex<Vec<DeadlineRq>>,
    /// Number of CPUs
    pub nr_cpus: usize,
    /// Is enabled
    pub enabled: AtomicBool,
    /// Tick interval (nanoseconds)
    pub tick_interval: u64,
}

impl DeadlineScheduler {
    pub fn new(nr_cpus: usize) -> Self {
        let mut rqs = Vec::new();
        for _ in 0..nr_cpus {
            rqs.push(DeadlineRq::new());
        }
        
        Self {
            run_queues: Mutex::new(rqs),
            nr_cpus,
            enabled: AtomicBool::new(true),
            tick_interval: 1_000_000, // 1ms
        }
    }

    /// Schedule next task
    pub fn schedule(&self, cpu: usize) -> Option<Arc<DeadlineTask>> {
        let rqs = self.run_queues.lock();
        if let Some(rq) = rqs.get(cpu) {
            rq.check_replenishments();
            rq.pick_next()
        } else {
            None
        }
    }

    /// Add task
    pub fn add_task(&self, task: Arc<DeadlineTask>, cpu: usize) -> Result<(), DlError> {
        let rqs = self.run_queues.lock();
        if let Some(rq) = rqs.get(cpu) {
            rq.enqueue(task)
        } else {
            Err(DlError::InvalidCpu)
        }
    }

    /// Remove task
    pub fn remove_task(&self, task: &DeadlineTask, cpu: usize) {
        let rqs = self.run_queues.lock();
        if let Some(rq) = rqs.get(cpu) {
            rq.dequeue(task);
        }
    }

    /// Timer tick
    pub fn tick(&self, cpu: usize) {
        let rqs = self.run_queues.lock();
        if let Some(rq) = rqs.get(cpu) {
            rq.check_replenishments();
            rq.check_deadline_misses();
            
            // Update running task
            if let Some(running) = rq.running.lock().as_ref() {
                running.consume_runtime(self.tick_interval);
                
                if running.throttled.load(Ordering::Relaxed) {
                    // Need to reschedule
                    // self.reschedule(cpu);
                }
            }
        }
    }

    /// Set bandwidth cap
    pub fn set_bandwidth_cap(&self, cap: u64) {
        // cap is percentage * 100 (e.g., 9000 = 90%)
        for rq in self.run_queues.lock().iter_mut() {
            rq.max_bw = cap;
        }
    }
}

lazy_static::lazy_static! {
    pub static ref DL_SCHEDULER: DeadlineScheduler = DeadlineScheduler::new(1);
}

// ============================================================================
// ERROR TYPE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DlError {
    BandwidthExceeded,
    InvalidCpu,
    TaskNotFound,
    InvalidParameters,
}

// ============================================================================
// SYSCALL INTERFACE
// ============================================================================

pub fn sys_sched_setattr(pid: u64, runtime: u64, period: u64, deadline: u64, flags: u64) -> i32 {
    let task = Arc::new(DeadlineTask::new(pid, runtime, period, deadline, flags));
    
    match DL_SCHEDULER.add_task(task, 0) {
        Ok(()) => 0,
        Err(DlError::BandwidthExceeded) => -16, // EBUSY
        Err(_) => -22,
    }
}

pub fn sys_sched_getattr(pid: u64, attr: &mut SchedAttr) -> i32 {
    // Find task and fill attr
    attr.sched_policy = SCHED_DEADLINE;
    attr.sched_runtime = DL_DEFAULT_RUNTIME;
    attr.sched_period = DL_DEFAULT_PERIOD;
    attr.sched_deadline = DL_DEFAULT_DEADLINE;
    0
}

#[repr(C)]
pub struct SchedAttr {
    pub sched_policy: i32,
    pub sched_flags: u64,
    pub sched_nice: i32,
    pub sched_priority: u32,
    pub sched_runtime: u64,
    pub sched_deadline: u64,
    pub sched_period: u64,
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init() {
    crate::serial_println!("[DL] Deadline scheduler initialized");
}
