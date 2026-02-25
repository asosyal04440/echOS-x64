//! # CPU Health Monitor
//!
//! Monitors CPU health, hung CPUs, and thermal events.

use core::sync::atomic::{AtomicU32, AtomicUsize, AtomicBool, Ordering};

use crate::fault::{Fault, FaultSource, FaultType, HealthStatus, ModuleHealth};

// ============================================================================
// CPU MONITOR STATE
// ============================================================================

pub struct CpuMonitor {
    /// CPU count
    cpu_count: AtomicU32,
    /// Hung CPU count
    hung_cpus: AtomicU32,
    /// Thermal events
    thermal_events: AtomicU32,
    /// Last check timestamp
    last_check: AtomicUsize,
    /// Monitor enabled
    enabled: AtomicBool,
}

impl CpuMonitor {
    pub const fn new() -> Self {
        Self {
            cpu_count: AtomicU32::new(1),
            hung_cpus: AtomicU32::new(0),
            thermal_events: AtomicU32::new(0),
            last_check: AtomicUsize::new(0),
            enabled: AtomicBool::new(true),
        }
    }
    
    /// Check CPU health
    fn check_cpu_health(&self) -> Option<Fault> {
        let cpu_count = self.cpu_count.load(Ordering::SeqCst);
        let online = crate::cpu::smp::online_cpu_count();
        
        if online < cpu_count {
            let offline = cpu_count - online;
            return Some(Fault::new(
                FaultSource::Cpu,
                FaultType::CpuHung,
                &alloc::format!("{} CPU(s) offline", offline)
            ));
        }
        
        None
    }
    
    /// Check thermal status
    fn check_thermal(&self) -> Option<Fault> {
        // Check for thermal events via ACPI
        // This would integrate with ACPI thermal zones
        None
    }
    
    /// Update CPU count
    pub fn set_cpu_count(&self, count: u32) {
        self.cpu_count.store(count, Ordering::SeqCst);
    }
    
    /// Record thermal event
    pub fn record_thermal_event(&self) {
        self.thermal_events.fetch_add(1, Ordering::SeqCst);
    }
}

impl super::HealthMonitor for CpuMonitor {
    fn name(&self) -> &'static str {
        "cpu"
    }
    
    fn check(&self) -> Option<Fault> {
        if !self.enabled.load(Ordering::SeqCst) {
            return None;
        }
        
        self.last_check.store(
            crate::task::scheduler::get_ticks(),
            Ordering::SeqCst
        );
        
        if let Some(fault) = self.check_cpu_health() {
            return Some(fault);
        }
        
        if let Some(fault) = self.check_thermal() {
            return Some(fault);
        }
        
        None
    }
    
    fn health(&self) -> HealthStatus {
        let hung = self.hung_cpus.load(Ordering::SeqCst);
        let thermal = self.thermal_events.load(Ordering::SeqCst);
        
        if hung > 0 || thermal > 2 {
            HealthStatus::Degraded
        } else if thermal > 0 {
            HealthStatus::Warning
        } else {
            HealthStatus::Healthy
        }
    }
    
    fn module_health(&self) -> ModuleHealth {
        ModuleHealth {
            name: self.name(),
            status: self.health(),
            fault_count: self.hung_cpus.load(Ordering::SeqCst) + self.thermal_events.load(Ordering::SeqCst),
            recovery_count: 0,
            last_fault_tick: self.last_check.load(Ordering::SeqCst),
            uptime_ticks: crate::task::scheduler::get_ticks(),
            is_critical: true,
            can_restart: false,
            has_fallback: false,
        }
    }
    
    fn reset(&self) {
        self.hung_cpus.store(0, Ordering::SeqCst);
        self.thermal_events.store(0, Ordering::SeqCst);
    }
}

pub static CPU_MONITOR: CpuMonitor = CpuMonitor::new();
