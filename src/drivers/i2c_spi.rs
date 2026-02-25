//! # I2C and SPI Subsystem
//!
//! I2C and SPI bus support.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

// ============================================================================
// I2C CONSTANTS
// ============================================================================

/// I2C standard mode (100 kHz)
pub const I2C_STANDARD_MODE: u32 = 100_000;
/// I2C fast mode (400 kHz)
pub const I2C_FAST_MODE: u32 = 400_000;
/// I2C fast mode plus (1 MHz)
pub const I2C_FAST_MODE_PLUS: u32 = 1_000_000;
/// I2C high speed mode (3.4 MHz)
pub const I2C_HIGH_SPEED_MODE: u32 = 3_400_000;

/// I2C commands
pub const I2C_RETRIES: u32 = 0x0701;
pub const I2C_TIMEOUT: u32 = 0x0702;
pub const I2C_SLAVE: u32 = 0x0703;
pub const I2C_SLAVE_FORCE: u32 = 0x0706;
pub const I2C_TENBIT: u32 = 0x0704;
pub const I2C_RDWR: u32 = 0x0707;
pub const I2C_PEC: u32 = 0x0708;
pub const I2C_SMBUS: u32 = 0x0720;

// ============================================================================
// I2C MESSAGE
// ============================================================================

#[derive(Clone, Debug)]
pub struct I2cMsg {
    /// Slave address
    pub addr: u16,
    /// Message flags
    pub flags: u16,
    /// Data buffer
    pub buf: Vec<u8>,
    /// Buffer length
    pub len: u16,
}

/// I2C message flags
pub const I2C_M_RD: u16 = 0x0001;
pub const I2C_M_TEN: u16 = 0x0010;
pub const I2C_M_DMA_SAFE: u16 = 0x0020;
pub const I2C_M_RECV_LEN: u16 = 0x0400;
pub const I2C_M_NO_RD_ACK: u16 = 0x0800;
pub const I2C_M_IGNORE_NAK: u16 = 0x1000;
pub const I2C_M_REV_DIR_ADDR: u16 = 0x2000;
pub const I2C_M_STOP: u16 = 0x8000;

// ============================================================================
// I2C ADAPTER
// ============================================================================

pub struct I2cAdapter {
    /// Adapter number
    pub nr: u32,
    /// Adapter name
    pub name: String,
    /// Bus frequency
    pub frequency: AtomicU32,
    /// Is 10-bit addressing supported
    pub ten_bit: AtomicBool,
    /// Clients on this bus
    pub clients: Mutex<Vec<Arc<I2cClient>>>,
    /// Lock
    pub lock: Mutex<()>,
}

impl I2cAdapter {
    pub fn new(nr: u32, name: &str) -> Self {
        Self {
            nr,
            name: String::from(name),
            frequency: AtomicU32::new(I2C_STANDARD_MODE),
            ten_bit: AtomicBool::new(false),
            clients: Mutex::new(Vec::new()),
            lock: Mutex::new(()),
        }
    }

    /// Transfer messages
    pub fn transfer(&self, msgs: &mut [I2cMsg]) -> Result<u32, I2cError> {
        let _lock = self.lock.lock();
        
        for msg in msgs.iter() {
            self.do_transfer(msg)?;
        }
        
        Ok(msgs.len() as u32)
    }

    /// Do single transfer
    fn do_transfer(&self, msg: &I2cMsg) -> Result<(), I2cError> {
        // Hardware-specific transfer
        // For now, placeholder
        Ok(())
    }

    /// SMBus read byte
    pub fn smbus_read_byte(&self, addr: u16) -> Result<u8, I2cError> {
        let mut msg = I2cMsg {
            addr,
            flags: I2C_M_RD,
            buf: vec![0],
            len: 1,
        };
        self.transfer(&mut [msg])?;
        Ok(msg.buf[0])
    }

    /// SMBus write byte
    pub fn smbus_write_byte(&self, addr: u16, value: u8) -> Result<(), I2cError> {
        let msg = I2cMsg {
            addr,
            flags: 0,
            buf: vec![value],
            len: 1,
        };
        self.transfer(&mut [msg.clone()])?;
        Ok(())
    }

    /// SMBus read byte data
    pub fn smbus_read_byte_data(&self, addr: u16, reg: u8) -> Result<u8, I2cError> {
        let mut msgs = [
            I2cMsg { addr, flags: 0, buf: vec![reg], len: 1 },
            I2cMsg { addr, flags: I2C_M_RD, buf: vec![0], len: 1 },
        ];
        self.transfer(&mut msgs)?;
        Ok(msgs[1].buf[0])
    }

    /// SMBus write byte data
    pub fn smbus_write_byte_data(&self, addr: u16, reg: u8, value: u8) -> Result<(), I2cError> {
        let msg = I2cMsg {
            addr,
            flags: 0,
            buf: vec![reg, value],
            len: 2,
        };
        self.transfer(&mut [msg])?;
        Ok(())
    }

    /// Add client
    pub fn add_client(&self, client: Arc<I2cClient>) {
        self.clients.lock().push(client);
    }
}

// ============================================================================
// I2C CLIENT
// ============================================================================

pub struct I2cClient {
    /// Client name
    pub name: String,
    /// Slave address
    pub addr: u16,
    /// Adapter
    pub adapter: Arc<I2cAdapter>,
    /// Driver data
    pub driver_data: Mutex<u64>,
}

impl I2cClient {
    pub fn new(name: &str, addr: u16, adapter: Arc<I2cAdapter>) -> Self {
        Self {
            name: String::from(name),
            addr,
            adapter,
            driver_data: Mutex::new(0),
        }
    }
}

// ============================================================================
// SPI CONSTANTS
// ============================================================================

/// SPI modes
pub const SPI_MODE_0: u32 = 0;
pub const SPI_MODE_1: u32 = 1;
pub const SPI_MODE_2: u32 = 2;
pub const SPI_MODE_3: u32 = 3;

/// SPI flags
pub const SPI_CPHA: u32 = 0x01;
pub const SPI_CPOL: u32 = 0x02;
pub const SPI_MODE_0_MASK: u32 = 0;
pub const SPI_MODE_1_MASK: u32 = SPI_CPHA;
pub const SPI_MODE_2_MASK: u32 = SPI_CPOL;
pub const SPI_MODE_3_MASK: u32 = SPI_CPHA | SPI_CPOL;
pub const SPI_CS_HIGH: u32 = 0x04;
pub const SPI_LSB_FIRST: u32 = 0x08;
pub const SPI_3WIRE: u32 = 0x10;
pub const SPI_LOOP: u32 = 0x20;
pub const SPI_NO_CS: u32 = 0x40;
pub const SPI_READY: u32 = 0x80;

// ============================================================================
// SPI MESSAGE
// ============================================================================

#[derive(Clone, Debug)]
pub struct SpiMessage {
    /// Transfer segments
    pub segments: Vec<SpiTransfer>,
}

#[derive(Clone, Debug)]
pub struct SpiTransfer {
    /// TX buffer
    pub tx_buf: Vec<u8>,
    /// RX buffer
    pub rx_buf: Vec<u8>,
    /// Transfer length
    pub len: usize,
    /// Speed (Hz)
    pub speed_hz: u32,
    /// Delay after transfer (us)
    pub delay_usecs: u16,
    /// Bits per word
    pub bits_per_word: u8,
    /// CS change
    pub cs_change: bool,
}

// ============================================================================
// SPI CONTROLLER
// ============================================================================

pub struct SpiController {
    /// Controller number
    pub nr: u32,
    /// Controller name
    pub name: String,
    /// Bus number
    pub bus_num: u32,
    /// Max speed
    pub max_speed_hz: u32,
    /// Bits per word
    pub bits_per_word: AtomicU32,
    /// Mode
    pub mode: AtomicU32,
    /// Devices on this bus
    pub devices: Mutex<Vec<Arc<SpiDevice>>>,
    /// Lock
    pub lock: Mutex<()>,
}

impl SpiController {
    pub fn new(nr: u32, name: &str, bus_num: u32) -> Self {
        Self {
            nr,
            name: String::from(name),
            bus_num,
            max_speed_hz: 50_000_000,
            bits_per_word: AtomicU32::new(8),
            mode: AtomicU32::new(SPI_MODE_0),
            devices: Mutex::new(Vec::new()),
            lock: Mutex::new(()),
        }
    }

    /// Transfer message
    pub fn transfer(&self, msg: &SpiMessage) -> Result<u32, SpiError> {
        let _lock = self.lock.lock();
        
        for segment in &msg.segments {
            self.do_transfer(segment)?;
        }
        
        Ok(msg.segments.len() as u32)
    }

    /// Do single transfer
    fn do_transfer(&self, transfer: &SpiTransfer) -> Result<(), SpiError> {
        // Hardware-specific transfer
        Ok(())
    }

    /// Write
    pub fn write(&self, data: &[u8]) -> Result<(), SpiError> {
        let msg = SpiMessage {
            segments: vec![SpiTransfer {
                tx_buf: data.to_vec(),
                rx_buf: Vec::new(),
                len: data.len(),
                speed_hz: self.max_speed_hz,
                delay_usecs: 0,
                bits_per_word: 8,
                cs_change: false,
            }],
        };
        self.transfer(&msg)?;
        Ok(())
    }

    /// Read
    pub fn read(&self, len: usize) -> Result<Vec<u8>, SpiError> {
        let msg = SpiMessage {
            segments: vec![SpiTransfer {
                tx_buf: vec![0; len],
                rx_buf: vec![0; len],
                len,
                speed_hz: self.max_speed_hz,
                delay_usecs: 0,
                bits_per_word: 8,
                cs_change: false,
            }],
        };
        self.transfer(&msg)?;
        Ok(msg.segments[0].rx_buf.clone())
    }

    /// Add device
    pub fn add_device(&self, device: Arc<SpiDevice>) {
        self.devices.lock().push(device);
    }
}

// ============================================================================
// SPI DEVICE
// ============================================================================

pub struct SpiDevice {
    /// Device name
    pub name: String,
    /// Chip select
    pub chip_select: u8,
    /// Controller
    pub controller: Arc<SpiController>,
    /// Max speed
    pub max_speed_hz: u32,
    /// Mode
    pub mode: u32,
    /// Driver data
    pub driver_data: Mutex<u64>,
}

impl SpiDevice {
    pub fn new(name: &str, cs: u8, controller: Arc<SpiController>) -> Self {
        Self {
            name: String::from(name),
            chip_select: cs,
            controller: controller.clone(),
            max_speed_hz: controller.max_speed_hz,
            mode: SPI_MODE_0,
            driver_data: Mutex::new(0),
        }
    }
}

// ============================================================================
// MANAGERS
// ============================================================================

pub struct I2cManager {
    adapters: Mutex<BTreeMap<u32, Arc<I2cAdapter>>>,
    next_nr: AtomicU32,
}

impl I2cManager {
    pub const fn new() -> Self {
        Self {
            adapters: Mutex::new(BTreeMap::new()),
            next_nr: AtomicU32::new(0),
        }
    }

    pub fn register(&self, name: &str) -> Arc<I2cAdapter> {
        let nr = self.next_nr.fetch_add(1, Ordering::SeqCst);
        let adapter = Arc::new(I2cAdapter::new(nr, name));
        self.adapters.lock().insert(nr, adapter.clone());
        adapter
    }

    pub fn get(&self, nr: u32) -> Option<Arc<I2cAdapter>> {
        self.adapters.lock().get(&nr).cloned()
    }
}

pub struct SpiManager {
    controllers: Mutex<BTreeMap<u32, Arc<SpiController>>>,
    next_nr: AtomicU32,
}

impl SpiManager {
    pub const fn new() -> Self {
        Self {
            controllers: Mutex::new(BTreeMap::new()),
            next_nr: AtomicU32::new(0),
        }
    }

    pub fn register(&self, name: &str, bus_num: u32) -> Arc<SpiController> {
        let nr = self.next_nr.fetch_add(1, Ordering::SeqCst);
        let controller = Arc::new(SpiController::new(nr, name, bus_num));
        self.controllers.lock().insert(nr, controller.clone());
        controller
    }

    pub fn get(&self, nr: u32) -> Option<Arc<SpiController>> {
        self.controllers.lock().get(&nr).cloned()
    }
}

lazy_static::lazy_static! {
    pub static ref I2C_MANAGER: I2cManager = I2cManager::new();
    pub static ref SPI_MANAGER: SpiManager = SpiManager::new();
}

// ============================================================================
// ERROR TYPES
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum I2cError {
    Nack,
    Timeout,
    ArbitrationLost,
    BusError,
    NoDevice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpiError {
    Timeout,
    BusError,
    NoDevice,
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init() {
    crate::serial_println!("[I2C/SPI] Subsystem initialized");
}
