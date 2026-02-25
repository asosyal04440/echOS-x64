//! # Power Management
//!
//! Device power management and system sleep states.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// POWER STATES
// ============================================================================

/// System sleep states (ACPI)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SleepState {
    /// Working state
    S0,
    /// Power On Suspend
    S1,
    /// Suspend to RAM
    S3,
    /// Suspend to Disk (Hibernate)
    S4,
    /// Soft Off
    S5,
}

/// Device power state
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevicePowerState {
    /// Fully on
    D0,
    /// Low power, context preserved
    D1,
    /// Low power, most context lost
    D2,
    /// Off, context lost
    D3Hot,
    /// Off, power removed
    D3Cold,
}

/// Runtime PM status
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimePmStatus {
    Active,
    Suspended,
    Suspending,
    Resuming,
    Error,
}

// ============================================================================
// POWER MANAGEABLE DEVICE
// ============================================================================

pub struct PowerManageable {
    /// Device ID
    pub device_id: u64,
    /// Current power state
    pub power_state: Mutex<DevicePowerState>,
    /// Runtime PM status
    pub runtime_status: Mutex<RuntimePmStatus>,
    /// Runtime suspend count
    pub suspend_count: AtomicU32,
    /// Usage count
    pub usage_count: AtomicU32,
    /// Autosuspend delay (ms)
    pub autosuspend_delay: AtomicU32,
    /// Last busy timestamp
    pub last_busy: AtomicU64,
    /// Can wake system
    pub can_wakeup: AtomicBool,
    /// Should wakeup
    pub should_wakeup: AtomicBool,
    /// Suspend callback
    pub suspend_cb: Option<fn(u64, DevicePowerState) -> Result<(), PmError>>,
    /// Resume callback
    pub resume_cb: Option<fn(u64) -> Result<(), PmError>>,
    /// Prepare callback
    pub prepare_cb: Option<fn(u64) -> Result<(), PmError>>,
    /// Complete callback
    pub complete_cb: Option<fn(u64)>,
}

impl PowerManageable {
    pub fn new(device_id: u64) -> Self {
        Self {
            device_id,
            power_state: Mutex::new(DevicePowerState::D0),
            runtime_status: Mutex::new(RuntimePmStatus::Active),
            suspend_count: AtomicU32::new(0),
            usage_count: AtomicU32::new(1),
            autosuspend_delay: AtomicU32::new(2000), // 2 seconds
            last_busy: AtomicU64::new(0),
            can_wakeup: AtomicBool::new(false),
            should_wakeup: AtomicBool::new(false),
            suspend_cb: None,
            resume_cb: None,
            prepare_cb: None,
            complete_cb: None,
        }
    }

    /// Suspend device
    pub fn suspend(&self, state: DevicePowerState) -> Result<(), PmError> {
        if let Some(cb) = self.suspend_cb {
            cb(self.device_id, state)?;
        }
        *self.power_state.lock() = state;
        *self.runtime_status.lock() = RuntimePmStatus::Suspended;
        self.suspend_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    /// Resume device
    pub fn resume(&self) -> Result<(), PmError> {
        if let Some(cb) = self.resume_cb {
            cb(self.device_id)?;
        }
        *self.power_state.lock() = DevicePowerState::D0;
        *self.runtime_status.lock() = RuntimePmStatus::Active;
        Ok(())
    }

    /// Runtime suspend
    pub fn runtime_suspend(&self) -> Result<(), PmError> {
        if self.usage_count.load(Ordering::SeqCst) > 0 {
            return Err(PmError::Busy);
        }
        
        *self.runtime_status.lock() = RuntimePmStatus::Suspending;
        self.suspend(DevicePowerState::D3Hot)?;
        Ok(())
    }

    /// Runtime resume
    pub fn runtime_resume(&self) -> Result<(), PmError> {
        *self.runtime_status.lock() = RuntimePmStatus::Resuming;
        self.resume()?;
        Ok(())
    }

    /// Get usage
    pub fn get(&self) {
        self.usage_count.fetch_add(1, Ordering::SeqCst);
        self.last_busy.store(
            crate::task::scheduler::get_ticks(),
            Ordering::SeqCst
        );
    }

    /// Put usage
    pub fn put(&self) {
        self.usage_count.fetch_sub(1, Ordering::SeqCst);
    }

    /// Check if idle
    pub fn is_idle(&self) -> bool {
        self.usage_count.load(Ordering::SeqCst) == 0
    }

    /// Check autosuspend timeout
    pub fn check_autosuspend(&self) -> bool {
        let last = self.last_busy.load(Ordering::SeqCst);
        let now = crate::task::scheduler::get_ticks();
        let delay = self.autosuspend_delay.load(Ordering::SeqCst) as u64;
        
        now - last > delay
    }
}

// ============================================================================
// POWER MANAGER
// ============================================================================

pub struct PowerManager {
    /// Current system sleep state
    system_state: Mutex<SleepState>,
    /// Power manageable devices
    devices: Mutex<BTreeMap<u64, Arc<PowerManageable>>>,
    /// Wakeup sources
    wakeup_sources: Mutex<Vec<WakeupSource>>,
    /// Suspend blockers
    suspend_blockers: Mutex<Vec<String>>,
    /// Is suspending
    suspending: AtomicBool,
    /// Statistics
    stats: Mutex<PmStats>,
}

#[derive(Clone, Debug)]
pub struct WakeupSource {
    pub name: String,
    pub device_id: u64,
    pub enabled: bool,
    pub count: u64,
}

#[derive(Clone, Debug, Default)]
pub struct PmStats {
    pub suspend_count: u64,
    pub resume_count: u64,
    pub suspend_fail_count: u64,
    pub last_suspend_time: u64,
    pub total_suspend_time: u64,
    pub deepest_state: SleepState,
}

impl PowerManager {
    pub const fn new() -> Self {
        Self {
            system_state: Mutex::new(SleepState::S0),
            devices: Mutex::new(BTreeMap::new()),
            wakeup_sources: Mutex::new(Vec::new()),
            suspend_blockers: Mutex::new(Vec::new()),
            suspending: AtomicBool::new(false),
            stats: Mutex::new(PmStats::default()),
        }
    }

    /// Register device for PM
    pub fn register_device(&self, device: Arc<PowerManageable>) {
        self.devices.lock().insert(device.device_id, device);
    }

    /// Unregister device
    pub fn unregister_device(&self, device_id: u64) {
        self.devices.lock().remove(&device_id);
    }

    /// Add wakeup source
    pub fn add_wakeup_source(&self, name: &str, device_id: u64) {
        let ws = WakeupSource {
            name: String::from(name),
            device_id,
            enabled: true,
            count: 0,
        };
        self.wakeup_sources.lock().push(ws);
    }

    /// Block suspend
    pub fn block_suspend(&self, reason: &str) {
        self.suspend_blockers.lock().push(String::from(reason));
    }

    /// Unblock suspend
    pub fn unblock_suspend(&self, reason: &str) {
        self.suspend_blockers.lock().retain(|r| r != reason);
    }

    /// Can suspend?
    pub fn can_suspend(&self) -> bool {
        self.suspend_blockers.lock().is_empty()
    }

    /// Enter system sleep state
    pub fn enter_sleep(&self, state: SleepState) -> Result<(), PmError> {
        if !self.can_suspend() {
            return Err(PmError::Blocked);
        }
        
        self.suspending.store(true, Ordering::SeqCst);
        let start_time = crate::task::scheduler::get_ticks();
        
        crate::serial_println!("[PM] Entering sleep state {:?}", state);
        
        // Prepare devices
        for device in self.devices.lock().values() {
            if let Some(cb) = device.prepare_cb {
                cb(device.device_id)?;
            }
        }
        
        // Suspend devices (in reverse order)
        let devices: Vec<Arc<PowerManageable>> = 
            self.devices.lock().values().cloned().collect();
        
        for device in devices.iter().rev() {
            let target_state = match state {
                SleepState::S1 => DevicePowerState::D1,
                SleepState::S3 => DevicePowerState::D3Hot,
                SleepState::S4 | SleepState::S5 => DevicePowerState::D3Cold,
                _ => DevicePowerState::D0,
            };
            device.suspend(target_state)?;
        }
        
        // Enter actual sleep state
        self.enter_acpi_state(state)?;
        
        // ... we're now asleep ...
        // ... and now we're awake ...
        
        // Resume devices
        for device in devices.iter() {
            device.resume()?;
            if let Some(cb) = device.complete_cb {
                cb(device.device_id);
            }
        }
        
        // Update stats
        let end_time = crate::task::scheduler::get_ticks();
        let mut stats = self.stats.lock();
        stats.suspend_count += 1;
        stats.resume_count += 1;
        stats.last_suspend_time = end_time - start_time;
        stats.total_suspend_time += stats.last_suspend_time;
        
        *self.system_state.lock() = SleepState::S0;
        self.suspending.store(false, Ordering::SeqCst);
        
        crate::serial_println!("[PM] Resumed from sleep state {:?}", state);
        
        Ok(())
    }

    /// Enter ACPI sleep state
    fn enter_acpi_state(&self, state: SleepState) -> Result<(), PmError> {
        *self.system_state.lock() = state;
        
        // Write to ACPI PM1a control register
        // For now, placeholder
        match state {
            SleepState::S5 => {
                // Power off
                crate::serial_println!("[PM] Powering off");
            }
            _ => {}
        }
        
        Ok(())
    }

    /// Hibernate (S4)
    pub fn hibernate(&self) -> Result<(), PmError> {
        // Save memory to swap
        self.enter_sleep(SleepState::S4)
    }

    /// Suspend to RAM (S3)
    pub fn suspend_to_ram(&self) -> Result<(), PmError> {
        self.enter_sleep(SleepState::S3)
    }

    /// Power off
    pub fn power_off(&self) -> Result<(), PmError> {
        self.enter_sleep(SleepState::S5)
    }

    /// Reboot
    pub fn reboot(&self) -> Result<(), PmError> {
        crate::serial_println!("[PM] Rebooting");
        // Reset via ACPI or keyboard controller
        Ok(())
    }

    /// Get statistics
    pub fn get_stats(&self) -> PmStats {
        self.stats.lock().clone()
    }

    /// Check if suspending
    pub fn is_suspending(&self) -> bool {
        self.suspending.load(Ordering::SeqCst)
    }
}

lazy_static::lazy_static! {
    pub static ref PM_MANAGER: PowerManager = PowerManager::new();
}

// ============================================================================
// ERROR TYPE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PmError {
    Busy,
    Blocked,
    DeviceError,
    NotSupported,
    PrepareFailed,
    SuspendFailed,
    ResumeFailed,
}

// ============================================================================
// SYSCALL INTERFACE
// ============================================================================

pub fn sys_reboot(cmd: u32) -> i32 {
    match cmd {
        0 => { // LINUX_REBOOT_CMD_RESTART
            let _ = PM_MANAGER.reboot();
            0
        }
        1 => { // LINUX_REBOOT_CMD_POWER_OFF
            let _ = PM_MANAGER.power_off();
            0
        }
        2 => { // LINUX_REBOOT_CMD_HALT
            0
        }
        3 => { // LINUX_REBOOT_CMD_SW_SUSPEND
            let _ = PM_MANAGER.hibernate();
            0
        }
        _ => -22
    }
}

pub fn sys_suspend(state: u32) -> i32 {
    let sleep_state = match state {
        1 => SleepState::S1,
        3 => SleepState::S3,
        4 => SleepState::S4,
        _ => return -22,
    };
    
    match PM_MANAGER.enter_sleep(sleep_state) {
        Ok(()) => 0,
        Err(_) => -5,
    }
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init() {
    crate::serial_println!("[PM] Power management initialized");
}
