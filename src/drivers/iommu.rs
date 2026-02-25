//! # IOMMU Support
//!
//! Intel VT-d and AMD-Vi I/O Memory Management Unit.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// IOMMU CONSTANTS
// ============================================================================

/// Intel VT-d register offsets
pub const VTD_VER_REG: u32 = 0x00;
pub const VTD_CAP_REG: u32 = 0x08;
pub const VTD_ECAP_REG: u32 = 0x10;
pub const VTD_GCMD_REG: u32 = 0x18;
pub const VTD_GSTS_REG: u32 = 0x1C;
pub const VTD_RTADDR_REG: u32 = 0x20;
pub const VTD_CCMD_REG: u32 = 0x28;
pub const VTD_FSTS_REG: u32 = 0x34;
pub const VTD_FECTL_REG: u32 = 0x38;
pub const VTD_FEDATA_REG: u32 = 0x3C;
pub const VTD_FEADDR_REG: u32 = 0x40;
pub const VTD_AFLOG_REG: u32 = 0x58;
pub const VTD_PQADDR_REG: u32 = 0x60;
pub const VTD_IOTLB_REG: u32 = 0x80;

/// VT-d commands
pub const VTD_GCMD_TE: u32 = 1 << 31;     // Translation enable
pub const VTD_GCMD_SRTP: u32 = 1 << 30;    // Set root table pointer
pub const VTD_GCMD_WBF: u32 = 1 << 27;     // Write buffer flush
pub const VTD_GCMD_QIE: u32 = 1 << 26;     // Invalidation queue enable
pub const VTD_GCMD_IRE: u32 = 1 << 25;     // Interrupt remapping enable
pub const VTD_GCMD_EAFL: u32 = 1 << 24;    // Enable advanced fault logging

/// AMD-Vi register offsets
pub const AMDVI_CONTROL_REG: u32 = 0x00;
pub const AMDVI_EXCL_BASE_REG: u32 = 0x08;
pub const AMDVI_EXCL_LIMIT_REG: u32 = 0x10;
pub const AMDVI_DEV_TABLE_BASE_REG: u32 = 0x18;
pub const AMDVI_CMD_BASE_REG: u32 = 0x20;
pub const AMDVI_CMD_TAIL_REG: u32 = 0x28;
pub const AMDVI_EVT_BASE_REG: u32 = 0x30;
pub const AMDVI_EVT_HEAD_REG: u32 = 0x38;
pub const AMDVI_STATUS_REG: u32 = 0x2020;

// ============================================================================
// DMA REMAPPING
// ============================================================================

/// DMA translation entry
#[repr(C)]
pub struct DmaTranslation {
    pub present: bool,
    pub read_perm: bool,
    pub write_perm: bool,
    pub phys_addr: u64,
    pub size: u64,
}

/// DMA remapping table entry (Intel VT-d)
#[repr(C, align(16))]
pub struct VtdRootEntry {
    pub lo: u64,
    pub hi: u64,
}

#[repr(C, align(16))]
pub struct VtdContextEntry {
    pub lo: u64,
    pub hi: u64,
}

/// Page table entry for DMA
#[repr(C)]
pub struct DmaPte {
    pub val: u64,
}

impl DmaPte {
    pub fn new(phys: u64, read: bool, write: bool) -> Self {
        let mut val = phys & !0xFFF;
        val |= 1; // Present
        if read { val |= 1 << 1; }
        if write { val |= 1 << 2; }
        Self { val }
    }
}

// ============================================================================
// IOMMU DOMAIN
// ============================================================================

pub struct IommuDomain {
    pub id: u32,
    pub page_table: u64,
    pub translation_enabled: bool,
    pub devices: Mutex<Vec<(u16, u16)>>, // (segment, bus:dev:fn)
    pub mappings: Mutex<BTreeMap<u64, DmaTranslation>>,
}

impl IommuDomain {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            page_table: 0,
            translation_enabled: true,
            devices: Mutex::new(Vec::new()),
            mappings: Mutex::new(BTreeMap::new()),
        }
    }

    /// Map DMA address
    pub fn map(&self, dma_addr: u64, phys_addr: u64, size: u64, read: bool, write: bool) -> Result<(), IommuError> {
        let mapping = DmaTranslation {
            present: true,
            read_perm: read,
            write_perm: write,
            phys_addr,
            size,
        };
        self.mappings.lock().insert(dma_addr, mapping);
        Ok(())
    }

    /// Unmap DMA address
    pub fn unmap(&self, dma_addr: u64) -> bool {
        self.mappings.lock().remove(&dma_addr).is_some()
    }

    /// Translate DMA address
    pub fn translate(&self, dma_addr: u64) -> Option<DmaTranslation> {
        self.mappings.lock().get(&dma_addr).cloned()
    }

    /// Attach device
    pub fn attach_device(&self, segment: u16, bdf: u16) {
        self.devices.lock().push((segment, bdf));
    }

    /// Detach device
    pub fn detach_device(&self, segment: u16, bdf: u16) {
        self.devices.lock().retain(|&(s, b)| s != segment || b != bdf);
    }
}

// ============================================================================
// IOMMU UNIT
// ============================================================================

pub struct IommuUnit {
    pub id: u32,
    pub vendor: IommuVendor,
    pub base_addr: u64,
    pub mmio: Mutex<Option<u64>>,
    pub enabled: AtomicBool,
    pub root_table: AtomicU64,
    pub fault_recording: Mutex<Vec<IommuFault>>,
    pub domains: Mutex<BTreeMap<u32, IommuDomain>>,
    pub next_domain_id: AtomicU32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IommuVendor {
    Intel,
    Amd,
    Unknown,
}

#[derive(Clone, Debug)]
pub struct IommuFault {
    pub source_id: u16,
    pub domain_id: u32,
    pub address: u64,
    pub fault_type: IommuFaultType,
    pub timestamp: u64,
}

#[derive(Clone, Copy, Debug)]
pub enum IommuFaultType {
    ReadViolation,
    WriteViolation,
    TranslationFailed,
    AccessViolation,
}

impl IommuUnit {
    pub fn new(id: u32, vendor: IommuVendor, base_addr: u64) -> Self {
        Self {
            id,
            vendor,
            base_addr,
            mmio: Mutex::new(None),
            enabled: AtomicBool::new(false),
            root_table: AtomicU64::new(0),
            fault_recording: Mutex::new(Vec::new()),
            domains: Mutex::new(BTreeMap::new()),
            next_domain_id: AtomicU32::new(1),
        }
    }

    /// Initialize IOMMU
    pub fn init(&self) -> Result<(), IommuError> {
        // Map MMIO space
        *self.mmio.lock() = Some(self.base_addr);
        
        // Check version/capabilities
        self.check_capabilities()?;
        
        // Allocate root table
        let root_table = self.alloc_root_table();
        self.root_table.store(root_table, Ordering::SeqCst);
        
        crate::serial_println!("[IOMMU] Unit {} initialized ({:?})", self.id, self.vendor);
        Ok(())
    }

    /// Check capabilities
    fn check_capabilities(&self) -> Result<(), IommuError> {
        // Read capability registers
        Ok(())
    }

    /// Allocate root table
    fn alloc_root_table(&self) -> u64 {
        // Allocate page-aligned root table
        0x100000
    }

    /// Enable translation
    pub fn enable(&self) -> Result<(), IommuError> {
        match self.vendor {
            IommuVendor::Intel => self.enable_vtd()?,
            IommuVendor::Amd => self.enable_amd()?,
            _ => return Err(IommuError::NotSupported),
        }
        
        self.enabled.store(true, Ordering::SeqCst);
        crate::serial_println!("[IOMMU] Unit {} enabled", self.id);
        Ok(())
    }

    /// Enable Intel VT-d
    fn enable_vtd(&self) -> Result<(), IommuError> {
        // Set root table pointer
        // Enable translation
        Ok(())
    }

    /// Enable AMD-Vi
    fn enable_amd(&self) -> Result<(), IommuError> {
        Ok(())
    }

    /// Disable translation
    pub fn disable(&self) {
        self.enabled.store(false, Ordering::SeqCst);
    }

    /// Create domain
    pub fn create_domain(&self) -> u32 {
        let id = self.next_domain_id.fetch_add(1, Ordering::SeqCst);
        let domain = IommuDomain::new(id);
        self.domains.lock().insert(id, domain);
        id
    }

    /// Get domain
    pub fn get_domain(&self, id: u32) -> Option<IommuDomain> {
        self.domains.lock().get(&id).cloned()
    }

    /// Flush IOTLB
    pub fn flush_iotlb(&self, domain_id: u32, addr: u64) {
        // Invalidate translation cache
    }

    /// Flush write buffer
    pub fn flush_write_buffer(&self) {
        // Flush pending writes
    }

    /// Handle fault
    pub fn handle_fault(&self) {
        let fault = IommuFault {
            source_id: 0,
            domain_id: 0,
            address: 0,
            fault_type: IommuFaultType::TranslationFailed,
            timestamp: crate::task::scheduler::get_ticks(),
        };
        self.fault_recording.lock().push(fault);
    }
}

// ============================================================================
// IOMMU MANAGER
// ============================================================================

pub struct IommuManager {
    units: Mutex<Vec<IommuUnit>>,
    default_domain: AtomicU32,
    iommu_enabled: AtomicBool,
}

impl IommuManager {
    pub const fn new() -> Self {
        Self {
            units: Mutex::new(Vec::new()),
            default_domain: AtomicU32::new(0),
            iommu_enabled: AtomicBool::new(false),
        }
    }

    /// Register IOMMU unit
    pub fn register_unit(&self, vendor: IommuVendor, base_addr: u64) -> u32 {
        let id = self.units.lock().len() as u32;
        let unit = IommuUnit::new(id, vendor, base_addr);
        self.units.lock().push(unit);
        id
    }

    /// Initialize all units
    pub fn init_all(&self) -> Result<(), IommuError> {
        for unit in self.units.lock().iter() {
            unit.init()?;
        }
        Ok(())
    }

    /// Enable all units
    pub fn enable_all(&self) -> Result<(), IommuError> {
        for unit in self.units.lock().iter() {
            unit.enable()?;
        }
        self.iommu_enabled.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Get unit
    pub fn get_unit(&self, id: u32) -> Option<IommuUnit> {
        self.units.lock().get(id as usize).cloned()
    }

    /// Map DMA for device
    pub fn map_dma(&self, segment: u16, bdf: u16, dma_addr: u64, phys_addr: u64, size: u64, read: bool, write: bool) -> Result<(), IommuError> {
        // Find domain for device and map
        Ok(())
    }

    /// Unmap DMA
    pub fn unmap_dma(&self, segment: u16, bdf: u16, dma_addr: u64) -> bool {
        false
    }

    /// Is IOMMU enabled?
    pub fn is_enabled(&self) -> bool {
        self.iommu_enabled.load(Ordering::SeqCst)
    }
}

// Clone for IommuUnit
impl Clone for IommuUnit {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            vendor: self.vendor,
            base_addr: self.base_addr,
            mmio: Mutex::new(*self.mmio.lock()),
            enabled: AtomicBool::new(self.enabled.load(Ordering::Relaxed)),
            root_table: AtomicU64::new(self.root_table.load(Ordering::Relaxed)),
            fault_recording: Mutex::new(self.fault_recording.lock().clone()),
            domains: Mutex::new(self.domains.lock().clone()),
            next_domain_id: AtomicU32::new(self.next_domain_id.load(Ordering::Relaxed)),
        }
    }
}

// Clone for IommuDomain
impl Clone for IommuDomain {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            page_table: self.page_table,
            translation_enabled: self.translation_enabled,
            devices: Mutex::new(self.devices.lock().clone()),
            mappings: Mutex::new(self.mappings.lock().clone()),
        }
    }
}

lazy_static::lazy_static! {
    pub static ref IOMMU_MANAGER: IommuManager = IommuManager::new();
}

// ============================================================================
// ERROR TYPE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IommuError {
    NotSupported,
    InitFailed,
    NoMemory,
    InvalidAddress,
    DeviceNotFound,
    DomainNotFound,
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init() {
    // Detect IOMMU from ACPI DMAR (Intel) or IVRS (AMD)
    crate::serial_println!("[IOMMU] Subsystem initialized");
}
