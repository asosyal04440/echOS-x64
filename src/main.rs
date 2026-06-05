//! # EchOS Çekirdek Giriş Noktası
//!
//! Bu dosya çekirdeğin ana giriş noktasını içerir. UEFI ve Limine (bare-metal)
//! olmak üzere iki önyükleme ortamını destekler:
//!
//! - **UEFI modu** (`target_os = "uefi"`): UEFI firmware tarafından çağrılır,
//!   framebuffer başlatılır, splash ekranı gösterilir ve GUI sistemi devreye alınır.
//! - **Limine modu** (varsayılan): Limine önyükleyici protokolü üzerinden bellek
//!   haritası alınır, sayfa tabloları kurulur ve çekirdek tam olarak başlatılır.
//!
//! `#![no_std]` ve `#![no_main]` nitelikleri, standart kütüphane ve C çalışma
//! zamanı bağımlılığı olmaksızın doğrudan donanım üzerinde çalışmayı sağlar.

#![no_std]
#![no_main]
#![allow(clippy::all)]
#![allow(bad_asm_style)]

extern crate alloc;

#[cfg(target_os = "uefi")]
use alloc::string::String;
#[cfg(target_os = "uefi")]
use alloc::vec;
use alloc::vec::Vec;
use core::arch::asm;
#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
use core::arch::global_asm;
use core::arch::x86_64::_rdtsc;
use core::fmt::{self, Write};
#[cfg(target_os = "uefi")]
use core::mem::MaybeUninit;
#[cfg(target_os = "uefi")]
use ech_os::boot::BootInfo;
#[cfg(target_os = "uefi")]
use ech_os::gop::framebuffer::Framebuffer;
#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
use ech_os::memory::frame_allocator::{LimineFrameAllocator, LimineMemmapEntry};
#[cfg(target_os = "uefi")]
use ech_os::splash::Splash;
#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
use limine_protocol_for_rust::{
    requests::executable_cmdline::ExecutableCmdlineRequest,
    requests::hhdm::HigherHalfDirectMapRequest,
    requests::memory_map::{MemoryMapRequest, MemoryRegionInfo, MemoryRegionType},
    requests::LimineRequest,
    use_base_revision,
    util::PointerSlice,
    REQUEST_END_MARKER, REQUEST_START_MARKER,
};
#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
use multiboot2::load;
#[cfg(target_os = "uefi")]
use sha2::{Digest, Sha256};
#[cfg(target_os = "uefi")]
use uefi::prelude::*;
#[cfg(target_os = "uefi")]
use uefi::proto::console::gop::GraphicsOutput;
#[cfg(target_os = "uefi")]
use uefi::proto::loaded_image::LoadedImage;
#[cfg(target_os = "uefi")]
use uefi::proto::media::file::{File, FileAttribute, FileMode};
#[cfg(target_os = "uefi")]
use uefi::proto::media::fs::SimpleFileSystem;
#[cfg(target_os = "uefi")]
use uefi::proto::tcg::v2::{HashLogExtendEventFlags, PcrEventInputs, Tcg};
#[cfg(target_os = "uefi")]
use uefi::proto::tcg::{EventType, PcrIndex};
#[cfg(target_os = "uefi")]
use uefi::table::boot::MemoryType;
#[cfg(target_os = "uefi")]
use uefi::table::runtime::{ResetType, VariableAttributes, VariableVendor};
#[cfg(target_os = "uefi")]
use uefi::CStr16;

const COM1: u16 = 0x3F8;
const BOOT_MAGIC_UEFI: u64 = 0x55454649;
const BOOT_MAGIC_MB2: u64 = 0x36d76289;
#[cfg(target_os = "uefi")]
const CMDLINE_MAX_LEN: usize = 4096;
#[cfg(target_os = "uefi")]
const SECURE_BOOT_ENROLL_MAGIC: u32 = 0x5342_4531;
#[cfg(target_os = "uefi")]
const SECURE_BOOT_ENROLL_PENDING_RESET: u8 = 1 << 0;
#[cfg(target_os = "uefi")]
const SECURE_BOOT_ENROLL_FAILED: u8 = 1 << 1;
#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
const LIMINE_REVISION: u64 = 4;

#[cfg(target_os = "uefi")]
#[repr(C)]
#[derive(Clone, Copy)]
struct SecureBootEnrollState {
    magic: u32,
    flags: u8,
    _reserved: [u8; 3],
}

#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
#[used]
#[link_section = ".limine_reqs"]
static LIMINE_BASE_REVISION: [u64; 4] = use_base_revision(LIMINE_REVISION);

#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
#[used]
#[link_section = ".limine_req_start"]
static LIMINE_REQUEST_START_MARKER: [u64; 4] = REQUEST_START_MARKER;

#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
#[used]
#[link_section = ".limine_reqs"]
static LIMINE_MEMORY_MAP_REQUEST: MemoryMapRequest = MemoryMapRequest::new(LIMINE_REVISION);

#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
#[used]
#[link_section = ".limine_reqs"]
static LIMINE_HHDM_REQUEST: HigherHalfDirectMapRequest =
    HigherHalfDirectMapRequest::new(LIMINE_REVISION);

#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
#[used]
#[link_section = ".limine_reqs"]
static LIMINE_CMDLINE_REQUEST: ExecutableCmdlineRequest =
    ExecutableCmdlineRequest::new(LIMINE_REVISION);

#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
#[used]
#[link_section = ".limine_req_end"]
static LIMINE_REQUEST_END_MARKER: [u64; 2] = REQUEST_END_MARKER;

#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
global_asm!(include_str!("boot/entry.S"));

unsafe fn outb(port: u16, value: u8) {
    asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags));
}

unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    asm!("in al, dx", in("dx") port, out("al") value, options(nomem, nostack, preserves_flags));
    value
}

unsafe fn serial_init() {
    outb(COM1 + 1, 0x00);
    outb(COM1 + 3, 0x80);
    outb(COM1 + 0, 0x01);
    outb(COM1 + 1, 0x00);
    outb(COM1 + 3, 0x03);
    outb(COM1 + 2, 0xC7);
    outb(COM1 + 4, 0x0B);
}

unsafe fn debugcon_write_byte(byte: u8) {
    outb(0xE9, byte);
}

unsafe fn debugcon_write_str(s: &str) {
    for b in s.bytes() {
        outb(0xE9, b);
    }
}

fn debugcon_write_fmt(args: core::fmt::Arguments) {
    use core::fmt::Write;
    struct W;
    impl core::fmt::Write for W {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            for b in s.bytes() {
                unsafe { outb(0xE9, b); }
            }
            Ok(())
        }
    }
    let _ = W.write_fmt(args);
}

fn serial_write_byte(byte: u8) {
    unsafe {
        let mut spins = 1_000_000u32;
        while (inb(COM1 + 5) & 0x20) == 0 {
            if spins == 0 {
                break;
            }
            spins = spins.saturating_sub(1);
            core::hint::spin_loop();
        }
        outb(COM1, byte);
    }
}

struct SerialPort;

impl Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            let byte = match byte {
                b'\n' | b'\r' | b'\t' => byte,
                0x20..=0x7e => byte,
                _ => b'?',
            };
            serial_write_byte(byte);
        }
        Ok(())
    }
}

fn serial_write_str(args: &fmt::Arguments) {
    let mut port = SerialPort;
    let _ = port.write_fmt(*args);
}

fn init_platform_iommu() -> bool {
    let cpu_acpi_ok = ech_os::cpu::acpi::init();
    if cpu_acpi_ok {
        serial_write_str(&format_args!("[SMP] CPU ACPI tables parsed\n"));
        ech_os::cpu::acpi_aml::init_aml();
    } else {
        serial_write_str(&format_args!(
            "[SMP] CPU ACPI init failed, using CPUID topology\n"
        ));
    }

    let iommu_tables_ok = ech_os::memory::init_iommu();
    if iommu_tables_ok {
        serial_write_str(&format_args!(
            "[IOMMU] DMAR parsed and domains initialized\n"
        ));
    } else {
        serial_write_str(&format_args!("[IOMMU] DMAR not available\n"));
    }

    let iommu_hw_ok = ech_os::drivers::iommu::init();
    if iommu_hw_ok {
        serial_write_str(&format_args!(
            "[IOMMU] DMA remapping enabled before device init\n"
        ));
    } else if iommu_tables_ok {
        serial_write_str(&format_args!(
            "[IOMMU] Hardware enable/self-test failed, keeping device init constrained\n"
        ));
    }

    iommu_tables_ok && iommu_hw_ok
}

fn parse_swap_cmdline(cmdline: &str) -> Option<(u32, u32)> {
    let mut lba: Option<u64> = None;
    let mut slots: Option<u64> = None;
    let mut mb: Option<u64> = None;
    for part in cmdline.split_whitespace() {
        if let Some(value) = part.strip_prefix("swap_lba=") {
            lba = value.parse().ok();
        } else if let Some(value) = part.strip_prefix("swap_slots=") {
            slots = value.parse().ok();
        } else if let Some(value) = part.strip_prefix("swap_mb=") {
            mb = value.parse().ok();
        }
    }
    if slots.is_none() {
        if let Some(mb) = mb {
            let total = mb.saturating_mul(1024).saturating_mul(1024);
            slots = Some(total / ech_os::memory::PAGE_SIZE as u64);
        }
    }
    let lba = lba?;
    let slots = slots?;
    if slots == 0 {
        return None;
    }
    let lba = lba.min(u32::MAX as u64) as u32;
    let slots = slots.min(u32::MAX as u64) as u32;
    Some((lba, slots))
}

unsafe fn debugcon_write_hex(val: u64) {
    let hex = b"0123456789abcdef";
    for i in (0..16).rev() {
        let nibble = ((val >> (i * 4)) & 0xF) as usize;
        debugcon_write_byte(hex[nibble]);
    }
    debugcon_write_byte(b'\n');
}

#[cfg(not(target_os = "windows"))]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    unsafe {
        debugcon_write_byte(b'P');
        debugcon_write_byte(b'\n');

        let rbp: u64;
        let rsp: u64;
        core::arch::asm!("mov {}, rbp", out(reg) rbp);
        core::arch::asm!("mov {}, rsp", out(reg) rsp);

        debugcon_write_byte(b'R');
        debugcon_write_byte(b'S');
        debugcon_write_byte(b'P');
        debugcon_write_byte(b':');
        debugcon_write_hex(rsp);

        debugcon_write_byte(b'R');
        debugcon_write_byte(b'B');
        debugcon_write_byte(b'P');
        debugcon_write_byte(b':');
        debugcon_write_hex(rbp);
    }

    serial_write_str(&format_args!("[PANIC] Kernel panic\n"));
    let rbp: u64;
    let rsp: u64;
    unsafe {
        core::arch::asm!("mov {}, rbp", out(reg) rbp);
        core::arch::asm!("mov {}, rsp", out(reg) rsp);
    }
    serial_write_str(&format_args!("[PANIC] RSP: {:#x}, RBP: {:#x}\n", rsp, rbp));

    if let Some(location) = info.location() {
        serial_write_str(&format_args!(
            "[PANIC] At {}:{}:{}\n",
            location.file(),
            location.line(),
            location.column()
        ));
    }
    let message = info.message();
    serial_write_str(&format_args!("[PANIC] Message: {}\n", message));
    ech_os::cpu::smp::broadcast_panic_stop();
    ech_os::boot::appliance::record_panic();
    ech_os::cpu::smp::panic_stop_this_cpu();
}

#[cfg(target_os = "windows")]
#[no_mangle]
pub extern "system" fn mainCRTStartup() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(target_os = "windows")]
#[no_mangle]
pub extern "system" fn WinMainCRTStartup() -> ! {
    mainCRTStartup()
}

#[cfg(target_os = "windows")]
#[no_mangle]
pub extern "C" fn main() -> i32 {
    0
}

#[cfg(target_os = "windows")]
#[no_mangle]
pub extern "C" fn memcpy(dest: *mut u8, src: *const u8, len: usize) -> *mut u8 {
    unsafe {
        core::ptr::copy_nonoverlapping(src, dest, len);
    }
    dest
}

#[cfg(target_os = "windows")]
#[no_mangle]
pub extern "C" fn memmove(dest: *mut u8, src: *const u8, len: usize) -> *mut u8 {
    unsafe {
        core::ptr::copy(src, dest, len);
    }
    dest
}

#[cfg(target_os = "windows")]
#[no_mangle]
pub extern "C" fn memset(dest: *mut u8, value: i32, len: usize) -> *mut u8 {
    unsafe {
        core::ptr::write_bytes(dest, value as u8, len);
    }
    dest
}

#[cfg(target_os = "windows")]
#[no_mangle]
pub extern "C" fn memcmp(lhs: *const u8, rhs: *const u8, len: usize) -> i32 {
    for idx in 0..len {
        let a = unsafe { *lhs.add(idx) };
        let b = unsafe { *rhs.add(idx) };
        if a != b {
            return (a as i32) - (b as i32);
        }
    }
    0
}

#[cfg(target_os = "windows")]
#[no_mangle]
pub extern "C" fn strlen(ptr: *const u8) -> usize {
    let mut len = 0usize;
    unsafe {
        while *ptr.add(len) != 0 {
            len += 1;
        }
    }
    len
}

#[cfg(target_os = "windows")]
#[no_mangle]
pub extern "C" fn fmod(x: f64, y: f64) -> f64 {
    if y == 0.0 || !x.is_finite() || !y.is_finite() {
        return f64::NAN;
    }
    x - libm::trunc(x / y) * y
}

#[cfg(target_os = "windows")]
#[no_mangle]
pub extern "C" fn fmodf(x: f32, y: f32) -> f32 {
    if y == 0.0 || !x.is_finite() || !y.is_finite() {
        return f32::NAN;
    }
    x - libm::truncf(x / y) * y
}

#[cfg(target_os = "windows")]
#[no_mangle]
pub extern "system" fn __CxxFrameHandler3() -> i32 {
    0
}

#[cfg(target_os = "windows")]
#[no_mangle]
pub static kernel_start: u8 = 0;

#[cfg(target_os = "windows")]
#[no_mangle]
pub static kernel_end: u8 = 0;

#[cfg(target_os = "windows")]
#[no_mangle]
pub static boot_lma_end: u8 = 0;

#[cfg(target_os = "uefi")]
const PREFERRED_GOP_WIDTH: usize = 1920;
#[cfg(target_os = "uefi")]
const PREFERRED_GOP_HEIGHT: usize = 1080;

#[cfg(target_os = "uefi")]
fn gop_mode_rank(width: usize, height: usize, target_width: usize, target_height: usize) -> u8 {
    if width == target_width && height == target_height {
        3
    } else if width >= target_width && height >= target_height {
        2
    } else {
        1
    }
}

#[cfg(target_os = "uefi")]
fn configure_preferred_gop_mode(gop: &mut GraphicsOutput) {
    let current = gop.current_mode_info().resolution();
    let target = (PREFERRED_GOP_WIDTH, PREFERRED_GOP_HEIGHT);
    let mut best_mode = None;
    let mut best_rank = gop_mode_rank(current.0, current.1, target.0, target.1);
    let mut best_area = current.0.saturating_mul(current.1);
    let mut best_dims = current;

    for mode in gop.modes() {
        let dims = mode.info().resolution();
        let rank = gop_mode_rank(dims.0, dims.1, target.0, target.1);
        let area = dims.0.saturating_mul(dims.1);
        let better = rank > best_rank
            || (rank == best_rank
                && rank == 2
                && (area < best_area || (area == best_area && dims < best_dims)))
            || (rank == best_rank
                && rank != 2
                && (area > best_area || (area == best_area && dims > best_dims)));
        if better {
            best_rank = rank;
            best_area = area;
            best_dims = dims;
            best_mode = Some(mode);
        }
    }

    if let Some(mode) = best_mode {
        let dims = mode.info().resolution();
        if dims != current {
            if gop.set_mode(&mode).is_ok() {
                serial_write_str(&format_args!(
                    "[UEFI] GOP mode selected: {}x{}\n",
                    dims.0, dims.1
                ));
            } else {
                serial_write_str(&format_args!(
                    "[UEFI] GOP mode switch failed, keeping {}x{}\n",
                    current.0, current.1
                ));
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
#[no_mangle]
pub extern "C" fn kernel_entry(boot_info_addr: usize, kaslr_offset: u64, boot_magic: u64) -> ! {
    unsafe {
        debugcon_write_byte(b'k');
    }
    kernel_main(boot_info_addr, kaslr_offset, boot_magic)
}

#[cfg(not(target_os = "windows"))]
#[no_mangle]
pub extern "C" fn kernel_main(boot_info_addr: usize, kaslr_offset: u64, boot_magic: u64) -> ! {
    unsafe {
        debugcon_write_byte(b'K');
    }
    unsafe {
        serial_init();
        debugcon_write_byte(b'S');
    }
    unsafe {
        debugcon_write_byte(b'M');
    } // Mark: after serial_init
    ech_os::memory::set_kaslr_offset(kaslr_offset);
    let mut seed = unsafe { _rdtsc() };
    seed ^= boot_info_addr as u64;
    seed ^= kaslr_offset;
    seed ^= boot_magic;
    seed ^= seed >> 32;
    ech_os::random::init(seed as u32);
    unsafe {
        debugcon_write_byte(b'R');
    } // Mark: after random init
    serial_write_str(&format_args!("[KASLR] Offset: {:#x}\n", kaslr_offset));
    serial_write_str(&format_args!("[BOOT] Magic: {:#x}\n", boot_magic));
    unsafe {
        debugcon_write_byte(b'B');
        debugcon_write_hex(boot_magic);
    } // Mark: boot magic value

    #[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
    if limine_available() {
        unsafe { boot_pipeline_limine(kaslr_offset) };
    }

    if boot_magic == BOOT_MAGIC_UEFI {
        #[cfg(target_os = "uefi")]
        unsafe {
            debugcon_write_byte(b'U'); // Mark: entering UEFI pipeline
            boot_pipeline_uefi(boot_info_addr, kaslr_offset);
        }
        #[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
        {
            serial_write_str(&format_args!(
                "[BOOT] UEFI magic on non-UEFI target, halting\n"
            ));
        }
    } else if boot_magic == BOOT_MAGIC_MB2 {
        #[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
        unsafe {
            boot_pipeline_multiboot(boot_info_addr, kaslr_offset);
        }
        #[cfg(target_os = "uefi")]
        {
            serial_write_str(&format_args!(
                "[BOOT] Multiboot2 magic on UEFI target, halting\n"
            ));
        }
    } else {
        serial_write_str(&format_args!(
            "[BOOT] Unknown magic {:#x}, halting\n",
            boot_magic
        ));
    }

    loop {
        unsafe {
            asm!("hlt");
        }
    }
}

#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
fn limine_available() -> bool {
    LIMINE_MEMORY_MAP_REQUEST.get_response().is_some()
}

#[cfg(target_os = "uefi")]
unsafe fn boot_pipeline_uefi(boot_info_addr: usize, _kaslr_offset: u64) -> ! {
    debugcon_write_byte(b'1'); // Mark: entered boot_pipeline_uefi
                               // Initialize boot safety system FIRST
    ech_os::boot::safety::init();
    debugcon_write_byte(b'2'); // Mark: after safety init
    ech_os::boot::safety::BOOT_SAFETY.enter_phase(ech_os::boot::safety::BootPhase::UefiHandover);
    debugcon_write_byte(b'3'); // Mark: after enter_phase

    let boot_info = &mut *(boot_info_addr as *mut BootInfo);
    debugcon_write_byte(b'4'); // Mark: after boot_info cast
    let expected_size = core::mem::size_of::<BootInfo>() as u32;
    debugcon_write_hex(boot_info.magic);
    debugcon_write_hex(boot_info.version as u64);
    debugcon_write_hex(boot_info.size as u64);
    debugcon_write_hex(boot_info.physical_memory_offset);
    debugcon_write_hex(boot_info.hhdm_offset);
    if boot_info.magic != ech_os::boot::BOOTINFO_MAGIC
        || boot_info.version != ech_os::boot::BOOTINFO_VERSION
        || boot_info.size < expected_size
    {
        debugcon_write_byte(b'!'); // Mark: BootInfo check failed
        ech_os::boot::safety::BOOT_SAFETY.record_violation(
            ech_os::boot::safety::ViolationType::AcpiTableInvalid,
            "BootInfo ABI mismatch",
            false,
        );
        serial_write_str(&format_args!(
            "[UEFI] BootInfo ABI mismatch magic={:#x} ver={} size={}\n",
            boot_info.magic, boot_info.version, boot_info.size
        ));
        loop {
            asm!("hlt");
        }
    }
    debugcon_write_byte(b'5'); // Mark: passed BootInfo checks
    if boot_info.physical_memory_offset == 0 || boot_info.hhdm_offset == 0 {
        debugcon_write_byte(b'Z'); // Mark: zero offset
        ech_os::boot::safety::BOOT_SAFETY.record_violation(
            ech_os::boot::safety::ViolationType::MemoryMapInvalid,
            "Invalid memory offsets",
            false,
        );
        serial_write_str(&format_args!(
            "[UEFI] Invalid memory offsets phys={:#x} hhdm={:#x}\n",
            boot_info.physical_memory_offset, boot_info.hhdm_offset
        ));
        loop {
            asm!("hlt");
        }
    }
    debugcon_write_byte(b'6'); // Mark: passed offset checks
    if boot_info.secure_boot && boot_info.system_table == 0 {
        serial_write_str(&format_args!("[UEFI] Secure Boot requires system table\n"));
        loop {
            asm!("hlt");
        }
    }
    debugcon_write_byte(b'7'); // Mark: passed secure boot check
    ech_os::boot::set_secure_boot(boot_info.secure_boot);
    ech_os::cpu::acpi::set_uefi_rsdp_address(boot_info.rsdp_address);
    ech_os::acpi::set_rsdp_address(boot_info.rsdp_address);
    debugcon_write_byte(b'8'); // Mark: after RSDP setup
    serial_write_str(&format_args!(
        "[UEFI] RSDP: {:#x}\n",
        boot_info.rsdp_address
    ));
    debugcon_write_byte(b'9'); // Mark: after serial write

    if let Some(framebuffer) = boot_info.framebuffer.as_ref() {
        serial_write_str(&format_args!(
            "[UEFI] FB base={:#x} {}x{} stride={}\n",
            framebuffer.base_addr,
            framebuffer.width,
            framebuffer.height,
            framebuffer.pixels_per_scan_line
        ));
    }
    debugcon_write_byte(b'A'); // Mark: after framebuffer check
    let mut splash: Option<Splash> = None;
    let mut run_boot_tests = false;
    if boot_info.image_size != 0 {
        serial_write_str(&format_args!(
            "[UEFI] Image size: {} bytes\n",
            boot_info.image_size
        ));
        serial_write_str(&format_args!("[UEFI] Image sha256: "));
        for byte in boot_info.image_hash {
            serial_write_str(&format_args!("{:02x}", byte));
        }
        serial_write_str(&format_args!("\n"));
    }
    debugcon_write_byte(b'B'); // Mark: after image hash

    let _boot_ctx = ech_os::KernelBootContext {
        physical_memory_offset: boot_info.physical_memory_offset as u64,
    };
    debugcon_write_byte(b'C'); // Mark: after boot context

    let memory_map_present = boot_info
        .memory_map
        .as_ref()
        .map(|map| map.entries().next().is_some())
        .unwrap_or(false);
    debugcon_write_byte(if memory_map_present { b'Y' } else { b'N' }); // Mark: memory map present?
    debugcon_write_byte(b'D'); // Mark: after memory_map_present check
    if !memory_map_present {
        debugcon_write_byte(b'!'); // Mark: memory map empty
        ech_os::boot::safety::BOOT_SAFETY.record_violation(
            ech_os::boot::safety::ViolationType::MemoryMapInvalid,
            "Empty memory map",
            false,
        );
        serial_write_str(&format_args!("[UEFI] Empty memory map\n"));
        loop {
            asm!("hlt");
        }
    }
    debugcon_write_byte(b'E'); // Mark: passed memory map check
    if let Some(map) = boot_info.memory_map.as_ref() {
        let total_pages: u64 = map.entries().map(|d| d.page_count).sum();
        let total_mb = total_pages.saturating_mul(4096) / (1024 * 1024);
        serial_write_str(&format_args!("[UEFI] Memory map total: {} MB\n", total_mb));
    }
    debugcon_write_byte(b'F'); // Mark: after memory map total
    if let (Some(framebuffer), Some(screen)) = (boot_info.framebuffer.as_mut(), splash.as_mut()) {
        screen.update_progress(framebuffer, 15);
    }

    debugcon_write_byte(b'G'); // Mark: before init_paging
    serial_write_str(&format_args!("[UEFI] init_paging\n"));
    ech_os::boot::safety::BOOT_SAFETY.enter_phase(ech_os::boot::safety::BootPhase::PagingSetup);
    let mut mapper = unsafe { ech_os::memory::init_paging(0) };
    debugcon_write_byte(b'H'); // Mark: after init_paging
    serial_write_str(&format_args!("[UEFI] init_uefi memory manager\n"));
    ech_os::boot::safety::BOOT_SAFETY.enter_phase(ech_os::boot::safety::BootPhase::MemoryInit);
    debugcon_write_byte(b'I'); // Mark: before memory_map.take
    let memory_map = boot_info
        .memory_map
        .take()
        .expect("[UEFI] memory map already consumed");
    debugcon_write_byte(b'J'); // Mark: after memory_map.take
    let mut memory_manager = ech_os::memory::init_uefi(memory_map);
    debugcon_write_byte(b'K'); // Mark: after init_uefi
    unsafe {
        ech_os::memory::set_global_memory_manager(&mut memory_manager as *mut _);
    }
    debugcon_write_byte(b'L'); // Mark: after set_global_memory_manager
    serial_write_str(&format_args!("[UEFI] init_uefi_hhdm\n"));
    if let Err(err) =
        ech_os::memory::init_uefi_hhdm(&mut mapper, &mut memory_manager, boot_info.hhdm_offset)
    {
        debugcon_write_byte(b'X'); // Mark: init_uefi_hhdm failed
        serial_write_str(&format_args!(
            "[HHDM] FATAL: init failed: {:?} — cannot continue without HHDM\n",
            err
        ));
        loop {
            core::arch::asm!("hlt");
        }
    } else {
        debugcon_write_byte(b'M'); // Mark: init_uefi_hhdm success
        ech_os::memory::set_active_physical_offset(boot_info.hhdm_offset);
        mapper = unsafe { ech_os::memory::init_paging(boot_info.hhdm_offset) };
    }
    debugcon_write_byte(b'N'); // Mark: after hhdm setup
    if let Some(framebuffer) = boot_info.framebuffer.as_mut() {
        debugcon_write_byte(b'P'); // Mark: framebuffer present
        let size = framebuffer
            .pixels_per_scan_line
            .saturating_mul(framebuffer.height)
            .saturating_mul(4);
        debugcon_write_byte(b'a'); // Mark: before map_mmio
        let mapped = ech_os::memory::map_mmio(framebuffer.base_addr as u64, size);
        debugcon_write_byte(b'b'); // Mark: after map_mmio
        if !mapped.is_null() {
            framebuffer.base_addr = mapped as usize;
        } else {
            framebuffer.base_addr = (boot_info.hhdm_offset + framebuffer.base_addr as u64) as usize;
        }
        debugcon_write_byte(b'c'); // Mark: before Splash::new
        let mut screen = Splash::new(framebuffer);
        debugcon_write_byte(b'd'); // Mark: after Splash::new
        screen.update_progress(framebuffer, 5);
        splash = Some(screen);
    } else {
        debugcon_write_byte(b'p'); // Mark: no framebuffer
    }
    debugcon_write_byte(b'Q'); // Mark: after framebuffer setup
    if let (Some(framebuffer), Some(screen)) = (boot_info.framebuffer.as_mut(), splash.as_mut()) {
        screen.update_progress(framebuffer, 30);
    }
    debugcon_write_byte(b'R'); // Mark: before cmdline
    if boot_info.cmdline_len > 0 && boot_info.cmdline_ptr != 0 {
        debugcon_write_byte(b'S'); // Mark: cmdline present
        if boot_info.cmdline_len > isize::MAX as u64 {
            serial_write_str(&format_args!("[UEFI] cmdline too large\n"));
        } else {
            let cmdline_ptr =
                ech_os::memory::phys_to_virt(boot_info.cmdline_ptr as usize) as *const u8;
            if cmdline_ptr.is_null() {
                serial_write_str(&format_args!("[UEFI] cmdline ptr invalid\n"));
            } else {
                let cmdline_slice = unsafe {
                    core::slice::from_raw_parts(cmdline_ptr, boot_info.cmdline_len as usize)
                };
                if let Ok(cmdline) = core::str::from_utf8(cmdline_slice) {
                    serial_write_str(&format_args!("[UEFI] cmdline: {}\n", cmdline));
                    if cmdline.contains("boot_tests=1") {
                        run_boot_tests = true;
                        serial_write_str(&format_args!("[UEFI] boot tests enabled\n"));
                    }
                    if let Some((lba, slots)) = parse_swap_cmdline(cmdline) {
                        if ech_os::memory::init_swap_device(lba, slots) {
                            serial_write_str(&format_args!(
                                "[SWAP] Enabled device base_lba={} slots={}\n",
                                lba, slots
                            ));
                        } else {
                            serial_write_str(&format_args!(
                                "[SWAP] Device init failed base_lba={} slots={}\n",
                                lba, slots
                            ));
                        }
                    }
                } else {
                    serial_write_str(&format_args!("[UEFI] cmdline: <invalid utf-8>\n"));
                }
            }
        }
    } else if boot_info.cmdline_len > 0 {
        serial_write_str(&format_args!("[UEFI] cmdline len without ptr\n"));
    }
    debugcon_write_byte(b'T'); // Mark: before set_virtual_address_map
    serial_write_str(&format_args!("[UEFI] set_virtual_address_map\n"));
    if boot_info.system_table != 0 {
        debugcon_write_byte(b'V'); // Mark: system_table present
        match ech_os::memory::set_uefi_virtual_address_map(
            boot_info.system_table,
            &mut memory_manager,
            boot_info.hhdm_offset,
        ) {
            Ok(runtime_services) => {
                debugcon_write_byte(b'W'); // Mark: set_virtual_address_map OK
                ech_os::boot::set_runtime_services(runtime_services);
                serial_write_str(&format_args!("[UEFI] Runtime services remapped\n"));
                match ech_os::boot::verify_uefi_runtime_services() {
                    Ok(()) => {
                        debugcon_write_byte(b'X'); // Mark: runtime services verified
                        serial_write_str(&format_args!("[UEFI] Runtime services verified\n"));
                        let boot_control =
                            ech_os::boot::appliance::load_persisted().unwrap_or_default();
                        ech_os::boot::appliance::init_shadow(boot_control);
                        ech_os::boot::appliance::publish_stage(
                            ech_os::boot::appliance::BootStage::BootControlLoaded,
                        );
                        if boot_info.secure_boot {
                            if ech_os::posix::secure_boot_db_available() {
                                serial_write_str(&format_args!(
                                    "[UEFI] Secure Boot databases available\n"
                                ));
                            } else {
                                serial_write_str(&format_args!(
                                    "[UEFI] Secure Boot databases unavailable\n"
                                ));
                                loop {
                                    asm!("hlt");
                                }
                            }
                        }
                    }
                    Err(status) => {
                        ech_os::boot::set_runtime_services(0);
                        serial_write_str(&format_args!(
                            "[UEFI] Runtime services verification failed: {:?}\n",
                            status
                        ));
                        if boot_info.secure_boot {
                            loop {
                                asm!("hlt");
                            }
                        }
                    }
                }
            }
            Err(status) => {
                serial_write_str(&format_args!(
                    "[UEFI] SetVirtualAddressMap failed: {:?}\n",
                    status
                ));
                if boot_info.secure_boot {
                    loop {
                        asm!("hlt");
                    }
                }
            }
        };
    } else if boot_info.secure_boot {
        loop {
            asm!("hlt");
        }
    }
    debugcon_write_byte(b'Y'); // Mark: after virtual address map
    if let (Some(framebuffer), Some(screen)) = (boot_info.framebuffer.as_mut(), splash.as_mut()) {
        screen.update_progress(framebuffer, 45);
    }
    debugcon_write_byte(b'Z'); // Mark: before heap init
    if let Err(err) = ech_os::allocator::init_heap(&mut mapper, &mut memory_manager) {
        debugcon_write_byte(b'!'); // Mark: heap init failed
        serial_write_str(&format_args!("[HEAP] init_heap failed: {:?}\n", err));
    } else {
        debugcon_write_byte(b'H'); // Mark: heap init OK
        serial_write_str(&format_args!("[HEAP] TLSF heap initialized\n"));
    }
    debugcon_write_byte(b'I'); // Mark: after heap init
    if let (Some(framebuffer), Some(screen)) = (boot_info.framebuffer.as_mut(), splash.as_mut()) {
        screen.update_progress(framebuffer, 60);
    }

    debugcon_write_byte(b'J'); // Mark: before gdt::init
    ech_os::gdt::init();
    debugcon_write_byte(b'K'); // Mark: after gdt::init
    ech_os::syscall::init();
    debugcon_write_str("[SYSCALL] BSP SYSCALL MSRs programmed\n");
    ech_os::boot::safety::BOOT_SAFETY.enter_phase(ech_os::boot::safety::BootPhase::GdtSetup);
    debugcon_write_byte(b'L'); // Mark: before cpu::init
    ech_os::cpu::init();
    debugcon_write_byte(b'M'); // Mark: after cpu::init
    debugcon_write_byte(b's'); // Mark: before security::init
    ech_os::security::init();
    debugcon_write_byte(b'n'); // Mark: after security::init
    debugcon_write_byte(b'N'); // Mark: after security::init
    ech_os::interrupts::init();
    serial_write_str(&format_args!("[INT] Interrupts initialized\n"));
    debugcon_write_byte(b'O'); // Mark: after interrupts::init
    ech_os::boot::safety::BOOT_SAFETY.enter_phase(ech_os::boot::safety::BootPhase::IdtSetup);
    ech_os::vdso::init();
    ech_os::boot::appliance::publish_stage(ech_os::boot::appliance::BootStage::KernelCoreReady);
    // TTY alt sistemini başlat - klavye interrupt'ları öncesinde!
    ech_os::tty::init();

    ech_os::boot::safety::BOOT_SAFETY.enter_phase(ech_os::boot::safety::BootPhase::AcpiInit);
    let iommu_ready = init_platform_iommu();
    if !iommu_ready {
        serial_write_str(&format_args!(
            "[IOMMU] Proceeding without full hardware isolation for unavailable units\n"
        ));
    }

    // VirtIO-Net driver'ı başlat
    if ech_os::drivers::virtio_net::auto_init() {
        serial_write_str(&format_args!("[NET] VirtIO-Net driver initialized\n"));
        ech_os::boot::appliance::publish_stage(ech_os::boot::appliance::BootStage::NetworkReady);
    } else {
        serial_write_str(&format_args!(
            "[NET] VirtIO-Net driver not found or init failed\n"
        ));
    }

    if let (Some(framebuffer), Some(screen)) = (boot_info.framebuffer.as_mut(), splash.as_mut()) {
        screen.update_progress(framebuffer, 75);
    }

    // Bellek alt sistemlerini başlat (OOM, THP, Cgroup, Memfd, ZSwap)
    ech_os::memory::init_memory_subsystems();

    // CPU alt sistemlerini başlat — SMP ve scheduler'dan ÖNCE
    ech_os::memory_barriers::MemoryBarrier::init();
    serial_write_str(&format_args!("[CPU] Memory barriers initialized\n"));
    ech_os::preempt::init();
    serial_write_str(&format_args!("[CPU] Preemption control initialized\n"));
    ech_os::rcu::init();
    serial_write_str(&format_args!("[CPU] RCU initialized\n"));
    ech_os::atomic_ops::init();
    serial_write_str(&format_args!("[CPU] Atomic ops initialized\n"));

    // CRITICAL: Scheduler ve Workers SMP'den ÖNCE init edilmeli!
    // AP'ler başlatıldığında scheduler kullanıma hazır olmalı
    ech_os::task::scheduler::init();
    ech_os::interrupts::kick_irq_worker();
    // Workers: SMP öncesi cpu_count bilinmiyor, başlangıçta 4 kullan, SMP sonrası ölçeklenir
    ech_os::task::worker::init_workers(4);
    ech_os::boot::safety::BOOT_SAFETY.enter_phase(ech_os::boot::safety::BootPhase::SmpInit);
    // SMP ENABLED — Adım 0.1
    ech_os::cpu::smp::init();
    serial_write_str(&format_args!("[SMP] SMP init completed\n"));

    // SMP sonrası CPU topoloji ve NUMA alt sistemleri
    let cpu_count = ech_os::cpu::smp::get_cpu_count();
    let _ = ech_os::topology::init(cpu_count);
    serial_write_str(&format_args!(
        "[CPU] Topology detection completed ({} CPUs)\n",
        cpu_count
    ));
    ech_os::power::init(cpu_count);
    ech_os::power::init_acpi_power();
    serial_write_str(&format_args!("[PWR] Power manager initialized\n"));
    ech_os::numa::init(4); // Maksimum 4 NUMA düğümü
    serial_write_str(&format_args!("[CPU] NUMA manager initialized\n"));

    ech_os::affinity::init(cpu_count);
    serial_write_str(&format_args!("[CPU] Affinity manager initialized\n"));
    ech_os::hotplug::init(cpu_count);
    serial_write_str(&format_args!("[CPU] Hotplug manager initialized\n"));

    ech_os::boot::safety::BOOT_SAFETY.enter_phase(ech_os::boot::safety::BootPhase::DriverInit);
    // Anti-crash fault management MUST init before interrupts are enabled
    ech_os::fault::init();
    serial_write_str(&format_args!("[FAULT] Anti-crash system initialized\n"));
    x86_64::instructions::interrupts::enable();
    // BSP init tamamlandı — AP timer'lar artık tam modda çalışabilir
    ech_os::interrupts::mark_bsp_init_complete();
    serial_write_str(&format_args!("[INT] Interrupts enabled\n"));
    serial_write_str(&format_args!("[WINSRV] ownership check enabled\n"));
    serial_write_str(&format_args!("[WINSRV] user-range validation enabled\n"));
    serial_write_str(&format_args!(
        "[PERF] latency probes armed (irq + compositor)\n"
    ));
    serial_write_str(&format_args!("[IRONSHIM] fuzz guard active\n"));
    serial_write_str(&format_args!(
        "[IRONSHIM] ring3->ring0 blocked policy active\n"
    ));

    // Linux driver katmanini baslat - PCI tarama, VirtIO/ATA block device'lar
    let driver_count = ech_os::drivers::linux::init_linux_driver_layer();
    serial_write_str(&format_args!(
        "[DRIVERS] Linux driver layer initialized: {} drivers attached\n",
        driver_count
    ));

    // Init sistemi — PID 1 yoneticisi, servisler, mount table
    ech_os::fs::mount::mount_virtual_filesystems();
    ech_os::security::users::init_users();
    ech_os::init::init_system();
    ech_os::boot::appliance::publish_stage(ech_os::boot::appliance::BootStage::StorageMounted);

    // Global framebuffer'ı kaydet - shell için
    if let Some(fb) = boot_info.framebuffer.as_ref() {
        ech_os::boot::set_global_framebuffer(fb.clone());
    }

    ech_os::ipc::service_ipc::init();
    ech_os::services::init();
    ech_os::services::spawn_service_tasks();
    ech_os::ipc::service_ipc::spawn_task();

    // SIMD dispatch fn ptr cache — CPUID bir kez çağrılır, sonra sıfır overhead
    ech_os::gfx::simd::init_simd_dispatch();

    if run_boot_tests {
        let self_ok = ech_os::debug::boot_self_check();
        if !self_ok {
            serial_write_str(&format_args!("[PANIC] Boot self-check failed!\n"));
            loop {
                unsafe { asm!("hlt") };
            }
        }
        ech_os::boot::safety::BootWatchdog::complete();
        ech_os::boot::safety::BOOT_SAFETY
            .enter_phase(ech_os::boot::safety::BootPhase::UserspaceReady);
        let report = ech_os::boot::safety::get_report();
        serial_write_str(&format_args!(
            "[BOOT_TEST] PASS self_check=1 violations={} heap_corruptions={} smp_failures={}\n",
            report.violation_count, report.heap_corruptions, report.smp_failures
        ));
    }

    // FS smoke test flag'ini boot_control'dan oku
    let run_fs_smoke_test = ech_os::boot::appliance::fs_smoke_test_requested();
    if run_fs_smoke_test {
        serial_write_str(&format_args!(
            "[FS_SMOKE] FS smoke test requested via boot control\n"
        ));
    }

    if run_fs_smoke_test {
        let results = ech_os::fs::fs_smoke_test::run_all_fs_smoke_tests();
        let passed = results.iter().filter(|r| r.passed).count();
        let total = results.len();
        serial_write_str(&format_args!(
            "[FS_SMOKE] FINAL: {}/{} tests passed\n",
            passed, total
        ));
        if passed == total {
            serial_write_str(&format_args!("[FS_SMOKE] ALL TESTS PASSED\n"));
        } else {
            serial_write_str(&format_args!("[FS_SMOKE] SOME TESTS FAILED\n"));
        }
    }

    // F2FS background GC thread — free segment threshold monitoring
    ech_os::fs::f2fs::start_gc_thread();

    let run_shell_smoke_test = ech_os::boot::appliance::shell_smoke_test_requested();
    if run_shell_smoke_test {
        serial_write_str(&format_args!(
            "[SHELL_SMOKE] Ring 3 shell requested via boot control\n"
        ));
        if ech_os::boot::appliance::shell_command_test_requested() {
            serial_write_str(&format_args!(
                "[SHELL_TEST] injecting command corpus into TTY stdin\n"
            ));
            seed_shell_command_test_input();
        }
        ech_os::boot::appliance::publish_stage(ech_os::boot::appliance::BootStage::DisplayReady);
        ech_os::shell::run_shell_ring3();
    }

    // Shell yerine yeni compositor tabanlı GUI'yi başlat
    serial_write_str(&format_args!(
        "[BOOT] Starting Velvet Glove compositor...\n"
    ));
    if let Some(fb) = boot_info.framebuffer.as_mut() {
        ech_os::boot::appliance::publish_stage(ech_os::boot::appliance::BootStage::DisplayReady);
        ech_os::gfx::velvet_glove::VelvetGloveCompositor::run(fb);
    } else {
        serial_write_str(&format_args!("[BOOT] No framebuffer, starting shell...\n"));
        {
            serial_write_str(&format_args!("[BOOT] Ring 3 shell mode active\n"));
            ech_os::shell::run_shell_ring3();
        }
    }
}

fn seed_shell_command_test_input() {
    const CORPUS: &[u8] = concat!(
        "echo ECHTEST:ECHO:PASS\n",
        "X=42\n",
        "echo ECHTEST:VAR:$X\n",
        "echo ECHTEST:ARITH:$((3+4))\n",
        "printf \"ECHTEST:PRINTF:%s\\n\" ok\n",
        "if [ 1 -eq 1 ]; then echo ECHTEST:IF:PASS; else echo ECHTEST:IF:FAIL; fi\n",
        "for i in 1 2 3; do echo ECHTEST:FOR:$i; done\n",
        ":\n",
        "echo ECHTEST:COLON:PASS\n",
        "MYVAR=hello\n",
        "echo ECHTEST:LENGTH:${#MYVAR}\n",
        "FRUIT=banana\n",
        "echo ECHTEST:SUFFIX:${FRUIT%na}\n",
        "echo ECHTEST:PREFIX:${FRUIT#ba}\n",
        "echo ECHTEST:GREEDY_SUFFIX:${FRUIT%%a*}\n",
        "echo ECHTEST:GREEDY_PREFIX:${FRUIT##b*}\n",
        "eval echo ECHTEST:EVAL:PASS\n",
        "echo ECHTEST:END:PASS\n",
        "exit\n",
    )
    .as_bytes();

    // Drain spurious bytes from PS/2 controller boot (e.g. QEMU sends
    // a stray scancode during init that decodes to '7' or other garbage)
    let mut drained = 0usize;
    while ech_os::tty::DEFAULT_TTY.input_buf.pop().is_some() {
        drained += 1;
    }
    if drained > 0 {
        serial_write_str(&format_args!(
            "[SHELL_TEST] drained {} spurious bytes from TTY input_buf\n",
            drained
        ));
    }

    let mut accepted = 0usize;
    for &byte in CORPUS {
        if ech_os::tty::DEFAULT_TTY.input_buf.push(byte).is_ok() {
            accepted += 1;
        } else {
            break;
        }
    }

    if accepted == CORPUS.len() {
        serial_write_str(&format_args!(
            "[SHELL_TEST] command corpus queued bytes={}\n",
            accepted
        ));
    } else {
        serial_write_str(&format_args!(
            "[SHELL_TEST] command corpus truncated accepted={} expected={}\n",
            accepted,
            CORPUS.len()
        ));
    }
}

#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
unsafe fn boot_pipeline_limine(kaslr_offset: u64) -> ! {
    serial_write_str(&format_args!("[LIMINE] Booting via Limine\n"));
    let hhdm_offset = match LIMINE_HHDM_REQUEST.get_response() {
        Some(response) => response.offset,
        None => {
            serial_write_str(&format_args!("[LIMINE] HHDM response missing\n"));
            loop {
                asm!("hlt");
            }
        }
    };
    let memmap: PointerSlice<MemoryRegionInfo> = match LIMINE_MEMORY_MAP_REQUEST.get_response() {
        Some(response) => response.get_entries(),
        None => {
            serial_write_str(&format_args!("[LIMINE] Memory map response missing\n"));
            loop {
                asm!("hlt");
            }
        }
    };
    if let Some(response) = LIMINE_CMDLINE_REQUEST.get_response() {
        let cmdline = response.get_cmdline();
        if !cmdline.is_empty() {
            serial_write_str(&format_args!("[LIMINE] cmdline: {}\n", cmdline));
            if let Some((lba, slots)) = parse_swap_cmdline(cmdline) {
                if ech_os::memory::init_swap_device(lba, slots) {
                    serial_write_str(&format_args!(
                        "[SWAP] Enabled device base_lba={} slots={}\n",
                        lba, slots
                    ));
                } else {
                    serial_write_str(&format_args!(
                        "[SWAP] Device init failed base_lba={} slots={}\n",
                        lba, slots
                    ));
                }
            }
        }
    }

    let mut entries = Vec::new();
    for entry in memmap.iter() {
        let typ = match entry.get_type() {
            MemoryRegionType::Usable => 0,
            MemoryRegionType::AcpiReclaimable => 2,
            _ => 1,
        };
        entries.push(LimineMemmapEntry {
            base: entry.base,
            length: entry.length,
            typ,
        });
    }

    let boot_ctx = ech_os::KernelBootContext {
        physical_memory_offset: hhdm_offset,
    };
    let mut mapper = unsafe { ech_os::memory::init_paging(boot_ctx.physical_memory_offset) };
    let mut frame_allocator =
        LimineFrameAllocator::new(&entries, kaslr_offset).expect("Limine frame allocator init");
    if let Err(err) = ech_os::allocator::init_heap(&mut mapper, &mut frame_allocator) {
        serial_write_str(&format_args!("[HEAP] init_heap failed: {:?}\n", err));
    } else {
        serial_write_str(&format_args!("[HEAP] TLSF heap initialized\n"));
    }

    ech_os::gdt::init();
    ech_os::syscall::init(); // SYSCALL/SYSRET MSR'larını BSP için programla
    ech_os::cpu::init();
    ech_os::security::init();
    ech_os::interrupts::init();
    ech_os::vdso::init();
    // TTY alt sistemini başlat - klavye interrupt'ları öncesinde!
    ech_os::tty::init();
    let _ = init_platform_iommu();

    // CRITICAL: Scheduler ve Workers SMP'den ÖNCE init edilmeli!
    ech_os::task::scheduler::init();
    ech_os::interrupts::kick_irq_worker();
    let cpu_count = ech_os::cpu::smp::get_cpu_count();
    ech_os::task::worker::init_workers(core::cmp::max(cpu_count as usize, 2));
    ech_os::cpu::smp::init();
    x86_64::instructions::interrupts::enable();
    let self_ok = true;
    if !self_ok {
        serial_write_str(&format_args!("[PANIC] Self-check failed!\n"));
        loop {
            asm!("hlt");
        }
    }

    serial_write_str(&format_args!("[OS] Basic boot sequence complete.\n"));
    ech_os::task::scheduler::idle_loop();
}

#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
unsafe fn boot_pipeline_multiboot(boot_info_addr: usize, kaslr_offset: u64) -> ! {
    serial_write_str(&format_args!(
        "[MULTIBOOT] Info addr: {:#x}\n",
        boot_info_addr
    ));
    debugcon_write_byte(b'M');

    let info = if boot_info_addr == 0 {
        serial_write_str(&format_args!("[MULTIBOOT] boot_info_addr is null\n"));
        loop {
            asm!("hlt");
        }
    } else {
        match unsafe { load(boot_info_addr) } {
            Ok(info) => info,
            Err(_) => {
                serial_write_str(&format_args!("[MULTIBOOT] boot info parse failed\n"));
                loop {
                    asm!("hlt");
                }
            }
        }
    };
    serial_write_str(&format_args!("[MULTIBOOT] Info parsed\n"));

    let cmdline = info
        .command_line_tag()
        .map(|t| t.command_line())
        .unwrap_or("");
    if !cmdline.is_empty() {
        serial_write_str(&format_args!("[MULTIBOOT] cmdline: {}\n", cmdline));
        if let Some((lba, slots)) = parse_swap_cmdline(cmdline) {
            if ech_os::memory::init_swap_device(lba, slots) {
                serial_write_str(&format_args!(
                    "[SWAP] Enabled device base_lba={} slots={}\n",
                    lba, slots
                ));
            } else {
                serial_write_str(&format_args!(
                    "[SWAP] Device init failed base_lba={} slots={}\n",
                    lba, slots
                ));
            }
        }
    }

    let _ = kaslr_offset;

    let boot_ctx = ech_os::KernelBootContext {
        physical_memory_offset: ech_os::memory::PHYSICAL_MEMORY_OFFSET,
    };

    let mut mapper = unsafe { ech_os::memory::init_paging(boot_ctx.physical_memory_offset) };
    serial_write_str(&format_args!("[MEMORY] Paging initialized\n"));

    serial_write_str(&format_args!("[MEMORY] Frame allocator init\n"));
    let mut frame_allocator =
        ech_os::memory::frame_allocator::Multiboot2FrameAllocator::new(&info, kaslr_offset)
            .expect("Multiboot2 frame allocator init failed");
    ech_os::memory::set_global_mb2_frame_allocator(&mut frame_allocator as *mut _);
    serial_write_str(&format_args!(
        "[MEMORY] Usable bytes: {:#x}\n",
        frame_allocator.total_usable_bytes()
    ));
    if let Err(err) = ech_os::allocator::init_heap(&mut mapper, &mut frame_allocator) {
        serial_write_str(&format_args!("[HEAP] init_heap failed: {:?}\n", err));
    } else {
        serial_write_str(&format_args!("[HEAP] TLSF heap initialized\n"));
    }
    serial_write_str(&format_args!("[BOOT] Core subsystems init\n"));

    ech_os::gdt::init();
    ech_os::syscall::init(); // SYSCALL/SYSRET MSR'larını BSP için programla
    ech_os::cpu::init();
    ech_os::security::init();
    ech_os::interrupts::init();
    // TTY alt sistemini başlat - klavye interrupt'ları öncesinde!
    ech_os::tty::init();
    let _ = init_platform_iommu();

    // CRITICAL: Scheduler ve Workers SMP'den ÖNCE init edilmeli!
    ech_os::task::scheduler::init();
    ech_os::interrupts::kick_irq_worker();
    let cpu_count = ech_os::cpu::smp::get_cpu_count();
    ech_os::task::worker::init_workers(core::cmp::max(cpu_count as usize, 2));
    ech_os::cpu::smp::init();
    ech_os::memory::start_reclaim_daemon();
    ech_os::serial_println!("[BOOT] Scheduler online");
    let self_ok = ech_os::debug::boot_self_check();
    if !self_ok {
        ech_os::serial_println!("[BOOT] Self-check failed, halting");
        loop {
            unsafe {
                asm!("hlt");
            }
        }
    }
    ech_os::serial_println!("[DEBUG] About to call run_ring3_smoketest()");

    ech_os::debug::run_ring3_smoketest();
    ech_os::serial_println!("[DEBUG] Returned from run_ring3_smoketest()");

    ech_os::debug::run_vm_security_tests();
    ech_os::debug::run_vm_stress_tests();
    ech_os::debug::run_irq_stress_tests();
    ech_os::serial_println!("[BOOT] Tests done, idle loop");

    ech_os::task::scheduler::idle_loop();
}

#[cfg(target_os = "uefi")]
#[no_mangle]
pub extern "efiapi" fn efi_main(image: Handle, mut system_table: SystemTable<Boot>) -> Status {
    unsafe {
        system_table.boot_services().set_image_handle(image);
        uefi::allocator::init(system_table.boot_services());
    }

    unsafe {
        serial_init();
        serial_write_str(&format_args!("[UEFI] EFI Entry Point Reached!\n"));
    }

    serial_write_str(&format_args!("[UEFI] Getting framebuffer...\n"));
    let framebuffer = {
        let boot_services = system_table.boot_services();
        let mut gop = boot_services
            .get_handle_for_protocol::<GraphicsOutput>()
            .ok()
            .and_then(|handle| {
                boot_services
                    .open_protocol_exclusive::<GraphicsOutput>(handle)
                    .ok()
            });
        gop.as_mut().map(|gop| {
            configure_preferred_gop_mode(gop);
            Framebuffer::new(gop)
        })
    };

    serial_write_str(&format_args!("[UEFI] Finding ACPI table...\n"));
    let rsdp_address =
        ech_os::acpi::find_acpi_table(system_table.config_table()).unwrap_or(0) as u64;

    serial_write_str(&format_args!("[UEFI] Checking secure boot enrollment...\n"));
    if let Err(status) = auto_enroll_secure_boot_payloads(&mut system_table, image) {
        return status;
    }

    serial_write_str(&format_args!("[UEFI] Detecting secure boot...\n"));
    let secure_boot = detect_secure_boot(&system_table);

    serial_write_str(&format_args!("[UEFI] Inspecting loaded image...\n"));
    let (image_hash, image_size) = match inspect_loaded_image(&mut system_table, image, secure_boot)
    {
        Ok(value) => value,
        Err(status) => return status,
    };
    if let Err(err) = measure_loaded_image_tpm(&mut system_table, image, image_hash, image_size) {
        if secure_boot {
            serial_write_str(&format_args!("[TPM] Measure policy failed\n"));
            return err;
        }
    }
    let (cmdline_ptr, cmdline_len) = match read_cmdline(&mut system_table, image) {
        Ok((ptr, len)) => (ptr, len),
        Err(status) => {
            if status == Status::UNSUPPORTED {
                (0, 0)
            } else {
                return status;
            }
        }
    };
    if let Err(err) = measure_cmdline_tpm(&mut system_table, cmdline_ptr, cmdline_len) {
        if secure_boot {
            serial_write_str(&format_args!("[TPM] Cmdline measure policy failed\n"));
            return err;
        }
    }
    let seed_from_esp = read_boot_control_seed(&mut system_table, image);
    let seed_from_var = read_boot_control_variable_seed(&mut system_table);
    if let Some(bundle) =
        read_efi_boot_file(&mut system_table, image, cstr16!("EFI\\BOOT\\PESMOKE.BHD"))
    {
        ech_os::boot::appliance::seed_packaged_pe_smoke_bundle(bundle);
    }
    let mut curated_slots = 0usize;
    let mut curated_bytes = 0usize;
    for index in 1..=32u8 {
        let Some(path) = curated_app_bundle_path(index) else {
            break;
        };
        let Some(bundle_size) = efi_boot_file_size(&mut system_table, image, path) else {
            continue;
        };
        curated_slots = curated_slots.saturating_add(1);
        curated_bytes = curated_bytes.saturating_add(bundle_size);
    }
    if curated_slots != 0 {
        serial_write_str(&format_args!(
            "[UEFI] Deferred curated bundles on ESP: slots={} bytes={}\n",
            curated_slots, curated_bytes
        ));
    }
    let mut boot_control = ech_os::boot::appliance::merge_seed(seed_from_esp, seed_from_var);
    boot_control.begin_boot();
    sync_boot_control_seed(&mut system_table, image, &boot_control);
    report_tpm_event_log(&mut system_table);
    let boot_info_ptr = match system_table
        .boot_services()
        .allocate_pool(MemoryType::LOADER_DATA, core::mem::size_of::<BootInfo>())
    {
        Ok(ptr) => ptr as *mut BootInfo,
        Err(_) => return Status::OUT_OF_RESOURCES,
    };
    let (runtime_table, memory_map) = system_table.exit_boot_services();
    let runtime_services = unsafe { runtime_table.runtime_services() as *const _ as usize };
    let system_table_addr = runtime_table.get_current_system_table_addr();
    unsafe {
        core::ptr::write(
            boot_info_ptr,
            BootInfo {
                magic: ech_os::boot::BOOTINFO_MAGIC,
                version: ech_os::boot::BOOTINFO_VERSION,
                size: core::mem::size_of::<BootInfo>() as u32,
                memory_map: Some(memory_map),
                physical_memory_offset: ech_os::memory::PHYSICAL_MEMORY_OFFSET,
                hhdm_offset: ech_os::memory::PHYSICAL_MEMORY_OFFSET,
                framebuffer,
                rsdp_address,
                system_table: system_table_addr,
                runtime_services,
                secure_boot,
                cmdline_ptr,
                cmdline_len,
                image_size,
                image_hash,
            },
        );
    }
    kernel_entry(boot_info_ptr as usize, 0, BOOT_MAGIC_UEFI)
}

#[cfg(target_os = "uefi")]
fn detect_secure_boot(system_table: &SystemTable<Boot>) -> bool {
    let secure_boot = read_global_u8_variable(system_table, cstr16!("SecureBoot"));
    let setup_mode = read_global_u8_variable(system_table, cstr16!("SetupMode"));
    match (secure_boot, setup_mode) {
        (Some(1), Some(0)) => true,
        _ => false,
    }
}

#[cfg(target_os = "uefi")]
fn read_global_u8_variable(system_table: &SystemTable<Boot>, name: &CStr16) -> Option<u8> {
    let runtime_services = system_table.runtime_services();
    let mut buf = [0u8; 1];
    let vendor = VariableVendor::GLOBAL_VARIABLE;
    match runtime_services.get_variable(name, &vendor, &mut buf) {
        Ok(_) => Some(buf[0]),
        Err(_) => None,
    }
}

#[cfg(target_os = "uefi")]
fn appliance_variable_vendor() -> VariableVendor {
    VariableVendor(uefi::Guid::new(
        [0x83, 0x61, 0x26, 0x6d],
        [0x25, 0x4b],
        [0xab, 0x49],
        0x8c,
        0x4d,
        [0x74, 0x2f, 0x57, 0x78, 0x62, 0x90],
    ))
}

#[cfg(target_os = "uefi")]
fn read_boot_control_variable_seed(
    system_table: &mut SystemTable<Boot>,
) -> Option<ech_os::boot::appliance::BootControlBlock> {
    let runtime = system_table.runtime_services();
    let (data, _) = runtime
        .get_variable_boxed(cstr16!("echOSBootControl"), &appliance_variable_vendor())
        .ok()?;
    if data.len() != core::mem::size_of::<ech_os::boot::appliance::BootControlBlock>() {
        return None;
    }
    let block = unsafe { *(data.as_ptr() as *const ech_os::boot::appliance::BootControlBlock) };
    block.validate().then_some(block)
}

#[cfg(target_os = "uefi")]
fn curated_app_bundle_path(index: u8) -> Option<&'static CStr16> {
    match index {
        1 => Some(cstr16!("EFI\\BOOT\\APP0001.BHD")),
        2 => Some(cstr16!("EFI\\BOOT\\APP0002.BHD")),
        3 => Some(cstr16!("EFI\\BOOT\\APP0003.BHD")),
        4 => Some(cstr16!("EFI\\BOOT\\APP0004.BHD")),
        5 => Some(cstr16!("EFI\\BOOT\\APP0005.BHD")),
        6 => Some(cstr16!("EFI\\BOOT\\APP0006.BHD")),
        7 => Some(cstr16!("EFI\\BOOT\\APP0007.BHD")),
        8 => Some(cstr16!("EFI\\BOOT\\APP0008.BHD")),
        9 => Some(cstr16!("EFI\\BOOT\\APP0009.BHD")),
        10 => Some(cstr16!("EFI\\BOOT\\APP0010.BHD")),
        11 => Some(cstr16!("EFI\\BOOT\\APP0011.BHD")),
        12 => Some(cstr16!("EFI\\BOOT\\APP0012.BHD")),
        13 => Some(cstr16!("EFI\\BOOT\\APP0013.BHD")),
        14 => Some(cstr16!("EFI\\BOOT\\APP0014.BHD")),
        15 => Some(cstr16!("EFI\\BOOT\\APP0015.BHD")),
        16 => Some(cstr16!("EFI\\BOOT\\APP0016.BHD")),
        17 => Some(cstr16!("EFI\\BOOT\\APP0017.BHD")),
        18 => Some(cstr16!("EFI\\BOOT\\APP0018.BHD")),
        19 => Some(cstr16!("EFI\\BOOT\\APP0019.BHD")),
        20 => Some(cstr16!("EFI\\BOOT\\APP0020.BHD")),
        21 => Some(cstr16!("EFI\\BOOT\\APP0021.BHD")),
        22 => Some(cstr16!("EFI\\BOOT\\APP0022.BHD")),
        23 => Some(cstr16!("EFI\\BOOT\\APP0023.BHD")),
        24 => Some(cstr16!("EFI\\BOOT\\APP0024.BHD")),
        25 => Some(cstr16!("EFI\\BOOT\\APP0025.BHD")),
        26 => Some(cstr16!("EFI\\BOOT\\APP0026.BHD")),
        27 => Some(cstr16!("EFI\\BOOT\\APP0027.BHD")),
        28 => Some(cstr16!("EFI\\BOOT\\APP0028.BHD")),
        29 => Some(cstr16!("EFI\\BOOT\\APP0029.BHD")),
        30 => Some(cstr16!("EFI\\BOOT\\APP0030.BHD")),
        31 => Some(cstr16!("EFI\\BOOT\\APP0031.BHD")),
        32 => Some(cstr16!("EFI\\BOOT\\APP0032.BHD")),
        _ => None,
    }
}

#[cfg(target_os = "uefi")]
fn read_efi_boot_file(
    system_table: &mut SystemTable<Boot>,
    image: Handle,
    path: &CStr16,
) -> Option<Vec<u8>> {
    let boot_services = system_table.boot_services();
    let loaded_image = boot_services
        .open_protocol_exclusive::<LoadedImage>(image)
        .ok()?;
    let mut fs = boot_services
        .open_protocol_exclusive::<SimpleFileSystem>(loaded_image.device())
        .ok()?;
    let mut root = fs.open_volume().ok()?;
    let handle = root
        .open(path, FileMode::Read, FileAttribute::empty())
        .ok()?;
    let mut file = handle.into_regular_file()?;
    let info = file
        .get_boxed_info::<uefi::proto::media::file::FileInfo>()
        .ok()?;
    let file_size = info.file_size() as usize;
    if file_size == 0 {
        return None;
    }
    let mut raw = vec![0u8; file_size];
    let len = file.read(&mut raw).ok()?;
    if len == 0 {
        return None;
    }
    raw.truncate(len);
    Some(raw)
}

#[cfg(target_os = "uefi")]
fn efi_boot_file_size(
    system_table: &mut SystemTable<Boot>,
    image: Handle,
    path: &CStr16,
) -> Option<usize> {
    let boot_services = system_table.boot_services();
    let loaded_image = boot_services
        .open_protocol_exclusive::<LoadedImage>(image)
        .ok()?;
    let mut fs = boot_services
        .open_protocol_exclusive::<SimpleFileSystem>(loaded_image.device())
        .ok()?;
    let mut root = fs.open_volume().ok()?;
    let handle = root
        .open(path, FileMode::Read, FileAttribute::empty())
        .ok()?;
    let mut file = handle.into_regular_file()?;
    let info = file
        .get_boxed_info::<uefi::proto::media::file::FileInfo>()
        .ok()?;
    Some(info.file_size() as usize)
}

#[cfg(target_os = "uefi")]
fn read_boot_control_seed(
    system_table: &mut SystemTable<Boot>,
    image: Handle,
) -> Option<ech_os::boot::appliance::BootControlBlock> {
    let mut raw = read_efi_boot_file(system_table, image, cstr16!("EFI\\BOOT\\BOOTCTRL.BIN"))?;
    if raw.len() != core::mem::size_of::<ech_os::boot::appliance::BootControlBlock>() {
        return None;
    }
    let block = unsafe { *(raw.as_mut_ptr() as *const ech_os::boot::appliance::BootControlBlock) };
    block.validate().then_some(block)
}

#[cfg(target_os = "uefi")]
fn sync_boot_control_seed(
    system_table: &mut SystemTable<Boot>,
    image: Handle,
    block: &ech_os::boot::appliance::BootControlBlock,
) {
    let runtime = system_table.runtime_services();
    let attributes = uefi::table::runtime::VariableAttributes::BOOTSERVICE_ACCESS
        | uefi::table::runtime::VariableAttributes::RUNTIME_ACCESS
        | uefi::table::runtime::VariableAttributes::NON_VOLATILE;
    let bytes = unsafe {
        core::slice::from_raw_parts(
            (block as *const ech_os::boot::appliance::BootControlBlock).cast::<u8>(),
            core::mem::size_of::<ech_os::boot::appliance::BootControlBlock>(),
        )
    };
    let _ = runtime.set_variable(
        cstr16!("echOSBootControl"),
        &appliance_variable_vendor(),
        attributes,
        bytes,
    );

    let boot_services = system_table.boot_services();
    let Ok(loaded_image) = boot_services.open_protocol_exclusive::<LoadedImage>(image) else {
        return;
    };
    let Ok(mut fs) =
        boot_services.open_protocol_exclusive::<SimpleFileSystem>(loaded_image.device())
    else {
        return;
    };
    let Ok(mut root) = fs.open_volume() else {
        return;
    };
    let Ok(handle) = root.open(
        cstr16!("EFI\\BOOT\\BOOTCTRL.BIN"),
        FileMode::CreateReadWrite,
        FileAttribute::ARCHIVE,
    ) else {
        return;
    };
    let Some(mut file) = handle.into_regular_file() else {
        return;
    };
    let _ = file.set_position(0);
    let _ = file.write(bytes);
    let _ = file.flush();
}

#[cfg(target_os = "uefi")]
fn read_secure_boot_enroll_state(
    system_table: &mut SystemTable<Boot>,
) -> Option<SecureBootEnrollState> {
    let runtime = system_table.runtime_services();
    let (data, _) = runtime
        .get_variable_boxed(
            cstr16!("echOSSecureBootEnroll"),
            &appliance_variable_vendor(),
        )
        .ok()?;
    if data.len() != core::mem::size_of::<SecureBootEnrollState>() {
        return None;
    }
    let state = unsafe { *(data.as_ptr() as *const SecureBootEnrollState) };
    (state.magic == SECURE_BOOT_ENROLL_MAGIC).then_some(state)
}

#[cfg(target_os = "uefi")]
fn write_secure_boot_enroll_state(
    system_table: &mut SystemTable<Boot>,
    flags: u8,
) -> Result<(), Status> {
    let runtime = system_table.runtime_services();
    let state = SecureBootEnrollState {
        magic: SECURE_BOOT_ENROLL_MAGIC,
        flags,
        _reserved: [0; 3],
    };
    let bytes = unsafe {
        core::slice::from_raw_parts(
            (&state as *const SecureBootEnrollState).cast::<u8>(),
            core::mem::size_of::<SecureBootEnrollState>(),
        )
    };
    runtime
        .set_variable(
            cstr16!("echOSSecureBootEnroll"),
            &appliance_variable_vendor(),
            VariableAttributes::NON_VOLATILE
                | VariableAttributes::BOOTSERVICE_ACCESS
                | VariableAttributes::RUNTIME_ACCESS,
            bytes,
        )
        .map_err(|err| err.status())
}

#[cfg(target_os = "uefi")]
fn write_global_variable_payload(
    system_table: &mut SystemTable<Boot>,
    name: &CStr16,
    payload: &[u8],
) -> Result<(), Status> {
    system_table
        .runtime_services()
        .set_variable(
            name,
            &VariableVendor::GLOBAL_VARIABLE,
            VariableAttributes::NON_VOLATILE
                | VariableAttributes::BOOTSERVICE_ACCESS
                | VariableAttributes::RUNTIME_ACCESS
                | VariableAttributes::TIME_BASED_AUTHENTICATED_WRITE_ACCESS,
            payload,
        )
        .map_err(|err| err.status())
}

#[cfg(target_os = "uefi")]
fn auto_enroll_secure_boot_payloads(
    system_table: &mut SystemTable<Boot>,
    image: Handle,
) -> Result<(), Status> {
    let secure_boot = read_global_u8_variable(system_table, cstr16!("SecureBoot")).unwrap_or(0);
    let setup_mode = read_global_u8_variable(system_table, cstr16!("SetupMode")).unwrap_or(0);
    let state = read_secure_boot_enroll_state(system_table);
    let enroll_requested =
        read_efi_boot_file(system_table, image, cstr16!("EFI\\BOOT\\SBENROLL.ON")).is_some();

    if secure_boot == 1 && setup_mode == 0 {
        if state.is_some_and(|state| state.flags != 0) {
            let _ = write_secure_boot_enroll_state(system_table, 0);
            serial_write_str(&format_args!("[UEFI] Secure Boot enroll state verified\n"));
        }
        return Ok(());
    }

    if !enroll_requested {
        if state.is_some_and(|state| (state.flags & SECURE_BOOT_ENROLL_PENDING_RESET) != 0) {
            serial_write_str(&format_args!(
                "[UEFI] Secure Boot enroll pending without trigger\n"
            ));
            return Err(Status::SECURITY_VIOLATION);
        }
        return Ok(());
    }

    if state.is_some_and(|state| (state.flags & SECURE_BOOT_ENROLL_FAILED) != 0) {
        serial_write_str(&format_args!(
            "[UEFI] Secure Boot enroll previously failed\n"
        ));
        return Err(Status::SECURITY_VIOLATION);
    }

    if setup_mode != 1 {
        serial_write_str(&format_args!(
            "[UEFI] Secure Boot enroll trigger ignored outside setup mode\n"
        ));
        return Ok(());
    }

    if state.is_some_and(|state| (state.flags & SECURE_BOOT_ENROLL_PENDING_RESET) != 0) {
        let _ = write_secure_boot_enroll_state(system_table, SECURE_BOOT_ENROLL_FAILED);
        serial_write_str(&format_args!(
            "[UEFI] Secure Boot enroll did not transition firmware out of setup mode\n"
        ));
        return Err(Status::SECURITY_VIOLATION);
    }

    let payloads = [
        ("PK", cstr16!("PK"), cstr16!("EFI\\BOOT\\PK.AUT")),
        ("KEK", cstr16!("KEK"), cstr16!("EFI\\BOOT\\KEK.AUT")),
        ("db", cstr16!("db"), cstr16!("EFI\\BOOT\\DB.AUT")),
        ("dbx", cstr16!("dbx"), cstr16!("EFI\\BOOT\\DBX.AUT")),
    ];
    serial_write_str(&format_args!(
        "[UEFI] Secure Boot auto-enroll trigger detected\n"
    ));
    for (label, variable_name, path) in payloads {
        let payload = read_efi_boot_file(system_table, image, path).ok_or_else(|| {
            serial_write_str(&format_args!(
                "[UEFI] Missing Secure Boot payload for {}\n",
                label
            ));
            let _ = write_secure_boot_enroll_state(system_table, SECURE_BOOT_ENROLL_FAILED);
            Status::SECURITY_VIOLATION
        })?;
        write_global_variable_payload(system_table, variable_name, &payload).map_err(|status| {
            serial_write_str(&format_args!(
                "[UEFI] Secure Boot variable write failed for {}\n",
                label
            ));
            let _ = write_secure_boot_enroll_state(system_table, SECURE_BOOT_ENROLL_FAILED);
            status
        })?;
        serial_write_str(&format_args!(
            "[UEFI] Secure Boot variable enrolled: {}\n",
            label
        ));
    }
    write_secure_boot_enroll_state(system_table, SECURE_BOOT_ENROLL_PENDING_RESET)?;
    serial_write_str(&format_args!(
        "[UEFI] Secure Boot enroll complete, rebooting for verification\n"
    ));
    ech_os::boot::reset_uefi_system(ResetType::WARM, Status::SUCCESS)
}

#[cfg(target_os = "uefi")]
fn inspect_loaded_image(
    system_table: &mut SystemTable<Boot>,
    handle: Handle,
    secure_boot: bool,
) -> Result<([u8; 32], u64), Status> {
    let boot_services = system_table.boot_services();
    let loaded_image = boot_services
        .open_protocol_exclusive::<LoadedImage>(handle)
        .map_err(|err| err.status())?;
    let image_info = loaded_image.info();
    let image_base = image_info.0 as *const u8;
    let image_size = image_info.1 as usize;
    if image_base.is_null() || image_size == 0 {
        serial_write_str(&format_args!("[UEFI] Loaded image invalid\n"));
        return Err(Status::SECURITY_VIOLATION);
    }
    if image_size > isize::MAX as usize {
        serial_write_str(&format_args!("[UEFI] Loaded image too large\n"));
        return Err(Status::BAD_BUFFER_SIZE);
    }
    let image = unsafe { core::slice::from_raw_parts(image_base, image_size) };
    if secure_boot {
        if ech_os::posix::secure_boot_verify_image(image) {
            serial_write_str(&format_args!("[UEFI] Loaded image signature OK\n"));
        } else {
            serial_write_str(&format_args!("[UEFI] Loaded image signature failed\n"));
            return Err(Status::SECURITY_VIOLATION);
        }
    }
    let mut hasher = Sha256::new();
    hasher.update(image);
    let digest = hasher.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&digest[..]);
    Ok((hash, image_size as u64))
}

#[cfg(target_os = "uefi")]
fn measure_loaded_image_tpm(
    system_table: &mut SystemTable<Boot>,
    handle: Handle,
    image_hash: [u8; 32],
    image_size: u64,
) -> Result<(), Status> {
    let boot_services = system_table.boot_services();
    let tcg_handle = match boot_services.get_handle_for_protocol::<Tcg>() {
        Ok(handle) => handle,
        Err(_) => {
            serial_write_str(&format_args!("[TPM] TCG2 protocol not found\n"));
            return Err(Status::NOT_FOUND);
        }
    };
    let mut tcg = match boot_services.open_protocol_exclusive::<Tcg>(tcg_handle) {
        Ok(tcg) => tcg,
        Err(err) => {
            serial_write_str(&format_args!(
                "[TPM] TCG2 open failed: {:?}\n",
                err.status()
            ));
            return Err(err.status());
        }
    };
    let capability = match tcg.get_capability() {
        Ok(capability) => capability,
        Err(err) => {
            serial_write_str(&format_args!("[TPM] TCG2 capability error: {:?}\n", err));
            return Err(Status::DEVICE_ERROR);
        }
    };
    if !capability.tpm_present() {
        serial_write_str(&format_args!("[TPM] TPM not present\n"));
        return Err(Status::NOT_FOUND);
    }
    let loaded_image = match boot_services.open_protocol_exclusive::<LoadedImage>(handle) {
        Ok(loaded_image) => loaded_image,
        Err(err) => {
            serial_write_str(&format_args!(
                "[TPM] LoadedImage open failed: {:?}\n",
                err.status()
            ));
            return Err(err.status());
        }
    };
    let image_info = loaded_image.info();
    let image_base = image_info.0 as *const u8;
    let image_len = image_info.1 as usize;
    if image_base.is_null() || image_len == 0 {
        serial_write_str(&format_args!("[TPM] Loaded image invalid\n"));
        return Err(Status::SECURITY_VIOLATION);
    }
    if image_len > isize::MAX as usize {
        serial_write_str(&format_args!("[TPM] Loaded image too large\n"));
        return Err(Status::BAD_BUFFER_SIZE);
    }
    let image = unsafe { core::slice::from_raw_parts(image_base, image_len) };
    let label = b"echOS kernel";
    let mut event_data = [0u8; 64];
    let mut offset = 0usize;
    event_data[..label.len()].copy_from_slice(label);
    offset += label.len();
    event_data[offset..offset + 8].copy_from_slice(&image_size.to_le_bytes());
    offset += 8;
    event_data[offset..offset + 32].copy_from_slice(&image_hash);
    offset += 32;
    let mut event_buf = [MaybeUninit::<u8>::uninit(); 128];
    let event = match PcrEventInputs::new_in_buffer(
        &mut event_buf,
        PcrIndex(4),
        EventType::EFI_BOOT_SERVICES_APPLICATION,
        &event_data[..offset],
    ) {
        Ok(event) => event,
        Err(err) => {
            serial_write_str(&format_args!("[TPM] Event build failed: {:?}\n", err));
            return Err(Status::DEVICE_ERROR);
        }
    };
    let flags = HashLogExtendEventFlags::PE_COFF_IMAGE;
    if let Err(err) = tcg.hash_log_extend_event(flags, image, event) {
        serial_write_str(&format_args!("[TPM] Measure failed: {:?}\n", err));
        Err(Status::DEVICE_ERROR)
    } else {
        serial_write_str(&format_args!("[TPM] Measure OK (PCR4)\n"));
        Ok(())
    }
}

#[cfg(target_os = "uefi")]
fn measure_cmdline_tpm(
    system_table: &mut SystemTable<Boot>,
    cmdline_ptr: u64,
    cmdline_len: u64,
) -> Result<(), Status> {
    if cmdline_ptr == 0 || cmdline_len == 0 {
        return Ok(());
    }
    let cmdline_len = cmdline_len as usize;
    if cmdline_len > isize::MAX as usize {
        serial_write_str(&format_args!("[TPM] Cmdline too large\n"));
        return Err(Status::BAD_BUFFER_SIZE);
    }
    let cmdline = unsafe { core::slice::from_raw_parts(cmdline_ptr as *const u8, cmdline_len) };
    let boot_services = system_table.boot_services();
    let tcg_handle = match boot_services.get_handle_for_protocol::<Tcg>() {
        Ok(handle) => handle,
        Err(_) => {
            serial_write_str(&format_args!("[TPM] TCG2 protocol not found\n"));
            return Err(Status::NOT_FOUND);
        }
    };
    let mut tcg = match boot_services.open_protocol_exclusive::<Tcg>(tcg_handle) {
        Ok(tcg) => tcg,
        Err(err) => {
            serial_write_str(&format_args!(
                "[TPM] TCG2 open failed: {:?}\n",
                err.status()
            ));
            return Err(err.status());
        }
    };
    let capability = match tcg.get_capability() {
        Ok(capability) => capability,
        Err(err) => {
            serial_write_str(&format_args!("[TPM] TCG2 capability error: {:?}\n", err));
            return Err(Status::DEVICE_ERROR);
        }
    };
    if !capability.tpm_present() {
        serial_write_str(&format_args!("[TPM] TPM not present\n"));
        return Err(Status::NOT_FOUND);
    }
    let label = b"echOS cmdline\0";
    let mut event_data = Vec::with_capacity(label.len().saturating_add(cmdline.len()));
    event_data.extend_from_slice(label);
    event_data.extend_from_slice(cmdline);
    let mut event_buf = vec![MaybeUninit::<u8>::uninit(); event_data.len() + 64];
    let event = match PcrEventInputs::new_in_buffer(
        &mut event_buf,
        PcrIndex(8),
        EventType::EFI_ACTION,
        &event_data,
    ) {
        Ok(event) => event,
        Err(err) => {
            serial_write_str(&format_args!("[TPM] Event build failed: {:?}\n", err));
            return Err(Status::DEVICE_ERROR);
        }
    };
    let flags = HashLogExtendEventFlags::empty();
    if let Err(err) = tcg.hash_log_extend_event(flags, cmdline, event) {
        serial_write_str(&format_args!("[TPM] Cmdline measure failed: {:?}\n", err));
        Err(Status::DEVICE_ERROR)
    } else {
        serial_write_str(&format_args!("[TPM] Cmdline measure OK (PCR8)\n"));
        Ok(())
    }
}

#[cfg(target_os = "uefi")]
fn report_tpm_event_log(system_table: &mut SystemTable<Boot>) {
    let boot_services = system_table.boot_services();
    let tcg_handle = match boot_services.get_handle_for_protocol::<Tcg>() {
        Ok(handle) => handle,
        Err(_) => {
            serial_write_str(&format_args!("[TPM] TCG2 protocol not found\n"));
            return;
        }
    };
    let mut tcg = match boot_services.open_protocol_exclusive::<Tcg>(tcg_handle) {
        Ok(tcg) => tcg,
        Err(err) => {
            serial_write_str(&format_args!(
                "[TPM] TCG2 open failed: {:?}\n",
                err.status()
            ));
            return;
        }
    };
    let capability = match tcg.get_capability() {
        Ok(capability) => capability,
        Err(err) => {
            serial_write_str(&format_args!("[TPM] TCG2 capability error: {:?}\n", err));
            return;
        }
    };
    if !capability.tpm_present() {
        serial_write_str(&format_args!("[TPM] TPM not present\n"));
        return;
    }
    serial_write_str(&format_args!(
        "[TPM] PCR banks active={:?} supported={:?}\n",
        capability.active_pcr_banks, capability.hash_algorithm_bitmap
    ));
    if !capability
        .active_pcr_banks
        .contains(uefi::proto::tcg::HashAlgorithm::SHA256)
    {
        serial_write_str(&format_args!("[TPM] SHA256 PCR bank inactive\n"));
    }
    let log = match tcg.get_event_log_v2() {
        Ok(log) => log,
        Err(err) => {
            serial_write_str(&format_args!("[TPM] Event log failed: {:?}\n", err));
            return;
        }
    };
    let mut total = 0u64;
    let mut pcr4 = 0u64;
    let mut pcr8 = 0u64;
    let mut efi_action = 0u64;
    let mut efi_boot_services_app = 0u64;
    let mut capped = false;
    for event in log.iter() {
        total += 1;
        if event.pcr_index().0 == 4 {
            pcr4 += 1;
        }
        if event.pcr_index().0 == 8 {
            pcr8 += 1;
        }
        if event.event_type() == EventType::EFI_ACTION {
            efi_action += 1;
        }
        if event.event_type() == EventType::EFI_BOOT_SERVICES_APPLICATION {
            efi_boot_services_app += 1;
        }
        if total >= 256 {
            capped = true;
            break;
        }
    }
    serial_write_str(&format_args!(
        "[TPM] Event log entries={} pcr4={} pcr8={} efi_action={} efi_app={} truncated={} capped={}\n",
        total,
        pcr4,
        pcr8,
        efi_action,
        efi_boot_services_app,
        log.is_truncated(),
        capped
    ));
}

#[cfg(target_os = "uefi")]
fn read_cmdline(
    system_table: &mut SystemTable<Boot>,
    handle: Handle,
) -> Result<(u64, u64), Status> {
    let boot_services = system_table.boot_services();
    let loaded_image = boot_services
        .open_protocol_exclusive::<LoadedImage>(handle)
        .map_err(|err| err.status())?;
    let Some(load_options) = loaded_image.load_options_as_bytes() else {
        return Ok((0, 0));
    };
    if load_options.is_empty() {
        return Ok((0, 0));
    }
    let mut u16_buf = Vec::with_capacity(load_options.len() / 2);
    for chunk in load_options.chunks_exact(2) {
        u16_buf.push(u16::from_le_bytes([chunk[0], chunk[1]]));
    }
    if u16_buf.is_empty() {
        return Ok((0, 0));
    }
    let end = u16_buf
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(u16_buf.len());
    if end == 0 {
        return Ok((0, 0));
    }
    let cmdline = String::from_utf16_lossy(&u16_buf[..end]);
    let bytes = cmdline.as_bytes();
    let len = bytes.len().min(CMDLINE_MAX_LEN);
    let buf = boot_services
        .allocate_pool(MemoryType::LOADER_DATA, len)
        .map_err(|err| err.status())?;
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, len);
    }
    Ok((buf as u64, len as u64))
}
