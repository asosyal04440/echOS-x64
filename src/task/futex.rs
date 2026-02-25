//! # Futex (Fast Userspace Mutex) Implementation
//!
//! Linux-compatible futex system call support.
//! Provides efficient synchronization primitives for userspace.

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

use super::task::TaskId;
use super::scheduler::{current_task_id, sleep};

// ============================================================================
// FUTEX CONSTANTS
// ============================================================================

/// Futex wait operation
pub const FUTEX_WAIT: i32 = 0;
/// Futex wake operation
pub const FUTEX_WAKE: i32 = 1;
/// Futex wait with timeout
pub const FUTEX_WAIT_BITSET: i32 = 9;
/// Futex wake with bitmask
pub const FUTEX_WAKE_BITSET: i32 = 10;
/// Futex requeue operation
pub const FUTEX_REQUEUE: i32 = 3;
/// Futex compare-and-swap requeue
pub const FUTEX_CMP_REQUEUE: i32 = 4;
/// Futex lock pi (priority inheritance)
pub const FUTEX_LOCK_PI: i32 = 6;
/// Futex unlock pi
pub const FUTEX_UNLOCK_PI: i32 = 7;
/// Futex trylock pi
pub const FUTEX_TRYLOCK_PI: i32 = 8;

/// Futex private flag (process-private futex)
pub const FUTEX_PRIVATE_FLAG: i32 = 128;
/// Futex clock realtime flag
pub const FUTEX_CLOCK_REALTIME: i32 = 256;

// ============================================================================
// FUTEX WAIT QUEUE
// ============================================================================

/// A waiter in the futex queue
#[derive(Clone, Debug)]
struct FutexWaiter {
    task_id: TaskId,
    /// Bitset for FUTEX_WAIT_BITSET
    bitset: u32,
    /// Timeout in ticks (0 = no timeout)
    timeout: u64,
    /// When the waiter was added
    start_tick: u64,
}

/// Futex wait queue (one per futex address)
#[derive(Debug)]
struct FutexQueue {
    /// Waiters waiting on this futex
    waiters: Vec<FutexWaiter>,
    /// Spinlock for fast path
    locked: AtomicBool,
}

impl FutexQueue {
    fn new() -> Self {
        Self {
            waiters: Vec::new(),
            locked: AtomicBool::new(false),
        }
    }

    /// Add a waiter
    fn add_waiter(&mut self, task_id: TaskId, bitset: u32, timeout: u64, start_tick: u64) {
        self.waiters.push(FutexWaiter {
            task_id,
            bitset,
            timeout,
            start_tick,
        });
    }

    /// Remove a waiter by task ID
    fn remove_waiter(&mut self, task_id: TaskId) {
        self.waiters.retain(|w| w.task_id != task_id);
    }

    /// Wake up to `count` waiters
    /// Returns number of waiters woken
    fn wake_waiters(&mut self, count: usize, bitset: Option<u32>) -> Vec<TaskId> {
        let mut woken = Vec::new();
        let mut i = 0;
        
        while i < self.waiters.len() && woken.len() < count {
            let waiter = &self.waiters[i];
            let matches = match bitset {
                Some(mask) => (waiter.bitset & mask) != 0,
                None => true,
            };
            
            if matches {
                let waiter = self.waiters.remove(i);
                woken.push(waiter.task_id);
            } else {
                i += 1;
            }
        }
        
        woken
    }

    /// Check for timed-out waiters
    fn check_timeouts(&mut self, current_tick: u64) -> Vec<TaskId> {
        let mut timed_out = Vec::new();
        
        self.waiters.retain(|w| {
            if w.timeout > 0 {
                let elapsed = current_tick.saturating_sub(w.start_tick);
                if elapsed >= w.timeout {
                    timed_out.push(w.task_id);
                    return false;
                }
            }
            true
        });
        
        timed_out
    }

    /// Get waiter count
    fn waiter_count(&self) -> usize {
        self.waiters.len()
    }
}

// ============================================================================
// FUTEX HASH TABLE
// ============================================================================

/// Global futex manager
pub struct FutexManager {
    /// Futex queues indexed by address
    queues: Mutex<BTreeMap<u64, Arc<Mutex<FutexQueue>>>>,
    /// Total wait count
    total_waits: AtomicU64,
    /// Total wake count
    total_wakes: AtomicU64,
    /// Total timeouts
    total_timeouts: AtomicU64,
}

impl FutexManager {
    pub const fn new() -> Self {
        Self {
            queues: Mutex::new(BTreeMap::new()),
            total_waits: AtomicU64::new(0),
            total_wakes: AtomicU64::new(0),
            total_timeouts: AtomicU64::new(0),
        }
    }

    /// Get or create a futex queue for the given address
    fn get_queue(&self, addr: u64) -> Arc<Mutex<FutexQueue>> {
        let mut queues = self.queues.lock();
        
        if let Some(queue) = queues.get(&addr) {
            queue.clone()
        } else {
            let queue = Arc::new(Mutex::new(FutexQueue::new()));
            queues.insert(addr, queue.clone());
            queue
        }
    }

    /// Clean up empty queues
    fn cleanup_empty_queues(&self) {
        let mut queues = self.queues.lock();
        queues.retain(|_, q| q.lock().waiter_count() > 0);
    }
}

lazy_static::lazy_static! {
    /// Global futex manager
    static ref FUTEX_MANAGER: FutexManager = FutexManager::new();
}

// ============================================================================
// FUTEX SYSCALL IMPLEMENTATION
// ============================================================================

/// futex syscall implementation
/// 
/// # Arguments
/// - `uaddr`: Userspace address of the futex
/// - `futex_op`: Operation (FUTEX_WAIT, FUTEX_WAKE, etc.)
/// - `val`: Operation-specific value
/// - `timeout`: Timeout for wait operations (in nanoseconds, or pointer for requeue)
/// - `uaddr2`: Second address for requeue operations
/// - `val3`: Third value (bitset for BITSET operations)
/// 
/// # Returns
/// Number of waiters woken/requeued, or negative errno on error
pub fn sys_futex(
    uaddr: u64,
    futex_op: i32,
    val: u32,
    timeout: u64,
    uaddr2: u64,
    val3: u32,
) -> i64 {
    // Extract operation and flags
    let op = futex_op & 0x7F;
    let is_private = (futex_op & FUTEX_PRIVATE_FLAG) != 0;
    
    match op {
        FUTEX_WAIT | FUTEX_WAIT_BITSET => {
            sys_futex_wait(uaddr, val, timeout, val3, op == FUTEX_WAIT_BITSET)
        }
        FUTEX_WAKE | FUTEX_WAKE_BITSET => {
            sys_futex_wake(uaddr, val, if op == FUTEX_WAKE_BITSET { Some(val3) } else { None })
        }
        FUTEX_REQUEUE => {
            sys_futex_requeue(uaddr, val, uaddr2, 0)
        }
        FUTEX_CMP_REQUEUE => {
            sys_futex_requeue(uaddr, val, uaddr2, val3)
        }
        FUTEX_LOCK_PI => {
            sys_futex_lock_pi(uaddr, timeout)
        }
        FUTEX_UNLOCK_PI => {
            sys_futex_unlock_pi(uaddr)
        }
        _ => {
            crate::serial_println!("[FUTEX] Unknown operation: {}", op);
            -22 // EINVAL
        }
    }
}

/// FUTEX_WAIT implementation
fn sys_futex_wait(uaddr: u64, expected: u32, timeout_ns: u64, bitset: u32, has_bitset: bool) -> i64 {
    let task_id = current_task_id();
    let queue = FUTEX_MANAGER.get_queue(uaddr);
    
    // Convert timeout from nanoseconds to ticks (assuming 1000 Hz)
    let timeout_ticks = if timeout_ns > 0 {
        timeout_ns / 1_000_000 // ns to ms, then assume 1 tick = 1ms
    } else {
        0
    };
    
    let bitset = if has_bitset && bitset == 0 {
        0xFFFFFFFF // Default bitset
    } else if has_bitset {
        bitset
    } else {
        0xFFFFFFFF
    };
    
    // Add waiter to queue
    {
        let mut q = queue.lock();
        q.add_waiter(task_id, bitset, timeout_ticks, super::scheduler::get_ticks() as u64);
    }
    
    FUTEX_MANAGER.total_waits.fetch_add(1, Ordering::Relaxed);
    
    crate::serial_println!(
        "[FUTEX] WAIT: task {} waiting at {:#x} (expected={}, timeout={}ms)",
        task_id, uaddr, expected, timeout_ticks
    );
    
    // Block the current task
    // In a real implementation, we would:
    // 1. Check if *uaddr == expected (userspace access)
    // 2. If not equal, return -EAGAIN
    // 3. Otherwise, block the task
    
    // For now, simulate blocking
    if timeout_ticks > 0 {
        sleep(timeout_ticks as usize);
        
        // Check if we timed out
        let q = queue.lock();
        let still_waiting = q.waiters.iter().any(|w| w.task_id == task_id);
        
        if still_waiting {
            // Timed out
            drop(q);
            queue.lock().remove_waiter(task_id);
            FUTEX_MANAGER.total_timeouts.fetch_add(1, Ordering::Relaxed);
            return -110; // ETIMEDOUT
        }
    } else {
        // Infinite wait - would need scheduler integration
        sleep(100); // Placeholder
    }
    
    0 // Success (woken by FUTEX_WAKE)
}

/// FUTEX_WAKE implementation
fn sys_futex_wake(uaddr: u64, count: u32, bitset: Option<u32>) -> i64 {
    let queue = FUTEX_MANAGER.get_queue(uaddr);
    
    let woken = {
        let mut q = queue.lock();
        q.wake_waiters(count as usize, bitset)
    };
    
    let woken_count = woken.len() as i64;
    
    // Wake up the tasks
    for task_id in woken {
        // wake_task(task_id); // Would need scheduler integration
        crate::serial_println!("[FUTEX] WAKE: woke task {} at {:#x}", task_id, uaddr);
    }
    
    FUTEX_MANAGER.total_wakes.fetch_add(woken_count as u64, Ordering::Relaxed);
    
    // Cleanup empty queue
    if queue.lock().waiter_count() == 0 {
        FUTEX_MANAGER.queues.lock().remove(&uaddr);
    }
    
    woken_count
}

/// FUTEX_REQUEUE implementation
fn sys_futex_requeue(uaddr: u64, wake_count: u32, uaddr2: u64, requeue_cmp: u32) -> i64 {
    let queue1 = FUTEX_MANAGER.get_queue(uaddr);
    let queue2 = FUTEX_MANAGER.get_queue(uaddr2);
    
    let mut woken = 0u64;
    let mut requeued = 0u64;
    
    // Wake up to wake_count waiters
    {
        let mut q1 = queue1.lock();
        let to_wake = q1.wake_waiters(wake_count as usize, None);
        woken = to_wake.len() as u64;
        
        for task_id in to_wake {
            crate::serial_println!("[FUTEX] REQUEUE: woke task {} from {:#x}", task_id, uaddr);
        }
    }
    
    // Requeue remaining waiters to uaddr2
    {
        let mut q1 = queue1.lock();
        let mut q2 = queue2.lock();
        
        // For CMP_REQUEUE, check if *uaddr == requeue_cmp
        // For now, just requeue all remaining
        
        while let Some(waiter) = q1.waiters.pop() {
            q2.add_waiter(waiter.task_id, waiter.bitset, waiter.timeout, waiter.start_tick);
            requeued += 1;
        }
    }
    
    crate::serial_println!(
        "[FUTEX] REQUEUE: {:#x} -> {:#x}: woke={}, requeued={}",
        uaddr, uaddr2, woken, requeued
    );
    
    // Cleanup
    if queue1.lock().waiter_count() == 0 {
        FUTEX_MANAGER.queues.lock().remove(&uaddr);
    }
    
    (woken + requeued) as i64
}

/// FUTEX_LOCK_PI implementation (priority inheritance)
fn sys_futex_lock_pi(uaddr: u64, timeout_ns: u64) -> i64 {
    // TODO: Implement priority inheritance futex
    // This requires integration with the RT scheduler
    crate::serial_println!("[FUTEX] LOCK_PI: not yet implemented at {:#x}", uaddr);
    -38 // ENOSYS
}

/// FUTEX_UNLOCK_PI implementation
fn sys_futex_unlock_pi(uaddr: u64) -> i64 {
    // TODO: Implement priority inheritance futex
    crate::serial_println!("[FUTEX] UNLOCK_PI: not yet implemented at {:#x}", uaddr);
    -38 // ENOSYS
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// Initialize futex subsystem
pub fn init() {
    crate::serial_println!("[FUTEX] Subsystem initialized");
}

/// Get futex statistics
pub struct FutexStats {
    pub queue_count: usize,
    pub total_waits: u64,
    pub total_wakes: u64,
    pub total_timeouts: u64,
}

/// Get futex statistics
pub fn get_stats() -> FutexStats {
    FutexStats {
        queue_count: FUTEX_MANAGER.queues.lock().len(),
        total_waits: FUTEX_MANAGER.total_waits.load(Ordering::Relaxed),
        total_wakes: FUTEX_MANAGER.total_wakes.load(Ordering::Relaxed),
        total_timeouts: FUTEX_MANAGER.total_timeouts.load(Ordering::Relaxed),
    }
}

/// Wake all waiters at a given address (used by robust futex handling)
pub fn wake_all_at_address(uaddr: u64) -> usize {
    let queue = FUTEX_MANAGER.get_queue(uaddr);
    let woken = queue.lock().wake_waiters(usize::MAX, None);
    
    // Would need scheduler integration to actually wake tasks
    woken.len()
}

/// Check for timed-out waiters (called periodically)
pub fn check_timeouts() {
    let current_tick = super::scheduler::get_ticks() as u64;
    
    let queues = FUTEX_MANAGER.queues.lock();
    for (_, queue) in queues.iter() {
        let mut q = queue.lock();
        let timed_out = q.check_timeouts(current_tick);
        
        for task_id in timed_out {
            // wake_task(task_id); // Would need scheduler integration
            FUTEX_MANAGER.total_timeouts.fetch_add(1, Ordering::Relaxed);
            crate::serial_println!("[FUTEX] Timeout for task {}", task_id);
        }
    }
}

// ============================================================================
// CLONE SYSCALL SUPPORT
// ============================================================================

/// Clone flags (Linux-compatible)
pub const CLONE_VM: u64 = 0x00000100;      // Share memory space
pub const CLONE_FS: u64 = 0x00000200;       // Share filesystem info
pub const CLONE_FILES: u64 = 0x00000400;    // Share file descriptors
pub const CLONE_SIGHAND: u64 = 0x00000800;  // Share signal handlers
pub const CLONE_PTRACE: u64 = 0x00002000;   // Trace via ptrace
pub const CLONE_VFORK: u64 = 0x00004000;    // Parent waits for child
pub const CLONE_PARENT: u64 = 0x00008000;   // Same parent as caller
pub const CLONE_THREAD: u64 = 0x00010000;   // Same thread group
pub const CLONE_NEWNS: u64 = 0x00020000;    // New mount namespace
pub const CLONE_SYSVSEM: u64 = 0x00040000;  // Share SysV semaphores
pub const CLONE_SETTLS: u64 = 0x00080000;   // Set TLS
pub const CLONE_PARENT_SETTID: u64 = 0x00100000;  // Set TID in parent
pub const CLONE_CHILD_CLEARTID: u64 = 0x00200000; // Clear TID in child
pub const CLONE_DETACHED: u64 = 0x00400000; // Detached thread
pub const CLONE_UNTRACED: u64 = 0x00800000; // Not traced
pub const CLONE_CHILD_SETTID: u64 = 0x01000000; // Set TID in child
pub const CLONE_NEWUTS: u64 = 0x04000000;   // New UTS namespace
pub const CLONE_NEWIPC: u64 = 0x08000000;   // New IPC namespace
pub const CLONE_NEWUSER: u64 = 0x10000000;  // New user namespace
pub const CLONE_NEWPID: u64 = 0x20000000;   // New PID namespace
pub const CLONE_NEWNET: u64 = 0x40000000;   // New network namespace
pub const CLONE_IO: u64 = 0x80000000;       // Share I/O context

/// Clone syscall implementation
/// 
/// Creates a new thread or process
/// 
/// # Arguments
/// - `flags`: Clone flags (CLONE_*)
/// - `child_stack`: Stack for child (0 = copy parent stack)
/// - `ptid`: Pointer to store parent TID
/// - `ctid`: Pointer to store child TID  
/// - `tls`: TLS pointer for child
/// 
/// # Returns
/// Child PID in parent, 0 in child, negative errno on error
pub fn sys_clone(
    flags: u64,
    child_stack: u64,
    ptid: u64,
    ctid: u64,
    tls: u64,
) -> i64 {
    let current_pid = current_task_id() as i64;
    
    // Validate flags
    if flags & CLONE_THREAD != 0 && flags & CLONE_SIGHAND == 0 {
        return -22; // EINVAL: CLONE_THREAD requires CLONE_SIGHAND
    }
    
    if flags & CLONE_SIGHAND != 0 && flags & CLONE_VM == 0 {
        return -22; // EINVAL: CLONE_SIGHAND requires CLONE_VM
    }
    
    // Determine if this is a thread or process
    let is_thread = (flags & (CLONE_VM | CLONE_FILES | CLONE_FS | CLONE_SIGHAND)) != 0;
    
    crate::serial_println!(
        "[CLONE] Creating {} (flags={:#x}, stack={:#x})",
        if is_thread { "thread" } else { "process" },
        flags, child_stack
    );
    
    // TODO: Actually create the new task
    // This would involve:
    // 1. Allocating new task ID
    // 2. Copying or sharing resources based on flags
    // 3. Setting up child stack
    // 4. Setting TLS if CLONE_SETTLS
    // 5. Setting TIDs if requested
    // 6. Scheduling the new task
    
    // For now, return a placeholder child PID
    let child_pid = current_pid + 1;
    
    child_pid
}

/// set_robust_list syscall implementation
/// 
/// Sets the list of robust futexes for the process
pub fn sys_set_robust_list(head: u64, len: usize) -> i64 {
    if len % 24 != 0 { // sizeof(struct robust_list_head)
        return -22; // EINVAL
    }
    
    // TODO: Store robust list head for current process
    // This is used for cleaning up futexes when a process exits
    
    0
}

/// get_robust_list syscall implementation
pub fn sys_get_robust_list(pid: i32, head_ptr: u64, len_ptr: u64) -> i64 {
    // TODO: Retrieve robust list for process
    0
}

/// set_tid_address syscall implementation
/// 
/// Sets the address for clear_child_tid
pub fn sys_set_tid_address(tidptr: u64) -> i64 {
    // TODO: Store tid_address for current process
    // This is cleared and woken when the process exits
    
    current_task_id() as i64
}
