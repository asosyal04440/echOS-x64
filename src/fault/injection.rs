//! # Hata Enjeksiyonu (Fault Injection)
//!
//! Hata enjeksiyonu ve kurtarma doğrulaması için test çerçevesi.
//! Yalnızca debug (hata ayıklama) derlemelerinde kullanılabilir.

#[cfg(debug_assertions)]
use alloc::string::String;
#[cfg(debug_assertions)]
use alloc::vec::Vec;

use crate::fault::{Fault, FaultSource, FaultType};

// ============================================================================
// HATA ENJEKSİYONU (YALNİZCA DEBUG)
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
    
    /// Hata enjeksiyonunu etkinleştirir/devre dışı bırakır
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, core::sync::atomic::Ordering::SeqCst);
    }
    
    /// Belirtilen kaynaktan belirtilen türde bir hata enjekte eder
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
    
    /// Bellek hatası enjekte eder (heap bozulması)
    pub fn inject_memory_fault(&self) {
        self.inject(
            FaultSource::Memory,
            FaultType::HeapCorruption,
            "Test amacıyla heap bozulması enjekte edildi"
        );
    }
    
    /// Bellek yetersizliği (OOM) hatası enjekte eder
    pub fn inject_oom(&self) {
        self.inject(
            FaultSource::Memory,
            FaultType::OutOfMemory,
            "Test amacıyla OOM enjekte edildi"
        );
    }
    
    /// Aygıt sürücsü hatası enjekte eder
    pub fn inject_driver_fault(&self) {
        self.inject(
            FaultSource::Driver,
            FaultType::DeviceTimeout,
            "Test amacıyla cihaz zaman aşımı enjekte edildi"
        );
    }
    
    /// Zamanlayıcı hatası enjekte eder
    pub fn inject_scheduler_fault(&self) {
        self.inject(
            FaultSource::Scheduler,
            FaultType::TaskLeak,
            "Test amacıyla görev sızıntısı enjekte edildi"
        );
    }
    
    /// Toplam enjeksiyon sayısını döndürür
    pub fn count(&self) -> u64 {
        self.injection_count.load(core::sync::atomic::Ordering::SeqCst)
    }
}

#[cfg(debug_assertions)]
lazy_static::lazy_static! {
    pub static ref FAULT_INJECTOR: FaultInjector = FaultInjector::new();
}

// ============================================================================
// TEST SENARYOLARI
// ============================================================================

#[cfg(debug_assertions)]
pub fn run_test_scenarios() {
    crate::serial_println!("[FAULT_INJECT] Hata enjeksiyonu test senaryoları çalıştırılıyor");
    
    // Enjeksiyonu etkinleştir
    FAULT_INJECTOR.set_enabled(true);
    
    // Test 1: Bellek hatası
    crate::serial_println!("[FAULT_INJECT] Test 1: Bellek hatası");
    FAULT_INJECTOR.inject_oom();
    
    // Test 2: Sürücsü hatası
    crate::serial_println!("[FAULT_INJECT] Test 2: Sürücsü hatası");
    FAULT_INJECTOR.inject_driver_fault();
    
    // Test 3: Zamanlayıcı hatası
    crate::serial_println!("[FAULT_INJECT] Test 3: Zamanlayıcı hatası");
    FAULT_INJECTOR.inject_scheduler_fault();
    
    // Sonuçları raporla
    let stats = crate::fault::get_stats();
    crate::serial_println!(
        "[FAULT_INJECT] Test tamamlandı: {} hata enjekte edildi, {} kurtarma",
        FAULT_INJECTOR.count(),
        stats.total_recoveries
    );
    
    // Enjeksiyonu devre dışı bırak
    FAULT_INJECTOR.set_enabled(false);
}

#[cfg(not(debug_assertions))]
pub fn run_test_scenarios() {
    // Release derlemesinde işlem yok
}
