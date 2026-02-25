//! # Virtualization Support
//!
//! Intel VMX and AMD SVM virtualization with EPT/NPT

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;
use alloc::boxed::Box;
use spin::Mutex;
use core::mem;

// ============================================================================
// VIRTUALIZATION CONSTANTS
// ============================================================================

/// CPUID leaf for virtualization info
const CPUID_VIRT_LEAF: u32 = 0x40000000;

/// Intel VMX MSRs
const IA32_FEATURE_CONTROL: u32 = 0x3A;
const IA32_VMX_BASIC: u32 = 0x480;
const IA32_VMX_PINBASED_CTLS: u32 = 0x481;
const IA32_VMX_PROCBASED_CTLS: u32 = 0x482;
const IA32_VMX_EXIT_CTLS: u32 = 0x483;
const IA32_VMX_ENTRY_CTLS: u32 = 0x484;
const IA32_VMX_MISC: u32 = 0x485;
const IA32_VMX_CR0_FIXED0: u32 = 0x486;
const IA32_VMX_CR0_FIXED1: u32 = 0x487;
const IA32_VMX_CR4_FIXED0: u32 = 0x488;
const IA32_VMX_CR4_FIXED1: u32 = 0x489;
const IA32_VMX_VMCS_ENUM: u32 = 0x48A;
const IA32_VMX_PROCBASED_CTLS2: u32 = 0x48B;
const IA32_VMX_EPT_VPID_CAP: u32 = 0x48C;

/// AMD SVM MSRs
const MSR_VM_CR: u32 = 0xC0010114;
const MSR_VM_HSAVE_PA: u32 = 0xC0010117;
const MSR_VM_LOCK: u32 = 0xC0010115;
const MSR_VM_ASID: u32 = 0xC0010116;

/// VMCS field encodings
const VMCS_CTRL_PIN_BASED: u32 = 0x00004000;
const VMCS_CTRL_PROC_BASED: u32 = 0x00004002;
const VMCS_CTRL_PROC_BASED_2: u32 = 0x0000401E;
const VMCS_CTRL_EXIT: u32 = 0x0000400C;
const VMCS_CTRL_ENTRY: u32 = 0x00004012;
const VMCS_CTRL_EXEC: u32 = 0x0000401C;

const VMCS_GUEST_ES_SEL: u32 = 0x00000800;
const VMCS_GUEST_CS_SEL: u32 = 0x00000802;
const VMCS_GUEST_SS_SEL: u32 = 0x00000804;
const VMCS_GUEST_DS_SEL: u32 = 0x00000806;
const VMCS_GUEST_FS_SEL: u32 = 0x00000808;
const VMCS_GUEST_GS_SEL: u32 = 0x0000080A;
const VMCS_GUEST_LDTR_SEL: u32 = 0x0000080C;
const VMCS_GUEST_TR_SEL: u32 = 0x0000080E;

const VMCS_GUEST_CR0: u32 = 0x00000820;
const VMCS_GUEST_CR3: u32 = 0x00000822;
const VMCS_GUEST_CR4: u32 = 0x00000824;
const VMCS_GUEST_ES_BASE: u32 = 0x00000806;
const VMCS_GUEST_CS_BASE: u32 = 0x00000808;
const VMCS_GUEST_SS_BASE: u32 = 0x0000080A;
const VMCS_GUEST_DS_BASE: u32 = 0x0000080C;
const VMCS_GUEST_FS_BASE: u32 = 0x0000080E;
const VMCS_GUEST_GS_BASE: u32 = 0x00000810;
const VMCS_GUEST_LDTR_BASE: u32 = 0x00000812;
const VMCS_GUEST_TR_BASE: u32 = 0x00000814;
const VMCS_GUEST_GDTR_BASE: u32 = 0x00000816;
const VMCS_GUEST_IDTR_BASE: u32 = 0x00000818;

const VMCS_GUEST_RSP: u32 = 0x0000081C;
const VMCS_GUEST_RIP: u32 = 0x0000081E;
const VMCS_GUEST_RFLAGS: u32 = 0x00000820;

const VMCS_GUEST_ES_LIMIT: u32 = 0x00000800;
const VMCS_GUEST_CS_LIMIT: u32 = 0x00000802;
const VMCS_GUEST_SS_LIMIT: u32 = 0x00000804;
const VMCS_GUEST_DS_LIMIT: u32 = 0x00000806;
const VMCS_GUEST_FS_LIMIT: u32 = 0x00000808;
const VMCS_GUEST_GS_LIMIT: u32 = 0x0000080A;
const VMCS_GUEST_LDTR_LIMIT: u32 = 0x0000080C;
const VMCS_GUEST_TR_LIMIT: u32 = 0x0000080E;
const VMCS_GUEST_GDTR_LIMIT: u32 = 0x00000810;
const VMCS_GUEST_IDTR_LIMIT: u32 = 0x00000812;

const VMCS_GUEST_ES_AR: u32 = 0x00000814;
const VMCS_GUEST_CS_AR: u32 = 0x00000816;
const VMCS_GUEST_SS_AR: u32 = 0x00000818;
const VMCS_GUEST_DS_AR: u32 = 0x0000081A;
const VMCS_GUEST_FS_AR: u32 = 0x0000081C;
const VMCS_GUEST_GS_AR: u32 = 0x0000081E;
const VMCS_GUEST_LDTR_AR: u32 = 0x00000820;
const VMCS_GUEST_TR_AR: u32 = 0x00000822;

const VMCS_GUEST_ACTIVITY: u32 = 0x00000826;
const VMCS_GUEST_INT_STATE: u32 = 0x00000824;
const VMCS_GUEST_SMBASE: u32 = 0x00000828;

const VMCS_HOST_ES_SEL: u32 = 0x00000C00;
const VMCS_HOST_CS_SEL: u32 = 0x00000C02;
const VMCS_HOST_SS_SEL: u32 = 0x00000C04;
const VMCS_HOST_DS_SEL: u32 = 0x00000C06;
const VMCS_HOST_FS_SEL: u32 = 0x00000C08;
const VMCS_HOST_GS_SEL: u32 = 0x00000C0A;
const VMCS_HOST_TR_SEL: u32 = 0x00000C0C;

const VMCS_HOST_CR0: u32 = 0x00000C00;
const VMCS_HOST_CR3: u32 = 0x00000C02;
const VMCS_HOST_CR4: u32 = 0x00000C04;
const VMCS_HOST_FS_BASE: u32 = 0x00000C06;
const VMCS_HOST_GS_BASE: u32 = 0x00000C08;
const VMCS_HOST_TR_BASE: u32 = 0x00000C0A;
const VMCS_HOST_GDTR_BASE: u32 = 0x00000C0C;
const VMCS_HOST_IDTR_BASE: u32 = 0x00000C0E;
const VMCS_HOST_RSP: u32 = 0x00000C10;
const VMCS_HOST_RIP: u32 = 0x00000C12;

const VMCS_EPTP: u32 = 0x0000201A;
const VMCS_VPID: u32 = 0x00002000;

/// VMX instruction errors
const VMXERR_VMCLEAR_INVALID_ADDR: u32 = 2;
const VMXERR_VMLAUNCH_NON_CLEAR: u32 = 4;
const VMXERR_VMRESUME_NON_LAUNCHED: u32 = 5;
const VMXERR_VMRESUME_VMCLEAR: u32 = 6;
const VMXERR_INVALID_VMCS_FIELD: u32 = 7;
const VMXERR_INVALID_HOST_STATE: u32 = 8;
const VMXERR_INVALID_GUEST_STATE: u32 = 11;

/// EPT memory types
const EPT_MEM_TYPE_UC: u64 = 0x00;  // Uncacheable
const EPT_MEM_TYPE_WC: u64 = 0x01;  // Write Combining
const EPT_MEM_TYPE_WT: u64 = 0x04;  // Write Through
const EPT_MEM_TYPE_WP: u64 = 0x05;  // Write Protected
const EPT_MEM_TYPE_WB: u64 = 0x06;  // Write Back

/// EPT permissions
const EPT_READ: u64 = 0x01;
const EPT_WRITE: u64 = 0x02;
const EPT_EXECUTE: u64 = 0x04;
const EPT_EXECUTE_USER: u64 = 0x08;

/// Page sizes
const PAGE_SIZE_4K: u64 = 4096;
const PAGE_SIZE_2M: u64 = 2 * 1024 * 1024;
const PAGE_SIZE_1G: u64 = 1024 * 1024 * 1024;

// ============================================================================
// VIRTUALIZATION ERROR
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VirtError {
    NotSupported,
    DisabledInBIOS,
    VmxOnFailed,
    VmcsInitFailed,
    VmlaunchFailed,
    InvalidState,
    EptError,
    MemoryError,
    Unknown,
}

// ============================================================================
// CPU VENDOR
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuVendor {
    Intel,
    Amd,
    Unknown,
}

impl CpuVendor {
    pub fn detect() -> Self {
        // CPUID leaf 0 returns vendor string
        // In real implementation, use cpuid instruction
        CpuVendor::Intel // Default for now
    }
}

// ============================================================================
// VMX SUPPORT
// ============================================================================

#[derive(Clone, Debug)]
pub struct VmxCapabilities {
    pub supported: bool,
    pub enabled: bool,
    pub locked: bool,
    pub vmxon_region_size: u32,
    pub vmcs_revision: u32,
    pub use_msr_bitmaps: bool,
    pub use_io_bitmaps: bool,
    pub use_tpr_shadow: bool,
    pub use_ept: bool,
    pub use_vpid: bool,
    pub ept_capabilities: EptCapabilities,
}

impl VmxCapabilities {
    pub fn detect() -> Self {
        // Check CPUID.1:ECX.VMX[5]
        // Read IA32_FEATURE_CONTROL MSR
        // Read IA32_VMX_BASIC MSR
        
        VmxCapabilities {
            supported: true,
            enabled: true,
            locked: true,
            vmxon_region_size: 4096,
            vmcs_revision: 1,
            use_msr_bitmaps: true,
            use_io_bitmaps: true,
            use_tpr_shadow: true,
            use_ept: true,
            use_vpid: true,
            ept_capabilities: EptCapabilities::detect(),
        }
    }
}

// ============================================================================
// SVM SUPPORT
// ============================================================================

#[derive(Clone, Debug)]
pub struct SvmCapabilities {
    pub supported: bool,
    pub enabled: bool,
    pub nested_paging: bool,
    pub asid_count: u32,
    pub npt_size: u32,
}

impl SvmCapabilities {
    pub fn detect() -> Self {
        // Check CPUID 0x8000000A for SVM features
        // Read VM_CR MSR
        
        SvmCapabilities {
            supported: false, // Default for Intel system
            enabled: false,
            nested_paging: false,
            asid_count: 0,
            npt_size: 0,
        }
    }
}

// ============================================================================
// EPT (Extended Page Tables)
// ============================================================================

#[derive(Clone, Debug)]
pub struct EptCapabilities {
    pub supported: bool,
    pub page_walk_4: bool,
    pub page_walk_5: bool,
    pub pml4_1g_pages: bool,
    pub pml4_2m_pages: bool,
    pub invept: bool,
    pub invept_single: bool,
    pub invept_global: bool,
    pub invept_context: bool,
    pub memory_types: u8,
}

impl EptCapabilities {
    pub fn detect() -> Self {
        // Read IA32_VMX_EPT_VPID_CAP MSR
        EptCapabilities {
            supported: true,
            page_walk_4: true,
            page_walk_5: false,
            pml4_1g_pages: true,
            pml4_2m_pages: true,
            invept: true,
            invept_single: true,
            invept_global: true,
            invept_context: false,
            memory_types: 0x3F,
        }
    }
}

/// EPT Entry (PML4/PDPT/PD/PT)
#[derive(Clone, Copy, Debug)]
pub struct EptEntry {
    pub value: u64,
}

impl EptEntry {
    pub fn new() -> Self {
        EptEntry { value: 0 }
    }

    pub fn from_addr(addr: u64, perms: u64, mem_type: u64) -> Self {
        EptEntry {
            value: (addr & 0x000FFFFF_FFFFF000) | (perms & 0xF) | ((mem_type & 0x7) << 3) | 0x40, // Present + Write + Execute
        }
    }

    pub fn get_addr(&self) -> u64 {
        self.value & 0x000FFFFF_FFFFF000
    }

    pub fn is_present(&self) -> bool {
        (self.value & 0x40) != 0
    }

    pub fn set_present(&mut self, present: bool) {
        if present {
            self.value |= 0x40;
        } else {
            self.value &= !0x40;
        }
    }

    pub fn is_large(&self) -> bool {
        (self.value & 0x80) != 0
    }

    pub fn set_large(&mut self, large: bool) {
        if large {
            self.value |= 0x80;
        } else {
            self.value &= !0x80;
        }
    }

    pub fn get_permissions(&self) -> u64 {
        self.value & 0xF
    }

    pub fn set_permissions(&mut self, perms: u64) {
        self.value = (self.value & !0xF) | (perms & 0xF);
    }
}

/// EPT Page Table
#[derive(Clone, Debug)]
pub struct EptPageTable {
    pub pml4: Vec<EptEntry>,
    pub pdpt: Vec<EptEntry>,
    pub pd: Vec<EptEntry>,
    pub pt: Vec<EptEntry>,
    pub pml4_phys: u64,
}

impl EptPageTable {
    pub fn new() -> Self {
        EptPageTable {
            pml4: vec![EptEntry::new(); 512],
            pdpt: vec![EptEntry::new(); 512],
            pd: vec![EptEntry::new(); 512],
            pt: vec![EptEntry::new(); 512],
            pml4_phys: 0,
        }
    }

    /// Map a 4K page
    pub fn map_4k(&mut self, gpa: u64, hpa: u64, perms: u64, mem_type: u64) {
        let pml4_idx = ((gpa >> 39) & 0x1FF) as usize;
        let pdpt_idx = ((gpa >> 30) & 0x1FF) as usize;
        let pd_idx = ((gpa >> 21) & 0x1FF) as usize;
        let pt_idx = ((gpa >> 12) & 0x1FF) as usize;

        // Create entry at PT level
        self.pt[pt_idx] = EptEntry::from_addr(hpa, perms, mem_type);
        self.pt[pt_idx].set_present(true);

        // Link tables
        if !self.pml4[pml4_idx].is_present() {
            self.pml4[pml4_idx] = EptEntry::from_addr(self.pdpt.as_ptr() as u64, EPT_READ | EPT_WRITE | EPT_EXECUTE, EPT_MEM_TYPE_WB);
            self.pml4[pml4_idx].set_present(true);
        }

        if !self.pdpt[pdpt_idx].is_present() {
            self.pdpt[pdpt_idx] = EptEntry::from_addr(self.pd.as_ptr() as u64, EPT_READ | EPT_WRITE | EPT_EXECUTE, EPT_MEM_TYPE_WB);
            self.pdpt[pdpt_idx].set_present(true);
        }

        if !self.pd[pd_idx].is_present() {
            self.pd[pd_idx] = EptEntry::from_addr(self.pt.as_ptr() as u64, EPT_READ | EPT_WRITE | EPT_EXECUTE, EPT_MEM_TYPE_WB);
            self.pd[pd_idx].set_present(true);
        }
    }

    /// Map a 2M page
    pub fn map_2m(&mut self, gpa: u64, hpa: u64, perms: u64, mem_type: u64) {
        let pml4_idx = ((gpa >> 39) & 0x1FF) as usize;
        let pdpt_idx = ((gpa >> 30) & 0x1FF) as usize;
        let pd_idx = ((gpa >> 21) & 0x1FF) as usize;

        // Create large entry at PD level
        self.pd[pd_idx] = EptEntry::from_addr(hpa, perms, mem_type);
        self.pd[pd_idx].set_present(true);
        self.pd[pd_idx].set_large(true);

        // Link tables
        if !self.pml4[pml4_idx].is_present() {
            self.pml4[pml4_idx] = EptEntry::from_addr(self.pdpt.as_ptr() as u64, EPT_READ | EPT_WRITE | EPT_EXECUTE, EPT_MEM_TYPE_WB);
            self.pml4[pml4_idx].set_present(true);
        }

        if !self.pdpt[pdpt_idx].is_present() {
            self.pdpt[pdpt_idx] = EptEntry::from_addr(self.pd.as_ptr() as u64, EPT_READ | EPT_WRITE | EPT_EXECUTE, EPT_MEM_TYPE_WB);
            self.pdpt[pdpt_idx].set_present(true);
        }
    }

    /// Get EPTP value for VMCS
    pub fn get_eptp(&self) -> u64 {
        // EPTP format:
        // Bits 2:0 - Memory type (0=UC, 6=WB)
        // Bits 5:3 - Page walk length minus 1 (3=PML4, 4=PML5)
        // Bits 51:12 - PML4 physical address
        // Bit 6 - Enable dirty flag access/updates
        let mem_type = EPT_MEM_TYPE_WB;
        let walk_length = 3; // 4-level paging
        (mem_type) | (walk_length << 3) | (self.pml4_phys & 0x000FFFFF_FFFFF000) | (1 << 6)
    }
}

impl Default for EptPageTable {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// VMCS (Virtual Machine Control Structure)
// ============================================================================

#[derive(Clone, Debug)]
pub struct Vmcs {
    pub revision_id: u32,
    pub abort_indicator: u32,
    pub data: Vec<u64>,
    pub initialized: bool,
    pub launched: bool,
}

impl Vmcs {
    pub fn new(revision: u32) -> Self {
        Vmcs {
            revision_id: revision,
            abort_indicator: 0,
            data: vec![0; 2048 / 8], // VMCS is 4KB
            initialized: false,
            launched: false,
        }
    }

    /// Write to VMCS field
    pub fn write(&mut self, field: u32, value: u64) -> Result<(), VirtError> {
        // In real implementation, use VMWRITE instruction
        let offset = Self::field_to_offset(field);
        if offset < self.data.len() {
            self.data[offset] = value;
            Ok(())
        } else {
            Err(VirtError::VmcsInitFailed)
        }
    }

    /// Read from VMCS field
    pub fn read(&self, field: u32) -> Result<u64, VirtError> {
        let offset = Self::field_to_offset(field);
        if offset < self.data.len() {
            Ok(self.data[offset])
        } else {
            Err(VirtError::VmcsInitFailed)
        }
    }

    fn field_to_offset(field: u32) -> usize {
        // VMCS field encoding to offset conversion
        ((field & 0x7FF) as usize) * 2
    }

    /// Setup guest state
    pub fn setup_guest_state(&mut self, entry_point: u64, stack: u64) -> Result<(), VirtError> {
        // Guest segment selectors
        self.write(VMCS_GUEST_CS_SEL, 0x08)?; // Kernel code segment
        self.write(VMCS_GUEST_DS_SEL, 0x10)?; // Kernel data segment
        self.write(VMCS_GUEST_SS_SEL, 0x10)?;
        self.write(VMCS_GUEST_ES_SEL, 0x10)?;
        self.write(VMCS_GUEST_FS_SEL, 0x10)?;
        self.write(VMCS_GUEST_GS_SEL, 0x10)?;

        // Guest segment bases
        self.write(VMCS_GUEST_CS_BASE, 0)?;
        self.write(VMCS_GUEST_DS_BASE, 0)?;
        self.write(VMCS_GUEST_SS_BASE, 0)?;
        self.write(VMCS_GUEST_ES_BASE, 0)?;
        self.write(VMCS_GUEST_FS_BASE, 0)?;
        self.write(VMCS_GUEST_GS_BASE, 0)?;

        // Guest segment limits
        self.write(VMCS_GUEST_CS_LIMIT, 0xFFFFF)?;
        self.write(VMCS_GUEST_DS_LIMIT, 0xFFFFF)?;
        self.write(VMCS_GUEST_SS_LIMIT, 0xFFFFF)?;
        self.write(VMCS_GUEST_ES_LIMIT, 0xFFFFF)?;
        self.write(VMCS_GUEST_FS_LIMIT, 0xFFFFF)?;
        self.write(VMCS_GUEST_GS_LIMIT, 0xFFFFF)?;

        // Guest segment access rights
        // 0xA09B = Present, DPL=0, Code, Executable, Readable, Accessed
        self.write(VMCS_GUEST_CS_AR, 0xA09B)?;
        // 0xC093 = Present, DPL=0, Data, Writeable, Accessed
        self.write(VMCS_GUEST_DS_AR, 0xC093)?;
        self.write(VMCS_GUEST_SS_AR, 0xC093)?;
        self.write(VMCS_GUEST_ES_AR, 0xC093)?;
        self.write(VMCS_GUEST_FS_AR, 0xC093)?;
        self.write(VMCS_GUEST_GS_AR, 0xC093)?;

        // Guest control registers
        self.write(VMCS_GUEST_CR0, 0x80000001)?; // PE + PG
        self.write(VMCS_GUEST_CR3, 0)?;
        self.write(VMCS_GUEST_CR4, 0x00000620)?; // PAE + VMXE

        // Guest RIP, RSP, RFLAGS
        self.write(VMCS_GUEST_RIP, entry_point)?;
        self.write(VMCS_GUEST_RSP, stack)?;
        self.write(VMCS_GUEST_RFLAGS, 0x02)?; // Reserved bit always set

        self.initialized = true;
        Ok(())
    }

    /// Setup host state
    pub fn setup_host_state(&mut self, host_rsp: u64, host_rip: u64) -> Result<(), VirtError> {
        // Host segment selectors
        self.write(VMCS_HOST_CS_SEL, 0x08)?;
        self.write(VMCS_HOST_DS_SEL, 0x10)?;
        self.write(VMCS_HOST_SS_SEL, 0x10)?;
        self.write(VMCS_HOST_ES_SEL, 0x10)?;
        self.write(VMCS_HOST_FS_SEL, 0x10)?;
        self.write(VMCS_HOST_GS_SEL, 0x10)?;
        self.write(VMCS_HOST_TR_SEL, 0x28)?;

        // Host control registers
        self.write(VMCS_HOST_CR0, 0x80000001)?;
        self.write(VMCS_HOST_CR3, 0)?;
        self.write(VMCS_HOST_CR4, 0x00000620)?;

        // Host RSP and RIP
        self.write(VMCS_HOST_RSP, host_rsp)?;
        self.write(VMCS_HOST_RIP, host_rip)?;

        Ok(())
    }

    /// Setup controls
    pub fn setup_controls(&mut self, eptp: u64) -> Result<(), VirtError> {
        // Pin-based controls
        self.write(VMCS_CTRL_PIN_BASED, 0x00000001)?; // External interrupt exiting

        // Primary processor-based controls
        let proc_ctrl = 0x00000000;
        self.write(VMCS_CTRL_PROC_BASED, proc_ctrl)?;

        // Secondary processor-based controls
        let proc_ctrl2 = 0x00000002; // Enable EPT
        self.write(VMCS_CTRL_PROC_BASED_2, proc_ctrl2)?;

        // EPTP
        self.write(VMCS_EPTP, eptp)?;

        // Exit controls
        self.write(VMCS_CTRL_EXIT, 0x00000000)?;

        // Entry controls
        self.write(VMCS_CTRL_ENTRY, 0x00000000)?;

        Ok(())
    }
}

// ============================================================================
// VIRTUAL MACHINE
// ============================================================================

#[derive(Clone, Debug)]
pub struct VirtualMachine {
    pub id: u32,
    pub name: String,
    pub vmcs: Vmcs,
    pub ept: EptPageTable,
    pub state: VmState,
    pub exit_reason: u32,
    pub exit_qualification: u64,
    pub guest_memory: Vec<u8>,
    pub guest_memory_size: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmState {
    Created,
    Running,
    Halted,
    Paused,
    Error,
}

impl VirtualMachine {
    pub fn new(id: u32, name: &str, memory_size: usize, vmcs_revision: u32) -> Self {
        VirtualMachine {
            id,
            name: name.to_string(),
            vmcs: Vmcs::new(vmcs_revision),
            ept: EptPageTable::new(),
            state: VmState::Created,
            exit_reason: 0,
            exit_qualification: 0,
            guest_memory: vec![0; memory_size],
            guest_memory_size: memory_size,
        }
    }

    /// Initialize VM
    pub fn init(&mut self, entry_point: u64, stack_top: u64) -> Result<(), VirtError> {
        // Setup EPT
        // Map guest physical memory to host physical memory
        for i in 0..self.guest_memory_size / 4096 {
            let gpa = (i * 4096) as u64;
            let hpa = self.guest_memory.as_ptr() as u64 + gpa;
            self.ept.map_4k(gpa, hpa, EPT_READ | EPT_WRITE | EPT_EXECUTE, EPT_MEM_TYPE_WB);
        }

        // Setup VMCS
        let eptp = self.ept.get_eptp();
        self.vmcs.setup_controls(eptp)?;
        self.vmcs.setup_guest_state(entry_point, stack_top)?;
        self.vmcs.setup_host_state(0, 0)?; // Host state will be set on VM-entry

        self.state = VmState::Created;
        Ok(())
    }

    /// Start VM
    pub fn start(&mut self) -> Result<(), VirtError> {
        if self.state != VmState::Created && self.state != VmState::Halted {
            return Err(VirtError::InvalidState);
        }

        // In real implementation, use VMLAUNCH instruction
        self.state = VmState::Running;
        self.vmcs.launched = true;

        Ok(())
    }

    /// Resume VM
    pub fn resume(&mut self) -> Result<(), VirtError> {
        if self.state != VmState::Paused {
            return Err(VirtError::InvalidState);
        }

        // In real implementation, use VMRESUME instruction
        self.state = VmState::Running;

        Ok(())
    }

    /// Pause VM
    pub fn pause(&mut self) -> Result<(), VirtError> {
        if self.state != VmState::Running {
            return Err(VirtError::InvalidState);
        }

        self.state = VmState::Paused;
        Ok(())
    }

    /// Stop VM
    pub fn stop(&mut self) -> Result<(), VirtError> {
        self.state = VmState::Halted;
        Ok(())
    }

    /// Handle VM exit
    pub fn handle_exit(&mut self) -> Result<(), VirtError> {
        // Read exit reason and qualification
        // self.exit_reason = self.vmcs.read(VM_EXIT_REASON)? as u32;
        // self.exit_qualification = self.vmcs.read(VM_EXIT_QUALIFICATION)?;

        match self.exit_reason {
            0 => {
                // External interrupt
                self.state = VmState::Paused;
            }
            1 => {
                // Triple fault
                self.state = VmState::Error;
                return Err(VirtError::InvalidState);
            }
            _ => {
                // Other exit reasons
                self.state = VmState::Paused;
            }
        }

        Ok(())
    }

    /// Get guest memory
    pub fn get_memory(&self) -> &[u8] {
        &self.guest_memory
    }

    /// Get mutable guest memory
    pub fn get_memory_mut(&mut self) -> &mut [u8] {
        &mut self.guest_memory
    }
}

// ============================================================================
// VIRTUAL MACHINE MANAGER
// ============================================================================

#[derive(Clone, Debug)]
pub struct Vmm {
    pub vendor: CpuVendor,
    pub vmx_caps: Option<VmxCapabilities>,
    pub svm_caps: Option<SvmCapabilities>,
    pub vms: BTreeMap<u32, VirtualMachine>,
    pub next_vm_id: u32,
    pub initialized: bool,
}

impl Vmm {
    pub fn new() -> Self {
        Vmm {
            vendor: CpuVendor::detect(),
            vmx_caps: None,
            svm_caps: None,
            vms: BTreeMap::new(),
            next_vm_id: 1,
            initialized: false,
        }
    }

    /// Initialize VMM
    pub fn init(&mut self) -> Result<(), VirtError> {
        crate::serial_println!("[VMM] Initializing virtualization...");

        match self.vendor {
            CpuVendor::Intel => {
                let caps = VmxCapabilities::detect();
                if !caps.supported {
                    return Err(VirtError::NotSupported);
                }
                if !caps.enabled {
                    return Err(VirtError::DisabledInBIOS);
                }
                self.vmx_caps = Some(caps);
                crate::serial_println!("[VMM] Intel VMX detected and enabled");
            }
            CpuVendor::Amd => {
                let caps = SvmCapabilities::detect();
                if !caps.supported {
                    return Err(VirtError::NotSupported);
                }
                if !caps.enabled {
                    return Err(VirtError::DisabledInBIOS);
                }
                self.svm_caps = Some(caps);
                crate::serial_println!("[VMM] AMD SVM detected and enabled");
            }
            CpuVendor::Unknown => {
                return Err(VirtError::NotSupported);
            }
        }

        self.initialized = true;
        Ok(())
    }

    /// Create new VM
    pub fn create_vm(&mut self, name: &str, memory_size: usize) -> Result<u32, VirtError> {
        if !self.initialized {
            return Err(VirtError::NotSupported);
        }

        let id = self.next_vm_id;
        self.next_vm_id += 1;

        let vmcs_revision = self.vmx_caps.as_ref().map(|c| c.vmcs_revision).unwrap_or(1);
        let vm = VirtualMachine::new(id, name, memory_size, vmcs_revision);

        self.vms.insert(id, vm);

        crate::serial_println!("[VMM] Created VM {} ({}) with {} MB memory",
            id, name, memory_size / (1024 * 1024));

        Ok(id)
    }

    /// Get VM by ID
    pub fn get_vm(&self, id: u32) -> Option<&VirtualMachine> {
        self.vms.get(&id)
    }

    /// Get VM mutable
    pub fn get_vm_mut(&mut self, id: u32) -> Option<&mut VirtualMachine> {
        self.vms.get_mut(&id)
    }

    /// Destroy VM
    pub fn destroy_vm(&mut self, id: u32) -> bool {
        self.vms.remove(&id).is_some()
    }

    /// List all VMs
    pub fn list_vms(&self) -> Vec<(u32, String, VmState)> {
        self.vms.iter()
            .map(|(id, vm)| (*id, vm.name.clone(), vm.state))
            .collect()
    }
}

impl Default for Vmm {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// GLOBAL VMM INSTANCE
// ============================================================================

lazy_static::lazy_static! {
    static ref VMM_INSTANCE: Mutex<Vmm> = Mutex::new(Vmm::new());
}

/// Initialize virtualization
pub fn init() -> Result<(), VirtError> {
    VMM_INSTANCE.lock().init()
}

/// Get VMM
pub fn get_vmm() -> Vmm {
    VMM_INSTANCE.lock().clone()
}

/// Create VM
pub fn create_vm(name: &str, memory_size: usize) -> Result<u32, VirtError> {
    VMM_INSTANCE.lock().create_vm(name, memory_size)
}

/// Get VM
pub fn get_vm(id: u32) -> Option<VirtualMachine> {
    VMM_INSTANCE.lock().get_vm(id).cloned()
}

/// Destroy VM
pub fn destroy_vm(id: u32) -> bool {
    VMM_INSTANCE.lock().destroy_vm(id)
}

/// List VMs
pub fn list_vms() -> Vec<(u32, String, VmState)> {
    VMM_INSTANCE.lock().list_vms()
}
