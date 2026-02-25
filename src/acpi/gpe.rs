//! # ACPI Events (GPE)
//!
//! General Purpose Events and ACPI event handling.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

// ============================================================================
// GPE CONSTANTS
// ============================================================================

/// GPE block registers
pub const GPE0_BLK: &str = "GPE0_BLK";
pub const GPE1_BLK: &str = "GPE1_BLK";

/// GPE register offsets
pub const GPE_STS_OFFSET: usize = 0;
pub const GPE_EN_OFFSET: usize = 2;

/// GPE event types
pub const GPE_TYPE_WAKE: u8 = 0x01;
pub const GPE_TYPE_RUNTIME: u8 = 0x02;
pub const GPE_TYPE_WAKE_RUNTIME: u8 = 0x03;

/// Fixed event numbers
pub const ACPI_EVENT_PMTIMER: u32 = 0;
pub const ACPI_EVENT_POWER_BUTTON: u32 = 2;
pub const ACPI_EVENT_SLEEP_BUTTON: u32 = 3;
pub const ACPI_EVENT_RTC: u32 = 4;

// ============================================================================
// GPE EVENT
// ============================================================================

#[derive(Clone, Debug)]
pub struct GpeEvent {
    /// GPE number
    pub number: u32,
    /// GPE block (0 or 1)
    pub block: u8,
    /// Event type
    pub event_type: u8,
    /// Handler method
    pub handler: Option<String>,
    /// Is enabled
    pub enabled: AtomicBool,
    /// Is wake capable
    pub wake_capable: AtomicBool,
    /// Handler type (0=none, 1=method, 2=handler)
    pub handler_type: AtomicU32,
}

impl GpeEvent {
    pub fn new(number: u32, block: u8) -> Self {
        Self {
            number,
            block,
            event_type: GPE_TYPE_RUNTIME,
            handler: None,
            enabled: AtomicBool::new(false),
            wake_capable: AtomicBool::new(false),
            handler_type: AtomicU32::new(0),
        }
    }

    /// Enable event
    pub fn enable(&self) {
        self.enabled.store(true, Ordering::SeqCst);
    }

    /// Disable event
    pub fn disable(&self) {
        self.enabled.store(false, Ordering::SeqCst);
    }

    /// Set wake capable
    pub fn set_wake(&self, wake: bool) {
        self.wake_capable.store(wake, Ordering::SeqCst);
    }
}

// ============================================================================
// FIXED EVENT
// ============================================================================

#[derive(Clone, Debug)]
pub struct FixedEvent {
    /// Event number
    pub number: u32,
    /// Event name
    pub name: String,
    /// Status register address
    pub status_reg: u32,
    /// Enable register address
    pub enable_reg: u32,
    /// Handler method
    pub handler: Option<String>,
    /// Is enabled
    pub enabled: AtomicBool,
}

impl FixedEvent {
    pub fn new(number: u32, name: &str, status_reg: u32, enable_reg: u32) -> Self {
        Self {
            number,
            name: String::from(name),
            status_reg,
            enable_reg,
            handler: None,
            enabled: AtomicBool::new(false),
        }
    }

    /// Enable event
    pub fn enable(&self) {
        // Write to enable register
        self.enabled.store(true, Ordering::SeqCst);
        crate::serial_println!("[GPE] Fixed event {} enabled", self.name);
    }

    /// Disable event
    pub fn disable(&self) {
        self.enabled.store(false, Ordering::SeqCst);
    }

    /// Check status
    pub fn check_status(&self) -> bool {
        // Read status register
        false
    }

    /// Clear status
    pub fn clear_status(&self) {
        // Write to status register
    }
}

// ============================================================================
// GPE BLOCK
// ============================================================================

pub struct GpeBlock {
    /// Block number
    pub block_number: u8,
    /// Base address
    pub base_address: u32,
    /// Number of GPEs
    pub gpe_count: u32,
    /// GPE events
    pub events: Mutex<BTreeMap<u32, GpeEvent>>,
    /// Status register cached value
    pub status_cache: AtomicU32,
    /// Enable register cached value
    pub enable_cache: AtomicU32,
}

impl GpeBlock {
    pub fn new(block_number: u8, base_address: u32, gpe_count: u32) -> Self {
        Self {
            block_number,
            base_address,
            gpe_count,
            events: Mutex::new(BTreeMap::new()),
            status_cache: AtomicU32::new(0),
            enable_cache: AtomicU32::new(0),
        }
    }

    /// Initialize GPE events
    pub fn init(&self) {
        let mut events = self.events.lock();
        
        for i in 0..self.gpe_count {
            events.insert(i, GpeEvent::new(i, self.block_number));
        }
    }

    /// Get GPE event
    pub fn get_event(&self, number: u32) -> Option<GpeEvent> {
        self.events.lock().get(&number).cloned()
    }

    /// Enable GPE
    pub fn enable_gpe(&self, number: u32) {
        if let Some(event) = self.events.lock().get(&number) {
            event.enable();
            
            // Update enable register
            let bit = 1u32 << (number % 32);
            self.enable_cache.fetch_or(bit, Ordering::SeqCst);
            
            crate::serial_println!("[GPE] GPE{} enabled", number);
        }
    }

    /// Disable GPE
    pub fn disable_gpe(&self, number: u32) {
        if let Some(event) = self.events.lock().get(&number) {
            event.disable();
            
            let bit = 1u32 << (number % 32);
            self.enable_cache.fetch_and(!bit, Ordering::SeqCst);
        }
    }

    /// Clear GPE status
    pub fn clear_gpe(&self, number: u32) {
        let bit = 1u32 << (number % 32);
        self.status_cache.fetch_and(!bit, Ordering::SeqCst);
    }

    /// Handle GPE events
    pub fn handle_events(&self) -> Vec<u32> {
        let mut triggered = Vec::new();
        
        // Read status register
        let status = self.status_cache.load(Ordering::SeqCst);
        let enabled = self.enable_cache.load(Ordering::SeqCst);
        
        let active = status & enabled;
        
        for i in 0..self.gpe_count {
            let bit = 1u32 << (i % 32);
            if active & bit != 0 {
                triggered.push(i);
                
                // Execute handler
                if let Some(event) = self.events.lock().get(&i) {
                    if let Some(ref handler) = event.handler {
                        crate::serial_println!("[GPE] Executing handler {} for GPE{}", handler, i);
                        // Execute AML method
                    }
                }
                
                // Clear status
                self.clear_gpe(i);
            }
        }
        
        triggered
    }
}

// ============================================================================
// ACPI EVENT MANAGER
// ============================================================================

pub struct AcpiEventManager {
    /// GPE blocks
    pub gpe_blocks: Mutex<Vec<GpeBlock>>,
    /// Fixed events
    pub fixed_events: Mutex<BTreeMap<u32, FixedEvent>>,
    /// Event handlers
    pub handlers: Mutex<BTreeMap<String, Arc<dyn AcpiEventHandler>>>,
    /// Is initialized
    pub initialized: AtomicBool,
    /// Statistics
    pub stats: Mutex<GpeStats>,
}

#[derive(Clone, Debug, Default)]
pub struct GpeStats {
    pub gpes_handled: u64,
    pub fixed_events_handled: u64,
    pub spurious_events: u64,
}

pub trait AcpiEventHandler: Send + Sync {
    fn handle(&self, event: u32) -> Result<(), AcpiEventError>;
}

impl AcpiEventManager {
    pub const fn new() -> Self {
        Self {
            gpe_blocks: Mutex::new(Vec::new()),
            fixed_events: Mutex::new(BTreeMap::new()),
            handlers: Mutex::new(BTreeMap::new()),
            initialized: AtomicBool::new(false),
            stats: Mutex::new(GpeStats::default()),
        }
    }

    /// Initialize from FADT
    pub fn init(&self, gpe0_base: u32, gpe0_count: u32, gpe1_base: u32, gpe1_count: u32) {
        // Create GPE block 0
        if gpe0_count > 0 {
            let block0 = GpeBlock::new(0, gpe0_base, gpe0_count);
            block0.init();
            self.gpe_blocks.lock().push(block0);
        }
        
        // Create GPE block 1
        if gpe1_count > 0 {
            let block1 = GpeBlock::new(1, gpe1_base, gpe1_count);
            block1.init();
            self.gpe_blocks.lock().push(block1);
        }
        
        // Initialize fixed events
        let mut fixed = self.fixed_events.lock();
        fixed.insert(ACPI_EVENT_PMTIMER, FixedEvent::new(
            ACPI_EVENT_PMTIMER, "PMTIMER", 0, 0
        ));
        fixed.insert(ACPI_EVENT_POWER_BUTTON, FixedEvent::new(
            ACPI_EVENT_POWER_BUTTON, "POWER_BUTTON", 0, 0
        ));
        fixed.insert(ACPI_EVENT_SLEEP_BUTTON, FixedEvent::new(
            ACPI_EVENT_SLEEP_BUTTON, "SLEEP_BUTTON", 0, 0
        ));
        fixed.insert(ACPI_EVENT_RTC, FixedEvent::new(
            ACPI_EVENT_RTC, "RTC", 0, 0
        ));
        
        self.initialized.store(true, Ordering::SeqCst);
        
        crate::serial_println!("[GPE] Event manager initialized");
    }

    /// Install GPE handler
    pub fn install_gpe_handler(&self, gpe_number: u32, block: u8, handler: &str) -> Result<(), AcpiEventError> {
        let blocks = self.gpe_blocks.lock();
        
        if let Some(gpe_block) = blocks.iter().find(|b| b.block_number == block) {
            if let Some(event) = gpe_block.events.lock().get_mut(&gpe_number) {
                event.handler = Some(String::from(handler));
                event.handler_type.store(1, Ordering::SeqCst);
                return Ok(());
            }
        }
        
        Err(AcpiEventError::InvalidGpe)
    }

    /// Enable GPE
    pub fn enable_gpe(&self, gpe_number: u32, block: u8) -> Result<(), AcpiEventError> {
        let blocks = self.gpe_blocks.lock();
        
        if let Some(gpe_block) = blocks.iter().find(|b| b.block_number == block) {
            gpe_block.enable_gpe(gpe_number);
            return Ok(());
        }
        
        Err(AcpiEventError::InvalidGpe)
    }

    /// Disable GPE
    pub fn disable_gpe(&self, gpe_number: u32, block: u8) -> Result<(), AcpiEventError> {
        let blocks = self.gpe_blocks.lock();
        
        if let Some(gpe_block) = blocks.iter().find(|b| b.block_number == block) {
            gpe_block.disable_gpe(gpe_number);
            return Ok(());
        }
        
        Err(AcpiEventError::InvalidGpe)
    }

    /// Handle all events
    pub fn handle_events(&self) {
        // Handle GPE events
        for block in self.gpe_blocks.lock().iter() {
            let triggered = block.handle_events();
            
            let mut stats = self.stats.lock();
            stats.gpes_handled += triggered.len() as u64;
        }
        
        // Handle fixed events
        for event in self.fixed_events.lock().values() {
            if event.check_status() {
                event.clear_status();
                
                if let Some(ref handler) = event.handler {
                    crate::serial_println!("[GPE] Fixed event {} triggered", event.name);
                }
                
                let mut stats = self.stats.lock();
                stats.fixed_events_handled += 1;
            }
        }
    }

    /// Enable fixed event
    pub fn enable_fixed_event(&self, event_number: u32) -> Result<(), AcpiEventError> {
        let events = self.fixed_events.lock();
        
        if let Some(event) = events.get(&event_number) {
            event.enable();
            return Ok(());
        }
        
        Err(AcpiEventError::InvalidEvent)
    }

    /// Disable fixed event
    pub fn disable_fixed_event(&self, event_number: u32) -> Result<(), AcpiEventError> {
        let events = self.fixed_events.lock();
        
        if let Some(event) = events.get(&event_number) {
            event.disable();
            return Ok(());
        }
        
        Err(AcpiEventError::InvalidEvent)
    }

    /// Get statistics
    pub fn get_stats(&self) -> GpeStats {
        self.stats.lock().clone()
    }
}

lazy_static::lazy_static! {
    pub static ref ACPI_EVENTS: AcpiEventManager = AcpiEventManager::new();
}

// ============================================================================
// ERROR TYPE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpiEventError {
    InvalidGpe,
    InvalidEvent,
    HandlerAlreadyInstalled,
    NoHandler,
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init(gpe0_base: u32, gpe0_count: u32, gpe1_base: u32, gpe1_count: u32) {
    ACPI_EVENTS.init(gpe0_base, gpe0_count, gpe1_base, gpe1_count);
}
