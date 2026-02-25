//! # POSIX Semaphores
//!
//! Named and unnamed semaphores.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// SEMAPHORE CONSTANTS
// ============================================================================

/// Maximum semaphore value
pub const SEMVMX: i32 = 32767;
/// Maximum number of semaphores per ID
pub const SEMMSL: usize = 250;
/// Maximum number of semaphore IDs
pub const SEMMNI: usize = 128;
/// Maximum semaphores system-wide
pub const SEMMNS: usize = SEMMSL * SEMMNI;
/// Maximum undo operations
pub const SEMOPM: usize = 500;
/// Maximum undo entries
pub const SEMUME: usize = SEMOPM;
/// Adjust on exit constant
pub const SEM_UNDO: i16 = 0x1000;
/// Flags
pub const IPC_CREAT: i32 = 0x0200;
pub const IPC_EXCL: i32 = 0x0400;
pub const IPC_NOWAIT: i32 = 0x0800;

/// GETALL, SETALL values
pub const GETALL: i32 = 13;
pub const SETALL: i32 = 14;
pub const GETVAL: i32 = 11;
pub const SETVAL: i32 = 16;
pub const GETPID: i32 = 12;
pub const GETNCNT: i32 = 15;
pub const GETZCNT: i32 = 17;
pub const IPC_RMID: i32 = 0;
pub const IPC_SET: i32 = 1;
pub const IPC_STAT: i32 = 2;

// ============================================================================
// UNNAMED SEMAPHORE
// ============================================================================

pub struct SemUnnamed {
    /// Current value
    pub value: AtomicI32,
    /// Maximum value
    pub max: i32,
    /// Waiters count
    pub waiters: AtomicU32,
    /// Is shared (process-shared)
    pub pshared: bool,
}

impl SemUnnamed {
    pub fn new(value: i32, pshared: bool) -> Self {
        Self {
            value: AtomicI32::new(value),
            max: SEMVMX,
            waiters: AtomicU32::new(0),
            pshared,
        }
    }

    /// Wait (decrement)
    pub fn wait(&self) -> Result<(), SemError> {
        self.waiters.fetch_add(1, Ordering::SeqCst);
        
        loop {
            let current = self.value.load(Ordering::SeqCst);
            
            if current <= 0 {
                // Block until value > 0
                // For now, spin
                continue;
            }
            
            if self.value.compare_exchange(current, current - 1, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                self.waiters.fetch_sub(1, Ordering::SeqCst);
                return Ok(());
            }
        }
    }

    /// Try wait (non-blocking)
    pub fn trywait(&self) -> Result<(), SemError> {
        loop {
            let current = self.value.load(Ordering::SeqCst);
            
            if current <= 0 {
                return Err(SemError::WouldBlock);
            }
            
            if self.value.compare_exchange(current, current - 1, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                return Ok(());
            }
        }
    }

    /// Post (increment)
    pub fn post(&self) -> Result<(), SemError> {
        loop {
            let current = self.value.load(Ordering::SeqCst);
            
            if current >= self.max {
                return Err(SemError::Overflow);
            }
            
            if self.value.compare_exchange(current, current + 1, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                // Wake waiters
                return Ok(());
            }
        }
    }

    /// Get value
    pub fn getvalue(&self) -> i32 {
        self.value.load(Ordering::SeqCst)
    }
}

// ============================================================================
// NAMED SEMAPHORE
// ============================================================================

pub struct SemNamed {
    /// Semaphore name
    pub name: String,
    /// Current value
    pub value: AtomicI32,
    /// Maximum value
    pub max: i32,
    /// Reference count
    pub ref_count: AtomicU32,
    /// Open count
    pub open_count: AtomicU32,
    /// Is unlinked
    pub unlinked: AtomicBool,
}

impl SemNamed {
    pub fn new(name: &str, value: i32) -> Self {
        Self {
            name: String::from(name),
            value: AtomicI32::new(value),
            max: SEMVMX,
            ref_count: AtomicU32::new(1),
            open_count: AtomicU32::new(1),
            unlinked: AtomicBool::new(false),
        }
    }

    /// Wait
    pub fn wait(&self) -> Result<(), SemError> {
        loop {
            let current = self.value.load(Ordering::SeqCst);
            
            if current <= 0 {
                continue;
            }
            
            if self.value.compare_exchange(current, current - 1, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                return Ok(());
            }
        }
    }

    /// Try wait
    pub fn trywait(&self) -> Result<(), SemError> {
        loop {
            let current = self.value.load(Ordering::SeqCst);
            
            if current <= 0 {
                return Err(SemError::WouldBlock);
            }
            
            if self.value.compare_exchange(current, current - 1, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                return Ok(());
            }
        }
    }

    /// Post
    pub fn post(&self) -> Result<(), SemError> {
        loop {
            let current = self.value.load(Ordering::SeqCst);
            
            if current >= self.max {
                return Err(SemError::Overflow);
            }
            
            if self.value.compare_exchange(current, current + 1, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                return Ok(());
            }
        }
    }

    /// Get value
    pub fn getvalue(&self) -> i32 {
        self.value.load(Ordering::SeqCst)
    }

    /// Close
    pub fn close(&self) {
        self.open_count.fetch_sub(1, Ordering::SeqCst);
        self.ref_count.fetch_sub(1, Ordering::SeqCst);
    }

    /// Unlink
    pub fn unlink(&self) {
        self.unlinked.store(true, Ordering::SeqCst);
    }
}

// ============================================================================
// SYSTEM V SEMAPHORE
// ============================================================================

#[derive(Clone, Debug)]
pub struct SemArray {
    /// Semaphore set ID
    pub id: i32,
    /// Key
    pub key: i32,
    /// Permissions
    pub mode: u16,
    /// Owner UID
    pub cuid: u32,
    /// Owner GID
    pub cgid: u32,
    /// Creator UID
    pub uid: u32,
    /// Creator GID
    pub gid: u32,
    /// Number of semaphores
    pub nsems: u16,
    /// Semaphore values
    pub values: Vec<AtomicI32>,
    /// Last operation PID
    pub last_pid: AtomicU32,
    /// Last change time
    pub otime: AtomicU64,
    /// Creation time
    pub ctime: AtomicU64,
    /// Undo list
    pub undo: Mutex<Vec<SemUndo>>,
}

#[derive(Clone, Debug)]
pub struct SemUndo {
    pub semid: i32,
    pub semnum: u16,
    pub adj: i16,
    pub pid: u32,
}

impl SemArray {
    pub fn new(id: i32, key: i32, nsems: u16, mode: u16) -> Self {
        let mut values = Vec::new();
        for _ in 0..nsems {
            values.push(AtomicI32::new(0));
        }
        
        Self {
            id,
            key,
            mode,
            cuid: 0,
            cgid: 0,
            uid: 0,
            gid: 0,
            nsems,
            values,
            last_pid: AtomicU32::new(0),
            otime: AtomicU64::new(0),
            ctime: AtomicU64::new(crate::task::scheduler::get_ticks()),
            undo: Mutex::new(Vec::new()),
        }
    }

    /// Initialize values
    pub fn set_all(&self, vals: &[u16]) {
        for (i, val) in vals.iter().enumerate() {
            if i < self.values.len() {
                self.values[i].store(*val as i32, Ordering::SeqCst);
            }
        }
        self.otime.store(crate::task::scheduler::get_ticks(), Ordering::SeqCst);
    }

    /// Get all values
    pub fn get_all(&self) -> Vec<u16> {
        self.values.iter().map(|v| v.load(Ordering::SeqCst) as u16).collect()
    }

    /// Set single value
    pub fn set_val(&self, semnum: u16, val: i32) {
        if (semnum as usize) < self.values.len() {
            self.values[semnum as usize].store(val, Ordering::SeqCst);
            self.otime.store(crate::task::scheduler::get_ticks(), Ordering::SeqCst);
        }
    }

    /// Get single value
    pub fn get_val(&self, semnum: u16) -> i32 {
        if (semnum as usize) < self.values.len() {
            self.values[semnum as usize].load(Ordering::SeqCst)
        } else {
            -1
        }
    }

    /// Perform operations
    pub fn op(&self, ops: &[Sembuf]) -> Result<(), SemError> {
        // First pass: check if all operations would succeed
        for op in ops {
            if op.sem_num as usize >= self.values.len() {
                return Err(SemError::InvalidSemNum);
            }
            
            let val = self.values[op.sem_num as usize].load(Ordering::SeqCst);
            let new_val = val + op.sem_op as i32;
            
            if new_val < 0 {
                if op.sem_flg & IPC_NOWAIT != 0 {
                    return Err(SemError::WouldBlock);
                }
                // Would need to block
            }
            
            if new_val > SEMVMX {
                return Err(SemError::Overflow);
            }
        }
        
        // Second pass: apply operations
        for op in ops {
            let val = self.values[op.sem_num as usize].load(Ordering::SeqCst);
            let new_val = val + op.sem_op as i32;
            self.values[op.sem_num as usize].store(new_val, Ordering::SeqCst);
            
            // Add undo entry if SEM_UNDO flag
            if op.sem_flg & SEM_UNDO != 0 {
                self.undo.lock().push(SemUndo {
                    semid: self.id,
                    semnum: op.sem_num,
                    adj: -op.sem_op,
                    pid: 0, // Current process
                });
            }
        }
        
        self.last_pid.store(0, Ordering::SeqCst); // Current PID
        self.otime.store(crate::task::scheduler::get_ticks(), Ordering::SeqCst);
        
        Ok(())
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Sembuf {
    pub sem_num: u16,
    pub sem_op: i16,
    pub sem_flg: i16,
}

// ============================================================================
// SEMAPHORE MANAGER
// ============================================================================

pub struct SemManager {
    /// Named semaphores
    named: Mutex<BTreeMap<String, Arc<SemNamed>>>,
    /// System V semaphore arrays
    sysv: Mutex<BTreeMap<i32, Arc<SemArray>>>,
    /// Next System V ID
    next_id: AtomicI32,
    /// Statistics
    stats: Mutex<SemStats>,
}

#[derive(Clone, Debug, Default)]
pub struct SemStats {
    pub named_count: u32,
    pub sysv_count: u32,
    pub total_ops: u64,
}

impl SemManager {
    pub const fn new() -> Self {
        Self {
            named: Mutex::new(BTreeMap::new()),
            sysv: Mutex::new(BTreeMap::new()),
            next_id: AtomicI32::new(1),
            stats: Mutex::new(SemStats::default()),
        }
    }

    /// Open named semaphore
    pub fn sem_open(&self, name: &str, oflag: i32, mode: u32, value: i32) -> Result<Arc<SemNamed>, SemError> {
        let mut named = self.named.lock();
        
        if let Some(sem) = named.get(name) {
            if oflag & IPC_EXCL != 0 {
                return Err(SemError::AlreadyExists);
            }
            sem.open_count.fetch_add(1, Ordering::SeqCst);
            return Ok(sem.clone());
        }
        
        if oflag & IPC_CREAT == 0 {
            return Err(SemError::NotFound);
        }
        
        let sem = Arc::new(SemNamed::new(name, value));
        named.insert(String::from(name), sem.clone());
        
        let mut stats = self.stats.lock();
        stats.named_count += 1;
        
        Ok(sem)
    }

    /// Close named semaphore
    pub fn sem_close(&self, name: &str) -> Result<(), SemError> {
        if let Some(sem) = self.named.lock().get(name) {
            sem.close();
            
            if sem.open_count.load(Ordering::SeqCst) == 0 && sem.unlinked.load(Ordering::SeqCst) {
                self.named.lock().remove(name);
            }
            
            return Ok(());
        }
        Err(SemError::NotFound)
    }

    /// Unlink named semaphore
    pub fn sem_unlink(&self, name: &str) -> Result<(), SemError> {
        if let Some(sem) = self.named.lock().get(name) {
            sem.unlink();
            
            if sem.open_count.load(Ordering::SeqCst) == 0 {
                self.named.lock().remove(name);
            }
            
            return Ok(());
        }
        Err(SemError::NotFound)
    }

    /// Create System V semaphore set
    pub fn semget(&self, key: i32, nsems: u16, semflg: i32) -> Result<i32, SemError> {
        let mut sysv = self.sysv.lock();
        
        // Check if exists
        for sem in sysv.values() {
            if sem.key == key && key != 0 {
                if semflg & IPC_EXCL != 0 {
                    return Err(SemError::AlreadyExists);
                }
                return Ok(sem.id);
            }
        }
        
        if semflg & IPC_CREAT == 0 {
            return Err(SemError::NotFound);
        }
        
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let sem = Arc::new(SemArray::new(id, key, nsems, (semflg & 0x1FF) as u16));
        sysv.insert(id, sem);
        
        let mut stats = self.stats.lock();
        stats.sysv_count += 1;
        
        Ok(id)
    }

    /// System V semaphore control
    pub fn semctl(&self, semid: i32, semnum: u16, cmd: i32, arg: u64) -> Result<i32, SemError> {
        let sysv = self.sysv.lock();
        
        let sem = sysv.get(&semid).ok_or(SemError::NotFound)?;
        
        match cmd {
            IPC_RMID => {
                drop(sysv);
                self.sysv.lock().remove(&semid);
                Ok(0)
            }
            IPC_STAT => {
                // Copy semid_ds to arg
                Ok(0)
            }
            IPC_SET => {
                // Set from arg
                Ok(0)
            }
            GETALL => {
                let vals = sem.get_all();
                // Copy to arg
                Ok(vals.len() as i32)
            }
            SETALL => {
                let vals = unsafe {
                    core::slice::from_raw_parts(arg as *const u16, sem.nsems as usize)
                };
                sem.set_all(vals);
                Ok(0)
            }
            GETVAL => {
                Ok(sem.get_val(semnum))
            }
            SETVAL => {
                sem.set_val(semnum, arg as i32);
                Ok(0)
            }
            GETPID => {
                Ok(sem.last_pid.load(Ordering::SeqCst) as i32)
            }
            GETNCNT => {
                // Number waiting for semval > current
                Ok(0)
            }
            GETZCNT => {
                // Number waiting for semval == 0
                Ok(0)
            }
            _ => Err(SemError::InvalidCommand),
        }
    }

    /// System V semaphore operation
    pub fn semop(&self, semid: i32, ops: &[Sembuf]) -> Result<(), SemError> {
        let sysv = self.sysv.lock();
        let sem = sysv.get(&semid).ok_or(SemError::NotFound)?;
        
        let result = sem.op(ops);
        
        let mut stats = self.stats.lock();
        stats.total_ops += 1;
        
        result
    }

    /// Get statistics
    pub fn get_stats(&self) -> SemStats {
        self.stats.lock().clone()
    }
}

lazy_static::lazy_static! {
    pub static ref SEM_MANAGER: SemManager = SemManager::new();
}

// ============================================================================
// ERROR TYPE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemError {
    NotFound,
    AlreadyExists,
    WouldBlock,
    Overflow,
    InvalidSemNum,
    InvalidCommand,
    PermissionDenied,
}

// ============================================================================
// SYSCALL INTERFACE
// ============================================================================

pub fn sys_semget(key: i32, nsems: u16, semflg: i32) -> i32 {
    match SEM_MANAGER.semget(key, nsems, semflg) {
        Ok(id) => id,
        Err(SemError::NotFound) => -2,
        Err(SemError::AlreadyExists) => -17,
        Err(_) => -22,
    }
}

pub fn sys_semop(semid: i32, ops: &[Sembuf]) -> i32 {
    match SEM_MANAGER.semop(semid, ops) {
        Ok(()) => 0,
        Err(SemError::WouldBlock) => -11,
        Err(SemError::NotFound) => -22,
        Err(_) => -5,
    }
}

pub fn sys_semctl(semid: i32, semnum: u16, cmd: i32, arg: u64) -> i32 {
    match SEM_MANAGER.semctl(semid, semnum, cmd, arg) {
        Ok(val) => val,
        Err(SemError::NotFound) => -22,
        Err(_) => -5,
    }
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init() {
    crate::serial_println!("[SEM] POSIX Semaphores initialized");
}
