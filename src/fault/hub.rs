//! # Hata Merkezi (Fault Hub)
//!
//! Merkezi hata toplama, yönlendirme ve yönetim modulu.
//! Tüm modüllerden gelen hataları tek noktada toplar ve kurtarma motorunu tetikler.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::boxed::Box;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

use super::{Fault, FaultId, FaultSource, FaultType, ModuleHealth, HealthStatus, FAULT_STATE};
use super::severity::{Severity, RecoveryResult, RecommendedAction};
use super::recovery::RecoveryEngine;

// ============================================================================
// HATA MERKEİ YAPISI
// ============================================================================

/// Merkezi hata yönetim merkezi (hub)
pub struct FaultHub {
    /// Modül sağlık takibi — her modülün durumunu izler
    modules: Mutex<BTreeMap<&'static str, ModuleHealth>>,
    /// Kurtarma motoru (recovery engine)
    recovery_engine: Mutex<Option<RecoveryEngine>>,
    /// Kaynağa göre hata işleyicileri
    handlers: Mutex<BTreeMap<FaultSource, Box<dyn FaultHandler>>>,
    /// Merkez başlatıldı mı?
    initialized: AtomicBool,
    /// Başlangıçtan bu yana toplam hata sayısı
    fault_count: AtomicU64,
    /// Başarılı kurtarma sayısı
    recovery_success: AtomicU64,
    /// Başarısız kurtarma sayısı
    recovery_failure: AtomicU64,
    /// Mevcut sistem şiddet seviyesi
    current_severity: AtomicU32,
    /// Son sağlık kontrolü zaman damgası
    last_check: AtomicUsize,
}

/// Özel hata işleyiciler için trait (arayüz)
pub trait FaultHandler: Send + Sync {
    /// Modülün hatalarını kontrol eder
    fn check(&self) -> Option<Fault>;
    /// Modül sağlık durumunu döndürür
    fn health(&self) -> HealthStatus;
    /// Hatadan kurtarmayı dener
    fn recover(&self, fault: &Fault) -> RecoveryResult;
    /// Modül adını döndürür
    fn name(&self) -> &'static str;
}

impl FaultHub {
    pub const fn new() -> Self {
        Self {
            modules: Mutex::new(BTreeMap::new()),
            recovery_engine: Mutex::new(None),
            handlers: Mutex::new(BTreeMap::new()),
            initialized: AtomicBool::new(false),
            fault_count: AtomicU64::new(0),
            recovery_success: AtomicU64::new(0),
            recovery_failure: AtomicU64::new(0),
            current_severity: AtomicU32::new(Severity::Normal as u32),
            last_check: AtomicUsize::new(0),
        }
    }
    
    /// Hata merkezini başlatır ve çekirdek modülleri kayıt eder
    pub fn init(&self) {
        if self.initialized.swap(true, Ordering::SeqCst) {
            return;
        }
        
        // Çekirdek modülleri kayıt et
        self.register_module(ModuleHealth::new("memory", true, false, false));
        self.register_module(ModuleHealth::new("cpu", true, false, false));
        self.register_module(ModuleHealth::new("smp", true, false, false));
        self.register_module(ModuleHealth::new("interrupts", true, false, false));
        self.register_module(ModuleHealth::new("scheduler", true, false, false));
        self.register_module(ModuleHealth::new("drivers", false, true, true));
        self.register_module(ModuleHealth::new("filesystem", false, true, true));
        self.register_module(ModuleHealth::new("network", false, true, true));
        self.register_module(ModuleHealth::new("security", true, false, false));
        self.register_module(ModuleHealth::new("acpi", false, true, false));
        
        // Kurtarma motorunu başlat
        *self.recovery_engine.lock() = Some(RecoveryEngine::new());
        
        crate::serial_println!("[FAULT_HUB] Initialized with {} modules", 
            self.modules.lock().len());
    }
    
    /// Sağlık takibi için modül kaydeder
    pub fn register_module(&self, health: ModuleHealth) {
        self.modules.lock().insert(health.name, health);
    }
    
    /// Özel bir hata işleyici kaydeder
    pub fn register_handler(&self, source: FaultSource, handler: Box<dyn FaultHandler>) {
        self.handlers.lock().insert(source, handler);
    }
    
    /// Hata bildirir, modül sağlığını günceller ve kurtarmayı başlatır
    pub fn report_fault(&self, fault: Fault) -> FaultId {
        let id = fault.id;
        let severity = fault.severity;
        let source = fault.source;
        
        self.fault_count.fetch_add(1, Ordering::SeqCst);
        
        // Update module health
        if let Some(module) = self.modules.lock().get_mut(&match source {
            FaultSource::Memory => "memory",
            FaultSource::Cpu => "cpu",
            FaultSource::Smp => "smp",
            FaultSource::Interrupt => "interrupts",
            FaultSource::Scheduler => "scheduler",
            FaultSource::Driver => "drivers",
            FaultSource::Filesystem => "filesystem",
            FaultSource::Network => "network",
            FaultSource::Security => "security",
            FaultSource::Acpi => "acpi",
            _ => "unknown",
        }) {
            module.record_fault();
            
            // Hata sayısına göre durumu güncelle
            if module.fault_count > 5 {
                module.update_status(HealthStatus::Failed);
            } else if module.fault_count > 2 {
                module.update_status(HealthStatus::Degraded);
            } else if module.fault_count > 0 {
                module.update_status(HealthStatus::Warning);
            }
        }
        
        // Global şiddet seviyesini güncelle
        self.update_severity(severity);
        
        // Hatayı günlülere yaz
        crate::serial_println!(
            "[FAULT_HUB] Fault #{:?}: {:?}/{:?} - {} (severity: {:?})",
            id, source, fault.fault_type, fault.message, severity
        );
        
        // Global duruma kayıt et
        FAULT_STATE.record_fault(&fault);
        
        // Kurtarmayı dene
        if FAULT_STATE.auto_recovery.load(Ordering::SeqCst) {
            self.attempt_recovery(&fault);
        }
        
        id
    }
    
    /// Bir hata için kurtarma denemesi yapar
    pub fn attempt_recovery(&self, fault: &Fault) -> RecoveryResult {
        let result = if let Some(engine) = self.recovery_engine.lock().as_ref() {
            engine.recover(fault)
        } else {
            RecoveryResult::Failed
        };
        
        // İstatistikleri güncelle
        if result.is_success() {
            self.recovery_success.fetch_add(1, Ordering::SeqCst);
        } else {
            self.recovery_failure.fetch_add(1, Ordering::SeqCst);
        }
        
        // Modül sağlığını güncelle
        if let Some(module) = self.modules.lock().get_mut(&match fault.source {
            FaultSource::Memory => "memory",
            FaultSource::Cpu => "cpu",
            FaultSource::Smp => "smp",
            FaultSource::Interrupt => "interrupts",
            FaultSource::Scheduler => "scheduler",
            FaultSource::Driver => "drivers",
            FaultSource::Filesystem => "filesystem",
            FaultSource::Network => "network",
            FaultSource::Security => "security",
            FaultSource::Acpi => "acpi",
            _ => "unknown",
        }) {
            module.record_recovery(result.is_success());
        }
        
        crate::serial_println!(
            "[FAULT_HUB] Recovery result for fault #{:?}: {:?}",
            fault.id, result
        );
        
        result
    }
    
    /// Tüm modülleri hata açısından kontrol eder
    pub fn check_all(&self) {
        if !self.initialized.load(Ordering::SeqCst) {
            return;
        }
        
        let current_tick = crate::task::scheduler::get_ticks();
        self.last_check.store(current_tick, Ordering::SeqCst);
        
        // Kayıtlı her işleyiciyi kontrol et
        for (source, handler) in self.handlers.lock().iter() {
            if let Some(fault) = handler.check() {
                self.report_fault(fault);
            }
        }
    }
    
    /// Belirtilen modülün sağlık durumunu döndürür
    pub fn get_health(&self, module: &str) -> Option<ModuleHealth> {
        self.modules.lock().get(module).cloned()
    }
    
    /// Tüm modül sağlık durumlarını döndürür
    pub fn get_all_health(&self) -> Vec<ModuleHealth> {
        self.modules.lock().values().cloned().collect()
    }
    
    /// Mevcut şiddet seviyesini günceller (yalnızca artış yönünde)
    fn update_severity(&self, severity: Severity) {
        let current = Severity::from(self.current_severity.load(Ordering::SeqCst) as u8);
        if severity > current {
            self.current_severity.store(severity as u32, Ordering::SeqCst);
        }
    }
    
    /// Mevcut sistem şiddet seviyesini döndürür
    pub fn current_severity(&self) -> Severity {
        Severity::from(self.current_severity.load(Ordering::SeqCst) as u8)
    }
    
    /// Hata istatistiklerini döndürür
    pub fn stats(&self) -> HubStats {
        HubStats {
            total_faults: self.fault_count.load(Ordering::SeqCst),
            recovery_success: self.recovery_success.load(Ordering::SeqCst),
            recovery_failure: self.recovery_failure.load(Ordering::SeqCst),
            current_severity: self.current_severity(),
            module_count: self.modules.lock().len(),
        }
    }
    
    /// Modül sağlık durumunu sıfırlar
    pub fn reset_module(&self, module: &str) -> bool {
        if let Some(health) = self.modules.lock().get_mut(module) {
            health.status = HealthStatus::Healthy;
            health.fault_count = 0;
            health.last_fault_tick = 0;
            crate::serial_println!("[FAULT_HUB] Reset health for module: {}", module);
            true
        } else {
            false
        }
    }
    
    /// Sistemin acil durum modunda olup olmadığını kontrol eder
    pub fn is_emergency(&self) -> bool {
        self.current_severity() == Severity::Emergency
    }
    
    /// Mevcut sistem durumu için önerilen eylemi döndürür
    pub fn recommended_action(&self) -> RecommendedAction {
        self.current_severity().recommended_action()
    }
}

impl Severity {
    fn from(value: u8) -> Self {
        match value {
            0 => Severity::Normal,
            1 => Severity::Warning,
            2 => Severity::Degraded,
            3 => Severity::Critical,
            4 => Severity::Emergency,
            _ => Severity::Emergency,
        }
    }
}

#[derive(Clone, Debug)]
pub struct HubStats {
    pub total_faults: u64,
    pub recovery_success: u64,
    pub recovery_failure: u64,
    pub current_severity: Severity,
    pub module_count: usize,
}

// ============================================================================
// GLOBAL MERKEZ ÖRNEKİ
// ============================================================================

lazy_static::lazy_static! {
    pub static ref FAULT_HUB: FaultHub = FaultHub::new();
}

// ============================================================================
// GENEL (PUBLIC) API
// ============================================================================

/// Hata merkezini başlatır
pub fn init() {
    FAULT_HUB.init();
}

/// Hata bildirir
pub fn report(source: FaultSource, fault_type: FaultType, message: &str) -> FaultId {
    let fault = Fault::new(source, fault_type, message);
    FAULT_HUB.report_fault(fault)
}

/// Tüm modülleri kontrol eder
pub fn check_all() {
    FAULT_HUB.check_all();
}

/// Modül sağlığını döndürür
pub fn health(module: &str) -> Option<ModuleHealth> {
    FAULT_HUB.get_health(module)
}

/// Tüm sağlık durumlarını döndürür
pub fn all_health() -> Vec<ModuleHealth> {
    FAULT_HUB.get_all_health()
}

/// Kurtarmayı dener
pub fn recover(fault: &Fault) -> RecoveryResult {
    FAULT_HUB.attempt_recovery(fault)
}

/// Merkez istatistiklerini döndürür
pub fn stats() -> HubStats {
    FAULT_HUB.stats()
}
