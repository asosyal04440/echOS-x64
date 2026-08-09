use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

use crate::cpu::acpi::{get_dmar_units, DmarDeviceScope, DmarDrhd};
use crate::memory::{active_physical_offset, alloc_phys, free_phys, map_mmio, phys_to_virt};

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
pub const VTD_FEUADDR_REG: u32 = 0x44;
pub const VTD_AFLOG_REG: u32 = 0x58;
pub const VTD_IVA_REG: u32 = 0x60;
pub const VTD_IRTA_REG: u32 = 0xB8;

pub const VTD_CAP_RWBF: u64 = 1 << 4;
pub const VTD_CAP_AFL: u64 = 1 << 3;
pub const VTD_CAP_MGAW_MASK: u64 = 0x3F << 16;
pub const VTD_CAP_SAGAW_MASK: u64 = 0x1F << 8;

pub const VTD_ECAP_IR: u64 = 1 << 3;
pub const VTD_ECAP_EIM: u64 = 1 << 4;
pub const VTD_ECAP_DT: u64 = 1 << 2;

pub const VTD_GCMD_TE: u32 = 1 << 31;
pub const VTD_GCMD_SRTP: u32 = 1 << 30;
pub const VTD_GCMD_SFL: u32 = 1 << 29;
pub const VTD_GCMD_EAFL: u32 = 1 << 28;
pub const VTD_GCMD_WBF: u32 = 1 << 27;
pub const VTD_GCMD_IRE: u32 = 1 << 25;
pub const VTD_GCMD_SIRTP: u32 = 1 << 24;

pub const VTD_GSTS_TES: u32 = 1 << 31;
pub const VTD_GSTS_RTPS: u32 = 1 << 30;
pub const VTD_GSTS_FLS: u32 = 1 << 29;
pub const VTD_GSTS_AFLS: u32 = 1 << 28;
pub const VTD_GSTS_WBFS: u32 = 1 << 27;
pub const VTD_GSTS_IRES: u32 = 1 << 25;
pub const VTD_GSTS_IRTPS: u32 = 1 << 24;

pub const IRTE_SIZE: usize = 16;

pub const IRTE_P: u64 = 1 << 0;
pub const IRTE_FPD: u64 = 1 << 1;
pub const IRTE_DM: u64 = 1 << 2;
pub const IRTE_TM: u64 = 1 << 4;
pub const IRTE_RH: u64 = 1 << 6;

pub const SVT_NONE: u8 = 0;
pub const SVT_RID: u8 = 1;
pub const SVT_BUS: u8 = 2;

pub const DEFAULT_IRT_ENTRIES: usize = 256;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Irte {
    pub low: u64,
    pub high: u64,
}

impl Irte {
    pub fn new() -> Self {
        Self { low: 0, high: 0 }
    }

    pub fn set_present(&mut self, present: bool) {
        if present {
            self.low |= IRTE_P;
        } else {
            self.low &= !IRTE_P;
        }
    }

    pub fn set_vector(&mut self, vector: u8) {
        self.low = (self.low & !0xFF00) | ((vector as u64) << 8);
    }

    pub fn set_delivery_mode(&mut self, mode: u8) {
        self.low = (self.low & !(0x7 << 5)) | ((mode as u64) << 5);
    }

    pub fn set_trigger_mode(&mut self, level: bool) {
        if level {
            self.low |= IRTE_TM;
        } else {
            self.low &= !IRTE_TM;
        }
    }

    pub fn set_dest_id(&mut self, dest: u32) {
        self.high = (self.high & !0xFFFF) | (dest as u64);
    }

    pub fn set_source(&mut self, svt: u8, sid: u16, sq: u8) {
        let val = ((svt as u64) << 18) | ((sid as u64) << 32) | ((sq as u64) << 17);
        self.high = (self.high & !(0x3FFFF << 17)) | val;
    }

    pub fn set_ir_handle(&mut self, handle: u64) {
        self.high = (self.high & !0xFFFF0000) | (handle << 16);
    }
}

pub struct IntrRemapTable {
    virt_addr: *mut Irte,
    phys_addr: u64,
    size: usize,
    lock: spin::Mutex<()>,
}

unsafe impl Send for IntrRemapTable {}
unsafe impl Sync for IntrRemapTable {}

impl IntrRemapTable {
    pub fn new(size: usize) -> Result<Self, IrError> {
        let bytes = size * IRTE_SIZE;
        let phys = alloc_phys(bytes).ok_or(IrError::NoMemory)?;
        let virt = phys_to_virt(phys as usize) as *mut Irte;

        unsafe {
            core::ptr::write_bytes(virt, 0, size);
        }

        Ok(Self {
            virt_addr: virt,
            phys_addr: phys,
            size,
            lock: Mutex::new(()),
        })
    }

    pub fn phys_addr(&self) -> u64 {
        self.phys_addr
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn get_entry(&self, index: usize) -> Option<Irte> {
        if index >= self.size {
            return None;
        }
        unsafe { Some(core::ptr::read_volatile(self.virt_addr.add(index))) }
    }

    pub fn set_entry(&self, index: usize, entry: &Irte) -> Result<(), IrError> {
        if index >= self.size {
            return Err(IrError::InvalidIndex);
        }
        let _lock = self.lock.lock();
        unsafe {
            core::ptr::write_volatile(self.virt_addr.add(index), *entry);
            core::sync::atomic::fence(Ordering::SeqCst);
        }
        Ok(())
    }

    pub fn allocate_entry(&self) -> Option<usize> {
        let _lock = self.lock.lock();
        for i in 0..self.size {
            let entry = unsafe { core::ptr::read_volatile(self.virt_addr.add(i)) };
            if entry.low & IRTE_P == 0 {
                return Some(i);
            }
        }
        None
    }
}

impl Drop for IntrRemapTable {
    fn drop(&mut self) {
        free_phys(self.phys_addr, self.size * IRTE_SIZE);
    }
}

pub struct IntrRemapUnit {
    pub id: u32,
    pub base_addr: u64,
    mmio_virt: u64,
    pub cap: AtomicU64,
    pub ecap: AtomicU64,
    pub irt: Mutex<Option<Arc<IntrRemapTable>>>,
    pub enabled: AtomicBool,
    pub fault_queue: Mutex<Vec<FaultRecord>>,
    pub devices: Vec<DmarDeviceScope>,
    pub include_all: bool,
    pub segment: u16,
}

#[derive(Clone, Debug)]
pub struct FaultRecord {
    pub fault_reason: u8,
    pub source_id: u16,
    pub address: u64,
    pub timestamp: u64,
}

impl IntrRemapUnit {
    pub fn new(id: u32, base_addr: u64) -> Self {
        let mapped = map_mmio(base_addr, 0x100);
        let mmio_virt = if mapped.is_null() {
            active_physical_offset() + base_addr
        } else {
            mapped as u64
        };

        Self {
            id,
            base_addr,
            mmio_virt,
            cap: AtomicU64::new(0),
            ecap: AtomicU64::new(0),
            irt: Mutex::new(None),
            enabled: AtomicBool::new(false),
            fault_queue: Mutex::new(Vec::new()),
            devices: Vec::new(),
            include_all: false,
            segment: 0,
        }
    }

    pub fn init(&self) -> Result<(), IrError> {
        let cap = self.read_reg(VTD_CAP_REG);
        let ecap = self.read_reg(VTD_ECAP_REG);

        self.cap.store(cap, Ordering::SeqCst);
        self.ecap.store(ecap, Ordering::SeqCst);

        if ecap & VTD_ECAP_IR == 0 {
            return Err(IrError::NotSupported);
        }

        crate::serial_println!(
            "[IR] Unit {} cap={:#x} ecap={:#x} base={:#x}",
            self.id,
            cap,
            ecap,
            self.base_addr
        );

        Ok(())
    }

    pub fn create_irt(&self, size: usize) -> Result<Arc<IntrRemapTable>, IrError> {
        let irt = IntrRemapTable::new(size)?;
        let irt = Arc::new(irt);

        let addr = irt.phys_addr();
        let s = (size as u64).ilog2().saturating_sub(1) as u64;
        let irta = addr | s;

        self.write_reg(VTD_IRTA_REG, irta);

        *self.irt.lock() = Some(irt.clone());

        crate::serial_println!(
            "[IR] Unit {} IRT phys={:#x} entries={} irta={:#x}",
            self.id,
            addr,
            size,
            irta
        );

        Ok(irt)
    }

    pub fn enable(&self) -> Result<(), IrError> {
        self.write_reg(VTD_GCMD_REG, VTD_GCMD_SIRTP as u64);

        if !self.wait_status(VTD_GSTS_IRTPS, 1000) {
            return Err(IrError::Timeout);
        }

        self.write_reg(VTD_GCMD_REG, VTD_GCMD_IRE as u64);

        if !self.wait_status(VTD_GSTS_IRES, 1000) {
            return Err(IrError::Timeout);
        }

        self.enabled.store(true, Ordering::SeqCst);

        crate::serial_println!("[IR] Interrupt remapping enabled for unit {}", self.id);

        Ok(())
    }

    pub fn disable(&self) {
        self.write_reg(VTD_GCMD_REG, 0);
        self.enabled.store(false, Ordering::SeqCst);
    }

    pub fn validate_source(&self, bus: u8, device: u8, function: u8) -> bool {
        if self.include_all {
            return true;
        }
        if self.devices.is_empty() {
            return true;
        }
        self.devices
            .iter()
            .any(|d| d.bus == bus && d.device == device && d.function == function)
    }

    pub fn program_interrupt(
        &self,
        index: usize,
        vector: u8,
        dest: u32,
        trigger: bool,
        source_bus: u8,
        source_device: u8,
        source_function: u8,
    ) -> Result<(), IrError> {
        let irt = self.irt.lock();
        let table = irt.as_ref().ok_or(IrError::TableNotSet)?;

        let mut entry = Irte::new();
        entry.set_present(true);
        entry.set_vector(vector);
        entry.set_trigger_mode(trigger);
        entry.set_dest_id(dest);

        let sid =
            ((source_bus as u16) << 8) | ((source_device as u16) << 3) | (source_function as u16);
        entry.set_source(SVT_RID, sid, 0);

        table.set_entry(index, &entry)?;

        Ok(())
    }

    pub fn handle_fault(&self) -> Option<FaultRecord> {
        let fsts = self.read_reg(VTD_FSTS_REG) as u32;

        if fsts & 0x80000000 != 0 {
            let reason = ((fsts >> 1) & 0xFF) as u8;
            let source = ((fsts >> 9) & 0xFFFF) as u16;

            let record = FaultRecord {
                fault_reason: reason,
                source_id: source,
                address: 0,
                timestamp: crate::task::scheduler::get_ticks() as u64,
            };

            self.write_reg(VTD_FSTS_REG, 0xFFFFFFFF);

            self.fault_queue.lock().push(record.clone());

            return Some(record);
        }

        None
    }

    fn read_reg(&self, offset: u32) -> u64 {
        let addr = (self.mmio_virt + offset as u64) as *mut u64;
        unsafe { core::ptr::read_volatile(addr) }
    }

    fn write_reg(&self, offset: u32, value: u64) {
        let addr = (self.mmio_virt + offset as u64) as *mut u64;
        unsafe {
            core::ptr::write_volatile(addr, value);
        }
    }

    fn read_reg32(&self, offset: u32) -> u32 {
        let addr = (self.mmio_virt + offset as u64) as *mut u32;
        unsafe { core::ptr::read_volatile(addr) }
    }

    fn write_reg32(&self, offset: u32, value: u32) {
        let addr = (self.mmio_virt + offset as u64) as *mut u32;
        unsafe {
            core::ptr::write_volatile(addr, value);
        }
    }

    fn wait_status(&self, bit: u32, timeout: u32) -> bool {
        for _ in 0..timeout {
            let status = self.read_reg32(VTD_GSTS_REG);
            if status & bit != 0 {
                return true;
            }
            unsafe { core::arch::asm!("pause") };
        }
        false
    }
}

pub struct IntrRemapManager {
    pub units: Mutex<BTreeMap<u32, Arc<IntrRemapUnit>>>,
    pub next_index: AtomicU32,
    pub stats: Mutex<IrStats>,
}

#[derive(Clone, Debug, Default)]
pub struct IrStats {
    pub interrupts_mapped: u64,
    pub faults_handled: u64,
}

impl IntrRemapManager {
    pub fn new() -> Self {
        Self {
            units: Mutex::new(BTreeMap::new()),
            next_index: AtomicU32::new(0),
            stats: Mutex::new(IrStats::default()),
        }
    }

    pub fn register_unit(&self, id: u32, base_addr: u64) -> Result<Arc<IntrRemapUnit>, IrError> {
        let unit = Arc::new(IntrRemapUnit::new(id, base_addr));
        unit.init()?;

        let irt = unit.create_irt(DEFAULT_IRT_ENTRIES)?;
        let _ = irt;
        unit.enable()?;

        self.units.lock().insert(id, unit.clone());

        crate::serial_println!("[IR] Unit {} registered and enabled", id);

        Ok(unit)
    }

    pub fn register_unit_from_drhd(
        &self,
        drhd: &DmarDrhd,
        id: u32,
    ) -> Result<Arc<IntrRemapUnit>, IrError> {
        let unit = Arc::new(IntrRemapUnit {
            id,
            base_addr: drhd.register_base,
            mmio_virt: {
                let mapped = map_mmio(drhd.register_base, 0x100);
                if mapped.is_null() {
                    active_physical_offset() + drhd.register_base
                } else {
                    mapped as u64
                }
            },
            cap: AtomicU64::new(0),
            ecap: AtomicU64::new(0),
            irt: Mutex::new(None),
            enabled: AtomicBool::new(false),
            fault_queue: Mutex::new(Vec::new()),
            devices: drhd.devices.clone(),
            include_all: drhd.include_all,
            segment: drhd.segment,
        });

        unit.init()?;

        let irt = unit.create_irt(DEFAULT_IRT_ENTRIES)?;
        let _ = irt;
        unit.enable()?;

        self.units.lock().insert(id, unit.clone());

        crate::serial_println!(
            "[IR] DMAR unit {} seg={} base={:#x} include_all={} devices={}",
            id,
            drhd.segment,
            drhd.register_base,
            drhd.include_all,
            drhd.devices.len()
        );

        Ok(unit)
    }

    pub fn get_unit(&self, id: u32) -> Option<Arc<IntrRemapUnit>> {
        self.units.lock().get(&id).cloned()
    }

    pub fn allocate_index(&self) -> u32 {
        self.next_index.fetch_add(1, Ordering::SeqCst)
    }

    pub fn map_interrupt(
        &self,
        unit_id: u32,
        vector: u8,
        dest: u32,
        trigger: bool,
        source_bus: u8,
        source_device: u8,
        source_function: u8,
    ) -> Result<u32, IrError> {
        let unit = self.get_unit(unit_id).ok_or(IrError::UnitNotFound)?;

        if !unit.validate_source(source_bus, source_device, source_function) {
            return Err(IrError::SourceInvalid);
        }

        let index = self.allocate_index();
        unit.program_interrupt(
            index as usize,
            vector,
            dest,
            trigger,
            source_bus,
            source_device,
            source_function,
        )?;

        let mut stats = self.stats.lock();
        stats.interrupts_mapped += 1;

        Ok(index)
    }

    pub fn handle_faults(&self) {
        for unit in self.units.lock().values() {
            while let Some(_fault) = unit.handle_fault() {
                let mut stats = self.stats.lock();
                stats.faults_handled += 1;
            }
        }
    }

    pub fn get_stats(&self) -> IrStats {
        self.stats.lock().clone()
    }
}

lazy_static::lazy_static! {
    pub static ref INTR_REMAP: IntrRemapManager = IntrRemapManager::new();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrError {
    NotSupported,
    UnitNotFound,
    TableNotSet,
    InvalidIndex,
    TableFull,
    NoMemory,
    Timeout,
    SourceInvalid,
}

pub fn init() {
    crate::serial_println!("[IR] Interrupt remapping manager initialized");
}

pub fn init_from_acpi() -> bool {
    let units = get_dmar_units();
    if units.is_empty() {
        crate::serial_println!("[IR] No DMAR units found");
        return false;
    }

    let mut unit_id = 0u32;
    for drhd in &units {
        match INTR_REMAP.register_unit_from_drhd(drhd, unit_id) {
            Ok(_) => {
                unit_id += 1;
            }
            Err(e) => {
                crate::serial_println!("[IR] DMAR unit {} init failed: {:?}", unit_id, e);
            }
        }
    }

    let count = unit_id;
    if count > 0 {
        crate::serial_println!(
            "[IR] {} VT-d unit(s) initialized with interrupt remapping",
            count
        );
        true
    } else {
        crate::serial_println!("[IR] No VT-d units initialized");
        false
    }
}
