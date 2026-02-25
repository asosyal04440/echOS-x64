//! Signal Handling and Job Control
//!
//! POSIX uyumlu signal ve job control sistemi.
//! SIGINT, SIGTERM, SIGKILL, SIGTSTP, SIGCONT, SIGCHLD vb.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

/// Signal numaraları (POSIX standard)
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Signal {
    /// Hangup (terminal kapatıldı)
    SIGHUP = 1,
    /// Interrupt (Ctrl+C)
    SIGINT = 2,
    /// Quit (Ctrl+\)
    SIGQUIT = 3,
    /// Illegal instruction
    SIGILL = 4,
    /// Trace trap (debugger)
    SIGTRAP = 5,
    /// Abort
    SIGABRT = 6,
    /// Bus error
    SIGBUS = 7,
    /// Floating-point exception
    SIGFPE = 8,
    /// Kill (uncatchable)
    SIGKILL = 9,
    /// User-defined 1
    SIGUSR1 = 10,
    /// Segmentation fault
    SIGSEGV = 11,
    /// User-defined 2
    SIGUSR2 = 12,
    /// Pipe write with no reader
    SIGPIPE = 13,
    /// Alarm clock
    SIGALRM = 14,
    /// Termination
    SIGTERM = 15,
    /// Stack fault
    SIGSTKFLT = 16,
    /// Child stopped or terminated
    SIGCHLD = 17,
    /// Continue (from stop)
    SIGCONT = 18,
    /// Stop (Ctrl+Z)
    SIGSTOP = 19,
    /// Terminal stop (Ctrl+Z)
    SIGTSTP = 20,
    /// Background read from tty
    SIGTTIN = 21,
    /// Background write to tty
    SIGTTOU = 22,
    /// Urgent condition
    SIGURG = 23,
    /// CPU limit exceeded
    SIGXCPU = 24,
    /// File size limit exceeded
    SIGXFSZ = 25,
    /// Virtual timer expired
    SIGVTALRM = 26,
    /// Profiling timer expired
    SIGPROF = 27,
    /// Window size change
    SIGWINCH = 28,
    /// I/O possible
    SIGIO = 29,
    /// Power failure
    SIGPWR = 30,
    /// Bad system call
    SIGSYS = 31,
}

impl Signal {
    /// Numaradan signal oluşturur
    pub fn from_number(n: u8) -> Option<Self> {
        match n {
            1 => Some(Signal::SIGHUP),
            2 => Some(Signal::SIGINT),
            3 => Some(Signal::SIGQUIT),
            4 => Some(Signal::SIGILL),
            5 => Some(Signal::SIGTRAP),
            6 => Some(Signal::SIGABRT),
            7 => Some(Signal::SIGBUS),
            8 => Some(Signal::SIGFPE),
            9 => Some(Signal::SIGKILL),
            10 => Some(Signal::SIGUSR1),
            11 => Some(Signal::SIGSEGV),
            12 => Some(Signal::SIGUSR2),
            13 => Some(Signal::SIGPIPE),
            14 => Some(Signal::SIGALRM),
            15 => Some(Signal::SIGTERM),
            16 => Some(Signal::SIGSTKFLT),
            17 => Some(Signal::SIGCHLD),
            18 => Some(Signal::SIGCONT),
            19 => Some(Signal::SIGSTOP),
            20 => Some(Signal::SIGTSTP),
            21 => Some(Signal::SIGTTIN),
            22 => Some(Signal::SIGTTOU),
            23 => Some(Signal::SIGURG),
            24 => Some(Signal::SIGXCPU),
            25 => Some(Signal::SIGXFSZ),
            26 => Some(Signal::SIGVTALRM),
            27 => Some(Signal::SIGPROF),
            28 => Some(Signal::SIGWINCH),
            29 => Some(Signal::SIGIO),
            30 => Some(Signal::SIGPWR),
            31 => Some(Signal::SIGSYS),
            _ => None,
        }
    }
    
    /// Signal numarasını döndürür
    pub fn number(&self) -> u8 {
        *self as u8
    }
    
    /// Signal adını döndürür
    pub fn name(&self) -> &'static str {
        match self {
            Signal::SIGHUP => "SIGHUP",
            Signal::SIGINT => "SIGINT",
            Signal::SIGQUIT => "SIGQUIT",
            Signal::SIGILL => "SIGILL",
            Signal::SIGTRAP => "SIGTRAP",
            Signal::SIGABRT => "SIGABRT",
            Signal::SIGBUS => "SIGBUS",
            Signal::SIGFPE => "SIGFPE",
            Signal::SIGKILL => "SIGKILL",
            Signal::SIGUSR1 => "SIGUSR1",
            Signal::SIGSEGV => "SIGSEGV",
            Signal::SIGUSR2 => "SIGUSR2",
            Signal::SIGPIPE => "SIGPIPE",
            Signal::SIGALRM => "SIGALRM",
            Signal::SIGTERM => "SIGTERM",
            Signal::SIGSTKFLT => "SIGSTKFLT",
            Signal::SIGCHLD => "SIGCHLD",
            Signal::SIGCONT => "SIGCONT",
            Signal::SIGSTOP => "SIGSTOP",
            Signal::SIGTSTP => "SIGTSTP",
            Signal::SIGTTIN => "SIGTTIN",
            Signal::SIGTTOU => "SIGTTOU",
            Signal::SIGURG => "SIGURG",
            Signal::SIGXCPU => "SIGXCPU",
            Signal::SIGXFSZ => "SIGXFSZ",
            Signal::SIGVTALRM => "SIGVTALRM",
            Signal::SIGPROF => "SIGPROF",
            Signal::SIGWINCH => "SIGWINCH",
            Signal::SIGIO => "SIGIO",
            Signal::SIGPWR => "SIGPWR",
            Signal::SIGSYS => "SIGSYS",
        }
    }
    
    /// Signal yakalanabilir mi?
    pub fn is_catchable(&self) -> bool {
        !matches!(self, Signal::SIGKILL | Signal::SIGSTOP)
    }
    
    /// Signal durdurma signalı mi?
    pub fn is_stop_signal(&self) -> bool {
        matches!(self, Signal::SIGSTOP | Signal::SIGTSTP | Signal::SIGTTIN | Signal::SIGTTOU)
    }
    
    /// Signal devam signalı mi?
    pub fn is_continue_signal(&self) -> bool {
        matches!(self, Signal::SIGCONT)
    }
}

/// Signal action
#[derive(Clone, Copy, Debug)]
pub enum SignalAction {
    /// Default action
    Default,
    /// Ignore signal
    Ignore,
    /// Catch with handler (user-space address)
    Catch(usize),
}

impl Default for SignalAction {
    fn default() -> Self {
        SignalAction::Default
    }
}

/// Default signal davranışları
pub fn default_action(sig: Signal) -> SignalDisposition {
    match sig {
        Signal::SIGCHLD | Signal::SIGURG | Signal::SIGWINCH | Signal::SIGIO | Signal::SIGPWR => {
            SignalDisposition::Ignore
        }
        Signal::SIGSTOP | Signal::SIGTSTP | Signal::SIGTTIN | Signal::SIGTTOU => {
            SignalDisposition::Stop
        }
        Signal::SIGCONT => {
            SignalDisposition::Continue
        }
        _ => SignalDisposition::Terminate,
    }
}

/// Signal disposition (default davranış)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalDisposition {
    Ignore,
    Terminate,
    Stop,
    Continue,
    CoreDump,
}

/// Signal info (siginfo_t benzeri)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SigInfo {
    pub si_signo: i32,
    pub si_errno: i32,
    pub si_code: i32,
    pub si_pid: i32,
    pub si_uid: u32,
    pub si_status: i32,
    pub si_value: usize,
}

/// sigaction yapısı
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SigAction {
    pub sa_handler: usize,  // Function pointer or SIG_IGN/SIG_DFL
    pub sa_mask: u64,
    pub sa_flags: u32,
    pub sa_restorer: usize,
}

// Special handler values
pub const SIG_DFL: usize = 0;  // Default
pub const SIG_IGN: usize = 1;  // Ignore

// sigaction flags (sa_flags)
pub const SA_NOCLDSTOP: u32 = 0x00000001;  // Don't send SIGCHLD when child stops
pub const SA_NOCLDWAIT: u32 = 0x00000002;  // Don't create zombie on child death
pub const SA_SIGINFO: u32   = 0x00000004;  // Use sa_sigaction handler
pub const SA_RESTART: u32   = 0x10000000;  // Restart syscalls on signal
pub const SA_NODEFER: u32   = 0x40000000;  // Don't block signal while handling
pub const SA_RESETHAND: u32 = 0x80000000;  // Reset to SIG_DFL after handling
pub const SA_ONSTACK: u32   = 0x08000000;  // Use alternate signal stack

// sigprocmask how values
pub const SIG_BLOCK: i32   = 0;
pub const SIG_UNBLOCK: i32 = 1;
pub const SIG_SETMASK: i32 = 2;

/// Signal handler tablosu (per-process)
pub struct SignalHandlers {
    handlers: [SignalAction; 32],
    mask: AtomicU64,
    pending: AtomicU64,
}

impl SignalHandlers {
    pub fn new() -> Self {
        let mut handlers = [SignalAction::Default; 32];
        // SIGCHLD default olarak ignore
        handlers[17] = SignalAction::Ignore;
        
        Self {
            handlers,
            mask: AtomicU64::new(0),
            pending: AtomicU64::new(0),
        }
    }
    
    /// Signal action ayarlar
    pub fn set_action(&mut self, sig: Signal, action: SignalAction) -> SignalAction {
        if !sig.is_catchable() {
            return SignalAction::Default;
        }
        let idx = sig.number() as usize;
        if idx >= 32 {
            return SignalAction::Default;
        }
        core::mem::replace(&mut self.handlers[idx], action)
    }
    
    /// Signal action döndürür
    pub fn get_action(&self, sig: Signal) -> &SignalAction {
        let idx = sig.number() as usize;
        if idx >= 32 {
            return &self.handlers[0];
        }
        &self.handlers[idx]
    }
    
    /// Signal mask ayarlar
    pub fn set_mask(&self, mask: u64) {
        self.mask.store(mask, Ordering::SeqCst);
    }
    
    /// Signal mask döndürür
    pub fn get_mask(&self) -> u64 {
        self.mask.load(Ordering::SeqCst)
    }
    
    /// Signal block'lar
    pub fn block(&self, sig: Signal) {
        let mask = 1u64 << (sig.number() - 1);
        self.mask.fetch_or(mask, Ordering::SeqCst);
    }
    
    /// Signal unblock'lar
    pub fn unblock(&self, sig: Signal) {
        let mask = 1u64 << (sig.number() - 1);
        self.mask.fetch_and(!mask, Ordering::SeqCst);
    }
    
    /// Pending signal ekler
    pub fn add_pending(&self, sig: Signal) {
        let mask = 1u64 << (sig.number() - 1);
        self.pending.fetch_or(mask, Ordering::SeqCst);
    }
    
    /// Pending signal'ı temizler
    pub fn clear_pending(&self, sig: Signal) {
        let mask = 1u64 << (sig.number() - 1);
        self.pending.fetch_and(!mask, Ordering::SeqCst);
    }
    
    /// Pending signal'ları döndürür
    pub fn get_pending(&self) -> u64 {
        self.pending.load(Ordering::SeqCst)
    }
    
    /// Bir sonraki pending signal'ı döndürür
    pub fn next_pending(&self) -> Option<Signal> {
        let pending = self.pending.load(Ordering::SeqCst);
        let mask = self.mask.load(Ordering::SeqCst);
        let deliverable = pending & !mask;
        
        if deliverable == 0 {
            return None;
        }
        
        // En düşük numaralı signal'i bul
        for i in 0..32 {
            if deliverable & (1u64 << i) != 0 {
                return Signal::from_number(i as u8 + 1);
            }
        }
        None
    }
}

impl Default for SignalHandlers {
    fn default() -> Self {
        Self::new()
    }
}

/// Process state (job control için)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessState {
    Running,
    Stopped,
    Zombie,
    Background,
    Foreground,
}

/// Job yapısı
#[derive(Clone, Debug)]
pub struct Job {
    /// Job ID (shell'de kullanılır)
    pub job_id: usize,
    /// Process group ID
    pub pgid: usize,
    /// Komut satırı
    pub command: String,
    /// Job durumu
    pub state: JobState,
    /// Process'ler (PID listesi)
    pub processes: Vec<usize>,
}

/// Job durumu
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobState {
    Running,
    Stopped,
    Done,
}

/// Job Control Manager
pub struct JobManager {
    /// Job listesi (job_id -> Job)
    jobs: Mutex<BTreeMap<usize, Job>>,
    /// Next job ID
    next_job_id: Mutex<usize>,
    /// Foreground process group ID
    foreground_pgid: Mutex<usize>,
}

impl JobManager {
    pub const fn new() -> Self {
        Self {
            jobs: Mutex::new(BTreeMap::new()),
            next_job_id: Mutex::new(1),
            foreground_pgid: Mutex::new(0),
        }
    }
    
    /// Yeni job oluşturur
    pub fn create_job(&self, pgid: usize, command: &str) -> usize {
        let mut next_id = self.next_job_id.lock();
        let job_id = *next_id;
        *next_id += 1;
        
        let job = Job {
            job_id,
            pgid,
            command: String::from(command),
            state: JobState::Running,
            processes: vec![pgid],
        };
        
        self.jobs.lock().insert(job_id, job);
        crate::serial_println!("[JOB] Created job [{}] {}", job_id, command);
        job_id
    }
    
    /// Job'ı durdurur (SIGTSTP)
    pub fn stop_job(&self, job_id: usize) -> bool {
        let mut jobs = self.jobs.lock();
        if let Some(job) = jobs.get_mut(&job_id) {
            job.state = JobState::Stopped;
            crate::serial_println!("[JOB] Stopped job [{}] {}", job_id, job.command);
            return true;
        }
        false
    }
    
    /// Job'ı devam ettirir (SIGCONT)
    pub fn continue_job(&self, job_id: usize, foreground: bool) -> bool {
        let mut jobs = self.jobs.lock();
        if let Some(job) = jobs.get_mut(&job_id) {
            job.state = JobState::Running;
            if foreground {
                *self.foreground_pgid.lock() = job.pgid;
            }
            crate::serial_println!("[JOB] Continued job [{}] {} ({})", 
                job_id, job.command, if foreground { "fg" } else { "bg" });
            return true;
        }
        false
    }
    
    /// Job'ı tamamlanmış olarak işaretler
    pub fn finish_job(&self, job_id: usize) -> bool {
        let mut jobs = self.jobs.lock();
        if let Some(job) = jobs.get_mut(&job_id) {
            job.state = JobState::Done;
            crate::serial_println!("[JOB] Finished job [{}] {}", job_id, job.command);
            return true;
        }
        false
    }
    
    /// Job'ı siler
    pub fn remove_job(&self, job_id: usize) -> Option<Job> {
        self.jobs.lock().remove(&job_id)
    }
    
    /// Job'ı ID ile bulur
    pub fn get_job(&self, job_id: usize) -> Option<Job> {
        self.jobs.lock().get(&job_id).cloned()
    }
    
    /// Tüm job'ları listeler
    pub fn list_jobs(&self) -> Vec<Job> {
        self.jobs.lock().values().cloned().collect()
    }
    
    /// Foreground process group ayarlar
    pub fn set_foreground(&self, pgid: usize) {
        *self.foreground_pgid.lock() = pgid;
    }
    
    /// Foreground process group döndürür
    pub fn get_foreground(&self) -> usize {
        *self.foreground_pgid.lock()
    }
    
    /// Process group'a job bulur
    pub fn find_by_pgid(&self, pgid: usize) -> Option<Job> {
        self.jobs.lock().values()
            .find(|j| j.pgid == pgid)
            .cloned()
    }
}

lazy_static::lazy_static! {
    /// Global job manager
    pub static ref JOB_MANAGER: JobManager = JobManager::new();
}

/// Signal gönderir (kill syscall benzeri)
pub fn send_signal(pid: usize, sig: Signal) -> Result<(), SignalError> {
    // TODO: Gerçek process'e signal gönder
    crate::serial_println!("[SIGNAL] Sending {} to PID {}", sig.name(), pid);
    
    // Signal'i process'in pending listesine ekle
    // Bu kısım scheduler entegrasyonu ile yapılacak
    
    Ok(())
}

/// Signal gönderir (process group'a)
pub fn send_signal_pgroup(pgid: usize, sig: Signal) -> Result<(), SignalError> {
    crate::serial_println!("[SIGNAL] Sending {} to PGID {}", sig.name(), pgid);
    
    // TODO: Process group'taki tüm process'lere signal gönder
    
    Ok(())
}

/// Signal gönderir (tüm process'lere)
pub fn send_signal_all(sig: Signal) -> Result<(), SignalError> {
    crate::serial_println!("[SIGNAL] Broadcasting {}", sig.name());
    
    // TODO: Tüm process'lere signal gönder
    
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalError {
    InvalidSignal,
    ProcessNotFound,
    PermissionDenied,
}

/// Terminal'den signal üretir (Ctrl+C, Ctrl+Z vb.)
pub fn generate_terminal_signal(sig: Signal) {
    // Foreground process group'a signal gönder
    let fg_pgid = JOB_MANAGER.get_foreground();
    if fg_pgid != 0 {
        let _ = send_signal_pgroup(fg_pgid, sig);
    }
    
    // Job'ı güncelle
    if sig == Signal::SIGTSTP {
        if let Some(job) = JOB_MANAGER.find_by_pgid(fg_pgid) {
            JOB_MANAGER.stop_job(job.job_id);
        }
    }
}

/// Signal alt sistemini başlatır
pub fn init() {
    crate::serial_println!("[SIGNAL] Subsystem initialized");
}

// ============================================================================
// SYSCALL INTERFACE
// ============================================================================

/// sigaction syscall implementation
/// 
/// # Arguments
/// - `signum`: Signal number (1-31)
/// - `act`: New signal action (optional)
/// - `oldact`: Buffer to store old action (optional)
/// 
/// # Returns
/// 0 on success, negative errno on failure
pub fn sys_sigaction(
    handlers: &mut SignalHandlers,
    signum: i32,
    act: Option<SigAction>,
    oldact: Option<&mut SigAction>,
) -> i32 {
    // Validate signal number
    if signum < 1 || signum > 31 {
        return -22; // EINVAL
    }
    
    let sig = match Signal::from_number(signum as u8) {
        Some(s) => s,
        None => return -22, // EINVAL
    };
    
    // SIGKILL and SIGSTOP cannot be caught
    if !sig.is_catchable() && act.is_some() {
        return -22; // EINVAL
    }
    
    // Store old action if requested
    if let Some(old) = oldact {
        let current = handlers.get_action(sig);
        *old = match current {
            SignalAction::Default => SigAction {
                sa_handler: SIG_DFL,
                sa_mask: 0,
                sa_flags: 0,
                sa_restorer: 0,
            },
            SignalAction::Ignore => SigAction {
                sa_handler: SIG_IGN,
                sa_mask: 0,
                sa_flags: 0,
                sa_restorer: 0,
            },
            SignalAction::Catch(addr) => SigAction {
                sa_handler: *addr,
                sa_mask: handlers.get_mask(),
                sa_flags: 0,
                sa_restorer: 0,
            },
        };
    }
    
    // Set new action if provided
    if let Some(new_act) = act {
        let action = if new_act.sa_handler == SIG_DFL {
            SignalAction::Default
        } else if new_act.sa_handler == SIG_IGN {
            SignalAction::Ignore
        } else {
            SignalAction::Catch(new_act.sa_handler)
        };
        
        handlers.set_action(sig, action);
        
        // Apply signal mask from sa_mask
        if new_act.sa_flags & SA_NODEFER == 0 {
            // Add signal to mask during handler execution
            let mut mask = new_act.sa_mask;
            if new_act.sa_flags & SA_RESETHAND != 0 {
                // Reset to default after handler
            }
            handlers.set_mask(mask);
        }
        
        crate::serial_println!("[SIGNAL] sigaction({}) -> {:?}", sig.name(), action);
    }
    
    0 // Success
}

/// sigprocmask syscall implementation
/// 
/// # Arguments
/// - `how`: SIG_BLOCK, SIG_UNBLOCK, or SIG_SETMASK
/// - `set`: New signal set (optional)
/// - `oldset`: Buffer to store old mask (optional)
/// 
/// # Returns
/// 0 on success, negative errno on failure
pub fn sys_sigprocmask(
    handlers: &SignalHandlers,
    how: i32,
    set: Option<u64>,
    oldset: Option<&mut u64>,
) -> i32 {
    // Store old mask if requested
    if let Some(old) = oldset {
        *old = handlers.get_mask();
    }
    
    // Apply new mask if provided
    if let Some(new_mask) = set {
        match how {
            SIG_BLOCK => {
                // Add signals to mask
                let current = handlers.get_mask();
                handlers.set_mask(current | new_mask);
            }
            SIG_UNBLOCK => {
                // Remove signals from mask
                let current = handlers.get_mask();
                handlers.set_mask(current & !new_mask);
            }
            SIG_SETMASK => {
                // Replace mask entirely
                handlers.set_mask(new_mask);
            }
            _ => return -22, // EINVAL
        }
        
        crate::serial_println!("[SIGNAL] sigprocmask(how={}, mask={:#x})", how, new_mask);
    }
    
    0 // Success
}

/// sigpending syscall implementation
/// 
/// Returns the set of pending signals that are blocked
pub fn sys_sigpending(handlers: &SignalHandlers) -> u64 {
    handlers.get_pending()
}

/// sigsuspend syscall implementation
/// 
/// Atomically replaces signal mask and suspends the process
/// until a signal is caught
pub fn sys_sigsuspend(handlers: &SignalHandlers, mask: u64) -> i32 {
    let old_mask = handlers.get_mask();
    handlers.set_mask(mask);
    
    // TODO: Actually suspend the process
    // This should be integrated with the scheduler
    
    // When we return, restore the old mask
    handlers.set_mask(old_mask);
    
    -4 // EINTR (interrupted by signal)
}

/// kill syscall implementation
/// 
/// Send a signal to a process or process group
pub fn sys_kill(pid: i32, signum: i32) -> i32 {
    if signum < 0 || signum > 31 {
        return -22; // EINVAL
    }
    
    let sig = if signum > 0 {
        match Signal::from_number(signum as u8) {
            Some(s) => Some(s),
            None => return -22, // EINVAL
        }
    } else {
        None // Signal 0 = existence check
    };
    
    if pid > 0 {
        // Send to specific process
        if let Some(signal) = sig {
            match send_signal(pid as usize, signal) {
                Ok(()) => 0,
                Err(SignalError::ProcessNotFound) => -3, // ESRCH
                Err(SignalError::PermissionDenied) => -1, // EPERM
                Err(SignalError::InvalidSignal) => -22, // EINVAL
            }
        } else {
            // Signal 0: just check if process exists
            // TODO: Check if process exists
            0
        }
    } else if pid == 0 {
        // Send to current process group
        if let Some(signal) = sig {
            // TODO: Get current process group
            0
        } else {
            0
        }
    } else if pid == -1 {
        // Send to all processes (except init)
        if let Some(signal) = sig {
            match send_signal_all(signal) {
                Ok(()) => 0,
                Err(_) => -1,
            }
        } else {
            0
        }
    } else {
        // pid < -1: Send to process group -pid
        if let Some(signal) = sig {
            match send_signal_pgroup((-pid) as usize, signal) {
                Ok(()) => 0,
                Err(SignalError::ProcessNotFound) => -3, // ESRCH
                Err(_) => -1,
            }
        } else {
            0
        }
    }
}

/// raise syscall implementation
/// 
/// Send a signal to the current process
pub fn sys_raise(signum: i32) -> i32 {
    let pid = crate::task::scheduler::current_task_id() as i32;
    sys_kill(pid, signum)
}

/// pause syscall implementation
/// 
/// Suspend the process until a signal is caught
pub fn sys_pause() -> i32 {
    // TODO: Integrate with scheduler to actually suspend
    -4 // EINTR
}

/// alarm syscall implementation
/// 
/// Schedule SIGALRM after specified seconds
pub fn sys_alarm(seconds: u32) -> u32 {
    // TODO: Implement timer-based alarm
    // For now, return 0 (no previous alarm)
    0
}

/// sigaltstack syscall implementation
/// 
/// Set/get alternate signal stack
pub fn sys_sigaltstack(ss: Option<usize>, old_ss: Option<&mut usize>) -> i32 {
    // TODO: Implement alternate signal stack
    0
}

// ============================================================================
// SIGNAL DELIVERY
// ============================================================================

/// Deliver pending signals to a process
/// 
/// This should be called before returning to userspace
/// 
/// # Returns
/// - `Some(handler_addr)`: Signal handler to call
/// - `None`: No signal to deliver
pub fn deliver_signals(handlers: &mut SignalHandlers) -> Option<(usize, SigInfo)> {
    // Get next pending signal that's not blocked
    let sig = handlers.next_pending()?;
    
    // Clear pending flag
    handlers.clear_pending(sig);
    
    // Get handler
    let action = handlers.get_action(sig).clone();
    
    match action {
        SignalAction::Default => {
            // Perform default action
            let disposition = default_action(sig);
            match disposition {
                SignalDisposition::Ignore => {
                    // Do nothing
                    return None;
                }
                SignalDisposition::Terminate => {
                    // Terminate process
                    crate::serial_println!("[SIGNAL] Default action: terminate due to {}", sig.name());
                    // TODO: Actually terminate the process
                    return None;
                }
                SignalDisposition::Stop => {
                    // Stop process
                    crate::serial_println!("[SIGNAL] Default action: stop due to {}", sig.name());
                    // TODO: Stop the process
                    return None;
                }
                SignalDisposition::Continue => {
                    // Continue if stopped
                    return None;
                }
                SignalDisposition::CoreDump => {
                    // Terminate with core dump
                    crate::serial_println!("[SIGNAL] Default action: core dump due to {}", sig.name());
                    // TODO: Core dump
                    return None;
                }
            }
        }
        SignalAction::Ignore => {
            return None;
        }
        SignalAction::Catch(handler_addr) => {
            // Build siginfo
            let siginfo = SigInfo {
                si_signo: sig.number() as i32,
                si_errno: 0,
                si_code: 0, // SI_USER
                si_pid: 0,
                si_uid: 0,
                si_status: 0,
                si_value: 0,
            };
            
            // Block the signal during handler (unless SA_NODEFER)
            handlers.block(sig);
            
            crate::serial_println!("[SIGNAL] Delivering {} to handler {:#x}", sig.name(), handler_addr);
            
            return Some((handler_addr, siginfo));
        }
    }
    
    None
}

/// Check if process has pending signals
pub fn has_pending_signals(handlers: &SignalHandlers) -> bool {
    let pending = handlers.get_pending();
    let mask = handlers.get_mask();
    (pending & !mask) != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    // ... (rest of the code remains the same)
    #[test]
    fn test_signal_numbers() {
        assert_eq!(Signal::SIGINT.number(), 2);
        assert_eq!(Signal::SIGKILL.number(), 9);
        assert_eq!(Signal::SIGTERM.number(), 15);
    }
    
    #[test]
    fn test_signal_handlers() {
        let mut handlers = SignalHandlers::new();
        handlers.set_action(Signal::SIGINT, SignalAction::Ignore);
        
        assert_eq!(handlers.get_action(Signal::SIGINT), &SignalAction::Ignore);
    }
    
    #[test]
    fn test_job_manager() {
        let job_id = JOB_MANAGER.create_job(100, "test command");
        assert!(job_id > 0);
        
        let job = JOB_MANAGER.get_job(job_id);
        assert!(job.is_some());
    }
}