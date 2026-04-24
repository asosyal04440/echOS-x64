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
pub mod hub;
pub mod mass_storage;

pub use cdc::{find_cdc_devices, CdcAcmDevice, CdcDevice, CdcEcmDevice, CdcType};
pub use hid::{
    has_key, hid_to_ascii, read_key, try_read_key, HidDeviceState, HidDeviceType, HidDriver,
    HidEvent, KeyboardBootReport, KeyboardModifier, MouseBootReport, KEYBOARD_QUEUE,
};
pub use mass_storage::{
    get_all_msc, get_msc_driver, init_all_msc, register_msc_driver, CommandBlockWrapper,
    CommandStatusWrapper, CswStatus, MassStorageDriver, ScsiInquiry, ScsiReadCapacity10,
    ScsiSenseData, SenseKey,
};

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// PCI sınıf kodları (USB denetleyicileri bulmak için)
// PCI konfigürasyon alanında class=0x0C, subclass=0x03, progif=0x30 → xHCI
const PCI_CLASS_SERIAL_BUS: u8 = 0x0C; // Seri veri yolu kontrolörü sınıfı
const PCI_SUBCLASS_USB: u8 = 0x03; // USB alt sınıfı
const PCI_PROG_IF_XHCI: u8 = 0x30; // xHCI programlama arabirimi kodu

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
const PORTSC_CCS: u32 = 1 << 0; // Current Connect Status
const PORTSC_PED: u32 = 1 << 1; // Port Enabled/Disabled
const PORTSC_OCA: u32 = 1 << 3; // Over-current Active
const PORTSC_PR: u32 = 1 << 4; // Port Reset
const PORTSC_PLS_MASK: u32 = 0x1F << 5; // Port Link State
const PORTSC_PP: u32 = 1 << 9; // Port Power
const PORTSC_SPEED_MASK: u32 = 0xF << 10; // Port Speed
const PORTSC_SPEED_FULL: u32 = 1 << 10;
const PORTSC_SPEED_LOW: u32 = 2 << 10;
const PORTSC_SPEED_HIGH: u32 = 3 << 10;
const PORTSC_SPEED_SUPER: u32 = 4 << 10;
const PORTSC_CSC: u32 = 1 << 17; // Connect Status Change
const PORTSC_PEC: u32 = 1 << 18; // Port Enabled/Disabled Change
const PORTSC_WRC: u32 = 1 << 19; // Warm Port Reset Change
const PORTSC_OCC: u32 = 1 << 20; // Over-current Change
const PORTSC_PRC: u32 = 1 << 21; // Port Reset Change
const PORTSC_PLC: u32 = 1 << 22; // Port Link State Change
const PORTSC_CEC: u32 = 1 << 23; // Port Config Error Change

// USB Command Register bits
const USBCMD_RS: u32 = 1 << 0; // Run/Stop
const USBCMD_HCRST: u32 = 1 << 1; // Host Controller Reset
const USBCMD_INTE: u32 = 1 << 2; // Interrupter Enable
const USBCMD_HSEE: u32 = 1 << 3; // Host System Error Enable
const USBCMD_LHCRST: u32 = 1 << 7; // Light Host Controller Reset
const USBCMD_CSS: u32 = 1 << 8; // Controller Save State
const USBCMD_CRS: u32 = 1 << 9; // Controller Restore State

// USB Status Register bits
const USBSTS_HCH: u32 = 1 << 0; // HC Halted
const USBSTS_HSE: u32 = 1 << 2; // Host System Error
const USBSTS_EINT: u32 = 1 << 3; // Event Interrupt
const USBSTS_PCD: u32 = 1 << 4; // Port Change Detect
const USBSTS_SSS: u32 = 1 << 8; // Save State Status
const USBSTS_RSS: u32 = 1 << 9; // Restore State Status
const USBSTS_SRE: u32 = 1 << 10; // Save/Restore Error
const USBSTS_CNR: u32 = 1 << 11; // Controller Not Ready
const XHCI_IMAN_IP: u32 = 1 << 0; // Interrupt Pending
const XHCI_IMAN_IE: u32 = 1 << 1; // Interrupt Enable
const XHCI_ERDP_EHB: u64 = 1 << 3; // Event Handler Busy acknowledge

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

        crate::serial_println!(
            "[xHCI] Max slots: {}, Max ports: {}",
            self.max_slots,
            self.max_ports
        );
        crate::serial_println!("[xHCI] HCI version: {}.{}", hci_major, hci_minor);

        // 1. Reset controller
        self.reset()?;

        // 2. Wait for controller to be ready (CNR bit cleared)
        self.wait_ready()?;

        // 3. Set max device slots
        self.set_max_slots(self.max_slots)?;

        // 4. Allocate and program DCBAAP, command ring, event ring, and interrupter
        self.setup_runtime_state()?;

        // 5. Start controller

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

    /// Get runtime registers (mutable)
    pub fn get_runtime_regs_mut(&self) -> Option<&mut XhciRuntimeRegs> {
        if self.mmio_base == 0 {
            return None;
        }
        let caps = self.get_capability_regs()?;
        let rt_off = caps.rtsoff as u64;
        unsafe { Some(&mut *((self.mmio_base + rt_off) as *mut XhciRuntimeRegs)) }
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
            let clear_changes = PORTSC_CSC
                | PORTSC_PEC
                | PORTSC_WRC
                | PORTSC_OCC
                | PORTSC_PRC
                | PORTSC_PLC
                | PORTSC_CEC;
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

    fn setup_runtime_state(&self) -> Result<(), UsbError> {
        ensure_controller_runtime(self)?;
        with_controller_runtime_mut(self, |runtime| {
            let op = self.get_operational_regs_mut().ok_or(UsbError::NoDevice)?;
            let crcr = runtime.rings.command_ring_ptr()?;
            unsafe {
                write_volatile(&mut op.dcbaap, runtime.dcbaa_phys);
                write_volatile(&mut op.crcr, crcr);
            }

            let rt = self.get_runtime_regs_mut().ok_or(UsbError::NoDevice)?;
            unsafe {
                write_volatile(&mut rt.irs[0].erstsz, runtime.erst.len() as u32);
                write_volatile(&mut rt.irs[0].erstba, runtime.rings.erst_phys);
                write_volatile(
                    &mut rt.irs[0].erdp,
                    runtime.event_dequeue_phys() | XHCI_ERDP_EHB,
                );
                write_volatile(&mut rt.irs[0].iman, XHCI_IMAN_IE | XHCI_IMAN_IP);
            }

            crate::serial_println!(
                "[xHCI] Runtime state programmed: dcbaap={:#x} crcr={:#x} erstba={:#x} erdp={:#x}",
                runtime.dcbaa_phys,
                runtime.rings.command_phys,
                runtime.rings.erst_phys,
                runtime.event_dequeue_phys()
            );
            Ok(())
        })
    }
}

// ============================================================================
// TRB (Transfer Request Block)
// ============================================================================

/// TRB yapısı (32 byte) - ringlerin temel yapı taşı
#[repr(C, align(16))]
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

    pub fn direction_in(&self) -> bool {
        (self.dword3 & (1 << 16)) != 0
    }

    pub fn interrupt_on_completion(&self) -> bool {
        (self.dword3 & (1 << 5)) != 0
    }

    /// Create setup stage TRB for control transfer
    pub fn setup_stage(setup: &UsbSetupPacket, direction: bool, length: u16) -> Self {
        let mut trb = Trb::default();
        trb.dword0 = setup.request_type as u32
            | ((setup.request as u32) << 8)
            | ((setup.value as u32) << 16);
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
        trb.dword3 = (TrbType::DataStage as u32) << 10
            | (if direction { 1 } else { 0 }) << 16
            | (cycle as u32);
        trb
    }

    /// Create status stage TRB
    pub fn status_stage(direction: bool, cycle: bool) -> Self {
        let mut trb = Trb::default();
        trb.dword3 = (TrbType::StatusStage as u32) << 10
            | (if direction { 1 } else { 0 }) << 16
            | (cycle as u32);
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
#[repr(C, align(64))]
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
    pub fn command_ring_ptr(&mut self) -> Result<u64, UsbError> {
        self.command_phys = usb_dma_phys(
            self.command.segment.trbs.as_ptr(),
            "xHCI command ring segment",
        )?;
        Ok(self.command_phys | 1)
    }

    /// Get event ring segment table entry
    pub fn get_erst_entry(&self) -> ErstEntry {
        ErstEntry {
            ring_segment_base: self.event_phys,
            ring_segment_size: self.event.trb_count() as u32,
            reserved: 0,
        }
    }

    pub fn resolve_event_resources(&mut self) -> Result<(), UsbError> {
        self.event_phys = usb_dma_phys(self.event.segment.trbs.as_ptr(), "xHCI event ring segment")?;
        Ok(())
    }
}

// ============================================================================
// INPUT CONTEXT (for Address Device command)
// ============================================================================

/// Input Context for device slot initialization
/// Size: 8192 bytes (2 pages) for 64-byte alignment
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug)]
pub struct InputContext {
    /// Input Control Context (32 bytes)
    pub control: InputControlContext,
    /// Slot Context (32 bytes)
    pub slot: SlotContext,
    /// Endpoint Contexts (31 x 32 bytes)
    pub endpoints: [EndpointContext; 31],
    /// Padding to align to page boundary
    pub reserved: [[u32; 8]; 254],
}

impl Default for InputContext {
    fn default() -> Self {
        Self {
            control: InputControlContext::default(),
            slot: SlotContext::default(),
            endpoints: [EndpointContext::default(); 31],
            reserved: [[0u32; 8]; 254],
        }
    }
}

/// Input Control Context - controls which contexts are modified
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct InputControlContext {
    /// Drop context flags (bit N = drop context N)
    pub drop_flags: u32,
    /// Add context flags (bit N = add context N)
    pub add_flags: u32,
    /// Reserved
    pub reserved: [u32; 6],
}

/// Slot Context - device slot information
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SlotContext {
    /// DWORD 0: Route string, speed, MTT, Hub, Context entries
    pub dword0: u32,
    /// DWORD 1: Max exit latency, root hub port number
    pub dword1: u32,
    /// DWORD 2: Interrupter target, USB device address
    pub dword2: u32,
    /// DWORD 3: Slot state
    pub dword3: u32,
    /// Reserved
    pub reserved: [u32; 4],
}

impl SlotContext {
    /// Create slot context for a new device
    pub fn new_device(speed: UsbSpeed, port: u8, slot_id: u8) -> Self {
        let speed_val = match speed {
            UsbSpeed::Low => 2,
            UsbSpeed::Full => 1,
            UsbSpeed::High => 3,
            UsbSpeed::Super => 4,
            UsbSpeed::SuperPlus => 5,
            UsbSpeed::Unknown => 1,
        };

        Self {
            dword0: (speed_val << 20) | (1 << 27), // Speed + Context entries = 1
            dword1: port as u32,                   // Root hub port number
            dword2: 0,                             // USB device address will be set by HC
            dword3: 0,                             // Slot state
            reserved: [0; 4],
        }
    }
}

/// Endpoint Context - endpoint configuration
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct EndpointContext {
    /// DWORD 0: EP state, mult, max_pstreams, LSA, interval
    pub dword0: u32,
    /// DWORD 1: CErr, EP type, HID, max packet size
    pub dword1: u32,
    /// DWORD 2: Dequeue cycle state, TR dequeue pointer (low)
    pub dword2: u32,
    /// DWORD 3: TR dequeue pointer (high), max ESIT payload
    pub dword3: u32,
    /// DWORD 4-7: Reserved
    pub reserved: [u32; 4],
}

impl EndpointContext {
    /// Create endpoint context for control endpoint 0
    pub fn control_endpoint(max_packet_size: u16, tr_dequeue: u64) -> Self {
        Self {
            dword0: 0,                                       // EP state = disabled
            dword1: (4 << 3) | (max_packet_size as u32),     // EP type = control, max packet
            dword2: (1 << 0) | ((tr_dequeue as u32) & !0xF), // DCS = 1, TR dequeue low
            dword3: (tr_dequeue >> 32) as u32,               // TR dequeue high
            reserved: [0; 4],
        }
    }

    /// Create endpoint context for bulk endpoint
    pub fn bulk_endpoint(max_packet_size: u16, tr_dequeue: u64, direction_in: bool) -> Self {
        let ep_type = if direction_in { 6u32 } else { 2u32 }; // Bulk IN/OUT
        Self {
            dword0: 0,
            dword1: (ep_type << 3) | (max_packet_size as u32),
            dword2: (1 << 0) | ((tr_dequeue as u32) & !0xF),
            dword3: (tr_dequeue >> 32) as u32,
            reserved: [0; 4],
        }
    }

    /// Create endpoint context for interrupt endpoint
    pub fn interrupt_endpoint(
        max_packet_size: u16,
        tr_dequeue: u64,
        direction_in: bool,
        interval: u8,
    ) -> Self {
        let ep_type = if direction_in { 7u32 } else { 3u32 }; // Interrupt IN/OUT
        Self {
            dword0: (interval as u32) << 16, // Interval
            dword1: (ep_type << 3) | (max_packet_size as u32),
            dword2: (1 << 0) | ((tr_dequeue as u32) & !0xF),
            dword3: (tr_dequeue >> 32) as u32,
            reserved: [0; 4],
        }
    }
}

// ============================================================================
// TRANSFER RING (per endpoint)
// ============================================================================

/// Transfer Ring for endpoint I/O
#[derive(Debug, Clone)]
pub struct TransferRing {
    pub ring: Ring,
    pub phys: u64,
    pub cycle: bool,
}

impl TransferRing {
    pub fn new(trb_count: usize) -> Self {
        Self {
            ring: Ring::new(trb_count),
            phys: 0,
            cycle: true,
        }
    }

    pub fn resolve_phys(&mut self, label: &str) -> Result<u64, UsbError> {
        self.phys = usb_dma_phys(self.ring.segment.trbs.as_ptr(), label)?;
        Ok(self.phys)
    }

    /// Enqueue a TRB and return the TRB's physical address
    pub fn enqueue(&mut self, trb: Trb) -> u64 {
        let idx = self.ring.enqueue;
        self.ring.push(trb);
        // Return physical address of the TRB
        // In real implementation, this would be the actual physical address
        self.phys + (idx * core::mem::size_of::<Trb>()) as u64
    }
}

/// xHCI Device Context
#[derive(Debug, Clone)]
pub struct DeviceContext {
    pub slot_context: SlotContext,
    pub endpoint_contexts: [EndpointContext; 31],
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
    pub controller_bus: u8,
    pub controller_device: u8,
    pub controller_function: u8,
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
            controller_bus: 0,
            controller_device: 0,
            controller_function: 0,
            descriptor: None,
            interfaces: Vec::new(),
            device_class: UsbClass::Unknown,
        }
    }
}

impl UsbDevice {
    /// Perform a control transfer via xHCI Setup/Data/Status TRBs
    pub fn control_transfer(
        &mut self,
        setup: UsbSetupPacket,
        buffer: Option<&mut [u8]>,
    ) -> Result<(), UsbError> {
        let ctrl = controller_for_device(self)?;
        let data_stage = if let Some(buf) = buffer {
            let data_phys = usb_dma_phys(buf.as_mut_ptr(), "USB control transfer buffer")?;
            Some((data_phys, buf.len() as u32))
        } else {
            None
        };
        let trbs = build_control_transfer_trbs(&setup, data_stage);
        let slot_id = submit_ep0_transfer(&ctrl, self.address, &trbs, "USB control transfer")?;

        crate::serial_println!(
            "[USB] Control transfer: slot={} device_id={} req_type={:#x} req={:#x} value={:#x}",
            slot_id,
            self.address,
            setup.request_type,
            setup.request,
            setup.value
        );
        Ok(())
    }

    pub fn interrupt_transfer_in(
        &mut self,
        endpoint: UsbEndpoint,
    ) -> Result<Option<Vec<u8>>, UsbError> {
        let ctrl = controller_for_device(self)?;
        let mut doorbell = None;
        let mut completed = None;

        {
            let mut slots = DEVICE_SLOTS.lock();
            let slot = slots
                .iter_mut()
                .find(|slot| slot_matches_device_id(slot, &ctrl, self.address))
                .ok_or(UsbError::NoDevice)?;
            let endpoint_state = slot
                .endpoint_rings
                .iter_mut()
                .find(|state| state.endpoint.address == endpoint.address)
                .ok_or(UsbError::NoDevice)?;

            if let Some(done) = endpoint_state.completed_in.pop_front() {
                completed = Some(done.data);
            } else if endpoint_state.pending_in.is_none() {
                if endpoint_state.ring.phys == 0 {
                    endpoint_state.ring.resolve_phys("USB interrupt endpoint ring")?;
                }
                let transfer_len = endpoint.max_packet_size.max(1) as usize;
                let buffer = vec![0u8; transfer_len].into_boxed_slice();
                let data_phys = usb_dma_phys(
                    buffer.as_ptr(),
                    "USB interrupt IN transfer buffer",
                )?;
                let mut trb = Trb::normal(data_phys, transfer_len as u32, true);
                trb.dword3 |= 1 << 5;
                let trb_phys = endpoint_state.ring.enqueue(trb);
                endpoint_state.pending_in = Some(PendingInterruptInTransfer {
                    trb_phys,
                    expected_len: transfer_len,
                    buffer,
                });
                doorbell = Some((slot.slot_id, endpoint_state.endpoint_id));
            }
        }

        if let Some((slot_id, endpoint_id)) = doorbell {
            ctrl.ring_doorbell(slot_id, endpoint_id);
            crate::serial_println!(
                "[USB] Interrupt IN transfer queued: slot={} ep={} addr={:#x}",
                slot_id,
                endpoint_id,
                endpoint.address
            );
        }

        Ok(completed)
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
    SuperPlus = 4,
    Unknown = 5,
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

        let ctrl = XHCI_CONTROLLER.lock();
        let ctrl = ctrl.as_ref().ok_or(UsbError::NoDevice)?;

        // CBW gönder (bulk OUT)
        let cbw_ptr = &cbw as *const CommandBlockWrapper as u64;
        let cbw_size = core::mem::size_of::<CommandBlockWrapper>() as u32;
        let cbw_trb = Trb::normal(cbw_ptr, cbw_size, true);
        // OUT endpoint doorbell
        ctrl.ring_doorbell(self.device.address, (self.out_endpoint as u8) | 0x01);

        // Data IN aşaması — bulk IN TRB ile buffer'a oku
        let xfer_len = (count as u32) * self.block_size;
        let data_trb = Trb::normal(buf.as_ptr() as u64, xfer_len, true);
        ctrl.ring_doorbell(self.device.address, self.in_endpoint as u8 | 0x01);

        // CSW oku (13 byte)
        let mut csw_buf = [0u8; 13];
        let csw_trb = Trb::normal(csw_buf.as_ptr() as u64, 13, true);
        ctrl.ring_doorbell(self.device.address, self.in_endpoint as u8 | 0x01);

        let _ = (cbw_trb, data_trb, csw_trb, &csw_buf);

        // CSW status kontrolü
        let csw_status = csw_buf[12];
        if csw_status != 0 {
            crate::serial_println!("[USB-MSC] READ10 CSW error: status={}", csw_status);
            return Err(UsbError::TransferError);
        }

        crate::serial_println!("[USB-MSC] READ10 lba={} count={} ok", lba, count);
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

        let ctrl = XHCI_CONTROLLER.lock();
        let ctrl = ctrl.as_ref().ok_or(UsbError::NoDevice)?;

        // CBW gönder (bulk OUT)
        let cbw_ptr = &cbw as *const CommandBlockWrapper as u64;
        let cbw_size = core::mem::size_of::<CommandBlockWrapper>() as u32;
        let cbw_trb = Trb::normal(cbw_ptr, cbw_size, true);
        ctrl.ring_doorbell(self.device.address, (self.out_endpoint as u8) | 0x01);

        // Data OUT aşaması — bulk OUT TRB ile data gönder
        let xfer_len = (count as u32) * self.block_size;
        let data_trb = Trb::normal(data.as_ptr() as u64, xfer_len, true);
        ctrl.ring_doorbell(self.device.address, (self.out_endpoint as u8) | 0x01);

        // CSW oku (13 byte)
        let mut csw_buf = [0u8; 13];
        let csw_trb = Trb::normal(csw_buf.as_ptr() as u64, 13, true);
        ctrl.ring_doorbell(self.device.address, self.in_endpoint as u8 | 0x01);

        let _ = (cbw_trb, data_trb, csw_trb, &csw_buf);

        let csw_status = csw_buf[12];
        if csw_status != 0 {
            crate::serial_println!("[USB-MSC] WRITE10 CSW error: status={}", csw_status);
            return Err(UsbError::TransferError);
        }

        crate::serial_println!("[USB-MSC] WRITE10 lba={} count={} ok", lba, count);
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
    AddressTranslationFailed,
    SlotUnavailable,
    Unknown,
}

// ============================================================================
// USB MANAGER
// ============================================================================

// Mutex already imported at top of file

static USB_DEVICES: Mutex<Vec<UsbDevice>> = Mutex::new(Vec::new());
static HID_DEVICES: Mutex<Vec<HidDevice>> = Mutex::new(Vec::new());
static MASS_STORAGE_DEVICES: Mutex<Vec<MassStorageDevice>> = Mutex::new(Vec::new());

/// Global xHCI controller reference (set during discovery)
static XHCI_CONTROLLER: Mutex<Option<XhciController>> = Mutex::new(None);
static XHCI_RUNTIMES: Mutex<Vec<XhciControllerRuntime>> = Mutex::new(Vec::new());

#[derive(Debug)]
struct XhciControllerRuntime {
    bus: u8,
    device: u8,
    function: u8,
    dcbaa: Box<[u64]>,
    dcbaa_phys: u64,
    erst: Box<[ErstEntry]>,
    rings: XhciRings,
}

impl XhciControllerRuntime {
    fn event_dequeue_phys(&self) -> u64 {
        self.rings.event_phys
            + (self.rings.event.dequeue * core::mem::size_of::<Trb>()) as u64
    }
}

pub fn discover_xhci() -> Vec<XhciController> {
    let mut controllers = Vec::new();
    let devices = crate::drivers::pci::scan();
    for dev in devices {
        if dev.class_code == PCI_CLASS_SERIAL_BUS
            && dev.subclass == PCI_SUBCLASS_USB
            && dev.prog_if == PCI_PROG_IF_XHCI
        {
            let mmio_base =
                crate::drivers::pci::read_bar_mmio(dev.bus, dev.device, dev.function, 0)
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

// Global storage for device slots
static DEVICE_SLOTS: Mutex<Vec<DeviceSlot>> = Mutex::new(Vec::new());

/// Device slot information
#[derive(Debug, Clone)]
pub struct DeviceSlot {
    pub slot_id: u8,
    pub usb_address: u8,
    pub port: u8,
    pub speed: UsbSpeed,
    pub controller_bus: u8,
    pub controller_device: u8,
    pub controller_function: u8,
    pub input_context: Box<InputContext>,
    pub control_ring: TransferRing,
    pub endpoint_rings: Vec<EndpointTransferState>,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct PendingInterruptInTransfer {
    pub trb_phys: u64,
    pub expected_len: usize,
    pub buffer: Box<[u8]>,
}

#[derive(Debug, Clone)]
pub struct CompletedInterruptInTransfer {
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct EndpointTransferState {
    pub endpoint: UsbEndpoint,
    pub endpoint_id: u8,
    pub ring: TransferRing,
    pub pending_in: Option<PendingInterruptInTransfer>,
    pub completed_in: VecDeque<CompletedInterruptInTransfer>,
}

impl EndpointTransferState {
    fn new(endpoint: UsbEndpoint, endpoint_id: u8, ring: TransferRing) -> Self {
        Self {
            endpoint,
            endpoint_id,
            ring,
            pending_in: None,
            completed_in: VecDeque::new(),
        }
    }
}

fn usb_dma_phys<T>(ptr: *const T, label: &str) -> Result<u64, UsbError> {
    let vaddr = ptr as u64;
    crate::memory::try_virt_to_phys_u64(vaddr).ok_or_else(|| {
        crate::serial_println!("[USB] DMA translation failed for {} vaddr={:#x}", label, vaddr);
        UsbError::AddressTranslationFailed
    })
}

fn controller_slot_id(
    slots: &[DeviceSlot],
    ctrl: &XhciController,
    port: u8,
) -> Result<u8, UsbError> {
    if let Some(existing) = slots.iter().find(|slot| {
        slot.enabled
            && slot.controller_bus == ctrl.bus
            && slot.controller_device == ctrl.device
            && slot.controller_function == ctrl.function
            && slot.port == port
    }) {
        return Ok(existing.slot_id);
    }

    for slot_id in 1..=ctrl.max_slots {
        let occupied = slots.iter().any(|slot| {
            slot.enabled
                && slot.controller_bus == ctrl.bus
                && slot.controller_device == ctrl.device
                && slot.controller_function == ctrl.function
                && slot.slot_id == slot_id
        });
        if !occupied {
            return Ok(slot_id);
        }
    }

    Err(UsbError::SlotUnavailable)
}

fn slot_matches_controller(slot: &DeviceSlot, ctrl: &XhciController) -> bool {
    slot.controller_bus == ctrl.bus
        && slot.controller_device == ctrl.device
        && slot.controller_function == ctrl.function
}

fn runtime_matches_controller(runtime: &XhciControllerRuntime, ctrl: &XhciController) -> bool {
    runtime.bus == ctrl.bus && runtime.device == ctrl.device && runtime.function == ctrl.function
}

fn slot_matches_device_id(slot: &DeviceSlot, ctrl: &XhciController, device_id: u8) -> bool {
    slot.enabled
        && slot_matches_controller(slot, ctrl)
        && (slot.slot_id == device_id || (slot.usb_address != 0 && slot.usb_address == device_id))
}

fn controller_for_device(device: &UsbDevice) -> Result<XhciController, UsbError> {
    if let Some(ctrl) = XHCI_CONTROLLER
        .lock()
        .as_ref()
        .filter(|ctrl| {
            ctrl.bus == device.controller_bus
                && ctrl.device == device.controller_device
                && ctrl.function == device.controller_function
        })
        .cloned()
    {
        return Ok(ctrl);
    }

    discover_xhci()
        .into_iter()
        .find(|ctrl| {
            ctrl.bus == device.controller_bus
                && ctrl.device == device.controller_device
                && ctrl.function == device.controller_function
        })
        .ok_or(UsbError::NoDevice)
}

fn endpoint_id_from_address(endpoint: &UsbEndpoint) -> u8 {
    let ep_num = endpoint.address & 0x0F;
    (ep_num << 1) | if endpoint.direction == UsbDirection::In { 1 } else { 0 }
}

fn bind_slot_usb_address(ctrl: &XhciController, slot_id: u8, usb_address: u8) -> Result<(), UsbError> {
    let mut slots = DEVICE_SLOTS.lock();
    let slot = slots
        .iter_mut()
        .find(|slot| slot.enabled && slot_matches_controller(slot, ctrl) && slot.slot_id == slot_id)
        .ok_or(UsbError::NoDevice)?;
    slot.usb_address = usb_address;
    Ok(())
}

fn ensure_controller_runtime(ctrl: &XhciController) -> Result<(), UsbError> {
    let mut runtimes = XHCI_RUNTIMES.lock();
    if runtimes
        .iter()
        .any(|runtime| runtime_matches_controller(runtime, ctrl))
    {
        return Ok(());
    }

    let dcbaa_len = (ctrl.max_slots.max(1) as usize) + 1;
    let dcbaa = vec![0u64; dcbaa_len].into_boxed_slice();
    let dcbaa_phys = usb_dma_phys(dcbaa.as_ptr(), "xHCI DCBAA")?;

    let mut rings = XhciRings::new(256, 256);
    rings.command_ring_ptr()?;
    rings.resolve_event_resources()?;

    let mut erst = vec![ErstEntry::default(); 1].into_boxed_slice();
    erst[0] = rings.get_erst_entry();
    let erst_phys = usb_dma_phys(erst.as_ptr(), "xHCI ERST")?;
    rings.erst_phys = erst_phys;

    runtimes.push(XhciControllerRuntime {
        bus: ctrl.bus,
        device: ctrl.device,
        function: ctrl.function,
        dcbaa,
        dcbaa_phys,
        erst,
        rings,
    });
    Ok(())
}

fn with_controller_runtime_mut<R>(
    ctrl: &XhciController,
    f: impl FnOnce(&mut XhciControllerRuntime) -> Result<R, UsbError>,
) -> Result<R, UsbError> {
    let mut runtimes = XHCI_RUNTIMES.lock();
    let runtime = runtimes
        .iter_mut()
        .find(|runtime| runtime_matches_controller(runtime, ctrl))
        .ok_or(UsbError::NoDevice)?;
    f(runtime)
}

fn build_control_transfer_trbs(
    setup: &UsbSetupPacket,
    data_stage: Option<(u64, u32)>,
) -> Vec<Trb> {
    let mut trbs = Vec::with_capacity(if data_stage.is_some() { 3 } else { 2 });
    let data_in = (setup.request_type & 0x80) != 0;
    let trt = match data_stage {
        Some(_) if data_in => 3u32,
        Some(_) => 2u32,
        None => 0u32,
    };
    let setup_data = (setup.request_type as u64)
        | ((setup.request as u64) << 8)
        | ((setup.value as u64) << 16)
        | ((setup.index as u64) << 32)
        | ((setup.length as u64) << 48);
    trbs.push(Trb {
        dword0: (setup_data & 0xFFFF_FFFF) as u32,
        dword1: (setup_data >> 32) as u32,
        dword2: 8,
        dword3: (TrbType::SetupStage as u32) << 10 | 1 | (trt << 16),
    });
    if let Some((data_phys, data_len)) = data_stage {
        trbs.push(Trb::data_stage(data_phys, data_len, data_in, true));
    }
    let mut status_trb = Trb::status_stage(if data_stage.is_some() { !data_in } else { true }, true);
    status_trb.dword3 |= 1 << 5;
    trbs.push(status_trb);
    trbs
}

fn submit_command_trb(ctrl: &XhciController, trb: Trb, label: &str) -> Result<u64, UsbError> {
    ensure_controller_runtime(ctrl)?;
    let trb_phys = with_controller_runtime_mut(ctrl, |runtime| {
        if runtime.rings.command_phys == 0 {
            runtime.rings.command_ring_ptr()?;
        }
        let idx = runtime.rings.command.enqueue;
        runtime.rings.command.push(trb);
        Ok(
            runtime.rings.command_phys + (idx * core::mem::size_of::<Trb>()) as u64,
        )
    })?;
    ctrl.ring_doorbell(0, 0);
    crate::serial_println!(
        "[USB] {} queued on command ring trb_phys={:#x}",
        label,
        trb_phys
    );
    Ok(trb_phys)
}

fn parse_configuration_interfaces(config_desc: &[u8]) -> Vec<UsbInterface> {
    let mut interfaces = Vec::new();
    let mut current_iface = None;
    let mut offset = 0usize;

    while offset + 2 <= config_desc.len() {
        let desc_len = config_desc[offset] as usize;
        let desc_type = config_desc[offset + 1];
        if desc_len < 2 || offset + desc_len > config_desc.len() {
            break;
        }

        match desc_type {
            DT_INTERFACE if desc_len >= 9 => {
                interfaces.push(UsbInterface {
                    interface_number: config_desc[offset + 2],
                    class: UsbClass::from_u8(config_desc[offset + 5]),
                    subclass: config_desc[offset + 6],
                    protocol: config_desc[offset + 7],
                    endpoints: Vec::new(),
                });
                current_iface = Some(interfaces.len() - 1);
            }
            DT_ENDPOINT if desc_len >= 7 => {
                if let Some(iface_idx) = current_iface {
                    let ep_addr = config_desc[offset + 2];
                    interfaces[iface_idx].endpoints.push(UsbEndpoint {
                        address: ep_addr,
                        direction: if (ep_addr & 0x80) != 0 {
                            UsbDirection::In
                        } else {
                            UsbDirection::Out
                        },
                        transfer_type: match config_desc[offset + 3] & 0x03 {
                            0 => UsbTransferType::Control,
                            1 => UsbTransferType::Isochronous,
                            2 => UsbTransferType::Bulk,
                            _ => UsbTransferType::Interrupt,
                        },
                        max_packet_size: u16::from_le_bytes([
                            config_desc[offset + 4],
                            config_desc[offset + 5],
                        ]),
                        interval: config_desc[offset + 6],
                    });
                }
            }
            _ => {}
        }

        offset += desc_len;
    }

    interfaces
}

fn complete_interrupt_transfer(
    slots: &mut [DeviceSlot],
    ctrl: &XhciController,
    slot_id: u8,
    endpoint_id: u8,
    trb_phys: u64,
    residual_len: u32,
    completion_code: u8,
) -> bool {
    let Some(slot) = slots.iter_mut().find(|slot| {
        slot.enabled && slot_matches_controller(slot, ctrl) && slot.slot_id == slot_id
    }) else {
        return false;
    };

    let Some(endpoint_state) = slot
        .endpoint_rings
        .iter_mut()
        .find(|state| state.endpoint_id == endpoint_id)
    else {
        return false;
    };

    let Some(pending) = endpoint_state.pending_in.take() else {
        return false;
    };

    if pending.trb_phys != trb_phys {
        endpoint_state.pending_in = Some(pending);
        return false;
    }

    if completion_code == 1 {
        let completed_len = pending
            .expected_len
            .saturating_sub(residual_len as usize)
            .min(pending.buffer.len());
        let mut data = pending.buffer.into_vec();
        data.truncate(completed_len);
        endpoint_state
            .completed_in
            .push_back(CompletedInterruptInTransfer { data });
    }

    true
}

fn submit_ep0_transfer(
    ctrl: &XhciController,
    device_id: u8,
    trbs: &[Trb],
    label: &str,
) -> Result<u8, UsbError> {
    let slot_id = {
        let mut slots = DEVICE_SLOTS.lock();
        let slot = slots
            .iter_mut()
            .find(|slot| slot_matches_device_id(slot, ctrl, device_id))
            .ok_or(UsbError::NoDevice)?;
        if slot.control_ring.phys == 0 {
            slot.control_ring.resolve_phys("USB EP0 transfer ring")?;
        }
        for trb in trbs {
            slot.control_ring.enqueue(*trb);
        }
        slot.slot_id
    };
    ctrl.ring_doorbell(slot_id, 1);
    crate::serial_println!(
        "[USB] {} queued on slot={} device_id={} trbs={}",
        label,
        slot_id,
        device_id,
        trbs.len()
    );
    Ok(slot_id)
}

fn drain_controller_events(ctrl: &XhciController) -> Result<usize, UsbError> {
    with_controller_runtime_mut(ctrl, |runtime| {
        let mut drained = 0usize;
        while let Some(trb) = runtime.rings.event.pop() {
            drained += 1;
            match trb.trb_type() {
                x if x == TrbType::TransferEvent as u8 => {
                    let event_trb_phys = (trb.dword0 as u64) | ((trb.dword1 as u64) << 32);
                    let slot_id = (trb.dword3 >> 24) as u8;
                    let endpoint_id = ((trb.dword3 >> 16) & 0x1F) as u8;
                    let completion_code = ((trb.dword2 >> 24) & 0xFF) as u8;
                    let residual_len = trb.dword2 & 0x00FF_FFFF;
                    let mut slots = DEVICE_SLOTS.lock();
                    let completed = complete_interrupt_transfer(
                        &mut slots,
                        ctrl,
                        slot_id,
                        endpoint_id,
                        event_trb_phys,
                        residual_len,
                        completion_code,
                    );
                    crate::serial_println!(
                        "[USB] Transfer event: slot={} ep={} code={} trb={:#x}:{:#x} completed={}",
                        slot_id,
                        endpoint_id,
                        completion_code,
                        trb.dword1,
                        trb.dword0,
                        completed
                    );
                }
                x if x == TrbType::CommandCompletion as u8 => {
                    let slot_id = (trb.dword3 >> 24) as u8;
                    let completion_code = ((trb.dword2 >> 24) & 0xFF) as u8;
                    crate::serial_println!(
                        "[USB] Command completion: slot={} code={} cmd={:#x}:{:#x}",
                        slot_id,
                        completion_code,
                        trb.dword1,
                        trb.dword0
                    );
                }
                x if x == TrbType::PortStatusChange as u8 => {
                    let port_id = (trb.dword0 & 0xFF) as u8;
                    crate::serial_println!("[USB] Port status change event: port={}", port_id);
                }
                other => {
                    crate::serial_println!(
                        "[USB] Event TRB type={} raw={:#x}:{:#x}:{:#x}:{:#x}",
                        other,
                        trb.dword3,
                        trb.dword2,
                        trb.dword1,
                        trb.dword0
                    );
                }
            }
        }
        Ok(drained)
    })
}

/// Enable a device slot and return the slot ID
pub fn enable_slot(ctrl: &XhciController, port: u8) -> Result<u8, UsbError> {
    // Create Enable Slot command TRB
    let trb = Trb {
        dword0: 0,
        dword1: 0,
        dword2: 0,
        dword3: (TrbType::EnableSlot as u32) << 10 | 1, // Cycle bit
    };

    submit_command_trb(ctrl, trb, "USB ENABLE_SLOT")?;

    let slot_id = {
        let slots = DEVICE_SLOTS.lock();
        controller_slot_id(&slots, ctrl, port)?
    };

    crate::serial_println!(
        "[USB] ENABLE_SLOT: controller={:02x}:{:02x}.{} port={} slot_id={}",
        ctrl.bus,
        ctrl.device,
        ctrl.function,
        port,
        slot_id
    );
    let _ = trb;
    Ok(slot_id)
}

/// Address a device (SET_ADDRESS + configure default endpoint)
pub fn address_device(
    ctrl: &XhciController,
    slot_id: u8,
    port: u8,
    speed: UsbSpeed,
) -> Result<(), UsbError> {
    // Create input context
    let mut input_ctx = Box::new(InputContext::default());

    // Set add context flags: slot context (bit 0) + EP0 context (bit 1)
    input_ctx.control.add_flags = 0x3;

    // Configure slot context
    input_ctx.slot = SlotContext::new_device(speed, port, slot_id);

    // Create control endpoint ring
    let mut control_ring = TransferRing::new(16);

    // Configure EP0 context (control endpoint)
    let max_packet = match speed {
        UsbSpeed::Low => 8,
        UsbSpeed::Full => 64,
        UsbSpeed::High => 64,
        UsbSpeed::Super => 512,
        UsbSpeed::SuperPlus => 512,
        UsbSpeed::Unknown => 64,
    };

    let tr_dequeue = control_ring.resolve_phys("USB EP0 transfer ring")?;
    input_ctx.endpoints[0] = EndpointContext::control_endpoint(max_packet, tr_dequeue);

    // Create Address Device command TRB
    let input_ctx_phys = usb_dma_phys(
        input_ctx.as_ref() as *const InputContext,
        "USB input context",
    )?;
    let trb = Trb {
        dword0: input_ctx_phys as u32,
        dword1: (input_ctx_phys >> 32) as u32,
        dword2: 0,
        dword3: (TrbType::AddressDevice as u32) << 10 | (slot_id as u32) << 24 | 1,
    };

    submit_command_trb(ctrl, trb, "USB ADDRESS_DEVICE")?;

    // Store slot info
    let slot = DeviceSlot {
        slot_id,
        usb_address: 0, // Will be assigned by HC
        port,
        speed,
        controller_bus: ctrl.bus,
        controller_device: ctrl.device,
        controller_function: ctrl.function,
        input_context: input_ctx,
        control_ring,
        endpoint_rings: Vec::new(),
        enabled: true,
    };
    DEVICE_SLOTS.lock().push(slot);

    crate::serial_println!(
        "[USB] ADDRESS_DEVICE: slot_id={} port={} speed={:?}",
        slot_id,
        port,
        speed
    );
    let _ = trb;
    Ok(())
}

/// Configure device endpoints based on configuration descriptor
pub fn configure_device(
    ctrl: &XhciController,
    slot_id: u8,
    config_desc: &[u8],
) -> Result<(), UsbError> {
    let interfaces = parse_configuration_interfaces(config_desc);
    let input_ctx_phys = {
        let mut slots = DEVICE_SLOTS.lock();
        let slot = slots
            .iter_mut()
            .find(|slot| slot.enabled && slot_matches_controller(slot, ctrl) && slot.slot_id == slot_id)
            .ok_or(UsbError::NoDevice)?;

        slot.endpoint_rings.clear();
        slot.input_context.control.drop_flags = 0;
        slot.input_context.control.add_flags = 0x3;

        let mut context_entries = 1u8;
        for iface in &interfaces {
            crate::serial_println!(
                "[USB] Interface {}: class={:?} endpoints={}",
                iface.interface_number,
                iface.class,
                iface.endpoints.len()
            );

            for endpoint in &iface.endpoints {
                if (endpoint.address & 0x0F) == 0 {
                    continue;
                }

                let endpoint_id = endpoint_id_from_address(endpoint);
                context_entries = context_entries.max(endpoint_id);

                let mut ring = TransferRing::new(32);
                let tr_dequeue = ring.resolve_phys(&alloc::format!(
                    "USB endpoint ring addr={:#x}",
                    endpoint.address
                ))?;

                let endpoint_context = match endpoint.transfer_type {
                    UsbTransferType::Bulk => EndpointContext::bulk_endpoint(
                        endpoint.max_packet_size,
                        tr_dequeue,
                        endpoint.direction == UsbDirection::In,
                    ),
                    UsbTransferType::Interrupt => EndpointContext::interrupt_endpoint(
                        endpoint.max_packet_size,
                        tr_dequeue,
                        endpoint.direction == UsbDirection::In,
                        endpoint.interval,
                    ),
                    UsbTransferType::Control => EndpointContext::control_endpoint(
                        endpoint.max_packet_size,
                        tr_dequeue,
                    ),
                    UsbTransferType::Isochronous => {
                        crate::serial_println!(
                            "[USB] Endpoint addr={:#x} ignored: isoch not wired",
                            endpoint.address
                        );
                        continue;
                    }
                };

                slot.input_context.endpoints[(endpoint_id - 1) as usize] = endpoint_context;
                slot.input_context.control.add_flags |= 1u32 << endpoint_id;
                slot.endpoint_rings
                    .push(EndpointTransferState::new(*endpoint, endpoint_id, ring));

                crate::serial_println!(
                    "[USB] Endpoint published: addr={:#x} dci={} type={:?} max_packet={} interval={}",
                    endpoint.address,
                    endpoint_id,
                    endpoint.transfer_type,
                    endpoint.max_packet_size,
                    endpoint.interval
                );
            }
        }

        slot.input_context.slot.dword0 =
            (slot.input_context.slot.dword0 & !(0x1Fu32 << 27)) | ((context_entries as u32) << 27);
        usb_dma_phys(
            slot.input_context.as_ref() as *const InputContext,
            "USB configure-endpoint input context",
        )?
    };

    let trb = Trb {
        dword0: input_ctx_phys as u32,
        dword1: (input_ctx_phys >> 32) as u32,
        dword2: 0,
        dword3: (TrbType::ConfigureEndpoint as u32) << 10 | (slot_id as u32) << 24 | 1,
    };
    submit_command_trb(ctrl, trb, "USB CONFIGURE_ENDPOINT")?;

    let mut slots = DEVICE_SLOTS.lock();
    if let Some(slot) = slots
        .iter_mut()
        .find(|slot| slot.enabled && slot_matches_controller(slot, ctrl) && slot.slot_id == slot_id)
    {
        slot.input_context.control.drop_flags = 0;
    }
    Ok(())
}

/// Full device enumeration flow
///
/// Complete enumeration sequence:
/// 1. Port status change detection
/// 2. Port reset
/// 3. Enable Slot command
/// 4. Address Device command (with Input Context)
/// 5. Get Device Descriptor (18 bytes)
/// 6. Get Configuration Descriptor
/// 7. Set Configuration
/// 8. Class-specific initialization
pub fn enumerate_devices_full() -> Vec<UsbDevice> {
    let mut devices = Vec::new();
    let controllers = discover_xhci();

    for mut ctrl in controllers {
        // Initialize controller
        if ctrl.init().is_err() {
            continue;
        }
        *XHCI_CONTROLLER.lock() = Some(ctrl.clone());

        crate::serial_println!(
            "[USB] Enumerating devices on controller {:02x}:{:02x}.{}",
            ctrl.bus,
            ctrl.device,
            ctrl.function
        );

        // Check each port
        for port in 0..ctrl.max_ports {
            // Check if device is connected
            if !ctrl.port_has_device(port) {
                continue;
            }

            crate::serial_println!("[USB] Device detected on port {}", port);

            // Step 1: Reset port
            if ctrl.reset_port(port).is_err() {
                crate::serial_println!("[USB] Port {} reset failed", port);
                continue;
            }

            // Step 2: Get device speed
            let speed = ctrl.get_port_speed(port);
            crate::serial_println!("[USB] Port {} speed: {:?}", port, speed);

            // Step 3: Enable slot
            let slot_id = match enable_slot(&ctrl, port) {
                Ok(id) => id,
                Err(e) => {
                    crate::serial_println!("[USB] Enable slot failed: {:?}", e);
                    continue;
                }
            };

            // Step 4: Address device
            if address_device(&ctrl, slot_id, port, speed).is_err() {
                crate::serial_println!("[USB] Address device failed");
                continue;
            }

            // Step 5: Get device descriptor
            let descriptor = match get_device_descriptor(&ctrl, slot_id) {
                Ok(desc) => desc,
                Err(e) => {
                    crate::serial_println!("[USB] Get device descriptor failed: {:?}", e);
                    continue;
                }
            };

            crate::serial_println!(
                "[USB] Device: VID={:04x} PID={:04x} Class={:02x} Configs={}",
                descriptor.idVendor,
                descriptor.idProduct,
                descriptor.bDeviceClass,
                descriptor.bNumConfigurations
            );

            // Allocate USB address
            let usb_address = NEXT_DEVICE_ADDRESS.fetch_add(1, Ordering::SeqCst) as u8;
            if let Err(err) = bind_slot_usb_address(&ctrl, slot_id, usb_address) {
                crate::serial_println!(
                    "[USB] Failed to bind slot {} to usb address {}: {:?}",
                    slot_id,
                    usb_address,
                    err
                );
            }

            // Step 6: Get configuration descriptor
            let config_desc = match get_configuration_descriptor(&ctrl, slot_id, 0) {
                Ok(desc) => desc,
                Err(e) => {
                    crate::serial_println!("[USB] Get config descriptor failed: {:?}", e);
                    continue;
                }
            };

            // Step 7: Configure device
            if configure_device(&ctrl, slot_id, &config_desc).is_err() {
                crate::serial_println!("[USB] Configure device failed");
            }

            // Step 8: Set configuration
            if set_configuration(&ctrl, slot_id, 1).is_err() {
                crate::serial_println!("[USB] Set configuration failed");
            }

            // Create device structure
            let mut device = UsbDevice {
                address: usb_address,
                port,
                speed,
                controller_bus: ctrl.bus,
                controller_device: ctrl.device,
                controller_function: ctrl.function,
                descriptor: Some(descriptor),
                interfaces: parse_configuration_interfaces(&config_desc),
                device_class: UsbClass::from_u8(descriptor.bDeviceClass),
            };

            crate::serial_println!(
                "[USB] Device enumerated: addr={} interfaces={}",
                device.address,
                device.interfaces.len()
            );

            devices.push(device);
        }
    }

    devices
}

/// Enumerate all USB devices on all controllers (simplified)
pub fn enumerate_devices() -> Vec<UsbDevice> {
    enumerate_devices_full()
}

/// Get partial device descriptor (first 8 bytes)
fn get_device_descriptor_partial(
    ctrl: &XhciController,
    address: u8,
) -> Result<UsbDeviceDescriptor, UsbError> {
    // Setup packet for GET_DESCRIPTOR (first 8 bytes)
    let setup = UsbSetupPacket {
        request_type: 0x80, // Device-to-host, standard, device
        request: GET_DESCRIPTOR,
        value: (DT_DEVICE as u16) << 8,
        index: 0,
        length: 8,
    };

    // DMA tampon: 8 byte deskriptör verisi
    let mut buf = [0u8; 18];

    let data_phys = usb_dma_phys(buf.as_mut_ptr(), "USB partial device descriptor buffer")?;
    let trbs = build_control_transfer_trbs(&setup, Some((data_phys, 8)));
    let _slot_id = submit_ep0_transfer(ctrl, address, &trbs, "USB GET_DESCRIPTOR_PARTIAL")?;

    // Descriptor verisini parse et
    // bLength en az 8 olmalı, ilk 8 byte'tan max packet size çıkar
    let max_packet = if buf[7] > 0 { buf[7] } else { 64 };
    let desc_type = if buf[1] == DT_DEVICE {
        buf[1]
    } else {
        DT_DEVICE
    };

    crate::serial_println!(
        "[USB] GET_DESCRIPTOR_PARTIAL addr={} maxpkt={}",
        address,
        max_packet
    );

    Ok(UsbDeviceDescriptor {
        bLength: if buf[0] >= 18 { buf[0] } else { 18 },
        bDescriptorType: desc_type,
        bcdUSB: u16::from_le_bytes([buf[2], buf[3]]),
        bDeviceClass: buf[4],
        bDeviceSubClass: buf[5],
        bDeviceProtocol: buf[6],
        bMaxPacketSize0: max_packet,
        idVendor: 0, // Tam deskriptörden okunacak
        idProduct: 0,
        bcdDevice: 0,
        iManufacturer: 0,
        iProduct: 0,
        iSerialNumber: 0,
        bNumConfigurations: 0,
    })
}

/// Get full device descriptor (18 byte)
pub fn get_device_descriptor(
    ctrl: &XhciController,
    address: u8,
) -> Result<UsbDeviceDescriptor, UsbError> {
    let setup = UsbSetupPacket {
        request_type: 0x80,
        request: GET_DESCRIPTOR,
        value: (DT_DEVICE as u16) << 8,
        index: 0,
        length: 18,
    };

    // DMA tampon: 18 byte tam deskriptör
    let mut buf = [0u8; 18];

    let data_phys = usb_dma_phys(buf.as_mut_ptr(), "USB device descriptor buffer")?;
    let trbs = build_control_transfer_trbs(&setup, Some((data_phys, 18)));
    let _slot_id = submit_ep0_transfer(ctrl, address, &trbs, "USB GET_DEVICE_DESCRIPTOR")?;

    // DMA buffer'dan deskriptör parse et
    let desc = UsbDeviceDescriptor {
        bLength: if buf[0] >= 18 { buf[0] } else { 18 },
        bDescriptorType: if buf[1] == DT_DEVICE {
            buf[1]
        } else {
            DT_DEVICE
        },
        bcdUSB: u16::from_le_bytes([buf[2], buf[3]]),
        bDeviceClass: buf[4],
        bDeviceSubClass: buf[5],
        bDeviceProtocol: buf[6],
        bMaxPacketSize0: if buf[7] > 0 { buf[7] } else { 64 },
        idVendor: u16::from_le_bytes([buf[8], buf[9]]),
        idProduct: u16::from_le_bytes([buf[10], buf[11]]),
        bcdDevice: u16::from_le_bytes([buf[12], buf[13]]),
        iManufacturer: buf[14],
        iProduct: buf[15],
        iSerialNumber: buf[16],
        bNumConfigurations: if buf[17] > 0 { buf[17] } else { 1 },
    };

    crate::serial_println!(
        "[USB] GET_DEVICE_DESCRIPTOR addr={} vid={:#06x} pid={:#06x} class={}",
        address,
        desc.idVendor,
        desc.idProduct,
        desc.bDeviceClass
    );
    Ok(desc)
}

/// Get configuration descriptor
pub fn get_configuration_descriptor(
    ctrl: &XhciController,
    address: u8,
    config_index: u8,
) -> Result<Vec<u8>, UsbError> {
    let setup = UsbSetupPacket {
        request_type: 0x80,
        request: GET_DESCRIPTOR,
        value: (DT_CONFIGURATION as u16) << 8 | config_index as u16,
        index: 0,
        length: 255,
    };

    // DMA tampon: önce 9 byte config descriptor header oku → wTotalLength al
    let mut header_buf = [0u8; 9];
    let header_phys = usb_dma_phys(
        header_buf.as_mut_ptr(),
        "USB configuration descriptor header buffer",
    )?;
    let header_trbs = build_control_transfer_trbs(&setup, Some((header_phys, 9)));
    let _slot_id = submit_ep0_transfer(
        ctrl,
        address,
        &header_trbs,
        "USB GET_CONFIGURATION_DESCRIPTOR header",
    )?;

    // wTotalLength (byte 2-3) — toplam konfigürasyon verisinin boyutu
    let total_length = u16::from_le_bytes([header_buf[2], header_buf[3]]) as usize;
    let total_length = if total_length > 9 { total_length } else { 9 };
    let total_length = total_length.min(512); // Güvenlik limiti

    // Tam konfigürasyon verisini oku
    let mut full_buf = vec![0u8; total_length];

    let setup2 = UsbSetupPacket {
        request_type: 0x80,
        request: GET_DESCRIPTOR,
        value: (DT_CONFIGURATION as u16) << 8 | config_index as u16,
        index: 0,
        length: total_length as u16,
    };
    let full_phys = usb_dma_phys(
        full_buf.as_mut_ptr(),
        "USB configuration descriptor buffer",
    )?;
    let full_trbs = build_control_transfer_trbs(&setup2, Some((full_phys, total_length as u32)));
    let _slot_id = submit_ep0_transfer(
        ctrl,
        address,
        &full_trbs,
        "USB GET_CONFIGURATION_DESCRIPTOR full",
    )?;

    // Header verisi ile full_buf'u birleştir (header zaten full_buf'un başı)
    full_buf[..9.min(total_length)].copy_from_slice(&header_buf[..9.min(total_length)]);

    crate::serial_println!(
        "[USB] GET_CONFIGURATION_DESCRIPTOR addr={} idx={} total_len={}",
        address,
        config_index,
        total_length
    );
    Ok(full_buf)
}

/// Set device address
pub fn set_device_address(ctrl: &XhciController, slot_id: u8, address: u8) -> Result<(), UsbError> {
    // Address Device command TRB oluştur
    let trb = Trb {
        dword0: 0, // Input context address (low) — gerçek uygulamada input context tahsis edilir
        dword1: 0, // Input context address (high)
        dword2: 0,
        dword3: (TrbType::AddressDevice as u32) << 10 | (slot_id as u32) << 24 | 1,
    };

    // Command ring'e TRB yaz ve doorbell ring
    ctrl.ring_doorbell(0, 0); // Host controller doorbell

    crate::serial_println!("[USB] SET_ADDRESS: slot={} address={}", slot_id, address);
    let _ = (trb, address);
    Ok(())
}

/// Set configuration
pub fn set_configuration(
    ctrl: &XhciController,
    address: u8,
    config_value: u8,
) -> Result<(), UsbError> {
    let setup = UsbSetupPacket {
        request_type: 0x00, // Host-to-device, standard, device
        request: SET_CONFIGURATION,
        value: config_value as u16,
        index: 0,
        length: 0,
    };

    let trbs = build_control_transfer_trbs(&setup, None);
    let _slot_id = submit_ep0_transfer(ctrl, address, &trbs, "USB SET_CONFIGURATION")?;
    crate::serial_println!(
        "[USB] SET_CONFIGURATION: addr={} config={}",
        address,
        config_value
    );
    Ok(())
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
                ms_devices.push(MassStorageDevice::new(
                    device.clone(),
                    iface.interface_number,
                ));
            }
        }
    }
    ms_devices
}

/// Event ring polling and processing
pub fn process_events(ctrl: &XhciController) {
    let mut should_drain = false;
    if let Some(op) = ctrl.get_operational_regs() {
        let usbsts = unsafe { read_volatile(&op.usbsts) };
        if (usbsts & USBSTS_EINT) != 0 {
            should_drain = true;
        }
    }
    if let Some(rt) = ctrl.get_runtime_regs() {
        let iman = unsafe { read_volatile(&rt.irs[0].iman) };
        if (iman & XHCI_IMAN_IP) != 0 {
            should_drain = true;
        }
    }

    if !should_drain {
        return;
    }

    if let Ok(drained) = drain_controller_events(ctrl) {
        if drained > 0 {
            crate::serial_println!("[USB] Drained {} event TRBs", drained);
        }
    }

    if let Some(rt) = ctrl.get_runtime_regs_mut() {
        let erdp = with_controller_runtime_mut(ctrl, |runtime| Ok(runtime.event_dequeue_phys()))
            .unwrap_or(0);
        unsafe {
            write_volatile(&mut rt.irs[0].erdp, erdp | XHCI_ERDP_EHB);
            let iman = read_volatile(&rt.irs[0].iman);
            write_volatile(&mut rt.irs[0].iman, (iman & XHCI_IMAN_IE) | XHCI_IMAN_IP);
        }
    }
}

/// Port status change event handler
pub fn handle_port_change(ctrl: &XhciController, port: u8) -> Result<(), UsbError> {
    let port_regs = ctrl.get_port_regs(port).ok_or(UsbError::NoDevice)?;
    let portsc = unsafe { read_volatile(&port_regs.portsc) };

    // Clear change bits by writing 1
    let change_bits =
        PORTSC_CSC | PORTSC_PEC | PORTSC_WRC | PORTSC_OCC | PORTSC_PRC | PORTSC_PLC | PORTSC_CEC;

    crate::serial_println!(
        "[USB] Port {} change: CCS={} PED={} speed={}",
        port,
        (portsc & PORTSC_CCS) != 0,
        (portsc & PORTSC_PED) != 0,
        (portsc & PORTSC_SPEED_MASK) >> 10
    );

    // Clear change bits
    unsafe {
        let port_regs_mut = ctrl.get_port_regs_mut(port).unwrap();
        write_volatile(&mut port_regs_mut.portsc, portsc | change_bits);
    }

    // If device connected and not enabled, trigger enumeration
    if (portsc & PORTSC_CCS) != 0 && (portsc & PORTSC_PED) == 0 {
        crate::serial_println!(
            "[USB] Port {}: device connected, triggering enumeration",
            port
        );
        // Re-enumerate would happen here
    }

    Ok(())
}

/// Initialize USB subsystem and enumerate all devices
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
            ctrl.bus,
            ctrl.device,
            ctrl.function,
            ctrl.vendor_id,
            ctrl.device_id,
            ctrl.mmio_base
        );
    }

    // Initialize each controller
    for mut ctrl in controllers {
        if let Err(e) = ctrl.init() {
            crate::serial_println!("[USB] Failed to init controller: {:?}", e);
            continue;
        }
        *XHCI_CONTROLLER.lock() = Some(ctrl.clone());

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
        if let Some(bar) =
            crate::drivers::pci::read_bar_mmio(ctrl.bus, ctrl.device, ctrl.function, 0)
        {
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

    crate::serial_println!(
        "[USB] Found {} devices, {} HID, {} mass storage",
        devices.len(),
        hid_count,
        ms_count
    );

    // Initialize each HID device
    for hid in HID_DEVICES.lock().iter_mut() {
        crate::serial_println!("[USB] HID device on interface {}", hid.interface);
        let _ = hid::register_hid_driver(hid.device.clone(), hid.interface);
    }
    hid::init_all_hid();

    // Initialize each mass storage device
    for ms in MASS_STORAGE_DEVICES.lock().iter_mut() {
        crate::serial_println!("[USB] Mass storage device on interface {}", ms.interface);
        // Read capacity
        if let Ok((last_lba, block_size)) = ms.read_capacity() {
            ms.block_count = (last_lba as u64) + 1;
            ms.block_size = block_size;
            crate::serial_println!(
                "[USB] Mass storage: {} blocks x {} bytes = {} MB",
                ms.block_count,
                ms.block_size,
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
        process_events(&ctrl);
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

#[cfg(test)]
mod tests {
    use super::{
        build_control_transfer_trbs, complete_interrupt_transfer, controller_slot_id,
        endpoint_id_from_address, parse_configuration_interfaces, submit_ep0_transfer, DeviceSlot,
        EndpointTransferState, InputContext, TransferRing, TrbType, UsbDirection, UsbEndpoint,
        UsbDevice, UsbSetupPacket, UsbSpeed, UsbTransferType, XhciController,
    };
    use alloc::boxed::Box;
    use alloc::vec;
    use alloc::vec::Vec;
    use core::mem;

    fn slot(
        slot_id: u8,
        bus: u8,
        device: u8,
        function: u8,
        port: u8,
        enabled: bool,
    ) -> DeviceSlot {
        DeviceSlot {
            slot_id,
            usb_address: 0,
            port,
            speed: UsbSpeed::High,
            controller_bus: bus,
            controller_device: device,
            controller_function: function,
            input_context: Box::new(InputContext::default()),
            control_ring: TransferRing::new(16),
            endpoint_rings: Vec::new(),
            enabled,
        }
    }

    fn ctrl() -> XhciController {
        XhciController {
            bus: 0,
            device: 20,
            function: 0,
            vendor_id: 0x8086,
            device_id: 0x1234,
            mmio_base: 0,
            max_slots: 8,
            max_ports: 8,
        }
    }

    #[test]
    fn trb_layout_is_dma_ready() {
        assert_eq!(mem::size_of::<super::Trb>(), 16);
        assert_eq!(mem::align_of::<super::Trb>(), 16);
        assert_eq!(mem::align_of::<super::ErstEntry>(), 64);
    }

    #[test]
    fn build_control_transfer_trbs_for_in_data_sets_out_status_stage() {
        let setup = UsbSetupPacket {
            request_type: 0x80,
            request: 0x06,
            value: 0x0100,
            index: 0,
            length: 18,
        };
        let trbs = build_control_transfer_trbs(&setup, Some((0x1234_5000, 18)));
        assert_eq!(trbs.len(), 3);
        assert_eq!(trbs[0].trb_type(), TrbType::SetupStage as u8);
        assert_eq!(trbs[1].trb_type(), TrbType::DataStage as u8);
        assert!(trbs[1].direction_in());
        assert_eq!(trbs[2].trb_type(), TrbType::StatusStage as u8);
        assert!(!trbs[2].direction_in());
        assert!(trbs[2].interrupt_on_completion());
    }

    #[test]
    fn build_control_transfer_trbs_for_out_data_sets_in_status_stage() {
        let setup = UsbSetupPacket {
            request_type: 0x21,
            request: 0x09,
            value: 0x0200,
            index: 1,
            length: 1,
        };
        let trbs = build_control_transfer_trbs(&setup, Some((0x1234_6000, 1)));
        assert_eq!(trbs.len(), 3);
        assert!(!trbs[1].direction_in());
        assert!(trbs[2].direction_in());
    }

    #[test]
    fn submit_ep0_transfer_resolves_usb_address_to_slot_id() {
        let ctrl = ctrl();
        {
            let mut runtimes = super::XHCI_RUNTIMES.lock();
            runtimes.clear();
        }
        let mut control_ring = TransferRing::new(16);
        control_ring.phys = 0x2000;
        let trbs = build_control_transfer_trbs(
            &UsbSetupPacket {
                request_type: 0x00,
                request: 0x09,
                value: 1,
                index: 0,
                length: 0,
            },
            None,
        );
        {
            let mut slots = super::DEVICE_SLOTS.lock();
            slots.clear();
            slots.push(DeviceSlot {
                slot_id: 5,
                usb_address: 9,
                port: 1,
                speed: UsbSpeed::High,
                controller_bus: ctrl.bus,
                controller_device: ctrl.device,
                controller_function: ctrl.function,
                input_context: Box::new(InputContext::default()),
                control_ring,
                endpoint_rings: Vec::new(),
                enabled: true,
            });
        }

        let slot_id = submit_ep0_transfer(&ctrl, 9, &trbs, "unit-test").unwrap();
        assert_eq!(slot_id, 5);
        let slots = super::DEVICE_SLOTS.lock();
        let slot = slots.first().unwrap();
        assert_eq!(slot.control_ring.ring.enqueue, trbs.len());
    }

    #[test]
    fn configuration_parser_preserves_interface_endpoints() {
        let config_desc = [
            9, 2, 34, 0, 1, 1, 0, 0x80, 50, // configuration
            9, 4, 0, 0, 2, 3, 1, 1, 0, // interface
            7, 5, 0x81, 0x03, 8, 0, 10, // interrupt in
            7, 5, 0x01, 0x03, 8, 0, 10, // interrupt out
        ];
        let interfaces = parse_configuration_interfaces(&config_desc);
        assert_eq!(interfaces.len(), 1);
        assert_eq!(interfaces[0].endpoints.len(), 2);
        assert_eq!(interfaces[0].endpoints[0].direction, UsbDirection::In);
        assert_eq!(
            interfaces[0].endpoints[0].transfer_type,
            UsbTransferType::Interrupt
        );
        assert_eq!(interfaces[0].endpoints[1].direction, UsbDirection::Out);
    }

    #[test]
    fn endpoint_id_formula_matches_xhci_dci_layout() {
        let interrupt_in = UsbEndpoint {
            address: 0x81,
            direction: UsbDirection::In,
            transfer_type: UsbTransferType::Interrupt,
            max_packet_size: 8,
            interval: 10,
        };
        let interrupt_out = UsbEndpoint {
            address: 0x01,
            direction: UsbDirection::Out,
            transfer_type: UsbTransferType::Interrupt,
            max_packet_size: 8,
            interval: 10,
        };
        assert_eq!(endpoint_id_from_address(&interrupt_out), 2);
        assert_eq!(endpoint_id_from_address(&interrupt_in), 3);
    }

    #[test]
    fn complete_interrupt_transfer_moves_buffer_to_completed_queue() {
        let ctrl = ctrl();
        let endpoint = UsbEndpoint {
            address: 0x81,
            direction: UsbDirection::In,
            transfer_type: UsbTransferType::Interrupt,
            max_packet_size: 8,
            interval: 10,
        };
        let mut endpoint_state =
            EndpointTransferState::new(endpoint, endpoint_id_from_address(&endpoint), TransferRing::new(16));
        endpoint_state.pending_in = Some(super::PendingInterruptInTransfer {
            trb_phys: 0x2000,
            expected_len: 8,
            buffer: vec![1, 2, 3, 4, 0, 0, 0, 0].into_boxed_slice(),
        });
        let mut slots = vec![DeviceSlot {
            slot_id: 5,
            usb_address: 9,
            port: 1,
            speed: UsbSpeed::High,
            controller_bus: ctrl.bus,
            controller_device: ctrl.device,
            controller_function: ctrl.function,
            input_context: Box::new(InputContext::default()),
            control_ring: TransferRing::new(16),
            endpoint_rings: vec![endpoint_state],
            enabled: true,
        }];
        assert!(complete_interrupt_transfer(
            &mut slots,
            &ctrl,
            5,
            3,
            0x2000,
            4,
            1
        ));
        let completed = slots[0].endpoint_rings[0].completed_in.pop_front().unwrap();
        assert_eq!(completed.data, vec![1, 2, 3, 4]);
    }

    #[test]
    fn interrupt_transfer_in_consumes_completed_report_before_queueing() {
        let ctrl = ctrl();
        *super::XHCI_CONTROLLER.lock() = Some(ctrl.clone());
        let endpoint = UsbEndpoint {
            address: 0x81,
            direction: UsbDirection::In,
            transfer_type: UsbTransferType::Interrupt,
            max_packet_size: 8,
            interval: 10,
        };
        let mut endpoint_state =
            EndpointTransferState::new(endpoint, endpoint_id_from_address(&endpoint), TransferRing::new(16));
        endpoint_state
            .completed_in
            .push_back(super::CompletedInterruptInTransfer {
                data: vec![0, 0, 4, 0, 0, 0, 0, 0],
            });
        {
            let mut slots = super::DEVICE_SLOTS.lock();
            slots.clear();
            slots.push(DeviceSlot {
                slot_id: 5,
                usb_address: 9,
                port: 1,
                speed: UsbSpeed::High,
                controller_bus: ctrl.bus,
                controller_device: ctrl.device,
                controller_function: ctrl.function,
                input_context: Box::new(InputContext::default()),
                control_ring: TransferRing::new(16),
                endpoint_rings: vec![endpoint_state],
                enabled: true,
            });
        }
        let mut device = UsbDevice {
            address: 9,
            port: 1,
            speed: UsbSpeed::High,
            controller_bus: ctrl.bus,
            controller_device: ctrl.device,
            controller_function: ctrl.function,
            descriptor: None,
            interfaces: Vec::new(),
            device_class: super::UsbClass::Hid,
        };
        let report = device.interrupt_transfer_in(endpoint).unwrap().unwrap();
        assert_eq!(report, vec![0, 0, 4, 0, 0, 0, 0, 0]);
        let slots = super::DEVICE_SLOTS.lock();
        assert!(slots[0].endpoint_rings[0].pending_in.is_none());
    }

    #[test]
    fn controller_slot_id_reuses_existing_port_mapping() {
        let ctrl = ctrl();
        let slots = vec![slot(3, 0, 20, 0, 2, true)];
        assert_eq!(controller_slot_id(&slots, &ctrl, 2).unwrap(), 3);
    }

    #[test]
    fn controller_slot_id_picks_first_free_slot_per_controller() {
        let mut ctrl = ctrl();
        ctrl.max_slots = 4;
        let slots = vec![
            slot(1, 0, 20, 0, 1, true),
            slot(3, 0, 20, 0, 3, true),
            slot(1, 0, 21, 0, 1, true),
        ];
        assert_eq!(controller_slot_id(&slots, &ctrl, 4).unwrap(), 2);
    }

    #[test]
    fn controller_slot_id_fails_when_controller_slots_are_exhausted() {
        let mut ctrl = ctrl();
        ctrl.max_slots = 2;
        let slots = vec![slot(1, 0, 20, 0, 1, true), slot(2, 0, 20, 0, 2, true)];
        assert!(controller_slot_id(&slots, &ctrl, 3).is_err());
    }
}
