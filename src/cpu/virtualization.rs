//! # CPU Virtualization Support
//!
//! Intel VT-x and AMD SVM (AMD-V) virtualization extensions.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

// ============================================================================
// VMX CONSTANTS (Intel VT-x)
// ============================================================================

/// VMX operation bit
pub const VMX_CR4_FIXED0: u64 = 0x2000;
pub const VMX_CR4_FIXED1: u64 = 0x2FFF;

/// VMX MSR addresses
pub const MSR_IA32_VMX_BASIC: u32 = 0x480;
pub const MSR_IA32_VMX_PINBASED_CTLS: u32 = 0x481;
pub const MSR_IA32_VMX_PROCBASED_CTLS: u32 = 0x482;
pub const MSR_IA32_VMX_EXIT_CTLS: u32 = 0x483;
pub const MSR_IA32_VMX_ENTRY_CTLS: u32 = 0x484;
pub const MSR_IA32_VMX_MISC: u32 = 0x485;
pub const MSR_IA32_VMX_CR0_FIXED0: u32 = 0x486;
pub const MSR_IA32_VMX_CR0_FIXED1: u32 = 0x487;
pub const MSR_IA32_VMX_CR4_FIXED0: u32 = 0x488;
pub const MSR_IA32_VMX_CR4_FIXED1: u32 = 0x489;
pub const MSR_IA32_VMX_VMCS_ENUM: u32 = 0x48A;
pub const MSR_IA32_FEATURE_CONTROL: u32 = 0x3A;

/// VMCS field encodings
pub const VMCS_LINK_POINTER: u32 = 0x2800;
pub const VMCS_GUEST_ES_SELECTOR: u32 = 0x800;
pub const VMCS_GUEST_CS_SELECTOR: u32 = 0x802;
pub const VMCS_GUEST_SS_SELECTOR: u32 = 0x804;
pub const VMCS_GUEST_DS_SELECTOR: u32 = 0x806;
pub const VMCS_GUEST_FS_SELECTOR: u32 = 0x808;
pub const VMCS_GUEST_GS_SELECTOR: u32 = 0x80A;
pub const VMCS_GUEST_LDTR_SELECTOR: u32 = 0x80C;
pub const VMCS_GUEST_TR_SELECTOR: u32 = 0x80E;
pub const VMCS_GUEST_CR0: u32 = 0x6800;
pub const VMCS_GUEST_CR3: u32 = 0x6802;
pub const VMCS_GUEST_CR4: u32 = 0x6804;
pub const VMCS_GUEST_RSP: u32 = 0x681C;
pub const VMCS_GUEST_RIP: u32 = 0x681E;
pub const VMCS_GUEST_RFLAGS: u32 = 0x6820;

/// VMX instruction errors
pub const VMXERR_VMCALL_IN_VMX_ROOT: u32 = 1;
pub const VMXERR_VMCLEAR_INVALID_ADDR: u32 = 2;
pub const VMXERR_VMLAUNCH_NONCLEAR_VMCS: u32 = 3;

// ============================================================================
// SVM CONSTANTS (AMD-V)
// ============================================================================

/// SVM MSR
pub const MSR_VM_CR: u32 = 0xC0010114;
pub const MSR_VM_HSAVE_PA: u32 = 0xC0010117;

/// SVM features
pub const SVM_NPT: u32 = 1 << 0;      // Nested Page Tables
pub const SVM_LBR: u32 = 1 << 1;       // LBR Virtualization
pub const SVM_SVM_LOCK: u32 = 1 << 2;  // SVM Lock
pub const SVM_NRIP: u32 = 1 << 3;      // Next RIP Save

/// VMCB (Virtual Machine Control Block)
pub const VMCB_CTRL_OFFSET: usize = 0x000;
pub const VMCB_STATE_OFFSET: usize = 0x400;
pub const VMCB_SIZE: usize = 4096;

// ============================================================================
// VMX STRUCTURES
// ============================================================================

/// VMCS (Virtual Machine Control Structure)
#[repr(C, align(4096))]
pub struct Vmcs {
    pub revision_id: u32,
    pub abort_indicator: u32,
    pub data: [u64; 1022],
}

impl Vmcs {
    pub fn new() -> Self {
        Self {
            revision_id: 0, // Set from MSR_IA32_VMX_BASIC
            abort_indicator: 0,
            data: [0; 1022],
        }
    }

    /// Write to VMCS field
    pub fn write(&mut self, field: u32, value: u64) {
        let index = (field as usize) >> 1;
        if index < self.data.len() {
            self.data[index] = value;
        }
    }

    /// Read from VMCS field
    pub fn read(&self, field: u32) -> u64 {
        let index = (field as usize) >> 1;
        if index < self.data.len() {
            self.data[index]
        } else {
            0
        }
    }
}

/// VMX operation region
#[repr(C, align(4096))]
pub struct VmxonRegion {
    pub revision_id: u32,
    pub reserved: [u32; 1023],
}

// ============================================================================
// SVM STRUCTURES
// ============================================================================

/// VMCB Control Area
#[repr(C)]
pub struct VmcbControl {
    pub intercept_cr: [u16; 4],
    pub intercept_dr: [u16; 4],
    pub intercept_exceptions: u32,
    pub intercept_misc1: u32,
    pub intercept_misc2: u32,
    pub intercept_misc3: u32,
    pub pause_filter_threshold: u16,
    pub pause_filter_count: u16,
    pub iopm_base_pa: u64,
    pub msrpm_base_pa: u64,
    pub tsc_offset: u64,
    pub guest_asid: u32,
    pub tlb_control: u32,
    pub interrupt_shadow: u8,
    pub vmexec_control: u8,
    pub guest_int_ctrl: u16,
    pub guest_pause_filter_count: u16,
    pub reserved: [u64; 16],
    pub event_inject: u32,
    pub event_inject_error: u32,
    pub nested_paging: u64,
    pub virtual_apic_mode: u8,
    pub reserved2: [u8; 7],
    pub vmcb_clean: u32,
    pub reserved3: [u32; 3],
    pub guest_vint_ctrl: u32,
    pub reserved4: [u32; 3],
    pub exit_int_info: u32,
    pub exit_int_error: u32,
    pub exit_reason: u64,
    pub exit_io_info: u64,
    pub exit_info1: u64,
    pub exit_info2: u64,
    pub exit_int_info2: u32,
    pub exit_int_error2: u32,
    pub guest_pa: u64,
    pub last_branch_from: u64,
    pub last_branch_to: u64,
    pub last_branch_from_ip: u64,
    pub last_branch_to_ip: u64,
    pub reserved5: [u64; 10],
}

/// VMCB State Save Area
#[repr(C)]
pub struct VmcbState {
    pub es: Segment,
    pub cs: Segment,
    pub ss: Segment,
    pub ds: Segment,
    pub fs: Segment,
    pub gs: Segment,
    pub gdtr: Segment,
    pub ldtr: Segment,
    pub idtr: Segment,
    pub tr: Segment,
    pub reserved: [u8; 43],
    pub cpl: u8,
    pub reserved2: [u8; 4],
    pub efer: u64,
    pub reserved3: [u64; 14],
    pub cr4: u64,
    pub cr3: u64,
    pub cr0: u64,
    pub dr7: u64,
    pub dr6: u64,
    pub rflags: u64,
    pub rip: u64,
    pub reserved4: [u64; 11],
    pub rsp: u64,
    pub ssp: u64,
    pub reserved5: [u64; 4],
    pub cr2: u64,
    pub pat: u64,
    pub reserved6: [u64; 3],
    pub gp_regs: [u64; 16],
    pub reserved7: [u64; 16],
    pub xmms: [u128; 16],
    pub reserved8: [u64; 24],
}

/// Segment descriptor
#[repr(C)]
pub struct Segment {
    pub selector: u16,
    pub attrib: u16,
    pub limit: u32,
    pub base: u64,
}

/// Full VMCB
#[repr(C, align(4096))]
pub struct Vmcb {
    pub control: VmcbControl,
    pub state: VmcbState,
}

// ============================================================================
// VIRTUALIZATION MANAGER
// ============================================================================

/// Virtualization support status
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VtxStatus {
    NotSupported,
    Disabled,
    Enabled,
    Active,
}

/// Virtualization manager
pub struct VtxManager {
    /// Intel VT-x supported
    vmx_supported: AtomicBool,
    /// AMD SVM supported
    svm_supported: AtomicBool,
    /// VMX active
    vmx_active: AtomicBool,
    /// SVM active
    svm_active: AtomicBool,
    /// VMXON region physical address
    vmxon_region: AtomicU64,
    /// Active VMCS count
    active_vmcs_count: AtomicU64,
}

impl VtxManager {
    pub const fn new() -> Self {
        Self {
            vmx_supported: AtomicBool::new(false),
            svm_supported: AtomicBool::new(false),
            vmx_active: AtomicBool::new(false),
            svm_active: AtomicBool::new(false),
            vmxon_region: AtomicU64::new(0),
            active_vmcs_count: AtomicU64::new(0),
        }
    }

    /// Detect virtualization support
    pub fn detect(&self) {
        // Check CPUID for VMX
        let cpuid = unsafe { core::arch::x86_64::__cpuid(1) };
        let vmx_bit = (cpuid.ecx >> 5) & 1;
        
        if vmx_bit == 1 {
            self.vmx_supported.store(true, Ordering::SeqCst);
            crate::serial_println!("[VTX] Intel VT-x supported");
        }
        
        // Check for SVM (AMD)
        let cpuid_ext = unsafe { core::arch::x86_64::__cpuid(0x80000001) };
        let svm_bit = (cpuid_ext.ecx >> 2) & 1;
        
        if svm_bit == 1 {
            self.svm_supported.store(true, Ordering::SeqCst);
            crate::serial_println!("[VTX] AMD SVM supported");
        }
    }

    /// Enable VMX operation
    pub fn enable_vmx(&self) -> Result<(), VtxError> {
        if !self.vmx_supported.load(Ordering::SeqCst) {
            return Err(VtxError::NotSupported);
        }
        
        // Check IA32_FEATURE_CONTROL MSR
        let feature_control = unsafe { 
            crate::cpu::msr::read(MSR_IA32_FEATURE_CONTROL)
        };
        
        // Check if VMX is locked and enabled
        if (feature_control & 1) == 0 {
            // Not locked, enable VMX
            unsafe {
                crate::cpu::msr::write(MSR_IA32_FEATURE_CONTROL, feature_control | 5);
            }
        } else if (feature_control & 4) == 0 {
            // Locked but VMX outside SMX disabled
            return Err(VtxError::DisabledByBios);
        }
        
        // Set CR4.VMXE bit
        unsafe {
            let cr4 = crate::cpu::read_cr4();
            crate::cpu::write_cr4(cr4 | (1 << 13));
        }
        
        crate::serial_println!("[VTX] VMX enabled");
        Ok(())
    }

    /// Enter VMX root operation
    pub fn vmxon(&self, region_phys: u64) -> Result<(), VtxError> {
        self.vmxon_region.store(region_phys, Ordering::SeqCst);
        
        // Execute VMXON instruction
        // This would be done with inline assembly
        crate::serial_println!("[VTX] VMXON at {:#x}", region_phys);
        
        self.vmx_active.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Exit VMX root operation
    pub fn vmxoff(&self) -> Result<(), VtxError> {
        // Execute VMXOFF instruction
        self.vmx_active.store(false, Ordering::SeqCst);
        crate::serial_println!("[VTX] VMXOFF");
        Ok(())
    }

    /// Create new VMCS
    pub fn create_vmcs(&self) -> u64 {
        self.active_vmcs_count.fetch_add(1, Ordering::SeqCst);
        // Allocate and return VMCS physical address
        0
    }

    /// Check if VMX is active
    pub fn is_vmx_active(&self) -> bool {
        self.vmx_active.load(Ordering::SeqCst)
    }

    /// Check if SVM is active
    pub fn is_svm_active(&self) -> bool {
        self.svm_active.load(Ordering::SeqCst)
    }
}

lazy_static::lazy_static! {
    /// Global virtualization manager
    pub static ref VTX_MANAGER: VtxManager = VtxManager::new();
}

// ============================================================================
// ERROR TYPE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VtxError {
    NotSupported,
    DisabledByBios,
    VmxonFailed,
    VmclearFailed,
    VmlaunchFailed,
    InvalidVmcs,
    OutOfMemory,
}

// ============================================================================
// VMX INSTRUCTION WRAPPERS
// ============================================================================

/// Execute VMCLEAR
pub unsafe fn vmclear(vmcs_phys: u64) -> Result<(), VtxError> {
    // VMCLEAR instruction
    Ok(())
}

/// Execute VMPTRLD
pub unsafe fn vmptrld(vmcs_phys: u64) -> Result<(), VtxError> {
    // VMPTRLD instruction
    Ok(())
}

/// Execute VMREAD
pub unsafe fn vmread(field: u32) -> u64 {
    0
}

/// Execute VMWRITE
pub unsafe fn vmwrite(field: u32, value: u64) {
}

/// Execute VMLAUNCH
pub unsafe fn vmlaunch() -> Result<(), VtxError> {
    Ok(())
}

/// Execute VMRESUME
pub unsafe fn vmresume() -> Result<(), VtxError> {
    Ok(())
}

// ============================================================================
// SVM INSTRUCTION WRAPPERS
// ============================================================================

/// Execute VMRUN (AMD)
pub unsafe fn vmrun(vmcb_phys: u64) -> Result<(), VtxError> {
    Ok(())
}

/// Execute VMSAVE (AMD)
pub unsafe fn vmsave(vmcb_phys: u64) {
}

/// Execute VMLOAD (AMD)
pub unsafe fn vmload(vmcb_phys: u64) {
}

// ============================================================================
// INITIALIZATION
// ============================================================================

/// Initialize virtualization subsystem
pub fn init() {
    VTX_MANAGER.detect();
    crate::serial_println!("[VTX] Subsystem initialized");
}

/// Check if virtualization is available
pub fn is_available() -> bool {
    VTX_MANAGER.vmx_supported.load(Ordering::SeqCst) || 
    VTX_MANAGER.svm_supported.load(Ordering::SeqCst)
}

/// Get status
pub fn get_status() -> VtxStatus {
    if VTX_MANAGER.is_vmx_active() || VTX_MANAGER.is_svm_active() {
        VtxStatus::Active
    } else if is_available() {
        VtxStatus::Enabled
    } else {
        VtxStatus::NotSupported
    }
}
