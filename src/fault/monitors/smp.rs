//! # SMP Sağlık Monitörü
//!
//! SMP (Simetrik Çoklu İşlemci) durumunu, AP başlatılmasını ve TLB atışını izler.
//! AP başlatı hataları ve TLB shootdown zaman aşımlarını erken tespit eder.
//!
//! ## SMP (Symmetric Multi-Processing) Nedir?
//!
//! Birden fazla CPU çekirdeğinin aynı bellek alanını paylaşarak çalıştığı mimaridir.
//! echOS, x86-64 SMP desteği sunar.
//!
//! ```
//! SMP Sistemi:
//!
//! +--------+  +--------+  +--------+  +--------+
//! | CPU 0  |  | CPU 1  |  | CPU 2  |  | CPU 3  |
//! | (BSP)  |  | (AP)   |  | (AP)   |  | (AP)   |
//! +---+----+  +---+----+  +---+----+  +---+----+
//!     |            |            |            |
//!     +------------+------------+------------+
//!                         |
//!                  Paylaşılan RAM
//!
//! BSP = Bootstrap Processor (ilk açılan, kernel'i başlatan)
//! AP  = Application Processor (BSP tarafından uyandırılan ek çekirdekler)
//! ```
//!
//! ## AP Başlatma Süreci
//!
//! ```
//! BSP kernel'i başlatır
//!      |
//!      v
//! BSP, LAPIC üzerinden AP'lere INIT+SIPI IPI gönderir
//!      |
//!      v
//! AP trampolin kodunu çalıştırır (16-bit -> 64-bit geçiş)
//!      |
//!      v
//! AP kernel yığınını hazırlar, GDT/IDT kurar
//!      |
//!      v
//! AP online duruma geçer --> online_cpu_count() artar
//! ```
//!
//! ## TLB Shootdown Nedir?
//!
//! Bir CPU sayfa tablosunu değiştirdiğinde, diğer CPU'ların TLB (Translation
//! Lookaside Buffer) önbelleklerini geçersiz kılması gerekir.
//! Bu, IPI (Inter-Processor Interrupt) ile yapılır.
//!
//! ```
//! CPU 0 sayfa tablosunu değiştirir
//!      |
//!      v
//! CPU 0, diğer CPU'lara "TLB flush" IPI gönderir
//!      |
//!      v
//! Diğer CPU'lar TLB'yi temizler ve onay verir
//!      |
//!      v
//! CPU 0 devam eder
//!
//! Zaman aşımı: Onay gelmezse → TLB shootdown timeout hatası
//! ```
//!
//! ## Sağlık Eşikleri
//!
//! ```
//! ap_fail > 2 veya tlb > 5  --> Degraded
//! ap_fail > 0 veya tlb > 0  --> Warning
//! diğer                      --> Healthy
//! ```

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use crate::fault::{Fault, FaultSource, FaultType, HealthStatus, ModuleHealth};

// SmpMonitor: Çok çekirdekli koordinasyon sorunlarını izler.
// AP başlatma ve TLB shootdown, SMP doğruluğu için kritiktir.
pub struct SmpMonitor {
    /// AP (Application Processor - Uygulama İşlemcisi) başlatı hataları
    ap_failures: AtomicU32,
    /// TLB shootdown zaman aşımı sayısı
    tlb_timeouts: AtomicU32,
    /// Son kontrol zaman damgası
    last_check: AtomicUsize,
    /// Monitör etkin mi?
    enabled: AtomicBool,
}

impl SmpMonitor {
    pub const fn new() -> Self {
        Self {
            ap_failures: AtomicU32::new(0),
            tlb_timeouts: AtomicU32::new(0),
            last_check: AtomicUsize::new(0),
            enabled: AtomicBool::new(true),
        }
    }

    /// AP başlatı hatası kaydeder
    pub fn record_ap_failure(&self) {
        // AP başlatılamazsa, bazı CPU çekirdekleri kullanılamaz hale gelir
        self.ap_failures.fetch_add(1, Ordering::SeqCst);
    }

    /// TLB shootdown zaman aşımı kaydeder
    pub fn record_tlb_timeout(&self) {
        // TLB tutarsızlığı → sayfalar yanlış adrese eşlenmiş olabilir
        self.tlb_timeouts.fetch_add(1, Ordering::SeqCst);
    }

    /// SMP sağlığını kontrol eder — çevrimdışı CPU'ları tespit eder
    fn check_smp(&self) -> Option<Fault> {
        // Topoloji bilgisinden beklenen mantıksal CPU sayısını al
        let expected = crate::cpu::CPU_INFO.lock().topology.logical_count;
        // Gerçekte çevrimiçi olan CPU sayısını al
        let online = crate::cpu::smp::online_cpu_count();

        // Eğer çevrimiçi sayı beklenenin altındaysa ve çok çekirdekli sistemdeyiz
        // → Bazı AP'ler başlatılamadı
        if online < expected && expected > 1 {
            return Some(Fault::new(
                FaultSource::Smp,
                FaultType::ApStartupFailed,
                &alloc::format!("{} of {} CPUs online", online, expected),
            ));
        }

        None
    }
}

impl super::HealthMonitor for SmpMonitor {
    fn name(&self) -> &'static str {
        "smp"
    }

    fn check(&self) -> Option<Fault> {
        // Devre dışıysa kontrol yapma
        if !self.enabled.load(Ordering::SeqCst) {
            return None;
        }

        // Son kontrol zamanını güncelle
        self.last_check
            .store(crate::task::scheduler::get_ticks(), Ordering::SeqCst);

        self.check_smp()
    }

    // health(): AP hata ve TLB zaman aşımı sayılarına göre durum döndür.
    fn health(&self) -> HealthStatus {
        let ap_fail = self.ap_failures.load(Ordering::SeqCst);
        let tlb = self.tlb_timeouts.load(Ordering::SeqCst);

        if ap_fail > 2 || tlb > 5 {
            HealthStatus::Degraded
        } else if ap_fail > 0 || tlb > 0 {
            HealthStatus::Warning
        } else {
            HealthStatus::Healthy
        }
    }

    // is_critical: true  → SMP hatası performansı ciddi düşürür veya sistem kararsızlaşır.
    // can_restart: false → CPU'lar yazılımla yeniden başlatılamaz.
    // has_fallback: false → Tek çekirdekli modda devam mümkün değil (bu izleme dahilinde).
    fn module_health(&self) -> ModuleHealth {
        ModuleHealth {
            name: self.name(),
            status: self.health(),
            fault_count: self.ap_failures.load(Ordering::SeqCst)
                + self.tlb_timeouts.load(Ordering::SeqCst),
            recovery_count: 0,
            last_fault_tick: self.last_check.load(Ordering::SeqCst),
            uptime_ticks: crate::task::scheduler::get_ticks(),
            is_critical: true,
            can_restart: false,
            has_fallback: false,
        }
    }

    // reset(): Sayaçları sıfırla.
    fn reset(&self) {
        self.ap_failures.store(0, Ordering::SeqCst);
        self.tlb_timeouts.store(0, Ordering::SeqCst);
    }
}

pub static SMP_MONITOR: SmpMonitor = SmpMonitor::new();
