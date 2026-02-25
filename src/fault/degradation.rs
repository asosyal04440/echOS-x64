//! # Graceful Degradation Manager
//!
//! Manages system degradation levels and module disabling.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::format;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicBool, Ordering};
use spin::Mutex;

use super::severity::RecoveryLevel;

// ============================================================================
// DEGRADATION STATE
// ============================================================================

/// Module degradation info
#[derive(Clone, Debug)]
pub struct ModuleState {
    pub name: String,
    pub enabled: bool,
    pub degraded: bool,
    pub fallback_active: bool,
    pub disable_reason: Option<String>,
}

/// Degradation manager
pub struct DegradationManager {
    /// Current degradation level
    level: AtomicU32,
    /// Module states
    modules: Mutex<BTreeMap<String, ModuleState>>,
    /// Degradation enabled
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
    
    /// Initialize with known modules
    pub fn init(&self) {
        let mut modules = self.modules.lock();
        
        // Core modules (cannot be disabled)
        modules.insert(String::from("memory"), ModuleState {
            name: String::from("memory"),
            enabled: true,
            degraded: false,
            fallback_active: false,
            disable_reason: None,
        });
        modules.insert(String::from("cpu"), ModuleState {
            name: String::from("cpu"),
            enabled: true,
            degraded: false,
            fallback_active: false,
            disable_reason: None,
        });
        modules.insert(String::from("scheduler"), ModuleState {
            name: String::from("scheduler"),
            enabled: true,
            degraded: false,
            fallback_active: false,
            disable_reason: None,
        });
        modules.insert(String::from("interrupts"), ModuleState {
            name: String::from("interrupts"),
            enabled: true,
            degraded: false,
            fallback_active: false,
            disable_reason: None,
        });
        
        // Non-critical modules (can be disabled)
        modules.insert(String::from("audio"), ModuleState {
            name: String::from("audio"),
            enabled: true,
            degraded: false,
            fallback_active: false,
            disable_reason: None,
        });
        modules.insert(String::from("bluetooth"), ModuleState {
            name: String::from("bluetooth"),
            enabled: true,
            degraded: false,
            fallback_active: false,
            disable_reason: None,
        });
        modules.insert(String::from("gui"), ModuleState {
            name: String::from("gui"),
            enabled: true,
            degraded: false,
            fallback_active: false,
            disable_reason: None,
        });
        modules.insert(String::from("network"), ModuleState {
            name: String::from("network"),
            enabled: true,
            degraded: false,
            fallback_active: false,
            disable_reason: None,
        });
        modules.insert(String::from("usb"), ModuleState {
            name: String::from("usb"),
            enabled: true,
            degraded: false,
            fallback_active: false,
            disable_reason: None,
        });
        modules.insert(String::from("fs_write"), ModuleState {
            name: String::from("fs_write"),
            enabled: true,
            degraded: false,
            fallback_active: false,
            disable_reason: None,
        });
    }
    
    /// Set degradation level
    pub fn set_level(&self, level: RecoveryLevel) {
        self.level.store(level as u32, Ordering::SeqCst);
        self.apply_level(level);
    }
    
    /// Apply degradation level
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
                if !state.enabled && state.disable_reason.as_ref().map_or(false, |r| r.starts_with("Degradation level")) {
                    state.enabled = true;
                    state.disable_reason = None;
                    crate::serial_println!("[DEGRADATION] Re-enabled module: {}", name);
                }
            }
        }
    }
    
    /// Disable a specific module
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
    
    /// Enable a specific module
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
    
    /// Check if module is enabled
    pub fn is_enabled(&self, name: &str) -> bool {
        self.modules.lock()
            .get(name)
            .map(|s| s.enabled)
            .unwrap_or(true)
    }
    
    /// Get current level
    pub fn current_level(&self) -> RecoveryLevel {
        RecoveryLevel::from(self.level.load(Ordering::SeqCst))
    }
    
    /// Get all module states
    pub fn module_states(&self) -> Vec<ModuleState> {
        self.modules.lock().values().cloned().collect()
    }
    
    /// Increase degradation level
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
    
    /// Decrease degradation level
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
// GLOBAL INSTANCE
// ============================================================================

lazy_static::lazy_static! {
    pub static ref DEGRADATION_MANAGER: DegradationManager = DegradationManager::new();
}

// ============================================================================
// PUBLIC API
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
