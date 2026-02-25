//! # USB Hub Driver
//!
//! USB hub enumeration and port management
//! Supports both root hubs (part of xHCI) and external hubs

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::format;
use alloc::vec;
use spin::Mutex;
use core::mem;

use super::{UsbDevice, UsbError, UsbSpeed, UsbDeviceAddress, UsbEndpoint, XhciController, UsbSetupPacket, UsbClass};

// ============================================================================
// HUB CONSTANTS
// ============================================================================

/// Hub class code
pub const USB_CLASS_HUB: u8 = 0x09;

/// Hub subclass (multiple speed)
pub const USB_SUBCLASS_HUB: u8 = 0x00;

/// Hub protocol (full-speed only)
pub const USB_PROTOCOL_HUB_FS: u8 = 0x00;

/// Hub protocol (single TT)
pub const USB_PROTOCOL_HUB_TT_SINGLE: u8 = 0x01;

/// Hub protocol (multiple TT)
pub const USB_PROTOCOL_HUB_TT_MULTI: u8 = 0x02;

/// Hub request types
pub const HUB_GET_STATUS: u8 = 0x00;
pub const HUB_CLEAR_FEATURE: u8 = 0x01;
pub const HUB_SET_FEATURE: u8 = 0x03;
pub const HUB_GET_DESCRIPTOR: u8 = 0x06;
pub const HUB_SET_DESCRIPTOR: u8 = 0x07;
pub const HUB_CLEAR_TT_BUFFER: u8 = 0x08;
pub const HUB_RESET_TT: u8 = 0x09;
pub const HUB_GET_TT_STATE: u8 = 0x0A;
pub const HUB_STOP_TT: u8 = 0x0B;

/// Hub feature selectors
pub const HUB_C_HUB_LOCAL_POWER: u8 = 0x00;
pub const HUB_C_HUB_OVER_CURRENT: u8 = 0x01;
pub const HUB_PORT_CONNECTION: u8 = 0x00;
pub const HUB_PORT_ENABLE: u8 = 0x01;
pub const HUB_PORT_SUSPEND: u8 = 0x02;
pub const HUB_PORT_OVER_CURRENT: u8 = 0x03;
pub const HUB_PORT_RESET: u8 = 0x04;
pub const HUB_PORT_POWER: u8 = 0x08;
pub const HUB_PORT_LOW_SPEED: u8 = 0x09;
pub const HUB_PORT_HIGH_SPEED: u8 = 0x0A;
pub const HUB_C_PORT_CONNECTION: u8 = 0x10;
pub const HUB_C_PORT_ENABLE: u8 = 0x11;
pub const HUB_C_PORT_SUSPEND: u8 = 0x12;
pub const HUB_C_PORT_OVER_CURRENT: u8 = 0x13;
pub const HUB_C_PORT_RESET: u8 = 0x14;
pub const HUB_PORT_TEST: u8 = 0x15;
pub const HUB_PORT_INDICATOR: u8 = 0x16;

// ============================================================================
// HUB DESCRIPTOR
// ============================================================================

/// USB hub descriptor
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct HubDescriptor {
    /// Descriptor length
    pub b_desc_length: u8,
    /// Descriptor type (0x29)
    pub b_descriptor_type: u8,
    /// Number of downstream ports
    pub b_nbr_ports: u8,
    /// Hub characteristics (w_hub_characteristic)
    pub w_hub_characteristics: u16,
    /// Power on to power good (in 2ms units)
    pub b_pwr_on_2_pwr_good: u8,
    /// Hub control current (in mA)
    pub b_hub_control_current: u8,
    /// Device removable bitmap (variable length)
    pub device_removable: [u8; 2],
    /// Port power control mask (variable length)
    pub port_pwr_ctrl_mask: [u8; 2],
}

impl HubDescriptor {
    /// Parse hub descriptor from bytes
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 9 {
            return None;
        }
        
        let mut desc: HubDescriptor = unsafe { mem::zeroed() };
        desc.b_desc_length = data[0];
        desc.b_descriptor_type = data[1];
        desc.b_nbr_ports = data[2];
        desc.w_hub_characteristics = u16::from_le_bytes([data[3], data[4]]);
        desc.b_pwr_on_2_pwr_good = data[5];
        desc.b_hub_control_current = data[6];
        
        // Variable length fields
        let rem_len = ((desc.b_nbr_ports + 7) / 8) as usize;
        if data.len() >= 7 + rem_len {
            desc.device_removable[0] = data[7];
            if rem_len > 1 && data.len() >= 8 + rem_len {
                desc.device_removable[1] = data[8];
            }
        }
        
        Some(desc)
    }
    
    /// Get number of ports
    pub fn port_count(&self) -> u8 {
        self.b_nbr_ports
    }
    
    /// Check if hub is power switched
    pub fn is_power_switched(&self) -> bool {
        (self.w_hub_characteristics & 0x01) == 0
    }
    
    /// Check if hub is ganged power (all ports at once)
    pub fn is_ganged_power(&self) -> bool {
        (self.w_hub_characteristics & 0x02) == 0
    }
    
    /// Check if hub has over-current protection
    pub fn has_over_current(&self) -> bool {
        (self.w_hub_characteristics & 0x08) != 0
    }
    
    /// Check if hub has individual over-current protection
    pub fn has_individual_oc(&self) -> bool {
        (self.w_hub_characteristics & 0x10) != 0
    }
    
    /// Get TT think time (in FS bit times)
    pub fn tt_think_time(&self) -> u8 {
        ((self.w_hub_characteristics >> 5) & 0x03) as u8
    }
    
    /// Check if port is removable
    pub fn is_port_removable(&self, port: u8) -> bool {
        let byte_idx = port as usize / 8;
        let bit_idx = port as usize % 8;
        
        if byte_idx < self.device_removable.len() {
            (self.device_removable[byte_idx] & (1 << bit_idx)) != 0
        } else {
            false
        }
    }
}

// ============================================================================
// PORT STATUS
// ============================================================================

/// USB port status (w_port_status)
#[derive(Clone, Copy, Debug, Default)]
pub struct PortStatus {
    /// Raw status value
    pub raw: u16,
}

impl PortStatus {
    /// Device connected
    pub fn is_connected(&self) -> bool {
        (self.raw & (1 << 0)) != 0
    }
    
    /// Port enabled
    pub fn is_enabled(&self) -> bool {
        (self.raw & (1 << 1)) != 0
    }
    
    /// Port suspended
    pub fn is_suspended(&self) -> bool {
        (self.raw & (1 << 2)) != 0
    }
    
    /// Over-current active
    pub fn is_over_current(&self) -> bool {
        (self.raw & (1 << 3)) != 0
    }
    
    /// Port reset active
    pub fn is_reset(&self) -> bool {
        (self.raw & (1 << 4)) != 0
    }
    
    /// Port powered
    pub fn is_powered(&self) -> bool {
        (self.raw & (1 << 8)) != 0
    }
    
    /// Low-speed device
    pub fn is_low_speed(&self) -> bool {
        (self.raw & (1 << 9)) != 0
    }
    
    /// High-speed device
    pub fn is_high_speed(&self) -> bool {
        (self.raw & (1 << 10)) != 0
    }
    
    /// Port test mode
    pub fn is_test_mode(&self) -> bool {
        (self.raw & (1 << 11)) != 0
    }
    
    /// Port indicator
    pub fn has_indicator(&self) -> bool {
        (self.raw & (1 << 12)) != 0
    }
    
    /// Get device speed
    pub fn speed(&self) -> UsbSpeed {
        if self.is_high_speed() {
            UsbSpeed::High
        } else if self.is_low_speed() {
            UsbSpeed::Low
        } else if self.is_connected() {
            UsbSpeed::Full
        } else {
            UsbSpeed::Unknown
        }
    }
}

/// USB port change status (w_port_change)
#[derive(Clone, Copy, Debug, Default)]
pub struct PortChange {
    /// Raw change value
    pub raw: u16,
}

impl PortChange {
    /// Connection status changed
    pub fn connection_changed(&self) -> bool {
        (self.raw & (1 << 0)) != 0
    }
    
    /// Enable status changed
    pub fn enable_changed(&self) -> bool {
        (self.raw & (1 << 1)) != 0
    }
    
    /// Suspend status changed
    pub fn suspend_changed(&self) -> bool {
        (self.raw & (1 << 2)) != 0
    }
    
    /// Over-current changed
    pub fn over_current_changed(&self) -> bool {
        (self.raw & (1 << 3)) != 0
    }
    
    /// Reset complete
    pub fn reset_changed(&self) -> bool {
        (self.raw & (1 << 4)) != 0
    }
    
    /// Clear all changes
    pub fn clear_all(&mut self) {
        self.raw = 0;
    }
}

/// Combined port status and change
#[derive(Clone, Copy, Debug)]
pub struct PortState {
    pub status: PortStatus,
    pub change: PortChange,
}

impl PortState {
    pub fn new() -> Self {
        PortState {
            status: PortStatus::default(),
            change: PortChange::default(),
        }
    }
    
    /// Parse from 4-byte status data
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 4 {
            return None;
        }
        
        Some(PortState {
            status: PortStatus { raw: u16::from_le_bytes([data[0], data[1]]) },
            change: PortChange { raw: u16::from_le_bytes([data[2], data[3]]) },
        })
    }
}

// ============================================================================
// USB HUB
// ============================================================================

/// USB hub device
pub struct UsbHub {
    /// Hub device
    device: Arc<Mutex<UsbDevice>>,
    /// Hub descriptor
    descriptor: HubDescriptor,
    /// Port states
    ports: Vec<PortState>,
    /// Hub address
    address: UsbDeviceAddress,
    /// Hub tier (depth in topology)
    tier: u8,
    /// Is root hub
    is_root: bool,
    /// Transaction translator port (for low/full speed devices)
    tt_port: Option<u8>,
    /// Hub name
    name: String,
}

impl UsbHub {
    /// Create new hub from device
    pub fn new(device: Arc<Mutex<UsbDevice>>, tier: u8) -> Result<Self, UsbError> {
        let address = device.lock().address;
        
        // Get hub descriptor
        let desc_data = Self::get_hub_descriptor(&device)?;
        let descriptor = HubDescriptor::parse(&desc_data)
            .ok_or(UsbError::DescriptorError)?;
        
        let port_count = descriptor.port_count();
        let mut ports = Vec::with_capacity(port_count as usize);
        for _ in 0..port_count {
            ports.push(PortState::new());
        }
        
        let name = format!("hub-{}", address);
        
        crate::serial_println!("[USB-HUB] Found hub at address {}: {} ports", 
            address, port_count);
        
        Ok(UsbHub {
            device,
            descriptor,
            ports,
            address,
            tier,
            is_root: false,
            tt_port: None,
            name,
        })
    }
    
    /// Create root hub (xHCI virtual hub)
    pub fn create_root_hub(controller_idx: usize, port_count: u8) -> Self {
        let mut ports = Vec::with_capacity(port_count as usize);
        for _ in 0..port_count {
            ports.push(PortState::new());
        }
        
        let name = format!("root-hub-{}", controller_idx);
        
        UsbHub {
            device: Arc::new(Mutex::new(UsbDevice::default())),
            descriptor: HubDescriptor {
                b_desc_length: 9,
                b_descriptor_type: 0x29,
                b_nbr_ports: port_count,
                w_hub_characteristics: 0x0009, // Ganged power, individual OC
                b_pwr_on_2_pwr_good: 50,       // 100ms
                b_hub_control_current: 0,
                device_removable: [0; 2],
                port_pwr_ctrl_mask: [0; 2],
            },
            ports,
            address: 0,
            tier: 0,
            is_root: true,
            tt_port: None,
            name,
        }
    }
    
    /// Get hub descriptor from device
    fn get_hub_descriptor(device: &Arc<Mutex<UsbDevice>>) -> Result<Vec<u8>, UsbError> {
        let mut dev = device.lock();
        
        // Setup packet for GET_DESCRIPTOR
        let setup = UsbSetupPacket {
            request_type: 0xA0,  // Type: Class, Interface-to-Host
            request: HUB_GET_DESCRIPTOR,
            value: 0x2900,  // Descriptor type (HUB) << 8 | index
            index: 0,
            length: 64,
        };
        
        // Allocate buffer
        let mut buffer = vec![0u8; 64];
        
        // Send control request
        dev.control_transfer(setup, Some(&mut buffer))?;
        
        Ok(buffer)
    }
    
    /// Get port count
    pub fn port_count(&self) -> u8 {
        self.descriptor.port_count()
    }
    
    /// Get hub tier
    pub fn tier(&self) -> u8 {
        self.tier
    }
    
    /// Is root hub
    pub fn is_root_hub(&self) -> bool {
        self.is_root
    }
    
    /// Get hub name
    pub fn name(&self) -> &str {
        &self.name
    }
    
    /// Power on all ports
    pub fn power_on_ports(&mut self) -> Result<(), UsbError> {
        for port in 1..=self.port_count() {
            self.set_port_feature(port, HUB_PORT_POWER)?;
        }
        
        // Wait for power good
        let delay_ms = self.descriptor.b_pwr_on_2_pwr_good as u64 * 2;
        crate::task::scheduler::sleep(delay_ms as usize);
        
        crate::serial_println!("[USB-HUB] Ports powered on ({}ms delay)", delay_ms);
        Ok(())
    }
    
    /// Get port status
    pub fn get_port_status(&mut self, port: u8) -> Result<PortState, UsbError> {
        if port == 0 || port > self.port_count() {
            return Err(UsbError::InvalidPort);
        }
        
        if self.is_root {
            // Root hub status comes from xHCI
            return Ok(self.ports[(port - 1) as usize]);
        }
        
        let mut dev = self.device.lock();
        
        // Setup packet for GET_STATUS
        let setup = UsbSetupPacket {
            request_type: 0xA3,  // Type: Class, Other-to-Host
            request: HUB_GET_STATUS,
            value: 0,
            index: port as u16,  // Port number
            length: 4,
        };
        
        let mut buffer = [0u8; 4];
        dev.control_transfer(setup, Some(&mut buffer))?;
        
        let state = PortState::parse(&buffer).ok_or(UsbError::TransferError)?;
        self.ports[(port - 1) as usize] = state;
        
        Ok(state)
    }
    
    /// Set port feature
    pub fn set_port_feature(&mut self, port: u8, feature: u8) -> Result<(), UsbError> {
        if port == 0 || port > self.port_count() {
            return Err(UsbError::InvalidPort);
        }
        
        if self.is_root {
            // Root hub handled by xHCI
            return Ok(());
        }
        
        let mut dev = self.device.lock();
        
        let setup = UsbSetupPacket {
            request_type: 0x23,  // Type: Class, Other-to-Device
            request: HUB_SET_FEATURE,
            value: feature as u16,
            index: port as u16,  // Port number
            length: 0,
        };
        
        dev.control_transfer(setup, None)?;
        
        Ok(())
    }
    
    /// Clear port feature
    pub fn clear_port_feature(&mut self, port: u8, feature: u8) -> Result<(), UsbError> {
        if port == 0 || port > self.port_count() {
            return Err(UsbError::InvalidPort);
        }
        
        if self.is_root {
            return Ok(());
        }
        
        let mut dev = self.device.lock();
        
        let setup = UsbSetupPacket {
            request_type: 0x23,  // Type: Class, Other-to-Device
            request: HUB_CLEAR_FEATURE,
            value: feature as u16,
            index: port as u16,
            length: 0,
        };
        
        dev.control_transfer(setup, None)?;
        
        Ok(())
    }
    
    /// Reset port
    pub fn reset_port(&mut self, port: u8) -> Result<UsbSpeed, UsbError> {
        if port == 0 || port > self.port_count() {
            return Err(UsbError::InvalidPort);
        }
        
        crate::serial_println!("[USB-HUB] Resetting port {}", port);
        
        // Issue reset
        self.set_port_feature(port, HUB_PORT_RESET)?;
        
        // Wait for reset to complete (up to 500ms)
        for _ in 0..50 {
            crate::task::scheduler::sleep(10);
            
            let state = self.get_port_status(port)?;
            
            if state.change.reset_changed() {
                // Clear reset change
                self.clear_port_feature(port, HUB_C_PORT_RESET)?;
                
                if state.status.is_enabled() {
                    let speed = state.status.speed();
                    crate::serial_println!("[USB-HUB] Port {} reset complete, speed={:?}", 
                        port, speed);
                    return Ok(speed);
                }
            }
        }
        
        Err(UsbError::Timeout)
    }
    
    /// Clear port connection change
    pub fn clear_connection_change(&mut self, port: u8) -> Result<(), UsbError> {
        self.clear_port_feature(port, HUB_C_PORT_CONNECTION)
    }
    
    /// Disable port
    pub fn disable_port(&mut self, port: u8) -> Result<(), UsbError> {
        self.clear_port_feature(port, HUB_PORT_ENABLE)
    }
    
    /// Suspend port
    pub fn suspend_port(&mut self, port: u8) -> Result<(), UsbError> {
        self.set_port_feature(port, HUB_PORT_SUSPEND)
    }
    
    /// Resume port
    pub fn resume_port(&mut self, port: u8) -> Result<(), UsbError> {
        self.clear_port_feature(port, HUB_PORT_SUSPEND)
    }
    
    /// Poll hub for port changes
    pub fn poll(&mut self) -> Result<Vec<u8>, UsbError> {
        let mut changed_ports = Vec::new();
        
        for port in 1..=self.port_count() {
            let state = self.get_port_status(port)?;
            
            if state.change.connection_changed() {
                changed_ports.push(port);
            }
        }
        
        Ok(changed_ports)
    }
    
    /// Check if device is removable
    pub fn is_device_removable(&self, port: u8) -> bool {
        self.descriptor.is_port_removable(port)
    }
}

// ============================================================================
// HUB MANAGER
// ============================================================================

/// Global hub registry
lazy_static::lazy_static! {
    static ref HUB_REGISTRY: Mutex<BTreeMap<String, Arc<Mutex<UsbHub>>>> = Mutex::new(BTreeMap::new());
    static ref ROOT_HUBS: Mutex<Vec<Arc<Mutex<UsbHub>>>> = Mutex::new(Vec::new());
}

/// Register a hub
pub fn register_hub(hub: UsbHub) -> Arc<Mutex<UsbHub>> {
    let name = hub.name().to_string();
    let is_root = hub.is_root_hub();
    let hub = Arc::new(Mutex::new(hub));
    
    HUB_REGISTRY.lock().insert(name.clone(), hub.clone());
    
    if is_root {
        ROOT_HUBS.lock().push(hub.clone());
    }
    
    crate::serial_println!("[USB-HUB] Registered hub: {}", name);
    hub
}

/// Get hub by name
pub fn get_hub(name: &str) -> Option<Arc<Mutex<UsbHub>>> {
    HUB_REGISTRY.lock().get(name).cloned()
}

/// Get all root hubs
pub fn get_root_hubs() -> Vec<Arc<Mutex<UsbHub>>> {
    ROOT_HUBS.lock().clone()
}

/// Poll all hubs for changes
pub fn poll_all_hubs() -> Vec<(String, Vec<u8>)> {
    let mut changes = Vec::new();
    
    let hubs = HUB_REGISTRY.lock();
    for (name, hub) in hubs.iter() {
        if let Ok(changed_ports) = hub.lock().poll() {
            if !changed_ports.is_empty() {
                changes.push((name.clone(), changed_ports));
            }
        }
    }
    
    changes
}

/// Initialize hub driver
pub fn init() {
    crate::serial_println!("[USB-HUB] Hub driver initialized");
}

// ============================================================================
// HUB ENUMERATION
// ============================================================================

/// Check if device is a hub
pub fn is_hub_device(device: &UsbDevice) -> bool {
    device.device_class == UsbClass::Hub
}

/// Enumerate hub ports
pub fn enumerate_hub_ports(hub: &mut UsbHub, enumerate_device: fn(&mut UsbDevice, u8) -> Result<(), UsbError>) -> Result<(), UsbError> {
    // Power on ports
    hub.power_on_ports()?;
    
    // Check each port
    for port in 1..=hub.port_count() {
        let state = hub.get_port_status(port)?;
        
        if state.status.is_connected() && state.change.connection_changed() {
            // Clear connection change
            hub.clear_connection_change(port)?;
            
            // Reset port
            let speed = hub.reset_port(port)?;
            
            crate::serial_println!("[USB-HUB] Port {} has device, speed={:?}", port, speed);
            
            // Device will be enumerated by caller
            // At this point, the device is in Default state with address 0
        }
    }
    
    Ok(())
}
