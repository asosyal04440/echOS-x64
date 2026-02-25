//! # Real-Time Scheduler
//!
//! POSIX gerçek zamanlı zamanlayıcı implementasyonu.
//! SCHED_FIFO ve SCHED_RR (Round-Robin) destekler.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use alloc::boxed::Box;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

use super::task::{Task, TaskId, TaskState};

// ============================================================================
// REAL-TIME SCHEDULING CONSTANTS
// ============================================================================

/// Minimum real-time priority (Linux: 1)
pub const RT_PRIO_MIN: i32 = 1;

/// Maximum real-time priority (Linux: 99)
pub const RT_PRIO_MAX: i32 = 99;

/// Default time slice for SCHED_RR (in ticks)
/// Linux default: 100ms (typically 100 ticks at 1000Hz)
pub const RR_DEFAULT_TIMESLICE: u64 = 100;

/// Maximum time slice for SCHED_RR
pub const RR_MAX_TIMESLICE: u64 = 200;

/// Minimum time slice for SCHED_RR
pub const RR_MIN_TIMESLICE: u64 = 10;

// ============================================================================
// SCHEDULING POLICY
// ============================================================================

/// Scheduling policy types
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum SchedPolicy {
    /// Normal scheduling (CFS-like)
    Normal = 0,
    /// First-In-First-Out real-time scheduling
    Fifo = 1,
    /// Round-Robin real-time scheduling
    RoundRobin = 2,
    /// Deadline scheduling (EDF)
    Deadline = 3,
    /// Idle scheduling (very low priority)
    Idle = 4,
    /// Batch scheduling (CPU-intensive)
    Batch = 5,
}

impl Default for SchedPolicy {
    fn default() -> Self {
        SchedPolicy::Normal
    }
}

/// Real-time scheduling parameters
#[derive(Debug, Clone, Copy)]
pub struct RtSchedParam {
    /// Real-time priority (1-99, higher = more important)
    pub sched_priority: i32,
    /// For SCHED_DEADLINE: runtime in nanoseconds
    pub sched_runtime: u64,
    /// For SCHED_DEADLINE: deadline in nanoseconds
    pub sched_deadline: u64,
    /// For SCHED_DEADLINE: period in nanoseconds
    pub sched_period: u64,
}

impl Default for RtSchedParam {
    fn default() -> Self {
        Self {
            sched_priority: 0,
            sched_runtime: 0,
            sched_deadline: 0,
            sched_period: 0,
        }
    }
}

// ============================================================================
// REAL-TIME TASK INFO
// ============================================================================

/// Real-time task tracking info
#[derive(Debug, Clone)]
pub struct RtTaskInfo {
    pub task_id: TaskId,
    pub policy: SchedPolicy,
    pub priority: i32,
    /// Time slice remaining (for SCHED_RR)
    pub time_slice: u64,
    /// Total time slice (for SCHED_RR)
    pub total_timeslice: u64,
    /// CPU affinity mask
    pub affinity: u64,
    /// Is this a real-time task?
    pub is_rt: bool,
}

impl RtTaskInfo {
    pub fn new(task_id: TaskId) -> Self {
        Self {
            task_id,
            policy: SchedPolicy::Normal,
            priority: 0,
            time_slice: RR_DEFAULT_TIMESLICE,
            total_timeslice: RR_DEFAULT_TIMESLICE,
            affinity: 0xFFFFFFFFFFFFFFFF, // All CPUs
            is_rt: false,
        }
    }

    pub fn with_rt(task_id: TaskId, policy: SchedPolicy, priority: i32) -> Self {
        let is_rt = policy == SchedPolicy::Fifo || policy == SchedPolicy::RoundRobin;
        let time_slice = if policy == SchedPolicy::RoundRobin {
            Self::calculate_timeslice(priority)
        } else {
            u64::MAX // FIFO: runs until blocked or yields
        };

        Self {
            task_id,
            policy,
            priority,
            time_slice,
            total_timeslice: time_slice,
            affinity: 0xFFFFFFFFFFFFFFFF,
            is_rt,
        }
    }

    /// Calculate time slice based on priority
    /// Higher priority = longer time slice
    fn calculate_timeslice(priority: i32) -> u64 {
        let normalized = (priority as f64 / RT_PRIO_MAX as f64).clamp(0.0, 1.0);
        let slice = RR_MIN_TIMESLICE as f64 + 
            normalized * (RR_MAX_TIMESLICE - RR_MIN_TIMESLICE) as f64;
        slice as u64
    }

    /// Reset time slice (called when task is scheduled)
    pub fn reset_timeslice(&mut self) {
        self.time_slice = self.total_timeslice;
    }

    /// Decrement time slice
    /// Returns true if time slice expired
    pub fn tick(&mut self) -> bool {
        if self.policy == SchedPolicy::RoundRobin && self.time_slice > 0 {
            self.time_slice = self.time_slice.saturating_sub(1);
            return self.time_slice == 0;
        }
        false
    }
}

// ============================================================================
// REAL-TIME RUN QUEUE
// ============================================================================

/// Real-time run queue (priority-ordered)
/// 
/// RT tasks are stored in priority buckets (1-99).
/// Higher priority tasks always run before lower priority.
/// Within same priority:
/// - SCHED_FIFO: FIFO order
/// - SCHED_RR: Round-robin with time slices
pub struct RtRunQueue {
    /// Priority buckets: priority -> list of tasks
    /// Priority 99 is highest, 1 is lowest (for RT)
    queues: BTreeMap<i32, Vec<Box<Task>>>,
    /// Task ID -> RT info mapping
    task_info: BTreeMap<TaskId, RtTaskInfo>,
    /// Number of RT tasks
    rt_count: AtomicU64,
    /// Highest priority with runnable tasks
    highest_prio: AtomicU64,
    /// RT throttling: bandwidth control
    rt_runtime: AtomicU64,
    rt_period: AtomicU64,
    rt_runtime_enabled: AtomicBool,
}

impl RtRunQueue {
    pub fn new() -> Self {
        Self {
            queues: BTreeMap::new(),
            task_info: BTreeMap::new(),
            rt_count: AtomicU64::new(0),
            highest_prio: AtomicU64::new(0),
            rt_runtime: AtomicU64::new(950_000_000), // 95% of 1s
            rt_period: AtomicU64::new(1_000_000_000), // 1s
            rt_runtime_enabled: AtomicBool::new(true),
        }
    }

    /// Add task to RT run queue
    pub fn enqueue(&mut self, task: Box<Task>) {
        let task_id = task.hot.id;
        let info = self.task_info.entry(task_id).or_insert_with(|| {
            RtTaskInfo::new(task_id)
        });

        let priority = info.priority;
        let is_rt = info.is_rt;

        // Add to appropriate priority queue
        let queue = self.queues.entry(priority).or_insert_with(Vec::new);
        queue.push(task);

        // Update highest priority
        if is_rt && priority as u64 > self.highest_prio.load(Ordering::Relaxed) {
            self.highest_prio.store(priority as u64, Ordering::Relaxed);
        }

        if is_rt {
            self.rt_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Remove task from RT run queue
    pub fn dequeue(&mut self, task_id: TaskId) -> Option<Box<Task>> {
        let info = self.task_info.get(&task_id)?;
        let priority = info.priority;

        if let Some(queue) = self.queues.get_mut(&priority) {
            // Find and remove task
            for i in 0..queue.len() {
                if queue[i].hot.id == task_id {
                    let task = queue.remove(i);
                    if info.is_rt {
                        self.rt_count.fetch_sub(1, Ordering::Relaxed);
                    }
                    // Update highest priority if queue empty
                    if queue.is_empty() {
                        self.queues.remove(&priority);
                        self.update_highest_prio();
                    }
                    return Some(task);
                }
            }
        }
        None
    }

    /// Pick next task to run
    /// Returns highest priority RT task, or None if no RT tasks
    pub fn pick_next(&mut self) -> Option<Box<Task>> {
        // Find highest priority with tasks
        let highest = self.find_highest_prio();
        if highest == 0 {
            return None;
        }

        if let Some(queue) = self.queues.get_mut(&highest) {
            if !queue.is_empty() {
                // For SCHED_RR: rotate queue (round-robin)
                // For SCHED_FIFO: take from front
                let task = queue.remove(0);
                
                // Check if we need to re-queue for RR
                if let Some(info) = self.task_info.get_mut(&task.hot.id) {
                    if info.policy == SchedPolicy::RoundRobin {
                        // Task will be re-enqueued when time slice expires
                        info.reset_timeslice();
                    }
                }
                
                return Some(task);
            }
        }
        None
    }

    /// Find highest priority with runnable tasks
    fn find_highest_prio(&self) -> i32 {
        // BTreeMap iterates in sorted order, get highest key with non-empty queue
        self.queues
            .iter()
            .rev()
            .find(|(_, q)| !q.is_empty())
            .map(|(p, _)| *p)
            .unwrap_or(0)
    }

    /// Update highest priority tracking
    fn update_highest_prio(&mut self) {
        let highest = self.find_highest_prio();
        self.highest_prio.store(highest as u64, Ordering::Relaxed);
    }

    /// Get RT task count
    pub fn rt_task_count(&self) -> u64 {
        self.rt_count.load(Ordering::Relaxed)
    }

    /// Check if there are RT tasks
    pub fn has_rt_tasks(&self) -> bool {
        self.rt_count.load(Ordering::Relaxed) > 0
    }

    /// Get/set scheduling parameters for a task
    pub fn set_sched_param(&mut self, task_id: TaskId, policy: SchedPolicy, param: &RtSchedParam) {
        let info = self.task_info.entry(task_id).or_insert_with(|| {
            RtTaskInfo::new(task_id)
        });

        let old_is_rt = info.is_rt;
        
        info.policy = policy;
        info.priority = param.sched_priority.clamp(0, RT_PRIO_MAX);
        info.is_rt = policy == SchedPolicy::Fifo || policy == SchedPolicy::RoundRobin;
        
        if policy == SchedPolicy::RoundRobin {
            info.total_timeslice = RtTaskInfo::calculate_timeslice(info.priority);
            info.time_slice = info.total_timeslice;
        } else {
            info.total_timeslice = u64::MAX;
            info.time_slice = u64::MAX;
        }

        // Update RT count
        if old_is_rt && !info.is_rt {
            self.rt_count.fetch_sub(1, Ordering::Relaxed);
        } else if !old_is_rt && info.is_rt {
            self.rt_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Get scheduling parameters
    pub fn get_sched_param(&self, task_id: TaskId) -> Option<(SchedPolicy, RtSchedParam)> {
        self.task_info.get(&task_id).map(|info| {
            (info.policy, RtSchedParam {
                sched_priority: info.priority,
                sched_runtime: 0,
                sched_deadline: 0,
                sched_period: 0,
            })
        })
    }

    /// Tick: decrement time slice for running task
    /// Returns true if preemption needed
    pub fn tick(&mut self, task_id: TaskId) -> bool {
        if let Some(info) = self.task_info.get_mut(&task_id) {
            if info.tick() {
                // Time slice expired for RR task
                return true;
            }
        }
        false
    }

    /// Re-enqueue task after time slice expiry (RR only)
    pub fn reenqueue_rr(&mut self, task: Box<Task>) {
        let task_id = task.hot.id;
        if let Some(info) = self.task_info.get_mut(&task_id) {
            if info.policy == SchedPolicy::RoundRobin {
                info.reset_timeslice();
                self.enqueue(task);
            }
        }
    }

    /// Set RT bandwidth (throttling)
    pub fn set_rt_bandwidth(&mut self, runtime: u64, period: u64) {
        self.rt_runtime.store(runtime, Ordering::Relaxed);
        self.rt_period.store(period, Ordering::Relaxed);
    }

    /// Enable/disable RT throttling
    pub fn set_rt_throttling(&mut self, enabled: bool) {
        self.rt_runtime_enabled.store(enabled, Ordering::Relaxed);
    }
}

impl Default for RtRunQueue {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// GLOBAL RT SCHEDULER STATE
// ============================================================================

lazy_static::lazy_static! {
    /// Global RT run queue
    static ref RT_RUNQUEUE: Mutex<RtRunQueue> = Mutex::new(RtRunQueue::new());
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// Initialize RT scheduler
pub fn init() {
    crate::serial_println!("[RT-SCHED] Real-Time Scheduler initialized");
}

/// Check if there are runnable RT tasks
pub fn has_rt_tasks() -> bool {
    RT_RUNQUEUE.lock().has_rt_tasks()
}

/// Get RT task count
pub fn rt_task_count() -> u64 {
    RT_RUNQUEUE.lock().rt_task_count()
}

/// Enqueue RT task
pub fn enqueue_rt_task(task: Box<Task>) {
    RT_RUNQUEUE.lock().enqueue(task);
}

/// Dequeue RT task
pub fn dequeue_rt_task(task_id: TaskId) -> Option<Box<Task>> {
    RT_RUNQUEUE.lock().dequeue(task_id)
}

/// Pick next RT task to run
/// Returns None if no RT tasks runnable
pub fn pick_next_rt_task() -> Option<Box<Task>> {
    RT_RUNQUEUE.lock().pick_next()
}

/// Set scheduling parameters for task
pub fn set_sched_param(task_id: TaskId, policy: SchedPolicy, param: &RtSchedParam) {
    RT_RUNQUEUE.lock().set_sched_param(task_id, policy, param);
}

/// Get scheduling parameters for task
pub fn get_sched_param(task_id: TaskId) -> Option<(SchedPolicy, RtSchedParam)> {
    RT_RUNQUEUE.lock().get_sched_param(task_id)
}

/// Tick: handle time slice for running RT task
/// Returns true if preemption needed
pub fn rt_tick(task_id: TaskId) -> bool {
    RT_RUNQUEUE.lock().tick(task_id)
}

/// Re-enqueue RR task after time slice expiry
pub fn reenqueue_rr_task(task: Box<Task>) {
    RT_RUNQUEUE.lock().reenqueue_rr(task);
}

/// Set RT bandwidth limits
pub fn set_rt_bandwidth(runtime: u64, period: u64) {
    RT_RUNQUEUE.lock().set_rt_bandwidth(runtime, period);
}

/// Enable/disable RT throttling
pub fn set_rt_throttling(enabled: bool) {
    RT_RUNQUEUE.lock().set_rt_throttling(enabled);
}

/// Check if task is real-time
pub fn is_rt_task(task_id: TaskId) -> bool {
    RT_RUNQUEUE.lock()
        .task_info
        .get(&task_id)
        .map(|info| info.is_rt)
        .unwrap_or(false)
}

/// Get task priority (RT or normal)
pub fn get_task_priority(task_id: TaskId) -> i32 {
    RT_RUNQUEUE.lock()
        .task_info
        .get(&task_id)
        .map(|info| info.priority)
        .unwrap_or(0)
}

/// Get task scheduling policy
pub fn get_task_policy(task_id: TaskId) -> SchedPolicy {
    RT_RUNQUEUE.lock()
        .task_info
        .get(&task_id)
        .map(|info| info.policy)
        .unwrap_or(SchedPolicy::Normal)
}

/// Yield current RT task
/// For SCHED_FIFO: move to end of priority queue
/// For SCHED_RR: same as yielding time slice
pub fn yield_rt_task(task: Box<Task>) {
    let task_id = task.hot.id;
    let mut rq = RT_RUNQUEUE.lock();
    
    if let Some(info) = rq.task_info.get(&task_id) {
        if info.is_rt {
            // Re-enqueue at end of priority queue
            rq.enqueue(task);
        }
    }
}

// ============================================================================
// SYSCALL INTERFACE HELPERS
// ============================================================================

/// sched_setscheduler syscall implementation
pub fn sys_sched_setscheduler(task_id: TaskId, policy: i32, param: &RtSchedParam) -> i32 {
    let policy = match policy as u8 {
        1 => SchedPolicy::Fifo,
        2 => SchedPolicy::RoundRobin,
        3 => SchedPolicy::Deadline,
        0 | _ => SchedPolicy::Normal,
    };

    // Validate priority
    if policy == SchedPolicy::Fifo || policy == SchedPolicy::RoundRobin {
        if param.sched_priority < RT_PRIO_MIN || param.sched_priority > RT_PRIO_MAX {
            return -22; // EINVAL
        }
    } else if param.sched_priority != 0 {
        return -22; // EINVAL
    }

    set_sched_param(task_id, policy, param);
    0 // Success
}

/// sched_getscheduler syscall implementation
pub fn sys_sched_getscheduler(task_id: TaskId) -> i32 {
    get_task_policy(task_id) as i32
}

/// sched_setparam syscall implementation
pub fn sys_sched_setparam(task_id: TaskId, param: &RtSchedParam) -> i32 {
    let policy = get_task_policy(task_id);
    set_sched_param(task_id, policy, param);
    0
}

/// sched_getparam syscall implementation  
pub fn sys_sched_getparam(task_id: TaskId) -> Option<RtSchedParam> {
    get_sched_param(task_id).map(|(_, p)| p)
}

/// sched_yield syscall implementation
pub fn sys_sched_yield() {
    // This should be called from the current task context
    // The actual yield will be handled by the main scheduler
}

/// sched_get_priority_max syscall implementation
pub fn sys_sched_get_priority_max(policy: i32) -> i32 {
    match policy as u8 {
        1 | 2 => RT_PRIO_MAX, // SCHED_FIFO, SCHED_RR
        _ => 0,
    }
}

/// sched_get_priority_min syscall implementation
pub fn sys_sched_get_priority_min(policy: i32) -> i32 {
    match policy as u8 {
        1 | 2 => RT_PRIO_MIN, // SCHED_FIFO, SCHED_RR
        _ => 0,
    }
}

/// sched_rr_get_interval syscall implementation
pub fn sys_sched_rr_get_interval(task_id: TaskId) -> u64 {
    RT_RUNQUEUE.lock()
        .task_info
        .get(&task_id)
        .map(|info| info.total_timeslice)
        .unwrap_or(RR_DEFAULT_TIMESLICE)
}
