//! # echOS Sinyal İşleme ve İş Kontrolü (Signal Handling & Job Control)
//!
//! POSIX uyumlu sinyal (signal) ve iş kontrolü (job control) sistemi.
//! SIGINT, SIGTERM, SIGKILL, SIGTSTP, SIGCONT, SIGCHLD vb. sinyalleri destekler.
//!
//! ## POSIX Sinyal Teslim Mekanizması
//!
//! ```text
//!  ┌──────────────────────────────────────────────────────────────┐
//!  │             POSIX SİNYAL TESLİM AKIŞI                       │
//!  │                                                              │
//!  │  Sinyal Kaynakları:                                          │
//!  │  kill(pid, SIGTERM)  ─→  sys_kill()                         │
//!  │  Ctrl+C              ─→  generate_terminal_signal(SIGINT)   │
//!  │  donanım hatası      ─→  SIGSEGV / SIGFPE / SIGILL          │
//!  │                           ↓                                  │
//!  │  Hedef process'in sigpending kuyruğuna eklenir               │
//!  │                           ↓                                  │
//!  │  Kernel → Kullanıcı alanına dönüşte deliver_signals() çağrılır│
//!  │        ├── Sinyal maskelenmiş (sigmask)?  → Bekle            │
//!  │        ├── Eylem: SIG_IGN?               → Yoksay           │
//!  │        ├── Eylem: SIG_DFL?               → Varsayılan uygula│
//!  │        └── Eylem: Catch(handler_addr)?   → Handler çağır    │
//!  │                           ↓                                  │
//!  │  Handler çalışırken sinyal otomatik maskelenir               │
//!  │  (SA_NODEFER bayraksız), handler bitince maske geri yüklenir │
//!  └──────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Signal Mask (Sinyal Maskesi)
//!
//! ```text
//!  64-bit mask: her bit bir sinyal numarasını temsil eder
//!  bit 0 = SIGHUP (1), bit 1 = SIGINT (2), ..., bit 30 = SIGSYS (31)
//!
//!  Örnek: SIGINT ve SIGTERM maskele (blokla)
//!  mask = (1 << (SIGINT-1)) | (1 << (SIGTERM-1)) = 0x4002
//!
//!  SIG_BLOCK   → mevcut mask | yeni_mask (sinyaller eklenir)
//!  SIG_UNBLOCK → mevcut mask & ~yeni_mask (sinyaller kaldırılır)
//!  SIG_SETMASK → mask tamamen yeni_mask olarak ayarlanır
//! ```

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
    /// Geçersiz komut (talimat)
    SIGILL = 4,
    /// İzleme kapanı (hata ayıklayıcı)
    SIGTRAP = 5,
    /// Durdurma (programın kendisini durdurması)
    SIGABRT = 6,
    /// Veri yolu hatası
    SIGBUS = 7,
    /// Kayan nokta istisnası
    SIGFPE = 8,
    /// Sonlandırma (yakalanamaz)
    SIGKILL = 9,
    /// Kullanıcı tanımlı sinyal 1
    SIGUSR1 = 10,
    /// Segmentasyon hatası
    SIGSEGV = 11,
    /// Kullanıcı tanımlı sinyal 2
    SIGUSR2 = 12,
    /// Okuyucusu olmayan boru yazma
    SIGPIPE = 13,
    /// Alarm saati
    SIGALRM = 14,
    /// Sonlandırma (nezaket isteği)
    SIGTERM = 15,
    /// Yığın hatası
    SIGSTKFLT = 16,
    /// Alt süreç durdu veya sonlandı
    SIGCHLD = 17,
    /// Devam et (durdurulmuştan)
    SIGCONT = 18,
    /// Stop (Ctrl+Z)
    SIGSTOP = 19,
    /// Terminal stop (Ctrl+Z)
    SIGTSTP = 20,
    /// Terminal okuma (arka plan, terminalden okuma engeli)
    SIGTTIN = 21,
    /// Terminal yazma (arka plan, terminale yazma engeli)
    SIGTTOU = 22,
    /// Acil durum (out-of-band veri)
    SIGURG = 23,
    /// CPU limiti aşıldı
    SIGXCPU = 24,
    /// Dosya boyutu limiti aşıldı
    SIGXFSZ = 25,
    /// Sanal zamanlayıcı süre aşımı
    SIGVTALRM = 26,
    /// Profilleme zamanlayıcısı süre aşımı
    SIGPROF = 27,
    /// Pencere boyutu değişikliği
    SIGWINCH = 28,
    /// Asenkron G/Ç (I/O) mümkün
    SIGIO = 29,
    /// Güç arızası
    SIGPWR = 30,
    /// Hatalı sistem çağrısı
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

/// Sinyal eylemi (Signal action)
#[derive(Clone, Copy, Debug)]
pub enum SignalAction {
    /// Varsayılan eylem (SIG_DFL)
    Default,
    /// Sinyali yoksay (SIG_IGN)
    Ignore,
    /// Kullanıcı alanı işleyicisi (handler) ile yakala — adres belirtilir
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
    pub sa_handler: usize,  // Fonksiyon işaretçisi veya SIG_IGN/SIG_DFL
    pub sa_mask: u64,
    pub sa_flags: u32,
    pub sa_restorer: usize,
}

// Özel işleyici değerleri
pub const SIG_DFL: usize = 0;  // Varsayılan eylem
pub const SIG_IGN: usize = 1;  // Sinyali yoksay

// sigaction bayrakları (sa_flags)
pub const SA_NOCLDSTOP: u32 = 0x00000001;  // Alt süreç durunca SIGCHLD gönderme
pub const SA_NOCLDWAIT: u32 = 0x00000002;  // Alt süreç ölünce zombi oluşturma
pub const SA_SIGINFO: u32   = 0x00000004;  // sa_sigaction işleyicisini kullan
pub const SA_RESTART: u32   = 0x10000000;  // Sinyal sonrası sistem çağrısını yeniden başlat
pub const SA_NODEFER: u32   = 0x40000000;  // İşleyici çalışırken sinyali bloklamaz
pub const SA_RESETHAND: u32 = 0x80000000;  // İşleyici sonrası SIG_DFL'ye döner
pub const SA_ONSTACK: u32   = 0x08000000;  // Değişken sinyal yığınını kullan

// sigprocmask yöntemi değerleri
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
// SİSTEM ÇAĞRISI ARAYÜZÜ
// ============================================================================

/// sigaction(2) sistem çağrısı uygulaması.
///
/// # Parametreler
/// - `signum`: Sinyal numarası (1-31)
/// - `act`: Yeni sinyal eylemi (isteğe bağlı)
/// - `oldact`: Eski eylemi saklamak için tampon (isteğe bağlı)
///
/// # Dönen Değer
/// Başarıda 0, hata durumunda negatif errno
pub fn sys_sigaction(
    handlers: &mut SignalHandlers,
    signum: i32,
    act: Option<SigAction>,
    oldact: Option<&mut SigAction>,
) -> i32 {
    // Sinyal numarası doğrulama
    if signum < 1 || signum > 31 {
        return -22; // EINVAL
    }

    let sig = match Signal::from_number(signum as u8) {
        Some(s) => s,
        None => return -22, // EINVAL
    };

    // SIGKILL ve SIGSTOP yakalanamaz
    if !sig.is_catchable() && act.is_some() {
        return -22; // EINVAL
    }

    // İsteniyorsa eski eylemi sakla
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
    
    // Yeni eylem varsa uygula
    if let Some(new_act) = act {
        let action = if new_act.sa_handler == SIG_DFL {
            SignalAction::Default
        } else if new_act.sa_handler == SIG_IGN {
            SignalAction::Ignore
        } else {
            SignalAction::Catch(new_act.sa_handler)
        };

        handlers.set_action(sig, action);

        // sa_mask'tan sinyal maskesini uygula
        if new_act.sa_flags & SA_NODEFER == 0 {
            // İşleyici çalışırken sinyali maske içine ekle
            let mut mask = new_act.sa_mask;
            if new_act.sa_flags & SA_RESETHAND != 0 {
                // İşleyici sonrası varsayılana döner
            }
            handlers.set_mask(mask);
        }
        
        crate::serial_println!("[SIGNAL] sigaction({}) -> {:?}", sig.name(), action);
    }
    
    0 // Success
}

/// sigprocmask(2) sistem çağrısı uygulaması.
///
/// # Parametreler
/// - `how`: SIG_BLOCK, SIG_UNBLOCK veya SIG_SETMASK
/// - `set`: Yeni sinyal kümesi (isteğe bağlı)
/// - `oldset`: Eski maskeyi saklamak için tampon (isteğe bağlı)
///
/// # Dönen Değer
/// Başarıda 0, hata durumunda negatif errno
pub fn sys_sigprocmask(
    handlers: &SignalHandlers,
    how: i32,
    set: Option<u64>,
    oldset: Option<&mut u64>,
) -> i32 {
    // İsteniyorsa eski maskeyi sakla
    if let Some(old) = oldset {
        *old = handlers.get_mask();
    }

    // Yeni maske varsa uygula
    if let Some(new_mask) = set {
        match how {
            SIG_BLOCK => {
                // Sinyalleri maskeye ekle
                let current = handlers.get_mask();
                handlers.set_mask(current | new_mask);
            }
            SIG_UNBLOCK => {
                // Sinyalleri maskeden çıkar
                let current = handlers.get_mask();
                handlers.set_mask(current & !new_mask);
            }
            SIG_SETMASK => {
                // Maskeyi tamamen değiştir
                handlers.set_mask(new_mask);
            }
            _ => return -22, // EINVAL
        }
        
        crate::serial_println!("[SIGNAL] sigprocmask(how={}, mask={:#x})", how, new_mask);
    }
    
    0 // Success
}

/// sigpending(2) sistem çağrısı uygulaması.
///
/// Maskelenmiş bekleyen sinyallerin kümesini döndürür.
pub fn sys_sigpending(handlers: &SignalHandlers) -> u64 {
    handlers.get_pending()
}

/// sigsuspend(2) sistem çağrısı uygulaması.
///
/// Sinyal maskesini atomik olarak değiştirir ve bir sinyal yakalanana
/// kadar process'i askıya alır.
pub fn sys_sigsuspend(handlers: &SignalHandlers, mask: u64) -> i32 {
    let old_mask = handlers.get_mask();
    handlers.set_mask(mask);

    // TODO: Process'i gerçekten askıya al
    // Zamanlayıcı entegrasyonu gerektirir

    // Geri dönerken eski maskeyi geri yükle
    handlers.set_mask(old_mask);

    -4 // EINTR (sinyal tarafından kesildi)
}

/// kill(2) sistem çağrısı uygulaması.
///
/// Bir process veya process grubuna sinyal gönderir.
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
        None // Sinyal 0 = process varlık kontrolü
    };
    
    if pid > 0 {
        // Belirli bir process'e gönder
        if let Some(signal) = sig {
            match send_signal(pid as usize, signal) {
                Ok(()) => 0,
                Err(SignalError::ProcessNotFound) => -3, // ESRCH
                Err(SignalError::PermissionDenied) => -1, // EPERM
                Err(SignalError::InvalidSignal) => -22, // EINVAL
            }
        } else {
            // Sinyal 0: sadece process'in var olup olmadığını kontrol et
            // TODO: Process varlığını kontrol et
            0
        }
    } else if pid == 0 {
        // Mevcut process grubuna gönder
        if let Some(signal) = sig {
            // TODO: Mevcut process grubunu al
            0
        } else {
            0
        }
    } else if pid == -1 {
        // Tüm process'lere gönder (init hariç)
        if let Some(signal) = sig {
            match send_signal_all(signal) {
                Ok(()) => 0,
                Err(_) => -1,
            }
        } else {
            0
        }
    } else {
        // pid < -1: -pid'li process grubuna gönder
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

/// raise(2) sistem çağrısı uygulaması.
///
/// Mevcut process'e sinyal gönderir.
pub fn sys_raise(signum: i32) -> i32 {
    let pid = crate::task::scheduler::current_task_id() as i32;
    sys_kill(pid, signum)
}

/// pause(2) sistem çağrısı uygulaması.
///
/// Bir sinyal yakalanana kadar process'i askıya alır.
pub fn sys_pause() -> i32 {
    // TODO: Gerçekten askıya almak için zamanlayıcı entegrasyonu gerektirir
    -4 // EINTR
}

/// alarm(2) sistem çağrısı uygulaması.
///
/// Belirtilen saniye sonra SIGALRM gönderilmesini zamanlar.
pub fn sys_alarm(seconds: u32) -> u32 {
    // TODO: Zamanlayıcı tabanlı alarm uygulaması
    // Şimdilik 0 döndür (önceki alarm yok)
    0
}

/// sigaltstack(2) sistem çağrısı uygulaması.
///
/// Değişken sinyal yığınını ayarlar/alır.
pub fn sys_sigaltstack(ss: Option<usize>, old_ss: Option<&mut usize>) -> i32 {
    // TODO: Değişken sinyal yığınını uygula
    0
}

// ============================================================================
// SİNYAL TESLİMİ
// ============================================================================

/// Bekleyen sinyalleri process'e teslim eder.
///
/// Kullanıcı alanına dönmeden önce çağrılmalıdır.
///
/// # Dönen Değer
/// - `Some((handler_addr, siginfo))`: Çağrılacak sinyal işleyicisi
/// - `None`: Teslim edilecek sinyal yok
pub fn deliver_signals(handlers: &mut SignalHandlers) -> Option<(usize, SigInfo)> {
    // Maskelenmemiş bir sonraki bekleyen sinyali al
    let sig = handlers.next_pending()?;

    // Bekleyen durumu temizle
    handlers.clear_pending(sig);

    // İşleyiciyi al
    let action = handlers.get_action(sig).clone();

    match action {
        SignalAction::Default => {
            // Varsayılan eylemi gerçekleştir
            let disposition = default_action(sig);
            match disposition {
                SignalDisposition::Ignore => {
                    // Hiçbir şey yapma
                    return None;
                }
                SignalDisposition::Terminate => {
                    // Process'i sonlandır
                    crate::serial_println!("[SIGNAL] Varsayılan eylem: {} sebebiyle sonlandırıldı", sig.name());
                    // TODO: Process'i gerçekten sonlandır
                    return None;
                }
                SignalDisposition::Stop => {
                    // Process'i durdur
                    crate::serial_println!("[SIGNAL] Varsayılan eylem: {} sebebiyle durduruldu", sig.name());
                    // TODO: Process'i durdur
                    return None;
                }
                SignalDisposition::Continue => {
                    // Durmuşsa devam et
                    return None;
                }
                SignalDisposition::CoreDump => {
                    // Bellek dökümü alarak sonlandır
                    crate::serial_println!("[SIGNAL] Varsayılan eylem: {} sebebiyle bellek dökümü", sig.name());
                    // TODO: Core dump
                    return None;
                }
            }
        }
        SignalAction::Ignore => {
            return None;
        }
        SignalAction::Catch(handler_addr) => {
            // siginfo yapısını oluştur
            let siginfo = SigInfo {
                si_signo: sig.number() as i32,
                si_errno: 0,
                si_code: 0, // SI_USER
                si_pid: 0,
                si_uid: 0,
                si_status: 0,
                si_value: 0,
            };

            // İşleyici çalışırken sinyali maskele (SA_NODEFER yoksa)
            handlers.block(sig);

            crate::serial_println!("[SIGNAL] {} sinyali {:#x} işleyicisine teslim ediliyor", sig.name(), handler_addr);

            return Some((handler_addr, siginfo));
        }
    }

    None
}

/// Process'in bekleyen sinyali olup olmadığını kontrol eder.
pub fn has_pending_signals(handlers: &SignalHandlers) -> bool {
    let pending = handlers.get_pending();
    let mask = handlers.get_mask();
    (pending & !mask) != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    // ... (kodun geri kalanı aynı kalır)
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