//! # SMP Health Monitor
//!
//! Monitors SMP state, AP startup, and TLB shootdown.

use core::sync::atomic::{AtomicU32, AtomicUsize, AtomicBool, Ordering};

use crate::fault::{Fault, FaultSource, FaultType, HealthStatus, ModuleHealth};

pub struct SmpMonitor {
    /// AP startup failures
    ap_failures: AtomicU32,
    /// TLB shootdown timeouts
    tlb_timeouts: AtomicU32,
    /// Last check timestamp
    last_check: AtomicUsize,
    /// Monitor enabled
    enabled: AtomicBool,
}

impl SmpMonitor {
    pub const fn new() -> Self {
        Self {
            ap_failures: AtomicU32::new(0),
            tlb_timeouts: AtomicU32::new(0),
            last_check: AtomicUsize::new(0),
            enabled: AtomicBool::new(true),
        }
    }
    
    /// Record AP startup failure
    pub fn record_ap_failure(&self) {
        self.ap_failures.fetch_add(1, Ordering::SeqCst);
    }
    
    /// Record TLB shootdown timeout
    pub fn record_tlb_timeout(&self) {
        self.tlb_timeouts.fetch_add(1, Ordering::SeqCst);
    }
    
    /// Check SMP health
    fn check_smp(&self) -> Option<Fault> {
        let expected = crate::cpu::CPU_INFO.lock().topology.logical_count;
        let online = crate::cpu::smp::online_cpu_count();
        
        if online < expected && expected > 1 {
            return Some(Fault::new(
                FaultSource::Smp,
                FaultType::ApStartupFailed,
                &alloc::format!("{} of {} CPUs online", online, expected)
            ));
        }
        
        None
    }
}

impl super::HealthMonitor for SmpMonitor {
    fn name(&self) -> &'static str {
        "smp"
    }
    
    fn check(&self) -> Option<Fault> {
        if !self.enabled.load(Ordering::SeqCst) {
            return None;
        }
        
        self.last_check.store(
            crate::task::scheduler::get_ticks(),
            Ordering::SeqCst
        );
        
        self.check_smp()
    }
    
    fn health(&self) -> HealthStatus {
        let ap_fail = self.ap_failures.load(Ordering::SeqCst);
        let tlb = self.tlb_timeouts.load(Ordering::SeqCst);
        
        if ap_fail > 2 || tlb > 5 {
            HealthStatus::Degraded
        } else if ap_fail > 0 || tlb > 0 {
            HealthStatus::Warning
        } else {
            HealthStatus::Healthy
        }
    }
    
    fn module_health(&self) -> ModuleHealth {
        ModuleHealth {
            name: self.name(),
            status: self.health(),
            fault_count: self.ap_failures.load(Ordering::SeqCst) + self.tlb_timeouts.load(Ordering::SeqCst),
            recovery_count: 0,
            last_fault_tick: self.last_check.load(Ordering::SeqCst),
            uptime_ticks: crate::task::scheduler::get_ticks(),
            is_critical: true,
            can_restart: false,
            has_fallback: false,
        }
    }
    
    fn reset(&self) {
        self.ap_failures.store(0, Ordering::SeqCst);
        self.tlb_timeouts.store(0, Ordering::SeqCst);
    }
}

pub static SMP_MONITOR: SmpMonitor = SmpMonitor::new();
