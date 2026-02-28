//! # Zamanlayıcı Sağlık Monitörü
//!
//! Görev zamanlayıcısı sağlığını, çalıştırma kuyruğunu ve görev sızıntılarını izler.
//! Zombie görev birikimi ve çalıştırılabilir görev taşmasını tespit eder.
//!
//! ## Zamanlayıcı (Scheduler) Nedir?
//!
//! Zamanlayıcı, hangi görevin (task/thread) CPU'da çalışacağına karar veren
//! kernel bileşenidir. echOS'ta öncelik tabanlı çoklu görev yönetimi kullanılır.
//!
//! ```
//! CPU Zamanı Paylaşımı:
//!
//! Tick 0: [Görev A çalışıyor]
//! Tick 1: [Görev A çalışıyor]
//! Tick 2: [Görev B çalışıyor]  <-- Zamanlayıcı geçiş yaptı
//! Tick 3: [Görev C çalışıyor]
//! Tick 4: [Görev A çalışıyor]  <-- Tekrar A'nın sırası
//! ```
//!
//! ## Zombie Görev Nedir?
//!
//! Çalışması biten ama henüz temizlenmemiş (reap edilmemiş) görevdir.
//! Ebeveyn görev wait() çağırmazsa, zombie birikir ve kaynak sızıntısı olur.
//!
//! ```
//! [Görev çalışıyor] --> [Görev bitti] --> [Zombie: Ebeveyn bekliyor]
//!                                                 |
//!                                          Ebeveyn wait() çağırdı
//!                                                 |
//!                                          [Görev tamamen temizlendi]
//! ```
//!
//! ## Açlık (Starvation) Nedir?
//!
//! Bir görev çalışma sırası yetersizliği nedeniyle uzun süre CPU alamazsa
//! "açlık" (starvation) yaşar. Öncelik tersine çevirme (priority inversion)
//! veya aşırı iş yükü bu duruma yol açabilir.
//!
//! ## Sağlık Eşikleri
//!
//! ```
//! Zombie görev (zombie_count) > 50     --> TaskLeak hatası raporlanır
//! Çalışabilir görev (runnable) > 1000  --> Starvation hatası raporlanır
//!
//! task_leaks > 10 veya starvation > 5  --> Degraded
//! task_leaks > 0  veya starvation > 0  --> Warning
//! diğer                                 --> Healthy
//! ```

use core::sync::atomic::{AtomicU32, AtomicUsize, AtomicBool, Ordering};

use crate::fault::{Fault, FaultSource, FaultType, HealthStatus, ModuleHealth};

// SchedulerMonitor: Zamanlayıcı anomalilerini izler.
// queue_anomalies alanı gelecekte çalıştırma kuyruğu
// bütünlük kontrolleri için ayrılmıştır.
pub struct SchedulerMonitor {
    /// Görev sızıntısı sayısı
    task_leaks: AtomicU32,
    /// Açlık (starvation) olay sayısı
    starvation_events: AtomicU32,
    /// Çalıştırma kuyruğu anomali sayısı
    queue_anomalies: AtomicU32,
    /// Son kontrol zaman damgası
    last_check: AtomicUsize,
    /// Monitör etkin mi?
    enabled: AtomicBool,
}

impl SchedulerMonitor {
    pub const fn new() -> Self {
        Self {
            task_leaks: AtomicU32::new(0),
            starvation_events: AtomicU32::new(0),
            queue_anomalies: AtomicU32::new(0),
            last_check: AtomicUsize::new(0),
            enabled: AtomicBool::new(true),
        }
    }

    /// Görev sızıntısı kaydeder
    pub fn record_task_leak(&self) {
        self.task_leaks.fetch_add(1, Ordering::SeqCst);
    }

    /// Açlık (starvation) olayı kaydeder
    pub fn record_starvation(&self) {
        self.starvation_events.fetch_add(1, Ordering::SeqCst);
    }

    /// Zamanlayıcı sağlığını kontrol eder — zombie birikimi ve yüksek kuyruk
    fn check_scheduler(&self) -> Option<Fault> {
        // Zamanlayıcı istatistiklerini al: zombie sayısı, çalışabilir görev sayısı vb.
        let stats = crate::task::scheduler::get_stats();

        // Zombie birikimsini kontrol et
        // 50'den fazla zombie → görevler temizlenmiyor, kaynak sızıntısı var
        if stats.zombie_count > 50 {
            self.record_task_leak();
            return Some(Fault::new(
                FaultSource::Scheduler,
                FaultType::TaskLeak,
                &alloc::format!("High zombie task count: {}", stats.zombie_count)
            ));
        }

        // Çalıştırma kuyruğu sorunlarını kontrol et
        // 1000'den fazla çalışabilir görev → CPU yetersiz veya görevler bloklanıyor
        if stats.runnable_tasks > 1000 {
            return Some(Fault::new(
                FaultSource::Scheduler,
                FaultType::Starvation,
                &alloc::format!("High runnable task count: {}", stats.runnable_tasks)
            ));
        }

        None
    }
}

impl super::HealthMonitor for SchedulerMonitor {
    fn name(&self) -> &'static str {
        "scheduler"
    }

    fn check(&self) -> Option<Fault> {
        // Devre dışıysa erken dön
        if !self.enabled.load(Ordering::SeqCst) {
            return None;
        }

        // Son kontrol zamanını güncelle
        self.last_check.store(
            crate::task::scheduler::get_ticks(),
            Ordering::SeqCst
        );

        self.check_scheduler()
    }

    // health(): Biriken sızıntı ve açlık sayılarına göre durum belirle.
    fn health(&self) -> HealthStatus {
        let leaks = self.task_leaks.load(Ordering::SeqCst);
        let starvation = self.starvation_events.load(Ordering::SeqCst);

        if leaks > 10 || starvation > 5 {
            HealthStatus::Degraded
        } else if leaks > 0 || starvation > 0 {
            HealthStatus::Warning
        } else {
            HealthStatus::Healthy
        }
    }

    // is_critical: true  → Zamanlayıcı çökerse sistem tamamen durur.
    // can_restart: false → Zamanlayıcı yeniden başlatılamaz; görev tablosu zarar görür.
    // has_fallback: false → Yedek zamanlayıcı mekanizması yoktur.
    fn module_health(&self) -> ModuleHealth {
        ModuleHealth {
            name: self.name(),
            status: self.health(),
            fault_count: self.task_leaks.load(Ordering::SeqCst) + self.starvation_events.load(Ordering::SeqCst),
            recovery_count: 0,
            last_fault_tick: self.last_check.load(Ordering::SeqCst),
            uptime_ticks: crate::task::scheduler::get_ticks(),
            is_critical: true,
            can_restart: false,
            has_fallback: false,
        }
    }

    // reset(): Tüm sayaçları sıfırla.
    // queue_anomalies de sıfırlanır — gelecekte kullanım için hazır tutulur.
    fn reset(&self) {
        self.task_leaks.store(0, Ordering::SeqCst);
        self.starvation_events.store(0, Ordering::SeqCst);
        self.queue_anomalies.store(0, Ordering::SeqCst);
    }
}

pub static SCHEDULER_MONITOR: SchedulerMonitor = SchedulerMonitor::new();
