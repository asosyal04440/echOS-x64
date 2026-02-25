//! # CFS (Completely Fair Scheduler)
//!
//! Linux-style fair scheduling with virtual runtime.

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// CFS CONSTANTS
// ============================================================================

/// Default time slice (microseconds)
pub const CFS_DEFAULT_SLICE: u64 = 1_000_000; // 1ms
/// Minimum granularity
pub const CFS_MIN_GRANULARITY: u64 = 1_000_000; // 1ms
/// Wakeup granularity
pub const CFS_WAKEUP_GRANULARITY: u64 = 1_000_000;
/// Default nice weight
pub const CFS_NICE_0_WEIGHT: u64 = 1024;
/// Load average period
pub const CFS_LOAD_AVG_PERIOD: u64 = 32;
/// Time constants for load tracking
pub const CFS_PELT_HALF_LIFE: u64 = 32; // 32ms

/// Nice to weight conversion
pub fn nice_to_weight(nice: i32) -> u64 {
    let weight = CFS_NICE_0_WEIGHT as i64;
    let delta = nice as i64;
    
    // Each nice level changes weight by ~25%
    let factor = 1.25_f64.powi(delta.abs() as i32);
    
    if delta > 0 {
        (weight as f64 / factor) as u64
    } else {
        (weight as f64 * factor) as u64
    }
}

/// Weight to vruntime delta
pub fn weight_to_vruntime(delta: u64, weight: u64) -> u64 {
    if weight == 0 {
        return delta;
    }
    (delta * CFS_NICE_0_WEIGHT) / weight
}

// ============================================================================
// CFS TASK
// ============================================================================

#[derive(Clone, Debug)]
pub struct CfsTask {
    /// Task ID
    pub task_id: u64,
    /// Nice value (-20 to 19)
    pub nice: AtomicI64,
    /// Weight
    pub weight: AtomicU64,
    /// Virtual runtime
    pub vruntime: AtomicU64,
    /// Actual runtime accumulated
    pub runtime: AtomicU64,
    /// Time slice
    pub slice: AtomicU64,
    /// Is running
    pub running: AtomicBool,
    /// Is on runqueue
    pub on_rq: AtomicBool,
    /// Last enqueue time
    pub enqueue_time: AtomicU64,
    /// Load average
    pub load_avg: AtomicU64,
    /// Utilization average
    pub util_avg: AtomicU64,
    /// Statistics
    pub stats: Mutex<CfsStats>,
}

#[derive(Clone, Debug, Default)]
pub struct CfsStats {
    pub wait_start: u64,
    pub wait_max: u64,
    pub wait_count: u64,
    pub wait_sum: u64,
    pub iowait_count: u64,
    pub iowait_sum: u64,
    pub slices: u64,
    pub migrations: u64,
}

impl CfsTask {
    pub fn new(task_id: u64, nice: i32) -> Self {
        Self {
            task_id,
            nice: AtomicI64::new(nice as i64),
            weight: AtomicU64::new(nice_to_weight(nice)),
            vruntime: AtomicU64::new(0),
            runtime: AtomicU64::new(0),
            slice: AtomicU64::new(CFS_DEFAULT_SLICE),
            running: AtomicBool::new(false),
            on_rq: AtomicBool::new(false),
            enqueue_time: AtomicU64::new(0),
            load_avg: AtomicU64::new(0),
            util_avg: AtomicU64::new(0),
            stats: Mutex::new(CfsStats::default()),
        }
    }

    /// Set nice value
    pub fn set_nice(&self, nice: i32) {
        self.nice.store(nice as i64, Ordering::SeqCst);
        self.weight.store(nice_to_weight(nice), Ordering::SeqCst);
    }

    /// Update vruntime after running
    pub fn update_vruntime(&self, delta: u64) {
        let weight = self.weight.load(Ordering::Relaxed);
        let vruntime_delta = weight_to_vruntime(delta, weight);
        self.vruntime.fetch_add(vruntime_delta, Ordering::Relaxed);
        self.runtime.fetch_add(delta, Ordering::Relaxed);
    }

    /// Get time slice based on weight
    pub fn calc_slice(&self, total_weight: u64, nr_running: u64) -> u64 {
        if nr_running == 0 {
            return CFS_DEFAULT_SLICE;
        }
        
        let weight = self.weight.load(Ordering::Relaxed);
        let slice = (weight * CFS_DEFAULT_SLICE * nr_running) / total_weight;
        
        slice.max(CFS_MIN_GRANULARITY)
    }

    /// Check if task is eligible to run (for buddy selection)
    pub fn is_eligible(&self, min_vruntime: u64) -> bool {
        self.vruntime.load(Ordering::Relaxed) <= min_vruntime
    }
}

// ============================================================================
// CFS RUN QUEUE
// ============================================================================

pub struct CfsRq {
    /// Tasks sorted by vruntime (rbtree simulation)
    pub tasks: Mutex<BTreeMap<u64, Arc<CfsTask>>>, // vruntime -> task
    /// Minimum vruntime
    pub min_vruntime: AtomicU64,
    /// Total weight
    pub total_weight: AtomicU64,
    /// Number of running tasks
    pub nr_running: AtomicU32,
    /// Currently running task
    pub curr: Mutex<Option<Arc<CfsTask>>>,
    /// Load average
    pub load_avg: AtomicU64,
    /// Utilization average
    pub util_avg: AtomicU64,
    /// Clock
    pub clock: AtomicU64,
}

impl CfsRq {
    pub fn new() -> Self {
        Self {
            tasks: Mutex::new(BTreeMap::new()),
            min_vruntime: AtomicU64::new(0),
            total_weight: AtomicU64::new(0),
            nr_running: AtomicU32::new(0),
            curr: Mutex::new(None),
            load_avg: AtomicU64::new(0),
            util_avg: AtomicU64::new(0),
            clock: AtomicU64::new(0),
        }
    }

    /// Enqueue task
    pub fn enqueue(&self, task: Arc<CfsTask>) {
        let vruntime = task.vruntime.load(Ordering::Relaxed);
        
        // Ensure vruntime is at least min_vruntime
        let min_vr = self.min_vruntime.load(Ordering::Relaxed);
        let adjusted_vr = vruntime.max(min_vr);
        
        task.vruntime.store(adjusted_vr, Ordering::SeqCst);
        task.on_rq.store(true, Ordering::SeqCst);
        task.enqueue_time.store(self.clock.load(Ordering::Relaxed), Ordering::SeqCst);
        
        self.tasks.lock().insert(adjusted_vr, task.clone());
        self.total_weight.fetch_add(task.weight.load(Ordering::Relaxed), Ordering::SeqCst);
        self.nr_running.fetch_add(1, Ordering::SeqCst);
    }

    /// Dequeue task
    pub fn dequeue(&self, task: &CfsTask) {
        let vruntime = task.vruntime.load(Ordering::Relaxed);
        
        self.tasks.lock().remove(&vruntime);
        self.total_weight.fetch_sub(task.weight.load(Ordering::Relaxed), Ordering::SeqCst);
        self.nr_running.fetch_sub(1, Ordering::SeqCst);
        task.on_rq.store(false, Ordering::SeqCst);
    }

    /// Pick next task (leftmost in rbtree)
    pub fn pick_next(&self) -> Option<Arc<CfsTask>> {
        let tasks = self.tasks.lock();
        
        // Get leftmost (lowest vruntime)
        if let Some((&vruntime, task)) = tasks.iter().next() {
            // Update min_vruntime
            self.min_vruntime.store(vruntime, Ordering::SeqCst);
            
            task.running.store(true, Ordering::SeqCst);
            *self.curr.lock() = Some(task.clone());
            
            return Some(task.clone());
        }
        
        None
    }

    /// Put prev task back
    pub fn put_prev(&self, task: &CfsTask) {
        task.running.store(false, Ordering::SeqCst);
        
        if task.on_rq.load(Ordering::Relaxed) {
            // Re-enqueue with updated vruntime
            let vruntime = task.vruntime.load(Ordering::Relaxed);
            self.tasks.lock().insert(vruntime, Arc::new(task.clone()));
        }
    }

    /// Update clock
    pub fn update_clock(&self, now: u64) {
        self.clock.store(now, Ordering::SeqCst);
    }

    /// Update load average (PELT - Per-Entity Load Tracking)
    pub fn update_load_avg(&self, task: &CfsTask, delta: u64) {
        // Simplified PELT calculation
        let weight = task.weight.load(Ordering::Relaxed);
        let contribution = weight * delta;
        
        task.load_avg.fetch_add(contribution, Ordering::Relaxed);
        self.load_avg.fetch_add(contribution, Ordering::Relaxed);
    }

    /// Check for buddy (preemption candidate)
    pub fn check_preempt_wakeup(&self, task: &CfsTask) -> bool {
        let curr = self.curr.lock();
        if let Some(curr_task) = curr.as_ref() {
            let curr_vr = curr_task.vruntime.load(Ordering::Relaxed);
            let task_vr = task.vruntime.load(Ordering::Relaxed);
            
            // Preempt if new task has significantly lower vruntime
            if task_vr + CFS_WAKEUP_GRANULARITY < curr_vr {
                return true;
            }
        }
        false
    }
}

// ============================================================================
// CFS SCHEDULER
// ============================================================================

pub struct CfsScheduler {
    /// Per-CPU run queues
    pub run_queues: Mutex<Vec<CfsRq>>,
    /// Number of CPUs
    pub nr_cpus: usize,
    /// Is enabled
    pub enabled: AtomicBool,
    /// Tick interval (nanoseconds)
    pub tick_interval: u64,
    /// Load balancer interval
    pub lb_interval: u64,
}

impl CfsScheduler {
    pub fn new(nr_cpus: usize) -> Self {
        let mut rqs = Vec::new();
        for _ in 0..nr_cpus {
            rqs.push(CfsRq::new());
        }
        
        Self {
            run_queues: Mutex::new(rqs),
            nr_cpus,
            enabled: AtomicBool::new(true),
            tick_interval: 1_000_000, // 1ms
            lb_interval: 100_000_000, // 100ms
        }
    }

    /// Schedule - pick next task
    pub fn schedule(&self, cpu: usize) -> Option<Arc<CfsTask>> {
        let rqs = self.run_queues.lock();
        if let Some(rq) = rqs.get(cpu) {
            rq.pick_next()
        } else {
            None
        }
    }

    /// Timer tick
    pub fn tick(&self, cpu: usize) {
        let rqs = self.run_queues.lock();
        if let Some(rq) = rqs.get(cpu) {
            let now = crate::task::scheduler::get_ticks();
            rq.update_clock(now);
            
            // Update current task
            let curr = rq.curr.lock();
            if let Some(task) = curr.as_ref() {
                task.update_vruntime(self.tick_interval);
                rq.update_load_avg(task, self.tick_interval);
                
                // Check if time slice expired
                let runtime = task.runtime.load(Ordering::Relaxed);
                let slice = task.slice.load(Ordering::Relaxed);
                
                if runtime >= slice {
                    // Reschedule needed
                    drop(curr);
                    rq.put_prev(task);
                }
            }
        }
    }

    /// Enqueue task
    pub fn enqueue(&self, task: Arc<CfsTask>, cpu: usize) {
        let rqs = self.run_queues.lock();
        if let Some(rq) = rqs.get(cpu) {
            rq.enqueue(task);
        }
    }

    /// Dequeue task
    pub fn dequeue(&self, task: &CfsTask, cpu: usize) {
        let rqs = self.run_queues.lock();
        if let Some(rq) = rqs.get(cpu) {
            rq.dequeue(task);
        }
    }

    /// Load balance between CPUs
    pub fn load_balance(&self) {
        // Find busiest and idlest CPUs
        let rqs = self.run_queues.lock();
        let mut busiest_load = 0u64;
        let mut busiest_cpu = 0;
        let mut idlest_load = u64::MAX;
        let mut idlest_cpu = 0;
        
        for (i, rq) in rqs.iter().enumerate() {
            let load = rq.load_avg.load(Ordering::Relaxed);
            
            if load > busiest_load {
                busiest_load = load;
                busiest_cpu = i;
            }
            
            if load < idlest_load {
                idlest_load = load;
                idlest_cpu = i;
            }
        }
        
        // Migrate tasks if imbalance
        if busiest_load > idlest_load * 2 {
            // Would migrate tasks here
        }
    }

    /// Set nice for task
    pub fn set_nice(&self, task: &CfsTask, nice: i32) {
        task.set_nice(nice);
    }
}

lazy_static::lazy_static! {
    pub static ref CFS_SCHEDULER: CfsScheduler = CfsScheduler::new(1);
}

// ============================================================================
// SYSCALL INTERFACE
// ============================================================================

pub fn sys_sched_setparam(pid: u64, nice: i32) -> i32 {
    // Find task and set nice
    nice.clamp(-20, 19);
    0
}

pub fn sys_sched_getparam(pid: u64) -> i32 {
    0 // Return nice value
}

pub fn sys_sched_yield() -> i32 {
    // Yield current task
    0
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init() {
    crate::serial_println!("[CFS] Completely Fair Scheduler initialized");
}
