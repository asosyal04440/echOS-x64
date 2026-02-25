//! # Driver Model
//!
//! Linux-like driver registration and sysfs interface.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::{Mutex, RwLock};

// ============================================================================
// DEVICE TYPES
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceType {
    Char,
    Block,
    Net,
    Misc,
    Usb,
    Pci,
    Platform,
    I2C,
    Spi,
    Hwmon,
    Gpu,
    Audio,
    Input,
    Gpio,
    Thermal,
}

impl DeviceType {
    pub fn name(&self) -> &'static str {
        match self {
            DeviceType::Char => "char",
            DeviceType::Block => "block",
            DeviceType::Net => "net",
            DeviceType::Misc => "misc",
            DeviceType::Usb => "usb",
            DeviceType::Pci => "pci",
            DeviceType::Platform => "platform",
            DeviceType::I2C => "i2c",
            DeviceType::Spi => "spi",
            DeviceType::Hwmon => "hwmon",
            DeviceType::Gpu => "gpu",
            DeviceType::Audio => "audio",
            DeviceType::Input => "input",
            DeviceType::Gpio => "gpio",
            DeviceType::Thermal => "thermal",
        }
    }
}

// ============================================================================
// DEVICE
// ============================================================================

pub struct Device {
    /// Device ID
    pub id: u64,
    /// Device name
    pub name: String,
    /// Device type
    pub dev_type: DeviceType,
    /// Parent device
    pub parent: Option<u64>,
    /// Children
    pub children: Mutex<Vec<u64>>,
    /// Driver bound
    pub driver: Mutex<Option<u64>>,
    /// Device attributes
    pub attrs: Mutex<BTreeMap<String, String>>,
    /// Is initialized
    pub initialized: AtomicBool,
    /// Is suspended
    pub suspended: AtomicBool,
    /// Reference count
    pub ref_count: AtomicU32,
    /// Private data
    pub private: Mutex<Option<u64>>,
}

impl Device {
    pub fn new(id: u64, name: &str, dev_type: DeviceType) -> Self {
        Self {
            id,
            name: String::from(name),
            dev_type,
            parent: None,
            children: Mutex::new(Vec::new()),
            driver: Mutex::new(None),
            attrs: Mutex::new(BTreeMap::new()),
            initialized: AtomicBool::new(false),
            suspended: AtomicBool::new(false),
            ref_count: AtomicU32::new(1),
            private: Mutex::new(None),
        }
    }

    /// Set attribute
    pub fn set_attr(&self, name: &str, value: &str) {
        self.attrs.lock().insert(String::from(name), String::from(value));
    }

    /// Get attribute
    pub fn get_attr(&self, name: &str) -> Option<String> {
        self.attrs.lock().get(name).cloned()
    }

    /// Add child device
    pub fn add_child(&self, child_id: u64) {
        self.children.lock().push(child_id);
    }

    /// Bind driver
    pub fn bind_driver(&self, driver_id: u64) {
        *self.driver.lock() = Some(driver_id);
    }
}

// ============================================================================
// DRIVER
// ============================================================================

pub struct Driver {
    /// Driver ID
    pub id: u64,
    /// Driver name
    pub name: String,
    /// Devices bound to this driver
    pub devices: Mutex<Vec<u64>>,
    /// Probe function
    pub probe_fn: Option<fn(&Device) -> Result<(), DriverError>>,
    /// Remove function
    pub remove_fn: Option<fn(&Device) -> Result<(), DriverError>>,
    /// Suspend function
    pub suspend_fn: Option<fn(&Device) -> Result<(), DriverError>>,
    /// Resume function
    pub resume_fn: Option<fn(&Device) -> Result<(), DriverError>>,
    /// Shutdown function
    pub shutdown_fn: Option<fn(&Device)>,
}

impl Driver {
    pub fn new(id: u64, name: &str) -> Self {
        Self {
            id,
            name: String::from(name),
            devices: Mutex::new(Vec::new()),
            probe_fn: None,
            remove_fn: None,
            suspend_fn: None,
            resume_fn: None,
            shutdown_fn: None,
        }
    }

    /// Probe device
    pub fn probe(&self, device: &Device) -> Result<(), DriverError> {
        if let Some(probe) = self.probe_fn {
            probe(device)?;
        }
        device.initialized.store(true, Ordering::SeqCst);
        self.devices.lock().push(device.id);
        Ok(())
    }

    /// Remove device
    pub fn remove(&self, device: &Device) -> Result<(), DriverError> {
        if let Some(remove) = self.remove_fn {
            remove(device)?;
        }
        self.devices.lock().retain(|&id| id != device.id);
        Ok(())
    }

    /// Suspend device
    pub fn suspend(&self, device: &Device) -> Result<(), DriverError> {
        if let Some(suspend) = self.suspend_fn {
            suspend(device)?;
        }
        device.suspended.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Resume device
    pub fn resume(&self, device: &Device) -> Result<(), DriverError> {
        if let Some(resume) = self.resume_fn {
            resume(device)?;
        }
        device.suspended.store(false, Ordering::SeqCst);
        Ok(())
    }

    /// Shutdown device
    pub fn shutdown(&self, device: &Device) {
        if let Some(shutdown) = self.shutdown_fn {
            shutdown(device);
        }
    }
}

// ============================================================================
// DEVICE CLASS
// ============================================================================

pub struct DeviceClass {
    pub name: String,
    pub dev_type: DeviceType,
    pub devices: Mutex<Vec<u64>>,
}

impl DeviceClass {
    pub fn new(name: &str, dev_type: DeviceType) -> Self {
        Self {
            name: String::from(name),
            dev_type,
            devices: Mutex::new(Vec::new()),
        }
    }

    pub fn add_device(&self, device_id: u64) {
        self.devices.lock().push(device_id);
    }
}

// ============================================================================
// DRIVER MODEL MANAGER
// ============================================================================

pub struct DriverModel {
    /// All devices
    devices: RwLock<BTreeMap<u64, Arc<Device>>>,
    /// All drivers
    drivers: RwLock<BTreeMap<u64, Arc<Driver>>>,
    /// Device classes
    classes: RwLock<BTreeMap<String, Arc<DeviceClass>>>,
    /// Next device ID
    next_device_id: AtomicU64,
    /// Next driver ID
    next_driver_id: AtomicU64,
    /// Device name index
    device_names: Mutex<BTreeMap<String, u64>>,
}

impl DriverModel {
    pub const fn new() -> Self {
        Self {
            devices: RwLock::new(BTreeMap::new()),
            drivers: RwLock::new(BTreeMap::new()),
            classes: RwLock::new(BTreeMap::new()),
            next_device_id: AtomicU64::new(1),
            next_driver_id: AtomicU64::new(1),
            device_names: Mutex::new(BTreeMap::new()),
        }
    }

    /// Register device
    pub fn register_device(&self, name: &str, dev_type: DeviceType) -> Arc<Device> {
        let id = self.next_device_id.fetch_add(1, Ordering::SeqCst);
        let device = Arc::new(Device::new(id, name, dev_type));
        
        self.devices.write().insert(id, device.clone());
        self.device_names.lock().insert(String::from(name), id);
        
        crate::serial_println!("[DRIVER] Registered device '{}' (id={})", name, id);
        
        device
    }

    /// Unregister device
    pub fn unregister_device(&self, id: u64) {
        if let Some(device) = self.devices.write().remove(&id) {
            self.device_names.lock().remove(&device.name);
        }
    }

    /// Register driver
    pub fn register_driver(&self, name: &str) -> Arc<Driver> {
        let id = self.next_driver_id.fetch_add(1, Ordering::SeqCst);
        let driver = Arc::new(Driver::new(id, name));
        
        self.drivers.write().insert(id, driver.clone());
        
        crate::serial_println!("[DRIVER] Registered driver '{}' (id={})", name, id);
        
        driver
    }

    /// Register class
    pub fn register_class(&self, name: &str, dev_type: DeviceType) -> Arc<DeviceClass> {
        let class = Arc::new(DeviceClass::new(name, dev_type));
        self.classes.write().insert(String::from(name), class.clone());
        class
    }

    /// Bind driver to device
    pub fn bind(&self, device_id: u64, driver_id: u64) -> Result<(), DriverError> {
        let device = self.devices.read().get(&device_id).cloned()
            .ok_or(DriverError::DeviceNotFound)?;
        let driver = self.drivers.read().get(&driver_id).cloned()
            .ok_or(DriverError::DriverNotFound)?;
        
        driver.probe(&device)?;
        device.bind_driver(driver_id);
        
        crate::serial_println!("[DRIVER] Bound driver {} to device {}", driver_id, device_id);
        
        Ok(())
    }

    /// Unbind driver from device
    pub fn unbind(&self, device_id: u64) -> Result<(), DriverError> {
        let device = self.devices.read().get(&device_id).cloned()
            .ok_or(DriverError::DeviceNotFound)?;
        
        if let Some(driver_id) = *device.driver.lock() {
            if let Some(driver) = self.drivers.read().get(&driver_id) {
                driver.remove(&device)?;
            }
        }
        
        *device.driver.lock() = None;
        Ok(())
    }

    /// Get device by ID
    pub fn get_device(&self, id: u64) -> Option<Arc<Device>> {
        self.devices.read().get(&id).cloned()
    }

    /// Get device by name
    pub fn get_device_by_name(&self, name: &str) -> Option<Arc<Device>> {
        let id = self.device_names.lock().get(name).copied()?;
        self.devices.read().get(&id).cloned()
    }

    /// Get driver by ID
    pub fn get_driver(&self, id: u64) -> Option<Arc<Driver>> {
        self.drivers.read().get(&id).cloned()
    }

    /// List all devices
    pub fn list_devices(&self) -> Vec<(u64, String, DeviceType)> {
        self.devices.read()
            .iter()
            .map(|(id, dev)| (*id, dev.name.clone(), dev.dev_type))
            .collect()
    }

    /// Suspend all devices
    pub fn suspend_all(&self) {
        for device in self.devices.read().values() {
            if let Some(driver_id) = *device.driver.lock() {
                if let Some(driver) = self.drivers.read().get(&driver_id) {
                    let _ = driver.suspend(device);
                }
            }
        }
    }

    /// Resume all devices
    pub fn resume_all(&self) {
        for device in self.devices.read().values() {
            if let Some(driver_id) = *device.driver.lock() {
                if let Some(driver) = self.drivers.read().get(&driver_id) {
                    let _ = driver.resume(device);
                }
            }
        }
    }

    /// Shutdown all devices
    pub fn shutdown_all(&self) {
        for device in self.devices.read().values() {
            if let Some(driver_id) = *device.driver.lock() {
                if let Some(driver) = self.drivers.read().get(&driver_id) {
                    driver.shutdown(device);
                }
            }
        }
    }
}

lazy_static::lazy_static! {
    pub static ref DRIVER_MODEL: DriverModel = DriverModel::new();
}

// ============================================================================
// SYSFS-LIKE INTERFACE
// ============================================================================

/// Read sysfs attribute
pub fn sysfs_read(path: &str) -> Option<String> {
    // Parse path: /sys/class/<class>/<device>/<attr>
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    
    if parts.len() < 4 || parts[0] != "sys" {
        return None;
    }
    
    let device_name = parts[2];
    let attr_name = parts[3];
    
    if let Some(device) = DRIVER_MODEL.get_device_by_name(device_name) {
        return device.get_attr(attr_name);
    }
    
    None
}

/// Write sysfs attribute
pub fn sysfs_write(path: &str, value: &str) -> bool {
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    
    if parts.len() < 4 || parts[0] != "sys" {
        return false;
    }
    
    let device_name = parts[2];
    let attr_name = parts[3];
    
    if let Some(device) = DRIVER_MODEL.get_device_by_name(device_name) {
        device.set_attr(attr_name, value);
        return true;
    }
    
    false
}

// ============================================================================
// ERROR TYPE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverError {
    DeviceNotFound,
    DriverNotFound,
    ProbeFailed,
    RemoveFailed,
    SuspendFailed,
    ResumeFailed,
    NotBound,
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init() {
    // Register default classes
    DRIVER_MODEL.register_class("char", DeviceType::Char);
    DRIVER_MODEL.register_class("block", DeviceType::Block);
    DRIVER_MODEL.register_class("net", DeviceType::Net);
    DRIVER_MODEL.register_class("misc", DeviceType::Misc);
    
    crate::serial_println!("[DRIVER] Driver model initialized");
}
