//! # Watchdog (Gözetleme) Sistemi
//!
//! Hata tespiti için modül başına watchdog zamanlayıcıları.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

// ============================================================================
// WATCHDOG YAPISI
// ============================================================================

/// Bireysel watchdog zamanlayıcısı
pub struct Watchdog {
    /// Watchdog adı
    pub name: &'static str,
    /// Tick cinsinden zaman aşımı
    pub timeout_ticks: u64,
    /// Son tetikleme zaman damgası
    last_kick: AtomicUsize,
    /// Watchdog süresi doldu mu?
    expired: AtomicBool,
    /// Watchdog etkin mi?
    enabled: AtomicBool,
    /// Süre dolumu sayısı
    expiration_count: AtomicU32,
    /// Süre dolumunda çağrılacak geri çağırma (callback)
    pub on_expire: Option<fn(&str)>,
}

impl Watchdog {
    pub const fn new(name: &'static str, timeout_ticks: u64) -> Self {
        Self {
            name,
            timeout_ticks,
            last_kick: AtomicUsize::new(0),
            expired: AtomicBool::new(false),
            enabled: AtomicBool::new(true),
            expiration_count: AtomicU32::new(0),
            on_expire: None,
        }
    }
    
    /// Watchdog'u tetikler (zamanlayıcıyı sıfırlar)
    pub fn kick(&self) {
        self.last_kick.store(
            crate::task::scheduler::get_ticks(),
            Ordering::SeqCst
        );
        self.expired.store(false, Ordering::SeqCst);
    }
    
    /// Watchdog süresinin dolup dolmadığını kontrol eder
    pub fn check(&self) -> bool {
        if !self.enabled.load(Ordering::SeqCst) {
            return false;
        }
        
        let current = crate::task::scheduler::get_ticks();
        let last = self.last_kick.load(Ordering::SeqCst);
        
        if current.saturating_sub(last) > self.timeout_ticks as usize {
            if !self.expired.swap(true, Ordering::SeqCst) {
                self.expiration_count.fetch_add(1, Ordering::SeqCst);
                
                crate::serial_println!(
                    "[WATCHDOG] '{}' süresi doldu (zaman aşımı: {} tick)",
                    self.name, self.timeout_ticks
                );
                
                // Süre dolumu geri çağırmasını çağır
                if let Some(callback) = self.on_expire {
                    callback(self.name);
                }
            }
            return true;
        }
        
        false
    }
    
    /// Watchdog'u etkinleştirir/devre dışı bırakır
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }
    
    /// Süre dolumu sayısını döndürür
    pub fn expiration_count(&self) -> u32 {
        self.expiration_count.load(Ordering::SeqCst)
    }
    
    /// Watchdog'u sıfırlar
    pub fn reset(&self) {
        self.last_kick.store(0, Ordering::SeqCst);
        self.expired.store(false, Ordering::SeqCst);
        self.kick();
    }
}

// ============================================================================
// WATCHDOG KAYIT DEFTERİ
// ============================================================================

/// Global watchdog kayıt defteri
pub struct WatchdogRegistry {
    /// Kayıtlı watchdog'lar
    watchdogs: Mutex<BTreeMap<String, &'static Watchdog>>,
    /// Tick cinsinden kontrol aralığı
    check_interval: AtomicUsize,
    /// Son kontrol zaman damgası
    last_check: AtomicUsize,
    /// Başlatıldı mı?
    initialized: AtomicBool,
}

impl WatchdogRegistry {
    pub const fn new() -> Self {
        Self {
            watchdogs: Mutex::new(BTreeMap::new()),
            check_interval: AtomicUsize::new(100), // Her 100 tick'te bir kontrol
            last_check: AtomicUsize::new(0),
            initialized: AtomicBool::new(false),
        }
    }
    
    /// Bir watchdog kaydeder
    pub fn register(&self, watchdog: &'static Watchdog) {
        self.watchdogs.lock().insert(String::from(watchdog.name), watchdog);
    }
    
    /// Belirli bir watchdog'u tetikler
    pub fn kick(&self, name: &str) -> bool {
        if let Some(wd) = self.watchdogs.lock().get(name) {
            wd.kick();
            true
        } else {
            false
        }
    }
    
    /// Tüm watchdog'ları kontrol eder
    pub fn check_all(&self) -> Vec<String> {
        let mut expired = Vec::new();
        
        for (name, wd) in self.watchdogs.lock().iter() {
            if wd.check() {
                expired.push(name.clone());
            }
        }
        
        expired
    }
    
    /// Periyodik kontrol (zamanlayıcı kesmesinden çağrılır)
    pub fn periodic_check(&self) {
        let current = crate::task::scheduler::get_ticks();
        let last = self.last_check.load(Ordering::SeqCst);
        let interval = self.check_interval.load(Ordering::SeqCst);
        
        if current.saturating_sub(last) >= interval {
            self.last_check.store(current, Ordering::SeqCst);
            self.check_all();
        }
    }
    
    /// Tüm watchdog durumlarını döndürür
    pub fn statuses(&self) -> Vec<WatchdogStatus> {
        self.watchdogs.lock()
            .iter()
            .map(|(name, wd)| WatchdogStatus {
                name: name.clone(),
                enabled: wd.enabled.load(Ordering::SeqCst),
                expired: wd.expired.load(Ordering::SeqCst),
                expiration_count: wd.expiration_count.load(Ordering::SeqCst),
                timeout_ticks: wd.timeout_ticks,
                last_kick: wd.last_kick.load(Ordering::SeqCst) as u64,
            })
            .collect()
    }
}

#[derive(Clone, Debug)]
pub struct WatchdogStatus {
    pub name: String,
    pub enabled: bool,
    pub expired: bool,
    pub expiration_count: u32,
    pub timeout_ticks: u64,
    pub last_kick: u64,
}

// ============================================================================
// ÖN TANIMLI WATCHDOG'LAR
// ============================================================================

/// Bellek alt sistemi watchdog'u
pub static MEMORY_WATCHDOG: Watchdog = Watchdog::new("memory", 5000);

/// Zamanlayıcı watchdog'u
pub static SCHEDULER_WATCHDOG: Watchdog = Watchdog::new("scheduler", 1000);

/// IRQ watchdog'u
pub static IRQ_WATCHDOG: Watchdog = Watchdog::new("irq", 2000);

/// Önyükleme watchdog'u
pub static BOOT_WATCHDOG: Watchdog = Watchdog::new("boot", 30000);

// ============================================================================
// GLOBAL KAYIT DEFTERİ
// ============================================================================

lazy_static::lazy_static! {
    pub static ref WATCHDOG_REGISTRY: WatchdogRegistry = WatchdogRegistry::new();
}

// ============================================================================
// BAŞLAŞMA
// ============================================================================

pub fn init() {
    if WATCHDOG_REGISTRY.initialized.swap(true, Ordering::SeqCst) {
        return;
    }
    
    // Temel watchdog'ları kaydet
    WATCHDOG_REGISTRY.register(&MEMORY_WATCHDOG);
    WATCHDOG_REGISTRY.register(&SCHEDULER_WATCHDOG);
    WATCHDOG_REGISTRY.register(&IRQ_WATCHDOG);
    WATCHDOG_REGISTRY.register(&BOOT_WATCHDOG);
    
    // Tüm watchdog'ları tetikle
    MEMORY_WATCHDOG.kick();
    SCHEDULER_WATCHDOG.kick();
    IRQ_WATCHDOG.kick();
    BOOT_WATCHDOG.kick();
    
    crate::serial_println!("[WATCHDOG] {} watchdog başlatıldı", 
        WATCHDOG_REGISTRY.watchdogs.lock().len());
}

pub fn check_all() -> Vec<String> {
    WATCHDOG_REGISTRY.check_all()
}

pub fn kick(name: &str) -> bool {
    WATCHDOG_REGISTRY.kick(name)
}

pub fn periodic_check() {
    WATCHDOG_REGISTRY.periodic_check();
}

pub fn statuses() -> Vec<WatchdogStatus> {
    WATCHDOG_REGISTRY.statuses()
}
