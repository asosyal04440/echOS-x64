//! # NVMe Driver
//!
//! Non-Volatile Memory Express (NVMe) storage driver

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;
use alloc::boxed::Box;
use spin::Mutex;
use core::mem;
use core::sync::atomic::{AtomicU16, AtomicU32, Ordering};

// ============================================================================
// NVMe CONSTANTS
// ============================================================================

/// NVMe PCI class codes
const PCI_CLASS_STORAGE: u8 = 0x01;
const PCI_SUBCLASS_NVME: u8 = 0x08;

/// Controller registers (memory-mapped)
const NVME_CAP: usize = 0x00;       // Controller Capabilities
const NVME_VS: usize = 0x08;        // Version
const NVME_INTMS: usize = 0x0C;     // Interrupt Mask Set
const NVME_INTMC: usize = 0x10;     // Interrupt Mask Clear
const NVME_CC: usize = 0x14;        // Controller Configuration
const NVME_CSTS: usize = 0x1C;      // Controller Status
const NVME_NSSR: usize = 0x20;      // NVM Subsystem Reset
const NVME_AQA: usize = 0x24;       // Admin Queue Attributes
const NVME_ASQ: usize = 0x28;       // Admin Submission Queue Base Address
const NVME_ACQ: usize = 0x30;       // Admin Completion Queue Base Address

/// Controller Capabilities bits
const CAP_MQES_SHIFT: u64 = 0;      // Max Queue Entries Supported
const CAP_CQR_SHIFT: u64 = 16;      // Contiguous Queues Required
const CAP_AMS_SHIFT: u64 = 17;      // Arbitration Mechanisms Supported
const CAP_TO_SHIFT: u64 = 24;       // Timeout
const CAP_DSTRD_SHIFT: u64 = 32;    // Doorbell Stride
const CAP_NSSRS_SHIFT: u64 = 33;    // NVM Subsystem Reset Supported
const CAP_CSS_SHIFT: u64 = 37;      // Command Sets Supported
const CAP_MPSMIN_SHIFT: u64 = 48;   // Memory Page Size Minimum
const CAP_MPSMAX_SHIFT: u64 = 52;   // Memory Page Size Maximum

/// Controller Configuration bits
const CC_EN: u32 = 0x00000001;      // Enable
const CC_CSS_SHIFT: u32 = 4;        // Command Set Selected
const CC_MPS_SHIFT: u32 = 7;         // Memory Page Size
const CC_AMS_SHIFT: u32 = 11;       // Arbitration Mechanism Selected
const CC_SHN_SHIFT: u32 = 14;       // Shutdown Notification
const CC_IOSQES_SHIFT: u32 = 16;    // I/O Submission Queue Entry Size
const CC_IOCQES_SHIFT: u32 = 20;    // I/O Completion Queue Entry Size

/// Controller Status bits
const CSTS_RDY: u32 = 0x00000001;   // Ready
const CSTS_CFS: u32 = 0x00000002;   // Controller Fatal Status
const CSTS_SHST_SHIFT: u32 = 2;     // Shutdown Status
const CSTS_NSSRO: u32 = 0x00000008; // NVM Subsystem Reset Occurred

/// NVMe opcodes
const OP_FLUSH: u8 = 0x00;
const OP_WRITE: u8 = 0x01;
const OP_READ: u8 = 0x02;
const OP_WRITE_UNCORRECTABLE: u8 = 0x04;
const OP_COMPARE: u8 = 0x05;
const OP_WRITE_ZEROES: u8 = 0x08;
const OP_DATASET_MANAGEMENT: u8 = 0x09;

/// Admin opcodes
const OP_ADMIN_DELETE_SQ: u8 = 0x00;
const OP_ADMIN_CREATE_SQ: u8 = 0x01;
const OP_ADMIN_GET_LOG_PAGE: u8 = 0x02;
const OP_ADMIN_DELETE_CQ: u8 = 0x04;
const OP_ADMIN_CREATE_CQ: u8 = 0x05;
const OP_ADMIN_IDENTIFY: u8 = 0x06;
const OP_ADMIN_SET_FEATURES: u8 = 0x09;
const OP_ADMIN_GET_FEATURES: u8 = 0x0A;
const OP_ADMIN_ASYNC_EVENT: u8 = 0x0C;

/// Queue sizes
const ADMIN_QUEUE_SIZE: u16 = 32;
const IO_QUEUE_SIZE: u16 = 256;

/// Page size (4KB)
const PAGE_SIZE: usize = 4096;

// ============================================================================
// NVMe ERROR
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NvmeError {
    NoController,
    ControllerError,
    Timeout,
    QueueFull,
    InvalidNamespace,
    DataTransferError,
    NotReady,
    FeatureNotSupported,
}

// ============================================================================
// NVMe CONTROLLER CAPABILITIES
// ============================================================================

#[derive(Clone, Copy, Debug)]
pub struct NvmeCapabilities {
    pub max_queue_entries: u16,
    pub contiguous_queues: bool,
    pub arbitration_mechanisms: u8,
    pub timeout_ms: u16,
    pub doorbell_stride: u16,
    pub nvm_subsystem_reset: bool,
    pub command_sets: u8,
    pub page_size_min: u8,
    pub page_size_max: u8,
}

impl NvmeCapabilities {
    pub fn parse(cap: u64) -> Self {
        NvmeCapabilities {
            max_queue_entries: ((cap >> CAP_MQES_SHIFT) & 0xFFFF) as u16 + 1,
            contiguous_queues: ((cap >> CAP_CQR_SHIFT) & 1) != 0,
            arbitration_mechanisms: ((cap >> CAP_AMS_SHIFT) & 0x7) as u8,
            timeout_ms: ((cap >> CAP_TO_SHIFT) & 0xFF) as u16 * 500,
            doorbell_stride: (4 << ((cap >> CAP_DSTRD_SHIFT) & 0xF)) as u16,
            nvm_subsystem_reset: ((cap >> CAP_NSSRS_SHIFT) & 1) != 0,
            command_sets: ((cap >> CAP_CSS_SHIFT) & 0xFF) as u8,
            page_size_min: ((cap >> CAP_MPSMIN_SHIFT) & 0xF) as u8,
            page_size_max: ((cap >> CAP_MPSMAX_SHIFT) & 0xF) as u8,
        }
    }
}

// ============================================================================
// NVMe IDENTIFY DATA
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NvmeIdentifyController {
    pub vid: u16,               // PCI Vendor ID
    pub ssvid: u16,             // PCI Subsystem Vendor ID
    pub serial: [u8; 20],       // Serial Number
    pub model: [u8; 40],        // Model Number
    pub firmware: [u8; 8],      // Firmware Revision
    pub rab: u8,                // Recommended Arbitration Burst
    pub ieee: [u8; 3],          // IEEE OUI Identifier
    pub cmic: u8,               // Controller Multi-Path I/O and Namespace Sharing Capabilities
    pub mdts: u8,               // Maximum Data Transfer Size
    pub cntlid: u16,            // Controller ID
    pub ver: u32,               // Version
    pub rtd3r: u32,             // RTD3 Resume Latency
    pub rtd3e: u32,             // RTD3 Entry Latency
    pub oaes: u32,              // Optional Async Events Supported
    pub ctratt: u32,            // Controller Attributes
    pub rrls: u16,              // Read Recovery Levels Supported
    pub cntrltype: u8,          // Controller Type
    pub fguid: [u8; 16],        // FRU GUID
    pub crdt1: u16,             // Command Retry Delay Time 1
    pub crdt2: u16,             // Command Retry Delay Time 2
    pub crdt3: u16,             // Command Retry Delay Time 3
    pub oacs: u16,              // Optional Admin Command Support
    pub acl: u8,                // Abort Command Limit
    pub aerl: u8,               // Async Event Request Limit
    pub frmw: u8,               // Firmware Updates
    pub lpa: u8,                // Log Page Attributes
    pub elpe: u8,               // Error Log Page Entries
    pub npss: u8,               // Number of Power States Support
    pub avscc: u8,              // Admin Vendor Specific Command Configuration
    pub apsta: u8,              // Autonomous Power State Transition Capabilities
    pub wctemp: u16,            // Warning Composite Temperature Threshold
    pub cctemp: u16,            // Critical Composite Temperature Threshold
    pub mtfa: u16,              // Maximum Time for Firmware Activation
    pub hmpre: u32,             // Host Memory Buffer Preferred Size
    pub hmmin: u32,             // Host Memory Buffer Minimum Size
    pub tnvmcap: [u8; 16],      // Total NVM Capacity
    pub unvmcap: [u8; 16],      // Unallocated NVM Capacity
    pub rpmbs: u32,             // Replay Protected Memory Block Support
    pub edstt: u16,             // Extended Device Self-test Time
    pub dsto: u8,               // Device Self-test Options
    pub fwug: u8,               // Firmware Update Granularity
    pub kas: u16,               // Keep Alive Support
    pub hctma: u16,             // Host Controlled Thermal Management Attributes
    pub mntmt: u16,             // Minimum Thermal Management Temperature
    pub mxtmt: u16,             // Maximum Thermal Management Temperature
    pub sanicap: u32,           // Sanitize Capabilities
    pub hmminds: u32,           // Host Memory Buffer Minimum Descriptor Entry Size
    pub hmmaxd: u16,            // Host Memory Buffer Maximum Descriptor Entries
    pub nsetidmax: u16,         // NVM Set Identifier Maximum
    pub endgidmax: u16,         // Endurance Group Identifier Maximum
    pub anatt: u8,              // ANA Transition Time
    pub anacap: u8,             // Asymmetric Namespace Access Capabilities
    pub anagrpmax: u32,         // ANA Group Identifier Maximum
    pub nanagrpid: u32,         // Number of ANA Group Identifiers
    pub sqes: u8,               // Submission Queue Entry Size
    pub cqes: u8,               // Completion Queue Entry Size
    pub maxcmd: u16,            // Maximum Outstanding Commands
    pub nn: u32,                // Number of Namespaces
    pub oncs: u16,              // Optional NVM Command Support
    pub fuses: u16,             // Fused Operation Support
    pub fna: u8,                // Format NVM Attributes
    pub vwc: u8,                // Volatile Write Cache
    pub awun: u16,              // Atomic Write Unit Normal
    pub awupf: u16,             // Atomic Write Unit Power Fail
    pub nvscc: u8,              // NVM Vendor Specific Command Configuration
    pub nwpc: u8,               // Namespace Write Protection Capabilities
    pub acwu: u16,              // Atomic Compare & Write Unit
    pub sgls: u32,              // SGL Support
    pub mnan: u32,              // Maximum Number of Allowed Namespaces
}

impl NvmeIdentifyController {
    pub fn get_serial(&self) -> String {
        String::from_utf8_lossy(&self.serial).trim().to_string()
    }

    pub fn get_model(&self) -> String {
        String::from_utf8_lossy(&self.model).trim().to_string()
    }

    pub fn get_firmware(&self) -> String {
        String::from_utf8_lossy(&self.firmware).trim().to_string()
    }

    pub fn get_max_submission_queue_entry_size(&self) -> u8 {
        1 << (self.sqes & 0xF)
    }

    pub fn get_max_completion_queue_entry_size(&self) -> u8 {
        1 << (self.cqes & 0xF)
    }

    pub fn get_namespace_count(&self) -> u32 {
        self.nn
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NvmeIdentifyNamespace {
    pub nsze: u64,              // Namespace Size
    pub ncap: u64,              // Namespace Capacity
    pub nuse: u64,              // Namespace Utilization
    pub nsfeat: u8,             // Namespace Features
    pub nlbaf: u8,              // Number of LBA Formats
    pub flbas: u8,              // Formatted LBA Size
    pub mc: u8,                 // Metadata Capabilities
    pub dpc: u8,                // End-to-end Data Protection Capabilities
    pub dps: u8,                // End-to-end Data Protection Type Settings
    pub nmic: u8,               // Namespace Multi-path I/O and Namespace Sharing Capabilities
    pub rescap: u8,             // Reservation Capabilities
    pub fpi: u8,                // Format Progress Indicator
    pub nsattr: u8,             // Namespace Attributes
    pub nvmsetid: u16,          // NVM Set Identifier
    pub endgid: u16,            // Endurance Group Identifier
    pub nguid: [u8; 16],        // Namespace Globally Unique Identifier
    pub eui64: [u8; 8],         // IEEE Extended Unique Identifier
    pub lbaf: [LbaFormat; 16],  // LBA Format Support
    pub vs: [u8; 3712],         // Vendor Specific
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LbaFormat {
    pub ms: u16,                // Metadata Size
    pub lbads: u8,              // LBA Data Size
    pub rp: u8,                 // Relative Performance
}

impl NvmeIdentifyNamespace {
    pub fn get_block_size(&self) -> u32 {
        let lbaf_index = (self.flbas & 0xF) as usize;
        if lbaf_index < self.lbaf.len() {
            1u32 << self.lbaf[lbaf_index].lbads
        } else {
            512
        }
    }

    pub fn get_block_count(&self) -> u64 {
        self.nsze
    }

    pub fn get_capacity_bytes(&self) -> u64 {
        self.get_block_count() * self.get_block_size() as u64
    }
}

// ============================================================================
// NVMe SUBMISSION QUEUE ENTRY
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NvmeCommand {
    pub opcode: u8,
    pub flags: u8,
    pub cid: u16,
    pub nsid: u32,
    pub cdw2: u32,
    pub cdw3: u32,
    pub mptr: u64,
    pub prp1: u64,
    pub prp2: u64,
    pub cdw10: u32,
    pub cdw11: u32,
    pub cdw12: u32,
    pub cdw13: u32,
    pub cdw14: u32,
    pub cdw15: u32,
}

impl NvmeCommand {
    pub fn new(opcode: u8, cid: u16, nsid: u32) -> Self {
        NvmeCommand {
            opcode,
            flags: 0,
            cid,
            nsid,
            cdw2: 0,
            cdw3: 0,
            mptr: 0,
            prp1: 0,
            prp2: 0,
            cdw10: 0,
            cdw11: 0,
            cdw12: 0,
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }

    /// Create read command
    pub fn read(cid: u16, nsid: u32, lba: u64, blocks: u16) -> Self {
        let mut cmd = Self::new(OP_READ, cid, nsid);
        cmd.cdw10 = lba as u32;
        cmd.cdw11 = (lba >> 32) as u32;
        cmd.cdw12 = (blocks as u32) - 1; // 0-based count
        cmd
    }

    /// Create write command
    pub fn write(cid: u16, nsid: u32, lba: u64, blocks: u16) -> Self {
        let mut cmd = Self::new(OP_WRITE, cid, nsid);
        cmd.cdw10 = lba as u32;
        cmd.cdw11 = (lba >> 32) as u32;
        cmd.cdw12 = (blocks as u32) - 1; // 0-based count
        cmd
    }

    /// Create flush command
    pub fn flush(cid: u16, nsid: u32) -> Self {
        Self::new(OP_FLUSH, cid, nsid)
    }

    /// Create identify command
    pub fn identify(cid: u16, cns: u8, nsid: u32) -> Self {
        let mut cmd = Self::new(OP_ADMIN_IDENTIFY, cid, nsid);
        cmd.cdw10 = cns as u32;
        cmd
    }

    /// Set data buffer
    pub fn set_buffer(&mut self, addr: u64, len: usize) {
        self.prp1 = addr;
        // If buffer spans page boundary, set prp2
        let page_offset = addr & 0xFFF;
        if page_offset as usize + len > PAGE_SIZE {
            self.prp2 = (addr & !0xFFF) + PAGE_SIZE as u64;
        }
    }
}

// ============================================================================
// NVMe COMPLETION QUEUE ENTRY
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NvmeCompletion {
    pub cid: u16,
    pub p: u16,
    pub sqid: u16,
    pub status: u16,
    pub cdw0: u32,
    pub cdw1: u32,
}

impl NvmeCompletion {
    pub fn is_success(&self) -> bool {
        (self.status & 0xFFFE) == 0
    }

    pub fn get_status(&self) -> u8 {
        (self.status >> 1) as u8
    }

    pub fn get_phase(&self) -> bool {
        (self.p & 1) != 0
    }
}

// ============================================================================
// NVMe QUEUE
// ============================================================================

#[derive(Clone, Debug)]
pub struct NvmeQueue {
    pub sqid: u16,
    pub cqid: u16,
    pub size: u16,
    pub sq_tail: u16,
    pub sq_head: u16,
    pub cq_head: u16,
    pub cq_phase: bool,
    pub sq_addr: u64,
    pub cq_addr: u64,
    pub sq_db: u64,             // Submission Queue Doorbell
    pub cq_db: u64,             // Completion Queue Doorbell
}

impl NvmeQueue {
    pub fn new(sqid: u16, cqid: u16, size: u16, sq_addr: u64, cq_addr: u64, db_stride: u16) -> Self {
        NvmeQueue {
            sqid,
            cqid,
            size,
            sq_tail: 0,
            sq_head: 0,
            cq_head: 0,
            cq_phase: true,
            sq_addr,
            cq_addr,
            sq_db: 0x1000 + (sqid as u64 * 2 * db_stride as u64),
            cq_db: 0x1000 + (cqid as u64 * 2 + 1) * db_stride as u64,
        }
    }

    /// Submit command to queue
    pub fn submit(&mut self, cmd: &NvmeCommand) -> Result<(), NvmeError> {
        // Write command to submission queue
        // In real implementation, this would write to sq_addr
        self.sq_tail = (self.sq_tail + 1) % self.size;
        Ok(())
    }

    /// Check for completion
    pub fn poll_completion(&mut self) -> Option<NvmeCompletion> {
        // In real implementation, this would read from cq_addr
        None
    }
}

// ============================================================================
// NVMe CONTROLLER
// ============================================================================

/// NVMe Controller with full hardware support
#[derive(Clone, Debug)]
pub struct NvmeController {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub mmio_base: u64,
    pub capabilities: NvmeCapabilities,
    pub identify: Option<NvmeIdentifyController>,
    pub namespaces: BTreeMap<u32, NvmeIdentifyNamespace>,
    pub admin_queue: Option<NvmeQueue>,
    pub io_queues: Vec<NvmeQueue>,
    pub next_cid: u16,
    pub ready: bool,
    /// IRQ vector for this controller
    pub irq_vector: Option<u8>,
    /// Command timeout in ms
    pub timeout_ms: u16,
}

impl NvmeController {
    pub fn new(bus: u8, device: u8, function: u8) -> Self {
        NvmeController {
            bus,
            device,
            function,
            mmio_base: 0,
            capabilities: unsafe { mem::zeroed() },
            identify: None,
            namespaces: BTreeMap::new(),
            admin_queue: None,
            io_queues: Vec::new(),
            next_cid: 1,
            ready: false,
            irq_vector: None,
            timeout_ms: 5000,
        }
    }

    /// Read 32-bit MMIO register
    #[inline]
    unsafe fn read_mmio32(&self, offset: usize) -> u32 {
        let addr = (self.mmio_base + offset as u64) as *const u32;
        core::ptr::read_volatile(addr)
    }

    /// Write 32-bit MMIO register
    #[inline]
    unsafe fn write_mmio32(&self, offset: usize, value: u32) {
        let addr = (self.mmio_base + offset as u64) as *mut u32;
        core::ptr::write_volatile(addr, value);
    }

    /// Read 64-bit MMIO register
    #[inline]
    unsafe fn read_mmio64(&self, offset: usize) -> u64 {
        let addr = (self.mmio_base + offset as u64) as *const u64;
        core::ptr::read_volatile(addr)
    }

    /// Write 64-bit MMIO register
    #[inline]
    unsafe fn write_mmio64(&self, offset: usize, value: u64) {
        let addr = (self.mmio_base + offset as u64) as *mut u64;
        core::ptr::write_volatile(addr, value);
    }

    /// Initialize controller with full hardware setup
    pub fn init(&mut self) -> Result<(), NvmeError> {
        // Get MMIO base from BAR0
        let bar = crate::drivers::pci::read_bar_mmio(self.bus, self.device, self.function, 0)
            .ok_or(NvmeError::NoController)?;
        self.mmio_base = bar.base;

        // Map MMIO region if needed
        let mapped = crate::memory::map_mmio(bar.base, bar.size as usize);
        if !mapped.is_null() {
            self.mmio_base = mapped as u64;
        } else {
            self.mmio_base = crate::memory::active_physical_offset() + bar.base;
        }

        unsafe {
            // Read capabilities
            let cap = self.read_mmio64(NVME_CAP);
            self.capabilities = NvmeCapabilities::parse(cap);
            self.timeout_ms = self.capabilities.timeout_ms;

            crate::serial_println!("[NVMe] CAP: MQES={}, TO={}ms, DSTRD={}", 
                self.capabilities.max_queue_entries,
                self.capabilities.timeout_ms,
                self.capabilities.doorbell_stride);

            // Disable controller first
            self.write_mmio32(NVME_CC, 0);
            
            // Wait for controller to be disabled (CSTS.RDY = 0)
            let start = crate::task::scheduler::get_ticks();
            loop {
                let csts = self.read_mmio32(NVME_CSTS);
                if (csts & CSTS_RDY) == 0 {
                    break;
                }
                if crate::task::scheduler::get_ticks() - start > 1000 {
                    crate::serial_println!("[NVMe] Timeout waiting for disable");
                    break;
                }
            }

            // Allocate and setup admin queue
            self.setup_admin_queue()?;

            // Enable controller with NVM command set
            let cc = CC_EN 
                | (0 << CC_CSS_SHIFT)      // NVM command set
                | (0 << CC_MPS_SHIFT)      // 4KB page size (0 = 2^(12+0))
                | (0 << CC_AMS_SHIFT)      // Round robin arbitration
                | (6 << CC_IOSQES_SHIFT)   // 64-byte SQ entry size (2^6)
                | (4 << CC_IOCQES_SHIFT);  // 16-byte CQ entry size (2^4)
            
            self.write_mmio32(NVME_CC, cc);

            // Wait for controller ready
            let start = crate::task::scheduler::get_ticks();
            loop {
                let csts = self.read_mmio32(NVME_CSTS);
                if (csts & CSTS_RDY) != 0 {
                    break;
                }
                if (csts & CSTS_CFS) != 0 {
                    return Err(NvmeError::ControllerError);
                }
                if crate::task::scheduler::get_ticks() as u64 - start as u64 > self.timeout_ms as u64 / 10 {
                    return Err(NvmeError::Timeout);
                }
            }

            // Setup MSI interrupt
            self.setup_interrupts()?;

            // Identify controller
            self.identify_controller()?;

            // Discover namespaces
            self.discover_namespaces()?;
        }

        self.ready = true;
        crate::serial_println!("[NVMe] Controller initialized: {} namespaces", 
            self.namespaces.len());

        if let Some(ref id) = self.identify {
            crate::serial_println!("[NVMe] Model: {}", id.get_model());
            crate::serial_println!("[NVMe] Serial: {}", id.get_serial());
            crate::serial_println!("[NVMe] Firmware: {}", id.get_firmware());
        }

        Ok(())
    }

    /// Setup admin queue
    unsafe fn setup_admin_queue(&mut self) -> Result<(), NvmeError> {
        let sq_size = ADMIN_QUEUE_SIZE;
        let cq_size = ADMIN_QUEUE_SIZE;

        // Allocate aligned buffers for queues
        let sq_pages = (sq_size as usize * 64 + PAGE_SIZE - 1) / PAGE_SIZE;
        let cq_pages = (cq_size as usize * 16 + PAGE_SIZE - 1) / PAGE_SIZE;

        let sq_phys = crate::memory::alloc_phys(sq_pages * PAGE_SIZE)
            .ok_or(NvmeError::DataTransferError)?;
        let cq_phys = crate::memory::alloc_phys(cq_pages * PAGE_SIZE)
            .ok_or(NvmeError::DataTransferError)?;

        // Zero the queues
        let sq_virt = (crate::memory::active_physical_offset() + sq_phys) as *mut u8;
        let cq_virt = (crate::memory::active_physical_offset() + cq_phys) as *mut u8;
        core::ptr::write_bytes(sq_virt, 0, sq_pages * PAGE_SIZE);
        core::ptr::write_bytes(cq_virt, 0, cq_pages * PAGE_SIZE);

        // Configure admin queue attributes
        let aqa = ((sq_size - 1) as u32) | (((cq_size - 1) as u32) << 16);
        self.write_mmio32(NVME_AQA, aqa);

        // Set admin queue addresses
        self.write_mmio64(NVME_ASQ, sq_phys);
        self.write_mmio64(NVME_ACQ, cq_phys);

        // Create queue structure
        let db_stride = self.capabilities.doorbell_stride;
        self.admin_queue = Some(NvmeQueue::new(
            0,  // Admin SQ ID
            0,  // Admin CQ ID
            sq_size,
            sq_phys,
            cq_phys,
            db_stride,
        ));

        crate::serial_println!("[NVMe] Admin queue configured (size={})", sq_size);
        Ok(())
    }

    /// Setup MSI interrupts
    unsafe fn setup_interrupts(&mut self) -> Result<(), NvmeError> {
        // Allocate MSI vector
        let vector = crate::interrupts::allocate_msi_vector(nvme_irq_handler)
            .ok_or(NvmeError::FeatureNotSupported)?;
        
        self.irq_vector = Some(vector);

        // Configure MSI
        let apic_id = crate::cpu::smp::current_cpu_id() as u32;
        if !crate::drivers::pci::configure_pci_interrupt(
            self.bus, self.device, self.function,
            vector, apic_id
        ) {
            crate::serial_println!("[NVMe] MSI configuration failed, using polling");
            self.irq_vector = None;
        } else {
            crate::serial_println!("[NVMe] MSI configured (vector={})", vector);
        }

        Ok(())
    }

    /// Identify controller
    unsafe fn identify_controller(&mut self) -> Result<(), NvmeError> {
        // Allocate identify data buffer (4KB)
        let buffer_phys = crate::memory::alloc_phys(PAGE_SIZE)
            .ok_or(NvmeError::DataTransferError)?;
        let buffer_virt = (crate::memory::active_physical_offset() + buffer_phys) as *mut u8;
        core::ptr::write_bytes(buffer_virt, 0, PAGE_SIZE);

        // Create identify command (CNS = 1 for controller)
        let cid = self.get_cid();
        let mut cmd = NvmeCommand::identify(cid, 1, 0);
        cmd.set_buffer(buffer_phys, PAGE_SIZE);

        // Submit and wait
        self.submit_admin_command(&cmd)?;

        // Parse identify data
        let idata = &*(buffer_virt as *const NvmeIdentifyController);
        self.identify = Some(*idata);

        Ok(())
    }

    /// Discover namespaces
    unsafe fn discover_namespaces(&mut self) -> Result<(), NvmeError> {
        let nn = self.identify.map(|i| i.nn).unwrap_or(0);

        for nsid in 1..=nn {
            if let Ok(ns) = self.identify_namespace(nsid) {
                if ns.ncap > 0 {
                    self.namespaces.insert(nsid, ns);
                    crate::serial_println!("[NVMe] Namespace {}: {} blocks, {} bytes/block",
                        nsid, ns.get_block_count(), ns.get_block_size());
                }
            }
        }

        Ok(())
    }

    /// Identify a specific namespace
    unsafe fn identify_namespace(&mut self, nsid: u32) -> Result<NvmeIdentifyNamespace, NvmeError> {
        let buffer_phys = crate::memory::alloc_phys(PAGE_SIZE)
            .ok_or(NvmeError::DataTransferError)?;
        let buffer_virt = (crate::memory::active_physical_offset() + buffer_phys) as *mut u8;
        core::ptr::write_bytes(buffer_virt, 0, PAGE_SIZE);

        let cid = self.get_cid();
        let mut cmd = NvmeCommand::identify(cid, 0, nsid); // CNS = 0 for namespace
        cmd.set_buffer(buffer_phys, PAGE_SIZE);

        self.submit_admin_command(&cmd)?;

        let nsdata = *(buffer_virt as *const NvmeIdentifyNamespace);
        Ok(nsdata)
    }

    /// Submit admin command and wait for completion
    unsafe fn submit_admin_command(&mut self, cmd: &NvmeCommand) -> Result<NvmeCompletion, NvmeError> {
        let queue = self.admin_queue.as_mut().ok_or(NvmeError::NotReady)?;

        // Write command to submission queue
        let sq_addr = (crate::memory::active_physical_offset() + queue.sq_addr) as *mut NvmeCommand;
        let sq_entry = &mut *sq_addr.add(queue.sq_tail as usize);
        *sq_entry = *cmd;

        // Memory barrier
        core::sync::atomic::fence(Ordering::SeqCst);

        // Ring doorbell
        let db_addr = (self.mmio_base as usize + queue.sq_db as usize) as *mut u32;
        let new_tail = (queue.sq_tail + 1) % queue.size;
        core::ptr::write_volatile(db_addr, new_tail as u32);
        queue.sq_tail = new_tail;

        // Poll for completion
        let start = crate::task::scheduler::get_ticks();
        loop {
            let cq_addr = (crate::memory::active_physical_offset() + queue.cq_addr) as *const NvmeCompletion;
            let cq_entry = &*cq_addr.add(queue.cq_head as usize);

            // Check phase tag
            let phase = (cq_entry.p & 1) != 0;
            if phase == queue.cq_phase {
                // New entry
                let completion = *cq_entry;

                // Update head
                queue.cq_head = (queue.cq_head + 1) % queue.size;
                
                // Ring completion doorbell
                let cdb_addr = (self.mmio_base as usize + queue.cq_db as usize) as *mut u32;
                core::ptr::write_volatile(cdb_addr, queue.cq_head as u32);

                // Toggle phase on wrap
                if queue.cq_head == 0 {
                    queue.cq_phase = !queue.cq_phase;
                }

                if !completion.is_success() {
                    crate::serial_println!("[NVMe] Command failed: status={:#x}", completion.status);
                    return Err(NvmeError::ControllerError);
                }

                return Ok(completion);
            }

            if crate::task::scheduler::get_ticks() as u64 - start as u64 > self.timeout_ms as u64 / 10 {
                return Err(NvmeError::Timeout);
            }

            // Check for interrupt
            if let Some(vector) = self.irq_vector {
                crate::interrupts::kick_irq_worker();
            }
        }
    }

    /// Get next command ID
    pub fn get_cid(&mut self) -> u16 {
        let cid = self.next_cid;
        self.next_cid = self.next_cid.wrapping_add(1);
        if self.next_cid == 0 {
            self.next_cid = 1;
        }
        cid
    }

    /// Read from namespace
    pub fn read(&mut self, nsid: u32, lba: u64, blocks: u16, buffer: &mut [u8]) -> Result<(), NvmeError> {
        if !self.ready {
            return Err(NvmeError::NotReady);
        }

        // For now, use admin queue for I/O (should use I/O queue)
        let cid = self.get_cid();
        let mut cmd = NvmeCommand::read(cid, nsid, lba, blocks);
        
        // Allocate physical buffer and copy
        let buffer_phys = crate::memory::virt_to_phys_u64(buffer.as_ptr() as u64);
        cmd.set_buffer(buffer_phys, buffer.len());

        unsafe {
            self.submit_admin_command(&cmd)?;
        }

        Ok(())
    }

    /// Write to namespace
    pub fn write(&mut self, nsid: u32, lba: u64, blocks: u16, buffer: &[u8]) -> Result<(), NvmeError> {
        if !self.ready {
            return Err(NvmeError::NotReady);
        }

        let cid = self.get_cid();
        let mut cmd = NvmeCommand::write(cid, nsid, lba, blocks);
        
        let buffer_phys = crate::memory::virt_to_phys_u64(buffer.as_ptr() as u64);
        cmd.set_buffer(buffer_phys, buffer.len());

        unsafe {
            self.submit_admin_command(&cmd)?;
        }

        Ok(())
    }

    /// Flush namespace
    pub fn flush(&mut self, nsid: u32) -> Result<(), NvmeError> {
        if !self.ready {
            return Err(NvmeError::NotReady);
        }

        let cid = self.get_cid();
        let cmd = NvmeCommand::flush(cid, nsid);

        unsafe {
            self.submit_admin_command(&cmd)?;
        }

        Ok(())
    }

    /// Get namespace block size
    pub fn get_block_size(&self, nsid: u32) -> u32 {
        self.namespaces.get(&nsid).map(|ns| ns.get_block_size()).unwrap_or(512)
    }

    /// Get namespace block count
    pub fn get_block_count(&self, nsid: u32) -> u64 {
        self.namespaces.get(&nsid).map(|ns| ns.get_block_count()).unwrap_or(0)
    }

    /// Get namespace capacity in bytes
    pub fn get_capacity(&self, nsid: u32) -> u64 {
        self.namespaces.get(&nsid).map(|ns| ns.get_capacity_bytes()).unwrap_or(0)
    }
}

// ============================================================================
// NVMe MANAGER
// ============================================================================

lazy_static::lazy_static! {
    static ref NVME_CONTROLLERS: Mutex<Vec<NvmeController>> = Mutex::new(Vec::new());
}

/// Discover NVMe controllers via PCI
pub fn discover_nvme_controllers() -> Vec<NvmeController> {
    let mut controllers = Vec::new();
    
    let devices = crate::drivers::pci::scan();
    for dev in devices {
        if dev.class_code == PCI_CLASS_STORAGE && dev.subclass == PCI_SUBCLASS_NVME {
            controllers.push(NvmeController::new(dev.bus, dev.device, dev.function));
        }
    }
    
    controllers
}

/// Initialize NVMe subsystem
pub fn init() {
    crate::serial_println!("[NVMe] Initializing NVMe subsystem...");
    
    let controllers = discover_nvme_controllers();
    let mut nvme_ctrls = NVME_CONTROLLERS.lock();
    
    for mut ctrl in controllers {
        if ctrl.init().is_ok() {
            nvme_ctrls.push(ctrl);
        }
    }
    
    crate::serial_println!("[NVMe] Found {} controllers", nvme_ctrls.len());
}

/// Get default controller
pub fn default_controller() -> Option<NvmeController> {
    NVME_CONTROLLERS.lock().first().cloned()
}

/// Read from NVMe
pub fn read(nsid: u32, lba: u64, blocks: u16, buffer: &mut [u8]) -> Result<(), NvmeError> {
    let mut controllers = NVME_CONTROLLERS.lock();
    let ctrl = controllers.first_mut().ok_or(NvmeError::NoController)?;
    ctrl.read(nsid, lba, blocks, buffer)
}

/// Write to NVMe
pub fn write(nsid: u32, lba: u64, blocks: u16, buffer: &[u8]) -> Result<(), NvmeError> {
    let mut controllers = NVME_CONTROLLERS.lock();
    let ctrl = controllers.first_mut().ok_or(NvmeError::NoController)?;
    ctrl.write(nsid, lba, blocks, buffer)
}

/// Flush NVMe
pub fn flush(nsid: u32) -> Result<(), NvmeError> {
    let mut controllers = NVME_CONTROLLERS.lock();
    let ctrl = controllers.first_mut().ok_or(NvmeError::NoController)?;
    ctrl.flush(nsid)
}

/// Get namespace info
pub fn get_namespace_info(nsid: u32) -> Option<(u32, u64, u64)> {
    let controllers = NVME_CONTROLLERS.lock();
    let ctrl = controllers.first()?;
    let ns = ctrl.namespaces.get(&nsid)?;
    Some((ns.get_block_size(), ns.get_block_count(), ns.get_capacity_bytes()))
}

// ============================================================================
// IRQ HANDLER
// ============================================================================

/// NVMe interrupt handler
fn nvme_irq_handler(vector: u8) {
    crate::serial_println!("[NVMe] IRQ received on vector {}", vector);
    
    // Wake up any waiting tasks
    // In a full implementation, would signal completion to waiting threads
}

// ============================================================================
// I/O QUEUE SUPPORT
// ============================================================================

/// Create I/O submission and completion queues
pub fn create_io_queue(controller: &mut NvmeController, qid: u16, size: u16) -> Result<(), NvmeError> {
    if !controller.ready {
        return Err(NvmeError::NotReady);
    }

    unsafe {
        // Allocate queues
        let sq_pages = (size as usize * 64 + PAGE_SIZE - 1) / PAGE_SIZE;
        let cq_pages = (size as usize * 16 + PAGE_SIZE - 1) / PAGE_SIZE;

        let sq_phys = crate::memory::alloc_phys(sq_pages * PAGE_SIZE)
            .ok_or(NvmeError::DataTransferError)?;
        let cq_phys = crate::memory::alloc_phys(cq_pages * PAGE_SIZE)
            .ok_or(NvmeError::DataTransferError)?;

        // Zero queues
        let sq_virt = (crate::memory::active_physical_offset() + sq_phys) as *mut u8;
        let cq_virt = (crate::memory::active_physical_offset() + cq_phys) as *mut u8;
        core::ptr::write_bytes(sq_virt, 0, sq_pages * PAGE_SIZE);
        core::ptr::write_bytes(cq_virt, 0, cq_pages * PAGE_SIZE);

        // Create completion queue (CQID = qid)
        let mut cmd = NvmeCommand::new(OP_ADMIN_CREATE_CQ, controller.get_cid(), 0);
        cmd.prp1 = cq_phys;
        cmd.cdw10 = ((size - 1) as u32) | ((qid as u32) << 16); // QSIZE | QID
        cmd.cdw11 = 1; // Physically contiguous, no interrupts for now
        
        controller.submit_admin_command(&cmd)?;

        // Create submission queue (SQID = qid, CQID = qid)
        let mut cmd = NvmeCommand::new(OP_ADMIN_CREATE_SQ, controller.get_cid(), 0);
        cmd.prp1 = sq_phys;
        cmd.cdw10 = ((size - 1) as u32) | ((qid as u32) << 16); // QSIZE | QID
        cmd.cdw11 = 1 | ((qid as u32) << 16); // Physically contiguous | CQID
        
        controller.submit_admin_command(&cmd)?;

        // Add to controller's queue list
        let db_stride = controller.capabilities.doorbell_stride;
        controller.io_queues.push(NvmeQueue::new(
            qid,
            qid,
            size,
            sq_phys,
            cq_phys,
            db_stride,
        ));

        crate::serial_println!("[NVMe] I/O queue {} created (size={})", qid, size);
    }

    Ok(())
}

// ============================================================================
// BLOCK DEVICE INTERFACE
// ============================================================================

use crate::drivers::block::{BlockDevice, BlockDeviceError, BlockDeviceType};

/// NVMe block device wrapper
pub struct NvmeBlockDevice {
    pub controller_idx: usize,
    pub nsid: u32,
    pub block_size: u32,
    pub block_count: u64,
}

impl NvmeBlockDevice {
    pub fn new(controller_idx: usize, nsid: u32) -> Option<Self> {
        let controllers = NVME_CONTROLLERS.lock();
        let ctrl = controllers.get(controller_idx)?;
        let ns = ctrl.namespaces.get(&nsid)?;
        
        Some(NvmeBlockDevice {
            controller_idx,
            nsid,
            block_size: ns.get_block_size(),
            block_count: ns.get_block_count(),
        })
    }
}

impl BlockDevice for NvmeBlockDevice {
    fn read_block(&mut self, lba: u64, buffer: &mut [u8]) -> Result<(), BlockDeviceError> {
        let mut controllers = NVME_CONTROLLERS.lock();
        let ctrl = controllers.get_mut(self.controller_idx)
            .ok_or(BlockDeviceError::DeviceNotFound)?;
        
        let blocks = (buffer.len() / self.block_size as usize) as u16;
        ctrl.read(self.nsid, lba, blocks.max(1), buffer)
            .map_err(|_| BlockDeviceError::IoError)
    }

    fn write_block(&mut self, lba: u64, buffer: &[u8]) -> Result<(), BlockDeviceError> {
        let mut controllers = NVME_CONTROLLERS.lock();
        let ctrl = controllers.get_mut(self.controller_idx)
            .ok_or(BlockDeviceError::DeviceNotFound)?;
        
        let blocks = (buffer.len() / self.block_size as usize) as u16;
        ctrl.write(self.nsid, lba, blocks.max(1), buffer)
            .map_err(|_| BlockDeviceError::IoError)
    }

    fn flush(&mut self) -> Result<(), BlockDeviceError> {
        let mut controllers = NVME_CONTROLLERS.lock();
        let ctrl = controllers.get_mut(self.controller_idx)
            .ok_or(BlockDeviceError::DeviceNotFound)?;
        
        ctrl.flush(self.nsid)
            .map_err(|_| BlockDeviceError::IoError)
    }

    fn block_size(&self) -> u32 {
        self.block_size
    }

    fn block_count(&self) -> u64 {
        self.block_count
    }

    fn device_type(&self) -> BlockDeviceType {
        BlockDeviceType::Nvme
    }

    fn device_name(&self) -> alloc::string::String {
        alloc::format!("nvme{}n{}", self.controller_idx, self.nsid)
    }
}
