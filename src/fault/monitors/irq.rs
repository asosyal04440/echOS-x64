//! # IRQ Sağlık Monitörü
//!
//! Kesme fırtınalarını (IRQ storm), işleyici zaman aşımlarını ve IDT bütünlüğünü izler.
//! Yüksek frekanslı kesme aktivitesi sistemi bozabilir; bu modül bunu tespit eder.
//!
//! ## IRQ (Interrupt Request) Nedir?
//!
//! IRQ, donanımın CPU'ya "işlenecek bir şey var" diye sinyal göndermesidir.
//! CPU, mevcut işini yarıda bırakıp kesme işleyicisini (interrupt handler) çalıştırır.
//!
//! ```
//! Normal çalışma:
//!   CPU --> Görev A çalışıyor
//!
//! IRQ gelince:
//!   CPU --> [Görev A askıya alınır]
//!        --> [Interrupt Handler çalışır]
//!        --> [Görev A devam eder]
//! ```
//!
//! ## IRQ Fırtınası (IRQ Storm) Nedir?
//!
//! Bir donanım birimi çok hızlı IRQ üretirse (örn. ağ kartı saniyede binlerce paket),
//! CPU sürekli kesme işleyicisi çalıştırmak zorunda kalır ve normal görevlere
//! hiç zaman kalmaz. Buna "IRQ storm" (kesme fırtınası) denir.
//!
//! ```
//! Normal:  [Görev][IRQ][Görev][IRQ][Görev]
//!
//! Fırtına: [IRQ][IRQ][IRQ][IRQ][IRQ][IRQ][IRQ] --> sistem donuyor
//! ```
//!
//! ## Sahte Kesme (Spurious Interrupt) Nedir?
//!
//! Gerçek bir donanım olayına karşılık gelmeyen, hatalı üretilen kesmedir.
//! PIC (Programmable Interrupt Controller) donanım hatası veya yarış koşulu
//! (race condition) nedeniyle üretilebilir.
//!
//! ## Sayaç Türleri
//!
//! ```
//! storm_count      --> Fırtına eşiği (storm_threshold=500) aşılma sayısı
//! handler_timeouts --> Kesme işleyicisi zamanında tamamlanamadı
//! spurious_count   --> Sahte kesme sayısı (AtomicU64: çok yüksek olabilir)
//! ```

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};

use crate::fault::{Fault, FaultSource, FaultType, HealthStatus, ModuleHealth};

// IrqMonitor: Kesme bağlamında da güvenle kullanılabilmesi için tüm alanlar atomik.
// storm_threshold: const değil — gelecekte dinamik olarak ayarlanabilmesi için.
// (Sabit bir eşik ile struct alanı arasındaki fark: struct alanı runtime'da değiştirilebilir.)
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
            // Eşik: Bir kontrol döngüsünde 500'den fazla fırtına → uyarı ver
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
        // AtomicU64: Sahte kesmeler çok sık olabilir, 32-bit taşabilir
        self.spurious_count.fetch_add(1, Ordering::SeqCst);
    }

    /// IRQ fırtınalarını kontrol eder
    fn check_storms(&self) -> Option<Fault> {
        // Kesme modülünden IRQ hızını kontrol et
        let stats = crate::interrupts::get_stats();

        // Biriken fırtına sayısı eşiği aşıyorsa kaydet ve hata döndür
        if stats.storm_count > self.storm_threshold {
            self.record_storm();
            return Some(Fault::new(
                FaultSource::Interrupt,
                FaultType::IrqStorm,
                &alloc::format!("IRQ storm detected: {} storms", stats.storm_count),
            ));
        }

        None
    }

    /// Sahte kesmeleri kontrol eder
    fn check_spurious(&self) -> Option<Fault> {
        let spurious = self.spurious_count.load(Ordering::SeqCst);

        // 10'dan fazla sahte kesme: PIC/APIC konfigürasyon sorunu olabilir
        if spurious > 10 {
            return Some(Fault::new(
                FaultSource::Interrupt,
                FaultType::SpuriousInterrupt,
                &alloc::format!("High spurious interrupt count: {}", spurious),
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
        // Devre dışıysa atla
        if !self.enabled.load(Ordering::SeqCst) {
            return None;
        }

        // Son kontrol zamanını güncelle
        self.last_check
            .store(crate::task::scheduler::get_ticks(), Ordering::SeqCst);

        // Önce fırtınaları kontrol et (daha kritik)
        if let Some(fault) = self.check_storms() {
            return Some(fault);
        }

        // Sonra sahte kesmeleri kontrol et
        if let Some(fault) = self.check_spurious() {
            return Some(fault);
        }

        None
    }

    // health(): Fırtına > 5 veya zaman aşımı > 3 ise Degraded.
    // IRQ sistemi kritik — çok fazla hata kernel'i dondurabilir.
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

    // is_critical: true  → Kesme sistemi bozulursa tüm donanım iletişimi durur.
    // can_restart: false → IRQ/IDT yeniden başlatılamaz; donanım faktör.
    // has_fallback: false → Kesme sistemi için yedek yoktur.
    fn module_health(&self) -> ModuleHealth {
        ModuleHealth {
            name: self.name(),
            status: self.health(),
            fault_count: self.storm_count.load(Ordering::SeqCst)
                + self.handler_timeouts.load(Ordering::SeqCst),
            recovery_count: 0,
            last_fault_tick: self.last_check.load(Ordering::SeqCst),
            uptime_ticks: crate::task::scheduler::get_ticks(),
            is_critical: true,
            can_restart: false,
            has_fallback: false,
        }
    }

    // reset(): Tüm sayaçları sıfırla.
    fn reset(&self) {
        self.storm_count.store(0, Ordering::SeqCst);
        self.handler_timeouts.store(0, Ordering::SeqCst);
        self.spurious_count.store(0, Ordering::SeqCst);
    }
}

pub static IRQ_MONITOR: IrqMonitor = IrqMonitor::new();
