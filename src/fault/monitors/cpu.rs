//! # CPU Sağlık Monitörü
//!
//! CPU sağlığını, askıya alınmış CPU'ları ve ısıl olayları izler.
//! SMP ortamında her CPU'nun aktifliğini takip eder.
//!
//! ## CPU Sağlık Kontrol Akışı
//!
//! ```
//! CpuMonitor::check()
//!      |
//!      +-- check_cpu_health()
//!      |       |
//!      |    online_cpu_count() < cpu_count ?
//!      |       |-- EVET --> Fault::CpuHung
//!      |       |-- HAYIR --> None
//!      |
//!      +-- check_thermal()
//!              |
//!           ACPI termal zone kontrol (gelecekte)
//! ```
//!
//! ## CPU Durumu Seviyeleri
//!
//! ```
//! hung_cpus > 0  VEYA  thermal > 2  -->  Degraded
//! thermal > 0                        -->  Warning
//! diğer durum                        -->  Healthy
//! ```
//!
//! ## Atomik Sayaçlar Hakkında
//!
//! Atomik tipler (AtomicU32, AtomicUsize, AtomicBool), birden fazla CPU çekirdeğinin
//! aynı anda güvenle okuyup yazmasına olanak tanır — kilit (mutex) gerektirmez.
//! Bu, kesme işleyicilerinden (interrupt handler) güvenle çağrılabilmeleri için kritiktir.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use crate::fault::{Fault, FaultSource, FaultType, HealthStatus, ModuleHealth};

// ============================================================================
// CPU MONİTÖR DURUMU
// ============================================================================
//
// CpuMonitor: Tüm alanlar atomik tipte — lock-free (kilit gerektirmez).
// Bu yapı 'static ömürlü global değişken olarak kullanılır;
// bu nedenle const fn new() ile başlatılabilmeli ve iç mutabilite (interior
// mutability) için atomik tipler kullanılmalıdır.

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
    // const fn: Derleme zamanı sabiti olarak kullanılabilir.
    // pub static CPU_MONITOR: CpuMonitor = CpuMonitor::new(); satırını mümkün kılar.
    pub const fn new() -> Self {
        Self {
            cpu_count: AtomicU32::new(1),
            hung_cpus: AtomicU32::new(0),
            thermal_events: AtomicU32::new(0),
            last_check: AtomicUsize::new(0),
            enabled: AtomicBool::new(true),
        }
    }

    /// CPU sağlığını kontrol eder — çevrimdışı CPU'ları tespit eder
    fn check_cpu_health(&self) -> Option<Fault> {
        // Beklenen toplam CPU sayısını oku (SeqCst: tüm çekirdekler aynı değeri görür)
        let cpu_count = self.cpu_count.load(Ordering::SeqCst);
        // Şu an çevrimiçi (aktif) olan CPU sayısını al
        let online = crate::cpu::smp::online_cpu_count();

        // Eğer aktif CPU sayısı beklenenin altındaysa, bazı CPU'lar çevrimdışı demektir
        if online < cpu_count {
            let offline = cpu_count - online;
            return Some(Fault::new(
                FaultSource::Cpu,
                FaultType::CpuHung,
                &alloc::format!("{} CPU(s) offline", offline),
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
        // store(): Atomik yazma; diğer çekirdekler bu değişikliği anında görür
        self.cpu_count.store(count, Ordering::SeqCst);
    }

    /// Isıl olay kaydeder
    pub fn record_thermal_event(&self) {
        // fetch_add(): Atomik artırma; eski değeri döndürür, yeni değeri yazar
        self.thermal_events.fetch_add(1, Ordering::SeqCst);
    }
}

// HealthMonitor trait implementasyonu — bu tür artık MonitorRegistry'ye kaydedilebilir.
impl super::HealthMonitor for CpuMonitor {
    fn name(&self) -> &'static str {
        "cpu"
    }

    fn check(&self) -> Option<Fault> {
        // Monitör devre dışıysa erken dön
        if !self.enabled.load(Ordering::SeqCst) {
            return None;
        }

        // Son kontrol zamanını şu anki tick değeriyle güncelle
        self.last_check
            .store(crate::task::scheduler::get_ticks(), Ordering::SeqCst);

        // CPU sağlığını kontrol et; hata varsa hemen döndür
        if let Some(fault) = self.check_cpu_health() {
            return Some(fault);
        }

        // Isıl durumu kontrol et
        if let Some(fault) = self.check_thermal() {
            return Some(fault);
        }

        None
    }

    // health(): Sayaç değerlerine bakarak özet durum döndürür.
    // Bu metot gerçekten hata araştırmaz, sadece biriken sayaçları yorumlar.
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

    // module_health(): Dışarıya raporlanacak tam sağlık snapshot'ı oluşturur.
    // is_critical: true --> Bu modül çöküyorsa sistem panic yapabilir.
    // can_restart: false --> CPU yeniden başlatılamaz; donanım müdahalesi gerekir.
    // has_fallback: false --> CPU için yedek (fallback) mekanizması yoktur.
    fn module_health(&self) -> ModuleHealth {
        ModuleHealth {
            name: self.name(),
            status: self.health(),
            fault_count: self.hung_cpus.load(Ordering::SeqCst)
                + self.thermal_events.load(Ordering::SeqCst),
            recovery_count: 0,
            last_fault_tick: self.last_check.load(Ordering::SeqCst),
            uptime_ticks: crate::task::scheduler::get_ticks(),
            is_critical: true,
            can_restart: false,
            has_fallback: false,
        }
    }

    // reset(): Sayaçları sıfırlar — kurtarma sonrası veya test amaçlı.
    fn reset(&self) {
        self.hung_cpus.store(0, Ordering::SeqCst);
        self.thermal_events.store(0, Ordering::SeqCst);
    }
}

// Global static örnek: Program başından sonuna kadar bellekte yaşar.
// 'static ömür, &'static dyn HealthMonitor ile MonitorRegistry'ye kaydedilmeyi sağlar.
pub static CPU_MONITOR: CpuMonitor = CpuMonitor::new();
