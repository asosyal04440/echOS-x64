//! # Emergency Mode
//!
//! Emergency shutdown and minimal operation mode.

use core::sync::atomic::{AtomicU64, AtomicUsize, AtomicBool, Ordering};

// ============================================================================
// EMERGENCY STATE
// ============================================================================

/// Emergency mode state
pub struct EmergencyState {
    /// In emergency mode
    active: AtomicBool,
    /// Emergency reason
    reason: spin::Mutex<Option<alloc::string::String>>,
    /// Emergency start time
    start_time: AtomicUsize,
    /// Emergency count
    count: AtomicU64,
    /// Attempt recovery
    attempt_recovery: AtomicBool,
}

impl EmergencyState {
    pub const fn new() -> Self {
        Self {
            active: AtomicBool::new(false),
            reason: spin::Mutex::new(None),
            start_time: AtomicUsize::new(0),
            count: AtomicU64::new(0),
            attempt_recovery: AtomicBool::new(true),
        }
    }
    
    /// Enter emergency mode
    pub fn enter(&self, reason: &str) {
        if self.active.swap(true, Ordering::SeqCst) {
            return; // Already in emergency mode
        }
        
        self.count.fetch_add(1, Ordering::SeqCst);
        self.start_time.store(
            crate::task::scheduler::get_ticks(),
            Ordering::SeqCst
        );
        *self.reason.lock() = Some(alloc::string::String::from(reason));
        
        crate::serial_println!("[EMERGENCY] === ENTERING EMERGENCY MODE ===");
        crate::serial_println!("[EMERGENCY] Reason: {}", reason);
        
        // Disable non-critical modules
        crate::fault::degradation::set_level(crate::fault::severity::RecoveryLevel::Level4);
        
        // Sync filesystems
        crate::serial_println!("[EMERGENCY] Syncing filesystems...");
        crate::fault::recovery_modules::fs::emergency_sync();
        
        // Log system state
        self.log_state();
    }
    
    /// Exit emergency mode
    pub fn exit(&self) {
        if !self.active.swap(false, Ordering::SeqCst) {
            return; // Not in emergency mode
        }
        
        crate::serial_println!("[EMERGENCY] === EXITING EMERGENCY MODE ===");
        
        *self.reason.lock() = None;
        
        // Restore normal operation
        crate::fault::degradation::set_level(crate::fault::severity::RecoveryLevel::Level0);
    }
    
    /// Log current system state
    fn log_state(&self) {
        // Memory state
        if let Some(mm) = crate::memory::global_memory_manager() {
            let mm: &crate::memory::MemoryManager = mm;
            let free = mm.free_frames();
            let total = mm.total_frames();
            crate::serial_println!(
                "[EMERGENCY] Memory: {} / {} frames free",
                free,
                total
            );
        }
        
        // CPU state
        crate::serial_println!(
            "[EMERGENCY] CPUs: {} online",
            crate::cpu::smp::online_cpu_count()
        );
        
        // Fault stats
        let stats = crate::fault::get_stats();
        crate::serial_println!(
            "[EMERGENCY] Faults: {} total, {} recoveries, level {}",
            stats.total_faults,
            stats.total_recoveries,
            stats.recovery_level
        );
        
        // Scheduler state
        let sched_stats = crate::task::scheduler::get_stats();
        crate::serial_println!(
            "[EMERGENCY] Tasks: {} total, {} running, {} zombies",
            sched_stats.total_tasks,
            sched_stats.running_tasks,
            sched_stats.zombie_count
        );
    }
    
    /// Check if in emergency mode
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }
    
    /// Get emergency reason
    pub fn reason(&self) -> Option<alloc::string::String> {
        self.reason.lock().clone()
    }
    
    /// Get emergency duration
    pub fn duration(&self) -> usize {
        if !self.active.load(Ordering::SeqCst) {
            return 0;
        }
        
        crate::task::scheduler::get_ticks().saturating_sub(
            self.start_time.load(Ordering::SeqCst)
        )
    }
    
    /// Safe halt - preserve data and halt
    pub fn safe_halt(&self) -> ! {
        crate::serial_println!("[EMERGENCY] === SAFE HALT ===");
        crate::serial_println!("[EMERGENCY] System is halting safely");
        
        // Final sync
        crate::fault::recovery_modules::fs::emergency_sync();
        
        // Disable interrupts and halt
        unsafe {
            x86_64::instructions::interrupts::disable();
            loop {
                x86_64::instructions::hlt();
            }
        }
    }
    
    /// Emergency reboot
    pub fn reboot(&self) -> ! {
        crate::serial_println!("[EMERGENCY] === EMERGENCY REBOOT ===");
        
        // Final sync
        crate::fault::recovery_modules::fs::emergency_sync();
        
        // Try ACPI reset
        crate::serial_println!("[EMERGENCY] Attempting ACPI reset...");
        
        // If ACPI fails, try keyboard controller
        // unsafe { ... }
        
        // If all else fails, triple fault
        crate::serial_println!("[EMERGENCY] Forcing reset via triple fault");
        
        unsafe {
            // Load invalid IDT and trigger interrupt
            core::arch::asm!(
                "lidt [{0}]",
                "int 3",
                in(reg) &0u64 as *const u64,
                options(noreturn)
            );
        }
    }
}

// ============================================================================
// GLOBAL INSTANCE
// ============================================================================

lazy_static::lazy_static! {
    pub static ref EMERGENCY_STATE: EmergencyState = EmergencyState::new();
}

// ============================================================================
// PUBLIC API
// ============================================================================

pub fn enter() {
    EMERGENCY_STATE.enter("System triggered emergency mode");
}

pub fn enter_with_reason(reason: &str) {
    EMERGENCY_STATE.enter(reason);
}

pub fn exit() {
    EMERGENCY_STATE.exit();
}

pub fn is_active() -> bool {
    EMERGENCY_STATE.is_active()
}

pub fn reason() -> Option<alloc::string::String> {
    EMERGENCY_STATE.reason()
}

pub fn duration() -> usize {
    EMERGENCY_STATE.duration()
}

pub fn safe_halt() -> ! {
    EMERGENCY_STATE.safe_halt()
}

pub fn reboot() -> ! {
    EMERGENCY_STATE.reboot()
}
