//! # echOS Hata Yönetimi Alt Sistemi
//!
//! Kapsamlı hata tespiti, izolasyonu ve kurtarma sistemi.
//! Sağlık izleme ve zarif bozunma (graceful degradation) aracılığıyla
//! çökme korumalı (anti-crash) bir yapı sağlar.
//!
//! ## Genel Mimari
//!
//! ```text
//!  ┌─────────────────────────────────────────────────────────────┐
//!  │               HATA YÖNETİMİ ALT SİSTEMİ                    │
//!  │                                                             │
//!  │  Modüller (Memory, CPU, Sched, ...)                         │
//!  │       │  hata bildir                                        │
//!  │       ▼                                                     │
//!  │  ┌──────────┐    ┌──────────────┐    ┌────────────────┐    │
//!  │  │ FaultHub │───▶│  RecoveryEng │───▶│  Degradation   │    │
//!  │  │ (merkez) │    │  (kurtarma)  │    │  (bozunma yönt)│    │
//!  │  └──────────┘    └──────────────┘    └────────────────┘    │
//!  │       │                                       │            │
//!  │       ▼                                       ▼            │
//!  │  ┌──────────┐    ┌──────────────┐    ┌────────────────┐    │
//!  │  │ Watchdog │    │  Checkpoint  │    │   Emergency    │    │
//!  │  │(zamanlcı)│    │ (kontol nkt) │    │  (acil durum)  │    │
//!  │  └──────────┘    └──────────────┘    └────────────────┘    │
//!  └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Hata Akış Şeması
//!
//! ```text
//!  Hata Oluştu
//!       │
//!       ▼
//!  report_fault() ──▶ FaultState'e kaydet ──▶ Şiddet belirle
//!       │
//!       ▼
//!  auto_recovery etkin mi?
//!       │ evet
//!       ▼
//!  RecoveryEngine::recover()
//!       │
//!       ├──▶ Birincil eylem dene ──▶ Başarılı? ──▶ Bitti
//!       │                              hayır
//!       ├──▶ Yedek eylem dene   ──▶ Başarılı? ──▶ Bitti
//!       │                              hayır
//!       └──▶ Son çare: EmergencyHalt / Reboot
//! ```
//!
//! ## Kurtarma Seviyeleri
//!
//! | Seviye | Ad        | Açıklama                                  |
//! |--------|-----------|-------------------------------------------|
//! | 0      | Normal    | Her şey yolunda                           |
//! | 1      | Warning   | Küçük sorunlar var, izleniyor             |
//! | 2      | Degraded  | Audio/BT/GUI devre dışı                   |
//! | 3      | Critical  | + Ağ ve USB de devre dışı                 |
//! | 4      | Emergency | + Dosya yazma devre dışı, halt yakın      |

pub mod hub;
pub mod severity;
pub mod recovery;
pub mod watchdog;
pub mod checkpoint;
pub mod degradation;
pub mod emergency;
pub mod injection;

pub mod monitors;
pub mod recovery_modules;

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, AtomicBool, Ordering};
use spin::Mutex;

// Re-export main types
// Dışarıdan `crate::fault::FaultHub` şeklinde erişim sağlamak için kısayollar.
// Bu sayede kullanıcı modülün iç yapısını bilmeden doğrudan ana tipleri kullanabilir.
pub use hub::FaultHub;
pub use severity::{Severity, RecoveryResult};
pub use recovery::{RecoveryAction, RecoveryEngine};

// ============================================================================
// HATA TÜRLERİ
// ============================================================================

/// Benzersiz hata tanımlayıcısı
/// Atomik sayaç ile üretilir — aynı anda birden fazla hata oluşsa bile ID'ler çakışmaz.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FaultId(pub u64);

/// Hatayı üreten kaynak modül
/// Her hata hangi alt sistemden geldiğini bildirir; bu sayede
/// kurtarma motoru kaynağa özel strateji seçebilir.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FaultSource {
    Memory,
    Cpu,
    Smp,
    Interrupt,
    Scheduler,
    Driver,
    Filesystem,
    Network,
    Security,
    Acpi,
    Boot,
    Unknown,
}

/// Özel hata türleri — hangi tür bir hatanın oluştuğunu belirtir
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultType {
    // Bellek hataları
    HeapCorruption,
    DoubleFree,
    UseAfterFree,
    NullPointer,
    InvalidPointer,
    OutOfMemory,
    PageFault,
    PmmCorruption,
    
    // CPU/SMP hataları
    ApStartupFailed,
    TlbShootdownTimeout,
    CpuHung,
    MicrocodeError,
    
    // Kesme (interrupt) hataları
    IdtCorruption,
    IrqStorm,
    HandlerTimeout,
    SpuriousInterrupt,
    
    // Zamanlayıcı (scheduler) hataları
    RunQueueCorruption,
    TaskLeak,
    PriorityInversion,
    Starvation,
    
    // Sürücsü (driver) hataları
    DmaCorruption,
    DeviceTimeout,
    DeviceError,
    DriverCrash,
    
    // Dosya sistemi (filesystem) hataları
    MetadataCorruption,
    JournalError,
    IoError,
    DiskFull,
    
    // Ağ (network) hataları
    ConnectionReset,
    StackCorruption,
    SocketLeak,
    
    // Güvenlik (security) hataları
    CanaryMismatch,
    SmepViolation,
    SmapViolation,
    
    // ACPI hataları
    AmlError,
    GpeStorm,
    ThermalEvent,
    
    // Açılış (boot) hataları
    BootTimeout,
    InitFailed,
    
    // Genel
    Unknown,
}

/// Tespit edilmiş bir hata olayini temsil eder
///
/// Her `Fault`, sistemde tespit edilen tek bir hatanın tam kaydıdır.
/// Kaynaktan (hangi modül), türden (ne tür hata), şiddetten (ne kadar ciddi)
/// ve kurtarma durumundan (denendi mi, başarılı mı) oluşur.
///
/// `with_context()` ile ek sayısal bağlam verileri (adresler, sayaçlar vb.) eklenebilir.
#[derive(Clone, Debug)]
pub struct Fault {
    /// Benzersiz hata kimliği
    pub id: FaultId,
    /// Kaynak modül
    pub source: FaultSource,
    /// Hata türü
    pub fault_type: FaultType,
    /// Şiddet seviyesi
    pub severity: Severity,
    /// İnsan okunabilir mesaj
    pub message: String,
    /// Zaman damgası (tick cinsinden)
    pub timestamp: usize,
    /// Hatanın gerçekleştiği CPU kimliği
    pub cpu_id: u32,
    /// Ek bağlam bilgisi
    pub context: Vec<u64>,
    /// Kurtarma denendi mi?
    pub recovery_attempted: bool,
    /// Kurtarma başarılı oldu mu?
    pub recovery_success: bool,
}

impl Fault {
    pub fn new(source: FaultSource, fault_type: FaultType, message: &str) -> Self {
        static FAULT_COUNTER: AtomicU64 = AtomicU64::new(0);
        
        Self {
            id: FaultId(FAULT_COUNTER.fetch_add(1, Ordering::SeqCst)),
            source,
            fault_type,
            severity: Severity::from_type(&fault_type),
            message: String::from(message),
            timestamp: crate::task::scheduler::get_ticks(),
            cpu_id: crate::cpu::smp::get_current_cpu_id(),
            context: Vec::new(),
            recovery_attempted: false,
            recovery_success: false,
        }
    }
    
    pub fn with_context(mut self, context: Vec<u64>) -> Self {
        self.context = context;
        self
    }
}

// ============================================================================
// MODÜL SAĞLIK DURUMU
// ============================================================================

/// Bir modülün sağlık durumu
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthStatus {
    /// Modül normal çalışıyor
    Healthy,
    /// Modülde küçük sorunlar var ancak işlevsel
    Warning,
    /// Modül azaltılmış işlevselliğle çalışıyor
    Degraded,
    /// Modül başarısız oldu ve devre dışı
    Failed,
    /// Modül kasıtlı olarak devre dışı bırakıldı
    Disabled,
}

impl Default for HealthStatus {
    fn default() -> Self {
        HealthStatus::Healthy
    }
}

/// Bir modülün sağlık bilgisi — hata sayıları ve kurtarma durumu
#[derive(Clone, Debug)]
pub struct ModuleHealth {
    /// Modül adı
    pub name: &'static str,
    /// Mevcut sağlık durumu
    pub status: HealthStatus,
    /// Tespit edilen hata sayısı
    pub fault_count: u32,
    /// Başarılı kurtarma sayısı
    pub recovery_count: u32,
    /// Son hata zaman damgası
    pub last_fault_tick: usize,
    /// Modül çalışma süresi (tick cinsinden)
    pub uptime_ticks: usize,
    /// Bu modül sistem çalışması için kritik mi?
    pub is_critical: bool,
    /// Modül yeniden başlatılabilir mi?
    pub can_restart: bool,
    /// Yedek modül mevcut mu?
    pub has_fallback: bool,
}

impl ModuleHealth {
    pub const fn new(name: &'static str, is_critical: bool, can_restart: bool, has_fallback: bool) -> Self {
        Self {
            name,
            status: HealthStatus::Healthy,
            fault_count: 0,
            recovery_count: 0,
            last_fault_tick: 0,
            uptime_ticks: 0,
            is_critical,
            can_restart,
            has_fallback,
        }
    }
    
    pub fn record_fault(&mut self) {
        self.fault_count += 1;
        self.last_fault_tick = crate::task::scheduler::get_ticks();
    }
    
    pub fn record_recovery(&mut self, success: bool) {
        if success {
            self.recovery_count += 1;
        }
    }
    
    pub fn update_status(&mut self, status: HealthStatus) {
        self.status = status;
    }
}

// ============================================================================
// GLOBAL HATA DURUMU
// ============================================================================

/// Global hata yönetim durumu
///
/// Tüm çekirdek kodu tarafından paylaşılan tekil (singleton) hata durumu.
/// `lazy_static!` ile başlatılır, `Mutex` ve `Atomic` tiplerle iç senkronizasyon sağlanır.
///
/// Not: `fault_history` son 100 hatayı tutar — en eski girdi yeni hata gelince silinir.
/// Bu kayan pencere (sliding window) yapısı hata hızını (fault rate) hesaplamakta kullanılır.
pub struct FaultState {
    /// Toplam tespit edilen hata sayısı
    pub total_faults: AtomicU64,
    /// Toplam başarılı kurtarma sayısı
    pub total_recoveries: AtomicU64,
    /// Mevcut sistem kurtarma seviyesi (0-4)
    pub recovery_level: AtomicU32,
    /// Sistem acil durum modunda mı?
    pub emergency_mode: AtomicBool,
    /// Hata tespiti etkin mi?
    pub detection_enabled: AtomicBool,
    /// Otomatik kurtarma etkin mi?
    pub auto_recovery: AtomicBool,
    /// Son hata zaman damgası
    pub last_fault_tick: AtomicUsize,
    /// Hata geçmişi (son 100 hata)
    pub fault_history: Mutex<Vec<Fault>>,
}

impl FaultState {
    pub const fn new() -> Self {
        Self {
            total_faults: AtomicU64::new(0),
            total_recoveries: AtomicU64::new(0),
            recovery_level: AtomicU32::new(0),
            emergency_mode: AtomicBool::new(false),
            detection_enabled: AtomicBool::new(true),
            auto_recovery: AtomicBool::new(true),
            last_fault_tick: AtomicUsize::new(0),
            fault_history: Mutex::new(Vec::new()),
        }
    }
    
    pub fn record_fault(&self, fault: &Fault) {
        self.total_faults.fetch_add(1, Ordering::SeqCst);
        self.last_fault_tick.store(fault.timestamp, Ordering::SeqCst);
        
        // Geçmişe ekle (maksimum 100 giriş)
        let mut history = self.fault_history.lock();
        if history.len() >= 100 {
            history.remove(0);
        }
        history.push(fault.clone());
    }
    
    pub fn record_recovery(&self) {
        self.total_recoveries.fetch_add(1, Ordering::SeqCst);
    }
    
    pub fn get_fault_rate(&self, window_ticks: u64) -> f64 {
        let current = crate::task::scheduler::get_ticks();
        let history = self.fault_history.lock();
        
        let count = history.iter()
            .filter(|f| current.saturating_sub(f.timestamp as usize) <= window_ticks as usize)
            .count();
        
        count as f64 / (window_ticks as f64 / 1000.0)
    }
}

lazy_static::lazy_static! {
    pub static ref FAULT_STATE: FaultState = FaultState::new();
}

// ============================================================================
// BAŞLAŞMA (INITIALIZATION)
// ============================================================================

/// Hata yönetimi alt sistemini başlatır
pub fn init() {
    crate::serial_println!("[FAULT] Initializing fault management subsystem");
    
    // Hata merkezini başlat
    hub::init();
    
    // Watchdog sistemini başlat
    watchdog::init();
    
    // Kurtarma motorunu başlat
    recovery::init();
    
    // Monitor/izleme modüllerini başlat
    monitors::init();
    
    crate::serial_println!("[FAULT] Fault management subsystem initialized");
}

/// Periyodik hata kontrolü yapılır (zamanlama timer'dan çağrılır)
pub fn periodic_check() {
    if !FAULT_STATE.detection_enabled.load(Ordering::SeqCst) {
        return;
    }
    
    // Tüm monitorrlerı kontrol et
    monitors::check_all();
    
    // Watchdog'ları kontrol et
    watchdog::check_all();
    
    // Kurtarma seviyesini güncelle
    update_recovery_level();
}

/// Hata geçmişine bakarak sistem kurtarma seviyesini otomatik günceller.
///
/// Karar mantığı şu şekilde çalışır:
///  - Son 10 saniyede kritik/acil hata varsa → Seviye 4 (Emergency)
///  - Son 10 saniyede 10'dan fazla hata varsa → Seviye 3 (Critical)
///  - Son 10 saniyede 5'ten fazla hata varsa  → Seviye 2 (Degraded)
///  - Herhangi bir son hata varsa             → Seviye 1 (Warning)
///  - Hata yoksa                              → Seviye 0 (Normal)
///
/// Seviye 4'e ulaşıldığında `emergency::enter()` çağrılır.
fn update_recovery_level() {
    let history = FAULT_STATE.fault_history.lock();
    let current = crate::task::scheduler::get_ticks();
    
    // Son 10 saniyedeki hataları say
    let recent_faults = history.iter()
        .filter(|f| current.saturating_sub(f.timestamp) <= 10000)
        .count();
    
    // Kritik hataları say
    let critical_faults = history.iter()
        .filter(|f| f.severity == Severity::Critical || f.severity == Severity::Emergency)
        .count();
    
    // Kurtarma seviyesini belirle
    let level = if critical_faults > 0 {
        4 // Acil durum (Emergency)
    } else if recent_faults > 10 {
        3 // Kritik (Critical)
    } else if recent_faults > 5 {
        2 // Bozunmuş (Degraded)
    } else if recent_faults > 0 {
        1 // Uyarı (Warning)
    } else {
        0 // Normal
    };
    
    FAULT_STATE.recovery_level.store(level, Ordering::SeqCst);
    
    if level >= 4 {
        FAULT_STATE.emergency_mode.store(true, Ordering::SeqCst);
        emergency::enter();
    }
}

/// Hata bildirir, otomatik kurtarma dener
pub fn report_fault(source: FaultSource, fault_type: FaultType, message: &str) -> FaultId {
    let fault = Fault::new(source, fault_type, message);
    FAULT_STATE.record_fault(&fault);
    
    crate::serial_println!(
        "[FAULT] {:?} fault from {:?}: {}",
        fault.severity, fault.source, fault.message
    );
    
    // Etkinse otomatik kurtarma dene
    if FAULT_STATE.auto_recovery.load(Ordering::SeqCst) {
        recovery::attempt_recovery(&fault);
    }
    
    fault.id
}

/// Hata istatistiklerini döndürür
pub fn get_stats() -> FaultStats {
    FaultStats {
        total_faults: FAULT_STATE.total_faults.load(Ordering::SeqCst),
        total_recoveries: FAULT_STATE.total_recoveries.load(Ordering::SeqCst),
        recovery_level: FAULT_STATE.recovery_level.load(Ordering::SeqCst),
        emergency_mode: FAULT_STATE.emergency_mode.load(Ordering::SeqCst),
        recent_fault_count: FAULT_STATE.fault_history.lock().len(),
    }
}

#[derive(Clone, Debug)]
pub struct FaultStats {
    pub total_faults: u64,
    pub total_recoveries: u64,
    pub recovery_level: u32,
    pub emergency_mode: bool,
    pub recent_fault_count: usize,
}
