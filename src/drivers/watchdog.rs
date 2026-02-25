//! # Watchdog Timer
//!
//! Hardware watchdog timer support.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

// ============================================================================
// WATCHDOG CONSTANTS
// ============================================================================

/// Default timeout in seconds
pub const WATCHDOG_DEFAULT_TIMEOUT: u32 = 60;
/// Minimum timeout
pub const WATCHDOG_MIN_TIMEOUT: u32 = 1;
/// Maximum timeout
pub const WATCHDOG_MAX_TIMEOUT: u32 = 65535;

// ============================================================================
// WATCHDOG OPERATIONS
// ============================================================================

pub trait WatchdogOps: Send + Sync {
    /// Start watchdog
    fn start(&self) -> Result<(), WatchdogError>;
    /// Stop watchdog
    fn stop(&self) -> Result<(), WatchdogError>;
    /// Ping/keepalive
    fn ping(&self) -> Result<(), WatchdogError>;
    /// Set timeout
    fn set_timeout(&self, seconds: u32) -> Result<(), WatchdogError>;
    /// Get timeout
    fn get_timeout(&self) -> u32;
    /// Get time remaining
    fn get_timeleft(&self) -> u32;
    /// Get boot status
    fn get_boot_status(&self) -> WatchdogBootStatus;
}

// ============================================================================
// WATCHDOG STATUS
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchdogBootStatus {
    /// Normal boot
    Normal,
    /// Boot after watchdog reset
    WatchdogReset,
    /// Unknown
    Unknown,
}

// ============================================================================
// WATCHDOG DEVICE
// ============================================================================

pub struct WatchdogDevice {
    /// Device name
    pub name: &'static str,
    /// Is running
    pub running: AtomicBool,
    /// Timeout in seconds
    pub timeout: AtomicU32,
    /// Last ping time
    pub last_ping: AtomicU64,
    /// Is nowayout set
    pub nowayout: AtomicBool,
    /// Boot status
    pub boot_status: AtomicU32,
    /// Operations
    pub ops: Option<&'static dyn WatchdogOps>,
}

impl WatchdogDevice {
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            running: AtomicBool::new(false),
            timeout: AtomicU32::new(WATCHDOG_DEFAULT_TIMEOUT),
            last_ping: AtomicU64::new(0),
            nowayout: AtomicBool::new(false),
            boot_status: AtomicU32::new(0),
            ops: None,
        }
    }

    /// Start watchdog
    pub fn start(&self) -> Result<(), WatchdogError> {
        if let Some(ops) = self.ops {
            ops.start()?;
            self.running.store(true, Ordering::SeqCst);
            self.last_ping.store(
                crate::task::scheduler::get_ticks(),
                Ordering::SeqCst
            );
            crate::serial_println!("[WATCHDOG] {} started", self.name);
        }
        Ok(())
    }

    /// Stop watchdog
    pub fn stop(&self) -> Result<(), WatchdogError> {
        if self.nowayout.load(Ordering::SeqCst) {
            return Err(WatchdogError::NoWayOut);
        }
        
        if let Some(ops) = self.ops {
            ops.stop()?;
            self.running.store(false, Ordering::SeqCst);
            crate::serial_println!("[WATCHDOG] {} stopped", self.name);
        }
        Ok(())
    }

    /// Ping watchdog
    pub fn ping(&self) -> Result<(), WatchdogError> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(WatchdogError::NotRunning);
        }
        
        if let Some(ops) = self.ops {
            ops.ping()?;
            self.last_ping.store(
                crate::task::scheduler::get_ticks(),
                Ordering::SeqCst
            );
        }
        Ok(())
    }

    /// Set timeout
    pub fn set_timeout(&self, seconds: u32) -> Result<(), WatchdogError> {
        if seconds < WATCHDOG_MIN_TIMEOUT || seconds > WATCHDOG_MAX_TIMEOUT {
            return Err(WatchdogError::InvalidTimeout);
        }
        
        if let Some(ops) = self.ops {
            ops.set_timeout(seconds)?;
        }
        self.timeout.store(seconds, Ordering::SeqCst);
        Ok(())
    }

    /// Get timeout
    pub fn get_timeout(&self) -> u32 {
        self.timeout.load(Ordering::SeqCst)
    }

    /// Get time remaining
    pub fn get_timeleft(&self) -> u32 {
        if let Some(ops) = self.ops {
            return ops.get_timeleft();
        }
        0
    }

    /// Check if running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Set nowayout
    pub fn set_nowayout(&self, value: bool) {
        self.nowayout.store(value, Ordering::SeqCst);
    }
}

// ============================================================================
// HPET WATCHDOG
// ============================================================================

pub struct HpetWatchdog {
    base_addr: u64,
    timer_id: u32,
    timeout: AtomicU32,
}

impl HpetWatchdog {
    pub fn new(base_addr: u64, timer_id: u32) -> Self {
        Self {
            base_addr,
            timer_id,
            timeout: AtomicU32::new(WATCHDOG_DEFAULT_TIMEOUT),
        }
    }
}

impl WatchdogOps for HpetWatchdog {
    fn start(&self) -> Result<(), WatchdogError> {
        // Configure HPET timer for watchdog
        Ok(())
    }

    fn stop(&self) -> Result<(), WatchdogError> {
        Ok(())
    }

    fn ping(&self) -> Result<(), WatchdogError> {
        // Reset timer
        Ok(())
    }

    fn set_timeout(&self, seconds: u32) -> Result<(), WatchdogError> {
        self.timeout.store(seconds, Ordering::SeqCst);
        Ok(())
    }

    fn get_timeout(&self) -> u32 {
        self.timeout.load(Ordering::SeqCst)
    }

    fn get_timeleft(&self) -> u32 {
        0
    }

    fn get_boot_status(&self) -> WatchdogBootStatus {
        WatchdogBootStatus::Unknown
    }
}

// ============================================================================
// TCO WATCHDOG (Intel ICH)
// ============================================================================

pub struct TcoWatchdog {
    iobase: u16,
    timeout: AtomicU32,
}

impl TcoWatchdog {
    pub fn new(iobase: u16) -> Self {
        Self {
            iobase,
            timeout: AtomicU32::new(WATCHDOG_DEFAULT_TIMEOUT),
        }
    }
}

impl WatchdogOps for TcoWatchdog {
    fn start(&self) -> Result<(), WatchdogError> {
        // Enable TCO timer
        Ok(())
    }

    fn stop(&self) -> Result<(), WatchdogError> {
        // Disable TCO timer
        Ok(())
    }

    fn ping(&self) -> Result<(), WatchdogError> {
        // Reload TCO timer
        Ok(())
    }

    fn set_timeout(&self, seconds: u32) -> Result<(), WatchdogError> {
        // TCO has limited timeout range
        let tco_timeout = if seconds > 613 { 613 } else { seconds };
        self.timeout.store(tco_timeout, Ordering::SeqCst);
        Ok(())
    }

    fn get_timeout(&self) -> u32 {
        self.timeout.load(Ordering::SeqCst)
    }

    fn get_timeleft(&self) -> u32 {
        // Read TCO timer value
        0
    }

    fn get_boot_status(&self) -> WatchdogBootStatus {
        // Check TCO status register
        WatchdogBootStatus::Unknown
    }
}

// ============================================================================
// WATCHDOG MANAGER
// ============================================================================

use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

pub struct WatchdogManager {
    devices: Mutex<Vec<Arc<WatchdogDevice>>>,
    active: Mutex<Option<Arc<WatchdogDevice>>>,
}

impl WatchdogManager {
    pub const fn new() -> Self {
        Self {
            devices: Mutex::new(Vec::new()),
            active: Mutex::new(None),
        }
    }

    pub fn register(&self, device: Arc<WatchdogDevice>) {
        self.devices.lock().push(device.clone());
        crate::serial_println!("[WATCHDOG] Registered {}", device.name);
    }

    pub fn set_active(&self, name: &str) -> Result<(), WatchdogError> {
        let devices = self.devices.lock();
        for device in devices.iter() {
            if device.name == name {
                *self.active.lock() = Some(device.clone());
                device.start()?;
                return Ok(());
            }
        }
        Err(WatchdogError::NotFound)
    }

    pub fn ping_active(&self) -> Result<(), WatchdogError> {
        if let Some(device) = self.active.lock().as_ref() {
            device.ping()?;
        }
        Ok(())
    }

    pub fn get_active(&self) -> Option<Arc<WatchdogDevice>> {
        self.active.lock().as_ref().cloned()
    }
}

lazy_static::lazy_static! {
    pub static ref WATCHDOG_MANAGER: WatchdogManager = WatchdogManager::new();
}

// ============================================================================
// ERROR TYPE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchdogError {
    NotRunning,
    AlreadyRunning,
    InvalidTimeout,
    NoWayOut,
    NotFound,
    IoError,
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init() {
    crate::serial_println!("[WATCHDOG] Subsystem initialized");
}

/// Ping watchdog (called from timer tick)
pub fn ping() {
    let _ = WATCHDOG_MANAGER.ping_active();
}
