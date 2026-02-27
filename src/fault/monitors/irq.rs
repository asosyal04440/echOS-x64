//! # IRQ Sağlık Monitörü
//!
//! Kesme fırtınalarını (IRQ storm), işleyici zaman aşımlarını ve IDT bütünlüğünü izler.
//! Yüksek frekanslı kesme aktivitesi sistemi bozabilir; bu modül bunu tespit eder.

use core::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, AtomicBool, Ordering};

use crate::fault::{Fault, FaultSource, FaultType, HealthStatus, ModuleHealth};

pub struct IrqMonitor {
    /// IRQ fırtınası sayısı
    storm_count: AtomicU32,
    /// İşleyici zaman aşımı sayısı
    handler_timeouts: AtomicU32,
    /// Sahte (spurious) kesme sayısı
    spurious_count: AtomicU64,
    /// Son kontrol zaman damgası
    last_check: AtomicUsize,
    /// Monitör etkin mi?
    enabled: AtomicBool,
    /// Fırtına eşiği (kontrol başına IRQ sayısı)
    storm_threshold: u64,
}

impl IrqMonitor {
    pub const fn new() -> Self {
        Self {
            storm_count: AtomicU32::new(0),
            handler_timeouts: AtomicU32::new(0),
            spurious_count: AtomicU64::new(0),
            last_check: AtomicUsize::new(0),
            enabled: AtomicBool::new(true),
            storm_threshold: 500,
        }
    }
    
    /// IRQ fırtınası kaydeder
    pub fn record_storm(&self) {
        self.storm_count.fetch_add(1, Ordering::SeqCst);
    }
    
    /// İşleyici zaman aşımı kaydeder
    pub fn record_handler_timeout(&self) {
        self.handler_timeouts.fetch_add(1, Ordering::SeqCst);
    }
    
    /// Sahte kesme kaydeder
    pub fn record_spurious(&self) {
        self.spurious_count.fetch_add(1, Ordering::SeqCst);
    }
    
    /// IRQ fırtınalarını kontrol eder
    fn check_storms(&self) -> Option<Fault> {
        // Kesme modülünden IRQ hızını kontrol et
        let stats = crate::interrupts::get_stats();
        
        if stats.storm_count > self.storm_threshold {
            self.record_storm();
            return Some(Fault::new(
                FaultSource::Interrupt,
                FaultType::IrqStorm,
                &alloc::format!("IRQ storm detected: {} storms", stats.storm_count)
            ));
        }
        
        None
    }
    
    /// Sahte kesmeleri kontrol eder
    fn check_spurious(&self) -> Option<Fault> {
        let spurious = self.spurious_count.load(Ordering::SeqCst);
        
        if spurious > 10 {
            return Some(Fault::new(
                FaultSource::Interrupt,
                FaultType::SpuriousInterrupt,
                &alloc::format!("High spurious interrupt count: {}", spurious)
            ));
        }
        
        None
    }
}

impl super::HealthMonitor for IrqMonitor {
    fn name(&self) -> &'static str {
        "interrupts"
    }
    
    fn check(&self) -> Option<Fault> {
        if !self.enabled.load(Ordering::SeqCst) {
            return None;
        }
        
        self.last_check.store(
            crate::task::scheduler::get_ticks(),
            Ordering::SeqCst
        );
        
        if let Some(fault) = self.check_storms() {
            return Some(fault);
        }
        
        if let Some(fault) = self.check_spurious() {
            return Some(fault);
        }
        
        None
    }
    
    fn health(&self) -> HealthStatus {
        let storms = self.storm_count.load(Ordering::SeqCst);
        let timeouts = self.handler_timeouts.load(Ordering::SeqCst);
        
        if storms > 5 || timeouts > 3 {
            HealthStatus::Degraded
        } else if storms > 0 || timeouts > 0 {
            HealthStatus::Warning
        } else {
            HealthStatus::Healthy
        }
    }
    
    fn module_health(&self) -> ModuleHealth {
        ModuleHealth {
            name: self.name(),
            status: self.health(),
            fault_count: self.storm_count.load(Ordering::SeqCst) + self.handler_timeouts.load(Ordering::SeqCst),
            recovery_count: 0,
            last_fault_tick: self.last_check.load(Ordering::SeqCst),
            uptime_ticks: crate::task::scheduler::get_ticks(),
            is_critical: true,
            can_restart: false,
            has_fallback: false,
        }
    }
    
    fn reset(&self) {
        self.storm_count.store(0, Ordering::SeqCst);
        self.handler_timeouts.store(0, Ordering::SeqCst);
        self.spurious_count.store(0, Ordering::SeqCst);
    }
}

pub static IRQ_MONITOR: IrqMonitor = IrqMonitor::new();
