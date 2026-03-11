//! # Zarif Bozunma (Graceful Degradation) Yöneticisi
//!
//! Sistem bozunma seviyelerini ve modül devre dışı bırakma işlemlerini yönetir.
//! Sistem kırılırken kritik olmayan modülleri devre dışı bırakar,
//! çekirdek işlevselliğini korur.
//!
//! ## Zarif Bozunma Nedir?
//!
//! Bir sistem parçası bozulduğunda tüm sistemi çöktürmek yerine,
//! yalnızca o parçayı devre dışı bırakıp geri kalanın çalışmaya devam etmesine
//! "zarif bozunma" denir. Uçak bileşenlerindeki "fail-safe" konseptine benzer.
//!
//! ## Modül Öncelik Sırası (Hangisi Önce Devre Dışı Kalır?)
//!
//! ```text
//!  Bozunma Yok (Level 0)
//!  ┌────────────────────────────────────────────────────┐
//!  │  memory  cpu  scheduler  interrupts                │
//!  │  audio  bluetooth  gui  network  usb  fs_write     │
//!  └────────────────────────────────────────────────────┘
//!                    Level 1 (Warning)
//!  ┌────────────────────────────────────────────────────┐
//!  │  [Hiçbir modül devre dışı bırakılmaz, yalnızca    │
//!  │   izleme artar]                                    │
//!  └────────────────────────────────────────────────────┘
//!                    Level 2 (Degraded)
//!  ┌────────────────────────────────────────────────────┐
//!  │  memory  cpu  scheduler  interrupts                │
//!  │  network  usb  fs_write                            │
//!  │  ✗ audio  ✗ bluetooth  ✗ gui  (devre dışı)        │
//!  └────────────────────────────────────────────────────┘
//!                    Level 3 (Critical)
//!  ┌────────────────────────────────────────────────────┐
//!  │  memory  cpu  scheduler  interrupts   fs_write     │
//!  │  ✗ audio  ✗ bluetooth  ✗ gui          (devre dışı)│
//!  │  ✗ network  ✗ usb                     (devre dışı)│
//!  └────────────────────────────────────────────────────┘
//!                    Level 4 (Emergency)
//!  ┌────────────────────────────────────────────────────┐
//!  │  memory  cpu  scheduler  interrupts                │
//!  │  ✗ audio  ✗ bluetooth  ✗ gui          (devre dışı)│
//!  │  ✗ network  ✗ usb  ✗ fs_write         (devre dışı)│
//!  └────────────────────────────────────────────────────┘
//! ```
//!
//! ## Devre Dışı Bırakma Türleri
//!
//! - **Seviye kaynaklı**: `set_level()` — bozunma seviyesi arttığında otomatik
//! - **Manuel**: `disable_module()` — belirli bir modül için açık sebep kaydedilir
//! - **Geri alma**: `enable_module()` — yalnızca seviye kaynaklı devre dışı bırakmalar geri alınır

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

use super::severity::RecoveryLevel;

// ============================================================================
// BOZUNMA DURUMU
// ============================================================================

/// Modül bozunma bilgisi — bir modülün etkinlik ve bozunma durumunu takip eder
#[derive(Clone, Debug)]
pub struct ModuleState {
    pub name: String,
    pub enabled: bool,
    pub degraded: bool,
    pub fallback_active: bool,
    pub disable_reason: Option<String>,
}

/// Bozunma yöneticisi — tüm modül durumlarını ve bozunma seviyesini yönetir
///
/// Singleton örüntüsüyle `DEGRADATION_MANAGER` static örneği üzerinden kullanılır.
/// `init()` çağrıldığında hem çekirdek modüller (kaldırılamaz) hem de
/// kritik olmayan modüller (devre dışı bırakılabilir) sisteme kayıt edilir.
pub struct DegradationManager {
    /// Mevcut bozunma seviyesi (0-4)
    level: AtomicU32,
    /// Modül durumları haritası
    modules: Mutex<BTreeMap<String, ModuleState>>,
    /// Bozunma yönetimi etkin mi?
    enabled: AtomicBool,
}

impl DegradationManager {
    pub const fn new() -> Self {
        Self {
            level: AtomicU32::new(0),
            modules: Mutex::new(BTreeMap::new()),
            enabled: AtomicBool::new(true),
        }
    }

    /// Bilinen modülleri kayıt ederek başlatır
    pub fn init(&self) {
        let mut modules = self.modules.lock();

        // Çekirdek modüller (devre dışı bırakılamaz)
        modules.insert(
            String::from("memory"),
            ModuleState {
                name: String::from("memory"),
                enabled: true,
                degraded: false,
                fallback_active: false,
                disable_reason: None,
            },
        );
        modules.insert(
            String::from("cpu"),
            ModuleState {
                name: String::from("cpu"),
                enabled: true,
                degraded: false,
                fallback_active: false,
                disable_reason: None,
            },
        );
        modules.insert(
            String::from("scheduler"),
            ModuleState {
                name: String::from("scheduler"),
                enabled: true,
                degraded: false,
                fallback_active: false,
                disable_reason: None,
            },
        );
        modules.insert(
            String::from("interrupts"),
            ModuleState {
                name: String::from("interrupts"),
                enabled: true,
                degraded: false,
                fallback_active: false,
                disable_reason: None,
            },
        );

        // Kritik olmayan modüller (devre dışı bırakılabilir)
        modules.insert(
            String::from("audio"),
            ModuleState {
                name: String::from("audio"),
                enabled: true,
                degraded: false,
                fallback_active: false,
                disable_reason: None,
            },
        );
        modules.insert(
            String::from("bluetooth"),
            ModuleState {
                name: String::from("bluetooth"),
                enabled: true,
                degraded: false,
                fallback_active: false,
                disable_reason: None,
            },
        );
        modules.insert(
            String::from("gui"),
            ModuleState {
                name: String::from("gui"),
                enabled: true,
                degraded: false,
                fallback_active: false,
                disable_reason: None,
            },
        );
        modules.insert(
            String::from("network"),
            ModuleState {
                name: String::from("network"),
                enabled: true,
                degraded: false,
                fallback_active: false,
                disable_reason: None,
            },
        );
        modules.insert(
            String::from("usb"),
            ModuleState {
                name: String::from("usb"),
                enabled: true,
                degraded: false,
                fallback_active: false,
                disable_reason: None,
            },
        );
        modules.insert(
            String::from("fs_write"),
            ModuleState {
                name: String::from("fs_write"),
                enabled: true,
                degraded: false,
                fallback_active: false,
                disable_reason: None,
            },
        );
    }

    /// Bozunma seviyesini ayarlar ve ilgili modüllere uygular
    pub fn set_level(&self, level: RecoveryLevel) {
        self.level.store(level as u32, Ordering::SeqCst);
        self.apply_level(level);
    }

    /// Bozunma seviyesini modüllere uygular — etkilenen modülleri devre dışı bırakır/etkinleştirir
    ///
    /// Mantık:
    ///  1. Verilen seviyenin `disabled_modules()` listesine bak.
    ///  2. O listede olan modüller etkinse → devre dışı bırak, sebebi "Degradation level X" kaydet.
    ///  3. Listede olmayan ama "Degradation level" sebebiyle kapalı olan modüller → yeniden etkinleştir.
    ///     (Manuel kapatmalar bu şekilde yanlışlıkla açılmaz.)
    fn apply_level(&self, level: RecoveryLevel) {
        let disabled = level.disabled_modules();

        let mut modules = self.modules.lock();
        for (name, state) in modules.iter_mut() {
            if disabled.contains(&name.as_str()) {
                if state.enabled {
                    state.enabled = false;
                    state.disable_reason = Some(format!("Degradation level {:?}", level));
                    crate::serial_println!("[DEGRADATION] Disabled module: {}", name);
                }
            } else {
                if !state.enabled
                    && state
                        .disable_reason
                        .as_ref()
                        .map_or(false, |r| r.starts_with("Degradation level"))
                {
                    state.enabled = true;
                    state.disable_reason = None;
                    crate::serial_println!("[DEGRADATION] Re-enabled module: {}", name);
                }
            }
        }
    }

    /// Belirli bir modülü devre dışı bırakır
    pub fn disable_module(&self, name: &str, reason: &str) -> bool {
        let mut modules = self.modules.lock();
        if let Some(state) = modules.get_mut(name) {
            if state.enabled {
                state.enabled = false;
                state.disable_reason = Some(String::from(reason));
                crate::serial_println!("[DEGRADATION] Disabled module: {} ({})", name, reason);
                return true;
            }
        }
        false
    }

    /// Belirli bir modülü etkinleştirir
    pub fn enable_module(&self, name: &str) -> bool {
        let mut modules = self.modules.lock();
        if let Some(state) = modules.get_mut(name) {
            if !state.enabled {
                state.enabled = true;
                state.disable_reason = None;
                crate::serial_println!("[DEGRADATION] Enabled module: {}", name);
                return true;
            }
        }
        false
    }

    /// Belirtilen modülün etkin olup olmadığını kontrol eder
    pub fn is_enabled(&self, name: &str) -> bool {
        self.modules
            .lock()
            .get(name)
            .map(|s| s.enabled)
            .unwrap_or(true)
    }

    /// Mevcut bozunma seviyesini döndürür
    pub fn current_level(&self) -> RecoveryLevel {
        RecoveryLevel::from(self.level.load(Ordering::SeqCst))
    }

    /// Tüm modül durumlarını döndürür
    pub fn module_states(&self) -> Vec<ModuleState> {
        self.modules.lock().values().cloned().collect()
    }

    /// Bozunma seviyesini bir kademe artırır
    pub fn increase_level(&self) {
        let current = self.current_level();
        let new_level = match current {
            RecoveryLevel::Level0 => RecoveryLevel::Level1,
            RecoveryLevel::Level1 => RecoveryLevel::Level2,
            RecoveryLevel::Level2 => RecoveryLevel::Level3,
            RecoveryLevel::Level3 | RecoveryLevel::Level4 => RecoveryLevel::Level4,
        };
        self.set_level(new_level);
    }

    /// Bozunma seviyesini bir kademe azaltır
    pub fn decrease_level(&self) {
        let current = self.current_level();
        let new_level = match current {
            RecoveryLevel::Level0 => RecoveryLevel::Level0,
            RecoveryLevel::Level1 => RecoveryLevel::Level0,
            RecoveryLevel::Level2 => RecoveryLevel::Level1,
            RecoveryLevel::Level3 => RecoveryLevel::Level2,
            RecoveryLevel::Level4 => RecoveryLevel::Level3,
        };
        self.set_level(new_level);
    }
}

// ============================================================================
// GLOBAL ÖRNEK
// ============================================================================

lazy_static::lazy_static! {
    pub static ref DEGRADATION_MANAGER: DegradationManager = DegradationManager::new();
}

// ============================================================================
// GENEL (PUBLIC) API
// ============================================================================

pub fn init() {
    DEGRADATION_MANAGER.init();
}

pub fn set_level(level: RecoveryLevel) {
    DEGRADATION_MANAGER.set_level(level);
}

pub fn disable_module(name: &str, reason: &str) -> bool {
    DEGRADATION_MANAGER.disable_module(name, reason)
}

pub fn enable_module(name: &str) -> bool {
    DEGRADATION_MANAGER.enable_module(name)
}

pub fn is_enabled(name: &str) -> bool {
    DEGRADATION_MANAGER.is_enabled(name)
}

pub fn current_level() -> RecoveryLevel {
    DEGRADATION_MANAGER.current_level()
}

pub fn module_states() -> Vec<ModuleState> {
    DEGRADATION_MANAGER.module_states()
}
