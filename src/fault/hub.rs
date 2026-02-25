//! # Fault Hub
//!
//! Central fault collection, routing, and management.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::boxed::Box;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

use super::{Fault, FaultId, FaultSource, FaultType, ModuleHealth, HealthStatus, FAULT_STATE};
use super::severity::{Severity, RecoveryResult, RecommendedAction};
use super::recovery::RecoveryEngine;

// ============================================================================
// FAULT HUB STRUCTURE
// ============================================================================

/// Central fault management hub
pub struct FaultHub {
    /// Module health tracking
    modules: Mutex<BTreeMap<&'static str, ModuleHealth>>,
    /// Recovery engine
    recovery_engine: Mutex<Option<RecoveryEngine>>,
    /// Fault handlers per source
    handlers: Mutex<BTreeMap<FaultSource, Box<dyn FaultHandler>>>,
    /// Hub initialized
    initialized: AtomicBool,
    /// Total fault count since boot
    fault_count: AtomicU64,
    /// Recovery success count
    recovery_success: AtomicU64,
    /// Recovery failure count
    recovery_failure: AtomicU64,
    /// Current system severity
    current_severity: AtomicU32,
    /// Last health check timestamp
    last_check: AtomicUsize,
}

/// Trait for custom fault handlers
pub trait FaultHandler: Send + Sync {
    /// Check for faults in the module
    fn check(&self) -> Option<Fault>;
    /// Get module health status
    fn health(&self) -> HealthStatus;
    /// Attempt recovery
    fn recover(&self, fault: &Fault) -> RecoveryResult;
    /// Get module name
    fn name(&self) -> &'static str;
}

impl FaultHub {
    pub const fn new() -> Self {
        Self {
            modules: Mutex::new(BTreeMap::new()),
            recovery_engine: Mutex::new(None),
            handlers: Mutex::new(BTreeMap::new()),
            initialized: AtomicBool::new(false),
            fault_count: AtomicU64::new(0),
            recovery_success: AtomicU64::new(0),
            recovery_failure: AtomicU64::new(0),
            current_severity: AtomicU32::new(Severity::Normal as u32),
            last_check: AtomicUsize::new(0),
        }
    }
    
    /// Initialize the fault hub
    pub fn init(&self) {
        if self.initialized.swap(true, Ordering::SeqCst) {
            return;
        }
        
        // Register core modules
        self.register_module(ModuleHealth::new("memory", true, false, false));
        self.register_module(ModuleHealth::new("cpu", true, false, false));
        self.register_module(ModuleHealth::new("smp", true, false, false));
        self.register_module(ModuleHealth::new("interrupts", true, false, false));
        self.register_module(ModuleHealth::new("scheduler", true, false, false));
        self.register_module(ModuleHealth::new("drivers", false, true, true));
        self.register_module(ModuleHealth::new("filesystem", false, true, true));
        self.register_module(ModuleHealth::new("network", false, true, true));
        self.register_module(ModuleHealth::new("security", true, false, false));
        self.register_module(ModuleHealth::new("acpi", false, true, false));
        
        // Initialize recovery engine
        *self.recovery_engine.lock() = Some(RecoveryEngine::new());
        
        crate::serial_println!("[FAULT_HUB] Initialized with {} modules", 
            self.modules.lock().len());
    }
    
    /// Register a module for health tracking
    pub fn register_module(&self, health: ModuleHealth) {
        self.modules.lock().insert(health.name, health);
    }
    
    /// Register a custom fault handler
    pub fn register_handler(&self, source: FaultSource, handler: Box<dyn FaultHandler>) {
        self.handlers.lock().insert(source, handler);
    }
    
    /// Report a fault
    pub fn report_fault(&self, fault: Fault) -> FaultId {
        let id = fault.id;
        let severity = fault.severity;
        let source = fault.source;
        
        self.fault_count.fetch_add(1, Ordering::SeqCst);
        
        // Update module health
        if let Some(module) = self.modules.lock().get_mut(&match source {
            FaultSource::Memory => "memory",
            FaultSource::Cpu => "cpu",
            FaultSource::Smp => "smp",
            FaultSource::Interrupt => "interrupts",
            FaultSource::Scheduler => "scheduler",
            FaultSource::Driver => "drivers",
            FaultSource::Filesystem => "filesystem",
            FaultSource::Network => "network",
            FaultSource::Security => "security",
            FaultSource::Acpi => "acpi",
            _ => "unknown",
        }) {
            module.record_fault();
            
            // Update status based on fault count
            if module.fault_count > 5 {
                module.update_status(HealthStatus::Failed);
            } else if module.fault_count > 2 {
                module.update_status(HealthStatus::Degraded);
            } else if module.fault_count > 0 {
                module.update_status(HealthStatus::Warning);
            }
        }
        
        // Update global severity
        self.update_severity(severity);
        
        // Log the fault
        crate::serial_println!(
            "[FAULT_HUB] Fault #{:?}: {:?}/{:?} - {} (severity: {:?})",
            id, source, fault.fault_type, fault.message, severity
        );
        
        // Record in global state
        FAULT_STATE.record_fault(&fault);
        
        // Attempt recovery
        if FAULT_STATE.auto_recovery.load(Ordering::SeqCst) {
            self.attempt_recovery(&fault);
        }
        
        id
    }
    
    /// Attempt to recover from a fault
    pub fn attempt_recovery(&self, fault: &Fault) -> RecoveryResult {
        let result = if let Some(engine) = self.recovery_engine.lock().as_ref() {
            engine.recover(fault)
        } else {
            RecoveryResult::Failed
        };
        
        // Update statistics
        if result.is_success() {
            self.recovery_success.fetch_add(1, Ordering::SeqCst);
        } else {
            self.recovery_failure.fetch_add(1, Ordering::SeqCst);
        }
        
        // Update module health
        if let Some(module) = self.modules.lock().get_mut(&match fault.source {
            FaultSource::Memory => "memory",
            FaultSource::Cpu => "cpu",
            FaultSource::Smp => "smp",
            FaultSource::Interrupt => "interrupts",
            FaultSource::Scheduler => "scheduler",
            FaultSource::Driver => "drivers",
            FaultSource::Filesystem => "filesystem",
            FaultSource::Network => "network",
            FaultSource::Security => "security",
            FaultSource::Acpi => "acpi",
            _ => "unknown",
        }) {
            module.record_recovery(result.is_success());
        }
        
        crate::serial_println!(
            "[FAULT_HUB] Recovery result for fault #{:?}: {:?}",
            fault.id, result
        );
        
        result
    }
    
    /// Check all modules for faults
    pub fn check_all(&self) {
        if !self.initialized.load(Ordering::SeqCst) {
            return;
        }
        
        let current_tick = crate::task::scheduler::get_ticks();
        self.last_check.store(current_tick, Ordering::SeqCst);
        
        // Check each registered handler
        for (source, handler) in self.handlers.lock().iter() {
            if let Some(fault) = handler.check() {
                self.report_fault(fault);
            }
        }
    }
    
    /// Get health status for a module
    pub fn get_health(&self, module: &str) -> Option<ModuleHealth> {
        self.modules.lock().get(module).cloned()
    }
    
    /// Get all module health statuses
    pub fn get_all_health(&self) -> Vec<ModuleHealth> {
        self.modules.lock().values().cloned().collect()
    }
    
    /// Update current severity level
    fn update_severity(&self, severity: Severity) {
        let current = Severity::from(self.current_severity.load(Ordering::SeqCst) as u8);
        if severity > current {
            self.current_severity.store(severity as u32, Ordering::SeqCst);
        }
    }
    
    /// Get current system severity
    pub fn current_severity(&self) -> Severity {
        Severity::from(self.current_severity.load(Ordering::SeqCst) as u8)
    }
    
    /// Get fault statistics
    pub fn stats(&self) -> HubStats {
        HubStats {
            total_faults: self.fault_count.load(Ordering::SeqCst),
            recovery_success: self.recovery_success.load(Ordering::SeqCst),
            recovery_failure: self.recovery_failure.load(Ordering::SeqCst),
            current_severity: self.current_severity(),
            module_count: self.modules.lock().len(),
        }
    }
    
    /// Reset module health status
    pub fn reset_module(&self, module: &str) -> bool {
        if let Some(health) = self.modules.lock().get_mut(module) {
            health.status = HealthStatus::Healthy;
            health.fault_count = 0;
            health.last_fault_tick = 0;
            crate::serial_println!("[FAULT_HUB] Reset health for module: {}", module);
            true
        } else {
            false
        }
    }
    
    /// Check if system is in emergency state
    pub fn is_emergency(&self) -> bool {
        self.current_severity() == Severity::Emergency
    }
    
    /// Get recommended action for current state
    pub fn recommended_action(&self) -> RecommendedAction {
        self.current_severity().recommended_action()
    }
}

impl Severity {
    fn from(value: u8) -> Self {
        match value {
            0 => Severity::Normal,
            1 => Severity::Warning,
            2 => Severity::Degraded,
            3 => Severity::Critical,
            4 => Severity::Emergency,
            _ => Severity::Emergency,
        }
    }
}

#[derive(Clone, Debug)]
pub struct HubStats {
    pub total_faults: u64,
    pub recovery_success: u64,
    pub recovery_failure: u64,
    pub current_severity: Severity,
    pub module_count: usize,
}

// ============================================================================
// GLOBAL HUB INSTANCE
// ============================================================================

lazy_static::lazy_static! {
    pub static ref FAULT_HUB: FaultHub = FaultHub::new();
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// Initialize fault hub
pub fn init() {
    FAULT_HUB.init();
}

/// Report a fault
pub fn report(source: FaultSource, fault_type: FaultType, message: &str) -> FaultId {
    let fault = Fault::new(source, fault_type, message);
    FAULT_HUB.report_fault(fault)
}

/// Check all modules
pub fn check_all() {
    FAULT_HUB.check_all();
}

/// Get module health
pub fn health(module: &str) -> Option<ModuleHealth> {
    FAULT_HUB.get_health(module)
}

/// Get all health statuses
pub fn all_health() -> Vec<ModuleHealth> {
    FAULT_HUB.get_all_health()
}

/// Attempt recovery
pub fn recover(fault: &Fault) -> RecoveryResult {
    FAULT_HUB.attempt_recovery(fault)
}

/// Get hub statistics
pub fn stats() -> HubStats {
    FAULT_HUB.stats()
}
