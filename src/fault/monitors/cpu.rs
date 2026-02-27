//! # CPU Sağlık Monitörü
//!
//! CPU sağlığını, askıya alınmış CPU'ları ve ısıl olayları izler.
//! SMP ortamında her CPU'nun aktifliğini takip eder.

use core::sync::atomic::{AtomicU32, AtomicUsize, AtomicBool, Ordering};

use crate::fault::{Fault, FaultSource, FaultType, HealthStatus, ModuleHealth};

// ============================================================================
// CPU MONİTÖR DURUMU
// ============================================================================

pub struct CpuMonitor {
    /// Toplam CPU sayısı
    cpu_count: AtomicU32,
    /// Askıya alınmış (hung) CPU sayısı
    hung_cpus: AtomicU32,
    /// Isıl (thermal) olay sayısı
    thermal_events: AtomicU32,
    /// Son kontrol zaman damgası
    last_check: AtomicUsize,
    /// Monitör etkin mi?
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
    
    /// CPU sağlığını kontrol eder — çevrimdişı CPU'ları tespit eder
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
    
    /// Isıl durumu kontrol eder (ACPI termal zone entegrasyonu)
    fn check_thermal(&self) -> Option<Fault> {
        // ACPI termal zonları üzerinden isıl olayları kontrol et
        // ACPI termal zone entegrasyonu burada gerçekleştirilecek
        None
    }
    
    /// CPU sayısını günceller
    pub fn set_cpu_count(&self, count: u32) {
        self.cpu_count.store(count, Ordering::SeqCst);
    }
    
    /// Isıl olay kaydeder
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
