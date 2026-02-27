//! # echOS Iron-Proton PE32+ Yükleyici — Omni-Matrix Tier 2
//!
//! ## PE32+ Formatı Nedir?
//!
//! Windows'taki `.exe` ve `.dll` dosyaları **PE (Portable Executable)** formatını
//! kullanır. PE32+, 64-bit PE biçimidir. İçyapısı:
//!
//! ```text
//! DOS Header (MZ) @ 0x00
//!   └─ e_lfanew → PE Signature ("PE\0\0") @ offset
//!       └─ COFF File Header (20 byte)
//!           └─ Optional Header (PE32+ = 240 byte)
//!               ├─ ImageBase, SizeOfImage, SizeOfHeaders
//!               ├─ AddressOfEntryPoint (RVA)
//!               └─ DataDirectory (16 giriş: .reloc, .idata, .edata vs.)
//!       └─ Section Table: .text, .data, .rdata, .bss ...
//! ```
//!
//! ## RVA (Relative Virtual Address) Kavramı
//!
//! PE dosyası içindeki tüm adresler "ImageBase'ťe göre göreli" yazılır.
//! Bunlara RVA (Relative Virtual Address = Göreli Sanal Adres) denir.
//! Dosya belleye yüklenince: gerçek VA = ImageBase + RVA
//! Eğer farklı adrese yüklenirse (.reloc tablosu) düzetme yapılır.
//!
//! ## 6 Aşamalı Yükleme Pipeline'u:
//!
//! ```text
//!  Ham .exe byte'ları
//!       │
//!  [1] goblin parse  → header, sections, imports, exports ayrıştır
//!  [2] Section flatten→ RVA düzenünde sıfırlanmış Vec'e kopyala (BSS = 0)
//!  [3] Relocation     → load_base != preferred_base ise tablo fixup uygula
//!  [4] IAT patch      → DLL import adreslerini pe_exec stub VA ile değiştir
//!  [5] Export tablosu → DLL yeniden-export kaydı için BTreeMap oluştur
//!  [6] IronShim sandbox → yetenek bileti + syscall filtresi
//!       │
//!  pe_exec::execute_loaded() → Ring-3 kullanıcı moduna geçiş
//! ```
//!
//! ## IAT (Import Address Table) Nedir?
//!
//! Windows EXE'ler, kullandıkları DLL fonksiyonlarının adreslerini
//! `.idata` bölümündeki IAT içinde tutarlar. Yükleyici (biz), bu adres
//! tablettini kernel'deki Win32 emulasyon stub'larıyla üst yazar.
//! Oyun `CALL [IAT_ptr]` çalıştırınca direkt bizim stub'a düşer.
//!
//! ## goblin Neden?
//!
//! `goblin` crate'i `no_std + alloc` uyumlu, sıfır-unsafe PE ayrıştırıcı.
//! Manuel byte-twiddling yerine tip güvenli API sunar.
//!
//! ```text
//!  Raw .exe bytes
//!       │
//!       ▼
//!  ┌──────────────────────────────────────┐
//!  │ 1. goblin::pe::PE::parse()           │  Header, sections, imports, exports
//!  └──────────────┬───────────────────────┘
//!                 │
//!  ┌──────────────▼───────────────────────┐
//!  │ 2. Section Flattening                │  RVA → physical layout, zero BSS
//!  └──────────────┬───────────────────────┘
//!                 │
//!  ┌──────────────▼───────────────────────┐
//!  │ 3. Base Relocation (.reloc)          │  delta = load_base − preferred_base
//!  └──────────────┬───────────────────────┘
//!                 │
//!  ┌──────────────▼───────────────────────┐
//!  │ 4. IAT Patching                      │  Win32 fn → pe_exec stub address
//!  └──────────────┬───────────────────────┘
//!                 │
//!  ┌──────────────▼───────────────────────┐
//!  │ 5. Export Table                      │  DLL re-export registry
//!  └──────────────┬───────────────────────┘
//!                 │
//!  ┌──────────────▼───────────────────────┐
//!  │ 6. IronShim-rs Sandbox               │  Capability manifest, syscall filter
//!  └──────────────┬───────────────────────┘
//!                 │
//!              pe_exec::execute_loaded()  →  Ring-3
//! ```
//!
//! ## IronShim Sandbox Model
//!
//! Each loaded PE receives a [`SandboxHandle`] that holds an IronShim capability
//! ticket.  The ticket:
//! * Enforces a `SyscallPolicy` allowing only the Win32-emulation syscall range
//! * Sets `max_heap` and `max_stack` from the PE optional header
//! * Routes any fault (page fault, GPF) to echOS's fault handler rather than
//!   the global IDT, preventing a misbehaving .exe from crashing the kernel.
//!
//! When the `SandboxHandle` is dropped, the ticket number is deregistered and
//! all sandbox resources are reclaimed.

#![allow(dead_code)]

use goblin::pe::PE;
use goblin::container::{Container, Ctx, Endian};

use alloc::vec::Vec;
use alloc::vec;
use alloc::string::{String, ToString};
use alloc::collections::BTreeMap;
use alloc::boxed::Box;
use alloc::format;
use spin::Mutex;
use core::sync::atomic::{AtomicU64, Ordering};

use x86_64::structures::paging::PageTableFlags;

pub use crate::pe_exec::{WIN32_STUB_VIRT, ECHOS_WIN32_BASE, STUB_SIZE};

// ============================================================================
// PE SECTION CHARACTERISTIC FLAGS
// ============================================================================

const SCN_MEM_EXECUTE: u32 = 0x2000_0000;
const SCN_MEM_READ:    u32 = 0x4000_0000;
const SCN_MEM_WRITE:   u32 = 0x8000_0000;
const SCN_CNT_UDATA:   u32 = 0x0000_0080; // BSS / uninitialized data

// ============================================================================
// BASE RELOCATION TYPE IDS (PE spec §6.6)
// ============================================================================

const IMAGE_REL_BASED_ABSOLUTE: u8 = 0;  // Padding — ignore
const IMAGE_REL_BASED_HIGH:     u8 = 1;  // High 16 bits
const IMAGE_REL_BASED_LOW:      u8 = 2;  // Low  16 bits
const IMAGE_REL_BASED_HIGHLOW:  u8 = 3;  // Full 32-bit delta
const IMAGE_REL_BASED_DIR64:    u8 = 10; // 64-bit absolute pointer (used by PE32+)

// ============================================================================
// DATA DIRECTORY INDICES
// ============================================================================

const DD_EXPORT: usize = 0;
const DD_IMPORT: usize = 1;
const DD_RELOC:  usize = 5;
const DD_TLS:    usize = 9;

// ============================================================================
// ERRORS
// ============================================================================

/// All failure modes for the PE loading pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeError {
    /// DOS "MZ" magic not found
    InvalidDosHeader,
    /// "PE\0\0" signature missing
    InvalidPeSignature,
    /// Not a PE32+ (64-bit) image
    NotPe64,
    /// A section header is malformed
    InvalidSection,
    /// `goblin` returned a parse error
    ParseError,
    /// IMAGE_FILE_EXECUTABLE_IMAGE bit is clear
    NotExecutable,
    /// Heap allocation failure
    MemoryAllocation,
    /// Base relocation table is corrupt
    RelocationFailed,
    /// IronShim sandbox creation failed
    SandboxFailed,
    /// An imported symbol cannot be resolved
    ImportNotFound,
    /// Export table is corrupt
    InvalidExport,
}

// ============================================================================
// MAPPED SECTION   — per-section descriptor built from goblin output
// ============================================================================

/// One section from the image, ready to be registered as a lazy region.
#[derive(Debug, Clone)]
pub struct MappedSection {
    /// Name from section table (e.g. ".text", ".data", ".reloc")
    pub name:             String,
    /// RVA (relative virtual address — offset from image base)
    pub rva:              u32,
    /// Virtual size (may be larger than `raw_size`; tail is zero-padded BSS)
    pub virtual_size:     u32,
    /// Size of raw data in the file
    pub raw_size:         u32,
    /// Byte offset of the raw bytes inside the *original* image slice
    pub raw_file_offset:  u32,
    /// PE section characteristics (MEM_READ | MEM_WRITE | MEM_EXECUTE | …)
    pub characteristics:  u32,
}

impl MappedSection {
    #[inline] pub fn is_exec(&self)     -> bool { self.characteristics & SCN_MEM_EXECUTE != 0 }
    #[inline] pub fn is_writable(&self) -> bool { self.characteristics & SCN_MEM_WRITE   != 0 }
    #[inline] pub fn is_readable(&self) -> bool { self.characteristics & SCN_MEM_READ    != 0 }
    #[inline] pub fn is_bss(&self)      -> bool { self.characteristics & SCN_CNT_UDATA   != 0 }
}

// ============================================================================
// IAT PATCH ENTRY   — internal bookkeeping for Stage 4
// ============================================================================

#[derive(Debug)]
struct IatPatch {
    /// Flat-image byte offset of the 8-byte pointer slot that was patched
    flat_offset: usize,
    /// Index in the Win32 dispatch table (pe_exec::WIN32_DISPATCH)
    func_idx:    usize,
}

// ============================================================================
// EXPORT ENTRY
// ============================================================================

/// One exported symbol from a loaded DLL, stored in the global DLL cache.
#[derive(Debug, Clone)]
pub struct ExportEntry {
    pub name:            String,
    /// Absolute virtual address (load_base + export_rva)
    pub virtual_address: u64,
}

// ============================================================================
// LOADED PE — the artifact produced by GoblinPeLoader::load()
// ============================================================================

/// A fully parsed, relocated, and IAT-patched PE image awaiting Ring-3 entry.
///
/// `flat` is a heap-allocated, 'static buffer whose layout is:
/// ```text
/// ┌────────────────────── flat ──────────────────────┐
/// │ [0 .. image_size)  : flattened PE sections       │
/// │ [image_size ..)    : Win32 syscall stub page      │
/// └──────────────────────────────────────────────────┘
/// ```
pub struct LoadedPe {
    /// Combined flat-image + stub buffer (leaked — lives for the process lifetime)
    pub flat:             &'static mut [u8],
    /// Kernel-VA of `flat[0]` (used to construct file-backed lazy regions)
    pub flat_kernel_va:   u64,
    /// Preferred image base from the PE optional header
    pub preferred_base:   u64,
    /// Actual load base (may differ if ASLR relocates the image)
    pub load_base:        u64,
    /// Absolute entry-point: load_base + address_of_entry_point
    pub entry:            u64,
    /// size_of_image from the PE optional header
    pub image_size:       u32,
    /// Per-section descriptors in section-table order
    pub sections:         Vec<MappedSection>,
    /// Export table (populated for DLL images; empty for EXEs)
    pub exports:          BTreeMap<String, ExportEntry>,
    /// IronShim fault-containment ticket
    pub sandbox:          SandboxHandle,
    /// User-space stack top address (from memory::user_stack_bounds())
    pub user_stack_top:   u64,
    /// Byte offset inside `flat` where the Win32 stub page begins
    pub stubs_flat_offset: usize,
}

// ============================================================================
// IRONSHIM SANDBOX HANDLE
// ============================================================================

/// A live IronShim-rs sandbox ticket.
///
/// The handle wraps the numeric ID and resource limits returned by
/// `ironshim_bridge::notify_sandbox_create()`.  Dropping the handle calls
/// `notify_sandbox_destroy()` which tears down the fault-containment fence
/// and reclaims all kernel resources associated with the sandbox.
pub struct SandboxHandle {
    pub id:        u64,
    pub max_heap:  u64,
    pub max_stack: u64,
    pub armed:     bool,
}

static NEXT_SANDBOX_ID: AtomicU64 = AtomicU64::new(1);

impl SandboxHandle {
    /// Allocate a new sandbox for a PE process with the given resource limits.
    pub fn create(max_heap: u64, max_stack: u64) -> Result<Self, PeError> {
        let id = NEXT_SANDBOX_ID.fetch_add(1, Ordering::Relaxed);
        crate::ironshim_bridge::notify_sandbox_create(id, max_heap, max_stack);
        crate::serial_println!(
            "[IronShim] Sandbox #{} armed  heap_cap={:#x}  stack_cap={:#x}",
            id, max_heap, max_stack
        );
        Ok(SandboxHandle { id, max_heap, max_stack, armed: true })
    }

    /// Teardown the sandbox; idempotent.
    pub fn destroy(&mut self) {
        if self.armed {
            crate::ironshim_bridge::notify_sandbox_destroy(self.id);
            crate::serial_println!("[IronShim] Sandbox #{} destroyed", self.id);
            self.armed = false;
        }
    }
}

impl Drop for SandboxHandle {
    fn drop(&mut self) { self.destroy(); }
}

// ============================================================================
// GOBLIN PE LOADER
// ============================================================================

/// Stateless loader — call [`GoblinPeLoader::load`] to run the full pipeline.
pub struct GoblinPeLoader;

impl GoblinPeLoader {

    // ─── Stage 1 ──────────────────────────────────────────────────────────────

    /// Invoke `goblin::pe::PE::parse()` and validate the result is PE32+.
    fn stage1_parse<'a>(bytes: &'a [u8]) -> Result<PE<'a>, PeError> {
        PE::parse(bytes).map_err(|e| {
            crate::serial_println!("[PeLoader] goblin error: {:?}", e);
            PeError::ParseError
        })
    }

    // ─── Stage 2 ──────────────────────────────────────────────────────────────

    /// Allocate a zeroed `vec![0u8; size_of_image]` and blit each section's
    /// raw data to its virtual-address offset.  BSS sections (raw_size == 0)
    /// remain zeroed by the initial allocation.
    fn stage2_flatten(
        bytes:       &[u8],
        sections:    &[MappedSection],
        image_size:  u32,
    ) -> Result<Vec<u8>, PeError> {
        let sz = image_size as usize;
        let mut flat = vec![0u8; sz];

        for sec in sections {
            if sec.raw_size == 0 || sec.raw_file_offset == 0 { continue; }

            let src_start = sec.raw_file_offset as usize;
            let src_end   = src_start.saturating_add(sec.raw_size as usize).min(bytes.len());
            let dst_start = sec.rva as usize;
            let copy_len  = (src_end - src_start).min(sz.saturating_sub(dst_start));

            if copy_len > 0 {
                flat[dst_start..dst_start + copy_len]
                    .copy_from_slice(&bytes[src_start..src_start + copy_len]);
            }
        }

        crate::serial_println!("[PeLoader] Stage 2: flat image {} bytes", flat.len());
        Ok(flat)
    }

    // ─── Stage 3 ──────────────────────────────────────────────────────────────

    /// Walk the `.reloc` data directory and apply all `DIR64` / `HIGHLOW`
    /// fixups so the image works at `load_base` even if it differs from the
    /// preferred base in the optional header.
    ///
    /// Base relocation block layout (PE spec §6.6):
    /// ```text
    /// struct BLOCK { page_rva: u32, block_size: u32, entries: [u16; …] }
    /// entry = (type << 12) | page_offset12
    /// ```
    fn stage3_relocate(
        flat:           &mut [u8],
        pe:             &PE<'_>,
        preferred_base: u64,
        load_base:      u64,
    ) -> Result<(), PeError> {
        let delta = load_base.wrapping_sub(preferred_base) as i64;
        if delta == 0 { return Ok(()); }

        let opt = match pe.header.optional_header.as_ref() {
            Some(o) => o,
            None    => return Ok(()),
        };

        // goblin 0.8 stores data directories as Option<(usize, DataDirectory)>
        let (reloc_rva, reloc_size) = match opt.data_directories.data_directories[DD_RELOC] {
            Some((_, ref dd)) => (dd.virtual_address as usize, dd.size as usize),
            None => (0, 0),
        };

        if reloc_rva == 0 || reloc_size == 0 {
            crate::serial_println!("[PeLoader] No .reloc — treating as fixed-base image");
            return Ok(());
        }
        if reloc_rva + reloc_size > flat.len() {
            return Err(PeError::RelocationFailed);
        }

        let mut cursor    = reloc_rva;
        let     reloc_end = reloc_rva + reloc_size;
        let mut fixup_count = 0usize;

        while cursor + 8 <= reloc_end {
            let page_rva   = u32::from_le_bytes(flat[cursor..cursor+4].try_into().unwrap()) as usize;
            let block_size = u32::from_le_bytes(flat[cursor+4..cursor+8].try_into().unwrap()) as usize;

            if block_size < 8 || cursor + block_size > reloc_end { break; }

            let mut e = cursor + 8;
            while e + 2 <= cursor + block_size {
                let entry      = u16::from_le_bytes(flat[e..e+2].try_into().unwrap());
                let rtype      = (entry >> 12) as u8;
                let page_off   = (entry & 0x0FFF) as usize;
                let target_rva = page_rva + page_off;
                e += 2;

                match rtype {
                    IMAGE_REL_BASED_ABSOLUTE => {}

                    IMAGE_REL_BASED_DIR64 => {
                        if target_rva + 8 <= flat.len() {
                            let old = i64::from_le_bytes(
                                flat[target_rva..target_rva+8].try_into().unwrap()
                            );
                            flat[target_rva..target_rva+8]
                                .copy_from_slice(&old.wrapping_add(delta).to_le_bytes());
                            fixup_count += 1;
                        }
                    }

                    IMAGE_REL_BASED_HIGHLOW => {
                        if target_rva + 4 <= flat.len() {
                            let old = u32::from_le_bytes(
                                flat[target_rva..target_rva+4].try_into().unwrap()
                            );
                            flat[target_rva..target_rva+4]
                                .copy_from_slice(&old.wrapping_add(delta as u32).to_le_bytes());
                            fixup_count += 1;
                        }
                    }

                    IMAGE_REL_BASED_HIGH => {
                        if target_rva + 2 <= flat.len() {
                            let old = u16::from_le_bytes(
                                flat[target_rva..target_rva+2].try_into().unwrap()
                            );
                            flat[target_rva..target_rva+2]
                                .copy_from_slice(&old.wrapping_add((delta >> 16) as u16).to_le_bytes());
                            fixup_count += 1;
                        }
                    }

                    IMAGE_REL_BASED_LOW => {
                        if target_rva + 2 <= flat.len() {
                            let old = u16::from_le_bytes(
                                flat[target_rva..target_rva+2].try_into().unwrap()
                            );
                            flat[target_rva..target_rva+2]
                                .copy_from_slice(&old.wrapping_add(delta as u16).to_le_bytes());
                            fixup_count += 1;
                        }
                    }

                    _ => {
                        crate::serial_println!(
                            "[PeLoader] Unknown reloc type={} at rva={:#x}", rtype, target_rva
                        );
                    }
                }
            }
            cursor += block_size;
        }

        crate::serial_println!(
            "[PeLoader] Stage 3: {} fixups applied (delta={:+#x})", fixup_count, delta
        );
        Ok(())
    }

    // ─── Stage 4 ──────────────────────────────────────────────────────────────

    /// Walk goblin's `pe.imports` list.  For each symbol, ask
    /// `pe_exec::register_import(dll, name)` for a stable stub index, then
    /// overwrite the flat-image IAT slot at `import.rva` with:
    ///
    /// ```text
    /// stub_addr = WIN32_STUB_VIRT + func_idx * STUB_SIZE
    /// ```
    ///
    /// Goblin guarantees that `import.rva` points to the IAT (first-thunk)
    /// slot, not the INT, so the write is directly into the call target.
    fn stage4_patch_iat(flat: &mut [u8], pe: &PE<'_>) -> Vec<IatPatch> {
        let mut patches = Vec::new();

        for import in &pe.imports {
            let rva = import.rva;
            if rva == 0 || rva + 8 > flat.len() {
                crate::serial_println!(
                    "[IAT] Skip {}!{} — bad rva {:#x}", import.dll, import.name, rva
                );
                continue;
            }

            let func_idx  = crate::pe_exec::register_import(import.dll, import.name.as_ref());
            let stub_addr = WIN32_STUB_VIRT + (func_idx * STUB_SIZE) as u64;
            flat[rva..rva+8].copy_from_slice(&stub_addr.to_le_bytes());

            crate::serial_println!(
                "[IAT]  {:32} {:32}  rva={:#08x} → stub[{:3}]={:#x}",
                import.dll, import.name, rva, func_idx, stub_addr
            );

            patches.push(IatPatch { flat_offset: rva, func_idx });
        }

        crate::serial_println!("[PeLoader] Stage 4: {} IAT slots patched", patches.len());
        patches
    }

    // ─── Stage 5 ──────────────────────────────────────────────────────────────

    /// Build the DLL export map from goblin's `pe.exports`.
    /// Each export gets an absolute VA entry (load_base + export_rva).
    fn stage5_exports(pe: &PE<'_>, load_base: u64) -> BTreeMap<String, ExportEntry> {
        let mut map = BTreeMap::new();
        for exp in &pe.exports {
            if let Some(name) = exp.name {
                let rva = exp.rva; // usize — always present for real exports
                map.insert(name.to_string(), ExportEntry {
                    name:            name.to_string(),
                    virtual_address: load_base + rva as u64,
                });
            }
        }
        if !map.is_empty() {
            crate::serial_println!("[PeLoader] Stage 5: {} exports indexed", map.len());
        }
        map
    }

    // ─── Main pipeline ────────────────────────────────────────────────────────

    /// Run the full 6-stage pipeline.
    ///
    /// Returns a [`LoadedPe`] that owns the flat image buffer and IronShim
    /// sandbox ticket.  Call `pe_exec::execute_loaded(loaded)` to enter Ring-3.
    pub fn load(bytes: &[u8]) -> Result<LoadedPe, PeError> {
        crate::serial_println!("[PeLoader] ── Iron-Proton pipeline start ({} bytes) ──", bytes.len());

        // ── Stage 1: Parse ────────────────────────────────────────────────────
        let pe = Self::stage1_parse(bytes)?;

        if !pe.is_64 { return Err(PeError::NotPe64); }

        let opt = pe.header.optional_header.ok_or(PeError::InvalidPeSignature)?;

        let preferred_base  = opt.windows_fields.image_base;
        let image_size      = opt.windows_fields.size_of_image;
        let entry_rva       = opt.standard_fields.address_of_entry_point;
        let stack_reserve   = opt.windows_fields.size_of_stack_reserve;
        let heap_reserve    = opt.windows_fields.size_of_heap_reserve;

        crate::serial_println!(
            "[PeLoader] preferred_base={:#x}  image_size={:#x}  entry_rva={:#x}",
            preferred_base, image_size, entry_rva
        );

        // ── Build section descriptors ─────────────────────────────────────────
        let sections: Vec<MappedSection> = pe.sections.iter().map(|s| {
            let name = core::str::from_utf8(&s.name)
                .unwrap_or("????????")
                .trim_end_matches('\0')
                .to_string();
            MappedSection {
                name,
                rva:             s.virtual_address,
                virtual_size:    s.virtual_size,
                raw_size:        s.size_of_raw_data,
                raw_file_offset: s.pointer_to_raw_data,
                characteristics: s.characteristics,
            }
        }).collect();

        // Print section table
        crate::serial_println!("[PeLoader] Section table ({} sections):", sections.len());
        for s in &sections {
            crate::serial_println!(
                "    {:8} rva={:#010x} vsz={:#08x} chars={:#010x} [r={} w={} x={}]",
                s.name, s.rva, s.virtual_size, s.characteristics,
                s.is_readable() as u8, s.is_writable() as u8, s.is_exec() as u8
            );
        }

        // ── Stage 2: Flatten ──────────────────────────────────────────────────
        let mut flat = Self::stage2_flatten(bytes, &sections, image_size)?;

        // ── ASLR decision ─────────────────────────────────────────────────────
        // Production: call memory::alloc_aslr_base(image_size) for a random VA.
        // For now, honour the preferred base so no reloc pass is needed
        // (simplifies Ring-3 page-table wiring).
        let load_base = preferred_base;

        // ── Stage 3: Base relocations ─────────────────────────────────────────
        Self::stage3_relocate(&mut flat, &pe, preferred_base, load_base)?;

        // ── Stage 4: IAT patch ────────────────────────────────────────────────
        let _patches  = Self::stage4_patch_iat(&mut flat, &pe);
        let num_stubs = crate::pe_exec::registered_import_count();

        // ── Append Win32 stub page ────────────────────────────────────────────
        let stubs_flat_offset = flat.len();
        let stubs_bytes       = crate::pe_exec::generate_stubs_for(num_stubs);
        flat.extend_from_slice(&stubs_bytes);

        // ── Stage 5: Exports ──────────────────────────────────────────────────
        let exports = Self::stage5_exports(&pe, load_base);

        // ── Leak flat buffer for 'static lifetime ─────────────────────────────
        let flat_kernel_va: u64 = flat.as_ptr() as u64;
        let flat_static: &'static mut [u8] = Box::leak(flat.into_boxed_slice());

        // ── Stage 6: IronShim sandbox ─────────────────────────────────────────
        let sandbox = SandboxHandle::create(heap_reserve, stack_reserve)?;

        let (_, user_stack_top) = crate::memory::user_stack_bounds();

        let loaded = LoadedPe {
            flat:              flat_static,
            flat_kernel_va,
            preferred_base,
            load_base,
            entry:             load_base + entry_rva as u64,
            image_size,
            sections,
            exports,
            sandbox,
            user_stack_top,
            stubs_flat_offset,
        };

        crate::serial_println!(
            "[PeLoader] ── Pipeline complete ──  entry={:#x}  sandbox=#{}  total={} bytes",
            loaded.entry, loaded.sandbox.id, loaded.flat.len()
        );

        Ok(loaded)
    }
}

// ============================================================================
// GLOBAL DLL CACHE  — maps lowercase DLL stem → export table
// ============================================================================

struct LoadedDllRef {
    exports: BTreeMap<String, ExportEntry>,
}

static DLL_CACHE: Mutex<BTreeMap<String, LoadedDllRef>> = Mutex::new(BTreeMap::new());

/// Look up a DLL export by DLL name and function name.
/// Used by cross-DLL IAT resolution after a DLL is loaded.
pub fn resolve_dll_export(dll: &str, func: &str) -> Option<u64> {
    let key = dll
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(dll)
        .to_ascii_lowercase()
        .trim_end_matches(".dll")
        .to_string();
    DLL_CACHE.lock()
        .get(&key)?
        .exports.get(func)
        .map(|e| e.virtual_address)
}

/// Register a loaded DLL's exports in the global cache.
pub fn register_dll(name: &str, exports: BTreeMap<String, ExportEntry>) {
    let key = name
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase()
        .trim_end_matches(".dll")
        .to_string();
    DLL_CACHE.lock().insert(key, LoadedDllRef { exports });
}

// ============================================================================
// HIGH-LEVEL ENTRY POINTS
// ============================================================================

/// Parse, load, and immediately execute a PE32+ image in Ring-3.
///
/// Does **not** return on success — jumps to the PE entry point via
/// `pe_exec::execute_loaded()`.
pub fn load_and_run(bytes: &[u8]) -> Result<(), PeError> {
    let loaded = GoblinPeLoader::load(bytes)?;
    crate::pe_exec::execute_loaded(loaded)
        .map_err(|_| PeError::SandboxFailed)
}

/// Load a PE without executing it (used for DLL loading).
pub fn load_pe(data: &[u8]) -> Result<LoadedPe, PeError> {
    GoblinPeLoader::load(data)
}

/// Resolve an import symbol: first checks the kernel Win32 shim, then the DLL cache.
pub fn resolve_import(dll_name: &str, func_name: &str) -> Option<u64> {
    crate::win32::get_proc_address(dll_name, func_name)
        .or_else(|| resolve_dll_export(dll_name, func_name))
}
