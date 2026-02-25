//! # Dynamic Linking
//!
//! ELF dynamic loader and dlopen/dlsym support.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::{AtomicU32, AtomicU64, AtomicPtr, Ordering};
use spin::Mutex;

// ============================================================================
// ELF CONSTANTS
// ============================================================================

/// ELF magic
pub const ELFMAG: [u8; 4] = [0x7F, b'E', b'L', b'F'];

/// ELF class
pub const ELFCLASS64: u8 = 2;

/// ELF data encoding
pub const ELFDATA2LSB: u8 = 1;

/// Section types
pub const SHT_NULL: u32 = 0;
pub const SHT_PROGBITS: u32 = 1;
pub const SHT_SYMTAB: u32 = 2;
pub const SHT_STRTAB: u32 = 3;
pub const SHT_RELA: u32 = 4;
pub const SHT_HASH: u32 = 5;
pub const SHT_DYNAMIC: u32 = 6;
pub const SHT_NOTE: u32 = 7;
pub const SHT_NOBITS: u32 = 8;
pub const SHT_REL: u32 = 9;
pub const SHT_DYNSYM: u32 = 11;

/// Program header types
pub const PT_NULL: u32 = 0;
pub const PT_LOAD: u32 = 1;
pub const PT_DYNAMIC: u32 = 2;
pub const PT_INTERP: u32 = 3;
pub const PT_NOTE: u32 = 4;
pub const PT_PHDR: u32 = 6;
pub const PT_GNU_EH_FRAME: u32 = 0x6474e550;
pub const PT_GNU_STACK: u32 = 0x6474e551;
pub const PT_GNU_RELRO: u32 = 0x6474e552;

/// Dynamic tags
pub const DT_NULL: i64 = 0;
pub const DT_NEEDED: i64 = 1;
pub const DT_PLTRELSZ: i64 = 2;
pub const DT_PLTGOT: i64 = 3;
pub const DT_HASH: i64 = 4;
pub const DT_STRTAB: i64 = 5;
pub const DT_SYMTAB: i64 = 6;
pub const DT_RELA: i64 = 7;
pub const DT_RELASZ: i64 = 8;
pub const DT_RELAENT: i64 = 9;
pub const DT_STRSZ: i64 = 10;
pub const DT_SYMENT: i64 = 11;
pub const DT_INIT: i64 = 12;
pub const DT_FINI: i64 = 13;
pub const DT_SONAME: i64 = 14;
pub const DT_RPATH: i64 = 15;
pub const DT_RUNPATH: i64 = 29;
pub const DT_FLAGS: i64 = 30;
pub const DT_GNU_HASH: i64 = 0x6ffffef5;
pub const DT_VERSYM: i64 = 0x6ffffff0;
pub const DT_RELACOUNT: i64 = 0x6ffffff9;
pub const DT_FLAGS_1: i64 = 0x6ffffffb;

/// Relocation types (x86_64)
pub const R_X86_64_NONE: u32 = 0;
pub const R_X86_64_64: u32 = 1;
pub const R_X86_64_PC32: u32 = 2;
pub const R_X86_64_GOT32: u32 = 3;
pub const R_X86_64_PLT32: u32 = 4;
pub const R_X86_64_COPY: u32 = 5;
pub const R_X86_64_GLOB_DAT: u32 = 6;
pub const R_X86_64_JUMP_SLOT: u32 = 7;
pub const R_X86_64_RELATIVE: u32 = 8;
pub const R_X86_64_GOTPCREL: u32 = 9;
pub const R_X86_64_32: u32 = 10;
pub const R_X86_64_IRELATIVE: u32 = 37;

/// Symbol binding
pub const STB_LOCAL: u8 = 0;
pub const STB_GLOBAL: u8 = 1;
pub const STB_WEAK: u8 = 2;

/// Symbol type
pub const STT_NOTYPE: u8 = 0;
pub const STT_OBJECT: u8 = 1;
pub const STT_FUNC: u8 = 2;
pub const STT_SECTION: u8 = 3;

// ============================================================================
// ELF HEADERS
// ============================================================================

#[repr(C)]
pub struct Elf64Ehdr {
    pub e_ident: [u8; 16],
    pub e_type: u16,
    pub e_machine: u16,
    pub e_version: u32,
    pub e_entry: u64,
    pub e_phoff: u64,
    pub e_shoff: u64,
    pub e_flags: u32,
    pub e_ehsize: u16,
    pub e_phentsize: u16,
    pub e_phnum: u16,
    pub e_shentsize: u16,
    pub e_shnum: u16,
    pub e_shstrndx: u16,
}

#[repr(C)]
pub struct Elf64Phdr {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

#[repr(C)]
pub struct Elf64Shdr {
    pub sh_name: u32,
    pub sh_type: u32,
    pub sh_flags: u64,
    pub sh_addr: u64,
    pub sh_offset: u64,
    pub sh_size: u64,
    pub sh_link: u32,
    pub sh_info: u32,
    pub sh_addralign: u64,
    pub sh_entsize: u64,
}

#[repr(C)]
pub struct Elf64Sym {
    pub st_name: u32,
    pub st_info: u8,
    pub st_other: u8,
    pub st_shndx: u16,
    pub st_value: u64,
    pub st_size: u64,
}

#[repr(C)]
pub struct Elf64Rela {
    pub r_offset: u64,
    pub r_info: u64,
    pub r_addend: i64,
}

#[repr(C)]
pub struct Elf64Dyn {
    pub d_tag: i64,
    pub d_val: u64,
}

// ============================================================================
// LOADED LIBRARY
// ============================================================================

pub struct LoadedLibrary {
    /// Library name
    pub name: String,
    /// Base address
    pub base: AtomicU64,
    /// Size
    pub size: u64,
    /// Entry point
    pub entry: u64,
    /// Symbol table
    pub symbols: Mutex<BTreeMap<String, u64>>,
    /// Reference count
    pub ref_count: AtomicU32,
    /// Init function
    pub init: AtomicU64,
    /// Fini function
    pub fini: AtomicU64,
    /// Dependencies
    pub needed: Mutex<Vec<String>>,
    /// TLS index
    pub tls_modid: AtomicU32,
    /// Is global
    pub is_global: AtomicU32,
}

impl LoadedLibrary {
    pub fn new(name: &str, base: u64, size: u64) -> Self {
        Self {
            name: String::from(name),
            base: AtomicU64::new(base),
            size,
            entry: 0,
            symbols: Mutex::new(BTreeMap::new()),
            ref_count: AtomicU32::new(1),
            init: AtomicU64::new(0),
            fini: AtomicU64::new(0),
            needed: Mutex::new(Vec::new()),
            tls_modid: AtomicU32::new(0),
            is_global: AtomicU32::new(0),
        }
    }

    /// Add symbol
    pub fn add_symbol(&self, name: &str, addr: u64) {
        self.symbols.lock().insert(String::from(name), addr);
    }

    /// Lookup symbol
    pub fn lookup(&self, name: &str) -> Option<u64> {
        self.symbols.lock().get(name).copied()
    }
}

// ============================================================================
// DYNAMIC LOADER
// ============================================================================

pub struct DynamicLoader {
    /// Loaded libraries
    libraries: Mutex<BTreeMap<String, Arc<LoadedLibrary>>>,
    /// Library handles (for dlopen)
    handles: Mutex<BTreeMap<u32, Arc<LoadedLibrary>>>,
    /// Next handle ID
    next_handle: AtomicU32,
    /// Search paths
    search_paths: Mutex<Vec<String>>,
    /// Statistics
    stats: Mutex<DlStats>,
}

#[derive(Clone, Debug, Default)]
pub struct DlStats {
    pub libraries_loaded: u32,
    pub symbols_resolved: u64,
    pub relocations_applied: u64,
}

impl DynamicLoader {
    pub const fn new() -> Self {
        Self {
            libraries: Mutex::new(BTreeMap::new()),
            handles: Mutex::new(BTreeMap::new()),
            next_handle: AtomicU32::new(1),
            search_paths: Mutex::new(Vec::new()),
            stats: Mutex::new(DlStats::default()),
        }
    }

    /// Initialize with default paths
    pub fn init(&self) {
        let mut paths = self.search_paths.lock();
        paths.push(String::from("/lib"));
        paths.push(String::from("/usr/lib"));
        paths.push(String::from("/usr/local/lib"));
        
        crate::serial_println!("[DLOPEN] Dynamic loader initialized");
    }

    /// Load library
    pub fn dlopen(&self, filename: &str, flags: i32) -> Result<u32, DlError> {
        // Check if already loaded
        if let Some(lib) = self.libraries.lock().get(filename) {
            lib.ref_count.fetch_add(1, Ordering::SeqCst);
            let handle = self.next_handle.fetch_add(1, Ordering::SeqCst);
            self.handles.lock().insert(handle, lib.clone());
            return Ok(handle);
        }
        
        // Load ELF file
        let data = self.load_file(filename)?;
        
        // Parse ELF
        let lib = self.load_elf(&data, filename)?;
        
        // Apply relocations
        self.apply_relocations(&lib, &data)?;
        
        // Call init
        self.call_init(&lib);
        
        // Register
        let handle = self.next_handle.fetch_add(1, Ordering::SeqCst);
        self.libraries.lock().insert(String::from(filename), lib.clone());
        self.handles.lock().insert(handle, lib.clone());
        
        if flags & 0x00100 != 0 { // RTLD_GLOBAL
            lib.is_global.store(1, Ordering::SeqCst);
        }
        
        let mut stats = self.stats.lock();
        stats.libraries_loaded += 1;
        
        Ok(handle)
    }

    /// Load ELF from data
    fn load_elf(&self, data: &[u8], name: &str) -> Result<Arc<LoadedLibrary>, DlError> {
        if data.len() < core::mem::size_of::<Elf64Ehdr>() {
            return Err(DlError::InvalidElf);
        }
        
        let ehdr = unsafe { &*(data.as_ptr() as *const Elf64Ehdr) };
        
        // Verify magic
        if ehdr.e_ident[0..4] != ELFMAG {
            return Err(DlError::InvalidElf);
        }
        
        // Verify class
        if ehdr.e_ident[4] != ELFCLASS64 {
            return Err(DlError::InvalidElf);
        }
        
        // Calculate total size
        let mut total_size = 0u64;
        let mut base_addr = u64::MAX;
        
        for i in 0..ehdr.e_phnum as usize {
            let phdr = self.get_phdr(data, i)?;
            
            if phdr.p_type == PT_LOAD {
                if phdr.p_vaddr < base_addr {
                    base_addr = phdr.p_vaddr;
                }
                let end = phdr.p_vaddr + phdr.p_memsz;
                if end > total_size {
                    total_size = end;
                }
            }
        }
        
        // Allocate memory (simplified - would use mmap)
        let load_base = 0x7F0000000000u64;
        
        let lib = Arc::new(LoadedLibrary::new(name, load_base, total_size));
        
        // Load segments
        for i in 0..ehdr.e_phnum as usize {
            let phdr = self.get_phdr(data, i)?;
            
            if phdr.p_type == PT_LOAD {
                // Copy segment data
                let file_start = phdr.p_offset as usize;
                let file_end = core::cmp::min(file_start + phdr.p_filesz as usize, data.len());
                
                // Would copy to memory at load_base + phdr.p_vaddr
            }
        }
        
        // Parse dynamic section
        self.parse_dynamic(&lib, data, ehdr)?;
        
        Ok(lib)
    }

    fn get_phdr(&self, data: &[u8], index: usize) -> Result<&'static Elf64Phdr, DlError> {
        let ehdr = unsafe { &*(data.as_ptr() as *const Elf64Ehdr) };
        
        let offset = ehdr.e_phoff as usize + index * ehdr.e_phentsize as usize;
        
        if offset + core::mem::size_of::<Elf64Phdr>() > data.len() {
            return Err(DlError::InvalidElf);
        }
        
        Ok(unsafe { &*(data.as_ptr().add(offset) as *const Elf64Phdr) })
    }

    fn parse_dynamic(&self, lib: &LoadedLibrary, data: &[u8], ehdr: &Elf64Ehdr) -> Result<(), DlError> {
        for i in 0..ehdr.e_phnum as usize {
            let phdr = self.get_phdr(data, i)?;
            
            if phdr.p_type == PT_DYNAMIC {
                // Parse dynamic entries
                let dyn_offset = phdr.p_offset as usize;
                let dyn_size = phdr.p_filesz as usize;
                
                let mut offset = dyn_offset;
                while offset + core::mem::size_of::<Elf64Dyn>() <= dyn_offset + dyn_size {
                    let dyn_entry = unsafe {
                        &*(data.as_ptr().add(offset) as *const Elf64Dyn)
                    };
                    
                    match dyn_entry.d_tag {
                        DT_NEEDED => {
                            // Add dependency
                        }
                        DT_INIT => {
                            lib.init.store(dyn_entry.d_val, Ordering::SeqCst);
                        }
                        DT_FINI => {
                            lib.fini.store(dyn_entry.d_val, Ordering::SeqCst);
                        }
                        DT_NULL => break,
                        _ => {}
                    }
                    
                    offset += core::mem::size_of::<Elf64Dyn>();
                }
            }
        }
        
        Ok(())
    }

    fn apply_relocations(&self, lib: &LoadedLibrary, _data: &[u8]) -> Result<(), DlError> {
        // Apply RELA relocations
        let mut stats = self.stats.lock();
        stats.relocations_applied += 1;
        
        Ok(())
    }

    fn call_init(&self, lib: &LoadedLibrary) {
        let init = lib.init.load(Ordering::SeqCst);
        if init != 0 {
            // Call init function
            crate::serial_println!("[DLOPEN] Calling init at {:#x}", init);
        }
    }

    /// Lookup symbol
    pub fn dlsym(&self, handle: u32, symbol: &str) -> Result<u64, DlError> {
        let handles = self.handles.lock();
        let lib = handles.get(&handle).ok_or(DlError::InvalidHandle)?;
        
        let addr = lib.lookup(symbol);
        
        let mut stats = self.stats.lock();
        stats.symbols_resolved += 1;
        
        addr.ok_or(DlError::SymbolNotFound)
    }

    /// Close library
    pub fn dlclose(&self, handle: u32) -> Result<(), DlError> {
        let mut handles = self.handles.lock();
        
        if let Some(lib) = handles.remove(&handle) {
            lib.ref_count.fetch_sub(1, Ordering::SeqCst);
            
            // Call fini if last reference
            if lib.ref_count.load(Ordering::SeqCst) == 0 {
                let fini = lib.fini.load(Ordering::SeqCst);
                if fini != 0 {
                    // Call fini
                }
            }
            
            return Ok(());
        }
        
        Err(DlError::InvalidHandle)
    }

    /// Load file (placeholder)
    fn load_file(&self, _filename: &str) -> Result<Vec<u8>, DlError> {
        // Would load from filesystem
        Ok(vec![0u8; 4096])
    }

    /// Get statistics
    pub fn get_stats(&self) -> DlStats {
        self.stats.lock().clone()
    }
}

lazy_static::lazy_static! {
    pub static ref DYN_LOADER: DynamicLoader = DynamicLoader::new();
}

// ============================================================================
// ERROR TYPE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DlError {
    InvalidElf,
    InvalidHandle,
    SymbolNotFound,
    FileNotFound,
    RelocationFailed,
    DependencyNotFound,
}

// ============================================================================
// SYSCALL INTERFACE
// ============================================================================

pub fn sys_dlopen(filename: &str, flags: i32) -> i64 {
    match DYN_LOADER.dlopen(filename, flags) {
        Ok(handle) => handle as i64,
        Err(_) => -1,
    }
}

pub fn sys_dlsym(handle: u32, symbol: &str) -> i64 {
    match DYN_LOADER.dlsym(handle, symbol) {
        Ok(addr) => addr as i64,
        Err(_) => 0,
    }
}

pub fn sys_dlclose(handle: u32) -> i32 {
    match DYN_LOADER.dlclose(handle) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init() {
    DYN_LOADER.init();
}
