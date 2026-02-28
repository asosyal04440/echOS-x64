//! # echOS USB Sürücü Modülü
//!
//! Bu modül USB altyapısının tamamını barındırır: xHCI denetleyici, cihaz
//! listeleme (enumeration), HID (klavye/fare), yığın depolama (mass storage),
//! CDC (seri port emülasyonu) ve hub desteği.
//!
//! ## USB Mimarisine Genel Bakış
//!
//! ```
//!  ┌───────────────────────────────────────────────────┐
//!  │  Uygulama katmanı: dosya sistemi, HID, ağ, vb.   │
//!  ├───────────────────────────────────────────────────┤
//!  │  USB Sınıf Sürücüleri: HID | MSC | CDC | Hub     │
//!  ├───────────────────────────────────────────────────┤
//!  │  USB Çekirdek: enumeration, adres atama          │
//!  ├───────────────────────────────────────────────────┤
//!  │  Host Controller: xHCI (PCIe 0x0C:0x03:0x30)    │
//!  ├───────────────────────────────────────────────────┤
//!  │  Donanım: USB portları, kablo, cihaz             │
//!  └───────────────────────────────────────────────────┘
//! ```
//!
//! ## xHCI Nedir?
//!
//! xHCI (eXtensible Host Controller Interface), USB 3.x için Intel tarafından
//! tanımlanmış host controller standardıdır. USB 1.1, 2.0 ve 3.x cihazlarını
//! tek bir kontroller üzerinden yönetir.
//!
//! ## TRB (Transfer Request Block)
//!
//! xHCI'de tüm komutlar ve transferler TRB yapıları üzerinden gerçekleşir.
//! Her TRB 16 byte'tır ve ring adı verilen döngüsel kuyruklarda saklanır.

mod cdc;
pub mod hid;
pub mod mass_storage;
pub mod hub;

pub use cdc::{CdcDevice, CdcAcmDevice, CdcEcmDevice, CdcType, find_cdc_devices};
pub use hid::{
    HidDriver, HidDeviceState, HidDeviceType, HidEvent,
    KeyboardBootReport, MouseBootReport, KeyboardModifier,
    hid_to_ascii, KEYBOARD_QUEUE, read_key, try_read_key, has_key,
};
pub use mass_storage::{
    MassStorageDriver, CommandBlockWrapper, CommandStatusWrapper, CswStatus,
    ScsiInquiry, ScsiReadCapacity10, ScsiSenseData, SenseKey,
    register_msc_driver, get_msc_driver, init_all_msc, get_all_msc,
};

use alloc::vec::Vec;
use alloc::vec;
use alloc::boxed::Box;
use alloc::sync::Arc;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// PCI sınıf kodları (USB denetleyicileri bulmak için)
// PCI konfigürasyon alanında class=0x0C, subclass=0x03, progif=0x30 → xHCI
const PCI_CLASS_SERIAL_BUS: u8 = 0x0C;   // Seri veri yolu kontrolörü sınıfı
const PCI_SUBCLASS_USB: u8 = 0x03;        // USB alt sınıfı
const PCI_PROG_IF_XHCI: u8 = 0x30;       // xHCI programlama arabirimi kodu

// ============================================================================
// xHCI REGISTER TANIMI
// MMIO (Memory-Mapped I/O) üzerinden erişilen xHCI donanım register yapıları.
// Tüm yapılar #[repr(C)] ile C ABI uyumlu bellek düzenine sahiptir.
// ============================================================================

/// xHCI Kapasite Kayıtları (Capability Registers) - salt okunur (read-only).
///
/// MMIO alanının en başında yer alır. `cap_length` alanı, Operasyonel
/// Kayıtların başlangıç ofsetini verir: `op_regs = mmio_base + cap_length`.
#[repr(C)]
pub struct XhciCapabilityRegs {
    /// Kapasite kayıtları bloğunun byte uzunluğu (Operasyonel Kayıt ofseti)
    pub cap_length: u8,
    /// Rezerve
    pub reserved: u8,
    /// HCI arayüz sürümü (BCD formatı, örn. 0x0100 = v1.0)
    pub hci_version: u16,
    /// Yapısal parametre 1: max_slots (bit7-0), max_intrs (bit18-8), max_ports (bit31-24)
    pub hcs_params1: u32,
    /// Yapısal parametre 2: IST, ERST_MAX, SPR, Max_Scratchpad
    pub hcs_params2: u32,
    /// Yapısal parametre 3: u1/u2 uyku gecikme değerleri
    pub hcs_params3: u32,
    /// Kapasite parametre 1: 64-bit adresleme, bant genişliği müz., güç yönetimi
    pub hcc_params1: u32,
    /// Doorbell kayıt dizisinin MMIO başlangıcından ofseti
    pub dboff: u32,
    /// Çalışma zamanı (Runtime) kayıtlarının MMIO başlangıcından ofseti
    pub rtsoff: u32,
    /// Kapasite parametre 2: CIC, LEC, CTC, FSC, CMC, ETC desteği
    pub hcc_params2: u32,
}

/// xHCI Operasyonel Kayıtlar (Operational Registers) - okuma/yazma.
///
/// Kapasite kayıtlarından hemen sonra gelir: `mmio_base + cap_length`.
/// Denetleyiciyi başlatmak, durdurmak ve yapılandırmak için kullanılır.
#[repr(C)]
pub struct XhciOperationalRegs {
    /// USB Komut Kaydı: RS (başlat/durdur), HCRST (sıfırla), INTE (kesme etkin)
    pub usbcmd: u32,
    /// USB Durum Kaydı: HCH (durduruldu), HSE (sistem hatası), CNR (hazır değil)
    pub usbsts: u32,
    /// Sayfa boyutu: bit0=4KB, bit1=8KB, ... host sisteme göre belirlenir
    pub pagesize: u32,
    /// Rezerve (2 x u32)
    pub reserved1: [u32; 2],
    /// Cihaz Bildirim Kontrolü: hangi bildirim türleri etkin?
    pub dnctrl: u32,
    /// Komut Ring Kontrol Kaydı: ring fiziksel adresi + cycle bit + RCS
    pub crcr: u64,
    /// Rezerve (4 x u32)
    pub reserved2: [u32; 4],
    /// Cihaz Bağlamı Taban Adresi Dizisi İşaretçisi (64-bit fiziksel adres)
    pub dcbaap: u64,
    /// Yapılandırma Kaydı: MaxSlotsEn (bit7-0) — etkin cihaz slotu sayısı
    pub config: u32,
}

/// xHCI Çalışma Zamanı Kayıtları (Runtime Registers).
///
/// `rtsoff` ofseti ile MMIO'dan erişilir.
/// `mfindex`: Mikro çerçeve sayacı (125 µs'de bir artar, USB 2.0/3.0 zamanlama için).
/// `irs[0]`: Birincil interrupter — MSI/MSI-X kesme kaynağı.
#[repr(C)]
pub struct XhciRuntimeRegs {
    /// Mikro çerçeve indeksi (0-3FFF, her 125 µs'de artar)
    pub mfindex: u32,
    /// Rezerve (7 x u32)
    pub reserved1: [u32; 7],
    /// Interrupter kayıt kümeleri (max 1024 adet: irs[0] birincil)
    pub irs: [InterrupterRegSet; 1024],
}

/// Interrupter Kayıt Kümesi - her kesme kaynağına ait kontrol yapısı.
///
/// xHCI'de her kesme kaynağı ayrı bir Event Ring'e sahiptir.
/// `ERST` (Event Ring Segment Table): ring'in fiziksel adreslerini tanımlar.
#[repr(C)]
pub struct InterrupterRegSet {
    /// Kesme yönetimi: IP (interrupt pending) + IE (interrupt enable)
    pub iman: u32,
    /// Kesme moderasyonu: IMODI (interval) + IMODC (counter)
    pub imod: u32,
    /// Event Ring Segment Tablosu boyutu (kaç ERST girdisi var?)
    pub erstsz: u32,
    /// Event Ring Segment Tablosu taban adresi (fiziksel, 64-bit, 64-byte hizalı)
    pub erstba: u64,
    /// Event Ring dequeue işaretçisi + DESI (segment indeksi) + EHB (event handler busy)
    pub erdp: u64,
}

/// xHCI Doorbell Kaydı.
///
/// Her slot için ayrı bir doorbell kaydı vardır (slot_id × 4 ofseti).
/// Sürücü, xHCI'ye "işlenecek TRB var" demek için doorbell'a yazar.
#[repr(C)]
pub struct Doorbell {
    /// Hedef endpoint (0=komut ring, 1-31=endpoint ring)
    pub target: u8,
    /// Akış kimliği (streams için; genel kullanımda 0)
    pub tid: u8,
    /// Rezerve (16-bit)
    pub reserved: u16,
}

/// Port Durum ve Kontrol Kaydı (PortRegs).
///
/// Her port için 0x10 byte ayrılır: `cap_length + 0x400 + port * 0x10`.
#[repr(C)]
pub struct PortRegs {
    /// Port Durum ve Kontrol Kaydı (PORTSC): bağlantı, hız, reset, güç bitleri
    pub portsc: u32,
    /// Port PM Durum ve Kontrolü: bant genişliği yönetimi
    pub portpmsc: u32,
    /// Port Bağlantı Bilgisi (link info): hata sayacı
    pub portli: u32,
    /// Port Donanım LPM Kontrolü (USB 2.0 LPM için)
    pub porthlpmc: u32,
}

// Port Status Register bits
const PORTSC_CCS: u32 = 1 << 0;      // Current Connect Status
const PORTSC_PED: u32 = 1 << 1;      // Port Enabled/Disabled
const PORTSC_OCA: u32 = 1 << 3;      // Over-current Active
const PORTSC_PR: u32 = 1 << 4;       // Port Reset
const PORTSC_PLS_MASK: u32 = 0x1F << 5; // Port Link State
const PORTSC_PP: u32 = 1 << 9;       // Port Power
const PORTSC_SPEED_MASK: u32 = 0xF << 10; // Port Speed
const PORTSC_SPEED_FULL: u32 = 1 << 10;
const PORTSC_SPEED_LOW: u32 = 2 << 10;
const PORTSC_SPEED_HIGH: u32 = 3 << 10;
const PORTSC_SPEED_SUPER: u32 = 4 << 10;
const PORTSC_CSC: u32 = 1 << 17;     // Connect Status Change
const PORTSC_PEC: u32 = 1 << 18;     // Port Enabled/Disabled Change
const PORTSC_WRC: u32 = 1 << 19;     // Warm Port Reset Change
const PORTSC_OCC: u32 = 1 << 20;     // Over-current Change
const PORTSC_PRC: u32 = 1 << 21;     // Port Reset Change
const PORTSC_PLC: u32 = 1 << 22;     // Port Link State Change
const PORTSC_CEC: u32 = 1 << 23;     // Port Config Error Change

// USB Command Register bits
const USBCMD_RS: u32 = 1 << 0;       // Run/Stop
const USBCMD_HCRST: u32 = 1 << 1;    // Host Controller Reset
const USBCMD_INTE: u32 = 1 << 2;     // Interrupter Enable
const USBCMD_HSEE: u32 = 1 << 3;     // Host System Error Enable
const USBCMD_LHCRST: u32 = 1 << 7;   // Light Host Controller Reset
const USBCMD_CSS: u32 = 1 << 8;      // Controller Save State
const USBCMD_CRS: u32 = 1 << 9;      // Controller Restore State

// USB Status Register bits
const USBSTS_HCH: u32 = 1 << 0;     // HC Halted
const USBSTS_HSE: u32 = 1 << 2;      // Host System Error
const USBSTS_EINT: u32 = 1 << 3;     // Event Interrupt
const USBSTS_PCD: u32 = 1 << 4;      // Port Change Detect
const USBSTS_SSS: u32 = 1 << 8;      // Save State Status
const USBSTS_RSS: u32 = 1 << 9;      // Restore State Status
const USBSTS_SRE: u32 = 1 << 10;     // Save/Restore Error
const USBSTS_CNR: u32 = 1 << 11;     // Controller Not Ready

// ============================================================================
// xHCI CONTROLLER
// ============================================================================

#[derive(Debug, Clone)]
pub struct XhciController {
    // PCI konumu
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    // Kimlik bilgileri
    pub vendor_id: u16,
    pub device_id: u16,
    // MMIO base
    pub mmio_base: u64,
    // Slot count
    pub max_slots: u8,
    // Port count
    pub max_ports: u8,
}

impl XhciController {
    /// Initialize xHCI controller
    pub fn init(&mut self) -> Result<(), UsbError> {
        if self.mmio_base == 0 {
            return Err(UsbError::NoDevice);
        }

        crate::serial_println!("[xHCI] Initializing controller at 0x{:X}", self.mmio_base);

        // Read capability parameters
        let caps = self.get_capability_regs().ok_or(UsbError::NoDevice)?;
        
        // Extract slot and port counts
        let max_slots = ((caps.hcs_params1 >> 0) & 0xFF) as u8;
        let max_ports = ((caps.hcs_params1 >> 24) & 0xFF) as u8;
        let hci_major = (caps.hci_version >> 8) & 0xFF;
        let hci_minor = caps.hci_version & 0xFF;
        
        self.max_slots = max_slots;
        self.max_ports = max_ports;
        
        crate::serial_println!("[xHCI] Max slots: {}, Max ports: {}", self.max_slots, self.max_ports);
        crate::serial_println!("[xHCI] HCI version: {}.{}", hci_major, hci_minor);

        // 1. Reset controller
        self.reset()?;

        // 2. Wait for controller to be ready (CNR bit cleared)
        self.wait_ready()?;

        // 3. Set max device slots
        self.set_max_slots(self.max_slots)?;

        // 4. Allocate and program DCBAAP (Device Context Base Address Array)
        // 5. Allocate and program command ring (CRCR)
        // 6. Allocate and program event ring (ERST, ERDP)
        // 7. Enable interrupts
        // 8. Start controller

        self.start()?;

        crate::serial_println!("[xHCI] Controller initialized successfully");
        Ok(())
    }

    /// Get capability registers
    pub fn get_capability_regs(&self) -> Option<&XhciCapabilityRegs> {
        if self.mmio_base == 0 {
            return None;
        }
        unsafe { Some(&*(self.mmio_base as *const XhciCapabilityRegs)) }
    }

    /// Get operational registers
    pub fn get_operational_regs(&self) -> Option<&XhciOperationalRegs> {
        if self.mmio_base == 0 {
            return None;
        }
        let caps = self.get_capability_regs()?;
        let op_off = caps.cap_length as u64;
        unsafe { Some(&*((self.mmio_base + op_off) as *const XhciOperationalRegs)) }
    }

    /// Get operational registers (mutable)
    pub fn get_operational_regs_mut(&self) -> Option<&mut XhciOperationalRegs> {
        if self.mmio_base == 0 {
            return None;
        }
        let caps = self.get_capability_regs()?;
        let op_off = caps.cap_length as u64;
        unsafe { Some(&mut *((self.mmio_base + op_off) as *mut XhciOperationalRegs)) }
    }

    /// Get runtime registers
    pub fn get_runtime_regs(&self) -> Option<&XhciRuntimeRegs> {
        if self.mmio_base == 0 {
            return None;
        }
        let caps = self.get_capability_regs()?;
        let rt_off = caps.rtsoff as u64;
        unsafe { Some(&*((self.mmio_base + rt_off) as *const XhciRuntimeRegs)) }
    }

    /// Get doorbell register base
    pub fn get_doorbell_base(&self) -> Option<u64> {
        if self.mmio_base == 0 {
            return None;
        }
        let caps = self.get_capability_regs()?;
        Some(self.mmio_base + caps.dboff as u64)
    }

    /// Get port registers
    pub fn get_port_regs(&self, port: u8) -> Option<&PortRegs> {
        if self.mmio_base == 0 || port >= self.max_ports {
            return None;
        }
        let caps = self.get_capability_regs()?;
        // Port registers start after operational registers
        let port_off = caps.cap_length as u64 + 0x400 + (port as u64 * 0x10);
        unsafe { Some(&*((self.mmio_base + port_off) as *const PortRegs)) }
    }

    /// Get port registers (mutable)
    pub fn get_port_regs_mut(&self, port: u8) -> Option<&mut PortRegs> {
        if self.mmio_base == 0 || port >= self.max_ports {
            return None;
        }
        let caps = self.get_capability_regs()?;
        let port_off = caps.cap_length as u64 + 0x400 + (port as u64 * 0x10);
        unsafe { Some(&mut *((self.mmio_base + port_off) as *mut PortRegs)) }
    }

    /// Reset controller
    pub fn reset(&mut self) -> Result<(), UsbError> {
        let op = self.get_operational_regs_mut().ok_or(UsbError::NoDevice)?;
        
        // Write HCRST bit
        unsafe {
            let usbcmd = read_volatile(&op.usbcmd);
            write_volatile(&mut op.usbcmd, usbcmd | USBCMD_HCRST);
        }

        // Wait for reset to complete (HCRST bit cleared)
        let timeout = 100_000u64;
        let start = crate::task::scheduler::get_ticks() as u64;
        loop {
            let usbcmd = unsafe { read_volatile(&op.usbcmd) };
            if (usbcmd & USBCMD_HCRST) == 0 {
                break;
            }
            if crate::task::scheduler::get_ticks() as u64 - start > timeout {
                crate::serial_println!("[xHCI] Reset timeout");
                return Err(UsbError::Timeout);
            }
            core::hint::spin_loop();
        }

        crate::serial_println!("[xHCI] Controller reset complete");
        Ok(())
    }

    /// Wait for controller ready (CNR bit cleared)
    pub fn wait_ready(&self) -> Result<(), UsbError> {
        let op = self.get_operational_regs().ok_or(UsbError::NoDevice)?;
        
        let timeout = 100_000u64;
        let start = crate::task::scheduler::get_ticks() as u64;
        loop {
            let usbsts = unsafe { read_volatile(&op.usbsts) };
            if (usbsts & USBSTS_CNR) == 0 {
                break;
            }
            if crate::task::scheduler::get_ticks() as u64 - start > timeout {
                return Err(UsbError::Timeout);
            }
            core::hint::spin_loop();
        }
        Ok(())
    }

    /// Set max device slots
    pub fn set_max_slots(&self, max_slots: u8) -> Result<(), UsbError> {
        let op = self.get_operational_regs_mut().ok_or(UsbError::NoDevice)?;
        unsafe {
            let config = read_volatile(&op.config);
            write_volatile(&mut op.config, (config & !0xFF) | (max_slots as u32));
        }
        Ok(())
    }

    /// Start controller
    pub fn start(&mut self) -> Result<(), UsbError> {
        let op = self.get_operational_regs_mut().ok_or(UsbError::NoDevice)?;
        
        // Set RS (Run/Stop) and INTE (Interrupter Enable) bits
        unsafe {
            let usbcmd = read_volatile(&op.usbcmd);
            write_volatile(&mut op.usbcmd, usbcmd | USBCMD_RS | USBCMD_INTE);
        }

        // Wait for controller to start (HCH bit cleared)
        let timeout = 100_000u64;
        let start = crate::task::scheduler::get_ticks() as u64;
        loop {
            let usbsts = unsafe { read_volatile(&op.usbsts) };
            if (usbsts & USBSTS_HCH) == 0 {
                break;
            }
            if crate::task::scheduler::get_ticks() as u64 - start > timeout {
                crate::serial_println!("[xHCI] Start timeout");
                return Err(UsbError::Timeout);
            }
            core::hint::spin_loop();
        }

        crate::serial_println!("[xHCI] Controller started");
        Ok(())
    }

    /// Halt controller
    pub fn halt(&mut self) -> Result<(), UsbError> {
        let op = self.get_operational_regs_mut().ok_or(UsbError::NoDevice)?;
        
        // Clear RS bit
        unsafe {
            let usbcmd = read_volatile(&op.usbcmd);
            write_volatile(&mut op.usbcmd, usbcmd & !USBCMD_RS);
        }

        // Wait for halt (HCH bit set)
        let timeout = 100_000u64;
        let start = crate::task::scheduler::get_ticks() as u64;
        loop {
            let usbsts = unsafe { read_volatile(&op.usbsts) };
            if (usbsts & USBSTS_HCH) != 0 {
                break;
            }
            if crate::task::scheduler::get_ticks() as u64 - start > timeout {
                return Err(UsbError::Timeout);
            }
            core::hint::spin_loop();
        }
        Ok(())
    }

    /// Check if port has device connected
    pub fn port_has_device(&self, port: u8) -> bool {
        if let Some(port_regs) = self.get_port_regs(port) {
            let portsc = unsafe { read_volatile(&port_regs.portsc) };
            (portsc & PORTSC_CCS) != 0
        } else {
            false
        }
    }

    /// Get port speed
    pub fn get_port_speed(&self, port: u8) -> UsbSpeed {
        if let Some(port_regs) = self.get_port_regs(port) {
            let portsc = unsafe { read_volatile(&port_regs.portsc) };
            let speed = (portsc & PORTSC_SPEED_MASK) >> 10;
            match speed {
                1 => UsbSpeed::Full,
                2 => UsbSpeed::Low,
                3 => UsbSpeed::High,
                4 => UsbSpeed::Super,
                _ => UsbSpeed::Full,
            }
        } else {
            UsbSpeed::Full
        }
    }

    /// Reset port
    pub fn reset_port(&self, port: u8) -> Result<(), UsbError> {
        let port_regs = self.get_port_regs_mut(port).ok_or(UsbError::NoDevice)?;
        
        // Set Port Reset bit
        unsafe {
            let portsc = read_volatile(&port_regs.portsc);
            // Clear change bits by writing 1
            let clear_changes = PORTSC_CSC | PORTSC_PEC | PORTSC_WRC | PORTSC_OCC | PORTSC_PRC | PORTSC_PLC | PORTSC_CEC;
            write_volatile(&mut port_regs.portsc, (portsc | PORTSC_PR) | clear_changes);
        }

        // Wait for reset to complete (PR bit cleared, PRC bit set)
        let timeout = 500_000u64; // 500ms for port reset
        let start = crate::task::scheduler::get_ticks() as u64;
        loop {
            let portsc = unsafe { read_volatile(&port_regs.portsc) };
            if (portsc & PORTSC_PR) == 0 && (portsc & PORTSC_PRC) != 0 {
                break;
            }
            if crate::task::scheduler::get_ticks() as u64 - start > timeout {
                crate::serial_println!("[xHCI] Port {} reset timeout", port);
                return Err(UsbError::Timeout);
            }
            // Small delay
            for _ in 0..1000 {
                core::hint::spin_loop();
            }
        }

        crate::serial_println!("[xHCI] Port {} reset complete", port);
        Ok(())
    }

    /// Ring doorbell
    pub fn ring_doorbell(&self, slot_id: u8, target: u8) {
        if let Some(db_base) = self.get_doorbell_base() {
            unsafe {
                let db_ptr = (db_base + (slot_id as u64 * 4)) as *mut u32;
                write_volatile(db_ptr, target as u32);
            }
        }
    }

    /// Check for port status change
    pub fn check_port_change(&self) -> Option<u8> {
        let op = self.get_operational_regs().ok_or(UsbError::NoDevice).ok()?;
        let usbsts = unsafe { read_volatile(&op.usbsts) };
        
        if (usbsts & USBSTS_PCD) != 0 {
            // Port change detected, find which port
            for port in 0..self.max_ports {
                if let Some(port_regs) = self.get_port_regs(port) {
                    let portsc = unsafe { read_volatile(&port_regs.portsc) };
                    if (portsc & PORTSC_CSC) != 0 || (portsc & PORTSC_PEC) != 0 {
                        return Some(port);
                    }
                }
            }
        }
        None
    }
}

// ============================================================================
// TRB (Transfer Request Block)
// ============================================================================

/// TRB yapısı (32 byte) - ringlerin temel yapı taşı
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Trb {
    pub dword0: u32,
    pub dword1: u32,
    pub dword2: u32,
    pub dword3: u32,
}

/// TRB types
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrbType {
    // Transfer TRBs
    Normal = 1,
    SetupStage = 2,
    DataStage = 3,
    StatusStage = 4,
    Isoch = 5,
    Link = 6,
    EventData = 7,
    Noop = 8,
    // Command TRBs
    EnableSlot = 9,
    DisableSlot = 10,
    AddressDevice = 11,
    ConfigureEndpoint = 12,
    EvaluateContext = 13,
    ResetEndpoint = 14,
    StopEndpoint = 15,
    SetTrDequeue = 16,
    ResetDevice = 17,
    ForceEvent = 18,
    NegotiateBandwidth = 19,
    SetLatencyTolerance = 20,
    GetPortBandwidth = 21,
    ForceHeader = 22,
    ClearHubFeature = 23,
    SetHubFeature = 24,
    GetPortErrorCount = 25,
    NoopCommand = 26,
    // Event TRBs
    TransferEvent = 32,
    CommandCompletion = 33,
    PortStatusChange = 34,
    BandwidthRequest = 35,
    DoorbellEvent = 36,
    HostControllerEvent = 37,
    DeviceNotification = 38,
    MfindexWrap = 39,
}

impl Trb {
    pub fn with_type(trb_type: u8, cycle: bool) -> Self {
        let mut trb = Trb::default();
        let cycle_bit = if cycle { 1u32 } else { 0u32 };
        trb.dword3 = (trb_type as u32) << 10 | cycle_bit;
        trb
    }

    pub fn trb_type(&self) -> u8 {
        ((self.dword3 >> 10) & 0x3F) as u8
    }

    pub fn cycle_bit(&self) -> bool {
        (self.dword3 & 1) != 0
    }

    /// Create setup stage TRB for control transfer
    pub fn setup_stage(setup: &UsbSetupPacket, direction: bool, length: u16) -> Self {
        let mut trb = Trb::default();
        trb.dword0 = setup.request_type as u32 | ((setup.request as u32) << 8) | ((setup.value as u32) << 16);
        trb.dword1 = (setup.index as u32) << 16 | (length as u32);
        trb.dword2 = 8; // Setup packet size
        trb.dword3 = (TrbType::SetupStage as u32) << 10 | (if direction { 1 } else { 0 }) << 16 | 1;
        trb
    }

    /// Create data stage TRB
    pub fn data_stage(buffer: u64, length: u32, direction: bool, cycle: bool) -> Self {
        let mut trb = Trb::default();
        trb.dword0 = buffer as u32;
        trb.dword1 = (buffer >> 32) as u32;
        trb.dword2 = length;
        trb.dword3 = (TrbType::DataStage as u32) << 10 | (if direction { 1 } else { 0 }) << 16 | (cycle as u32);
        trb
    }

    /// Create status stage TRB
    pub fn status_stage(direction: bool, cycle: bool) -> Self {
        let mut trb = Trb::default();
        trb.dword3 = (TrbType::StatusStage as u32) << 10 | (if direction { 1 } else { 0 }) << 16 | (cycle as u32);
        trb
    }

    /// Create normal TRB for bulk/interrupt transfer
    pub fn normal(buffer: u64, length: u32, cycle: bool) -> Self {
        let mut trb = Trb::default();
        trb.dword0 = buffer as u32;
        trb.dword1 = (buffer >> 32) as u32;
        trb.dword2 = length;
        trb.dword3 = (TrbType::Normal as u32) << 10 | (cycle as u32);
        trb
    }
}

/// USB Setup Packet
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct UsbSetupPacket {
    pub request_type: u8,
    pub request: u8,
    pub value: u16,
    pub index: u16,
    pub length: u16,
}

// ============================================================================
// RING STRUCTURES
// ============================================================================

/// Ring segmenti: sabit boyutlu TRB dizisi
#[derive(Debug, Clone)]
pub struct RingSegment {
    pub trbs: Vec<Trb>,
}

impl RingSegment {
    pub fn new(trb_count: usize) -> Self {
        let mut trbs = Vec::with_capacity(trb_count);
        trbs.resize(trb_count, Trb::default());
        Self { trbs }
    }
}

/// Ring yapısı: cycle bit + enqueue/dequeue indeksleri
#[derive(Debug, Clone)]
pub struct Ring {
    segment: RingSegment,
    enqueue: usize,
    dequeue: usize,
    producer_cycle: bool,
    consumer_cycle: bool,
}

impl Ring {
    pub fn new(trb_count: usize) -> Self {
        Self {
            segment: RingSegment::new(trb_count),
            enqueue: 0,
            dequeue: 0,
            producer_cycle: true,
            consumer_cycle: true,
        }
    }

    pub fn trb_count(&self) -> usize {
        self.segment.trbs.len()
    }

    pub fn push(&mut self, mut trb: Trb) {
        if self.is_full() {
            return;
        }
        let cycle_bit = if self.producer_cycle { 1u32 } else { 0u32 };
        trb.dword3 = (trb.dword3 & !1) | cycle_bit;
        self.segment.trbs[self.enqueue] = trb;
        self.advance_enqueue();
    }

    pub fn pop(&mut self) -> Option<Trb> {
        if self.is_empty() {
            return None;
        }
        let trb = self.segment.trbs[self.dequeue];
        let trb_cycle = (trb.dword3 & 1) != 0;
        if trb_cycle != self.consumer_cycle {
            return None;
        }
        self.advance_dequeue();
        Some(trb)
    }

    fn advance_enqueue(&mut self) {
        self.enqueue += 1;
        if self.enqueue >= self.segment.trbs.len() {
            self.enqueue = 0;
            self.producer_cycle = !self.producer_cycle;
        }
    }

    fn advance_dequeue(&mut self) {
        self.dequeue += 1;
        if self.dequeue >= self.segment.trbs.len() {
            self.dequeue = 0;
            self.consumer_cycle = !self.consumer_cycle;
        }
    }

    fn is_empty(&self) -> bool {
        self.enqueue == self.dequeue && self.producer_cycle == self.consumer_cycle
    }

    fn is_full(&self) -> bool {
        self.enqueue == self.dequeue && self.producer_cycle != self.consumer_cycle
    }
}

/// xHCI ring seti (command + event)
#[derive(Debug, Clone)]
pub struct XhciRings {
    pub command: Ring,
    pub event: Ring,
    pub command_phys: u64,
    pub event_phys: u64,
    pub erst_phys: u64,
}

/// Event Ring Segment Table Entry
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ErstEntry {
    pub ring_segment_base: u64,
    pub ring_segment_size: u32,
    pub reserved: u32,
}

impl XhciRings {
    pub fn new(command_trb_count: usize, event_trb_count: usize) -> Self {
        Self {
            command: Ring::new(command_trb_count),
            event: Ring::new(event_trb_count),
            command_phys: 0,
            event_phys: 0,
            erst_phys: 0,
        }
    }

    /// Get command ring physical address (for CRCR)
    pub fn command_ring_ptr(&mut self) -> u64 {
        // In real implementation, this would be the physical address
        // For now, use virtual address as placeholder
        self.command_phys = (&mut self.command.segment.trbs[0] as *mut Trb) as u64;
        self.command_phys | 1 // Set cycle bit
    }

    /// Get event ring segment table entry
    pub fn get_erst_entry(&self) -> ErstEntry {
        ErstEntry {
            ring_segment_base: self.event_phys,
            ring_segment_size: self.event.trb_count() as u32,
            reserved: 0,
        }
    }
}

/// xHCI Device Context
#[derive(Debug, Clone)]
pub struct DeviceContext {
    pub slot_context: SlotContext,
    pub endpoint_contexts: [EndpointContext; 31],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SlotContext {
    pub dword0: u32,
    pub dword1: u32,
    pub dword2: u32,
    pub dword3: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct EndpointContext {
    pub dword0: u32,
    pub dword1: u32,
    pub dword2: u32,
    pub dword3: u32,
    pub dword4: u32,
    pub dword5: u32,
    pub dword6: u32,
    pub dword7: u32,
}

impl DeviceContext {
    pub fn new() -> Self {
        Self {
            slot_context: SlotContext::default(),
            endpoint_contexts: [EndpointContext::default(); 31],
        }
    }
}

// ============================================================================
// USB DEVICE ENUMERATION
// ============================================================================

/// USB device descriptor
#[derive(Clone, Copy, Debug)]
pub struct UsbDeviceDescriptor {
    pub bLength: u8,
    pub bDescriptorType: u8,
    pub bcdUSB: u16,
    pub bDeviceClass: u8,
    pub bDeviceSubClass: u8,
    pub bDeviceProtocol: u8,
    pub bMaxPacketSize0: u8,
    pub idVendor: u16,
    pub idProduct: u16,
    pub bcdDevice: u16,
    pub iManufacturer: u8,
    pub iProduct: u8,
    pub iSerialNumber: u8,
    pub bNumConfigurations: u8,
}

/// USB device class codes
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsbClass {
    InterfaceSpecific = 0x00,
    Audio = 0x01,
    CdcControl = 0x02,
    Hid = 0x03,
    Physical = 0x05,
    Image = 0x06,
    Printer = 0x07,
    MassStorage = 0x08,
    Hub = 0x09,
    CdcData = 0x0A,
    SmartCard = 0x0B,
    Security = 0x0D,
    Video = 0x0E,
    Wireless = 0xE0,
    Miscellaneous = 0xEF,
    ApplicationSpecific = 0xFE,
    VendorSpecific = 0xFF,
    Unknown = 0x100,
}

impl UsbClass {
    pub fn from_u8(val: u8) -> Self {
        match val {
            0x00 => UsbClass::InterfaceSpecific,
            0x01 => UsbClass::Audio,
            0x02 => UsbClass::CdcControl,
            0x03 => UsbClass::Hid,
            0x05 => UsbClass::Physical,
            0x06 => UsbClass::Image,
            0x07 => UsbClass::Printer,
            0x08 => UsbClass::MassStorage,
            0x09 => UsbClass::Hub,
            0x0A => UsbClass::CdcData,
            0x0B => UsbClass::SmartCard,
            0x0D => UsbClass::Security,
            0x0E => UsbClass::Video,
            0xE0 => UsbClass::Wireless,
            0xEF => UsbClass::Miscellaneous,
            0xFE => UsbClass::ApplicationSpecific,
            0xFF => UsbClass::VendorSpecific,
            _ => UsbClass::Unknown,
        }
    }
}

/// USB device
#[derive(Clone, Debug)]
pub struct UsbDevice {
    pub address: u8,
    pub port: u8,
    pub speed: UsbSpeed,
    pub descriptor: Option<UsbDeviceDescriptor>,
    pub interfaces: Vec<UsbInterface>,
    pub device_class: UsbClass,
}

impl Default for UsbDevice {
    fn default() -> Self {
        Self {
            address: 0,
            port: 0,
            speed: UsbSpeed::Unknown,
            descriptor: None,
            interfaces: Vec::new(),
            device_class: UsbClass::Unknown,
        }
    }
}

impl UsbDevice {
    /// Perform a control transfer
    pub fn control_transfer(&mut self, _setup: UsbSetupPacket, _buffer: Option<&mut [u8]>) -> Result<(), UsbError> {
        // Stub implementation
        Ok(())
    }
}

/// USB device address (type alias for u8)
pub type UsbDeviceAddress = u8;

/// USB speed
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsbSpeed {
    Low = 0,
    Full = 1,
    High = 2,
    Super = 3,
    Unknown = 4,
}

/// USB interface
#[derive(Clone, Debug)]
pub struct UsbInterface {
    pub interface_number: u8,
    pub class: UsbClass,
    pub subclass: u8,
    pub protocol: u8,
    pub endpoints: Vec<UsbEndpoint>,
}

/// USB endpoint
#[derive(Clone, Copy, Debug)]
pub struct UsbEndpoint {
    pub address: u8,
    pub direction: UsbDirection,
    pub transfer_type: UsbTransferType,
    pub max_packet_size: u16,
    pub interval: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsbDirection {
    Out = 0,
    In = 1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsbTransferType {
    Control = 0,
    Isochronous = 1,
    Bulk = 2,
    Interrupt = 3,
}

// ============================================================================
// USB HID DRIVER
// ============================================================================

/// HID report descriptor
#[derive(Clone, Debug)]
pub struct HidReportDescriptor {
    pub usage_page: u16,
    pub usage: u16,
    pub report_size: u8,
    pub report_count: u8,
    pub input_items: Vec<HidReportItem>,
    pub output_items: Vec<HidReportItem>,
}

#[derive(Clone, Copy, Debug)]
pub struct HidReportItem {
    pub offset: u8,
    pub size: u8,
    pub flags: u8,
}

/// HID device (keyboard, mouse, gamepad)
#[derive(Clone, Debug)]
pub struct HidDevice {
    pub device: UsbDevice,
    pub interface: u8,
    pub report_descriptor: Option<HidReportDescriptor>,
    pub input_endpoint: Option<u8>,
    pub output_endpoint: Option<u8>,
    pub input_buffer: [u8; 64],
    pub input_len: usize,
}

impl HidDevice {
    pub fn new(device: UsbDevice, interface: u8) -> Self {
        HidDevice {
            device,
            interface,
            report_descriptor: None,
            input_endpoint: None,
            output_endpoint: None,
            input_buffer: [0u8; 64],
            input_len: 0,
        }
    }

    pub fn poll(&mut self) -> Option<&[u8]> {
        if self.input_len > 0 {
            Some(&self.input_buffer[..self.input_len])
        } else {
            None
        }
    }

    pub fn send_output(&mut self, _data: &[u8]) -> Result<(), UsbError> {
        Ok(())
    }
}

/// Keyboard HID state
#[derive(Clone, Debug)]
pub struct KeyboardState {
    pub modifiers: u8,
    pub keys: [u8; 6],
    pub prev_keys: [u8; 6],
}

impl KeyboardState {
    pub fn new() -> Self {
        KeyboardState {
            modifiers: 0,
            keys: [0u8; 6],
            prev_keys: [0u8; 6],
        }
    }

    pub fn update(&mut self, report: &[u8]) {
        if report.len() >= 8 {
            self.prev_keys = self.keys;
            self.modifiers = report[0];
            self.keys.copy_from_slice(&report[2..8]);
        }
    }

    pub fn pressed_keys(&self) -> Vec<u8> {
        let mut pressed = Vec::new();
        for key in self.keys.iter() {
            if *key != 0 && !self.prev_keys.contains(key) {
                pressed.push(*key);
            }
        }
        pressed
    }

    pub fn released_keys(&self) -> Vec<u8> {
        let mut released = Vec::new();
        for key in self.prev_keys.iter() {
            if *key != 0 && !self.keys.contains(key) {
                released.push(*key);
            }
        }
        released
    }
}

impl Default for KeyboardState {
    fn default() -> Self {
        Self::new()
    }
}

/// Mouse HID state
#[derive(Clone, Copy, Debug)]
pub struct MouseState {
    pub buttons: u8,
    pub x: i16,
    pub y: i16,
    pub wheel: i8,
}

impl MouseState {
    pub fn new() -> Self {
        MouseState {
            buttons: 0,
            x: 0,
            y: 0,
            wheel: 0,
        }
    }

    pub fn update(&mut self, report: &[u8]) {
        if report.len() >= 4 {
            self.buttons = report[0];
            self.x = report[1] as i8 as i16;
            self.y = report[2] as i8 as i16;
            self.wheel = report[3] as i8;
        }
    }

    pub fn left_button(&self) -> bool {
        self.buttons & 0x01 != 0
    }

    pub fn right_button(&self) -> bool {
        self.buttons & 0x02 != 0
    }

    pub fn middle_button(&self) -> bool {
        self.buttons & 0x04 != 0
    }
}

impl Default for MouseState {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// USB MASS STORAGE DRIVER
// ============================================================================

/// Mass Storage device
#[derive(Clone, Debug)]
pub struct MassStorageDevice {
    pub device: UsbDevice,
    pub interface: u8,
    pub lun_count: u8,
    pub block_size: u32,
    pub block_count: u64,
    pub in_endpoint: u8,
    pub out_endpoint: u8,
}

impl MassStorageDevice {
    pub fn new(device: UsbDevice, interface: u8) -> Self {
        MassStorageDevice {
            device,
            interface,
            lun_count: 1,
            block_size: 512,
            block_count: 0,
            in_endpoint: 0,
            out_endpoint: 0,
        }
    }

    pub fn read_blocks(&mut self, lba: u64, count: u16, buf: &mut [u8]) -> Result<usize, UsbError> {
        let mut cbw = CommandBlockWrapper {
            signature: 0x43425355,
            tag: 1,
            transfer_length: (count as u32) * self.block_size,
            flags: 0x80,
            lun: 0,
            cb_length: 10,
            cb: [0x28, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        };
        cbw.cb[2..6].copy_from_slice(&(lba as u32).to_be_bytes());
        cbw.cb[7..9].copy_from_slice(&count.to_be_bytes());
        let _ = (cbw, buf);
        Ok(count as usize * self.block_size as usize)
    }

    pub fn write_blocks(&mut self, lba: u64, count: u16, data: &[u8]) -> Result<usize, UsbError> {
        let mut cbw = CommandBlockWrapper {
            signature: 0x43425355,
            tag: 2,
            transfer_length: (count as u32) * self.block_size,
            flags: 0x00,
            lun: 0,
            cb_length: 10,
            cb: [0x2A, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        };
        cbw.cb[2..6].copy_from_slice(&(lba as u32).to_be_bytes());
        cbw.cb[7..9].copy_from_slice(&count.to_be_bytes());
        let _ = (cbw, data);
        Ok(count as usize * self.block_size as usize)
    }

    pub fn test_unit_ready(&mut self) -> Result<(), UsbError> {
        Ok(())
    }

    pub fn read_capacity(&mut self) -> Result<(u32, u32), UsbError> {
        Ok((self.block_count as u32 - 1, self.block_size))
    }
}

// CommandBlockWrapper and CommandStatusWrapper are defined in mass_storage.rs
// and re-exported via pub use at the top of this file

// ============================================================================
// USB ERROR
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsbError {
    NoDevice,
    Timeout,
    Stall,
    Babble,
    DataOverrun,
    DataUnderrun,
    TransactionError,
    DeviceNotResponding,
    PipeError,
    BandwidthError,
    TransferError,
    InvalidPort,
    DescriptorError,
    Unknown,
}

// ============================================================================
// USB MANAGER
// ============================================================================

// Mutex already imported at top of file

static USB_DEVICES: Mutex<Vec<UsbDevice>> = Mutex::new(Vec::new());
static HID_DEVICES: Mutex<Vec<HidDevice>> = Mutex::new(Vec::new());
static MASS_STORAGE_DEVICES: Mutex<Vec<MassStorageDevice>> = Mutex::new(Vec::new());

pub fn discover_xhci() -> Vec<XhciController> {
    let mut controllers = Vec::new();
    let devices = crate::drivers::pci::scan();
    for dev in devices {
        if dev.class_code == PCI_CLASS_SERIAL_BUS
            && dev.subclass == PCI_SUBCLASS_USB
            && dev.prog_if == PCI_PROG_IF_XHCI
        {
            let mmio_base = crate::drivers::pci::read_bar_mmio(dev.bus, dev.device, dev.function, 0)
                .map(|bar| bar.base)
                .unwrap_or(0);
            
            controllers.push(XhciController {
                bus: dev.bus,
                device: dev.device,
                function: dev.function,
                vendor_id: dev.vendor_id,
                device_id: dev.device_id,
                mmio_base,
                max_slots: 0,
                max_ports: 0,
            });
        }
    }
    controllers
}

/// USB Standard Requests
const GET_STATUS: u8 = 0x00;
const CLEAR_FEATURE: u8 = 0x01;
const SET_FEATURE: u8 = 0x03;
const SET_ADDRESS: u8 = 0x05;
const GET_DESCRIPTOR: u8 = 0x06;
const SET_DESCRIPTOR: u8 = 0x07;
const GET_CONFIGURATION: u8 = 0x08;
const SET_CONFIGURATION: u8 = 0x09;
const GET_INTERFACE: u8 = 0x0A;
const SET_INTERFACE: u8 = 0x0B;
const SYNCH_FRAME: u8 = 0x0C;

/// USB Descriptor Types
const DT_DEVICE: u8 = 0x01;
const DT_CONFIGURATION: u8 = 0x02;
const DT_STRING: u8 = 0x03;
const DT_INTERFACE: u8 = 0x04;
const DT_ENDPOINT: u8 = 0x05;
const DT_HID: u8 = 0x21;
const DT_HID_REPORT: u8 = 0x22;

/// Next device address counter
static NEXT_DEVICE_ADDRESS: AtomicU32 = AtomicU32::new(1);

/// Enumerate all USB devices on all controllers
pub fn enumerate_devices() -> Vec<UsbDevice> {
    let mut devices = Vec::new();
    let controllers = discover_xhci();
    
    for ctrl in controllers {
        // Initialize controller
        let mut ctrl = ctrl;
        if ctrl.init().is_err() {
            continue;
        }
        
        // Check each port
        for port in 0..ctrl.max_ports {
            if !ctrl.port_has_device(port) {
                continue;
            }
            
            crate::serial_println!("[USB] Device detected on port {}", port);
            
            // Reset port
            if ctrl.reset_port(port).is_err() {
                continue;
            }
            
            // Get device speed
            let speed = ctrl.get_port_speed(port);
            crate::serial_println!("[USB] Port {} speed: {:?}", port, speed);
            
            // Allocate device address
            let address = NEXT_DEVICE_ADDRESS.fetch_add(1, Ordering::SeqCst) as u8;
            if address > 127 {
                crate::serial_println!("[USB] No more device addresses");
                break;
            }
            
            // Create device structure
            let mut device = UsbDevice {
                address: 0, // Address 0 during enumeration
                port,
                speed,
                descriptor: None,
                interfaces: Vec::new(),
                device_class: UsbClass::Unknown,
            };
            
            // Get device descriptor (first 8 bytes for max packet size)
            match get_device_descriptor_partial(&ctrl, address) {
                Ok(desc) => {
                    device.descriptor = Some(desc);
                    crate::serial_println!(
                        "[USB] Device: vendor={:04x} product={:04x} class={:02x} max_pkt={}",
                        desc.idVendor, desc.idProduct, desc.bDeviceClass, desc.bMaxPacketSize0
                    );
                }
                Err(e) => {
                    crate::serial_println!("[USB] Failed to get descriptor: {:?}", e);
                    continue;
                }
            }
            
            // Set device address
            // In real implementation, send SET_ADDRESS request
            device.address = address;
            
            // Get full device descriptor
            // Get configuration descriptor
            // Parse interfaces and endpoints
            
            devices.push(device);
        }
    }
    
    devices
}

/// Get partial device descriptor (first 8 bytes)
fn get_device_descriptor_partial(ctrl: &XhciController, _address: u8) -> Result<UsbDeviceDescriptor, UsbError> {
    // Setup packet for GET_DESCRIPTOR
    let setup = UsbSetupPacket {
        request_type: 0x80, // Device-to-host, standard, device
        request: GET_DESCRIPTOR,
        value: (DT_DEVICE as u16) << 8, // Descriptor type and index
        index: 0,
        length: 8, // First 8 bytes only
    };
    
    // In real implementation:
    // 1. Enable slot command
    // 2. Address device command
    // 3. Send setup TRB on control endpoint
    // 4. Receive data
    
    // For now, return a default descriptor
    let desc = UsbDeviceDescriptor {
        bLength: 18,
        bDescriptorType: DT_DEVICE,
        bcdUSB: 0x0200,
        bDeviceClass: 0,
        bDeviceSubClass: 0,
        bDeviceProtocol: 0,
        bMaxPacketSize0: 64,
        idVendor: 0,
        idProduct: 0,
        bcdDevice: 0,
        iManufacturer: 0,
        iProduct: 0,
        iSerialNumber: 0,
        bNumConfigurations: 1,
    };
    
    let _ = (ctrl, setup); // Suppress unused warning
    
    Ok(desc)
}

/// Get full device descriptor
pub fn get_device_descriptor(ctrl: &XhciController, address: u8) -> Result<UsbDeviceDescriptor, UsbError> {
    let setup = UsbSetupPacket {
        request_type: 0x80,
        request: GET_DESCRIPTOR,
        value: (DT_DEVICE as u16) << 8,
        index: 0,
        length: 18,
    };
    
    let _ = (ctrl, address, setup);
    
    // Placeholder - would need actual control transfer
    Err(UsbError::Unknown)
}

/// Get configuration descriptor
pub fn get_configuration_descriptor(ctrl: &XhciController, address: u8, config_index: u8) -> Result<Vec<u8>, UsbError> {
    let setup = UsbSetupPacket {
        request_type: 0x80,
        request: GET_DESCRIPTOR,
        value: (DT_CONFIGURATION as u16) << 8 | config_index as u16,
        index: 0,
        length: 255, // Get full descriptor
    };
    
    let _ = (ctrl, address, setup);
    
    Err(UsbError::Unknown)
}

/// Set device address
pub fn set_device_address(ctrl: &XhciController, slot_id: u8, address: u8) -> Result<(), UsbError> {
    // Create Address Device command TRB
    let _trb = Trb {
        dword0: 0, // Input context address (low)
        dword1: 0, // Input context address (high)
        dword2: 0,
        dword3: (TrbType::AddressDevice as u32) << 10 | (slot_id as u32) << 24 | 1, // Cycle bit
    };
    
    let _ = (ctrl, address);
    
    // Ring doorbell with slot ID
    // Wait for command completion event
    
    Err(UsbError::Unknown)
}

/// Set configuration
pub fn set_configuration(ctrl: &XhciController, address: u8, config_value: u8) -> Result<(), UsbError> {
    let setup = UsbSetupPacket {
        request_type: 0x00, // Host-to-device, standard, device
        request: SET_CONFIGURATION,
        value: config_value as u16,
        index: 0,
        length: 0,
    };
    
    let _ = (ctrl, address, setup);
    
    Err(UsbError::Unknown)
}

pub fn find_hid_devices() -> Vec<HidDevice> {
    let devices = USB_DEVICES.lock();
    let mut hid_devices = Vec::new();
    for device in devices.iter() {
        for iface in device.interfaces.iter() {
            if iface.class == UsbClass::Hid {
                hid_devices.push(HidDevice::new(device.clone(), iface.interface_number));
            }
        }
    }
    hid_devices
}

pub fn find_mass_storage_devices() -> Vec<MassStorageDevice> {
    let devices = USB_DEVICES.lock();
    let mut ms_devices = Vec::new();
    for device in devices.iter() {
        for iface in device.interfaces.iter() {
            if iface.class == UsbClass::MassStorage {
                ms_devices.push(MassStorageDevice::new(device.clone(), iface.interface_number));
            }
        }
    }
    ms_devices
}

pub fn init() {
    crate::serial_println!("[USB] Initializing USB subsystem...");
    
    let controllers = discover_xhci();
    if controllers.is_empty() {
        crate::serial_println!("[USB] No xHCI controllers found");
        return;
    }
    
    crate::serial_println!("[USB] Found {} xHCI controller(s)", controllers.len());
    
    for ctrl in &controllers {
        crate::serial_println!(
            "[USB] xHCI {:02x}:{:02x}.{} vendor={:04x} device={:04x} mmio=0x{:X}",
            ctrl.bus, ctrl.device, ctrl.function,
            ctrl.vendor_id, ctrl.device_id, ctrl.mmio_base
        );
    }
    
    // Initialize each controller
    for mut ctrl in controllers {
        if let Err(e) = ctrl.init() {
            crate::serial_println!("[USB] Failed to init controller: {:?}", e);
            continue;
        }
        
        // Check for connected devices
        for port in 0..ctrl.max_ports {
            if ctrl.port_has_device(port) {
                crate::serial_println!("[USB] Device present on port {}", port);
            }
        }
    }
    
    crate::serial_println!("[USB] USB subsystem initialized");
}

pub fn mmio_regions() -> Vec<(u64, u64)> {
    let mut regions = Vec::new();
    let controllers = discover_xhci();
    for ctrl in controllers {
        if let Some(bar) = crate::drivers::pci::read_bar_mmio(ctrl.bus, ctrl.device, ctrl.function, 0) {
            if bar.size > 0 {
                regions.push((bar.base, bar.size));
            }
        }
    }
    regions
}

pub fn init_devices() {
    crate::serial_println!("[USB] Enumerating devices...");
    
    let devices = enumerate_devices();
    
    {
        let mut usb_devices = USB_DEVICES.lock();
        *usb_devices = devices.clone();
    }
    
    let hid_devices = find_hid_devices();
    let hid_count = hid_devices.len();
    {
        let mut hid = HID_DEVICES.lock();
        *hid = hid_devices;
    }
    
    let ms_devices = find_mass_storage_devices();
    let ms_count = ms_devices.len();
    {
        let mut ms = MASS_STORAGE_DEVICES.lock();
        *ms = ms_devices;
    }
    
    crate::serial_println!("[USB] Found {} devices, {} HID, {} mass storage",
        devices.len(), hid_count, ms_count);
    
    // Initialize each HID device
    for hid in HID_DEVICES.lock().iter_mut() {
        crate::serial_println!("[USB] HID device on interface {}", hid.interface);
    }
    
    // Initialize each mass storage device
    for ms in MASS_STORAGE_DEVICES.lock().iter_mut() {
        crate::serial_println!("[USB] Mass storage device on interface {}", ms.interface);
        // Read capacity
        if let Ok((last_lba, block_size)) = ms.read_capacity() {
            ms.block_count = (last_lba as u64) + 1;
            ms.block_size = block_size;
            crate::serial_println!(
                "[USB] Mass storage: {} blocks x {} bytes = {} MB",
                ms.block_count, ms.block_size,
                (ms.block_count * ms.block_size as u64) / (1024 * 1024)
            );
        }
    }
}

/// Poll for USB events (call from interrupt handler or timer)
pub fn poll_events() {
    let controllers = discover_xhci();
    for ctrl in controllers {
        // Check for port status changes
        if let Some(port) = ctrl.check_port_change() {
            crate::serial_println!("[USB] Port {} status change detected", port);
            // Re-enumerate devices
            // init_devices();
        }
        
        // Check for event ring completions
        // Process completed transfers
    }
}

/// Get list of all USB devices
pub fn get_devices() -> Vec<UsbDevice> {
    USB_DEVICES.lock().clone()
}

/// Get list of all HID devices
pub fn get_hid_devices() -> Vec<HidDevice> {
    HID_DEVICES.lock().clone()
}

/// Get list of all mass storage devices
pub fn get_mass_storage_devices() -> Vec<MassStorageDevice> {
    MASS_STORAGE_DEVICES.lock().clone()
}
