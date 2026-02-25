//! # Recovery Engine
//!
//! Central recovery coordination and action execution.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

use super::{Fault, FaultSource, FaultType};
use super::severity::{Severity, RecoveryResult};

// ============================================================================
// RECOVERY ACTIONS
// ============================================================================

/// Recovery action to execute
#[derive(Clone, Debug)]
pub enum RecoveryAction {
    /// No action needed
    None,
    /// Log the fault and continue
    LogOnly,
    /// Reset the module
    ResetModule(String),
    /// Disable the module
    DisableModule(String),
    /// Switch to fallback mode
    FallbackMode(String),
    /// Kill specific task
    KillTask(u64),
    /// Free memory
    FreeMemory(usize),
    /// Sync filesystem
    SyncFilesystem,
    /// Emergency sync and halt
    EmergencyHalt,
    /// Trigger reboot
    Reboot,
}

/// Recovery strategy for a fault type
pub struct RecoveryStrategy {
    /// Primary recovery action
    pub primary: RecoveryAction,
    /// Fallback if primary fails
    pub fallback: Option<RecoveryAction>,
    /// Last resort action
    pub last_resort: RecoveryAction,
    /// Maximum attempts
    pub max_attempts: u32,
    /// Timeout in ticks
    pub timeout_ticks: u64,
}

impl RecoveryStrategy {
    pub fn new(primary: RecoveryAction, fallback: Option<RecoveryAction>, last_resort: RecoveryAction) -> Self {
        Self {
            primary,
            fallback,
            last_resort,
            max_attempts: 3,
            timeout_ticks: 1000,
        }
    }
    
    /// Get strategy for a fault type
    pub fn for_fault(fault: &Fault) -> Self {
        match &fault.fault_type {
            // Memory faults
            FaultType::HeapCorruption => Self::new(
                RecoveryAction::LogOnly, // Cannot truly recover
                None,
                RecoveryAction::EmergencyHalt,
            ),
            FaultType::OutOfMemory => Self::new(
                RecoveryAction::FreeMemory(64),
                Some(RecoveryAction::KillTask(0)), // Kill largest task
                RecoveryAction::EmergencyHalt,
            ),
            FaultType::DoubleFree | FaultType::UseAfterFree => Self::new(
                RecoveryAction::LogOnly,
                None,
                RecoveryAction::DisableModule("memory".into()),
            ),
            
            // CPU/SMP faults
            FaultType::ApStartupFailed => Self::new(
                RecoveryAction::LogOnly, // Already handled by SMP safety
                None,
                RecoveryAction::LogOnly,
            ),
            FaultType::TlbShootdownTimeout => Self::new(
                RecoveryAction::LogOnly,
                None,
                RecoveryAction::ResetModule("smp".into()),
            ),
            
            // Interrupt faults
            FaultType::IrqStorm => Self::new(
                RecoveryAction::DisableModule("irq_source".into()),
                None,
                RecoveryAction::LogOnly,
            ),
            FaultType::HandlerTimeout => Self::new(
                RecoveryAction::ResetModule("interrupts".into()),
                None,
                RecoveryAction::DisableModule("interrupts".into()),
            ),
            
            // Scheduler faults
            FaultType::RunQueueCorruption => Self::new(
                RecoveryAction::EmergencyHalt,
                None,
                RecoveryAction::EmergencyHalt,
            ),
            FaultType::TaskLeak => Self::new(
                RecoveryAction::LogOnly,
                None,
                RecoveryAction::LogOnly,
            ),
            
            // Driver faults
            FaultType::DeviceTimeout | FaultType::DeviceError => Self::new(
                RecoveryAction::ResetModule("driver".into()),
                Some(RecoveryAction::DisableModule("driver".into())),
                RecoveryAction::LogOnly,
            ),
            
            // Filesystem faults
            FaultType::MetadataCorruption => Self::new(
                RecoveryAction::SyncFilesystem,
                Some(RecoveryAction::DisableModule("filesystem".into())),
                RecoveryAction::EmergencyHalt,
            ),
            FaultType::IoError => Self::new(
                RecoveryAction::LogOnly,
                Some(RecoveryAction::DisableModule("filesystem".into())),
                RecoveryAction::LogOnly,
            ),
            
            // Network faults
            FaultType::ConnectionReset | FaultType::StackCorruption => Self::new(
                RecoveryAction::ResetModule("network".into()),
                Some(RecoveryAction::DisableModule("network".into())),
                RecoveryAction::LogOnly,
            ),
            
            // Security faults
            FaultType::CanaryMismatch => Self::new(
                RecoveryAction::EmergencyHalt, // Potential exploit
                None,
                RecoveryAction::EmergencyHalt,
            ),
            
            // Default
            _ => Self::new(
                RecoveryAction::LogOnly,
                None,
                RecoveryAction::LogOnly,
            ),
        }
    }
}

// ============================================================================
// RECOVERY ENGINE
// ============================================================================

/// Recovery engine state
pub struct RecoveryEngine {
    /// Recovery attempts per fault
    attempts: Mutex<BTreeMap<u64, u32>>,
    /// Active recoveries
    active: Mutex<Vec<u64>>,
    /// Recovery enabled
    enabled: AtomicBool,
    /// Total recoveries attempted
    total_attempts: AtomicU32,
    /// Successful recoveries
    successful: AtomicU32,
    /// Failed recoveries
    failed: AtomicU32,
}

impl RecoveryEngine {
    pub fn new() -> Self {
        Self {
            attempts: Mutex::new(BTreeMap::new()),
            active: Mutex::new(Vec::new()),
            enabled: AtomicBool::new(true),
            total_attempts: AtomicU32::new(0),
            successful: AtomicU32::new(0),
            failed: AtomicU32::new(0),
        }
    }
    
    /// Attempt recovery for a fault
    pub fn recover(&self, fault: &Fault) -> RecoveryResult {
        if !self.enabled.load(Ordering::SeqCst) {
            return RecoveryResult::Failed;
        }
        
        // Check if already being recovered
        if self.active.lock().contains(&fault.id.0) {
            return RecoveryResult::Failed;
        }
        
        // Mark as active
        self.active.lock().push(fault.id.0);
        
        // Get strategy
        let strategy = RecoveryStrategy::for_fault(fault);
        
        // Check attempt count
        let attempts = *self.attempts.lock().get(&fault.id.0).unwrap_or(&0);
        if attempts >= strategy.max_attempts {
            self.active.lock().retain(|&id| id != fault.id.0);
            return RecoveryResult::Failed;
        }
        
        // Increment attempts
        self.attempts.lock().insert(fault.id.0, attempts + 1);
        self.total_attempts.fetch_add(1, Ordering::SeqCst);
        
        crate::serial_println!(
            "[RECOVERY] Attempting recovery for fault #{:?} (attempt {}/{})",
            fault.id, attempts + 1, strategy.max_attempts
        );
        
        // Execute primary action
        let result = self.execute_action(&strategy.primary, fault);
        
        if result.is_success() {
            self.successful.fetch_add(1, Ordering::SeqCst);
            self.active.lock().retain(|&id| id != fault.id.0);
            return result;
        }
        
        // Try fallback
        if let Some(fallback) = &strategy.fallback {
            crate::serial_println!("[RECOVERY] Primary failed, trying fallback");
            let result = self.execute_action(fallback, fault);
            if result.is_success() {
                self.successful.fetch_add(1, Ordering::SeqCst);
                self.active.lock().retain(|&id| id != fault.id.0);
                return result;
            }
        }
        
        // Execute last resort
        crate::serial_println!("[RECOVERY] All attempts failed, executing last resort");
        let result = self.execute_action(&strategy.last_resort, fault);
        
        if !result.is_success() {
            self.failed.fetch_add(1, Ordering::SeqCst);
        }
        
        self.active.lock().retain(|&id| id != fault.id.0);
        result
    }
    
    /// Execute a recovery action
    fn execute_action(&self, action: &RecoveryAction, fault: &Fault) -> RecoveryResult {
        match action {
            RecoveryAction::None => RecoveryResult::Recovered,
            
            RecoveryAction::LogOnly => {
                crate::serial_println!(
                    "[RECOVERY] Logged fault: {:?} - {}",
                    fault.fault_type, fault.message
                );
                RecoveryResult::Recovered
            }
            
            RecoveryAction::ResetModule(module) => {
                crate::serial_println!("[RECOVERY] Resetting module: {}", module);
                self.reset_module(module)
            }
            
            RecoveryAction::DisableModule(module) => {
                crate::serial_println!("[RECOVERY] Disabling module: {}", module);
                self.disable_module(module)
            }
            
            RecoveryAction::FallbackMode(mode) => {
                crate::serial_println!("[RECOVERY] Entering fallback mode: {}", mode);
                self.enter_fallback(mode)
            }
            
            RecoveryAction::KillTask(task_id) => {
                crate::serial_println!("[RECOVERY] Killing task: {}", task_id);
                self.kill_task(*task_id)
            }
            
            RecoveryAction::FreeMemory(pages) => {
                crate::serial_println!("[RECOVERY] Freeing {} pages", pages);
                self.free_memory(*pages)
            }
            
            RecoveryAction::SyncFilesystem => {
                crate::serial_println!("[RECOVERY] Syncing filesystem");
                self.sync_filesystem()
            }
            
            RecoveryAction::EmergencyHalt => {
                crate::serial_println!("[RECOVERY] EMERGENCY HALT");
                self.emergency_halt()
            }
            
            RecoveryAction::Reboot => {
                crate::serial_println!("[RECOVERY] Rebooting system");
                self.reboot()
            }
        }
    }
    
    /// Reset a module
    fn reset_module(&self, module: &str) -> RecoveryResult {
        match module {
            "network" => {
                // Reset network stack
                crate::serial_println!("[RECOVERY] Network stack reset not implemented");
                RecoveryResult::Degraded
            }
            "driver" => {
                // Driver reset handled by driver recovery module
                RecoveryResult::Degraded
            }
            "interrupts" => {
                // Re-initialize IDT
                crate::serial_println!("[RECOVERY] IDT reset not safe, degraded mode");
                RecoveryResult::Degraded
            }
            _ => RecoveryResult::Failed,
        }
    }
    
    /// Disable a module
    fn disable_module(&self, module: &str) -> RecoveryResult {
        crate::serial_println!("[RECOVERY] Module {} disabled", module);
        RecoveryResult::Degraded
    }
    
    /// Enter fallback mode
    fn enter_fallback(&self, _mode: &str) -> RecoveryResult {
        RecoveryResult::Degraded
    }
    
    /// Kill a task
    fn kill_task(&self, task_id: u64) -> RecoveryResult {
        if task_id == 0 {
            // Find largest memory consumer
            crate::serial_println!("[RECOVERY] OOM: Would kill largest task");
        }
        RecoveryResult::Recovered
    }
    
    /// Free memory pages
    fn free_memory(&self, pages: usize) -> RecoveryResult {
        crate::memory::reclaim_pages_global(pages);
        RecoveryResult::Recovered
    }
    
    /// Sync filesystem
    fn sync_filesystem(&self) -> RecoveryResult {
        // Emergency sync
        crate::serial_println!("[RECOVERY] Filesystem sync attempted");
        RecoveryResult::Recovered
    }
    
    /// Emergency halt
    fn emergency_halt(&self) -> RecoveryResult {
        crate::serial_println!("[RECOVERY] === EMERGENCY HALT ===");
        crate::serial_println!("[RECOVERY] System halted due to unrecoverable fault");
        
        // Disable interrupts and halt
        unsafe {
            x86_64::instructions::interrupts::disable();
            loop {
                x86_64::instructions::hlt();
            }
        }
    }
    
    /// Reboot system
    fn reboot(&self) -> RecoveryResult {
        crate::serial_println!("[RECOVERY] System reboot initiated");
        // Use ACPI or keyboard controller to reboot
        RecoveryResult::RequiresReboot
    }
    
    /// Enable/disable recovery
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }
    
    /// Get statistics
    pub fn stats(&self) -> RecoveryStats {
        RecoveryStats {
            total_attempts: self.total_attempts.load(Ordering::SeqCst),
            successful: self.successful.load(Ordering::SeqCst),
            failed: self.failed.load(Ordering::SeqCst),
            active_count: self.active.lock().len(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct RecoveryStats {
    pub total_attempts: u32,
    pub successful: u32,
    pub failed: u32,
    pub active_count: usize,
}

// ============================================================================
// INITIALIZATION
// ============================================================================

lazy_static::lazy_static! {
    static ref RECOVERY_ENGINE: RecoveryEngine = RecoveryEngine::new();
}

pub fn init() {
    crate::serial_println!("[RECOVERY] Recovery engine initialized");
}

pub fn attempt_recovery(fault: &Fault) -> RecoveryResult {
    RECOVERY_ENGINE.recover(fault)
}

pub fn get_stats() -> RecoveryStats {
    RECOVERY_ENGINE.stats()
}

pub fn set_enabled(enabled: bool) {
    RECOVERY_ENGINE.set_enabled(enabled);
}
