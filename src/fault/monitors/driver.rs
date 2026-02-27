//! # Sürücsü Sağlık Monitörü
//!
//! Sürücsü sağlığını, cihaz zaman aşımlarını ve DMA bütünlüğünü izler.
//! Donanım sürücsülerinin güvenilirliğini sürekli denetler.

use core::sync::atomic::{AtomicU32, AtomicUsize, AtomicBool, Ordering};

use crate::fault::{Fault, FaultSource, FaultType, HealthStatus, ModuleHealth};

pub struct DriverMonitor {
    /// Cihaz zaman aşımı sayısı
    device_timeouts: AtomicU32,
    /// Cihaz hatası sayısı
    device_errors: AtomicU32,
    /// DMA hatası sayısı
    dma_errors: AtomicU32,
    /// Son kontrol zaman damgası
    last_check: AtomicUsize,
    /// Monitör etkin mi?
    enabled: AtomicBool,
}

impl DriverMonitor {
    pub const fn new() -> Self {
        Self {
            device_timeouts: AtomicU32::new(0),
            device_errors: AtomicU32::new(0),
            dma_errors: AtomicU32::new(0),
            last_check: AtomicUsize::new(0),
            enabled: AtomicBool::new(true),
        }
    }
    
    /// Cihaz zaman aşımı kaydeder
    pub fn record_timeout(&self) {
        self.device_timeouts.fetch_add(1, Ordering::SeqCst);
    }
    
    /// Cihaz hatası kaydeder
    pub fn record_error(&self) {
        self.device_errors.fetch_add(1, Ordering::SeqCst);
    }
    
    /// DMA hatası kaydeder
    pub fn record_dma_error(&self) {
        self.dma_errors.fetch_add(1, Ordering::SeqCst);
    }
    
    /// Sürücsü sağlığını kontrol eder — zaman aşımı ve hata eşiklerini değerlendirir
    fn check_drivers(&self) -> Option<Fault> {
        let timeouts = self.device_timeouts.load(Ordering::SeqCst);
        let errors = self.device_errors.load(Ordering::SeqCst);
        
        if timeouts > 5 {
            return Some(Fault::new(
                FaultSource::Driver,
                FaultType::DeviceTimeout,
                &alloc::format!("Multiple device timeouts: {}", timeouts)
            ));
        }
        
        if errors > 10 {
            return Some(Fault::new(
                FaultSource::Driver,
                FaultType::DeviceError,
                &alloc::format!("Multiple device errors: {}", errors)
            ));
        }
        
        None
    }
}

impl super::HealthMonitor for DriverMonitor {
    fn name(&self) -> &'static str {
        "drivers"
    }
    
    fn check(&self) -> Option<Fault> {
        if !self.enabled.load(Ordering::SeqCst) {
            return None;
        }
        
        self.last_check.store(
            crate::task::scheduler::get_ticks(),
            Ordering::SeqCst
        );
        
        self.check_drivers()
    }
    
    fn health(&self) -> HealthStatus {
        let timeouts = self.device_timeouts.load(Ordering::SeqCst);
        let errors = self.device_errors.load(Ordering::SeqCst);
        let dma = self.dma_errors.load(Ordering::SeqCst);
        
        if dma > 0 || timeouts > 10 || errors > 20 {
            HealthStatus::Failed
        } else if timeouts > 3 || errors > 5 {
            HealthStatus::Degraded
        } else if timeouts > 0 || errors > 0 {
            HealthStatus::Warning
        } else {
            HealthStatus::Healthy
        }
    }
    
    fn module_health(&self) -> ModuleHealth {
        ModuleHealth {
            name: self.name(),
            status: self.health(),
            fault_count: self.device_timeouts.load(Ordering::SeqCst) + self.device_errors.load(Ordering::SeqCst),
            recovery_count: 0,
            last_fault_tick: self.last_check.load(Ordering::SeqCst),
            uptime_ticks: crate::task::scheduler::get_ticks(),
            is_critical: false,
            can_restart: true,
            has_fallback: true,
        }
    }
    
    fn reset(&self) {
        self.device_timeouts.store(0, Ordering::SeqCst);
        self.device_errors.store(0, Ordering::SeqCst);
        self.dma_errors.store(0, Ordering::SeqCst);
    }
}

pub static DRIVER_MONITOR: DriverMonitor = DriverMonitor::new();
