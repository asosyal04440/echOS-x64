//! # AHCI SÃ¼rÃ¼cÃ¼sÃ¼ (Advanced Host Controller Interface)
//!
//! Intel ICH10 ve uyumlu SATA denetleyicileri iÃSection in AHCI 1.3.1 sÃ¼rÃ¼cÃ¼sÃ¼.
//! SATA disklerine PIO/DMA modunda eriÅŸim saÄŸlar.
//!
//! ## AHCI Mimarisi
//!
//! ```text
//!   CPU                          AHCI HBA                    SATA Disk
//!    |                              |                            |
//!    |-- MMIO (BAR5) ------------->|                            |
//!    |                              |-- Port 0-31 ------------->|
//!    |                              |                            |
//!    |-- Command List (RAM) ------->|                            |
//!    |   (32 commands/port)         |                            |
//!    |                              |-- Command Issue ---------->|
//!    |                              |                            |
//!    |<-- Completion IRQ -----------|<-- D2H FIS ---------------|
//! ```

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::ptr;
use core::ptr::NonNull;
use lazy_static::lazy_static;
use spin::Mutex;
use x86_64::instructions::port::Port;
use x86_64::PhysAddr;

use crate::drivers::block::{BlockDevice, BlockDeviceError, BlockDeviceType};
use crate::drivers::pci::{scan, PciDevice};

// ============================================================================
// AHCI SABÄ°TLERÄ°
// ============================================================================

/// AHCI HBA genel yazmaÃSection  ofsetleri
const AHCI_GHC: usize = 0x04; // Global HBA Control
const AHCI_IS: usize = 0x08; // Interrupt Status
const AHCI_PI: usize = 0x0C; // Ports Implemented
const AHCI_VS: usize = 0x10; // AHCI Version
const AHCI_CCC_CTL: usize = 0x14; // Coalescing Control
const AHCI_CCC_PORTS: usize = 0x18;
const AHCI_EM_LOC: usize = 0x1C;
const AHCI_EM_CTL: usize = 0x20;
const AHCI_CAP2: usize = 0x24;
const AHCI_BOHC: usize = 0x28;

/// GHC (Global HBA Control) bitleri
const GHC_HR: u32 = 1 << 0; // HBA Reset
const GHC_IE: u32 = 1 << 1; // Interrupt Enable
const GHC_MRSM: u32 = 1 << 2; // MSI Revert to Single Message
const GHC_AE: u32 = 1 << 31; // AHCI Enable

/// CAP (HBA Capabilities) bitleri
const CAP_S64A: u32 = 1 << 31; // 64-bit Addressing
const CAP_SNCQ: u32 = 1 << 30; // Native Command Queuing
const CAP_SSS: u32 = 1 << 27; // Staggered Spin-up
const CAP_SALP: u32 = 1 << 26; // Aggressive Link Power Mgmt
const CAP_SAL: u32 = 1 << 25; // Activity LED
const CAP_SCLO: u32 = 1 << 24; // Command List Override

/// Port yazmaÃSection  ofsetleri (her port 0x80 byte)
const PORT_CLB: usize = 0x00; // Command List Base
const PORT_CLBU: usize = 0x04; // Command List Base Upper
const PORT_FB: usize = 0x08; // FIS Base
const PORT_FBU: usize = 0x0C; // FIS Base Upper
const PORT_IS: usize = 0x10; // Interrupt Status
const PORT_IE: usize = 0x14; // Interrupt Enable
const PORT_CMD: usize = 0x18; // Command and Status
const PORT_TFD: usize = 0x20; // Task File Data
const PORT_SIG: usize = 0x24; // Signature
const PORT_SSTS: usize = 0x28; // SATA Status
const PORT_SCTL: usize = 0x2C; // SATA Control
const PORT_SERR: usize = 0x30; // SATA Error
const PORT_SACT: usize = 0x34; // SATA Active
const PORT_CI: usize = 0x38; // Command Issue
const PORT_SNTF: usize = 0x3C; // SATA Notification
const PORT_FBS: usize = 0x40; // FIS-based Switching Control

/// Port CMD bitleri
const CMD_ST: u32 = 1 << 0; // Start
const CMD_SUD: u32 = 1 << 1; // Spin-Up Device
const CMD_POD: u32 = 1 << 2; // Power On Device
const CMD_CLO: u32 = 1 << 3; // Command List Override
const CMD_FRE: u32 = 1 << 4; // FIS Receive Enable
const CMD_FR: u32 = 1 << 14; // FIS Receive Running
const CMD_CR: u32 = 1 << 15; // Command List Running

/// SATA Status (SSTS) deÄŸerleri
const SSTS_DET_PRESENT: u32 = 0x03; // Device present, Phy established

/// FIS tipleri
const FIS_TYPE_REG_H2D: u8 = 0x27; // Register - Host to Device
const FIS_TYPE_REG_D2H: u8 = 0x34; // Register - Device to Host

/// ATA komutlarÄ±
const ATA_CMD_IDENTIFY: u8 = 0xEC;
const ATA_CMD_READ_DMA_EXT: u8 = 0x25;
const ATA_CMD_WRITE_DMA_EXT: u8 = 0x35;
const ATA_CMD_FLUSH_CACHE: u8 = 0xE7;

/// SektÃ¶r boyutu
pub const BLOCK_SIZE: usize = 512;

/// IDENTIFY DEVICE yanÄ±tÄ± (512 byte = 256 word)
#[repr(C, align(512))]
pub struct IdentifyDeviceData {
    pub words: [u16; 256],
}

impl IdentifyDeviceData {
    pub fn is_atapi(&self) -> bool {
        (self.words[0] & (1 << 15)) != 0
    }

    pub fn lba28_sectors(&self) -> u32 {
        ((self.words[61] as u32) << 16) | (self.words[60] as u32)
    }

    pub fn lba48_sectors(&self) -> u64 {
        ((self.words[103] as u64) << 48)
            | ((self.words[102] as u64) << 32)
            | ((self.words[101] as u64) << 16)
            | (self.words[100] as u64)
    }

    pub fn model_string(&self) -> [u8; 40] {
        let mut buf = [0u8; 40];
        for i in 0..20 {
            let w = self.words[27 + i];
            buf[i * 2] = (w >> 8) as u8;
            buf[i * 2 + 1] = (w & 0xFF) as u8;
        }
        buf
    }
}

// ============================================================================
// AHCI YAPILARI
// ============================================================================

/// AHCI HBA bellek haritasÄ±
#[repr(C)]
struct AhciHba {
    cap: u32,        // 0x00 - HBA Capabilities
    ghc: u32,        // 0x04 - Global HBA Control
    is: u32,         // 0x08 - Interrupt Status
    pi: u32,         // 0x0C - Ports Implemented
    vs: u32,         // 0x10 - AHCI Version
    ccc_ctl: u32,    // 0x14
    ccc_pts: u32,    // 0x18
    em_loc: u32,     // 0x1C
    em_ctl: u32,     // 0x20
    cap2: u32,       // 0x24
    bohc: u32,       // 0x28
    _rsv: [u8; 116], // 0x2C - 0x9F
    vendor: [u8; 96], // 0xA0 - 0xFF
                     // Portlar 0x100'den baÅŸlar
}

/// AHCI Port yazmaÃSection larÄ±
#[repr(C)]
struct AhciPort {
    clb: u32,         // 0x00 - Command List Base
    clbu: u32,        // 0x04
    fb: u32,          // 0x08 - FIS Base
    fbu: u32,         // 0x0C
    is: u32,          // 0x10 - Interrupt Status
    ie: u32,          // 0x14 - Interrupt Enable
    cmd: u32,         // 0x18 - Command and Status
    _rsv0: u32,       // 0x1C
    tfd: u32,         // 0x20 - Task File Data
    sig: u32,         // 0x24 - Signature
    ssts: u32,        // 0x28 - SATA Status
    sctl: u32,        // 0x2C - SATA Control
    serr: u32,        // 0x30 - SATA Error
    sact: u32,        // 0x34 - SATA Active
    ci: u32,          // 0x38 - Command Issue
    sntf: u32,        // 0x3C
    fbs: u32,         // 0x40
    _rsv1: [u32; 11], // 0x44 - 0x6F
    vendor: [u32; 4], // 0x70 - 0x7F
}

/// Command Table yapÄ±sÄ±
#[repr(C, align(128))]
struct AhciCommandTable {
    /// FIS (Host to Device)
    cfis: [u8; 64],
    /// ATAPI Command
    acmd: [u8; 16],
    /// Reserved
    _rsv: [u8; 48],
    /// Physical Region Descriptor Table (max 65535 entries, we use 8)
    prdt: [AhciPrdt; 8],
}

/// Physical Region Descriptor Table entry
#[repr(C)]
#[derive(Clone, Copy)]
struct AhciPrdt {
    dba: u32,  // Data Base Address
    dbau: u32, // Data Base Address Upper
    _rsv: u32,
    dbc: u32, // Data Byte Count (bit 0 = interrupt on completion)
}

/// Command Header
#[repr(C, align(128))]
#[derive(Clone, Copy)]
struct AhciCommandHeader {
    dw0: u32,   // Flags
    prdbc: u32, // PRD Byte Count
    ctba: u32,  // Command Table Base
    ctbau: u32, // Command Table Base Upper
    _rsv: [u32; 4],
}

/// FIS (Host to Device) yapÄ±
#[repr(C)]
struct FisRegH2D {
    fis_type: u8,
    pm_port: u8,
    command: u8,
    lba0: u8,
    feature: u8,
    lba1: u8,
    lba2: u8,
    device: u8,
    lba3: u8,
    lba4: u8,
    lba5: u8,
    feature_exp: u8,
    control: u8,
    lba_exp: u8,
    count: u8,
    _rsv: [u8; 6],
}

// ============================================================================
// AHCI DENETLEYÄ°CÄ°SÄ°
// ============================================================================

/// AHCI denetleyicisi
pub struct AhciController {
    /// MMIO base address (virtual)
    base: *mut AhciHba,
    /// Implemented ports bitmap
    port_mask: u32,
    /// Port count
    port_count: usize,
    /// BAR5 physical address
    bar5_phys: u64,
    /// BAR5 size
    bar5_size: u64,
}

// Raw pointers iÃSection in Send implementasyonu (MMIO eriÅŸimi iÃSection in gÃ¼venli)
unsafe impl Send for AhciController {}
unsafe impl Sync for AhciController {}

/// AHCI portu
pub struct AhciPortDevice {
    port_idx: usize,
    port_base: *mut AhciPort,
    cmd_list_phys: u64,
    cmd_table_phys: u64,
    fis_phys: u64,
    /// Command list buffer (aligned)
    cmd_list: Box<[AhciCommandHeader; 32]>,
    /// Command table buffer (aligned)
    cmd_table: Box<AhciCommandTable>,
    /// FIS receive buffer
    fis_recv: Box<[u8; 256]>,
    /// Device signature
    signature: u32,
}

// Raw pointers iÃSection in Send implementasyonu
unsafe impl Send for AhciPortDevice {}
unsafe impl Sync for AhciPortDevice {}

// ============================================================================
// AHCI HATALARI
// ============================================================================

#[derive(Debug, Clone, Copy)]
pub enum AhciError {
    NoController,
    IoError,
    Timeout,
    NoDevice,
    PortNotReady,
    CommandFailed,
    InvalidSignature,
}

// ============================================================================
// AHCI CONTROLLER IMPLEMENTATION
// ============================================================================

impl AhciController {
    /// PCI taramasÄ±ndan AHCI denetleyicisini bul
    pub fn find() -> Option<Self> {
        let devices = scan();
        for dev in devices {
            // SATA AHCI controller: class=0x01, subclass=0x06, prog_if=0x01
            if dev.class_code == 0x01 && dev.subclass == 0x06 && dev.prog_if == 0x01 {
                crate::serial_println!(
                    "[AHCI] Found SATA AHCI controller at {:02x}:{:02x}.{:x}",
                    dev.bus,
                    dev.device,
                    dev.function
                );
                return Self::init(&dev);
            }
        }
        crate::serial_println!("[AHCI] No AHCI controller found");
        None
    }

    /// AHCI denetleyicisini baÅŸlat
    fn init(pci_dev: &PciDevice) -> Option<Self> {
        // BAR5'i oku (AHCI ABAR)
        let bar5_phys = Self::read_bar5(pci_dev)?;
        crate::serial_println!("[AHCI] BAR5 phys = 0x{:x}", bar5_phys);

        // BAR5 boyutunu hesapla
        let bar5_size = Self::get_bar_size(pci_dev, 0x24);
        crate::serial_println!("[AHCI] BAR5 size = {} bytes", bar5_size);

        // MMIO mapping - BAR5 fiziksel adresini sanal adrese map et
        let base = crate::memory::map_mmio(bar5_phys, bar5_size as usize) as *mut AhciHba;
        crate::serial_println!("[AHCI] BAR5 virt = 0x{:x}", base as u64);

        // AHCI enable kontrolÃ¼
        unsafe {
            let ghc = core::ptr::read_volatile(&(*base).ghc);
            crate::serial_println!("[AHCI] GHC = 0x{:x}", ghc);

            let cap = core::ptr::read_volatile(&(*base).cap);
            crate::serial_println!("[AHCI] CAP = 0x{:x}", cap);

            let vs = core::ptr::read_volatile(&(*base).vs);
            crate::serial_println!("[AHCI] Version = {}.{}", (vs >> 16) & 0xFF, vs & 0xFF);

            // Ports Implemented
            let pi = core::ptr::read_volatile(&(*base).pi);
            crate::serial_println!("[AHCI] PI (ports) = 0x{:x}", pi);

            // AHCI enable et
            if ghc & GHC_AE == 0 {
                core::ptr::write_volatile(&mut (*base).ghc, ghc | GHC_AE);
            }

            // Interrupt enable
            core::ptr::write_volatile(
                &mut (*base).ghc,
                core::ptr::read_volatile(&(*base).ghc) | GHC_IE,
            );

            let port_count = pi.count_ones() as usize;
            crate::serial_println!("[AHCI] {} ports implemented", port_count);

            return Some(Self {
                base,
                port_mask: pi,
                port_count,
                bar5_phys: bar5_phys,
                bar5_size,
            });
        }
    }

    /// BAR5 deÄŸerini oku
    fn read_bar5(dev: &PciDevice) -> Option<u64> {
        // BAR5 offset = 0x24
        let addr = 0x80000000
            | ((dev.bus as u32) << 16)
            | ((dev.device as u32) << 11)
            | ((dev.function as u32) << 8)
            | 0x24;
        unsafe {
            Port::<u32>::new(0xCF8).write(addr);
            let bar = Port::<u32>::new(0xCFC).read();
            // Bit 0 = memory space indicator (0 = memory)
            // Bit 2-3 = type (00 = 32-bit, 10 = 64-bit)
            let bar_type = (bar >> 1) & 0x3;
            if bar & 1 == 0 && bar_type == 0 {
                // 32-bit memory BAR
                return Some((bar & 0xFFFFFFF0) as u64);
            } else if bar & 1 == 0 && bar_type == 2 {
                // 64-bit memory BAR
                let addr2 = addr + 4;
                Port::<u32>::new(0xCF8).write(addr2);
                let bar_hi = Port::<u32>::new(0xCFC).read();
                return Some(((bar_hi as u64) << 32) | ((bar & 0xFFFFFFF0) as u64));
            }
        }
        None
    }

    /// BAR boyutunu hesapla
    fn get_bar_size(dev: &PciDevice, bar_offset: u32) -> u64 {
        unsafe {
            let addr = 0x80000000
                | ((dev.bus as u32) << 16)
                | ((dev.device as u32) << 11)
                | ((dev.function as u32) << 8)
                | bar_offset;

            // Original deÄŸeri kaydet
            Port::<u32>::new(0xCF8).write(addr);
            let original = Port::<u32>::new(0xCFC).read();

            // TÃ¼m bitleri yaz
            Port::<u32>::new(0xCFC).write(0xFFFFFFFF);
            Port::<u32>::new(0xCF8).write(addr);
            let size_bits = Port::<u32>::new(0xCFC).read();

            // Restore
            Port::<u32>::new(0xCF8).write(addr);
            Port::<u32>::new(0xCFC).write(original);

            // Size = ~(size_bits & 0xFFFFFFF0) + 1
            let size = !(size_bits & 0xFFFFFFF0) + 1;
            size as u64
        }
    }

    /// Portu al
    fn get_port(&self, port_idx: usize) -> Option<*mut AhciPort> {
        if port_idx >= 32 || (self.port_mask & (1 << port_idx)) == 0 {
            return None;
        }
        // Port offset = 0x100 + port_idx * 0x80
        let port_offset = 0x100 + port_idx * 0x80;
        unsafe { Some((self.base as *mut u8).add(port_offset) as *mut AhciPort) }
    }

    /// Aktif portu bul ve baÅŸlat
    pub fn find_active_port(&self) -> Option<AhciPortDevice> {
        for port_idx in 0..32 {
            if (self.port_mask & (1 << port_idx)) == 0 {
                continue;
            }

            let port = self.get_port(port_idx)?;

            unsafe {
                let ssts = core::ptr::read_volatile(&(*port).ssts);
                let det = ssts & 0xF;

                crate::serial_println!("[AHCI] Port {} SSTS=0x{:x} DET={}", port_idx, ssts, det);

                // Device present?
                if det == SSTS_DET_PRESENT {
                    let sig = core::ptr::read_volatile(&(*port).sig);
                    crate::serial_println!("[AHCI] Port {} signature = 0x{:x}", port_idx, sig);

                    // ATA signature = 0x00000101, ATAPI = 0xEB140101
                    if sig == 0x00000101 {
                        crate::serial_println!("[AHCI] Port {} has ATA disk", port_idx);
                        return AhciPortDevice::init(port_idx, port);
                    }
                }
            }
        }
        crate::serial_println!("[AHCI] No active ATA device found");
        None
    }
}

// ============================================================================
// AHCI PORT IMPLEMENTATION
// ============================================================================

impl AhciPortDevice {
    /// Portu baÅŸlat
    fn init(port_idx: usize, port: *mut AhciPort) -> Option<Self> {
        unsafe {
            // Port durumunu kontrol et
            let cmd = core::ptr::read_volatile(&(*port).cmd);
            crate::serial_println!("[AHCI] Port {} CMD = 0x{:x}", port_idx, cmd);

            // Portu durdur
            core::ptr::write_volatile(&mut (*port).cmd, cmd & !(CMD_ST | CMD_FRE));

            // Bekle
            for _ in 0..100000 {
                core::hint::spin_loop();
            }

            // DMA-capable memory ayÄ±r (fiziksel olarak contiguous)
            // Command list: 32 * 32 bytes = 1024 bytes = 1 page
            // Command table: 128 + 48 + 8*16 = 256 bytes (ama alignment gerekir)
            // FIS: 256 bytes

            let (cmd_list_phys, cmd_list_virt) = crate::memory::dma_alloc(1)?;
            let (cmd_table_phys, cmd_table_virt) = crate::memory::dma_alloc(1)?;
            let (fis_phys, fis_virt) = crate::memory::dma_alloc(1)?;

            let cmd_list_phys = cmd_list_phys as u64;
            let cmd_table_phys = cmd_table_phys as u64;
            let fis_phys = fis_phys as u64;

            crate::serial_println!(
                "[AHCI] DMA buffers: cmd_list={:#x}, cmd_table={:#x}, fis={:#x}",
                cmd_list_phys,
                cmd_table_phys,
                fis_phys
            );

            // Buffer'larÄ± sÄ±fÄ±rla
            core::ptr::write_bytes(cmd_list_virt.as_ptr(), 0, 4096);
            core::ptr::write_bytes(cmd_table_virt.as_ptr(), 0, 4096);
            core::ptr::write_bytes(fis_virt.as_ptr(), 0, 4096);

            // Box wrapper'lar oluÅŸtur (virtual address kullanarak)
            let mut cmd_list: Box<[AhciCommandHeader; 32]> =
                Box::from_raw(cmd_list_virt.as_ptr() as *mut [AhciCommandHeader; 32]);
            let mut cmd_table: Box<AhciCommandTable> =
                Box::from_raw(cmd_table_virt.as_ptr() as *mut AhciCommandTable);
            let fis_recv: Box<[u8; 256]> = Box::from_raw(fis_virt.as_ptr() as *mut [u8; 256]);

            // Command List Base (physical address)
            core::ptr::write_volatile(&mut (*port).clb, cmd_list_phys as u32);
            core::ptr::write_volatile(&mut (*port).clbu, (cmd_list_phys >> 32) as u32);

            // FIS Base (physical address)
            core::ptr::write_volatile(&mut (*port).fb, fis_phys as u32);
            core::ptr::write_volatile(&mut (*port).fbu, (fis_phys >> 32) as u32);

            // Interrupt clear
            core::ptr::write_volatile(&mut (*port).is, 0xFFFFFFFF);
            core::ptr::write_volatile(&mut (*port).ie, 0);

            // Command Table adresini Command Header'a yaz (physical address)
            let cmd_header = cmd_list.as_mut_ptr();
            (*cmd_header).ctba = cmd_table_phys as u32;
            (*cmd_header).ctbau = (cmd_table_phys >> 32) as u32;
            (*cmd_header).dw0 = (5 << 0) | (1 << 16); // 5 PRD entries, 1 PRDTL

            // FIS Receive Enable + Start
            core::ptr::write_volatile(&mut (*port).cmd, CMD_FRE | CMD_ST);

            // Bekle
            for _ in 0..100000 {
                core::hint::spin_loop();
            }

            let signature = core::ptr::read_volatile(&(*port).sig);

            crate::serial_println!("[AHCI] Port {} initialized, sig={:#x}", port_idx, signature);

            Some(Self {
                port_idx,
                port_base: port,
                cmd_list_phys,
                cmd_table_phys,
                fis_phys,
                cmd_list,
                cmd_table,
                fis_recv,
                signature,
            })
        }
    }

    /// SektÃ¶r oku
    pub fn read_sector(&mut self, lba: u64, buffer: &mut [u8]) -> Result<(), AhciError> {
        if buffer.len() < BLOCK_SIZE {
            return Err(AhciError::IoError);
        }

        unsafe {
            let port = self.port_base;

            let phys_addr = crate::memory::try_virt_to_phys_u64(buffer.as_ptr() as u64)
                .ok_or(AhciError::IoError)?;

            // Command table hazÄ±rla
            let ct = &mut *self.cmd_table.as_mut();
            let cmd_header = &mut *self.cmd_list.as_mut_ptr();

            // PRDT ayarla
            ct.prdt[0].dba = phys_addr as u32;
            ct.prdt[0].dbau = (phys_addr >> 32) as u32;
            ct.prdt[0].dbc = (BLOCK_SIZE - 1) as u32; // 0-indexed

            // FIS hazÄ±rla
            let fis = &mut ct.cfis;
            fis[0] = FIS_TYPE_REG_H2D; // FIS type
            fis[1] = 0x80; // Command bit
            fis[2] = ATA_CMD_READ_DMA_EXT;
            fis[3] = 0; // feature

            // LBA (48-bit)
            fis[4] = lba as u8;
            fis[5] = (lba >> 8) as u8;
            fis[6] = (lba >> 16) as u8;
            fis[7] = 0xE0 | ((lba >> 24) as u8 & 0x0F); // device + LBA bits
            fis[8] = (lba >> 24) as u8;
            fis[9] = (lba >> 32) as u8;
            fis[10] = (lba >> 40) as u8;
            fis[11] = 0; // feature exp
            fis[12] = 1; // sector count low
            fis[13] = 0; // sector count high

            // Command header
            cmd_header.dw0 = (1 << 16) | 5; // 1 PRDT entry, 5 Dwords
            cmd_header.prdbc = 0;
            cmd_header.ctba = self.cmd_table_phys as u32;

            // Interrupt temizle
            core::ptr::write_volatile(&mut (*port).is, 0xFFFFFFFF);

            // Command issue
            core::ptr::write_volatile(&mut (*port).ci, 1);

            // Bekle (timeout ile)
            let mut timeout = 10000000u64;
            while timeout > 0 {
                let ci = core::ptr::read_volatile(&(*port).ci);
                if ci == 0 {
                    break;
                }
                timeout -= 1;
                core::hint::spin_loop();
            }

            if timeout == 0 {
                crate::serial_println!("[AHCI] Read timeout at LBA {}", lba);
                return Err(AhciError::Timeout);
            }

            // Task File Data kontrol et
            let tfd = core::ptr::read_volatile(&(*port).tfd);
            if tfd & 0x01 != 0 {
                crate::serial_println!("[AHCI] Read error TFD=0x{:x}", tfd);
                return Err(AhciError::CommandFailed);
            }

            Ok(())
        }
    }

    /// SektÃ¶r yaz
    pub fn write_sector(&mut self, lba: u64, buffer: &[u8]) -> Result<(), AhciError> {
        if buffer.len() < BLOCK_SIZE {
            return Err(AhciError::IoError);
        }

        unsafe {
            let port = self.port_base;

            let phys_addr = crate::memory::try_virt_to_phys_u64(buffer.as_ptr() as u64)
                .ok_or(AhciError::IoError)?;

            let ct = &mut *self.cmd_table.as_mut();
            let cmd_header = &mut *self.cmd_list.as_mut_ptr();

            ct.prdt[0].dba = phys_addr as u32;
            ct.prdt[0].dbau = (phys_addr >> 32) as u32;
            ct.prdt[0].dbc = (BLOCK_SIZE - 1) as u32;

            let fis = &mut ct.cfis;
            fis[0] = FIS_TYPE_REG_H2D;
            fis[1] = 0x80;
            fis[2] = ATA_CMD_WRITE_DMA_EXT;
            fis[3] = 0;
            fis[4] = lba as u8;
            fis[5] = (lba >> 8) as u8;
            fis[6] = (lba >> 16) as u8;
            fis[7] = 0xE0 | ((lba >> 24) as u8 & 0x0F);
            fis[8] = (lba >> 24) as u8;
            fis[9] = (lba >> 32) as u8;
            fis[10] = (lba >> 40) as u8;
            fis[11] = 0;
            fis[12] = 1;
            fis[13] = 0;

            cmd_header.dw0 = (1 << 16) | 5 | (1 << 6); // Write bit
            cmd_header.prdbc = 0;

            core::ptr::write_volatile(&mut (*port).is, 0xFFFFFFFF);
            core::ptr::write_volatile(&mut (*port).ci, 1);

            let mut timeout = 10000000u64;
            while timeout > 0 {
                let ci = core::ptr::read_volatile(&(*port).ci);
                if ci == 0 {
                    break;
                }
                timeout -= 1;
                core::hint::spin_loop();
            }

            if timeout == 0 {
                return Err(AhciError::Timeout);
            }

            let tfd = core::ptr::read_volatile(&(*port).tfd);
            if tfd & 0x01 != 0 {
                return Err(AhciError::CommandFailed);
            }

            Ok(())
        }
    }

    /// IDENTIFY DEVICE komutu gÃ¶nder (ATA/ACS T13 spec, PIO Data-In)
    pub fn identify_device(&mut self) -> Result<IdentifyDeviceData, AhciError> {
        unsafe {
            let port = self.port_base;

            let (buf_phys, buf_virt) = crate::memory::dma_alloc(1).ok_or(AhciError::IoError)?;
            let buf_phys = buf_phys as u64;
            core::ptr::write_bytes(buf_virt.as_ptr(), 0, 4096);

            let ct = &mut *self.cmd_table.as_mut();
            let cmd_header = &mut *self.cmd_list.as_mut_ptr();

            ct.prdt[0].dba = buf_phys as u32;
            ct.prdt[0].dbau = (buf_phys >> 32) as u32;
            ct.prdt[0].dbc = (512 - 1) as u32;

            let fis = &mut ct.cfis;
            fis[0] = FIS_TYPE_REG_H2D;
            fis[1] = 0x80;
            fis[2] = ATA_CMD_IDENTIFY;
            fis[3] = 0;
            fis[4] = 0;
            fis[5] = 0;
            fis[6] = 0;
            fis[7] = 0;
            fis[8] = 0;
            fis[9] = 0;
            fis[10] = 0;
            fis[11] = 0;
            fis[12] = 0;
            fis[13] = 0;

            cmd_header.dw0 = (1 << 16) | 5;
            cmd_header.prdbc = 0;

            core::ptr::write_volatile(&mut (*port).is, 0xFFFFFFFF);
            core::ptr::write_volatile(&mut (*port).ci, 1);

            let mut timeout = 10000000u64;
            while timeout > 0 {
                let ci = core::ptr::read_volatile(&(*port).ci);
                if ci == 0 {
                    break;
                }
                timeout -= 1;
                core::hint::spin_loop();
            }

            if timeout == 0 {
                return Err(AhciError::Timeout);
            }

            let tfd = core::ptr::read_volatile(&(*port).tfd);
            if tfd & 0x01 != 0 {
                return Err(AhciError::CommandFailed);
            }

            let data = core::ptr::read(buf_virt.as_ptr() as *const IdentifyDeviceData);
            Ok(data)
        }
    }
}

// ============================================================================
// BLOCK DEVICE TRAIT
// ============================================================================

/// AHCI Block Device wrapper
pub struct AhciBlockDevice {
    port: Mutex<AhciPortDevice>,
    block_count: u64,
    device_name: String,
}

impl AhciBlockDevice {
    pub fn new() -> Option<Self> {
        let controller = AhciController::find()?;
        let mut port = controller.find_active_port()?;
        let identify = port.identify_device().ok();
        let count = identify
            .as_ref()
            .map(|id| {
                let lba48 = id.lba48_sectors();
                if lba48 > 0 {
                    lba48
                } else {
                    id.lba28_sectors() as u64
                }
            })
            .unwrap_or(1024 * 1024);

        let name = if let Some(ref id) = identify {
            let model = id.model_string();
            let trimmed = core::str::from_utf8(&model)
                .unwrap_or("Unknown")
                .trim();
            if trimmed.is_empty() {
                "sda".to_string()
            } else {
                format!("sda:{}", trimmed)
            }
        } else {
            "sda".to_string()
        };

        if let Some(id) = identify {
            crate::serial_println!(
                "[AHCI] IDENTIFY: LBA48={} sectors ({} MB)",
                id.lba48_sectors(),
                id.lba48_sectors() * 512 / (1024 * 1024)
            );
        }

        Some(Self {
            port: Mutex::new(port),
            block_count: count,
            device_name: name,
        })
    }
}

impl BlockDevice for AhciBlockDevice {
    fn read_block(&mut self, lba: u64, buffer: &mut [u8]) -> Result<(), BlockDeviceError> {
        let mut port = self.port.lock();
        port.read_sector(lba, buffer)
            .map_err(|_| BlockDeviceError::IoError)
    }

    fn write_block(&mut self, lba: u64, buffer: &[u8]) -> Result<(), BlockDeviceError> {
        let mut port = self.port.lock();
        port.write_sector(lba, buffer)
            .map_err(|_| BlockDeviceError::IoError)
    }

    fn block_size(&self) -> u32 {
        BLOCK_SIZE as u32
    }

    fn block_count(&self) -> u64 {
        self.block_count
    }

    fn device_name(&self) -> String {
        self.device_name.clone()
    }

    fn device_type(&self) -> BlockDeviceType {
        BlockDeviceType::Hdd
    }
}

// ============================================================================
// GLOBAL AHCI MANAGER
// ============================================================================

lazy_static! {
    static ref AHCI_CONTROLLER: Mutex<Option<AhciController>> = Mutex::new(None);
    static ref AHCI_BLOCK_DEVICE: Mutex<Option<AhciBlockDevice>> = Mutex::new(None);
}

/// AHCI'yi baÅŸlat
pub fn init() -> bool {
    crate::serial_println!("[AHCI] Initializing AHCI subsystem...");

    match AhciController::find() {
        Some(ctrl) => {
            // Aktif port bul
            if let Some(port) = ctrl.find_active_port() {
                if let Some(block_dev) = AhciBlockDevice::new() {
                    *AHCI_BLOCK_DEVICE.lock() = Some(block_dev);
                    crate::serial_println!("[AHCI] AHCI block device ready");
                    return true;
                }
            }
            *AHCI_CONTROLLER.lock() = Some(ctrl);
        }
        None => {
            crate::serial_println!("[AHCI] No AHCI controller found");
        }
    }
    false
}

/// AHCI block device al
pub fn get_block_device() -> Option<AhciBlockDevice> {
    let guard = AHCI_BLOCK_DEVICE.lock();
    if guard.is_some() {
        // Clone yapamayÄ±z, bu yÃ¼zden init'i tekrar ÃSection aÄŸÄ±r
        // GerÃSection ek implementasyonda Arc kullanÄ±lmalÄ±
        drop(guard);
        return AhciBlockDevice::new();
    }
    None
}

// ============================================================================
// Test Corpus (Intel AHCI 1.3.1 + INCITS/T10 SPC/SBC)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ahci_block_size_is_512() {
        assert_eq!(BLOCK_SIZE, 512);
    }

    #[test]
    fn ahci_ata_command_codes() {
        // INCITS/T10 ATA/ATAPI spec
        assert_eq!(ATA_CMD_IDENTIFY, 0xEC);
        assert_eq!(ATA_CMD_READ_DMA_EXT, 0x25);
        assert_eq!(ATA_CMD_WRITE_DMA_EXT, 0x35);
        assert_eq!(ATA_CMD_FLUSH_CACHE, 0xE7);
    }

    #[test]
    fn ahci_ghc_bit_definitions() {
        // Intel AHCI 1.3.1 ÂSection 5.1.2
        assert_eq!(GHC_HR, 1 << 0);     // HBA Reset
        assert_eq!(GHC_IE, 1 << 1);     // Interrupt Enable
        assert_eq!(GHC_MRSM, 1 << 2);   // MSI Revert
        assert_eq!(GHC_AE, 1 << 31);    // AHCI Enable
    }

    #[test]
    fn ahci_cap_bit_definitions() {
        // Intel AHCI 1.3.1 ÂSection 5.1.1
        assert_eq!(CAP_S64A, 1 << 31);  // 64-bit Addressing
        assert_eq!(CAP_SNCQ, 1 << 30);  // Native Command Queuing
        assert_eq!(CAP_SSS, 1 << 27);   // Staggered Spin-up
        assert_eq!(CAP_SALP, 1 << 26);  // Aggressive Link Power Mgmt
        assert_eq!(CAP_SCLO, 1 << 24);  // Command List Override
    }

    #[test]
    fn ahci_port_cmd_bit_definitions() {
        // Intel AHCI 1.3.1 ÂSection 5.3.16
        assert_eq!(CMD_ST, 1 << 0);     // Start
        assert_eq!(CMD_SUD, 1 << 1);    // Spin-Up Device
        assert_eq!(CMD_POD, 1 << 2);    // Power On Device
        assert_eq!(CMD_CLO, 1 << 3);    // Command List Override
        assert_eq!(CMD_FRE, 1 << 4);    // FIS Receive Enable
        assert_eq!(CMD_FR, 1 << 14);    // FIS Receive Running
        assert_eq!(CMD_CR, 1 << 15);    // Command List Running
    }

    #[test]
    fn ahci_sata_status_device_present() {
        // Intel AHCI 1.3.1 ÂSection 5.3.26
        assert_eq!(SSTS_DET_PRESENT, 0x03);
    }

    #[test]
    fn ahci_fis_types() {
        // Intel AHCI 1.3.1 ÂSection 3.1
        assert_eq!(FIS_TYPE_REG_H2D, 0x27); // Register H2D
        assert_eq!(FIS_TYPE_REG_D2H, 0x34); // Register D2H
    }

    #[test]
    fn ahci_identify_atapi_detection() {
        // ATA/ATAPI spec: word 0 bit 15 = 1 means ATAPI, 0 means ATA
        let mut id = IdentifyDeviceData { words: [0; 256] };
        assert!(!id.is_atapi()); // ATA disk

        id.words[0] = 1 << 15;
        assert!(id.is_atapi()); // ATAPI device
    }

    #[test]
    fn ahci_identify_lba28_sectors() {
        let mut id = IdentifyDeviceData { words: [0; 256] };
        // LBA28: word 60 = low 16 bits, word 61 = high 16 bits
        // 1 GB disk = 2097152 sectors
        id.words[60] = 0x0000;
        id.words[61] = 0x0020;
        assert_eq!(id.lba28_sectors(), 0x0020_0000); // 2,097,152 sectors
    }

    #[test]
    fn ahci_identify_lba48_sectors() {
        let mut id = IdentifyDeviceData { words: [0; 256] };
        // LBA48: words 100-103 (little-endian)
        // 1 TB disk = 1,953,525,168 sectors
        let sectors: u64 = 1_953_525_168;
        id.words[100] = (sectors & 0xFFFF) as u16;
        id.words[101] = ((sectors >> 16) & 0xFFFF) as u16;
        id.words[102] = ((sectors >> 32) & 0xFFFF) as u16;
        id.words[103] = ((sectors >> 48) & 0xFFFF) as u16;
        assert_eq!(id.lba48_sectors(), sectors);
    }

    #[test]
    fn ahci_identify_model_string_byte_swap() {
        let mut id = IdentifyDeviceData { words: [0; 256] };
        // Model string is at words 27-46, each word is byte-swapped
        // "Test" â†’ 'T' (0x54), 'e' (0x65), 's' (0x73), 't' (0x74)
        id.words[27] = ((b'T' as u16) << 8) | (b'e' as u16);
        id.words[28] = ((b's' as u16) << 8) | (b't' as u16);
        let model = id.model_string();
        assert_eq!(model[0], b'T');
        assert_eq!(model[1], b'e');
        assert_eq!(model[2], b's');
        assert_eq!(model[3], b't');
    }

    #[test]
    fn ahci_port_register_offsets() {
        // Intel AHCI 1.3.1 ÂSection 5.3, each port is 0x80 bytes
        assert_eq!(PORT_CLB, 0x00);
        assert_eq!(PORT_IS, 0x10);
        assert_eq!(PORT_CMD, 0x18);
        assert_eq!(PORT_TFD, 0x20);
        assert_eq!(PORT_SIG, 0x24);
        assert_eq!(PORT_SSTS, 0x28);
        assert_eq!(PORT_SCTL, 0x2C);
        assert_eq!(PORT_SERR, 0x30);
        assert_eq!(PORT_SACT, 0x34);
        assert_eq!(PORT_CI, 0x38);
    }

    #[test]
    fn ahci_hba_global_register_offsets() {
        // Intel AHCI 1.3.1 ÂSection 5.1
        assert_eq!(AHCI_GHC, 0x04);
        assert_eq!(AHCI_IS, 0x08);
        assert_eq!(AHCI_PI, 0x0C);
        assert_eq!(AHCI_VS, 0x10);
        assert_eq!(AHCI_CAP2, 0x24);
        assert_eq!(AHCI_BOHC, 0x28);
    }
}
