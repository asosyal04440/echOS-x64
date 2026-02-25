//! # echOS PE/COFF Loader
//!
//! Windows Portable Executable (PE) loader for running Windows binaries
//! Supports PE32+ (64-bit) executables and DLLs

use alloc::vec::Vec;
use alloc::vec;
use alloc::string::String;
use alloc::string::ToString;
use alloc::collections::BTreeMap;
use alloc::boxed::Box;
use alloc::sync::Arc;
use spin::Mutex;
use core::mem::size_of;

// ============================================================================
// PE CONSTANTS
// ============================================================================

/// DOS header magic number ("MZ")
const DOS_MAGIC: u16 = 0x5A4D;

/// PE signature ("PE\0\0")
const PE_SIGNATURE: u32 = 0x00004550;

/// PE32+ (64-bit) optional header magic
const PE32_PLUS_MAGIC: u16 = 0x20B;

/// PE32 (32-bit) optional header magic
const PE32_MAGIC: u16 = 0x10B;

// Image characteristics
const IMAGE_FILE_EXECUTABLE_IMAGE: u16 = 0x0002;
const IMAGE_FILE_LARGE_ADDRESS_AWARE: u16 = 0x0020;
const IMAGE_FILE_32BIT_MACHINE: u16 = 0x0100;
const IMAGE_FILE_DLL: u16 = 0x2000;

// Section characteristics
const IMAGE_SCN_CNT_CODE: u32 = 0x00000020;
const IMAGE_SCN_CNT_INITIALIZED_DATA: u32 = 0x00000040;
const IMAGE_SCN_CNT_UNINITIALIZED_DATA: u32 = 0x00000080;
const IMAGE_SCN_MEM_EXECUTE: u32 = 0x20000000;
const IMAGE_SCN_MEM_READ: u32 = 0x40000000;
const IMAGE_SCN_MEM_WRITE: u32 = 0x80000000;

// Directory entries
const IMAGE_DIRECTORY_ENTRY_EXPORT: usize = 0;
const IMAGE_DIRECTORY_ENTRY_IMPORT: usize = 1;
const IMAGE_DIRECTORY_ENTRY_RESOURCE: usize = 2;
const IMAGE_DIRECTORY_ENTRY_EXCEPTION: usize = 3;
const IMAGE_DIRECTORY_ENTRY_SECURITY: usize = 4;
const IMAGE_DIRECTORY_ENTRY_BASERELOC: usize = 5;
const IMAGE_DIRECTORY_ENTRY_DEBUG: usize = 6;
const IMAGE_DIRECTORY_ENTRY_TLS: usize = 9;
const IMAGE_DIRECTORY_ENTRY_IAT: usize = 12;

// ============================================================================
// PE ERROR
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeError {
    InvalidDosHeader,
    InvalidPeSignature,
    InvalidMachine,
    InvalidOptionalHeader,
    InvalidSection,
    NotPe64,
    NotExecutable,
    ImportNotFound,
    RelocationFailed,
    MemoryAllocation,
    EntryNotFound,
    DllNotFound,
    SymbolNotFound,
    InvalidExport,
}

// ============================================================================
// DOS HEADER
// ============================================================================

/// DOS Header (64 bytes)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct ImageDosHeader {
    pub e_magic: u16,           // 0x00: Magic number (MZ)
    pub e_cblp: u16,            // 0x02: Bytes on last page
    pub e_cp: u16,              // 0x04: Pages in file
    pub e_crlc: u16,            // 0x06: Relocations
    pub e_cparhdr: u16,         // 0x08: Size of header in paragraphs
    pub e_minalloc: u16,        // 0x0A: Minimum extra paragraphs
    pub e_maxalloc: u16,        // 0x0C: Maximum extra paragraphs
    pub e_ss: u16,              // 0x0E: Initial SS value
    pub e_sp: u16,              // 0x10: Initial SP value
    pub e_csum: u16,            // 0x12: Checksum
    pub e_ip: u16,              // 0x14: Initial IP value
    pub e_cs: u16,              // 0x16: Initial CS value
    pub e_lfarlc: u16,          // 0x18: File address of relocation table
    pub e_ovno: u16,            // 0x1A: Overlay number
    pub e_res: [u16; 4],        // 0x1C: Reserved
    pub e_oemid: u16,           // 0x24: OEM identifier
    pub e_oeminfo: u16,         // 0x26: OEM information
    pub e_res2: [u16; 10],      // 0x28: Reserved
    pub e_lfanew: u32,          // 0x3C: File address of new exe header
}

// ============================================================================
// PE FILE HEADER
// ============================================================================

/// PE File Header (20 bytes)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct ImageFileHeader {
    pub machine: u16,           // 0x00: Machine type
    pub number_of_sections: u16, // 0x02: Number of sections
    pub time_date_stamp: u32,   // 0x04: Timestamp
    pub pointer_to_symbol_table: u32, // 0x08: Symbol table pointer
    pub number_of_symbols: u32, // 0x0C: Number of symbols
    pub size_of_optional_header: u16, // 0x10: Size of optional header
    pub characteristics: u16,   // 0x12: Characteristics
}

/// Machine types
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineType {
    Unknown = 0x0000,
    I386 = 0x014C,
    AMD64 = 0x8664,
    ARM = 0x01C0,
    ARM64 = 0xAA64,
}

impl MachineType {
    pub fn from_u16(value: u16) -> Self {
        match value {
            0x014C => MachineType::I386,
            0x8664 => MachineType::AMD64,
            0x01C0 => MachineType::ARM,
            0xAA64 => MachineType::ARM64,
            _ => MachineType::Unknown,
        }
    }
}

// ============================================================================
// PE OPTIONAL HEADER (PE32+)
// ============================================================================

/// PE32+ Optional Header (240 bytes for PE32+)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct ImageOptionalHeader64 {
    pub magic: u16,                     // 0x00: Magic (0x20B for PE32+)
    pub major_linker_version: u8,       // 0x02: Linker major version
    pub minor_linker_version: u8,       // 0x03: Linker minor version
    pub size_of_code: u32,              // 0x04: Size of code section
    pub size_of_initialized_data: u32,  // 0x08: Size of initialized data
    pub size_of_uninitialized_data: u32, // 0x0C: Size of uninitialized data
    pub address_of_entry_point: u32,    // 0x10: Entry point RVA
    pub base_of_code: u32,              // 0x14: Base of code RVA
    pub image_base: u64,                // 0x18: Image base (64-bit)
    pub section_alignment: u32,         // 0x20: Section alignment
    pub file_alignment: u32,            // 0x24: File alignment
    pub major_operating_system_version: u16, // 0x28: OS major version
    pub minor_operating_system_version: u16, // 0x2A: OS minor version
    pub major_image_version: u16,       // 0x2C: Image major version
    pub minor_image_version: u16,       // 0x2E: Image minor version
    pub major_subsystem_version: u16,   // 0x30: Subsystem major version
    pub minor_subsystem_version: u16,   // 0x32: Subsystem minor version
    pub win32_version_value: u32,       // 0x34: Win32 version value
    pub size_of_image: u32,             // 0x38: Size of image
    pub size_of_headers: u32,           // 0x3C: Size of headers
    pub check_sum: u32,                 // 0x40: Checksum
    pub subsystem: u16,                 // 0x44: Subsystem
    pub dll_characteristics: u16,       // 0x46: DLL characteristics
    pub size_of_stack_reserve: u64,     // 0x48: Size of stack reserve
    pub size_of_stack_commit: u64,      // 0x50: Size of stack commit
    pub size_of_heap_reserve: u64,      // 0x58: Size of heap reserve
    pub size_of_heap_commit: u64,       // 0x60: Size of heap commit
    pub loader_flags: u32,              // 0x68: Loader flags
    pub number_of_rva_and_sizes: u32,   // 0x6C: Number of RVA and sizes
    // Data directories follow (16 entries, 8 bytes each)
}

/// Data Directory entry
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ImageDataDirectory {
    pub virtual_address: u32,
    pub size: u32,
}

// ============================================================================
// SECTION HEADER
// ============================================================================

/// Section Header (40 bytes)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct ImageSectionHeader {
    pub name: [u8; 8],           // 0x00: Section name
    pub virtual_size: u32,       // 0x08: Virtual size
    pub virtual_address: u32,    // 0x0C: Virtual address (RVA)
    pub size_of_raw_data: u32,   // 0x10: Size of raw data
    pub pointer_to_raw_data: u32, // 0x14: Pointer to raw data
    pub pointer_to_relocations: u32, // 0x18: Pointer to relocations
    pub pointer_to_linenumbers: u32, // 0x1C: Pointer to line numbers
    pub number_of_relocations: u16, // 0x20: Number of relocations
    pub number_of_linenumbers: u16, // 0x22: Number of line numbers
    pub characteristics: u32,    // 0x24: Characteristics
}

impl ImageSectionHeader {
    pub fn name_as_string(&self) -> String {
        let mut name = String::new();
        for &b in &self.name {
            if b == 0 {
                break;
            }
            name.push(b as char);
        }
        name
    }
}

// ============================================================================
// IMPORT TABLE
// ============================================================================

/// Import Directory Entry
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct ImageImportDescriptor {
    pub original_first_thunk: u32,  // 0x00: Original first thunk (RVA)
    pub time_date_stamp: u32,       // 0x04: Time date stamp
    pub forwarder_chain: u32,       // 0x08: Forwarder chain
    pub name: u32,                  // 0x0C: Name RVA
    pub first_thunk: u32,           // 0x10: First thunk (RVA)
}

/// Import Lookup (64-bit)
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct ImageThunkData64 {
    pub ordinal_or_address: u64,
}

impl ImageThunkData64 {
    pub fn is_ordinal(&self) -> bool {
        (self.ordinal_or_address & (1 << 63)) != 0
    }
    
    pub fn ordinal(&self) -> u16 {
        (self.ordinal_or_address & 0xFFFF) as u16
    }
    
    pub fn hint_name_rva(&self) -> u32 {
        (self.ordinal_or_address & 0x7FFFFFFF) as u32
    }
}

/// Import Hint/Name entry
#[repr(C, packed)]
pub struct ImageImportHintName {
    pub hint: u16,
    // Followed by null-terminated function name
}

// ============================================================================
// EXPORT TABLE
// ============================================================================

/// Export Directory Table
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct ImageExportDirectory {
    pub characteristics: u32,       // 0x00: Characteristics
    pub time_date_stamp: u32,      // 0x04: Time date stamp
    pub major_version: u16,        // 0x08: Major version
    pub minor_version: u16,        // 0x0A: Minor version
    pub name: u32,                 // 0x0C: Name RVA
    pub base: u32,                 // 0x10: Export ordinal base
    pub number_of_functions: u32,  // 0x14: Number of functions
    pub number_of_names: u32,      // 0x18: Number of names
    pub address_of_functions: u32, // 0x1C: Address of functions (RVA)
    pub address_of_names: u32,     // 0x20: Address of names (RVA)
    pub address_of_name_ordinals: u32, // 0x24: Address of name ordinals (RVA)
}

// ============================================================================
// BASE RELOCATION
// ============================================================================

/// Base Relocation Block
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct ImageBaseRelocation {
    pub virtual_address: u32,  // 0x00: Page RVA
    pub size_of_block: u32,    // 0x04: Block size
}

/// Relocation types (stored in high 4 bits of each entry)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelocationType {
    Absolute = 0,
    High = 1,
    Low = 2,
    HighLow = 3,
    Dir64 = 10,  // For PE32+
}

// ============================================================================
// PE IMAGE
// ============================================================================

/// Loaded PE Image
#[derive(Clone, Debug)]
pub struct PeImage {
    /// Image base address
    pub image_base: u64,
    /// Entry point address
    pub entry_point: u64,
    /// Image size
    pub image_size: u32,
    /// Sections
    pub sections: Vec<PeSection>,
    /// Imports
    pub imports: Vec<ImportEntry>,
    /// Exports
    pub exports: BTreeMap<String, u64>,
    /// Is DLL
    pub is_dll: bool,
    /// Machine type
    pub machine: MachineType,
}

/// Loaded section
#[derive(Clone, Debug)]
pub struct PeSection {
    pub name: String,
    pub virtual_address: u32,
    pub virtual_size: u32,
    pub raw_data: Vec<u8>,
    pub characteristics: u32,
    pub is_code: bool,
    pub is_data: bool,
    pub is_readable: bool,
    pub is_writable: bool,
    pub is_executable: bool,
}

/// Import entry
#[derive(Clone, Debug)]
pub struct ImportEntry {
    pub dll_name: String,
    pub functions: Vec<ImportFunction>,
}

/// Import function
#[derive(Clone, Debug)]
pub struct ImportFunction {
    pub name: String,
    pub ordinal: Option<u16>,
    pub thunk_address: u64,
    pub resolved_address: Option<u64>,
}

// ============================================================================
// PE LOADER
// ============================================================================

pub struct PeLoader {
    /// Loaded DLLs
    loaded_dlls: BTreeMap<String, Arc<Mutex<PeImage>>>,
}

impl PeLoader {
    pub fn new() -> Self {
        PeLoader {
            loaded_dlls: BTreeMap::new(),
        }
    }
    
    /// Load PE from raw data
    pub fn load(&mut self, data: &[u8]) -> Result<PeImage, PeError> {
        // Parse DOS header
        if data.len() < size_of::<ImageDosHeader>() {
            return Err(PeError::InvalidDosHeader);
        }
        
        let dos_header = unsafe { &*(data.as_ptr() as *const ImageDosHeader) };
        
        if dos_header.e_magic != DOS_MAGIC {
            return Err(PeError::InvalidDosHeader);
        }
        
        // Get PE header offset
        let pe_offset = dos_header.e_lfanew as usize;
        if pe_offset + 4 > data.len() {
            return Err(PeError::InvalidPeSignature);
        }
        
        // Check PE signature
        let pe_sig = read_u32(&data[pe_offset..]);
        if pe_sig != PE_SIGNATURE {
            return Err(PeError::InvalidPeSignature);
        }
        
        // Parse file header
        let file_header_offset = pe_offset + 4;
        if file_header_offset + size_of::<ImageFileHeader>() > data.len() {
            return Err(PeError::InvalidPeSignature);
        }
        
        let file_header = unsafe {
            &*(data.as_ptr().add(file_header_offset) as *const ImageFileHeader)
        };
        
        // Check machine type
        let machine = MachineType::from_u16(file_header.machine);
        if machine != MachineType::AMD64 {
            return Err(PeError::NotPe64);
        }
        
        // Check if DLL
        let is_dll = (file_header.characteristics & IMAGE_FILE_DLL) != 0;
        
        // Parse optional header
        let optional_offset = file_header_offset + size_of::<ImageFileHeader>();
        if optional_offset + size_of::<ImageOptionalHeader64>() > data.len() {
            return Err(PeError::InvalidOptionalHeader);
        }
        
        let optional_header = unsafe {
            &*(data.as_ptr().add(optional_offset) as *const ImageOptionalHeader64)
        };
        
        // Check magic (must be PE32+)
        if optional_header.magic != PE32_PLUS_MAGIC {
            return Err(PeError::NotPe64);
        }
        
        // Parse sections
        let section_offset = optional_offset + file_header.size_of_optional_header as usize;
        let num_sections = file_header.number_of_sections as usize;
        let mut sections = Vec::with_capacity(num_sections);
        
        for i in 0..num_sections {
            let sec_offset = section_offset + i * size_of::<ImageSectionHeader>();
            if sec_offset + size_of::<ImageSectionHeader>() > data.len() {
                return Err(PeError::InvalidSection);
            }
            
            let sec_header = unsafe {
                &*(data.as_ptr().add(sec_offset) as *const ImageSectionHeader)
            };
            
            // Read section data
            let raw_size = sec_header.size_of_raw_data as usize;
            let raw_offset = sec_header.pointer_to_raw_data as usize;
            let raw_data = if raw_offset + raw_size <= data.len() {
                data[raw_offset..raw_offset + raw_size].to_vec()
            } else {
                vec![0u8; raw_size]
            };
            
            let section = PeSection {
                name: sec_header.name_as_string(),
                virtual_address: sec_header.virtual_address,
                virtual_size: sec_header.virtual_size,
                raw_data,
                characteristics: sec_header.characteristics,
                is_code: (sec_header.characteristics & IMAGE_SCN_CNT_CODE) != 0,
                is_data: (sec_header.characteristics & IMAGE_SCN_CNT_INITIALIZED_DATA) != 0,
                is_readable: (sec_header.characteristics & IMAGE_SCN_MEM_READ) != 0,
                is_writable: (sec_header.characteristics & IMAGE_SCN_MEM_WRITE) != 0,
                is_executable: (sec_header.characteristics & IMAGE_SCN_MEM_EXECUTE) != 0,
            };
            
            sections.push(section);
        }
        
        // Parse imports (simplified)
        let imports = self.parse_imports(data, optional_offset, optional_header)?;
        
        // Parse exports (simplified)
        let exports = self.parse_exports(data, optional_offset, optional_header)?;
        
        let image = PeImage {
            image_base: optional_header.image_base,
            entry_point: optional_header.image_base + optional_header.address_of_entry_point as u64,
            image_size: optional_header.size_of_image,
            sections,
            imports,
            exports,
            is_dll,
            machine,
        };
        
        Ok(image)
    }
    
    /// Parse import table
    fn parse_imports(
        &self,
        data: &[u8],
        optional_offset: usize,
        optional_header: &ImageOptionalHeader64,
    ) -> Result<Vec<ImportEntry>, PeError> {
        let mut imports = Vec::new();
        
        // Get import directory
        let import_dir_offset = optional_offset + 112; // After optional header fields
        if import_dir_offset + size_of::<ImageDataDirectory>() > data.len() {
            return Ok(imports);
        }
        
        let import_dir = unsafe {
            &*(data.as_ptr().add(import_dir_offset) as *const ImageDataDirectory)
        };
        
        if import_dir.virtual_address == 0 {
            return Ok(imports);
        }
        
        // Find import directory in sections
        let import_rva = import_dir.virtual_address;
        
        // Parse import descriptors (simplified)
        // In real implementation, we'd iterate through descriptors
        
        Ok(imports)
    }
    
    /// Parse export table
    fn parse_exports(
        &self,
        data: &[u8],
        optional_offset: usize,
        optional_header: &ImageOptionalHeader64,
    ) -> Result<BTreeMap<String, u64>, PeError> {
        let mut exports = BTreeMap::new();
        
        // Get export directory
        let export_dir_offset = optional_offset + 96; // First data directory
        if export_dir_offset + size_of::<ImageDataDirectory>() > data.len() {
            return Ok(exports);
        }
        
        let export_dir = unsafe {
            &*(data.as_ptr().add(export_dir_offset) as *const ImageDataDirectory)
        };
        
        if export_dir.virtual_address == 0 {
            return Ok(exports);
        }
        
        // Parse exports (simplified)
        
        Ok(exports)
    }
    
    /// Get or load DLL
    pub fn get_dll(&mut self, name: &str) -> Option<Arc<Mutex<PeImage>>> {
        self.loaded_dlls.get(name).cloned()
    }
    
    /// Register loaded DLL
    pub fn register_dll(&mut self, name: String, image: PeImage) {
        self.loaded_dlls.insert(name, Arc::new(Mutex::new(image)));
    }
    
    /// Resolve import
    pub fn resolve_import(&mut self, dll_name: &str, func_name: &str) -> Option<u64> {
        // Check loaded DLLs
        if let Some(dll) = self.loaded_dlls.get(dll_name) {
            let dll = dll.lock();
            return dll.exports.get(func_name).copied();
        }
        
        // Try Win32 API emulation
        crate::win32::get_proc_address(dll_name, func_name)
    }
}

impl Default for PeLoader {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

fn read_u16(data: &[u8]) -> u16 {
    u16::from_le_bytes([data[0], data[1]])
}

fn read_u32(data: &[u8]) -> u32 {
    u32::from_le_bytes([data[0], data[1], data[2], data[3]])
}

fn read_u64(data: &[u8]) -> u64 {
    u64::from_le_bytes([
        data[0], data[1], data[2], data[3],
        data[4], data[5], data[6], data[7],
    ])
}

// ============================================================================
// GLOBAL LOADER
// ============================================================================

static PE_LOADER: Mutex<PeLoader> = Mutex::new(PeLoader {
    loaded_dlls: BTreeMap::new(),
});

/// Load PE executable
pub fn load_pe(data: &[u8]) -> Result<PeImage, PeError> {
    PE_LOADER.lock().load(data)
}

/// Get loaded DLL
pub fn get_dll(name: &str) -> Option<Arc<Mutex<PeImage>>> {
    PE_LOADER.lock().get_dll(name)
}

/// Resolve import
pub fn resolve_import(dll_name: &str, func_name: &str) -> Option<u64> {
    PE_LOADER.lock().resolve_import(dll_name, func_name)
}
