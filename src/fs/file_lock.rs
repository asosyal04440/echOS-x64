//! # File Locking Implementation
//!
//! POSIX file locking support (flock, fcntl locks).
//! Provides advisory and mandatory file locking.

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// FILE LOCK CONSTANTS
// ============================================================================

/// Lock types for flock()
pub const LOCK_SH: i32 = 1;      // Shared lock
pub const LOCK_EX: i32 = 2;      // Exclusive lock
pub const LOCK_UN: i32 = 8;      // Unlock
pub const LOCK_NB: i32 = 4;      // Non-blocking

/// Lock types for fcntl (F_SETLK, F_SETLKW, F_GETLK)
pub const F_RDLCK: i32 = 0;      // Read lock
pub const F_WRLCK: i32 = 1;      // Write lock
pub const F_UNLCK: i32 = 2;      // Unlock

/// fcntl lock commands
pub const F_SETLK: i32 = 6;      // Set lock (non-blocking)
pub const F_SETLKW: i32 = 7;     // Set lock (blocking)
pub const F_GETLK: i32 = 5;      // Get lock info

// ============================================================================
// FILE LOCK STRUCTURES
// ============================================================================

/// A file lock (POSIX fcntl style)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileLock {
    /// Lock type (F_RDLCK, F_WRLCK, F_UNLCK)
    pub l_type: i32,
    /// Lock origin (SEEK_SET, SEEK_CUR, SEEK_END)
    pub l_whence: i32,
    /// Start offset
    pub l_start: u64,
    /// Length (0 = to EOF)
    pub l_len: u64,
    /// Process ID holding the lock
    pub l_pid: u64,
}

impl FileLock {
    pub fn new(l_type: i32, l_whence: i32, l_start: u64, l_len: u64, l_pid: u64) -> Self {
        Self {
            l_type,
            l_whence,
            l_start,
            l_len,
            l_pid,
        }
    }

    /// Check if this lock conflicts with another
    pub fn conflicts_with(&self, other: &FileLock) -> bool {
        // Different processes
        if self.l_pid == other.l_pid {
            return false;
        }

        // Unlock never conflicts
        if self.l_type == F_UNLCK || other.l_type == F_UNLCK {
            return false;
        }

        // Two read locks don't conflict
        if self.l_type == F_RDLCK && other.l_type == F_RDLCK {
            return false;
        }

        // Check range overlap
        self.overlaps(other)
    }

    /// Check if two lock ranges overlap
    pub fn overlaps(&self, other: &FileLock) -> bool {
        let self_end = if self.l_len == 0 {
            u64::MAX
        } else {
            self.l_start.saturating_add(self.l_len)
        };

        let other_end = if other.l_len == 0 {
            u64::MAX
        } else {
            other.l_start.saturating_add(other.l_len)
        };

        self.l_start < other_end && other.l_start < self_end
    }

    /// Check if this lock contains the given offset
    pub fn contains_offset(&self, offset: u64) -> bool {
        let end = if self.l_len == 0 {
            u64::MAX
        } else {
            self.l_start.saturating_add(self.l_len)
        };
        offset >= self.l_start && offset < end
    }
}

/// flock-style lock (whole file)
#[derive(Clone, Debug)]
pub struct FlockLock {
    /// File descriptor
    pub fd: u64,
    /// Lock type (LOCK_SH, LOCK_EX)
    pub lock_type: i32,
    /// Process ID
    pub pid: u64,
}

// ============================================================================
// LOCK MANAGER
// ============================================================================

/// Global file lock manager
pub struct FileLockManager {
    /// POSIX locks by file inode
    posix_locks: Mutex<BTreeMap<u64, Vec<FileLock>>>,
    /// flock locks by file descriptor
    flock_locks: Mutex<BTreeMap<u64, Vec<FlockLock>>>,
    /// Total locks
    total_locks: AtomicU64,
    /// Total conflicts
    total_conflicts: AtomicU64,
}

impl FileLockManager {
    pub const fn new() -> Self {
        Self {
            posix_locks: Mutex::new(BTreeMap::new()),
            flock_locks: Mutex::new(BTreeMap::new()),
            total_locks: AtomicU64::new(0),
            total_conflicts: AtomicU64::new(0),
        }
    }

    /// Check if a POSIX lock can be acquired
    pub fn can_acquire_posix_lock(&self, inode: u64, lock: &FileLock) -> bool {
        let locks = self.posix_locks.lock();
        
        if let Some(existing_locks) = locks.get(&inode) {
            for existing in existing_locks {
                if lock.conflicts_with(existing) {
                    return false;
                }
            }
        }
        
        true
    }

    /// Acquire a POSIX lock
    pub fn acquire_posix_lock(&self, inode: u64, lock: FileLock) -> Result<(), FileLockError> {
        let mut locks = self.posix_locks.lock();
        
        // Check for conflicts
        if let Some(existing_locks) = locks.get(&inode) {
            for existing in existing_locks {
                if lock.conflicts_with(existing) {
                    self.total_conflicts.fetch_add(1, Ordering::Relaxed);
                    return Err(FileLockError::Conflict);
                }
            }
        }
        
        // Remove any existing lock from this process in this range
        if lock.l_type == F_UNLCK {
            // Unlock: remove overlapping locks
            if let Some(existing_locks) = locks.get_mut(&inode) {
                existing_locks.retain(|l| {
                    l.l_pid != lock.l_pid || !l.overlaps(&lock)
                });
            }
        } else {
            // Add new lock
            let entry = locks.entry(inode).or_insert_with(Vec::new);
            entry.push(lock);
            self.total_locks.fetch_add(1, Ordering::Relaxed);
        }
        
        Ok(())
    }

    /// Get conflicting lock (for F_GETLK)
    pub fn get_conflicting_lock(&self, inode: u64, lock: &FileLock) -> Option<FileLock> {
        let locks = self.posix_locks.lock();
        
        if let Some(existing_locks) = locks.get(&inode) {
            for existing in existing_locks {
                if lock.conflicts_with(existing) {
                    return Some(existing.clone());
                }
            }
        }
        
        None
    }

    /// Release all locks for a process
    pub fn release_all_locks(&self, pid: u64) {
        // Release POSIX locks
        {
            let mut locks = self.posix_locks.lock();
            for (_, lock_list) in locks.iter_mut() {
                lock_list.retain(|l| l.l_pid != pid);
            }
        }
        
        // Release flock locks
        {
            let mut locks = self.flock_locks.lock();
            for (_, lock_list) in locks.iter_mut() {
                lock_list.retain(|l| l.pid != pid);
            }
        }
        
        crate::serial_println!("[FILELOCK] Released all locks for PID {}", pid);
    }

    /// Acquire flock lock
    pub fn acquire_flock(&self, fd: u64, lock_type: i32, pid: u64) -> Result<(), FileLockError> {
        let mut locks = self.flock_locks.lock();
        
        // Check for conflicts
        if let Some(existing_locks) = locks.get(&fd) {
            for existing in existing_locks {
                if existing.pid == pid {
                    continue; // Same process can re-lock
                }
                
                // Exclusive lock conflicts with anything
                if lock_type == LOCK_EX || existing.lock_type == LOCK_EX {
                    self.total_conflicts.fetch_add(1, Ordering::Relaxed);
                    return Err(FileLockError::Conflict);
                }
                
                // Two shared locks are OK
            }
        }
        
        // Remove existing lock from this process
        if let Some(existing_locks) = locks.get_mut(&fd) {
            existing_locks.retain(|l| l.pid != pid);
        }
        
        // Add new lock (unless unlocking)
        if lock_type != LOCK_UN {
            let entry = locks.entry(fd).or_insert_with(Vec::new);
            entry.push(FlockLock { fd, lock_type, pid });
            self.total_locks.fetch_add(1, Ordering::Relaxed);
        }
        
        Ok(())
    }

    /// Check if file is locked (for mandatory locking)
    pub fn is_locked(&self, inode: u64, offset: u64, write: bool) -> bool {
        let locks = self.posix_locks.lock();
        
        if let Some(existing_locks) = locks.get(&inode) {
            for lock in existing_locks {
                if lock.contains_offset(offset) {
                    // Write lock blocks both reads and writes
                    if lock.l_type == F_WRLCK {
                        return true;
                    }
                    // Read lock only blocks writes
                    if lock.l_type == F_RDLCK && write {
                        return true;
                    }
                }
            }
        }
        
        false
    }
}

lazy_static::lazy_static! {
    /// Global file lock manager
    static ref FILE_LOCK_MANAGER: FileLockManager = FileLockManager::new();
}

// ============================================================================
// ERROR TYPE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileLockError {
    Conflict,
    InvalidLock,
    Deadlock,
    PermissionDenied,
}

// ============================================================================
// SYSCALL INTERFACE
// ============================================================================

/// flock syscall implementation
/// 
/// # Arguments
/// - `fd`: File descriptor
/// - `operation`: Lock operation (LOCK_SH | LOCK_EX | LOCK_UN | LOCK_NB)
/// 
/// # Returns
/// 0 on success, negative errno on failure
pub fn sys_flock(fd: i32, operation: i32) -> i32 {
    if fd < 0 {
        return -9; // EBADF
    }
    
    let lock_type = operation & !LOCK_NB;
    let nonblock = (operation & LOCK_NB) != 0;
    
    if lock_type != LOCK_SH && lock_type != LOCK_EX && lock_type != LOCK_UN {
        return -22; // EINVAL
    }
    
    let pid = crate::task::scheduler::current_task_id() as u64;
    
    loop {
        match FILE_LOCK_MANAGER.acquire_flock(fd as u64, lock_type, pid) {
            Ok(()) => {
                crate::serial_println!(
                    "[FLOCK] fd={} type={} pid={}",
                    fd, lock_type, pid
                );
                return 0;
            }
            Err(FileLockError::Conflict) => {
                if nonblock {
                    return -11; // EAGAIN
                }
                // Block and retry
                // In real implementation, would sleep and retry
                crate::task::sleep(10);
            }
            Err(_) => {
                return -22; // EINVAL
            }
        }
    }
}

/// fcntl lock implementation (F_SETLK/F_SETLKW/F_GETLK)
/// 
/// # Arguments
/// - `fd`: File descriptor
/// - `cmd`: Command (F_SETLK, F_SETLKW, F_GETLK)
/// - `lock`: Lock structure
/// 
/// # Returns
/// 0 on success, negative errno on failure
pub fn sys_fcntl_lock(fd: i32, cmd: i32, lock: &mut FileLock) -> i32 {
    if fd < 0 {
        return -9; // EBADF
    }
    
    // Validate lock type
    if lock.l_type != F_RDLCK && lock.l_type != F_WRLCK && lock.l_type != F_UNLCK {
        return -22; // EINVAL
    }
    
    // Get inode from fd (placeholder)
    let inode = fd as u64; // In real impl, would look up inode from fd
    
    lock.l_pid = crate::task::scheduler::current_task_id() as u64;
    
    match cmd {
        F_GETLK => {
            // Find conflicting lock
            if let Some(conflict) = FILE_LOCK_MANAGER.get_conflicting_lock(inode, lock) {
                *lock = conflict;
            } else {
                lock.l_type = F_UNLCK;
            }
            0
        }
        F_SETLK => {
            // Non-blocking set
            match FILE_LOCK_MANAGER.acquire_posix_lock(inode, lock.clone()) {
                Ok(()) => {
                    crate::serial_println!(
                        "[FCNTL] SETLK: fd={} type={} start={} len={}",
                        fd, lock.l_type, lock.l_start, lock.l_len
                    );
                    0
                }
                Err(FileLockError::Conflict) => -11, // EAGAIN
                Err(_) => -22, // EINVAL
            }
        }
        F_SETLKW => {
            // Blocking set
            loop {
                match FILE_LOCK_MANAGER.acquire_posix_lock(inode, lock.clone()) {
                    Ok(()) => {
                        crate::serial_println!(
                            "[FCNTL] SETLKW: fd={} type={} start={} len={}",
                            fd, lock.l_type, lock.l_start, lock.l_len
                        );
                        return 0;
                    }
                    Err(FileLockError::Conflict) => {
                        // TODO: Check for deadlock
                        crate::task::sleep(10);
                    }
                    Err(_) => {
                        return -22; // EINVAL
                    }
                }
            }
        }
        _ => -22, // EINVAL
    }
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// Initialize file locking subsystem
pub fn init() {
    crate::serial_println!("[FILELOCK] Subsystem initialized");
}

/// Release all locks held by a process (called on exit)
pub fn release_process_locks(pid: u64) {
    FILE_LOCK_MANAGER.release_all_locks(pid);
}

/// Check if a file region is locked (for mandatory locking)
pub fn check_lock(inode: u64, offset: u64, write: bool) -> bool {
    FILE_LOCK_MANAGER.is_locked(inode, offset, write)
}

/// Get lock statistics
pub struct LockStats {
    pub total_locks: u64,
    pub total_conflicts: u64,
    pub posix_lock_count: usize,
    pub flock_lock_count: usize,
}

/// Get lock statistics
pub fn get_stats() -> LockStats {
    LockStats {
        total_locks: FILE_LOCK_MANAGER.total_locks.load(Ordering::Relaxed),
        total_conflicts: FILE_LOCK_MANAGER.total_conflicts.load(Ordering::Relaxed),
        posix_lock_count: FILE_LOCK_MANAGER.posix_locks.lock().len(),
        flock_lock_count: FILE_LOCK_MANAGER.flock_locks.lock().len(),
    }
}

/// Check if mandatory locking is enabled for a file
/// (file has setgid bit set but group execute disabled)
pub fn is_mandatory_locking_enabled(mode: u32) -> bool {
    // Mandatory locking: setgid bit set, group execute disabled
    (mode & 0o2000) != 0 && (mode & 0o040) == 0
}
