//! # Health Monitors
//!
//! Per-module health monitoring and fault detection.

pub mod memory;
pub mod cpu;
pub mod smp;
pub mod irq;
pub mod scheduler;
pub mod driver;
pub mod fs;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::fault::{Fault, FaultSource, FaultType, HealthStatus, ModuleHealth};

// ============================================================================
// MONITOR TRAIT
// ============================================================================

/// Trait for health monitors
pub trait HealthMonitor: Send + Sync {
    /// Get module name
    fn name(&self) -> &'static str;
    
    /// Check for faults
    fn check(&self) -> Option<Fault>;
    
    /// Get current health status
    fn health(&self) -> HealthStatus;
    
    /// Get module health info
    fn module_health(&self) -> ModuleHealth;
    
    /// Reset monitor state
    fn reset(&self);
}

// ============================================================================
// MONITOR REGISTRY
// ============================================================================

/// Global monitor registry
pub struct MonitorRegistry {
    monitors: spin::Mutex<Vec<&'static dyn HealthMonitor>>,
    initialized: AtomicBool,
}

impl MonitorRegistry {
    pub const fn new() -> Self {
        Self {
            monitors: spin::Mutex::new(Vec::new()),
            initialized: AtomicBool::new(false),
        }
    }
    
    pub fn register(&self, monitor: &'static dyn HealthMonitor) {
        self.monitors.lock().push(monitor);
    }
    
    pub fn check_all(&self) {
        for monitor in self.monitors.lock().iter() {
            if let Some(fault) = monitor.check() {
                crate::fault::hub::report(fault.source, fault.fault_type, &fault.message);
            }
        }
    }
    
    pub fn get_health(&self, name: &str) -> Option<ModuleHealth> {
        for monitor in self.monitors.lock().iter() {
            if monitor.name() == name {
                return Some(monitor.module_health());
            }
        }
        None
    }
    
    pub fn all_health(&self) -> Vec<ModuleHealth> {
        self.monitors.lock().iter().map(|m| m.module_health()).collect()
    }
}

lazy_static::lazy_static! {
    pub static ref MONITORS: MonitorRegistry = MonitorRegistry::new();
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init() {
    if MONITORS.initialized.swap(true, Ordering::SeqCst) {
        return;
    }
    
    // Register monitors
    MONITORS.register(&memory::MEMORY_MONITOR);
    MONITORS.register(&cpu::CPU_MONITOR);
    MONITORS.register(&smp::SMP_MONITOR);
    MONITORS.register(&irq::IRQ_MONITOR);
    MONITORS.register(&scheduler::SCHEDULER_MONITOR);
    MONITORS.register(&driver::DRIVER_MONITOR);
    MONITORS.register(&fs::FS_MONITOR);
    
    crate::serial_println!("[MONITORS] Initialized {} monitors", MONITORS.monitors.lock().len());
}

pub fn check_all() {
    MONITORS.check_all();
}

pub fn get_health(name: &str) -> Option<ModuleHealth> {
    MONITORS.get_health(name)
}

pub fn all_health() -> Vec<ModuleHealth> {
    MONITORS.all_health()
}
