//! # Sağlık Monitörleri
//!
//! Modül başına sağlık izleme ve hata tespiti.
//! Her modül için ayrı bir monitör çalışarak periyodik kontrol yapar.
//!
//! ## Monitör Mimarisi
//!
//! ```
//! +-------------------------------------------------------------+
//! |                    MONITORS (MonitorRegistry)               |
//! |                                                             |
//! |  +----------+  +---------+  +--------+  +---------+        |
//! |  | Memory   |  |   CPU   |  |  SMP   |  |   IRQ   |  ...  |
//! |  | Monitor  |  | Monitor |  | Monitor|  | Monitor |        |
//! |  +----+-----+  +----+----+  +----+---+  +----+----+        |
//! |       |             |            |            |             |
//! |       +-------------+------------+------------+             |
//! |                          |                                  |
//! |                    check_all()                              |
//! |                          |                                  |
//! |                    FaultHub::report()                       |
//! +-------------------------------------------------------------+
//! ```
//!
//! ## Çalışma Döngüsü
//!
//! ```
//! Zamanlayıcı tick gelir
//!      |
//!      v
//! monitors::check_all()
//!      |
//!      +--> monitor.check() --> Option<Fault>
//!      |         |
//!      |    Some(fault) --> fault::hub::report(...)
//!      |    None        --> devam et
//!      |
//!      +--> bir sonraki monitör...
//! ```
//!
//! ## HealthStatus Seviyeleri
//!
//! ```
//! Healthy   --> Sorun yok, normal çalışma
//! Warning   --> Dikkat edilmesi gereken durum
//! Degraded  --> Performans düşmüş, hizmet kısmen çalışıyor
//! Failed    --> Modül çalışamaz durumda
//! ```

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
//
// Rust'ta trait, birden fazla türün paylaşabileceği bir davranış sözleşmesidir.
// HealthMonitor trait'i, tüm monitörlerin uygulaması gereken metotları tanımlar.
// Bu sayede MonitorRegistry hangi tür monitörle çalıştığını bilmek zorunda kalmaz
// (dinamik dispatch: &'static dyn HealthMonitor).
//
// Send + Sync: Bu trait'i implemente eden tipler thread-safe olmalıdır.
//   - Send:  Farklı thread'lere sahiplik devredilebilir.
//   - Sync:  Referanslar birden fazla thread'den güvenle kullanılabilir.

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
//
// MonitorRegistry, tüm aktif monitörleri tek bir noktada toplar.
// Registry pattern: Nesneleri merkezi bir yerde kaydedip, toplu işlem yapmayı sağlar.
//
// spin::Mutex: Standart kütüphane olmayan (no_std) ortamda kullanılan
//   spin-lock tabanlı karşılıklı dışlama kilidleri.
//   Kernel içinde blocking I/O yapılamadığı için spinning tercih edilir.
//
// AtomicBool: Kilit kullanmadan atomik bool okuma/yazma.
//   initialized flag'i tekrar başlatmayı önlemek için kullanılır.

/// Global monitör kayıt defteri
pub struct MonitorRegistry {
    // Vec<&'static dyn HealthMonitor>: Trait object (dinamik dispatch) listesi.
    // 'static lifetime: Monitörlerin program boyunca yaşaması garanti edilir.
    monitors: spin::Mutex<Vec<&'static dyn HealthMonitor>>,
    // Çift başlatmayı önlemek için atomik bayrak
    initialized: AtomicBool,
}

impl MonitorRegistry {
    // const fn: Derleme zamanında (compile-time) çalışabilir fonksiyon.
    // static değişken başlatıcısı olarak kullanılabilmesi için gereklidir.
    pub const fn new() -> Self {
        Self {
            monitors: spin::Mutex::new(Vec::new()),
            initialized: AtomicBool::new(false),
        }
    }

    // Yeni bir monitörü kayıt defterine ekler.
    // &'static dyn: Heap'te yaşayan, trait object olarak saklanan referans.
    pub fn register(&self, monitor: &'static dyn HealthMonitor) {
        self.monitors.lock().push(monitor);
    }

    // Tüm monitörleri sırayla kontrol eder.
    // Hata bulunursa FaultHub'a raporlanır.
    pub fn check_all(&self) {
        for monitor in self.monitors.lock().iter() {
            if let Some(fault) = monitor.check() {
                crate::fault::hub::report(fault.source, fault.fault_type, &fault.message);
            }
        }
    }

    // Belirli isimli monitörün sağlık bilgisini döndürür.
    // Option<T>: Sonuç bulunmayabilir; None döndürmek güvenlidir.
    pub fn get_health(&self, name: &str) -> Option<ModuleHealth> {
        for monitor in self.monitors.lock().iter() {
            if monitor.name() == name {
                return Some(monitor.module_health());
            }
        }
        None
    }

    // Tüm monitörlerin sağlık bilgisini toplar ve döndürür.
    // .map() + .collect(): iterator zinciri ile fonksiyonel dönüşüm.
    pub fn all_health(&self) -> Vec<ModuleHealth> {
        self.monitors.lock().iter().map(|m| m.module_health()).collect()
    }
}

// lazy_static!: Rust'ta static değişkenlere çalışma zamanı (runtime) başlatıcısı eklemenin
// standart yolu. İlk erişimde başlatılır, sonraki erişimlerde önbellekten döner.
lazy_static::lazy_static! {
    pub static ref MONITORS: MonitorRegistry = MonitorRegistry::new();
}

// ============================================================================
// BAŞLAŞMA
// ============================================================================
//
// init() fonksiyonu kernel başlatma sırası (boot sequence) içinde bir kez çağrılır.
// swap(true, SeqCst): Eski değeri döndürür, yenisini yazar — tek adımda (atomik).
//   Eğer önceki değer true ise zaten başlatılmış, erken dön.
// Ordering::SeqCst (Sequentially Consistent): En güçlü bellek sıralama garantisi.
//   Tüm thread'ler bu işlemi aynı sırada görür; veri yarışını önler.

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

// check_all ve yardımcı fonksiyonlar — dışarıdan MONITORS'a doğrudan erişimi sarmalar.
// Bu sarmalama (wrapper) pattern'i API sınırlarını temiz tutar.

pub fn check_all() {
    MONITORS.check_all();
}

pub fn get_health(name: &str) -> Option<ModuleHealth> {
    MONITORS.get_health(name)
}

pub fn all_health() -> Vec<ModuleHealth> {
    MONITORS.all_health()
}
