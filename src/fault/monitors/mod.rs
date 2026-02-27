//! # Sağlık Monitörleri
//!
//! Modül başına sağlık izleme ve hata tespiti.
//! Her modül için ayrı bir monitör çalışarak periyodik kontrol yapar.

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
// MONİTÖR TRAIT'İ
// ============================================================================

/// Sağlık monitörleri için ortak trait (arayüz)
pub trait HealthMonitor: Send + Sync {
    /// Modül adını döndürür
    fn name(&self) -> &'static str;
    
    /// Hataları kontrol eder
    fn check(&self) -> Option<Fault>;
    
    /// Mevcut sağlık durumunu döndürür
    fn health(&self) -> HealthStatus;
    
    /// Modül sağlık bilgisini döndürür
    fn module_health(&self) -> ModuleHealth;
    
    /// Monitör durumunu sıfırlar
    fn reset(&self);
}

// ============================================================================
// MONİTÖR KAYIT DEFTERİ
// ============================================================================

/// Global monitör kayıt defteri
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
// BAŞLAŞMA
// ============================================================================

pub fn init() {
    if MONITORS.initialized.swap(true, Ordering::SeqCst) {
        return;
    }
    
    // Monitörleri kayıt et
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
