//! # Thermal Zone
//!
//! Temperature monitoring and cooling device support.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// THERMAL CONSTANTS
// ============================================================================

/// Trip point types
pub const THERMAL_TRIP_ACTIVE: u32 = 0;
pub const THERMAL_TRIP_PASSIVE: u32 = 1;
pub const THERMAL_TRIP_HOT: u32 = 2;
pub const THERMAL_TRIP_CRITICAL: u32 = 3;

/// Cooling states
pub const THERMAL_NO_LIMIT: u32 = u32::MAX;

/// Default polling delay (ms)
pub const THERMAL_POLLING_DELAY: u32 = 1000;
pub const THERMAL_PASSIVE_DELAY: u32 = 1000;

// ============================================================================
// TRIP POINT
// ============================================================================

#[derive(Clone, Debug)]
pub struct TripPoint {
    /// Trip point ID
    pub id: u32,
    /// Temperature in millidegrees Celsius
    pub temperature: AtomicI32,
    /// Hysteresis in millidegrees
    pub hysteresis: AtomicI32,
    /// Trip type
    pub trip_type: u32,
    /// Is enabled
    pub enabled: AtomicBool,
}

impl TripPoint {
    pub fn new(id: u32, temp: i32, hyst: i32, trip_type: u32) -> Self {
        Self {
            id,
            temperature: AtomicI32::new(temp),
            hysteresis: AtomicI32::new(hyst),
            trip_type,
            enabled: AtomicBool::new(true),
        }
    }

    /// Check if temperature exceeds trip
    pub fn is_exceeded(&self, temp: i32) -> bool {
        if !self.enabled.load(Ordering::Relaxed) {
            return false;
        }
        temp >= self.temperature.load(Ordering::Relaxed)
    }

    /// Check if temperature is below hysteresis
    pub fn is_below_hysteresis(&self, temp: i32) -> bool {
        let trip = self.temperature.load(Ordering::Relaxed);
        let hyst = self.hysteresis.load(Ordering::Relaxed);
        temp < trip - hyst
    }
}

// ============================================================================
// THERMAL ZONE
// ============================================================================

pub struct ThermalZone {
    /// Zone ID
    pub id: u32,
    /// Zone name
    pub name: String,
    /// Zone type
    pub zone_type: String,
    /// Current temperature
    pub temperature: AtomicI32,
    /// Trip points
    pub trips: Mutex<Vec<TripPoint>>,
    /// Cooling devices
    pub cooling_devices: Mutex<Vec<Arc<CoolingDevice>>>,
    /// Governor
    pub governor: Mutex<String>,
    /// Polling delay
    pub polling_delay: AtomicU32,
    /// Passive delay
    pub passive_delay: AtomicU32,
    /// Is passive cooling active
    pub passive_active: AtomicBool,
    /// Last update time
    pub last_update: AtomicU64,
    /// Zone mode
    pub mode: AtomicBool, // true = enabled
}

impl ThermalZone {
    pub fn new(id: u32, name: &str, zone_type: &str) -> Self {
        Self {
            id,
            name: String::from(name),
            zone_type: String::from(zone_type),
            temperature: AtomicI32::new(0),
            trips: Mutex::new(Vec::new()),
            cooling_devices: Mutex::new(Vec::new()),
            governor: Mutex::new(String::from("step_wise")),
            polling_delay: AtomicU32::new(THERMAL_POLLING_DELAY),
            passive_delay: AtomicU32::new(THERMAL_PASSIVE_DELAY),
            passive_active: AtomicBool::new(false),
            last_update: AtomicU64::new(0),
            mode: AtomicBool::new(true),
        }
    }

    /// Add trip point
    pub fn add_trip(&self, trip: TripPoint) {
        self.trips.lock().push(trip);
    }

    /// Add cooling device
    pub fn add_cooling(&self, device: Arc<CoolingDevice>) {
        self.cooling_devices.lock().push(device);
    }

    /// Update temperature
    pub fn update_temperature(&self, temp: i32) {
        self.temperature.store(temp, Ordering::SeqCst);
        self.last_update.store(
            crate::task::scheduler::get_ticks(),
            Ordering::SeqCst
        );
        
        // Check trip points
        self.check_trips();
    }

    /// Check trip points
    fn check_trips(&self) {
        let temp = self.temperature.load(Ordering::Relaxed);
        let trips = self.trips.lock();
        
        for trip in trips.iter() {
            if trip.is_exceeded(temp) {
                self.handle_trip(trip);
            }
        }
    }

    /// Handle trip point
    fn handle_trip(&self, trip: &TripPoint) {
        match trip.trip_type {
            THERMAL_TRIP_CRITICAL => {
                // Critical temperature - shutdown
                crate::serial_println!(
                    "[THERMAL] CRITICAL: {} reached {}°C - shutting down!",
                    self.name,
                    trip.temperature.load(Ordering::Relaxed) / 1000
                );
                // Emergency shutdown
            }
            THERMAL_TRIP_HOT => {
                // Hot trip - aggressive cooling
                self.activate_max_cooling();
            }
            THERMAL_TRIP_PASSIVE => {
                // Passive trip - activate passive cooling
                self.passive_active.store(true, Ordering::SeqCst);
                self.activate_cooling();
            }
            THERMAL_TRIP_ACTIVE => {
                // Active trip - activate cooling
                self.activate_cooling();
            }
            _ => {}
        }
    }

    /// Activate cooling
    fn activate_cooling(&self) {
        let cooling_devices = self.cooling_devices.lock();
        for device in cooling_devices.iter() {
            let _ = device.set_cur_state(device.get_max_state());
        }
    }

    /// Activate maximum cooling
    fn activate_max_cooling(&self) {
        let cooling_devices = self.cooling_devices.lock();
        for device in cooling_devices.iter() {
            let _ = device.set_cur_state(device.get_max_state());
        }
    }

    /// Get temperature
    pub fn get_temperature(&self) -> i32 {
        self.temperature.load(Ordering::Relaxed)
    }

    /// Enable/disable zone
    pub fn set_mode(&self, enabled: bool) {
        self.mode.store(enabled, Ordering::SeqCst);
    }
}

// ============================================================================
// COOLING DEVICE
// ============================================================================

pub struct CoolingDevice {
    /// Device ID
    pub id: u32,
    /// Device name
    pub name: String,
    /// Device type
    pub cooling_type: String,
    /// Current state
    pub cur_state: AtomicU32,
    /// Maximum state
    pub max_state: AtomicU32,
    /// Min state
    pub min_state: AtomicU32,
    /// Operations
    pub ops: Option<&'static dyn CoolingOps>,
}

pub trait CoolingOps: Send + Sync {
    fn get_max_state(&self) -> u32;
    fn get_cur_state(&self) -> u32;
    fn set_cur_state(&self, state: u32) -> Result<(), ThermalError>;
    fn get_requested_power(&self, state: u32) -> u32;
    fn state2power(&self, state: u32) -> u32;
    fn power2state(&self, power: u32) -> u32;
}

impl CoolingDevice {
    pub fn new(id: u32, name: &str, cooling_type: &str) -> Self {
        Self {
            id,
            name: String::from(name),
            cooling_type: String::from(cooling_type),
            cur_state: AtomicU32::new(0),
            max_state: AtomicU32::new(10),
            min_state: AtomicU32::new(0),
            ops: None,
        }
    }

    pub fn get_max_state(&self) -> u32 {
        self.max_state.load(Ordering::Relaxed)
    }

    pub fn get_cur_state(&self) -> u32 {
        self.cur_state.load(Ordering::Relaxed)
    }

    pub fn set_cur_state(&self, state: u32) -> Result<(), ThermalError> {
        let max = self.max_state.load(Ordering::Relaxed);
        let min = self.min_state.load(Ordering::Relaxed);
        
        if state > max || state < min {
            return Err(ThermalError::InvalidState);
        }
        
        if let Some(ops) = self.ops {
            ops.set_cur_state(state)?;
        }
        
        self.cur_state.store(state, Ordering::SeqCst);
        Ok(())
    }
}

// ============================================================================
// CPU COOLING
// ============================================================================

pub struct CpuCooling {
    cpu_id: u32,
    frequencies: Vec<u32>,
    current_freq_idx: AtomicU32,
}

impl CpuCooling {
    pub fn new(cpu_id: u32, frequencies: Vec<u32>) -> Self {
        Self {
            cpu_id,
            frequencies,
            current_freq_idx: AtomicU32::new(0),
        }
    }
}

impl CoolingOps for CpuCooling {
    fn get_max_state(&self) -> u32 {
        (self.frequencies.len() - 1) as u32
    }

    fn get_cur_state(&self) -> u32 {
        self.current_freq_idx.load(Ordering::Relaxed)
    }

    fn set_cur_state(&self, state: u32) -> Result<(), ThermalError> {
        if state as usize >= self.frequencies.len() {
            return Err(ThermalError::InvalidState);
        }
        
        // Set CPU frequency
        let freq = self.frequencies[state as usize];
        // crate::cpu::set_frequency(self.cpu_id, freq);
        
        self.current_freq_idx.store(state, Ordering::SeqCst);
        Ok(())
    }

    fn get_requested_power(&self, _state: u32) -> u32 {
        0
    }

    fn state2power(&self, _state: u32) -> u32 {
        0
    }

    fn power2state(&self, _power: u32) -> u32 {
        0
    }
}

// ============================================================================
// FAN COOLING
// ============================================================================

pub struct FanCooling {
    fan_id: u32,
    speeds: Vec<u32>, // RPM values
    current_speed_idx: AtomicU32,
}

impl FanCooling {
    pub fn new(fan_id: u32, speeds: Vec<u32>) -> Self {
        Self {
            fan_id,
            speeds,
            current_speed_idx: AtomicU32::new(0),
        }
    }
}

impl CoolingOps for FanCooling {
    fn get_max_state(&self) -> u32 {
        (self.speeds.len() - 1) as u32
    }

    fn get_cur_state(&self) -> u32 {
        self.current_speed_idx.load(Ordering::Relaxed)
    }

    fn set_cur_state(&self, state: u32) -> Result<(), ThermalError> {
        if state as usize >= self.speeds.len() {
            return Err(ThermalError::InvalidState);
        }
        
        // Set fan speed
        let rpm = self.speeds[state as usize];
        // crate::drivers::fan::set_speed(self.fan_id, rpm);
        
        self.current_speed_idx.store(state, Ordering::SeqCst);
        Ok(())
    }

    fn get_requested_power(&self, _state: u32) -> u32 {
        0
    }

    fn state2power(&self, _state: u32) -> u32 {
        0
    }

    fn power2state(&self, _power: u32) -> u32 {
        0
    }
}

// ============================================================================
// THERMAL MANAGER
// ============================================================================

pub struct ThermalManager {
    zones: Mutex<BTreeMap<u32, Arc<ThermalZone>>>,
    cooling_devices: Mutex<BTreeMap<u32, Arc<CoolingDevice>>>,
    next_zone_id: AtomicU32,
    next_cooling_id: AtomicU32,
}

impl ThermalManager {
    pub const fn new() -> Self {
        Self {
            zones: Mutex::new(BTreeMap::new()),
            cooling_devices: Mutex::new(BTreeMap::new()),
            next_zone_id: AtomicU32::new(0),
            next_cooling_id: AtomicU32::new(0),
        }
    }

    pub fn register_zone(&self, name: &str, zone_type: &str) -> Arc<ThermalZone> {
        let id = self.next_zone_id.fetch_add(1, Ordering::SeqCst);
        let zone = Arc::new(ThermalZone::new(id, name, zone_type));
        self.zones.lock().insert(id, zone.clone());
        zone
    }

    pub fn register_cooling(&self, name: &str, cooling_type: &str) -> Arc<CoolingDevice> {
        let id = self.next_cooling_id.fetch_add(1, Ordering::SeqCst);
        let device = Arc::new(CoolingDevice::new(id, name, cooling_type));
        self.cooling_devices.lock().insert(id, device.clone());
        device
    }

    pub fn get_zone(&self, id: u32) -> Option<Arc<ThermalZone>> {
        self.zones.lock().get(&id).cloned()
    }

    pub fn get_cooling(&self, id: u32) -> Option<Arc<CoolingDevice>> {
        self.cooling_devices.lock().get(&id).cloned()
    }

    /// Update all zones
    pub fn update_all(&self) {
        for zone in self.zones.lock().values() {
            // Read temperature from sensor
            // zone.update_temperature(temp);
        }
    }
}

lazy_static::lazy_static! {
    pub static ref THERMAL_MANAGER: ThermalManager = ThermalManager::new();
}

// ============================================================================
// ERROR TYPE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThermalError {
    InvalidState,
    DeviceNotFound,
    ZoneNotFound,
    SensorError,
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init() {
    // Create CPU thermal zone
    let cpu_zone = THERMAL_MANAGER.register_zone("cpu-thermal", "cpu");
    
    // Add trip points
    cpu_zone.add_trip(TripPoint::new(0, 85000, 5000, THERMAL_TRIP_PASSIVE));
    cpu_zone.add_trip(TripPoint::new(1, 95000, 2000, THERMAL_TRIP_HOT));
    cpu_zone.add_trip(TripPoint::new(2, 105000, 0, THERMAL_TRIP_CRITICAL));
    
    crate::serial_println!("[THERMAL] Subsystem initialized");
}
