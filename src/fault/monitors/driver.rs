//! # Sürücü Sağlık Monitörü
//!
//! Sürücü sağlığını, cihaz zaman aşımlarını ve DMA bütünlüğünü izler.
//! Donanım sürücülerinin güvenilirliğini sürekli denetler.
//!
//! ## Sürücü Hata Hiyerarşisi
//!
//! ```
//! Zaman Aşımı (timeout)  -->  Cihaz yanıt vermedi (geçici düzeltilebilir)
//!      |
//!      v
//! Cihaz Hatası (error)   -->  Cihaz hatalı yanıt verdi (sürücü reset gerekli)
//!      |
//!      v
//! DMA Bozulması           -->  Bellek bütünlüğü tehdit altında (KRİTİK)
//! ```
//!
//! ## Durum Geçişleri
//!
//! ```
//! Healthy   <-- timeout=0, error=0, dma=0
//!    |
//!    | timeout>0 veya error>0
//!    v
//! Warning   <-- timeout<=3, error<=5
//!    |
//!    | timeout>3 veya error>5
//!    v
//! Degraded  <-- timeout<=10, error<=20
//!    |
//!    | dma>0 veya timeout>10 veya error>20
//!    v
//! Failed    <-- Sürücü tamamen devre dışı
//! ```
//!
//! ## DMA (Direct Memory Access) Nedir?
//!
//! DMA, donanım aygıtlarının CPU'yu atlatarak doğrudan belleğe
//! yazabildiği bir mekanizmadır. DMA bozulması, CPU'nun bilgisi dışında
//! bellek içeriğinin değişmesi anlamına gelir — bu son derece tehlikelidir.
//!
//! ```
//! CPU  <-->  Bellek
//!        ^
//!        |  (normal yol)
//!
//! Aygıt --[DMA]--> Bellek
//!        (CPU devrede değil)
//! ```

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use crate::fault::{Fault, FaultSource, FaultType, HealthStatus, ModuleHealth};

// DriverMonitor: Tüm alanlar atomik; sürücü kesme işleyicilerinden güvenle çağrılabilir.
// Sürücüler çoğunlukla IRQ bağlamında çalıştığından, kilit almak kilitlenmeye yol açabilir.
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
        // fetch_add: Oku-artır-yaz işlemini tek atomik adımda yapar
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

    /// Sürücü sağlığını kontrol eder — zaman aşımı ve hata eşiklerini değerlendirir
    fn check_drivers(&self) -> Option<Fault> {
        // Mevcut sayaç değerlerini oku
        let timeouts = self.device_timeouts.load(Ordering::SeqCst);
        let errors = self.device_errors.load(Ordering::SeqCst);

        // 5'ten fazla zaman aşımı: Cihaz sürekli yanıt vermiyor — acil müdahale gerek
        if timeouts > 5 {
            return Some(Fault::new(
                FaultSource::Driver,
                FaultType::DeviceTimeout,
                &alloc::format!("Multiple device timeouts: {}", timeouts),
            ));
        }

        // 10'dan fazla hata: Sürücü kararsız — kurtarma veya devre dışı bırakma gerekli
        if errors > 10 {
            return Some(Fault::new(
                FaultSource::Driver,
                FaultType::DeviceError,
                &alloc::format!("Multiple device errors: {}", errors),
            ));
        }

        None
    }
}

// super::HealthMonitor: Üst modüldeki (monitors/mod.rs) trait'i implemente et.
impl super::HealthMonitor for DriverMonitor {
    fn name(&self) -> &'static str {
        "drivers"
    }

    fn check(&self) -> Option<Fault> {
        // Monitör devre dışıysa erken çık
        if !self.enabled.load(Ordering::SeqCst) {
            return None;
        }

        // Son kontrol zamanını güncelle
        self.last_check
            .store(crate::task::scheduler::get_ticks(), Ordering::SeqCst);

        self.check_drivers()
    }

    // health(): Üç sayacı birlikte değerlendirerek özet durum döndürür.
    // DMA hatası en ağır — hemen Failed döner.
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

    // is_critical: false → Sürücü çökmesi sistemi durdurmaz, devam edilebilir.
    // can_restart: true  → Sürücüler yazılımsal olarak yeniden başlatılabilir.
    // has_fallback: true → Bazı sürücülerin yedek/stub implementasyonu olabilir.
    fn module_health(&self) -> ModuleHealth {
        ModuleHealth {
            name: self.name(),
            status: self.health(),
            fault_count: self.device_timeouts.load(Ordering::SeqCst)
                + self.device_errors.load(Ordering::SeqCst),
            recovery_count: 0,
            last_fault_tick: self.last_check.load(Ordering::SeqCst),
            uptime_ticks: crate::task::scheduler::get_ticks(),
            is_critical: false,
            can_restart: true,
            has_fallback: true,
        }
    }

    // reset(): Tüm sayaçları sıfırla — sürücü başarıyla kurtarıldıktan sonra çağrılır.
    fn reset(&self) {
        self.device_timeouts.store(0, Ordering::SeqCst);
        self.device_errors.store(0, Ordering::SeqCst);
        self.dma_errors.store(0, Ordering::SeqCst);
    }
}

pub static DRIVER_MONITOR: DriverMonitor = DriverMonitor::new();
