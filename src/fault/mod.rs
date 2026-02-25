//! # echOS Fault Management Subsystem
//!
//! Comprehensive fault detection, isolation, and recovery system.
//! Provides anti-crash protection through health monitoring and graceful degradation.

pub mod hub;
pub mod severity;
pub mod recovery;
pub mod watchdog;
pub mod checkpoint;
pub mod degradation;
pub mod emergency;
pub mod injection;

pub mod monitors;
pub mod recovery_modules;

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, AtomicBool, Ordering};
use spin::Mutex;

// Re-export main types
pub use hub::FaultHub;
pub use severity::{Severity, RecoveryResult};
pub use recovery::{RecoveryAction, RecoveryEngine};

// ============================================================================
// FAULT TYPES
// ============================================================================

/// Unique fault identifier
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FaultId(pub u64);

/// Source module that generated the fault
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FaultSource {
    Memory,
    Cpu,
    Smp,
    Interrupt,
    Scheduler,
    Driver,
    Filesystem,
    Network,
    Security,
    Acpi,
    Boot,
    Unknown,
}

/// Specific fault types
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultType {
    // Memory faults
    HeapCorruption,
    DoubleFree,
    UseAfterFree,
    NullPointer,
    InvalidPointer,
    OutOfMemory,
    PageFault,
    PmmCorruption,
    
    // CPU/SMP faults
    ApStartupFailed,
    TlbShootdownTimeout,
    CpuHung,
    MicrocodeError,
    
    // Interrupt faults
    IdtCorruption,
    IrqStorm,
    HandlerTimeout,
    SpuriousInterrupt,
    
    // Scheduler faults
    RunQueueCorruption,
    TaskLeak,
    PriorityInversion,
    Starvation,
    
    // Driver faults
    DmaCorruption,
    DeviceTimeout,
    DeviceError,
    DriverCrash,
    
    // Filesystem faults
    MetadataCorruption,
    JournalError,
    IoError,
    DiskFull,
    
    // Network faults
    ConnectionReset,
    StackCorruption,
    SocketLeak,
    
    // Security faults
    CanaryMismatch,
    SmepViolation,
    SmapViolation,
    
    // ACPI faults
    AmlError,
    GpeStorm,
    ThermalEvent,
    
    // Boot faults
    BootTimeout,
    InitFailed,
    
    // Generic
    Unknown,
}

/// A detected fault
#[derive(Clone, Debug)]
pub struct Fault {
    /// Unique fault ID
    pub id: FaultId,
    /// Source module
    pub source: FaultSource,
    /// Fault type
    pub fault_type: FaultType,
    /// Severity level
    pub severity: Severity,
    /// Human-readable message
    pub message: String,
    /// Timestamp (ticks)
    pub timestamp: usize,
    /// CPU where fault occurred
    pub cpu_id: u32,
    /// Additional context
    pub context: Vec<u64>,
    /// Recovery attempted
    pub recovery_attempted: bool,
    /// Recovery successful
    pub recovery_success: bool,
}

impl Fault {
    pub fn new(source: FaultSource, fault_type: FaultType, message: &str) -> Self {
        static FAULT_COUNTER: AtomicU64 = AtomicU64::new(0);
        
        Self {
            id: FaultId(FAULT_COUNTER.fetch_add(1, Ordering::SeqCst)),
            source,
            fault_type,
            severity: Severity::from_type(&fault_type),
            message: String::from(message),
            timestamp: crate::task::scheduler::get_ticks(),
            cpu_id: crate::cpu::smp::get_current_cpu_id(),
            context: Vec::new(),
            recovery_attempted: false,
            recovery_success: false,
        }
    }
    
    pub fn with_context(mut self, context: Vec<u64>) -> Self {
        self.context = context;
        self
    }
}

// ============================================================================
// MODULE HEALTH STATUS
// ============================================================================

/// Health status of a module
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthStatus {
    /// Module is functioning normally
    Healthy,
    /// Module has minor issues but is operational
    Warning,
    /// Module is operating with reduced functionality
    Degraded,
    /// Module has failed and is disabled
    Failed,
    /// Module is intentionally disabled
    Disabled,
}

impl Default for HealthStatus {
    fn default() -> Self {
        HealthStatus::Healthy
    }
}

/// Health information for a module
#[derive(Clone, Debug)]
pub struct ModuleHealth {
    /// Module name
    pub name: &'static str,
    /// Current health status
    pub status: HealthStatus,
    /// Number of faults detected
    pub fault_count: u32,
    /// Number of successful recoveries
    pub recovery_count: u32,
    /// Last fault timestamp
    pub last_fault_tick: usize,
    /// Module uptime in ticks
    pub uptime_ticks: usize,
    /// Is module critical for system operation
    pub is_critical: bool,
    /// Can module be restarted
    pub can_restart: bool,
    /// Fallback module available
    pub has_fallback: bool,
}

impl ModuleHealth {
    pub const fn new(name: &'static str, is_critical: bool, can_restart: bool, has_fallback: bool) -> Self {
        Self {
            name,
            status: HealthStatus::Healthy,
            fault_count: 0,
            recovery_count: 0,
            last_fault_tick: 0,
            uptime_ticks: 0,
            is_critical,
            can_restart,
            has_fallback,
        }
    }
    
    pub fn record_fault(&mut self) {
        self.fault_count += 1;
        self.last_fault_tick = crate::task::scheduler::get_ticks();
    }
    
    pub fn record_recovery(&mut self, success: bool) {
        if success {
            self.recovery_count += 1;
        }
    }
    
    pub fn update_status(&mut self, status: HealthStatus) {
        self.status = status;
    }
}

// ============================================================================
// GLOBAL FAULT STATE
// ============================================================================

/// Global fault management state
pub struct FaultState {
    /// Total faults detected
    pub total_faults: AtomicU64,
    /// Total successful recoveries
    pub total_recoveries: AtomicU64,
    /// Current system recovery level (0-4)
    pub recovery_level: AtomicU32,
    /// System in emergency mode
    pub emergency_mode: AtomicBool,
    /// Fault detection enabled
    pub detection_enabled: AtomicBool,
    /// Auto-recovery enabled
    pub auto_recovery: AtomicBool,
    /// Last fault timestamp
    pub last_fault_tick: AtomicUsize,
    /// Fault history
    pub fault_history: Mutex<Vec<Fault>>,
}

impl FaultState {
    pub const fn new() -> Self {
        Self {
            total_faults: AtomicU64::new(0),
            total_recoveries: AtomicU64::new(0),
            recovery_level: AtomicU32::new(0),
            emergency_mode: AtomicBool::new(false),
            detection_enabled: AtomicBool::new(true),
            auto_recovery: AtomicBool::new(true),
            last_fault_tick: AtomicUsize::new(0),
            fault_history: Mutex::new(Vec::new()),
        }
    }
    
    pub fn record_fault(&self, fault: &Fault) {
        self.total_faults.fetch_add(1, Ordering::SeqCst);
        self.last_fault_tick.store(fault.timestamp, Ordering::SeqCst);
        
        // Add to history (limit to 100 entries)
        let mut history = self.fault_history.lock();
        if history.len() >= 100 {
            history.remove(0);
        }
        history.push(fault.clone());
    }
    
    pub fn record_recovery(&self) {
        self.total_recoveries.fetch_add(1, Ordering::SeqCst);
    }
    
    pub fn get_fault_rate(&self, window_ticks: u64) -> f64 {
        let current = crate::task::scheduler::get_ticks();
        let history = self.fault_history.lock();
        
        let count = history.iter()
            .filter(|f| current.saturating_sub(f.timestamp as usize) <= window_ticks as usize)
            .count();
        
        count as f64 / (window_ticks as f64 / 1000.0)
    }
}

lazy_static::lazy_static! {
    pub static ref FAULT_STATE: FaultState = FaultState::new();
}

// ============================================================================
// INITIALIZATION
// ============================================================================

/// Initialize fault management subsystem
pub fn init() {
    crate::serial_println!("[FAULT] Initializing fault management subsystem");
    
    // Initialize fault hub
    hub::init();
    
    // Initialize watchdog system
    watchdog::init();
    
    // Initialize recovery engine
    recovery::init();
    
    // Initialize monitors
    monitors::init();
    
    crate::serial_println!("[FAULT] Fault management subsystem initialized");
}

/// Perform periodic fault check
pub fn periodic_check() {
    if !FAULT_STATE.detection_enabled.load(Ordering::SeqCst) {
        return;
    }
    
    // Check all monitors
    monitors::check_all();
    
    // Check watchdogs
    watchdog::check_all();
    
    // Update recovery level
    update_recovery_level();
}

fn update_recovery_level() {
    let history = FAULT_STATE.fault_history.lock();
    let current = crate::task::scheduler::get_ticks();
    
    // Count faults in last 10 seconds
    let recent_faults = history.iter()
        .filter(|f| current.saturating_sub(f.timestamp) <= 10000)
        .count();
    
    // Count critical faults
    let critical_faults = history.iter()
        .filter(|f| f.severity == Severity::Critical || f.severity == Severity::Emergency)
        .count();
    
    // Determine recovery level
    let level = if critical_faults > 0 {
        4 // Emergency
    } else if recent_faults > 10 {
        3 // Critical
    } else if recent_faults > 5 {
        2 // Degraded
    } else if recent_faults > 0 {
        1 // Warning
    } else {
        0 // Normal
    };
    
    FAULT_STATE.recovery_level.store(level, Ordering::SeqCst);
    
    if level >= 4 {
        FAULT_STATE.emergency_mode.store(true, Ordering::SeqCst);
        emergency::enter();
    }
}

/// Report a fault
pub fn report_fault(source: FaultSource, fault_type: FaultType, message: &str) -> FaultId {
    let fault = Fault::new(source, fault_type, message);
    FAULT_STATE.record_fault(&fault);
    
    crate::serial_println!(
        "[FAULT] {:?} fault from {:?}: {}",
        fault.severity, fault.source, fault.message
    );
    
    // Attempt automatic recovery if enabled
    if FAULT_STATE.auto_recovery.load(Ordering::SeqCst) {
        recovery::attempt_recovery(&fault);
    }
    
    fault.id
}

/// Get fault statistics
pub fn get_stats() -> FaultStats {
    FaultStats {
        total_faults: FAULT_STATE.total_faults.load(Ordering::SeqCst),
        total_recoveries: FAULT_STATE.total_recoveries.load(Ordering::SeqCst),
        recovery_level: FAULT_STATE.recovery_level.load(Ordering::SeqCst),
        emergency_mode: FAULT_STATE.emergency_mode.load(Ordering::SeqCst),
        recent_fault_count: FAULT_STATE.fault_history.lock().len(),
    }
}

#[derive(Clone, Debug)]
pub struct FaultStats {
    pub total_faults: u64,
    pub total_recoveries: u64,
    pub recovery_level: u32,
    pub emergency_mode: bool,
    pub recent_fault_count: usize,
}
