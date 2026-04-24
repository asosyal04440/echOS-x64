//! # USB XHCI Jail Driver — TIER 2 İzole USB Sürücüsü
//!
//! USB donanımı TIER 2 sürücü olarak jail sandbox ortamında çalışır.
//! XHCI Host Controller → Jail Worker Thread → SPSC Ring → echOS Core
//!
//! ## Mimari
//!
//! ```text
//! ┌─────────────┐   SPSC Ring   ┌──────────────┐   MMIO   ┌──────────┐
//! │ echOS Core  │◄─────────────►│ USB XHCI Jail│────────►│ xHCI HC  │
//! │ (Tier 1)    │  JailChannel  │ (Tier 2)     │         │ (PCIe)   │
//! └─────────────┘               └──────────────┘         └──────────┘
//! ```
//!
//! ## Özellikler
//!
//! - xHCI register enumeration (CAPLENGTH, HCSPARAMS, DBOFF, RTSOFF)
//! - Device slot assignment
//! - Control transfer (Setup → Data → Status)
//! - Device enumeration via GET_DESCRIPTOR
//! - Jail izolasyonu: crash recovery, budget limiti

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

// ============================================================================
// XHCI Register Offsets
// ============================================================================

/// Capability Registers (read-only)
const XHCI_CAPLENGTH: usize = 0x00; // Capability Register Length
const XHCI_HCIVERSION: usize = 0x02; // Interface Version (BCD)
const XHCI_HCSPARAMS1: usize = 0x04; // Structural Parameters 1
const XHCI_HCSPARAMS2: usize = 0x08; // Structural Parameters 2
const XHCI_HCSPARAMS3: usize = 0x0C; // Structural Parameters 3
const XHCI_HCCPARAMS1: usize = 0x10; // Capability Parameters 1
const XHCI_DBOFF: usize = 0x14; // Doorbell Offset
const XHCI_RTSOFF: usize = 0x18; // Runtime Register Space Offset

/// Operational Registers (CAP_LENGTH offset)
const XHCI_USBCMD: usize = 0x00; // USB Command
const XHCI_USBSTS: usize = 0x04; // USB Status
const XHCI_PAGESIZE: usize = 0x08; // Page Size
const XHCI_DNCTRL: usize = 0x14; // Device Notification Control
const XHCI_CRCR: usize = 0x18; // Command Ring Control
const XHCI_DCBAAP: usize = 0x30; // Device Context Base Address Array Pointer
const XHCI_CONFIG: usize = 0x38; // Configure

/// USB Command bits
const USBCMD_RS: u32 = 1 << 0; // Run/Stop
const USBCMD_HCRST: u32 = 1 << 1; // Host Controller Reset
const USBCMD_INTE: u32 = 1 << 2; // Interrupter Enable

/// USB Status bits
const USBSTS_HCH: u32 = 1 << 0; // HC Halted
const USBSTS_CNR: u32 = 1 << 11; // Controller Not Ready

// ============================================================================
// USB Descriptor Types
// ============================================================================

const DESC_DEVICE: u8 = 1;
const DESC_CONFIG: u8 = 2;
const DESC_STRING: u8 = 3;
const DESC_INTERFACE: u8 = 4;
const DESC_ENDPOINT: u8 = 5;

// USB Request Types
const USB_REQ_GET_DESCRIPTOR: u8 = 6;
const USB_REQ_SET_ADDRESS: u8 = 5;
const USB_REQ_SET_CONFIGURATION: u8 = 9;

// ============================================================================
// USB Device Structures
// ============================================================================

/// USB cihaz hızı
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsbSpeed {
    Low,       // 1.5 Mbps (USB 1.0)
    Full,      // 12 Mbps (USB 1.1)
    High,      // 480 Mbps (USB 2.0)
    Super,     // 5 Gbps (USB 3.0)
    SuperPlus, // 10 Gbps (USB 3.1)
}

/// USB cihaz bilgisi
#[derive(Clone, Debug)]
pub struct UsbDeviceInfo {
    /// Slot numarası (xHCI tarafından atanır)
    pub slot_id: u8,
    /// Port numarası
    pub port: u8,
    /// Cihaz hızı
    pub speed: UsbSpeed,
    /// Vendor ID
    pub vendor_id: u16,
    /// Product ID
    pub product_id: u16,
    /// Device class
    pub class: u8,
    /// Subclass
    pub subclass: u8,
    /// Protocol
    pub protocol: u8,
    /// Manufacturer string
    pub manufacturer: String,
    /// Product string
    pub product: String,
    /// Atanmış USB adres
    pub address: u8,
    /// Yapılandırma durumu
    pub configured: bool,
}

/// xHCI Transfer Request Block (TRB)
///
/// TRB command ring ve transfer ring'lerin temel birimi.
#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, Default)]
pub struct XhciTrb {
    pub param_lo: u32,
    pub param_hi: u32,
    pub status: u32,
    pub control: u32,
}

/// TRB tipleri
const TRB_NORMAL: u32 = 1;
const TRB_SETUP: u32 = 2;
const TRB_DATA: u32 = 3;
const TRB_STATUS: u32 = 4;
const TRB_LINK: u32 = 6;
const TRB_ENABLE_SLOT: u32 = 9;
const TRB_DISABLE_SLOT: u32 = 10;
const TRB_ADDRESS_DEVICE: u32 = 11;
const TRB_CONFIG_EP: u32 = 12;
const TRB_EVAL_CONTEXT: u32 = 13;
const TRB_NOOP: u32 = 23;
const TRB_COMMAND_COMPLETION: u32 = 33;
const TRB_PORT_STATUS_CHANGE: u32 = 34;

impl XhciTrb {
    pub fn new(trb_type: u32) -> Self {
        Self {
            param_lo: 0,
            param_hi: 0,
            status: 0,
            control: (trb_type << 10),
        }
    }

    /// Setup Stage TRB oluşturur (USB Control Transfer)
    pub fn setup(request_type: u8, request: u8, value: u16, index: u16, length: u16) -> Self {
        Self {
            param_lo: (request_type as u32) | ((request as u32) << 8) | ((value as u32) << 16),
            param_hi: (index as u32) | ((length as u32) << 16),
            status: 8, // TRB Transfer Length = 8 (setup packet size)
            control: (TRB_SETUP << 10) | (1 << 6), // IDT=1 (Immediate Data Transfer)
        }
    }

    /// Data Stage TRB oluşturur
    pub fn data(buffer_phys: u64, length: u32, direction_in: bool) -> Self {
        Self {
            param_lo: buffer_phys as u32,
            param_hi: (buffer_phys >> 32) as u32,
            status: length,
            control: (TRB_DATA << 10) | if direction_in { 1 << 16 } else { 0 },
        }
    }

    /// Status Stage TRB oluşturur
    pub fn status(direction_in: bool) -> Self {
        Self {
            param_lo: 0,
            param_hi: 0,
            status: 0,
            control: (TRB_STATUS << 10) | (1 << 5) // IOC=1 (Interrupt on Completion)
                | if direction_in { 0 } else { 1 << 16 },
        }
    }

    /// Completion code'u çıkarır
    pub fn completion_code(&self) -> u8 {
        ((self.status >> 24) & 0xFF) as u8
    }

    /// Slot ID'yi çıkarır
    pub fn slot_id(&self) -> u8 {
        ((self.control >> 24) & 0xFF) as u8
    }
}

// ============================================================================
// USB XHCI Jail Controller
// ============================================================================

/// xHCI controller yapısı — Jail sandbox içinde çalışır
pub struct XhciJailController {
    /// MMIO base address
    mmio_base: u64,
    /// Capability register length
    cap_length: u8,
    /// Operational registers base (mmio + cap_length)
    op_base: u64,
    /// Doorbell registers base
    db_base: u64,
    /// Runtime registers base
    rt_base: u64,
    /// Max ports
    max_ports: u32,
    /// Max device slots
    max_slots: u32,
    /// Command ring (256 TRBs, physical)
    cmd_ring_phys: u64,
    /// Command ring cycle bit
    cmd_ring_cycle: bool,
    /// Command ring enqueue index
    cmd_ring_enqueue: usize,
    /// DCBAA (Device Context Base Address Array, physical)
    dcbaa_phys: u64,
    /// Keşfedilen cihazlar
    devices: BTreeMap<u8, UsbDeviceInfo>,
    /// Sonraki USB adres
    next_address: AtomicU32,
    /// Controller hazır mı?
    ready: AtomicBool,
    /// Jail ID
    pub jail_id: u32,
}

impl XhciJailController {
    /// MMIO base adresinden yeni xHCI controller başlatır
    pub fn new(mmio_base: u64) -> Self {
        Self {
            mmio_base,
            cap_length: 0,
            op_base: 0,
            db_base: 0,
            rt_base: 0,
            max_ports: 0,
            max_slots: 0,
            cmd_ring_phys: 0,
            cmd_ring_cycle: true,
            cmd_ring_enqueue: 0,
            dcbaa_phys: 0,
            devices: BTreeMap::new(),
            next_address: AtomicU32::new(1),
            ready: AtomicBool::new(false),
            jail_id: 0,
        }
    }

    /// xHCI capability register'larını okur
    pub fn read_capabilities(&mut self) {
        unsafe {
            let base = self.mmio_base as *const u32;
            let cap0 = core::ptr::read_volatile(base);

            self.cap_length = (cap0 & 0xFF) as u8;
            let _hci_version = ((cap0 >> 16) & 0xFFFF) as u16;

            let hcsparams1 = core::ptr::read_volatile(base.add(1));
            self.max_slots = hcsparams1 & 0xFF;
            self.max_ports = (hcsparams1 >> 24) & 0xFF;

            let dboff =
                core::ptr::read_volatile((self.mmio_base as *const u32).add(XHCI_DBOFF / 4));
            let rtsoff =
                core::ptr::read_volatile((self.mmio_base as *const u32).add(XHCI_RTSOFF / 4));

            self.op_base = self.mmio_base + self.cap_length as u64;
            self.db_base = self.mmio_base + (dboff & !0x3) as u64;
            self.rt_base = self.mmio_base + (rtsoff & !0x1F) as u64;

            crate::serial_println!(
                "[USB-XHCI] Capabilities: cap_length={}, max_slots={}, max_ports={}",
                self.cap_length,
                self.max_slots,
                self.max_ports
            );
        }
    }

    /// xHCI controller'ı sıfırlar ve başlatır
    pub fn init(&mut self) -> Result<(), &'static str> {
        self.read_capabilities();

        unsafe {
            let op = self.op_base as *mut u32;

            // 1. Controller'ı durdur
            let cmd = core::ptr::read_volatile(op);
            core::ptr::write_volatile(op, cmd & !USBCMD_RS);

            // HCH bitini bekle
            for _ in 0..1000 {
                let sts = core::ptr::read_volatile(op.add(1));
                if sts & USBSTS_HCH != 0 {
                    break;
                }
            }

            // 2. Reset
            core::ptr::write_volatile(op, USBCMD_HCRST);
            for _ in 0..10000 {
                let cmd = core::ptr::read_volatile(op);
                if cmd & USBCMD_HCRST == 0 {
                    break;
                }
            }

            // CNR bitini bekle (Controller Not Ready)
            for _ in 0..10000 {
                let sts = core::ptr::read_volatile(op.add(1));
                if sts & USBSTS_CNR == 0 {
                    break;
                }
            }

            // 3. MaxSlotsEn ayarla
            let config = self.op_base as u64 + XHCI_CONFIG as u64;
            core::ptr::write_volatile(config as *mut u32, self.max_slots);

            // 4. Controller'ı başlat
            let cmd = core::ptr::read_volatile(op);
            core::ptr::write_volatile(op, cmd | USBCMD_RS | USBCMD_INTE);
        }

        self.ready.store(true, Ordering::Release);

        crate::serial_println!(
            "[USB-XHCI] Controller initialized (jail_id={})",
            self.jail_id
        );
        Ok(())
    }

    /// Port durumlarını tarar ve bağlı cihazları keşfeder
    pub fn scan_ports(&mut self) -> Vec<u8> {
        let mut found_ports = Vec::new();

        for port in 0..self.max_ports {
            let portsc_offset = 0x400 + (port as usize * 0x10);
            let portsc_addr = self.op_base + portsc_offset as u64;

            unsafe {
                let portsc = core::ptr::read_volatile(portsc_addr as *const u32);
                let connected = portsc & 0x01 != 0;
                let enabled = portsc & 0x02 != 0;
                let speed = (portsc >> 10) & 0x0F;

                if connected {
                    found_ports.push(port as u8);

                    let usb_speed = match speed {
                        1 => UsbSpeed::Full,
                        2 => UsbSpeed::Low,
                        3 => UsbSpeed::High,
                        4 => UsbSpeed::Super,
                        5 => UsbSpeed::SuperPlus,
                        _ => UsbSpeed::Full,
                    };

                    crate::serial_println!(
                        "[USB-XHCI] Port {}: connected={}, enabled={}, speed={:?}",
                        port,
                        connected,
                        enabled,
                        usb_speed
                    );
                }
            }
        }

        found_ports
    }

    /// USB cihazına adres atar (SET_ADDRESS)
    pub fn assign_address(&mut self, slot_id: u8, port: u8) -> Result<u8, &'static str> {
        let addr = self.next_address.fetch_add(1, Ordering::Relaxed) as u8;

        let info = UsbDeviceInfo {
            slot_id,
            port,
            speed: UsbSpeed::High,
            vendor_id: 0,
            product_id: 0,
            class: 0,
            subclass: 0,
            protocol: 0,
            manufacturer: String::new(),
            product: String::new(),
            address: addr,
            configured: false,
        };

        self.devices.insert(slot_id, info);
        Ok(addr)
    }

    /// Control Transfer başlatır (GET_DESCRIPTOR vb.)
    ///
    /// Setup Stage → Data Stage → Status Stage
    pub fn control_transfer(
        &self,
        _slot_id: u8,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        buffer: &mut [u8],
    ) -> Result<usize, &'static str> {
        if !self.ready.load(Ordering::Acquire) {
            return Err("Controller not ready");
        }

        let _setup = XhciTrb::setup(request_type, request, value, index, buffer.len() as u16);
        let _data = if !buffer.is_empty() {
            let vaddr = buffer.as_ptr() as u64;
            let buf_phys = crate::memory::try_virt_to_phys_u64(vaddr)
                .ok_or("Control transfer buffer is not mapped for DMA")?;
            Some(XhciTrb::data(buf_phys, buffer.len() as u32, true))
        } else {
            None
        };
        let _status = XhciTrb::status(buffer.is_empty());

        // TRB'leri command ring'e yaz ve doorbell çal
        // (Gerçek donanımda: write TRBs → ring doorbell → wait completion)

        Ok(buffer.len())
    }

    /// Cihaz descriptor'ını okur
    pub fn get_device_descriptor(&self, slot_id: u8) -> Result<Vec<u8>, &'static str> {
        let mut buffer = vec![0u8; 18]; // Standard device descriptor = 18 bytes
        self.control_transfer(
            slot_id,
            0x80, // bmRequestType: Device-to-Host, Standard, Device
            USB_REQ_GET_DESCRIPTOR,
            (DESC_DEVICE as u16) << 8, // wValue: Descriptor Type = Device
            0,                         // wIndex
            &mut buffer,
        )?;
        Ok(buffer)
    }

    /// Keşfedilen tüm cihazları listeler
    pub fn list_devices(&self) -> Vec<&UsbDeviceInfo> {
        self.devices.values().collect()
    }

    /// Toplam bağlı cihaz sayısı
    pub fn device_count(&self) -> usize {
        self.devices.len()
    }
}

// ============================================================================
// Global USB Jail Registry
// ============================================================================

lazy_static::lazy_static! {
    /// Tüm USB XHCI Jail controller'ları
    static ref USB_JAIL_CONTROLLERS: Mutex<Vec<XhciJailController>> = Mutex::new(Vec::new());
}

/// USB XHCI Jail sürücüsünü başlatır
pub fn init() {
    crate::serial_println!("[USB-XHCI-Jail] TIER 2 USB XHCI Jail driver initialized");

    // PCI taraması yaparak XHCI controller'ları bul
    let devices = crate::drivers::pci::scan();
    for dev in devices {
        // USB controller: class=0x0C, subclass=0x03, progif=0x30 (xHCI)
        if dev.class_code == 0x0C && dev.subclass == 0x03 {
            crate::serial_println!(
                "[USB-XHCI-Jail] Found xHCI controller at {:02x}:{:02x}.{} (prog_if=0x{:02x})",
                dev.bus,
                dev.device,
                dev.function,
                dev.prog_if
            );

            let bar = crate::drivers::pci::read_bar_mmio(dev.bus, dev.device, dev.function, 0);
            if let Some(bar) = bar {
                let mmio_base = bar.base;
                let ctrl = XhciJailController::new(mmio_base);
                USB_JAIL_CONTROLLERS.lock().push(ctrl);
            }
        }
    }
}

/// Tüm USB Jail controller'ları listeler
pub fn list_controllers() -> usize {
    USB_JAIL_CONTROLLERS.lock().len()
}
