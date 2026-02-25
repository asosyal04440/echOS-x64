//! # CPU Microcode Update Support
//!
//! Intel and AMD CPU microcode loading at boot/runtime.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

// ============================================================================
// MICROCODE CONSTANTS
// ============================================================================

/// Intel microcode MSR
pub const MSR_IA32_UCODE_WRITE: u32 = 0x79;
pub const MSR_IA32_UCODE_REV: u32 = 0x8B;
pub const MSR_IA32_BIOS_SIGN_ID: u32 = 0x8B;
pub const MSR_IA32_UCODE_API_VERSION: u32 = 0x8C;

/// AMD microcode MSR
pub const MSR_AMD_PATCH_LOADER: u32 = 0xC0010020;

/// Maximum microcode size
pub const MICROCODE_MAX_SIZE: usize = 2 * 1024 * 1024; // 2MB

// ============================================================================
// INTEL MICROCODE HEADER
// ============================================================================

/// Intel microcode update header
#[repr(C, packed)]
pub struct IntelMicrocodeHeader {
    /// Version stamp
    pub header_version: u32,
    /// Unique version number
    pub update_revision: u32,
    /// Date of creation (BCD: 0xYYYYMMDD)
    pub date: u32,
    /// Extended signature table size
    pub ext_sig_table_size: u32,
    /// Extended signature table checksum
    pub ext_sig_checksum: u32,
    /// Reserved
    pub reserved: [u32; 3],
    /// Processor family/model/stepping
    pub processor_signature: u32,
    /// Checksum of update data and header
    pub checksum: u32,
    /// Loader version
    pub loader_revision: u32,
    /// Platform ID bit mask
    pub processor_flags: u32,
    /// Size of data in bytes (divided by 4)
    pub data_size: u32,
    /// Total size in bytes (divided by 4)
    pub total_size: u32,
    // Followed by: data[datasize], extended signatures
}

/// Intel extended signature
#[repr(C)]
pub struct IntelExtSignature {
    pub processor_signature: u32,
    pub processor_flags: u32,
    pub checksum: u32,
}

// ============================================================================
// AMD MICROCODE HEADER
// ============================================================================

/// AMD microcode update header
#[repr(C, packed)]
pub struct AmdMicrocodeHeader {
    /// Data size in bytes
    pub data_size: u32,
    /// Patch level (revision)
    pub patch_id: u32,
    /// Reserved
    pub reserved1: [u8; 4],
    /// Chip 1 ID
    pub chip1_id: u16,
    /// Chip 2 ID
    pub chip2_id: u16,
    /// Processor revision ID
    pub proc_rev_id: u16,
    /// Chip 1 revision ID
    pub chip1_rev_id: u16,
    /// Chip 2 revision ID
    pub chip2_rev_id: u16,
    /// North Bridge ID
    pub nb_id: u16,
    /// South Bridge ID
    pub sb_id: u16,
    /// BIOS revision
    pub bios_rev: u32,
    /// Reserved
    pub reserved2: [u32; 3],
    /// Match register
    pub match_reg: u32,
    /// Patch data block ID
    pub patch_data_id: u32,
    /// Patch block length
    pub patch_block_len: u8,
    /// Init block length
    pub init_block_len: u8,
    /// Block load base
    pub block_load_base: u16,
    /// Number of blocks
    pub num_blocks: u8,
    /// Header version
    pub header_version: u8,
    /// Reserved
    pub reserved3: [u8; 6],
    /// Patch data block
    pub patch_data_block: [u8; 896],
}

// ============================================================================
// MICROCODE MANAGER
// ============================================================================

/// Microcode information
#[derive(Clone, Debug)]
pub struct MicrocodeInfo {
    pub vendor: CpuVendor,
    pub current_revision: u32,
    pub processor_signature: u32,
    pub processor_flags: u32,
    pub loaded_patch: Option<u32>,
}

/// CPU vendor
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuVendor {
    Intel,
    Amd,
    Unknown,
}

/// Microcode manager
pub struct MicrocodeManager {
    /// Current microcode revision
    current_revision: AtomicU32,
    /// Processor signature
    processor_signature: AtomicU32,
    /// Processor flags
    processor_flags: AtomicU32,
    /// Vendor
    vendor: CpuVendor,
    /// Update count
    update_count: AtomicU32,
    /// Last update time
    last_update: AtomicU64,
}

impl MicrocodeManager {
    pub const fn new() -> Self {
        Self {
            current_revision: AtomicU32::new(0),
            processor_signature: AtomicU32::new(0),
            processor_flags: AtomicU32::new(0),
            vendor: CpuVendor::Unknown,
            update_count: AtomicU32::new(0),
            last_update: AtomicU64::new(0),
        }
    }

    /// Initialize and detect CPU info
    pub fn init(&mut self) {
        // Get CPU vendor
        let vendor_str = self.get_vendor_string();
        self.vendor = if vendor_str.starts_with("GenuineIntel") {
            CpuVendor::Intel
        } else if vendor_str.starts_with("AuthenticAMD") {
            CpuVendor::Amd
        } else {
            CpuVendor::Unknown
        };
        
        // Get processor signature
        let cpuid = unsafe { core::arch::x86_64::__cpuid(1) };
        self.processor_signature.store(cpuid.eax, Ordering::SeqCst);
        
        // Get current microcode revision
        self.read_current_revision();
        
        crate::serial_println!(
            "[MICROCODE] Vendor: {:?}, Signature: {:#x}, Revision: {}",
            self.vendor,
            self.processor_signature.load(Ordering::SeqCst),
            self.current_revision.load(Ordering::SeqCst)
        );
    }

    /// Get CPU vendor string
    fn get_vendor_string(&self) -> [u8; 13] {
        let mut vendor = [0u8; 13];
        unsafe {
            let cpuid = core::arch::x86_64::__cpuid(0);
            let ebx = cpuid.ebx.to_le_bytes();
            let ecx = cpuid.ecx.to_le_bytes();
            let edx = cpuid.edx.to_le_bytes();
            vendor[0..4].copy_from_slice(&ebx);
            vendor[4..8].copy_from_slice(&edx);
            vendor[8..12].copy_from_slice(&ecx);
        }
        vendor
    }

    /// Read current microcode revision
    fn read_current_revision(&self) {
        // Write 0 to MSR_IA32_BIOS_SIGN_ID to trigger read
        unsafe {
            crate::cpu::msr::write(MSR_IA32_BIOS_SIGN_ID, 0);
            // CPUID to trigger update
            let _ = core::arch::x86_64::__cpuid(1);
            // Read revision
            let rev = crate::cpu::msr::read(MSR_IA32_BIOS_SIGN_ID) >> 32;
            self.current_revision.store(rev as u32, Ordering::SeqCst);
        }
    }

    /// Load Intel microcode
    pub fn load_intel_microcode(&self, data: &[u8]) -> Result<u32, MicrocodeError> {
        if data.len() < core::mem::size_of::<IntelMicrocodeHeader>() {
            return Err(MicrocodeError::InvalidFormat);
        }
        
        let header = unsafe { &*(data.as_ptr() as *const IntelMicrocodeHeader) };
        
        // Validate header
        if header.header_version != 1 {
            return Err(MicrocodeError::InvalidVersion);
        }
        
        // Check processor signature match
        let sig = self.processor_signature.load(Ordering::SeqCst);
        if header.processor_signature != sig {
            // Check extended signatures
            if header.ext_sig_table_size > 0 {
                let ext_offset = header.total_size as usize * 4;
                let num_ext = header.ext_sig_table_size as usize / 
                    core::mem::size_of::<IntelExtSignature>();
                
                for i in 0..num_ext {
                    let ext = unsafe {
                        &*(data.as_ptr().add(ext_offset + i * 
                            core::mem::size_of::<IntelExtSignature>()) 
                            as *const IntelExtSignature)
                    };
                    if ext.processor_signature == sig {
                        break;
                    }
                    if i == num_ext - 1 {
                        return Err(MicrocodeError::SignatureMismatch);
                    }
                }
            } else {
                return Err(MicrocodeError::SignatureMismatch);
            }
        }
        
        // Check if this is newer
        if header.update_revision <= self.current_revision.load(Ordering::SeqCst) {
            return Err(MicrocodeError::OlderRevision);
        }
        
        // Validate checksum
        if !self.verify_intel_checksum(data, header) {
            return Err(MicrocodeError::ChecksumFailed);
        }
        
        // Load microcode
        unsafe {
            // Write microcode to MSR
            let data_offset = core::mem::size_of::<IntelMicrocodeHeader>();
            let data_ptr = data.as_ptr().add(data_offset) as u64;
            crate::cpu::msr::write(MSR_IA32_UCODE_WRITE, data_ptr);
        }
        
        // Read new revision
        self.read_current_revision();
        let new_rev = self.current_revision.load(Ordering::SeqCst);
        
        if new_rev != header.update_revision {
            return Err(MicrocodeError::LoadFailed);
        }
        
        self.update_count.fetch_add(1, Ordering::SeqCst);
        crate::serial_println!("[MICROCODE] Updated to revision {}", new_rev);
        
        Ok(new_rev)
    }

    /// Verify Intel microcode checksum
    fn verify_intel_checksum(&self, data: &[u8], header: &IntelMicrocodeHeader) -> bool {
        let total_size = if header.data_size == 0 {
            1024 // Default size
        } else {
            header.total_size as usize * 4
        };
        
        if data.len() < total_size {
            return false;
        }
        
        // Checksum should sum to 0
        let mut sum: u32 = 0;
        for i in (0..total_size).step_by(4) {
            let word = unsafe {
                *(data.as_ptr().add(i) as *const u32)
            };
            sum = sum.wrapping_add(word);
        }
        
        sum == 0
    }

    /// Load AMD microcode
    pub fn load_amd_microcode(&self, data: &[u8]) -> Result<u32, MicrocodeError> {
        if data.len() < core::mem::size_of::<AmdMicrocodeHeader>() {
            return Err(MicrocodeError::InvalidFormat);
        }
        
        let header = unsafe { &*(data.as_ptr() as *const AmdMicrocodeHeader) };
        
        // Load microcode using WRMSR to MSR_AMD_PATCH_LOADER
        unsafe {
            crate::cpu::msr::write(MSR_AMD_PATCH_LOADER, data.as_ptr() as u64);
        }
        
        // Verify update
        self.read_current_revision();
        let new_rev = self.current_revision.load(Ordering::SeqCst);
        
        self.update_count.fetch_add(1, Ordering::SeqCst);
        crate::serial_println!("[MICROCODE] AMD patch loaded, revision {}", new_rev);
        
        Ok(new_rev)
    }

    /// Load microcode from buffer
    pub fn load(&self, data: &[u8]) -> Result<u32, MicrocodeError> {
        match self.vendor {
            CpuVendor::Intel => self.load_intel_microcode(data),
            CpuVendor::Amd => self.load_amd_microcode(data),
            CpuVendor::Unknown => Err(MicrocodeError::UnknownVendor),
        }
    }

    /// Get current info
    pub fn get_info(&self) -> MicrocodeInfo {
        MicrocodeInfo {
            vendor: self.vendor,
            current_revision: self.current_revision.load(Ordering::SeqCst),
            processor_signature: self.processor_signature.load(Ordering::SeqCst),
            processor_flags: self.processor_flags.load(Ordering::SeqCst),
            loaded_patch: None,
        }
    }
}

lazy_static::lazy_static! {
    /// Global microcode manager
    static ref MICROCODE_MANAGER: spin::Mutex<MicrocodeManager> = 
        spin::Mutex::new(MicrocodeManager::new());
}

// ============================================================================
// ERROR TYPE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicrocodeError {
    InvalidFormat,
    InvalidVersion,
    SignatureMismatch,
    ChecksumFailed,
    OlderRevision,
    LoadFailed,
    UnknownVendor,
    NotFound,
}

// ============================================================================
// INITIALIZATION
// ============================================================================

/// Initialize microcode subsystem
pub fn init() {
    MICROCODE_MANAGER.lock().init();
    crate::serial_println!("[MICROCODE] Subsystem initialized");
}

/// Load microcode from buffer
pub fn load(data: &[u8]) -> Result<u32, MicrocodeError> {
    MICROCODE_MANAGER.lock().load(data)
}

/// Get current revision
pub fn get_revision() -> u32 {
    MICROCODE_MANAGER.lock().current_revision.load(Ordering::SeqCst)
}

/// Get microcode info
pub fn get_info() -> MicrocodeInfo {
    MICROCODE_MANAGER.lock().get_info()
}

/// Check if microcode update is available
pub fn check_update_available(data: &[u8]) -> bool {
    let manager = MICROCODE_MANAGER.lock();
    if data.len() < core::mem::size_of::<IntelMicrocodeHeader>() {
        return false;
    }
    
    match manager.vendor {
        CpuVendor::Intel => {
            let header = unsafe { &*(data.as_ptr() as *const IntelMicrocodeHeader) };
            header.update_revision > manager.current_revision.load(Ordering::SeqCst)
        }
        _ => false,
    }
}
