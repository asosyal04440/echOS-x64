//! # Dosya Sistemi Sağlık Monitörü
//!
//! Dosya sistemi bütünlüğünü, G/Ç hatalarını ve disk alanını izler.
//! Metadata bozulması ve yüksek hata oranlarını erken tespit eder.
//!
//! ## Dosya Sistemi Hata Türleri
//!
//! ```
//! G/Ç Hatası (IoError)
//!   Disk okuma/yazma fiziksel olarak başarısız oldu.
//!   Genellikle geçici; yeniden deneme ile düzeltilebilir.
//!
//! Metadata Bozulması (MetadataCorruption)
//!   İnode, dizin girdisi veya superblock zarar gördü.
//!   Çok daha tehlikeli — fsck gerektirebilir.
//!
//! Disk Dolu (DiskFull)
//!   Ayrılabilir boş alan kalmadı.
//!   Yeni dosya oluşturma veya büyütme başarısız olur.
//! ```
//!
//! ## Dosya Sistemi Katmanları
//!
//! ```
//! Uygulama (kullanıcı alanı)
//!        |
//!        v
//! VFS (Virtual File System) — ortak arayüz
//!        |
//!        v
//! Dosya Sistemi Sürücüsü (ext4, fat32, vb.)
//!        |
//!        v
//! Blok Aygıt Katmanı
//!        |
//!        v
//! Disk / NVMe / Sanal Depolama
//! ```
//!
//! ## Sağlık Eşikleri
//!
//! ```
//! meta > 2            --> Failed   (ciddi bozulma)
//! meta > 0 | io > 20  --> Degraded (dikkat gerekli)
//! io > 5              --> Warning  (artan hata oranı)
//! diğer               --> Healthy
//! ```

use core::sync::atomic::{AtomicU32, AtomicUsize, AtomicBool, Ordering};

use crate::fault::{Fault, FaultSource, FaultType, HealthStatus, ModuleHealth};

// FsMonitor: Dosya sistemi hata istatistiklerini atomik sayaçlarla tutar.
// Atomik kullanımı: Dosya sistemi işlemleri farklı görevlerden/thread'lerden
// çağrılabilir; kilit olmadan güvenli erişim sağlar.
pub struct FsMonitor {
    /// G/Ç (I/O) hatası sayısı
    io_errors: AtomicU32,
    /// Metadata (st veri) hatası sayısı
    metadata_errors: AtomicU32,
    /// Disk doldu olay sayısı
    disk_full_events: AtomicU32,
    /// Son kontrol zaman damgası
    last_check: AtomicUsize,
    /// Monitör etkin mi?
    enabled: AtomicBool,
}

impl FsMonitor {
    pub const fn new() -> Self {
        Self {
            io_errors: AtomicU32::new(0),
            metadata_errors: AtomicU32::new(0),
            disk_full_events: AtomicU32::new(0),
            last_check: AtomicUsize::new(0),
            enabled: AtomicBool::new(true),
        }
    }

    /// G/Ç hatası kaydeder
    pub fn record_io_error(&self) {
        // fetch_add(1): Sayacı atomik olarak 1 artır
        self.io_errors.fetch_add(1, Ordering::SeqCst);
    }

    /// Metadata hatası kaydeder
    pub fn record_metadata_error(&self) {
        // Metadata hatası G/Ç hatasından çok daha ciddidir — ayrı sayılır
        self.metadata_errors.fetch_add(1, Ordering::SeqCst);
    }

    /// Disk doldu olayı kaydeder
    pub fn record_disk_full(&self) {
        self.disk_full_events.fetch_add(1, Ordering::SeqCst);
    }

    /// Dosya sistemi sağlığını kontrol eder — metadata ve G/Ç hatalarını değerlendirir
    fn check_fs(&self) -> Option<Fault> {
        let io = self.io_errors.load(Ordering::SeqCst);
        let meta = self.metadata_errors.load(Ordering::SeqCst);

        // Metadata bozulması her zaman öncelikli — tek bir olay bile hata döndürür.
        // Metadata kaybı veri kaybına veya sistemi açamaz hale getirmeye yol açabilir.
        if meta > 0 {
            return Some(Fault::new(
                FaultSource::Filesystem,
                FaultType::MetadataCorruption,
                &alloc::format!("Metadata errors detected: {}", meta)
            ));
        }

        // G/Ç hataları belirli bir eşiği (10) aştığında raporlanır.
        // Az sayıda G/Ç hatası disk sektörü yeniden denemesiyle düzeltilebilir.
        if io > 10 {
            return Some(Fault::new(
                FaultSource::Filesystem,
                FaultType::IoError,
                &alloc::format!("High I/O error count: {}", io)
            ));
        }

        None
    }
}

impl super::HealthMonitor for FsMonitor {
    fn name(&self) -> &'static str {
        "filesystem"
    }

    fn check(&self) -> Option<Fault> {
        // Monitör kapalıysa hiçbir şey yapma
        if !self.enabled.load(Ordering::SeqCst) {
            return None;
        }

        // Son kontrol tick'ini kaydet
        self.last_check.store(
            crate::task::scheduler::get_ticks(),
            Ordering::SeqCst
        );

        self.check_fs()
    }

    // health(): Özet sağlık durumu — metadata hatası ağır basar.
    fn health(&self) -> HealthStatus {
        let meta = self.metadata_errors.load(Ordering::SeqCst);
        let io = self.io_errors.load(Ordering::SeqCst);

        if meta > 2 {
            HealthStatus::Failed
        } else if meta > 0 || io > 20 {
            HealthStatus::Degraded
        } else if io > 5 {
            HealthStatus::Warning
        } else {
            HealthStatus::Healthy
        }
    }

    // is_critical: false → Dosya sistemi hatası sistemi durdurmaz (salt okunur modda devam edilebilir).
    // can_restart: true  → Dosya sistemi yeniden bağlanabilir (remount).
    // has_fallback: true → Salt okunur mod yedek strateji olarak kullanılabilir.
    fn module_health(&self) -> ModuleHealth {
        ModuleHealth {
            name: self.name(),
            status: self.health(),
            fault_count: self.io_errors.load(Ordering::SeqCst) + self.metadata_errors.load(Ordering::SeqCst),
            recovery_count: 0,
            last_fault_tick: self.last_check.load(Ordering::SeqCst),
            uptime_ticks: crate::task::scheduler::get_ticks(),
            is_critical: false,
            can_restart: true,
            has_fallback: true,
        }
    }

    // reset(): Kurtarma sonrasında tüm sayaçları sıfırla.
    fn reset(&self) {
        self.io_errors.store(0, Ordering::SeqCst);
        self.metadata_errors.store(0, Ordering::SeqCst);
        self.disk_full_events.store(0, Ordering::SeqCst);
    }
}

pub static FS_MONITOR: FsMonitor = FsMonitor::new();
