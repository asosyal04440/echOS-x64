//! # Ftrace — Function Tracing Altyapısı
//!
//! Linux ftrace ile uyumlu fonksiyon giriş/çıkış izleme sistemi.
//! Ring buffer tabanlı, düşük overhead trace kaydı.
//!
//! ## Mimari
//!
//! ```text
//! ┌─────────────┐  mcount/fentry  ┌──────────────┐  read  ┌───────────┐
//! │ Kernel Code │────────────────►│ Ftrace Ring  │───────►│ /sys/     │
//! │ fn foo() {  │                 │ Buffer       │        │ kernel/   │
//! │   __fentry()│                 │ (per-CPU)    │        │ tracing/  │
//! │   ...       │                 └──────────────┘        └───────────┘
//! └─────────────┘
//! ```
//!
//! ## Desteklenen Tracer'lar
//!
//! - **function** — fonksiyon giriş noktaları
//! - **function_graph** — fonksiyon giriş + çıkış (call graph)
//! - **irqsoff** — interrupt disable süreleri
//! - **nop** — tracer kapalı
//!
//! ## Kullanım
//!
//! ```text
//! echo function > /sys/kernel/tracing/current_tracer
//! echo 1 > /sys/kernel/tracing/tracing_on
//! cat /sys/kernel/tracing/trace
//! echo 0 > /sys/kernel/tracing/tracing_on
//! ```

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

// ============================================================================
// Trace Event Types
// ============================================================================

/// Trace event türleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceEventType {
    /// Fonksiyon girişi
    FunctionEntry,
    /// Fonksiyon çıkışı
    FunctionReturn,
    /// IRQ handler girişi
    IrqEntry,
    /// IRQ handler çıkışı
    IrqExit,
    /// Softirq handler
    SoftirqEntry,
    SoftirqExit,
    /// Context switch
    SchedSwitch,
    /// Wakeup event
    SchedWakeup,
    /// Syscall entry
    SyscallEntry,
    /// Syscall exit
    SyscallExit,
    /// Custom event (tracepoint)
    Tracepoint,
    /// Timer event
    TimerExpiry,
}

/// Aktif tracer türleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TracerType {
    /// Tracer kapalı
    Nop,
    /// Fonksiyon giriş noktaları
    Function,
    /// Fonksiyon giriş + çıkış (call graph)
    FunctionGraph,
    /// IRQ kapalı süreleri
    IrqsOff,
    /// Preemption kapalı süreleri
    PreemptOff,
    /// IRQ + preemption kapalı süreleri
    PreemptIrqsOff,
    /// Wakeup latency
    Wakeup,
}

impl TracerType {
    pub fn from_str(s: &str) -> Self {
        match s {
            "function" => Self::Function,
            "function_graph" => Self::FunctionGraph,
            "irqsoff" => Self::IrqsOff,
            "preemptoff" => Self::PreemptOff,
            "preemptirqsoff" => Self::PreemptIrqsOff,
            "wakeup" => Self::Wakeup,
            _ => Self::Nop,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Nop => "nop",
            Self::Function => "function",
            Self::FunctionGraph => "function_graph",
            Self::IrqsOff => "irqsoff",
            Self::PreemptOff => "preemptoff",
            Self::PreemptIrqsOff => "preemptirqsoff",
            Self::Wakeup => "wakeup",
        }
    }
}

// ============================================================================
// Trace Entry (Ring Buffer Record)
// ============================================================================

/// Per-CPU ring buffer'daki tek bir trace kaydı
#[derive(Clone, Debug)]
pub struct TraceEntry {
    /// Zaman damgası (TSC ticks)
    pub timestamp: u64,
    /// Event tipi
    pub event_type: TraceEventType,
    /// CPU numarası
    pub cpu: u32,
    /// PID (çalışan task)
    pub pid: u32,
    /// Fonksiyon adresi (caller)
    pub func_addr: u64,
    /// Parent fonksiyon adresi (çağıran)
    pub parent_addr: u64,
    /// Ek veri (IRQ numarası, syscall no, vs.)
    pub data: u64,
    /// Fonksiyon ismi (debug build'de)
    pub func_name: Option<String>,
    /// Latency (nanoseconds, function_graph return için)
    pub duration_ns: u64,
    /// Call depth (function_graph indentation)
    pub depth: u32,
}

impl TraceEntry {
    pub fn new(event_type: TraceEventType, func_addr: u64, parent_addr: u64) -> Self {
        Self {
            timestamp: read_tsc(),
            event_type,
            cpu: 0, // TODO: per-CPU
            pid: 0,
            func_addr,
            parent_addr,
            data: 0,
            func_name: None,
            duration_ns: 0,
            depth: 0,
        }
    }
}

// ============================================================================
// Per-CPU Ring Buffer
// ============================================================================

/// Ring buffer kapasitesi (entry sayısı)
const RING_BUFFER_SIZE: usize = 8192;

/// Per-CPU trace ring buffer
pub struct TraceRingBuffer {
    /// Sabit boyutlu ring buffer
    entries: Vec<TraceEntry>,
    /// Yazma indeksi
    write_idx: usize,
    /// Okuma indeksi
    read_idx: usize,
    /// Buffer doldu mu? (overwrite mode)
    overflowed: bool,
    /// Toplam yazılan kayıt
    total_written: u64,
    /// Düşürülen (overwritten) kayıt sayısı
    dropped: u64,
}

impl TraceRingBuffer {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            write_idx: 0,
            read_idx: 0,
            overflowed: false,
            total_written: 0,
            dropped: 0,
        }
    }

    /// Yeni entry yazar
    pub fn push(&mut self, entry: TraceEntry) {
        if self.entries.len() < RING_BUFFER_SIZE {
            self.entries.push(entry);
        } else {
            self.entries[self.write_idx] = entry;
            if self.overflowed {
                self.dropped += 1;
            }
        }

        self.write_idx = (self.write_idx + 1) % RING_BUFFER_SIZE;
        self.total_written += 1;

        if self.write_idx == self.read_idx && self.entries.len() >= RING_BUFFER_SIZE {
            self.overflowed = true;
            self.read_idx = (self.read_idx + 1) % RING_BUFFER_SIZE;
        }
    }

    /// Tüm entry'leri sıralı okur (eski → yeni)
    pub fn read_all(&self) -> Vec<&TraceEntry> {
        let mut result = Vec::new();

        if self.entries.is_empty() {
            return result;
        }

        if !self.overflowed {
            // Buffer dolu değil — 0'dan write_idx'e kadar
            for entry in &self.entries[..self.write_idx] {
                result.push(entry);
            }
        } else {
            // Buffer dolmuş — read_idx'ten başla, wrap around
            let len = self.entries.len();
            for i in 0..len {
                let idx = (self.read_idx + i) % len;
                result.push(&self.entries[idx]);
            }
        }

        result
    }

    /// Buffer'ı temizler
    pub fn clear(&mut self) {
        self.entries.clear();
        self.write_idx = 0;
        self.read_idx = 0;
        self.overflowed = false;
    }

    /// Kayıt sayısı
    pub fn len(&self) -> usize {
        if !self.overflowed {
            self.write_idx
        } else {
            self.entries.len()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ============================================================================
// Ftrace Filter
// ============================================================================

/// Fonksiyon filtresi — hangi fonksiyonlar trace edilecek?
#[derive(Clone, Debug)]
pub struct FtraceFilter {
    /// Sadece bu fonksiyon adreslerini trace et (boş = hepsi)
    pub include_addrs: Vec<u64>,
    /// Bu fonksiyon adreslerini trace ETME
    pub exclude_addrs: Vec<u64>,
    /// PID filtresi (boş = hepsi)
    pub pid_filter: Vec<u32>,
    /// Fonksiyon isim filtresi (glob pattern)
    pub func_filter: Option<String>,
    /// Max trace derinliği (function_graph)
    pub max_depth: u32,
}

impl FtraceFilter {
    pub fn new() -> Self {
        Self {
            include_addrs: Vec::new(),
            exclude_addrs: Vec::new(),
            pid_filter: Vec::new(),
            func_filter: None,
            max_depth: 16,
        }
    }

    /// Bu fonksiyon/PID trace edilmeli mi?
    pub fn should_trace(&self, func_addr: u64, pid: u32) -> bool {
        // PID filtresi
        if !self.pid_filter.is_empty() && !self.pid_filter.contains(&pid) {
            return false;
        }

        // Exclude listesinde mi?
        if self.exclude_addrs.contains(&func_addr) {
            return false;
        }

        // Include listesi varsa, orada mı?
        if !self.include_addrs.is_empty() {
            return self.include_addrs.contains(&func_addr);
        }

        true
    }
}

// ============================================================================
// Ftrace Global State
// ============================================================================

/// Tracing aktif mi?
static TRACING_ON: AtomicBool = AtomicBool::new(false);

/// Toplam trace edilen event sayısı
static TOTAL_EVENTS: AtomicU64 = AtomicU64::new(0);

lazy_static::lazy_static! {
    /// Aktif tracer
    static ref CURRENT_TRACER: Mutex<TracerType> = Mutex::new(TracerType::Nop);

    /// Per-CPU ring buffer'lar (CPU index → buffer)
    static ref TRACE_BUFFERS: Mutex<BTreeMap<u32, TraceRingBuffer>> =
        Mutex::new(BTreeMap::new());

    /// Trace filtresi
    static ref TRACE_FILTER: Mutex<FtraceFilter> = Mutex::new(FtraceFilter::new());

    /// Fonksiyon symbol tablosu (adres → isim)
    static ref SYMBOL_TABLE: Mutex<BTreeMap<u64, String>> = Mutex::new(BTreeMap::new());
}

// ============================================================================
// TSC (Time Stamp Counter) Okuma
// ============================================================================

/// x86_64 RDTSC komutu ile zaman damgası okur
#[inline(always)]
fn read_tsc() -> u64 {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::x86_64::_rdtsc()
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        0
    }
}

// ============================================================================
// Public API
// ============================================================================

/// Ftrace modülünü başlatır
pub fn init() {
    // CPU 0 için ring buffer oluştur
    let mut buffers = TRACE_BUFFERS.lock();
    buffers.insert(0, TraceRingBuffer::new());

    crate::serial_println!(
        "[Ftrace] Function tracing module initialized (ring_size={})",
        RING_BUFFER_SIZE
    );
}

/// Tracing'i açar
pub fn enable() {
    TRACING_ON.store(true, Ordering::Release);
    crate::serial_println!(
        "[Ftrace] Tracing enabled (tracer={})",
        current_tracer().as_str()
    );
}

/// Tracing'i kapatır
pub fn disable() {
    TRACING_ON.store(false, Ordering::Release);
    crate::serial_println!("[Ftrace] Tracing disabled");
}

/// Tracing aktif mi?
pub fn is_enabled() -> bool {
    TRACING_ON.load(Ordering::Acquire)
}

/// Aktif tracer'ı değiştirir
pub fn set_tracer(tracer: TracerType) {
    *CURRENT_TRACER.lock() = tracer;
    crate::serial_println!("[Ftrace] Tracer set to: {}", tracer.as_str());
}

/// Aktif tracer'ı döndürür
pub fn current_tracer() -> TracerType {
    *CURRENT_TRACER.lock()
}

/// Fonksiyon girişi kaydeder (mcount/__fentry__ ile çağrılır)
///
/// Bu fonksiyon her instrumented fonksiyonun başında çağrılır.
#[inline(always)]
pub fn trace_function_entry(func_addr: u64, parent_addr: u64) {
    if !TRACING_ON.load(Ordering::Relaxed) {
        return;
    }

    let tracer = *CURRENT_TRACER.lock();
    if tracer != TracerType::Function && tracer != TracerType::FunctionGraph {
        return;
    }

    let filter = TRACE_FILTER.lock();
    if !filter.should_trace(func_addr, 0) {
        return;
    }
    drop(filter);

    let mut entry = TraceEntry::new(TraceEventType::FunctionEntry, func_addr, parent_addr);

    // Symbol tablosundan isim bul
    let symbols = SYMBOL_TABLE.lock();
    if let Some(name) = symbols.get(&func_addr) {
        entry.func_name = Some(name.clone());
    }
    drop(symbols);

    let mut buffers = TRACE_BUFFERS.lock();
    let buffer = buffers.entry(0).or_insert_with(TraceRingBuffer::new);
    buffer.push(entry);

    TOTAL_EVENTS.fetch_add(1, Ordering::Relaxed);
}

/// Fonksiyon çıkışı kaydeder (function_graph tracer)
pub fn trace_function_return(func_addr: u64, duration_ns: u64) {
    if !TRACING_ON.load(Ordering::Relaxed) {
        return;
    }

    if *CURRENT_TRACER.lock() != TracerType::FunctionGraph {
        return;
    }

    let mut entry = TraceEntry::new(TraceEventType::FunctionReturn, func_addr, 0);
    entry.duration_ns = duration_ns;

    let mut buffers = TRACE_BUFFERS.lock();
    let buffer = buffers.entry(0).or_insert_with(TraceRingBuffer::new);
    buffer.push(entry);
}

/// IRQ entry kaydeder
pub fn trace_irq_entry(irq_num: u64) {
    if !TRACING_ON.load(Ordering::Relaxed) {
        return;
    }

    let mut entry = TraceEntry::new(TraceEventType::IrqEntry, 0, 0);
    entry.data = irq_num;

    let mut buffers = TRACE_BUFFERS.lock();
    let buffer = buffers.entry(0).or_insert_with(TraceRingBuffer::new);
    buffer.push(entry);
}

/// IRQ exit kaydeder
pub fn trace_irq_exit(irq_num: u64) {
    if !TRACING_ON.load(Ordering::Relaxed) {
        return;
    }

    let mut entry = TraceEntry::new(TraceEventType::IrqExit, 0, 0);
    entry.data = irq_num;

    let mut buffers = TRACE_BUFFERS.lock();
    let buffer = buffers.entry(0).or_insert_with(TraceRingBuffer::new);
    buffer.push(entry);
}

/// Syscall entry kaydeder
pub fn trace_syscall_entry(syscall_nr: u64, pid: u32) {
    if !TRACING_ON.load(Ordering::Relaxed) {
        return;
    }

    let mut entry = TraceEntry::new(TraceEventType::SyscallEntry, 0, 0);
    entry.data = syscall_nr;
    entry.pid = pid;

    let mut buffers = TRACE_BUFFERS.lock();
    let buffer = buffers.entry(0).or_insert_with(TraceRingBuffer::new);
    buffer.push(entry);
}

/// Symbol tablosuna fonksiyon ekler
pub fn register_symbol(addr: u64, name: &str) {
    SYMBOL_TABLE.lock().insert(addr, String::from(name));
}

/// Fonksiyon filtresi ayarlar (sadece bu fonksiyonu trace et)
pub fn set_function_filter(func_addr: u64) {
    TRACE_FILTER.lock().include_addrs.push(func_addr);
}

/// PID filtresi ayarlar
pub fn set_pid_filter(pid: u32) {
    TRACE_FILTER.lock().pid_filter.push(pid);
}

/// Tüm filtreleri temizler
pub fn clear_filters() {
    let mut filter = TRACE_FILTER.lock();
    filter.include_addrs.clear();
    filter.exclude_addrs.clear();
    filter.pid_filter.clear();
    filter.func_filter = None;
}

/// Tüm buffer'ları temizler
pub fn clear_trace() {
    let mut buffers = TRACE_BUFFERS.lock();
    for (_, buffer) in buffers.iter_mut() {
        buffer.clear();
    }
    TOTAL_EVENTS.store(0, Ordering::Relaxed);
    crate::serial_println!("[Ftrace] Trace buffer cleared");
}

/// Toplam kayıtlı event sayısı
pub fn total_events() -> u64 {
    TOTAL_EVENTS.load(Ordering::Relaxed)
}

/// Buffer'daki event sayısı
pub fn buffer_size() -> usize {
    let buffers = TRACE_BUFFERS.lock();
    buffers.values().map(|b| b.len()).sum()
}

/// Trace çıktısını yazdırır (cat /sys/kernel/tracing/trace)
pub fn print_trace() {
    let buffers = TRACE_BUFFERS.lock();
    let tracer = *CURRENT_TRACER.lock();

    crate::serial_println!("# tracer: {}", tracer.as_str());
    crate::serial_println!("#");
    crate::serial_println!("#                              _-----=> irqs-off");
    crate::serial_println!("#                             / _----=> need-resched");
    crate::serial_println!("#                            | / _---=> hardirq/softirq");
    crate::serial_println!("#                            || / _--=> preempt-depth");
    crate::serial_println!("#                            ||| /     delay");
    crate::serial_println!("#           TASK-PID   CPU#  ||||    TIMESTAMP  FUNCTION");
    crate::serial_println!("#              | |       |   ||||       |         |");

    for (cpu, buffer) in buffers.iter() {
        for entry in buffer.read_all() {
            let func = entry.func_name.as_deref().unwrap_or("???");

            match entry.event_type {
                TraceEventType::FunctionEntry => {
                    crate::serial_println!(
                        "  {:>16}-{:<5} [{}] .... {:>12}: {} <-{:#x}",
                        "task",
                        entry.pid,
                        cpu,
                        entry.timestamp,
                        func,
                        entry.parent_addr
                    );
                }
                TraceEventType::FunctionReturn => {
                    crate::serial_println!(
                        "  {:>16}-{:<5} [{}] .... {:>12}: {} ({}ns)",
                        "task",
                        entry.pid,
                        cpu,
                        entry.timestamp,
                        func,
                        entry.duration_ns
                    );
                }
                TraceEventType::IrqEntry => {
                    crate::serial_println!(
                        "  {:>16}-{:<5} [{}] d... {:>12}: irq={} action=handler",
                        "task",
                        entry.pid,
                        cpu,
                        entry.timestamp,
                        entry.data
                    );
                }
                TraceEventType::SyscallEntry => {
                    crate::serial_println!(
                        "  {:>16}-{:<5} [{}] .... {:>12}: sys_nr={}",
                        "task",
                        entry.pid,
                        cpu,
                        entry.timestamp,
                        entry.data
                    );
                }
                _ => {
                    crate::serial_println!(
                        "  {:>16}-{:<5} [{}] .... {:>12}: {:?}",
                        "task",
                        entry.pid,
                        cpu,
                        entry.timestamp,
                        entry.event_type
                    );
                }
            }
        }
    }
}
