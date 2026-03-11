//! # strace — Syscall İzleme Alt Sistemi
//!
//! İşlem bazında sistem çağrısı giriş/çıkış izleme.
//! Her syscall'ın numarası, argümanları, dönüş değeri ve süresini kaydeder.
//!
//! ## Kullanım
//!
//! ```text
//! shell> strace <pid>         # PID izlemeye başla
//! shell> strace -c <pid>      # Özet istatistikler
//! shell> strace -e trace=file <pid>  # Yalnızca dosya syscall'ları
//! ```

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// SABITLER
// ============================================================================

/// Maksimum izleme kaydı (ring buffer)
pub const STRACE_RING_SIZE: usize = 4096;
/// Syscall kategorileri
pub const TRACE_FILE: u32 = 0x01;
pub const TRACE_PROCESS: u32 = 0x02;
pub const TRACE_NETWORK: u32 = 0x04;
pub const TRACE_SIGNAL: u32 = 0x08;
pub const TRACE_IPC: u32 = 0x10;
pub const TRACE_MEMORY: u32 = 0x20;
pub const TRACE_DESC: u32 = 0x40; // file descriptor ops
pub const TRACE_ALL: u32 = 0xFF;

// ============================================================================
// Syscall İsim Tablosu
// ============================================================================

/// Syscall numarasından isme eşleme.
pub fn syscall_name(nr: u64) -> &'static str {
    match nr {
        0 => "read",
        1 => "write",
        2 => "open",
        3 => "close",
        4 => "stat",
        5 => "fstat",
        6 => "lstat",
        7 => "poll",
        8 => "lseek",
        9 => "mmap",
        10 => "mprotect",
        11 => "munmap",
        12 => "brk",
        13 => "rt_sigaction",
        14 => "rt_sigprocmask",
        19 => "readv",
        20 => "writev",
        21 => "access",
        22 => "pipe",
        23 => "select",
        24 => "sched_yield",
        35 => "nanosleep",
        39 => "getpid",
        41 => "socket",
        42 => "connect",
        43 => "accept",
        44 => "sendto",
        45 => "recvfrom",
        46 => "sendmsg",
        47 => "recvmsg",
        48 => "shutdown",
        49 => "bind",
        50 => "listen",
        53 => "socketpair",
        56 => "clone",
        57 => "fork",
        58 => "vfork",
        59 => "execve",
        60 => "exit",
        61 => "wait4",
        62 => "kill",
        63 => "uname",
        72 => "fcntl",
        78 => "getdents",
        79 => "getcwd",
        80 => "chdir",
        83 => "mkdir",
        84 => "rmdir",
        85 => "creat",
        87 => "unlink",
        88 => "symlink",
        89 => "readlink",
        90 => "chmod",
        92 => "chown",
        96 => "gettimeofday",
        102 => "getuid",
        104 => "getgid",
        110 => "getppid",
        157 => "prctl",
        186 => "gettid",
        202 => "futex",
        228 => "clock_gettime",
        231 => "exit_group",
        257 => "openat",
        262 => "newfstatat",
        290 => "eventfd2",
        293 => "pipe2",
        302 => "prlimit64",
        318 => "getrandom",
        319 => "memfd_create",
        332 => "statx",
        _ => "unknown",
    }
}

/// Syscall kategori filtresi.
pub fn syscall_category(nr: u64) -> u32 {
    match nr {
        0 | 1 | 8 | 72 | 19 | 20 => TRACE_DESC, // read, write, lseek, fcntl, readv, writev
        2 | 3 | 4 | 5 | 6 | 21 | 78..=90 | 257 | 262 | 332 => TRACE_FILE, // open, close, stat...
        41..=53 => TRACE_NETWORK,               // socket, connect, bind...
        56..=62 | 110 | 186 | 231 => TRACE_PROCESS, // clone, fork, wait4, exit...
        13 | 14 | 62 => TRACE_SIGNAL,           // sigaction, sigprocmask, kill
        202 | 290 | 319 => TRACE_IPC,           // futex, eventfd2, memfd_create
        9..=12 => TRACE_MEMORY,                 // mmap, mprotect, munmap, brk
        _ => TRACE_ALL,
    }
}

// ============================================================================
// Syscall Trace Entry
// ============================================================================

/// Tek bir syscall izleme kaydı.
#[derive(Debug, Clone)]
pub struct StraceEntry {
    /// İşlem ID
    pub pid: u64,
    /// Syscall numarası
    pub syscall_nr: u64,
    /// Argümanlar (en fazla 6)
    pub args: [u64; 6],
    /// Dönüş değeri
    pub ret_val: i64,
    /// Giriş zaman damgası (TSC)
    pub enter_tsc: u64,
    /// Çıkış zaman damgası (TSC)
    pub exit_tsc: u64,
    /// Hata mı?
    pub is_error: bool,
}

impl StraceEntry {
    /// Süre (TSC tick).
    pub fn duration_tsc(&self) -> u64 {
        self.exit_tsc.saturating_sub(self.enter_tsc)
    }

    /// Formatlanmış çıktı.
    pub fn format(&self) -> String {
        let name = syscall_name(self.syscall_nr);
        if self.is_error {
            alloc::format!(
                "[{}] {}({:#x}, {:#x}, {:#x}) = {} (error)",
                self.pid,
                name,
                self.args[0],
                self.args[1],
                self.args[2],
                self.ret_val
            )
        } else {
            alloc::format!(
                "[{}] {}({:#x}, {:#x}, {:#x}) = {}",
                self.pid,
                name,
                self.args[0],
                self.args[1],
                self.args[2],
                self.ret_val
            )
        }
    }
}

// ============================================================================
// Syscall İstatistikleri
// ============================================================================

/// Per-syscall istatistik sayacı.
#[derive(Debug, Clone, Default)]
pub struct SyscallStat {
    /// Çağrı sayısı
    pub count: u64,
    /// Hata sayısı
    pub errors: u64,
    /// Toplam süre (TSC)
    pub total_time: u64,
    /// Minimum süre
    pub min_time: u64,
    /// Maksimum süre
    pub max_time: u64,
}

impl SyscallStat {
    pub fn new() -> Self {
        Self {
            count: 0,
            errors: 0,
            total_time: 0,
            min_time: u64::MAX,
            max_time: 0,
        }
    }

    /// Ortalama süre.
    pub fn avg_time(&self) -> u64 {
        if self.count == 0 {
            0
        } else {
            self.total_time / self.count
        }
    }
}

// ============================================================================
// StraceContext — İşlem Bazında İzleme
// ============================================================================

/// İşlem bazında strace bağlamı.
pub struct StraceContext {
    /// İzlenen PID
    pub pid: u64,
    /// Filtre kategorileri
    pub filter: u32,
    /// İzleme aktif mi
    pub active: bool,
    /// Ring buffer
    pub entries: Vec<StraceEntry>,
    /// Ring buffer yazma indeksi
    pub write_idx: usize,
    /// Per-syscall istatistikleri
    pub stats: BTreeMap<u64, SyscallStat>,
    /// Toplam kayıt sayısı
    pub total_count: u64,
}

impl StraceContext {
    pub fn new(pid: u64, filter: u32) -> Self {
        Self {
            pid,
            filter,
            active: true,
            entries: Vec::with_capacity(STRACE_RING_SIZE),
            write_idx: 0,
            stats: BTreeMap::new(),
            total_count: 0,
        }
    }

    /// Syscall girişini kaydeder.
    pub fn record_entry(&mut self, syscall_nr: u64, args: [u64; 6]) {
        if !self.active {
            return;
        }

        // Filtre kontrolü
        if self.filter != TRACE_ALL {
            let cat = syscall_category(syscall_nr);
            if cat & self.filter == 0 {
                return;
            }
        }

        let tsc = unsafe { core::arch::x86_64::_rdtsc() };

        let entry = StraceEntry {
            pid: self.pid,
            syscall_nr,
            args,
            ret_val: 0,
            enter_tsc: tsc,
            exit_tsc: 0,
            is_error: false,
        };

        if self.entries.len() < STRACE_RING_SIZE {
            self.entries.push(entry);
        } else {
            self.entries[self.write_idx] = entry;
        }
    }

    /// Syscall çıkışını kaydeder.
    pub fn record_exit(&mut self, syscall_nr: u64, ret_val: i64) {
        if !self.active {
            return;
        }

        let tsc = unsafe { core::arch::x86_64::_rdtsc() };

        // Son girişi bul ve güncelle
        if let Some(entry) = self
            .entries
            .iter_mut()
            .rev()
            .find(|e| e.syscall_nr == syscall_nr && e.exit_tsc == 0)
        {
            entry.ret_val = ret_val;
            entry.exit_tsc = tsc;
            entry.is_error = ret_val < 0;

            // İstatistik güncelle
            let stat = self
                .stats
                .entry(syscall_nr)
                .or_insert_with(SyscallStat::new);
            stat.count += 1;
            if ret_val < 0 {
                stat.errors += 1;
            }
            let duration = entry.duration_tsc();
            stat.total_time += duration;
            if duration < stat.min_time {
                stat.min_time = duration;
            }
            if duration > stat.max_time {
                stat.max_time = duration;
            }
        }

        self.write_idx = (self.write_idx + 1) % STRACE_RING_SIZE;
        self.total_count += 1;
    }

    /// Son N kaydı döner.
    pub fn recent(&self, count: usize) -> Vec<&StraceEntry> {
        self.entries
            .iter()
            .filter(|e| e.exit_tsc > 0)
            .rev()
            .take(count)
            .collect()
    }

    /// İstatistik özeti döner.
    pub fn summary(&self) -> Vec<(u64, &str, &SyscallStat)> {
        let mut result: Vec<_> = self
            .stats
            .iter()
            .map(|(nr, stat)| (*nr, syscall_name(*nr), stat))
            .collect();
        result.sort_by(|a, b| b.2.total_time.cmp(&a.2.total_time));
        result
    }
}

// ============================================================================
// Global State
// ============================================================================

lazy_static::lazy_static! {
    /// PID → StraceContext eşlemesi
    static ref STRACE_CONTEXTS: Mutex<BTreeMap<u64, StraceContext>> = Mutex::new(BTreeMap::new());
    /// strace aktif mi (global switch)
    static ref STRACE_ENABLED: AtomicBool = AtomicBool::new(false);
}

/// PID için izleme başlatır.
pub fn attach(pid: u64, filter: u32) {
    let ctx = StraceContext::new(pid, filter);
    STRACE_CONTEXTS.lock().insert(pid, ctx);
    STRACE_ENABLED.store(true, Ordering::Release);
    crate::serial_println!(
        "[strace] PID {} izleme başlatıldı (filtre: {:#x})",
        pid,
        filter
    );
}

/// PID izlemesini durdurur.
pub fn detach(pid: u64) {
    STRACE_CONTEXTS.lock().remove(&pid);
    if STRACE_CONTEXTS.lock().is_empty() {
        STRACE_ENABLED.store(false, Ordering::Release);
    }
}

/// Syscall girişini kaydeder (syscall handler'dan çağrılır).
pub fn on_syscall_enter(pid: u64, syscall_nr: u64, args: [u64; 6]) {
    if !STRACE_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    if let Some(ctx) = STRACE_CONTEXTS.lock().get_mut(&pid) {
        ctx.record_entry(syscall_nr, args);
    }
}

/// Syscall çıkışını kaydeder.
pub fn on_syscall_exit(pid: u64, syscall_nr: u64, ret_val: i64) {
    if !STRACE_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    if let Some(ctx) = STRACE_CONTEXTS.lock().get_mut(&pid) {
        ctx.record_exit(syscall_nr, ret_val);
    }
}

/// İzlenen PID sayısını döner.
pub fn traced_count() -> usize {
    STRACE_CONTEXTS.lock().len()
}

/// Modülü başlatır.
pub fn init() {
    crate::serial_println!("[strace] Syscall izleme alt sistemi hazır");
}
