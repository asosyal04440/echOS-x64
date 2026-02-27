//! # echOS PE Kullanıcı Modu Yürütücüsü
//!
//! ## Bu Modülün Amacı
//!
//! `pe_loader` modülü bir PE32+ dosyasını bellekte açar ve bölümlerini düzleştirir.
//! Bu modül (`pe_exec`) ise o düzleştirilmiş görüntüyü gerçek bir kullanıcı sürecine dönüştürür:
//!
//!   1. Her PE bölümü (`text`, `data`, `rdata`, ...) lazy (tembel) sayfalar olarak Ring-3
//!      sanal adres uzayına kayıt edilir — henüz fiziksel sayfa tahsis edilmez.
//!   2. IAT (Import Address Table) Win32 sistem çağrısı trambolinleriyle yamalanır.
//!   3. Win32 stub sayfası `WIN32_STUB_VIRT` adresine eşlenir.
//!   4. Kullanıcı yığını (stack) oluşturulur + guard sayfası eklenir.
//!   5. `syscall` talimatıyla Ring-3'e (kullanıcı moduna) girilir — geri dönmez.
//!
//! ## Çağrı Kuralı Köprüsü
//!
//! Windows x64: ilk 4 argüman RCX, RDX, R8, R9 kayıtlarında taşınır.
//! Linux x64 syscall: argümanlar RDI, RSI, RDX, R10 sırasında olmalıdır.
//!
//! Her stub (32 bayt) bu dönüşümü donanım hızında yapar:
//!   movabs rax, ECHOS_WIN32_BASE + func_idx   ; 10 bayt — syscall numarası
//!   mov    rdi, rcx                            ;  3 bayt — win arg1 → linux arg1
//!   mov    rsi, rdx                            ;  3 bayt — win arg2 → linux arg2
//!   mov    rdx, r8                             ;  3 bayt — win arg3 → linux arg3
//!   mov    r10, r9                             ;  3 bayt — win arg4 → linux arg4
//!   syscall                                    ;  2 bayt — çekirdek geçişi
//!   ret                                        ;  1 bayt — GKüTürk döndür
//!   nop × 7                                    ;  7 bayt — 32 bayt hizalama dolgusu

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::collections::BTreeMap;
use alloc::boxed::Box;
use spin::Mutex;
use x86_64::structures::paging::PageTableFlags;
use x86_64::VirtAddr;
use x86_64::registers::control::{Cr3, Cr3Flags};

// ============================================================================
// CONSTANTS
// ============================================================================

/// Virtual address where Win32 syscall stubs will be mapped in every PE process
pub const WIN32_STUB_VIRT: u64 = 0x0000_7FFF_0000_0000;

/// Syscall number base for Win32 calls (high bits to avoid conflicts with Linux)
pub const ECHOS_WIN32_BASE: usize = 0xEC00_0000;

/// Size of each stub in bytes (must be power-of-two for alignment)
pub const STUB_SIZE: usize = 32;

// ============================================================================
// GLOBAL WIN32 DISPATCH TABLE
// Populated when a PE is loaded; maps func_idx → (dll_name, func_name)
// ============================================================================

static WIN32_DISPATCH: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// Register a Win32 function and return its global index.
/// Called by build_stubs for each imported function.
fn register_win32_func(dll: &str, func: &str) -> usize {
    let mut table = WIN32_DISPATCH.lock();
    // Deduplicate: if (dll, func) is already in the table return existing index
    for (i, (d, f)) in table.iter().enumerate() {
        if d == dll && f == func { return i; }
    }
    let idx = table.len();
    table.push((dll.to_string(), func.to_string()));
    idx
}

/// Win32 IAT girdisini çekirdek global tablosuna kaydet ve indeksini döndür.
///
/// ## IAT (Import Address Table) Nasıl Çalışır?
///
/// PE32+ dosyaları çalışma zamanında harici DLL fonksiyonlarını şöyle çağırır:
///   1. IAT'da belirli adrese `CALL` talimatı var.
///   2. Normal Windows'ta: Loader, IAT hücresine DLL'in yüklü adresini yazar.
///   3. echOS'ta: IAT hücresine Win32 stub'ın sanal adresi yazılır.
///      Stub ise `syscall` talimatıyla çekirdeğe düşer.
///
/// Bu fonksiyon `pe_loader` Stage 4 tarafından çağrılır. Aynı DLL+fonksiyon
/// çifti birden fazla kez import edilirse tablo büyümez (deduplikasyon).
///
/// Dönüş: `WIN32_STUB_VIRT + idx * STUB_SIZE` fonksiyonunun sanal adresi için indeks.
pub fn register_import(dll: &str, func: &str) -> usize {
    register_win32_func(dll, func)
}

/// Return the number of Win32 functions currently in the dispatch table.
pub fn registered_import_count() -> usize {
    WIN32_DISPATCH.lock().len()
}

/// Generate the full Win32 stub page for `n` functions.
/// Public wrapper around the private `generate_stubs()`.
pub fn generate_stubs_for(n: usize) -> Vec<u8> {
    generate_stubs(n)
}

/// Dispatch a Win32 syscall (called from posix::dispatch when number ≥ ECHOS_WIN32_BASE).
pub fn dispatch_win32(func_idx: usize, args: [usize; 6]) -> usize {
    let (dll, func) = {
        let table = WIN32_DISPATCH.lock();
        match table.get(func_idx) {
            Some(e) => e.clone(),
            None => {
                crate::serial_println!("[WIN32] Unknown func_idx={}", func_idx);
                return usize::MAX; // ERROR value
            }
        }
    };

    crate::serial_println!(
        "[WIN32] Call: {}!{} args=[{:#x},{:#x},{:#x},{:#x}]",
        dll, func,
        args[0], args[1], args[2], args[3]
    );

    dispatch_win32_by_name(&dll, &func, args)
}

/// Dispatch by DLL + function name to the actual kernel Win32 implementation.
fn dispatch_win32_by_name(dll: &str, func: &str, args: [usize; 6]) -> usize {
    // Normalize DLL name (strip path, lowercase, strip .dll)
    let dll_key = dll
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(dll)
        .to_ascii_lowercase()
        .trim_end_matches(".dll")
        .to_string();

    match (dll_key.as_str(), func) {
        // --- KERNEL32 ---
        ("kernel32" | "kernelbase", "VirtualAlloc") => {
            let addr     = args[0];
            let size     = args[1];
            let _fltype  = args[2];
            let _protect = args[3];
            // Simple bump allocation in the user mmap range
            let result = crate::posix::sys_mmap_user(addr, size);
            crate::serial_println!("[W32] VirtualAlloc({:#x},{:#x}) -> {:#x}", addr, size, result);
            result
        }
        ("kernel32" | "kernelbase", "VirtualFree") => {
            // Unmap the region — for now just return TRUE (1)
            crate::serial_println!("[W32] VirtualFree stub");
            1
        }
        ("kernel32" | "kernelbase", "GetModuleHandleA" | "GetModuleHandleW") => {
            // Return a fake module handle (0 = self).
            // NULL input = current process, return fake base 0x140000000
            if args[0] == 0 { 0x1_4000_0000 } else { 0 }
        }
        ("kernel32" | "kernelbase", "GetProcAddress") => {
            // Return stub address for known functions; 0 for unknown
            // args[0] = HMODULE, args[1] = name ptr
            let name = read_user_cstring_safe(args[1]);
            let name_str = name.as_deref().unwrap_or("");
            crate::serial_println!("[W32] GetProcAddress: {}", name_str);
            0 // Not found — caller will handle
        }
        ("kernel32" | "kernelbase", "CreateFileA" | "CreateFileW") => {
            let path = read_user_cstring_safe(args[0]);
            let path_str = path.as_deref().unwrap_or("?");
            crate::serial_println!("[W32] CreateFileA: {}", path_str);
            // Open file via VFS, return a fake handle
            let fd = crate::posix::open_path_for_win32(path_str) as usize;
            if fd == usize::MAX { 0xFFFF_FFFF_FFFF_FFFF } else { fd | 0xF000_0000 }
        }
        ("kernel32" | "kernelbase", "ReadFile") => {
            // args: handle, buf, nToRead, lpRead, lpOverlapped
            let handle = args[0] & !0xF000_0000;
            let buf    = args[1];
            let count  = args[2];
            let lpread = args[3];
            let nr = crate::posix::read_fd_for_win32(handle, buf, count);
            if lpread != 0 {
                unsafe { *(lpread as *mut u32) = nr as u32; }
            }
            if nr == usize::MAX { 0 } else { 1 } // BOOL TRUE
        }
        ("kernel32" | "kernelbase", "CloseHandle") => {
            let handle = args[0] & !0xF000_0000;
            crate::posix::close_fd_for_win32(handle);
            1
        }
        ("kernel32" | "kernelbase", "GetLastError") => 0,
        ("kernel32" | "kernelbase", "SetLastError") => 0,
        ("kernel32" | "kernelbase", "ExitProcess" | "TerminateProcess") => {
            crate::serial_println!("[W32] ExitProcess({})", args[0]);
            crate::task::scheduler::exit(args[0] as i32);
        }
        ("kernel32" | "kernelbase", "GetSystemInfo") => {
            // Fill SYSTEM_INFO struct at args[0]
            let ptr = args[0] as *mut u32;
            if ptr != core::ptr::null_mut() {
                unsafe {
                    // SYSTEM_INFO layout (partial, 48 bytes)
                    // wProcessorArchitecture = 9 (x64)
                    *(ptr as *mut u16) = 9;
                    // dwPageSize = 4096
                    *(ptr.add(1).cast::<u32>()) = 4096;
                    // dwNumberOfProcessors = 4
                    *(ptr.add(8).cast::<u32>()) = 4;
                }
            }
            0
        }
        ("kernel32" | "kernelbase", "Sleep") => {
            let ms = args[0] as u64;
            let ticks = ms / 10; // rough: timer tick ~10ms
            crate::task::scheduler::sleep(ticks.max(1) as usize);
            0
        }
        ("kernel32" | "kernelbase", "CreateThread") => {
            // args: attr, stack, fn_ptr, parameter, flags, tid_ptr
            let entry_fn = args[2];
            let param    = args[3];
            let _tid_ptr = args[5];
            let task_id = crate::task::scheduler::spawn_win32_thread(entry_fn as u64, param as u64);
            crate::serial_println!("[W32] CreateThread entry={:#x} -> tid={}", entry_fn, task_id);
            task_id | 0xA000_0000
        }
        ("kernel32" | "kernelbase", "HeapCreate") => 0x1_0000_0000, // fake heap handle
        ("kernel32" | "kernelbase", "HeapAlloc") => {
            let size = args[2];
            crate::posix::sys_mmap_user(0, size)
        }
        ("kernel32" | "kernelbase", "HeapFree") => 1,
        ("kernel32" | "kernelbase", "LocalAlloc") => {
            let size = args[1];
            crate::posix::sys_mmap_user(0, size)
        }
        ("kernel32" | "kernelbase", "GlobalAlloc") => {
            let size = args[1];
            crate::posix::sys_mmap_user(0, size)
        }
        // GetConsoleWindow etc. — stubs
        _ => {
            crate::serial_println!("[W32] STUB {}!{} -> 0", dll_key, func);
            0
        }
    }
}

// ============================================================================
// MINIMAL PE PARSING (raw bytes → sections + imports)
// ============================================================================

#[derive(Debug)]
struct PeHeader {
    image_base:  u64,
    entry_rva:   u32,
    image_size:  u32,
    sections:    Vec<PeSection>,
    import_rva:  u32,
    import_size: u32,
}

#[derive(Debug)]
struct PeSection {
    virtual_address: u32,
    virtual_size:    u32,
    raw_ptr:         u32,
    raw_size:        u32,
    characteristics: u32,
}

#[derive(Debug)]
struct IatEntry {
    /// Byte offset inside the flat image where the pointer must be written
    flat_offset: usize,
    /// Global Win32 func index assigned by register_win32_func
    func_idx:    usize,
}

/// Parse PE header, sections, and collect IAT patch entries.
/// Returns `(header, iat_patches)` where `iat_patches` contains all IAT
/// slots that must be filled with `WIN32_STUB_VIRT + func_idx * STUB_SIZE`.
fn parse_pe(data: &[u8]) -> Result<(PeHeader, Vec<IatEntry>), &'static str> {
    if data.len() < 64 { return Err("too small"); }
    if data[0] != b'M' || data[1] != b'Z' { return Err("not MZ"); }

    let e_lfanew = u32::from_le_bytes(data[60..64].try_into().unwrap()) as usize;
    if e_lfanew + 4 > data.len() { return Err("bad e_lfanew"); }
    if &data[e_lfanew..e_lfanew+4] != b"PE\0\0" { return Err("not PE"); }

    let coff = e_lfanew + 4;
    if coff + 20 > data.len() { return Err("no coff"); }

    let _machine              = u16::from_le_bytes(data[coff..coff+2].try_into().unwrap());
    let num_sections          = u16::from_le_bytes(data[coff+2..coff+4].try_into().unwrap()) as usize;
    let opt_hdr_size          = u16::from_le_bytes(data[coff+16..coff+18].try_into().unwrap()) as usize;

    let opt = coff + 20;
    if opt + 2 > data.len() { return Err("no opt hdr"); }
    let magic = u16::from_le_bytes(data[opt..opt+2].try_into().unwrap());
    if magic != 0x20B { return Err("not PE32+"); }

    if opt + 240 > data.len() { return Err("opt hdr too small"); }

    let entry_rva  = u32::from_le_bytes(data[opt+16..opt+20].try_into().unwrap());
    let image_base = u64::from_le_bytes(data[opt+24..opt+32].try_into().unwrap());
    let image_size = u32::from_le_bytes(data[opt+56..opt+60].try_into().unwrap());

    // Data directory [1] = Import directory (at opt+112 for PE32+)
    // opt_hdr fields: 0..4=magic/version, 4..16=sizes, 16..20=entry, 20..24=baseofcode,
    //   24..32=ImageBase, 32..36=SectionAlign, 36..40=FileAlign, 40..48=OSver/imgver/subsysver/reserved,
    //   48..52=Win32VersionValue, 52..56=SizeOfImage, 56..60=SizeOfHeaders, 60..64=Checksum,
    //   64..66=Subsystem, 66..68=DllCharacteristics,
    //   68..76=SizeOfStackReserve, 76..84=SizeOfStackCommit,
    //   84..92=SizeOfHeapReserve, 92..100=SizeOfHeapCommit,
    //   100..104=LoaderFlags, 104..108=NumberOfRvaAndSizes,
    //   108 = DataDirectory start [0]=export,[1]=import,...
    let import_rva  = u32::from_le_bytes(data[opt+116..opt+120].try_into().unwrap());
    let import_size = u32::from_le_bytes(data[opt+120..opt+124].try_into().unwrap());

    // Sections start after: PE sig(4) + COFF(20) + optional header
    let sect_off = opt + opt_hdr_size;
    let mut sections = Vec::with_capacity(num_sections);
    for i in 0..num_sections {
        let s = sect_off + i * 40;
        if s + 40 > data.len() { break; }
        sections.push(PeSection {
            virtual_address: u32::from_le_bytes(data[s+12..s+16].try_into().unwrap()),
            virtual_size:    u32::from_le_bytes(data[s+16..s+20].try_into().unwrap()),
            raw_ptr:         u32::from_le_bytes(data[s+20..s+24].try_into().unwrap()),
            raw_size:        u32::from_le_bytes(data[s+24..s+28].try_into().unwrap()),
            characteristics: u32::from_le_bytes(data[s+36..s+40].try_into().unwrap()),
        });
    }

    let header = PeHeader { image_base, entry_rva, image_size, sections, import_rva, import_size };

    // Parse import descriptors (if present) to build IAT patch list
    let iat_patches = if import_rva == 0 || image_size == 0 {
        Vec::new()
    } else {
        parse_iat(&data, &header)?
    };

    Ok((header, iat_patches))
}

/// Parse the import table from the raw PE bytes.
/// Returns a list of (flat_image_offset, func_idx) pairs to patch after flattening.
fn parse_iat(data: &[u8], pe: &PeHeader) -> Result<Vec<IatEntry>, &'static str> {
    let mut patches = Vec::new();
    let image_size = pe.image_size as usize;
    if image_size == 0 { return Ok(patches); }

    // Helper: RVA → flat image offset (for sections already in the flat layout)
    let rva_to_flat = |rva: u32| -> Option<usize> {
        let rva = rva as usize;
        for s in &pe.sections {
            let va = s.virtual_address as usize;
            let vsz = s.virtual_size.max(s.raw_size) as usize;
            if rva >= va && rva < va + vsz {
                return Some(rva); // flat offset == RVA (post-flatten)
            }
        }
        None
    };

    // Helper: read u32 from flat image at given RVA
    let read_u32_rva = |flat: &[u8], rva: u32| -> Option<u32> {
        let off = rva_to_flat(rva)?;
        if off + 4 <= flat.len() {
            Some(u32::from_le_bytes(flat[off..off+4].try_into().unwrap()))
        } else { None }
    };

    // Helper: read u64 from flat image at given RVA
    let read_u64_rva = |flat: &[u8], rva: u32| -> Option<u64> {
        let off = rva_to_flat(rva)?;
        if off + 8 <= flat.len() {
            Some(u64::from_le_bytes(flat[off..off+8].try_into().unwrap()))
        } else { None }
    };

    // We need to build the flat image temporarily to read import data
    let mut flat = vec![0u8; image_size];
    for s in &pe.sections {
        let va  = s.virtual_address as usize;
        let rp  = s.raw_ptr as usize;
        let rsz = s.raw_size as usize;
        if rp == 0 || rsz == 0 { continue; }
        let end = (rp + rsz).min(data.len());
        let copy = (end - rp).min(image_size.saturating_sub(va));
        if copy > 0 {
            flat[va..va+copy].copy_from_slice(&data[rp..rp+copy]);
        }
    }

    // Walk IMAGE_IMPORT_DESCRIPTOR array (20 bytes each, terminated by zero entry)
    let mut desc_rva = pe.import_rva;
    loop {
        let off = match rva_to_flat(desc_rva) {
            Some(o) => o,
            None => break,
        };
        if off + 20 > flat.len() { break; }

        let name_rva  = u32::from_le_bytes(flat[off+12..off+16].try_into().unwrap());
        let iat_rva   = u32::from_le_bytes(flat[off+16..off+20].try_into().unwrap());

        // Zero entry = end of descriptor table
        if name_rva == 0 && iat_rva == 0 { break; }

        // DLL name
        let dll_name = if name_rva != 0 {
            let name_off = rva_to_flat(name_rva).unwrap_or(0);
            read_cstring_from(&flat, name_off)
        } else {
            String::new()
        };

        // Walk the IAT (array of u64 IMAGE_THUNK_DATA for PE32+)
        let mut thunk_rva = iat_rva;
        loop {
            let thunk_off = match rva_to_flat(thunk_rva) {
                Some(o) => o,
                None => break,
            };
            if thunk_off + 8 > flat.len() { break; }
            let thunk_val = u64::from_le_bytes(flat[thunk_off..thunk_off+8].try_into().unwrap());
            if thunk_val == 0 { break; }

            // Determine function name
            let func_name = if thunk_val & (1u64 << 63) != 0 {
                // Import by ordinal
                format!("#{}", thunk_val & 0xFFFF)
            } else {
                // Import by name: thunk_val is RVA to IMAGE_IMPORT_BY_NAME {u16 hint, char name[]}
                let ibn_rva = thunk_val as u32;
                let ibn_off = rva_to_flat(ibn_rva).unwrap_or(0);
                if ibn_off + 2 < flat.len() {
                    read_cstring_from(&flat, ibn_off + 2) // skip 2-byte hint
                } else {
                    String::from("?")
                }
            };

            // Register function in global dispatch table
            let func_idx = register_win32_func(&dll_name, &func_name);

            // Remember: patch this IAT slot (flat_offset = thunk_off, which is == thunk_rva)
            patches.push(IatEntry { flat_offset: thunk_off, func_idx });

            thunk_rva += 8; // next 8-byte thunk
        }

        desc_rva += 20; // next descriptor
    }

    Ok(patches)
}

// ============================================================================
// STUB GENERATION
// ============================================================================

/// Generate a stub page: all stubs for Win32 functions registered so far.
/// Returns the stub bytes (each stub STUB_SIZE bytes).
fn generate_stubs(num_stubs: usize) -> Vec<u8> {
    let mut stubs = Vec::with_capacity(num_stubs * STUB_SIZE);
    for idx in 0..num_stubs {
        let syscall_num = (ECHOS_WIN32_BASE + idx) as u64;
        let mut stub = [0x90u8; STUB_SIZE]; // fill with NOP

        // movabs rax, syscall_num  (10 bytes: 48 B8 <8-byte-imm>)
        stub[0]  = 0x48;
        stub[1]  = 0xB8;
        stub[2..10].copy_from_slice(&syscall_num.to_le_bytes());

        // mov rdi, rcx  (48 89 CF — 3 bytes)
        stub[10] = 0x48; stub[11] = 0x89; stub[12] = 0xCF;

        // mov rsi, rdx  (48 89 D6 — 3 bytes)
        stub[13] = 0x48; stub[14] = 0x89; stub[15] = 0xD6;

        // mov rdx, r8   (4C 89 C2 — 3 bytes)
        stub[16] = 0x4C; stub[17] = 0x89; stub[18] = 0xC2;

        // mov r10, r9   (4D 89 CA — 3 bytes)
        stub[19] = 0x4D; stub[20] = 0x89; stub[21] = 0xCA;

        // syscall       (0F 05 — 2 bytes)
        stub[22] = 0x0F; stub[23] = 0x05;

        // ret           (C3 — 1 byte)
        stub[24] = 0xC3;

        // Bytes 25-31 remain 0x90 (NOP)
        stubs.extend_from_slice(&stub);
    }
    stubs
}

// ============================================================================
// MAIN ENTRY POINT
// ============================================================================

/// Load and execute a PE32+ image in Ring-3.
///
/// This function does NOT return on success (it enters user mode).
pub fn execute(image: &[u8]) -> Result<(), &'static str> {
    crate::serial_println!("[PE_EXEC] Starting PE execution ({} bytes)", image.len());

    // 1. Clear previous Win32 dispatch table (clean slate per process)
    WIN32_DISPATCH.lock().clear();

    // 2. Parse PE header + IAT list (IAT list comes back empty until flat image is built)
    //    First pass: get section layout and know which functions are imported.
    let (pe_hdr, _iat_pre) = parse_pe(image)?;

    crate::serial_println!(
        "[PE_EXEC] image_base={:#x} entry_rva={:#x} image_size={:#x} sections={}",
        pe_hdr.image_base, pe_hdr.entry_rva, pe_hdr.image_size, pe_hdr.sections.len()
    );

    let image_size = pe_hdr.image_size as usize;
    if image_size == 0 { return Err("image_size=0"); }

    // 3. Build flat image (PE sections flattened to their virtual addresses)
    let mut flat = vec![0u8; image_size];
    for s in &pe_hdr.sections {
        let va  = s.virtual_address as usize;
        let rp  = s.raw_ptr as usize;
        let rsz = s.raw_size as usize;
        if rp == 0 || rsz == 0 { continue; }
        let file_end  = (rp + rsz).min(image.len());
        let copy_len  = (file_end - rp).min(image_size.saturating_sub(va));
        if copy_len > 0 {
            flat[va..va+copy_len].copy_from_slice(&image[rp..rp+copy_len]);
        }
    }

    // 4. Parse IAT using the flat image
    let iat_patches = parse_iat(image, &pe_hdr)?;
    let num_stubs   = WIN32_DISPATCH.lock().len();

    crate::serial_println!(
        "[PE_EXEC] IAT patches={} Win32 functions registered={}",
        iat_patches.len(), num_stubs
    );

    // 5. Generate Win32 stubs
    let stubs = generate_stubs(num_stubs);

    // 6. Patch IAT in flat image with stub virtual addresses
    for patch in &iat_patches {
        if patch.flat_offset + 8 <= flat.len() {
            let stub_addr = WIN32_STUB_VIRT + (patch.func_idx * STUB_SIZE) as u64;
            flat[patch.flat_offset..patch.flat_offset+8]
                .copy_from_slice(&stub_addr.to_le_bytes());
        }
    }

    // 7. Combine: flat image + stubs (stubs appended at image_size offset)
    let mut combined = flat;
    let stubs_offset = combined.len();
    combined.extend_from_slice(&stubs);

    // Leak combined buffer — must live for the process lifetime
    let combined_static: &'static mut [u8] = Box::leak(combined.into_boxed_slice());

    crate::serial_println!(
        "[PE_EXEC] Combined image size={} (flat={} stubs={})",
        combined_static.len(), stubs_offset, stubs.len()
    );

    // 8. Create address space and register user image
    let address_space = crate::memory::create_address_space(combined_static);
    crate::memory::set_active_address_space(Some(address_space));

    let user_pml4 = crate::memory::create_user_pml4()
        .ok_or("[PE_EXEC] create_user_pml4 failed")?;

    crate::memory::set_user_image(combined_static);

    // 9. Register PE sections as lazy file-backed regions
    for s in &pe_hdr.sections {
        let virt = pe_hdr.image_base + s.virtual_address as u64;
        let vsz  = (s.virtual_size.max(s.raw_size)) as u64;
        if vsz == 0 || virt < pe_hdr.image_base { continue; }
        if !crate::memory::is_user_range(virt, vsz) {
            crate::serial_println!("[PE_EXEC] Section virt={:#x} not in user range, skipping", virt);
            continue;
        }

        let mut flags = PageTableFlags::USER_ACCESSIBLE;
        if s.characteristics & 0x8000_0000 != 0 { flags |= PageTableFlags::WRITABLE; }
        if s.characteristics & 0x2000_0000 == 0 { flags |= PageTableFlags::NO_EXECUTE; }

        // file_offset = virtual_address (sections are flattened by RVA)
        let ok = crate::memory::register_file_lazy_region(
            virt, vsz, flags,
            s.virtual_address as u64, (s.raw_size as u64).min(vsz),
        );
        if !ok {
            crate::serial_println!("[PE_EXEC] Warning: section lazy register failed virt={:#x}", virt);
        }
    }

    // Special case: make all writable sections writable (some PEs have IAT in .rdata/+write)
    // Re-register the IAT region as writable so our patches are visible
    if pe_hdr.import_rva != 0 {
        let iat_flat_off = pe_hdr.import_rva as u64;
        let iat_virt = pe_hdr.image_base + iat_flat_off;
        let iat_sz   = 0x2000u64; // generous: 8KB to cover IAT region
        if crate::memory::is_user_range(iat_virt, iat_sz) {
            let flags = PageTableFlags::USER_ACCESSIBLE | PageTableFlags::WRITABLE
                | PageTableFlags::NO_EXECUTE;
            crate::memory::register_file_lazy_region(iat_virt, iat_sz, flags, iat_flat_off, iat_sz);
        }
    }

    // 10. Register Win32 stub page (executable, from combined[stubs_offset..])
    let stub_total = stubs_offset as u64 + stubs.len() as u64;
    if stubs.len() > 0 {
        // No WRITABLE, no NO_EXECUTE → executable + readable
        let stub_flags = PageTableFlags::USER_ACCESSIBLE;
        let ok = crate::memory::register_file_lazy_region(
            WIN32_STUB_VIRT,
            stubs.len() as u64,
            stub_flags,
            stubs_offset as u64,
            stubs.len() as u64,
        );
        crate::serial_println!(
            "[PE_EXEC] Stub page registered at {:#x} (ok={})", WIN32_STUB_VIRT, ok
        );
    }

    // 11. Map user stack
    let (_, stack_top_addr) = crate::memory::user_stack_bounds();
    let stack_size  = crate::memory::USER_STACK_PAGES as u64 * crate::memory::PAGE_SIZE as u64;
    let guard_start = stack_top_addr - stack_size + crate::memory::PAGE_SIZE as u64;
    let lazy_size   = stack_size - crate::memory::PAGE_SIZE as u64;
    let stack_flags = PageTableFlags::USER_ACCESSIBLE | PageTableFlags::WRITABLE
        | PageTableFlags::NO_EXECUTE;
    if !crate::memory::register_lazy_region(guard_start, lazy_size, stack_flags) {
        return Err("[PE_EXEC] stack map failed");
    }

    // 12. Switch to user PML4 and enter Ring-3 at PE entry point
    unsafe { Cr3::write(user_pml4, Cr3Flags::empty()); }

    let entry_virt = pe_hdr.image_base + pe_hdr.entry_rva as u64;
    let stack_virt = stack_top_addr;

    crate::serial_println!(
        "[PE_EXEC] Entering Ring-3: entry={:#x} stack={:#x}",
        entry_virt, stack_virt
    );

    unsafe {
        crate::task::user::enter_user_mode(
            VirtAddr::new(entry_virt),
            VirtAddr::new(stack_virt),
        )
    }
}

// ============================================================================
// HELPERS
// ============================================================================

// ============================================================================
// execute_loaded — entry point when the caller has already run the pipeline
// ============================================================================

/// Execute a pre-loaded PE image that was built by [`crate::pe_loader::GoblinPeLoader`].
///
/// This variant skips parsing and flattening (already done) and goes straight
/// to page-table wiring and Ring-3 entry.  Does NOT return on success.
pub fn execute_loaded(loaded: crate::pe_loader::LoadedPe) -> Result<(), &'static str> {
    use x86_64::registers::control::{Cr3, Cr3Flags};

    crate::serial_println!(
        "[PE_EXEC] execute_loaded entry={:#x} sandbox=#{} image_size={:#x}",
        loaded.entry, loaded.sandbox.id, loaded.image_size
    );

    let image_size = loaded.image_size as usize;
    crate::memory::set_user_image(loaded.flat);

    let user_pml4 = crate::memory::create_user_pml4()
        .ok_or("[PE_EXEC] create_user_pml4 failed")?;

    // Register each section as a lazy file-backed region
    for s in &loaded.sections {
        let virt = loaded.load_base + s.rva as u64;
        let vsz  = (s.virtual_size.max(s.raw_size)) as u64;
        if vsz == 0 { continue; }
        if !crate::memory::is_user_range(virt, vsz) { continue; }

        let mut flags = PageTableFlags::USER_ACCESSIBLE;
        if s.is_writable()  { flags |= PageTableFlags::WRITABLE; }
        if !s.is_exec()     { flags |= PageTableFlags::NO_EXECUTE; }

        crate::memory::register_file_lazy_region(
            virt, vsz, flags,
            s.rva as u64, (s.raw_size as u64).min(vsz),
        );
    }

    // Register the Win32 stub page (r-x) at WIN32_STUB_VIRT
    let stubs_len = loaded.flat.len() - loaded.stubs_flat_offset;
    if stubs_len > 0 {
        crate::memory::register_file_lazy_region(
            WIN32_STUB_VIRT,
            stubs_len as u64,
            PageTableFlags::USER_ACCESSIBLE, // no WRITE, no NX → exec
            loaded.stubs_flat_offset as u64,
            stubs_len as u64,
        );
        crate::serial_println!("[PE_EXEC] Stub page at {:#x} ({} bytes)", WIN32_STUB_VIRT, stubs_len);
    }

    // User stack
    let (_, stack_top) = crate::memory::user_stack_bounds();
    let stack_size = crate::memory::USER_STACK_PAGES as u64 * crate::memory::PAGE_SIZE as u64;
    let guard_start = stack_top - stack_size + crate::memory::PAGE_SIZE as u64;
    let lazy_sz     = stack_size - crate::memory::PAGE_SIZE as u64;
    let stack_flags = PageTableFlags::USER_ACCESSIBLE | PageTableFlags::WRITABLE
        | PageTableFlags::NO_EXECUTE;
    if !crate::memory::register_lazy_region(guard_start, lazy_sz, stack_flags) {
        return Err("[PE_EXEC] stack map failed");
    }

    unsafe { Cr3::write(user_pml4, Cr3Flags::empty()); }

    crate::serial_println!(
        "[PE_EXEC] Entering Ring-3  entry={:#x}  rsp={:#x}",
        loaded.entry, stack_top
    );

    unsafe {
        crate::task::user::enter_user_mode(
            VirtAddr::new(loaded.entry),
            VirtAddr::new(stack_top),
        )
    }
}

fn read_cstring_from(data: &[u8], offset: usize) -> String {
    let mut s = String::new();
    let mut i = offset;
    while i < data.len() && data[i] != 0 && s.len() < 256 {
        s.push(data[i] as char);
        i += 1;
    }
    s
}

fn read_user_cstring_safe(ptr: usize) -> Option<String> {
    if ptr == 0 { return None; }
    // Read up to 512 bytes from user space via STAC/CLAC guard
    let smap = crate::cpu::smap_enabled();
    if smap { unsafe { crate::cpu::stac(); } }
    let mut s = String::new();
    for i in 0..512usize {
        let b = unsafe { *((ptr + i) as *const u8) };
        if b == 0 { break; }
        s.push(b as char);
    }
    if smap { unsafe { crate::cpu::clac(); } }
    Some(s)
}
