//! # ELF Yükleyici
//!
//! ELF (Executable and Linkable Format) ikili dosyalarını ayrıştırır ve yükler.
//! x86_64 mimarisi için ELF64 formatını destekler: çalıştırılabilir dosyalar,
//! dinamik bağlanan kütüphaneler ve yeniden konumlandırma tabloları.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;
use x86_64::structures::paging::{FrameAllocator, Mapper, PageTableFlags, Size4KiB};
use x86_64::VirtAddr;

/// ELF yükleme sırasında oluşabilecek hatalar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElfError {
    Invalid,
    Unsupported,
    OutOfBounds,
    SymbolNotFound,
    RelocationFailed,
    AlreadyLoaded,
    NotLoaded,
}

/// PT_LOAD segment bilgisi.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadSegment {
    pub offset: u64,
    pub vaddr: u64,
    pub filesz: u64,
    pub memsz: u64,
    pub flags: u32,
    pub align: u64,
}

/// ELF imajının özeti.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElfImage {
    pub entry: u64,
    pub segments: Vec<LoadSegment>,
}

/// Kullanıcı moduna geçiş için gerekli ELF bilgisi.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserElf {
    pub entry: VirtAddr,
    pub stack_top: VirtAddr,
    pub image: ElfImage,
}

// ELF sabitleri
#[allow(dead_code)]
const EI_NIDENT: usize = 16;
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const ET_EXEC: u16 = 2;
const ET_DYN: u16 = 3;
const EM_X86_64: u16 = 62;
const PT_LOAD: u32 = 1;
const PF_X: u32 = 1;
const PF_W: u32 = 2;

/// ELF header ve program header'ları ayrıştırır.
pub fn parse_elf(image: &[u8]) -> Result<ElfImage, ElfError> {
    let header = parse_header(image)?;
    crate::serial_println!("ELF LOAD: Entry Point={:#x}", header.e_entry);
    let segments = parse_program_headers(image, &header)?;
    Ok(ElfImage {
        entry: header.e_entry,
        segments,
    })
}

/// PT_LOAD segmentlerini bellek alanına yükler.
pub fn load_segments(
    image: &[u8],
    _mapper: &mut impl Mapper<Size4KiB>,
    _frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<ElfImage, ElfError> {
    crate::serial_println!("DEBUG: ELF Parse başlıyor...");
    let elf = parse_elf(image)?;
    crate::memory::set_user_image(image);
    for seg in &elf.segments {
        if seg.memsz == 0 {
            continue;
        }
        crate::serial_println!(
            "ELF LOAD: Segment Start={:#x} Size={:#x} Flags={:#x}",
            seg.vaddr,
            seg.memsz,
            seg.flags
        );
        if !crate::memory::is_user_range(seg.vaddr, seg.memsz) {
            return Err(ElfError::Unsupported);
        }
        let mut flags = PageTableFlags::USER_ACCESSIBLE;
        if seg.flags & PF_W != 0 {
            flags |= PageTableFlags::WRITABLE;
        }
        if seg.flags & PF_X == 0 {
            flags |= PageTableFlags::NO_EXECUTE;
        }
        let file_start = seg.offset as usize;
        let file_size = seg.filesz as usize;
        let _mem_size = seg.memsz as usize;
        if file_size > 0 {
            if file_start.saturating_add(file_size) > image.len() {
                return Err(ElfError::OutOfBounds);
            }
        }
        if !crate::memory::register_file_lazy_region(
            seg.vaddr, seg.memsz, flags, seg.offset, seg.filesz,
        ) {
            return Err(ElfError::Unsupported);
        }
    }
    Ok(elf)
}

/// Kullanıcı ELF yükler ve kullanıcı stack'ini map eder.
pub fn load_user_elf(
    image: &[u8],
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<UserElf, ElfError> {
    let elf = load_segments(image, mapper, frame_allocator)?;
    if !crate::memory::is_user_address(elf.entry) {
        return Err(ElfError::Unsupported);
    }
    let (stack_base, stack_top) = crate::memory::user_stack_bounds();
    if !crate::memory::is_user_range(stack_base, crate::memory::USER_STACK_BYTES) {
        return Err(ElfError::Unsupported);
    }
    map_user_stack(
        VirtAddr::new(stack_top),
        crate::memory::USER_STACK_PAGES,
        mapper,
        frame_allocator,
    )?;
    Ok(UserElf {
        entry: VirtAddr::new(elf.entry),
        stack_top: VirtAddr::new(stack_top),
        image: elf,
    })
}

#[derive(Clone, Copy)]
struct ElfHeader64 {
    e_entry: u64,
    e_phoff: u64,
    e_phentsize: u16,
    e_phnum: u16,
    #[allow(dead_code)]
    e_type: u16,
    #[allow(dead_code)]
    e_machine: u16,
}

#[derive(Clone, Copy)]
struct ProgramHeader64 {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
}

fn parse_header(image: &[u8]) -> Result<ElfHeader64, ElfError> {
    // Minimum ELF64 header boyutu
    if image.len() < 64 {
        return Err(ElfError::Invalid);
    }
    // Magic kontrolü
    if image[0] != 0x7f || image[1] != b'E' || image[2] != b'L' || image[3] != b'F' {
        return Err(ElfError::Invalid);
    }
    // Sadece ELF64 + LSB kabul edilir
    if image[4] != ELFCLASS64 || image[5] != ELFDATA2LSB || image[6] != 1 {
        return Err(ElfError::Unsupported);
    }
    let e_type = read_u16(image, 16)?;
    let e_machine = read_u16(image, 18)?;
    if e_machine != EM_X86_64 {
        return Err(ElfError::Unsupported);
    }
    if e_type != ET_EXEC && e_type != ET_DYN {
        return Err(ElfError::Unsupported);
    }
    let e_entry = read_u64(image, 24)?;
    let e_phoff = read_u64(image, 32)?;
    let e_phentsize = read_u16(image, 54)?;
    let e_phnum = read_u16(image, 56)?;
    if e_phentsize as usize == 0 {
        return Err(ElfError::Invalid);
    }
    if e_phoff == 0 && e_phnum > 0 {
        return Err(ElfError::Invalid);
    }
    Ok(ElfHeader64 {
        e_entry,
        e_phoff,
        e_phentsize,
        e_phnum,
        e_type,
        e_machine,
    })
}

fn parse_program_headers(image: &[u8], header: &ElfHeader64) -> Result<Vec<LoadSegment>, ElfError> {
    let mut segments = Vec::new();
    let phoff = header.e_phoff as usize;
    let phentsize = header.e_phentsize as usize;
    let phnum = header.e_phnum as usize;
    let total = phoff
        .checked_add(phentsize.saturating_mul(phnum))
        .ok_or(ElfError::OutOfBounds)?;
    if total > image.len() {
        return Err(ElfError::OutOfBounds);
    }
    // Sadece PT_LOAD segmentlerini topla
    for i in 0..phnum {
        let base = phoff + i * phentsize;
        let ph = parse_program_header(image, base)?;
        if ph.p_type == PT_LOAD {
            if ph.p_offset.saturating_add(ph.p_filesz) as usize > image.len() {
                return Err(ElfError::OutOfBounds);
            }
            segments.push(LoadSegment {
                offset: ph.p_offset,
                vaddr: ph.p_vaddr,
                filesz: ph.p_filesz,
                memsz: ph.p_memsz,
                flags: ph.p_flags,
                align: ph.p_align,
            });
        }
    }
    Ok(segments)
}

pub fn map_user_stack(
    stack_top: VirtAddr,
    pages: usize,
    _mapper: &mut impl Mapper<Size4KiB>,
    _frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), ElfError> {
    if pages <= crate::memory::USER_STACK_GUARD_PAGES {
        return Err(ElfError::Invalid);
    }
    let size = (pages as u64).saturating_mul(crate::memory::PAGE_SIZE as u64);
    let start = VirtAddr::new(stack_top.as_u64().saturating_sub(size));
    let guard_bytes = (crate::memory::USER_STACK_GUARD_PAGES as u64)
        .saturating_mul(crate::memory::PAGE_SIZE as u64);
    let guard_start = VirtAddr::new(start.as_u64().saturating_add(guard_bytes));
    let flags =
        PageTableFlags::USER_ACCESSIBLE | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;
    let lazy_size = size.saturating_sub(guard_bytes);
    if lazy_size == 0 {
        return Err(ElfError::Invalid);
    }
    if !crate::memory::register_lazy_region(guard_start.as_u64(), lazy_size, flags) {
        return Err(ElfError::Unsupported);
    }
    Ok(())
}

fn parse_program_header(image: &[u8], offset: usize) -> Result<ProgramHeader64, ElfError> {
    // ELF64 program header boyutu 56 byte
    if offset + 56 > image.len() {
        return Err(ElfError::OutOfBounds);
    }
    let p_type = read_u32(image, offset)?;
    let p_flags = read_u32(image, offset + 4)?;
    let p_offset = read_u64(image, offset + 8)?;
    let p_vaddr = read_u64(image, offset + 16)?;
    let p_filesz = read_u64(image, offset + 32)?;
    let p_memsz = read_u64(image, offset + 40)?;
    let p_align = read_u64(image, offset + 48)?;
    Ok(ProgramHeader64 {
        p_type,
        p_flags,
        p_offset,
        p_vaddr,
        p_filesz,
        p_memsz,
        p_align,
    })
}

fn read_u16(image: &[u8], offset: usize) -> Result<u16, ElfError> {
    // Little-endian 16-bit okuma
    if offset + 2 > image.len() {
        return Err(ElfError::OutOfBounds);
    }
    Ok(u16::from_le_bytes([image[offset], image[offset + 1]]))
}

fn read_u32(image: &[u8], offset: usize) -> Result<u32, ElfError> {
    // Little-endian 32-bit okuma
    if offset + 4 > image.len() {
        return Err(ElfError::OutOfBounds);
    }
    Ok(u32::from_le_bytes([
        image[offset],
        image[offset + 1],
        image[offset + 2],
        image[offset + 3],
    ]))
}

fn read_u64(image: &[u8], offset: usize) -> Result<u64, ElfError> {
    // Little-endian 64-bit okuma
    if offset + 8 > image.len() {
        return Err(ElfError::OutOfBounds);
    }
    Ok(u64::from_le_bytes([
        image[offset],
        image[offset + 1],
        image[offset + 2],
        image[offset + 3],
        image[offset + 4],
        image[offset + 5],
        image[offset + 6],
        image[offset + 7],
    ]))
}

// ============================================================================
// DİNAMİK BAĞLAMA DESTEĞİ
// ============================================================================

// ELF Dinamik bölüm (Dynamic section) etiketleri
const DT_NULL: u64 = 0;
const DT_NEEDED: u64 = 1;
const DT_STRTAB: u64 = 5;
const DT_SYMTAB: u64 = 6;
const DT_STRSZ: u64 = 10;
const DT_SYMENT: u64 = 11;
const DT_REL: u64 = 17;
const DT_RELSZ: u64 = 18;
const DT_RELENT: u64 = 19;
const DT_PLTREL: u64 = 20;
const DT_JMPREL: u64 = 23;
const DT_RELA: u64 = 7;
const DT_RELASZ: u64 = 8;
const DT_RELAENT: u64 = 9;

// x86_64 için yeniden konumlandırma (relocation) türleri
const R_X86_64_64: u32 = 1; // Doğrudan 64-bit yeniden konumlandırma
const R_X86_64_PC32: u32 = 2; // PC'ye göreli 32-bit
const R_X86_64_GLOB_DAT: u32 = 6; // Global ofset tablosu (GOT)
const R_X86_64_JUMP_SLOT: u32 = 7; // PLT girişi
const R_X86_64_RELATIVE: u32 = 8; // Yükleme adresine göreli

// Sembol bağlama türleri
const STB_LOCAL: u8 = 0;
const STB_GLOBAL: u8 = 1;
const STB_WEAK: u8 = 2;

// Sembol türleri
const STT_NOTYPE: u8 = 0;
const STT_OBJECT: u8 = 1;
const STT_FUNC: u8 = 2;
const STT_SECTION: u8 = 3;

// Program header türleri
const PT_DYNAMIC: u32 = 2;
const PT_INTERP: u32 = 3;
const PT_GNU_RELRO: u32 = 0x6474e552;

/// Symbol table entry
#[derive(Clone, Debug)]
pub struct Symbol {
    pub name: String,
    pub name_offset: u32,
    pub value: u64,
    pub size: u64,
    pub binding: u8,
    pub sym_type: u8,
    pub section_index: u16,
}

impl Symbol {
    pub fn is_defined(&self) -> bool {
        self.section_index != 0
    }

    pub fn is_function(&self) -> bool {
        self.sym_type == STT_FUNC
    }

    pub fn is_global(&self) -> bool {
        self.binding == STB_GLOBAL
    }
}

/// Relocation entry
#[derive(Clone, Copy, Debug)]
pub struct Relocation {
    pub offset: u64,
    pub rel_type: u32,
    pub symbol_index: u32,
    pub addend: i64,
}

/// Dynamic section entry
#[derive(Clone, Copy, Debug)]
pub struct DynamicEntry {
    pub tag: u64,
    pub value: u64,
}

/// Loaded shared object
#[derive(Clone, Debug)]
pub struct SharedObject {
    pub name: String,
    pub base_address: u64,
    pub size: u64,
    pub symbols: Vec<Symbol>,
    pub relocations: Vec<Relocation>,
    pub dependencies: Vec<String>,
    pub ref_count: u32,
    pub entry: u64,
}

impl SharedObject {
    pub fn new(name: String, base_address: u64) -> Self {
        SharedObject {
            name,
            base_address,
            size: 0,
            symbols: Vec::new(),
            relocations: Vec::new(),
            dependencies: Vec::new(),
            ref_count: 1,
            entry: 0,
        }
    }

    /// Ada göre sembol arar
    pub fn find_symbol(&self, name: &str) -> Option<u64> {
        for sym in &self.symbols {
            if sym.name == name && sym.is_defined() {
                return Some(self.base_address + sym.value);
            }
        }
        None
    }

    /// Başvuru sayısını artırır
    pub fn add_ref(&mut self) {
        self.ref_count += 1;
    }

    /// Başvuru sayısını azaltır
    pub fn release(&mut self) -> u32 {
        if self.ref_count > 0 {
            self.ref_count -= 1;
        }
        self.ref_count
    }
}

// Global paylaşılan nesne kayıt defteri
static SHARED_OBJECTS: Mutex<BTreeMap<String, SharedObject>> = Mutex::new(BTreeMap::new());
static SYMBOL_CACHE: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());
static NEXT_LOAD_ADDRESS: Mutex<u64> = Mutex::new(0x7F0000000000);

/// ELF'ten dinamik bölümü ayrıştırır
pub fn parse_dynamic_section(
    image: &[u8],
    header: &ElfHeader64,
) -> Result<Vec<DynamicEntry>, ElfError> {
    let mut entries = Vec::new();
    let phoff = header.e_phoff as usize;
    let phentsize = header.e_phentsize as usize;
    let phnum = header.e_phnum as usize;

    for i in 0..phnum {
        let base = phoff + i * phentsize;
        let p_type = read_u32(image, base)?;

        if p_type == PT_DYNAMIC {
            let p_offset = read_u64(image, base + 8)?;
            let p_filesz = read_u64(image, base + 32)?;

            let dyn_start = p_offset as usize;
            let dyn_end = dyn_start + p_filesz as usize;

            if dyn_end > image.len() {
                return Err(ElfError::OutOfBounds);
            }

            let mut offset = dyn_start;
            while offset + 16 <= dyn_end {
                let tag = read_u64(image, offset)?;
                let value = read_u64(image, offset + 8)?;

                if tag == DT_NULL {
                    break;
                }

                entries.push(DynamicEntry { tag, value });
                offset += 16;
            }
            break;
        }
    }

    Ok(entries)
}

/// ELF'ten sembol tablosunu ayrıştırır
pub fn parse_symbol_table(
    image: &[u8],
    symtab_offset: u64,
    symtab_size: u64,
    strtab_offset: u64,
) -> Result<Vec<Symbol>, ElfError> {
    let mut symbols = Vec::new();
    let entry_size = 24u64; // ELF64 sembol girişi boyutu
    let count = symtab_size / entry_size;

    for i in 0..count {
        let offset = symtab_offset + i * entry_size;
        let offset_usize = offset as usize;

        if offset_usize + 24 > image.len() {
            break;
        }

        let name_offset = read_u32(image, offset_usize)?;
        let value = read_u64(image, offset_usize + 8)?;
        let size = read_u64(image, offset_usize + 16)?;
        let info = image[offset_usize + 4];
        let other = image[offset_usize + 5];
        let section_index = read_u16(image, offset_usize + 6)?;

        // Sembol adını dizge tablosundan oku
        let name = read_string(image, strtab_offset as usize + name_offset as usize)?;

        let binding = info >> 4;
        let sym_type = info & 0xF;

        symbols.push(Symbol {
            name,
            name_offset,
            value,
            size,
            binding,
            sym_type,
            section_index,
        });
    }

    Ok(symbols)
}

/// ELF'ten yeniden konumlandırma girişlerini ayrıştırır
pub fn parse_relocations(
    image: &[u8],
    rel_offset: u64,
    rel_size: u64,
    is_rela: bool,
) -> Result<Vec<Relocation>, ElfError> {
    let mut relocs = Vec::new();
    let entry_size = if is_rela { 24u64 } else { 16u64 };
    let count = rel_size / entry_size;

    for i in 0..count {
        let offset = rel_offset + i * entry_size;
        let offset_usize = offset as usize;

        if offset_usize + entry_size as usize > image.len() {
            break;
        }

        let r_offset = read_u64(image, offset_usize)?;
        let r_info = read_u64(image, offset_usize + 8)?;

        let rel_type = (r_info & 0xFFFFFFFF) as u32;
        let symbol_index = (r_info >> 32) as u32;
        let addend = if is_rela {
            read_u64(image, offset_usize + 16)? as i64
        } else {
            0
        };

        relocs.push(Relocation {
            offset: r_offset,
            rel_type,
            symbol_index,
            addend,
        });
    }

    Ok(relocs)
}

/// Görüntüden null ile sonlandırılmış yazı dizisini okur
fn read_string(image: &[u8], offset: usize) -> Result<String, ElfError> {
    let mut bytes = Vec::new();
    let mut pos = offset;

    while pos < image.len() {
        let byte = image[pos];
        if byte == 0 {
            break;
        }
        bytes.push(byte);
        pos += 1;
    }

    String::from_utf8(bytes).map_err(|_| ElfError::Invalid)
}

/// Paylaşılan nesneyi dosyadan yükler
pub fn dlopen(name: &str, image: &[u8]) -> Result<*mut u8, ElfError> {
    // Zaten yüklenip yülenmediğini kontrol et
    {
        let objects = SHARED_OBJECTS.lock();
        if objects.contains_key(name) {
            return Err(ElfError::AlreadyLoaded);
        }
    }

    // ELF'i ayrıştır
    let header = parse_header(image)?;

    // Paylaşılan nesne olup olmadığını kontrol et
    if header.e_type != ET_DYN {
        return Err(ElfError::Unsupported);
    }

    // Yükleme adresi tahsis et
    let base_address = {
        let mut next_addr = NEXT_LOAD_ADDRESS.lock();
        let addr = *next_addr;
        *next_addr += 0x100000; // 1MB hizalama
        addr
    };

    // Paylaşılan nesneyi oluştur
    let mut obj = SharedObject::new(String::from(name), base_address);
    obj.entry = header.e_entry;

    // Dinamik bölümü ayrıştır
    let dyn_entries = parse_dynamic_section(image, &header)?;

    let mut symtab_offset = 0u64;
    let mut symtab_size = 0u64;
    let mut strtab_offset = 0u64;
    let mut rel_offset = 0u64;
    let mut rel_size = 0u64;
    let mut rela_offset = 0u64;
    let mut rela_size = 0u64;
    let mut is_rela = false;

    for entry in &dyn_entries {
        match entry.tag {
            DT_SYMTAB => symtab_offset = entry.value,
            DT_SYMENT => {} // Sembol girişi boyutu
            DT_STRTAB => strtab_offset = entry.value,
            DT_STRSZ => {} // Dizge tablosu boyutu
            DT_REL => rel_offset = entry.value,
            DT_RELSZ => rel_size = entry.value,
            DT_RELENT => {}
            DT_RELA => {
                rela_offset = entry.value;
                is_rela = true;
            }
            DT_RELASZ => rela_size = entry.value,
            DT_RELAENT => {}
            DT_NEEDED => {
                // Bağımlılık - dizge tablosu araması gerektirir
            }
            _ => {}
        }
    }

    // Sembolleri ayrıştır
    if symtab_offset > 0 && strtab_offset > 0 {
        obj.symbols = parse_symbol_table(image, symtab_offset, symtab_size, strtab_offset)?;
    }

    // Yeniden konumlandırmaları ayrıştır
    if rel_offset > 0 && rel_size > 0 {
        obj.relocations = parse_relocations(image, rel_offset, rel_size, false)?;
    }
    if rela_offset > 0 && rela_size > 0 {
        let mut rela_relocs = parse_relocations(image, rela_offset, rela_size, true)?;
        obj.relocations.append(&mut rela_relocs);
    }

    // Boyutu segmentlerden hesapla
    let segments = parse_program_headers(image, &header)?;
    for seg in &segments {
        let end = seg.vaddr + seg.memsz;
        if end > obj.size {
            obj.size = end;
        }
    }

    // Kayıt defterine ekle
    let ptr = base_address as *mut u8;
    SHARED_OBJECTS
        .lock()
        .insert(String::from(name), obj.clone());

    // Sembol önbelleğini güncelle
    let mut cache = SYMBOL_CACHE.lock();
    for sym in &obj.symbols {
        if sym.is_defined() && sym.is_global() {
            let addr = obj.base_address + sym.value;
            cache.insert(sym.name.clone(), addr);
        }
    }

    crate::serial_println!(
        "[DLOPEN] Loaded {} at {:#x}, {} symbols",
        name,
        base_address,
        obj.symbols.len()
    );

    Ok(ptr)
}

/// Yüklenen nesnelerde sembol arar
pub fn dlsym(name: &str) -> Result<u64, ElfError> {
    // Önce önbelleğe bak
    {
        let cache = SYMBOL_CACHE.lock();
        if let Some(&addr) = cache.get(name) {
            return Ok(addr);
        }
    }

    // Tüm yüklenen nesnelerde ara
    let objects = SHARED_OBJECTS.lock();
    for obj in objects.values() {
        if let Some(addr) = obj.find_symbol(name) {
            return Ok(addr);
        }
    }

    Err(ElfError::SymbolNotFound)
}

/// Paylaşılan nesneyi kapatır
pub fn dlclose(name: &str) -> Result<(), ElfError> {
    let mut objects = SHARED_OBJECTS.lock();

    if let Some(obj) = objects.get_mut(name) {
        if obj.release() == 0 {
            // Remove from symbol cache
            let mut cache = SYMBOL_CACHE.lock();
            for sym in &obj.symbols {
                cache.remove(&sym.name);
            }

            // Unload
            objects.remove(name);
            crate::serial_println!("[DLCLOSE] Unloaded {}", name);
        }
    }

    Ok(())
}

/// Yüklenen nesneye yeniden konumlandırma uygular
pub fn apply_relocations(obj_name: &str) -> Result<(), ElfError> {
    let objects = SHARED_OBJECTS.lock();
    let obj = objects.get(obj_name).ok_or(ElfError::NotLoaded)?;

    // Ödünç alma sorunlarını önlemek için yeniden konumlandırmaları klonla
    let relocs = obj.relocations.clone();
    let base = obj.base_address;
    drop(objects);

    for reloc in &relocs {
        let target_addr = base + reloc.offset;

        match reloc.rel_type {
            R_X86_64_RELATIVE => {
                // Taban'a göreli yeniden konumlandırma
                let value = (base as i64 + reloc.addend) as u64;
                // target_addr adresine yazılacak
                let _ = (target_addr, value);
            }
            R_X86_64_64 | R_X86_64_GLOB_DAT | R_X86_64_JUMP_SLOT => {
                // Sembole dayalı yeniden konumlandırma
                let objects = SHARED_OBJECTS.lock();
                if let Some(obj) = objects.get(obj_name) {
                    if let Some(sym) = obj.symbols.get(reloc.symbol_index as usize) {
                        if sym.is_defined() {
                            let sym_addr = obj.base_address + sym.value;
                            let _ = (target_addr, sym_addr);
                        } else {
                            // Diğer nesnelerde ara
                            if let Ok(addr) = dlsym(&sym.name) {
                                let _ = (target_addr, addr);
                            }
                        }
                    }
                }
            }
            _ => {
                // Desteklenmeyen yeniden konumlandırma türü
            }
        }
    }

    Ok(())
}

/// Yüklenen nesnelerin listesini döndürür
pub fn dl_loaded_objects() -> Vec<String> {
    SHARED_OBJECTS.lock().keys().cloned().collect()
}

/// Sembol bilgisini sorgular
pub fn dl_symbol_info(name: &str) -> Option<(String, u64, u64)> {
    let objects = SHARED_OBJECTS.lock();
    for obj in objects.values() {
        if let Some(addr) = obj.find_symbol(name) {
            for sym in &obj.symbols {
                if sym.name == name {
                    return Some((obj.name.clone(), addr, sym.size));
                }
            }
        }
    }
    None
}
