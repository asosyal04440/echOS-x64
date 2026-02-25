//! # echOS Debug Analyzer
//!
//! Sistem izleme ve hata ayıklama yardımcı araçları.
//! Şu an için basit loglama fonksiyonları içerir.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use core::sync::atomic::{AtomicU64, Ordering};
use lazy_static::lazy_static;
use spin::Mutex;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Clone)]
pub struct LogEntry {
    pub seq: u64,
    pub level: LogLevel,
    pub file: &'static str,
    pub line: u32,
    pub module: &'static str,
    pub message: String,
}

struct LogRing {
    entries: Vec<LogEntry>,
    capacity: usize,
    next: usize,
    filled: bool,
}

impl LogRing {
    fn new(capacity: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity),
            capacity,
            next: 0,
            filled: false,
        }
    }

    fn push(&mut self, entry: LogEntry) {
        if self.capacity == 0 {
            return;
        }
        if self.entries.len() < self.capacity {
            self.entries.push(entry);
        } else {
            self.entries[self.next] = entry;
            self.next += 1;
            if self.next >= self.capacity {
                self.next = 0;
                self.filled = true;
            }
        }
    }

    fn snapshot(&self) -> Vec<LogEntry> {
        if !self.filled {
            return self.entries.clone();
        }
        let mut out = Vec::with_capacity(self.capacity);
        let start = self.next;
        for i in 0..self.capacity {
            let idx = (start + i) % self.capacity;
            out.push(self.entries[idx].clone());
        }
        out
    }

    fn resize(&mut self, capacity: usize) {
        let mut snapshot = self.snapshot();
        if capacity == 0 {
            self.entries.clear();
            self.capacity = 0;
            self.next = 0;
            self.filled = false;
            return;
        }
        if snapshot.len() > capacity {
            let skip = snapshot.len() - capacity;
            snapshot = snapshot.split_off(skip);
        }
        self.entries = Vec::with_capacity(capacity);
        self.capacity = capacity;
        self.next = 0;
        self.filled = false;
        for entry in snapshot {
            self.push(entry);
        }
    }
}

lazy_static! {
    static ref LOG_RING: Mutex<LogRing> = Mutex::new(LogRing::new(512));
}

static LOG_SEQ: AtomicU64 = AtomicU64::new(0);

fn level_prefix(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Trace => "TRACE",
        LogLevel::Debug => "DEBUG",
        LogLevel::Info => "INFO",
        LogLevel::Warn => "WARN",
        LogLevel::Error => "ERROR",
    }
}

pub fn log(
    level: LogLevel,
    args: fmt::Arguments,
    file: &'static str,
    line: u32,
    module: &'static str,
) {
    let seq = LOG_SEQ.fetch_add(1, Ordering::Relaxed);
    let message = alloc::format!("{}", args);
    {
        let mut ring = LOG_RING.lock();
        let entry = LogEntry {
            seq,
            level,
            file,
            line,
            module,
            message: message.clone(),
        };
        ring.push(entry);
    }
    let prefix = level_prefix(level);
    crate::serial::uart::_print_with_meta(
        format_args!("[{} {}] {}", prefix, seq, message),
        file,
        line,
        module,
    );
}

pub fn trace(msg: &str) {
    log(
        LogLevel::Trace,
        format_args!("{}", msg),
        "debug::analyzer",
        0,
        "debug::analyzer",
    );
}

pub fn snapshot() -> Vec<LogEntry> {
    LOG_RING.lock().snapshot()
}

pub fn set_capacity(capacity: usize) {
    LOG_RING.lock().resize(capacity);
}

pub fn dump_recent(max: usize) {
    let snapshot = LOG_RING.lock().snapshot();
    if snapshot.is_empty() {
        crate::serial_println!("[LOG] empty");
        return;
    }
    let count = max.min(snapshot.len());
    crate::serial_println!("[LOG] begin count={}", count);
    for entry in snapshot.into_iter().rev().take(count).rev() {
        crate::serial_println!(
            "[LOG] {} {} {}:{} {}",
            entry.seq,
            level_prefix(entry.level),
            entry.module,
            entry.line,
            entry.message
        );
    }
    crate::serial_println!("[LOG] end");
}

pub fn flush_to_path(path: &str) -> bool {
    let snapshot = LOG_RING.lock().snapshot();
    if snapshot.is_empty() {
        return true;
    }
    let inode = match crate::fs::vfs_open_inode(path) {
        Ok(value) => value,
        Err(_) => return false,
    };
    let mut offset = 0usize;
    for entry in snapshot {
        let line = alloc::format!(
            "{} {} {}:{} {}\n",
            entry.seq,
            level_prefix(entry.level),
            entry.module,
            entry.line,
            entry.message
        );
        if crate::fs::vfs_write_at(&inode, offset, line.as_bytes()).is_err() {
            return false;
        }
        offset = offset.saturating_add(line.len());
    }
    true
}

pub struct TraceGuard {
    label: &'static str,
    file: &'static str,
    line: u32,
    module: &'static str,
}

impl TraceGuard {
    pub fn new(label: &'static str, file: &'static str, line: u32, module: &'static str) -> Self {
        log(
            LogLevel::Trace,
            format_args!("TRACE ENTER: {}", label),
            file,
            line,
            module,
        );
        Self {
            label,
            file,
            line,
            module,
        }
    }
}

impl Drop for TraceGuard {
    fn drop(&mut self) {
        log(
            LogLevel::Trace,
            format_args!("TRACE EXIT: {}", self.label),
            self.file,
            self.line,
            self.module,
        );
    }
}

#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {
        $crate::debug::analyzer::log(
            $crate::debug::analyzer::LogLevel::Trace,
            format_args!($($arg)*),
            file!(),
            line!(),
            module_path!(),
        );
    };
}

#[macro_export]
macro_rules! trace_scope {
    ($label:expr) => {
        let _trace_guard =
            $crate::debug::analyzer::TraceGuard::new($label, file!(), line!(), module_path!());
    };
}

#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => {
        $crate::debug::analyzer::log(
            $crate::debug::analyzer::LogLevel::Debug,
            format_args!($($arg)*),
            file!(),
            line!(),
            module_path!(),
        );
    };
}

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        $crate::debug::analyzer::log(
            $crate::debug::analyzer::LogLevel::Info,
            format_args!($($arg)*),
            file!(),
            line!(),
            module_path!(),
        );
    };
}

#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {
        $crate::debug::analyzer::log(
            $crate::debug::analyzer::LogLevel::Warn,
            format_args!($($arg)*),
            file!(),
            line!(),
            module_path!(),
        );
    };
}

#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        $crate::debug::analyzer::log(
            $crate::debug::analyzer::LogLevel::Error,
            format_args!($($arg)*),
            file!(),
            line!(),
            module_path!(),
        );
    };
}
