//! # Fault Injection
//!
//! Testing framework for fault injection and recovery verification.
//! Only available in debug builds.

#[cfg(debug_assertions)]
use alloc::string::String;
#[cfg(debug_assertions)]
use alloc::vec::Vec;

use crate::fault::{Fault, FaultSource, FaultType};

// ============================================================================
// FAULT INJECTION (DEBUG ONLY)
// ============================================================================

#[cfg(debug_assertions)]
pub struct FaultInjector {
    enabled: core::sync::atomic::AtomicBool,
    injection_count: core::sync::atomic::AtomicU64,
}

#[cfg(debug_assertions)]
impl FaultInjector {
    pub const fn new() -> Self {
        Self {
            enabled: core::sync::atomic::AtomicBool::new(false),
            injection_count: core::sync::atomic::AtomicU64::new(0),
        }
    }
    
    /// Enable/disable fault injection
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, core::sync::atomic::Ordering::SeqCst);
    }
    
    /// Inject a specific fault
    pub fn inject(&self, source: FaultSource, fault_type: FaultType, message: &str) {
        if !self.enabled.load(core::sync::atomic::Ordering::SeqCst) {
            return;
        }
        
        self.injection_count.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
        
        crate::serial_println!(
            "[FAULT_INJECT] Injecting fault: {:?}/{:?}",
            source, fault_type
        );
        
        crate::fault::hub::report(source, fault_type, message);
    }
    
    /// Inject memory fault
    pub fn inject_memory_fault(&self) {
        self.inject(
            FaultSource::Memory,
            FaultType::HeapCorruption,
            "Injected heap corruption for testing"
        );
    }
    
    /// Inject OOM
    pub fn inject_oom(&self) {
        self.inject(
            FaultSource::Memory,
            FaultType::OutOfMemory,
            "Injected OOM for testing"
        );
    }
    
    /// Inject driver fault
    pub fn inject_driver_fault(&self) {
        self.inject(
            FaultSource::Driver,
            FaultType::DeviceTimeout,
            "Injected device timeout for testing"
        );
    }
    
    /// Inject scheduler fault
    pub fn inject_scheduler_fault(&self) {
        self.inject(
            FaultSource::Scheduler,
            FaultType::TaskLeak,
            "Injected task leak for testing"
        );
    }
    
    /// Get injection count
    pub fn count(&self) -> u64 {
        self.injection_count.load(core::sync::atomic::Ordering::SeqCst)
    }
}

#[cfg(debug_assertions)]
lazy_static::lazy_static! {
    pub static ref FAULT_INJECTOR: FaultInjector = FaultInjector::new();
}

// ============================================================================
// TEST SCENARIOS
// ============================================================================

#[cfg(debug_assertions)]
pub fn run_test_scenarios() {
    crate::serial_println!("[FAULT_INJECT] Running fault injection test scenarios");
    
    // Enable injection
    FAULT_INJECTOR.set_enabled(true);
    
    // Test 1: Memory fault
    crate::serial_println!("[FAULT_INJECT] Test 1: Memory fault");
    FAULT_INJECTOR.inject_oom();
    
    // Test 2: Driver fault
    crate::serial_println!("[FAULT_INJECT] Test 2: Driver fault");
    FAULT_INJECTOR.inject_driver_fault();
    
    // Test 3: Scheduler fault
    crate::serial_println!("[FAULT_INJECT] Test 3: Scheduler fault");
    FAULT_INJECTOR.inject_scheduler_fault();
    
    // Report results
    let stats = crate::fault::get_stats();
    crate::serial_println!(
        "[FAULT_INJECT] Test complete: {} faults injected, {} recoveries",
        FAULT_INJECTOR.count(),
        stats.total_recoveries
    );
    
    // Disable injection
    FAULT_INJECTOR.set_enabled(false);
}

#[cfg(not(debug_assertions))]
pub fn run_test_scenarios() {
    // No-op in release builds
}
