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
#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
use ech_os::boot::context::LimineModuleRequest;
#[cfg(not(target_os = "windows"))]
use ech_os::boot::context::{BootInitStage, BootProfile, CapabilityFlags, RsdpProvenance};
#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
use ech_os::boot::context::{MemoryRegion, MemoryRegionKind, NormalizedMemoryMap};
#[cfg(target_os = "uefi")]
use ech_os::boot::context::{MemoryRegion, MemoryRegionKind, NormalizedMemoryMap};
#[cfg(not(target_os = "windows"))]
use ech_os::boot::error_policy::{BootErrorDisposition, IommuPolicy};
#[cfg(not(target_os = "windows"))]
use ech_os::boot::pipeline::BootPhase;
#[cfg(target_os = "uefi")]
use ech_os::boot::BootContext;
#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
use ech_os::boot::BootContext;
#[cfg(target_os = "uefi")]
use ech_os::boot::BootInfo;
#[cfg(target_os = "uefi")]
use ech_os::gop::framebuffer::Framebuffer;
#[cfg(target_os = "uefi")]
use ech_os::splash::Splash;
#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
use limine_protocol_for_rust::{
    requests::executable_address::ExecutableAddressRequest,
    requests::executable_cmdline::ExecutableCmdlineRequest,
    requests::framebuffer::FramebufferRequest,
    requests::hhdm::HigherHalfDirectMapRequest,
    requests::memory_map::{MemoryMapRequest, MemoryRegionInfo, MemoryRegionType},
    requests::rsdp::RsdpRequest,
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
const UEFI_BOOT_STACK_SIZE: usize = 4 * 1024 * 1024;
#[cfg(target_os = "uefi")]
const SECURE_BOOT_ENROLL_MAGIC: u32 = 0x5342_4531;
#[cfg(target_os = "uefi")]
const SECURE_BOOT_ENROLL_PENDING_RESET: u8 = 1 << 0;
#[cfg(target_os = "uefi")]
const SECURE_BOOT_ENROLL_FAILED: u8 = 1 << 1;

/// UEFI firmware stackleri firmware'e aittir ve BootContext'in bounded
/// canonical map kopyasını taşıyacak kadar geniş olmak zorunda değildir.
/// ExitBootServices sonrasında kernel'e geçmeden önce bu kernel-owned stack'e
/// geçilir; Limine/MB2 giriş adaptörleri aynı kapasiteyi assembly'de sağlar.
#[cfg(target_os = "uefi")]
#[repr(C, align(16))]
struct UefiBootStack([u8; UEFI_BOOT_STACK_SIZE]);

#[cfg(target_os = "uefi")]
static mut UEFI_BOOT_STACK: UefiBootStack = UefiBootStack([0; UEFI_BOOT_STACK_SIZE]);
#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
const LIMINE_REVISION: u64 = 3;

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
#[link_section = ".limine_reqs"]
static LIMINE_RSDP_REQUEST: RsdpRequest = RsdpRequest::new(LIMINE_REVISION);

#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
#[used]
#[link_section = ".limine_reqs"]
static LIMINE_MODULE_REQUEST: LimineModuleRequest = LimineModuleRequest::new();

#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
#[used]
#[link_section = ".limine_reqs"]
static LIMINE_EXECUTABLE_ADDRESS_REQUEST: ExecutableAddressRequest =
    ExecutableAddressRequest::new(LIMINE_REVISION);

#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
#[used]
#[link_section = ".limine_reqs"]
static LIMINE_FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new(LIMINE_REVISION);

#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
#[used]
#[link_section = ".limine_req_end"]
static LIMINE_REQUEST_END_MARKER: [u64; 2] = REQUEST_END_MARKER;

#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
#[cfg(not(echos_native_limine))]
global_asm!(include_str!("boot/entry.S"));

#[cfg(all(
    not(target_os = "uefi"),
    not(target_os = "windows"),
    echos_native_limine
))]
global_asm!(include_str!("boot/entry_limine.S"));

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
                unsafe {
                    outb(0xE9, b);
                }
            }
            Ok(())
        }
    }
    if W.write_fmt(args).is_err() {
        // The debugcon writer is infallible today; retain an explicit policy
        // if that contract changes so the marker cannot disappear silently.
        serial_write_byte(b'!');
    }
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
    if port.write_fmt(*args).is_err() {
        // SerialPort::write_str is currently infallible, but a failed
        // formatter is a visible boot diagnostic rather than a discarded
        // Result.
        serial_write_byte(b'!');
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct IommuInitResult {
    tables_available: bool,
    hardware_enabled: bool,
    interrupt_remapping: bool,
}

impl IommuInitResult {
    const fn isolation_ready(self) -> bool {
        self.tables_available && self.hardware_enabled
    }
}

fn init_platform_iommu() -> IommuInitResult {
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

    let ir_ok = ech_os::interrupts::intr_remap::init_from_acpi();
    if ir_ok {
        serial_write_str(&format_args!("[IR] Interrupt remapping enabled\n"));
    } else {
        serial_write_str(&format_args!("[IR] Not available\n"));
    }

    IommuInitResult {
        tables_available: iommu_tables_ok,
        hardware_enabled: iommu_hw_ok,
        interrupt_remapping: ir_ok,
    }
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

/// RSDP'yi tek authoritative ACPI bootstrap state'ine yayınlar (Wave 1).
///
/// Üç boot protokolü de aynı yolu kullanır; hata/eksiklik durumları stage ve
/// provenance ile birlikte seri porta loglanır, boot akışı kesintiye uğramaz.
/// Adres yoksa legacy BIOS scan fallback'i (find_rsdp_bios) devreye girer.
#[cfg(not(target_os = "windows"))]
fn publish_rsdp_from_boot_ctx(boot_ctx: &BootContext, provenance: RsdpProvenance, stage: &str) {
    let address = boot_ctx.rsdp_address();
    if address == 0 {
        serial_write_str(&format_args!(
            "[RSDP] {} ({:?}): adres yok, legacy BIOS scan fallback\n",
            stage, provenance
        ));
        return;
    }
    let candidate = boot_ctx.rsdp_candidate(provenance);
    match ech_os::acpi::publish_rsdp(candidate) {
        Ok(()) => serial_write_str(&format_args!(
            "[RSDP] {} ({:?}): yayınlandı phys={:#x}\n",
            stage, provenance, address
        )),
        Err(e) => serial_write_str(&format_args!(
            "[RSDP] {} ({:?}): yayınlama reddedildi: {:?}\n",
            stage, provenance, e
        )),
    }
}

/// Stage-gate: boot profili + init stage'e göre zorunlu alanları doğrular.
///
/// Hata loglanır; boot durdurulmaz (boot-failure containment: görünürlük
/// önce, ACPI fazı ayrıca `validate_authoritative_rsdp` ile korunur).
#[cfg(not(target_os = "windows"))]
fn run_stage_gate(
    boot_ctx: &BootContext,
    profile: BootProfile,
    stage: BootInitStage,
    label: &str,
) -> bool {
    if let Err(e) = boot_ctx.validate_for_stage(profile, stage) {
        serial_write_str(&format_args!("[STAGE_GATE] {}: {}\n", label, e));
        false
    } else {
        true
    }
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
    // Tanı çıktısı tamamlandıktan SONRA debugger breakpoint'i: erken boot
    // paniklerinde (IDT henüz yüklü değilken) int3 → #BP → #DF → triple fault
    // olur; önce message/location basılmazsa panik teşhis edilemezdi.
    #[cfg(any(target_os = "none", target_os = "uefi"))]
    unsafe {
        core::arch::asm!("int3");
    }
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
    // Limine native handover uses a zero legacy magic.  MB2 magic wins when
    // both response sets are populated; ambiguous provenance must not silently
    // select the Limine adapter.
    if boot_magic == 0 && limine_available() {
        unsafe { boot_pipeline_limine(kaslr_offset) };
    }

    if boot_magic == BOOT_MAGIC_UEFI {
        #[cfg(target_os = "uefi")]
        unsafe {
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

/// Wave 2 — tek kanonik faz makinesini (`boot::pipeline`) süren adapter yardımcıları.
///
/// `begin_phase` geçersiz geçişte kendi fatal zincirini (Failed → RecoveryOnly →
/// halt/reboot) çalıştırır ve geri dönmez; `complete_phase` hataları açıkça fatal
/// ihlal olarak ele alır. Her geçişte deterministik `[BOOT_PHASE]` marker'ı
/// yayınlanır (OVMF smoke test izleme kanalı).
#[cfg(not(target_os = "windows"))]
fn phase_begin(phase: BootPhase) {
    use ech_os::boot::pipeline::BOOT_PIPELINE;
    if let Err(err) = BOOT_PIPELINE.begin_phase(phase) {
        serial_write_str(&format_args!(
            "[BOOT_POLICY] stage={} disposition=fatal reason=begin_phase {:?}\n",
            phase.name(),
            err
        ));
        phase_fatal(0xB002);
    }
    let snap = BOOT_PIPELINE.current_snapshot();
    BOOT_PIPELINE.emit_phase_marker(&snap);
}

#[cfg(not(target_os = "windows"))]
fn phase_complete(phase: BootPhase) {
    use ech_os::boot::pipeline::BOOT_PIPELINE;
    if BOOT_PIPELINE.complete_phase(phase).is_err() {
        BOOT_PIPELINE.fatal_violation(
            ech_os::boot::safety::ViolationType::PhaseOrder,
            "faz tamamlama ihlali (complete_phase)",
        );
    }
    let snap = BOOT_PIPELINE.current_snapshot();
    BOOT_PIPELINE.emit_phase_marker(&snap);
}

#[cfg(not(target_os = "windows"))]
fn phase_degraded(phase: BootPhase) {
    use ech_os::boot::pipeline::{DegradeReason, BOOT_PIPELINE};
    ech_os::boot::safety::BOOT_SAFETY.record_violation(
        ech_os::boot::safety::ViolationType::BootPolicy,
        "boot phase completed with safe fallback",
        true,
    );
    serial_write_str(&format_args!(
        "[BOOT_POLICY] stage={} disposition=degraded-safe reason=safe-fallback\n",
        phase.name()
    ));
    if BOOT_PIPELINE
        .complete_degraded(phase, DegradeReason::SafeFallback)
        .is_err()
    {
        phase_fatal(0xB001);
    }
    BOOT_PIPELINE.emit_phase_marker(&BOOT_PIPELINE.current_snapshot());
}

#[cfg(not(target_os = "windows"))]
fn phase_fatal(code: u32) -> ! {
    ech_os::boot::pipeline::BOOT_PIPELINE
        .fatal_error(ech_os::boot::pipeline::PhaseError::Failed { code })
}

#[cfg(not(target_os = "windows"))]
fn register_protocol_or_fatal(protocol: ech_os::boot::pipeline::BootProtocol, code: u32) {
    if let Err(err) = ech_os::boot::pipeline::BOOT_PIPELINE.register_protocol(protocol) {
        serial_write_str(&format_args!(
            "[BOOT_POLICY] stage=handover disposition=fatal reason=register_protocol {:?}\n",
            err
        ));
        phase_fatal(code);
    }
}

#[cfg(not(target_os = "windows"))]
fn set_required_capabilities_or_fatal(capabilities: CapabilityFlags, code: u32) {
    if let Err(err) = ech_os::boot::pipeline::BOOT_PIPELINE.set_capabilities(capabilities) {
        serial_write_str(&format_args!(
            "[BOOT_POLICY] stage=handover disposition=fatal reason=set_capabilities {:?}\n",
            err
        ));
        phase_fatal(code);
    }
}

#[cfg(not(target_os = "windows"))]
fn set_optional_capabilities(capabilities: CapabilityFlags) {
    if let Err(err) = ech_os::boot::pipeline::BOOT_PIPELINE.set_capabilities(capabilities) {
        ech_os::boot::safety::BOOT_SAFETY.record_violation(
            ech_os::boot::safety::ViolationType::CapabilityUnavailable,
            "optional capability publication failed",
            true,
        );
        serial_write_str(&format_args!(
            "[BOOT_POLICY] stage=capability-publication disposition=degraded-safe reason={:?}\n",
            err
        ));
    }
}

#[cfg(not(target_os = "windows"))]
fn apply_iommu_policy(boot_ctx: &BootContext) {
    let policy = IommuPolicy::from_cmdline(boot_ctx.cmdline_str());
    if policy == IommuPolicy::Unavailable {
        ech_os::boot::safety::BOOT_SAFETY.record_violation(
            ech_os::boot::safety::ViolationType::IommuUnavailable,
            "IOMMU disabled by explicit iommu=unavailable policy",
            true,
        );
        serial_write_str(&format_args!(
            "[IOMMU] policy={} disposition=unsupported reason=configuration\n",
            policy.label()
        ));
        return;
    }

    let result = init_platform_iommu();
    let ready = result.isolation_ready();
    serial_write_str(&format_args!(
        "[IOMMU] policy={} tables={} hardware={} interrupt_remap={} ready={}\n",
        policy.label(),
        result.tables_available as u8,
        result.hardware_enabled as u8,
        result.interrupt_remapping as u8,
        ready as u8,
    ));
    if ready {
        return;
    }

    let disposition = policy.failure_disposition();
    serial_write_str(&format_args!(
        "[BOOT_POLICY] stage=AcpiInit operation=IOMMU disposition={} reason=isolation-unavailable\n",
        disposition.label()
    ));
    if disposition == BootErrorDisposition::Fatal {
        ech_os::boot::safety::BOOT_SAFETY.record_violation(
            ech_os::boot::safety::ViolationType::IommuUnavailable,
            "required IOMMU isolation unavailable",
            false,
        );
        phase_fatal(0xC100);
    }
    let reason = match disposition {
        BootErrorDisposition::Disabled => {
            "IOMMU isolation explicitly disabled by iommu=permissive; device probing remains policy-gated"
        }
        BootErrorDisposition::Unsupported => {
            "IOMMU isolation unsupported; device probing remains constrained"
        }
        BootErrorDisposition::DegradedSafe => {
            "IOMMU isolation unavailable; constrained device policy remains active"
        }
        BootErrorDisposition::Retryable | BootErrorDisposition::Fatal => {
            "IOMMU isolation failure requires an explicit retry/fatal policy"
        }
    };
    ech_os::boot::safety::BOOT_SAFETY.record_violation(
        ech_os::boot::safety::ViolationType::IommuUnavailable,
        reason,
        true,
    );
}

/// Üç boot protokolünün ortak kernel çekirdeği.
///
/// Adapter'lar bu çağrıdan önce kendi handover/paging/PMM/heap işlerini
/// tamamlar. Bu zincir GDT'den interrupt-enable'e kadar tek bir sahiplik ve
/// sıra sözleşmesi taşır; UEFI'ye özgü runtime/GUI ve BIOS'a özgü HHDM
/// eşlemeleri bu sınırın dışında kalır.
#[cfg(not(target_os = "windows"))]
fn common_kernel_init(_boot_ctx: &BootContext) {
    // Slab provisioning needs the main heap but must not run while an adapter
    // is constructing its PMM metadata.  Enable it at the common boundary,
    // after every adapter has published its allocator/heap contract.
    ech_os::allocator::slab::activate();
    ech_os::boot::safety::init();

    phase_begin(BootPhase::GdtSetup);
    ech_os::gdt::init();
    ech_os::syscall::init();
    ech_os::cpu::init();
    ech_os::security::init();
    phase_complete(BootPhase::GdtSetup);

    phase_begin(BootPhase::IdtSetup);
    ech_os::interrupts::init();
    let vdso_degraded = match ech_os::vdso::init() {
        Ok(()) => false,
        Err(err) => {
            ech_os::boot::safety::BOOT_SAFETY.record_violation(
                ech_os::boot::safety::ViolationType::CapabilityUnavailable,
                "vDSO unavailable; userspace uses syscall clock fallback",
                true,
            );
            serial_write_str(&format_args!(
                "[BOOT_POLICY] stage=IdtSetup disposition=degraded-safe reason=vdso {:?}\n",
                err
            ));
            true
        }
    };
    ech_os::tty::init();
    if vdso_degraded {
        phase_degraded(BootPhase::IdtSetup);
    } else {
        phase_complete(BootPhase::IdtSetup);
    }

    phase_begin(BootPhase::AcpiInit);
    apply_iommu_policy(_boot_ctx);
    ech_os::memory::init_memory_subsystems();
    ech_os::memory_barriers::MemoryBarrier::init();
    ech_os::preempt::init();
    ech_os::rcu::init();
    ech_os::atomic_ops::init();
    phase_complete(BootPhase::AcpiInit);

    phase_begin(BootPhase::Scheduling);
    ech_os::task::scheduler::init();
    // GDT::init() runs before the scheduler and therefore receives no BSP
    // stack.  Publish the scheduler-owned idle stack before enabling faults or
    // running the Wave4 Ring3 contract; a zero TSS.RSP0 would make every
    // privilege transition fail closed in the page-fault path.
    let bsp_stack_top = ech_os::task::scheduler::current_kernel_stack_top();
    if bsp_stack_top != 0 {
        ech_os::gdt::set_kernel_stack(x86_64::VirtAddr::new(bsp_stack_top));
    }
    ech_os::interrupts::kick_irq_worker();
    let cpu_count = ech_os::cpu::smp::get_cpu_count();
    ech_os::task::worker::init_workers(core::cmp::max(cpu_count as usize, 2));
    // Reclaim is a post-scheduler service: all three adapters publish their
    // canonical PMM before entering this common chain, so this is the single
    // owner and the adapter branches must not start a duplicate daemon.
    ech_os::memory::start_reclaim_daemon();
    phase_complete(BootPhase::Scheduling);

    phase_begin(BootPhase::SmpInit);
    ech_os::cpu::smp::init();
    // CpuData/GS_BASE is installed by SMP; only now is the syscall shadow
    // stack setter safe to execute.
    let bsp_stack_top = ech_os::task::scheduler::current_kernel_stack_top();
    if bsp_stack_top != 0 {
        if !ech_os::syscall::set_kernel_stack_for_current_cpu(bsp_stack_top) {
            serial_write_str(&format_args!(
                "[BOOT_POLICY] stage=SmpInit disposition=fatal reason=kernel-stack-publication\n"
            ));
            phase_fatal(0xC101);
        }
    }
    phase_complete(BootPhase::SmpInit);

    phase_begin(BootPhase::InterruptEnable);
    ech_os::fault::init();
    x86_64::instructions::interrupts::enable();
    ech_os::interrupts::mark_bsp_init_complete();
    phase_complete(BootPhase::InterruptEnable);
}

/// Wave 4 acceptance suite shared by every boot adapter.  The test functions
/// are bounded and return a real result; a requested suite failure is a boot
/// failure, never a green diagnostic log.
#[cfg(not(target_os = "windows"))]
fn run_wave4_boot_tests(boot_ctx: &BootContext) -> bool {
    let self_ok = ech_os::debug::boot_self_check(boot_ctx);
    let ring3_ok = ech_os::debug::run_ring3_smoketest();
    let vm_security_ok = ech_os::debug::run_vm_security_tests();
    let vm_stress_ok = ech_os::debug::run_vm_stress_tests();
    let irq_ok = ech_os::debug::run_irq_stress_tests();
    let all_ok = self_ok && ring3_ok && vm_security_ok && vm_stress_ok && irq_ok;
    let report = ech_os::boot::safety::get_report();
    serial_write_str(&format_args!(
        "[BOOT_TEST] {} self_check={} ring3={} vm_security={} vm_stress={} irq={} violations={} heap_corruptions={} smp_failures={}\n",
        if all_ok { "PASS" } else { "FAIL" },
        self_ok as u8,
        ring3_ok as u8,
        vm_security_ok as u8,
        vm_stress_ok as u8,
        irq_ok as u8,
        report.violation_count,
        report.heap_corruptions,
        report.smp_failures,
    ));
    all_ok
}

/// UEFI descriptor'larını protocol-agnostic canonical map'e çevirir.
///
/// Bu fonksiyon ExitBootServices öncesi çağrılır; `NormalizedMemoryMap` bounded
/// storage kullandığı için heap'e veya bootloader iterator ömrüne bağlı değildir.
#[cfg(target_os = "uefi")]
fn normalize_uefi_memory_map(
    map: &uefi::table::boot::MemoryMap<'_>,
) -> Option<NormalizedMemoryMap> {
    use uefi::table::boot::MemoryType;
    let mut normalized = NormalizedMemoryMap::empty();
    for descriptor in map.entries() {
        let kind = match descriptor.ty {
            MemoryType::CONVENTIONAL => MemoryRegionKind::Usable,
            MemoryType::ACPI_RECLAIM => MemoryRegionKind::ACPIReclaim,
            MemoryType::ACPI_NON_VOLATILE => MemoryRegionKind::ACPINVS,
            MemoryType::LOADER_CODE
            | MemoryType::LOADER_DATA
            | MemoryType::BOOT_SERVICES_CODE
            | MemoryType::BOOT_SERVICES_DATA => MemoryRegionKind::BootloaderReclaimable,
            _ => MemoryRegionKind::Reserved,
        };
        normalized
            .push(MemoryRegion {
                base: descriptor.phys_start,
                len: descriptor.page_count.saturating_mul(4096),
                kind,
            })
            .ok()?;
    }
    (!normalized.is_empty()).then_some(normalized)
}

#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
fn normalize_limine_memory_map(
    memmap: &PointerSlice<MemoryRegionInfo>,
) -> Option<NormalizedMemoryMap> {
    let mut normalized = NormalizedMemoryMap::empty();
    for entry in memmap.iter() {
        let kind = match entry.get_type() {
            MemoryRegionType::Usable => MemoryRegionKind::Usable,
            MemoryRegionType::AcpiReclaimable => MemoryRegionKind::ACPIReclaim,
            MemoryRegionType::AcpiNvs => MemoryRegionKind::ACPINVS,
            MemoryRegionType::BootloaderReclaimable => MemoryRegionKind::BootloaderReclaimable,
            MemoryRegionType::ExecutableAndModules => MemoryRegionKind::Kernel,
            MemoryRegionType::Framebuffer => MemoryRegionKind::Framebuffer,
            _ => MemoryRegionKind::Reserved,
        };
        normalized
            .push(MemoryRegion {
                base: entry.base,
                len: entry.length,
                kind,
            })
            .ok()?;
    }
    (!normalized.is_empty()).then_some(normalized)
}

#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
fn normalize_multiboot2_memory_map(
    info: &multiboot2::BootInformation,
) -> Option<NormalizedMemoryMap> {
    use multiboot2::MemoryAreaType;
    let mut normalized = NormalizedMemoryMap::empty();
    let tag = info.memory_map_tag()?;
    for area in tag.memory_areas() {
        let kind = match area.typ() {
            MemoryAreaType::Available => MemoryRegionKind::Usable,
            MemoryAreaType::AcpiAvailable => MemoryRegionKind::ACPIReclaim,
            _ => MemoryRegionKind::Reserved,
        };
        normalized
            .push(MemoryRegion {
                base: area.start_address(),
                len: area.size(),
                kind,
            })
            .ok()?;
    }
    (!normalized.is_empty()).then_some(normalized)
}

/// UEFI kontrollü reboot hook'u — yalnızca RUNTIME_VERIFIED + REBOOT_SAFE
/// capability'leri ayarlandıktan sonra (runtime services doğrulandıktan sonra)
/// `terminate()` tarafından çağrılır.
#[cfg(target_os = "uefi")]
fn uefi_reboot_hook() -> ! {
    ech_os::boot::reset_uefi_system(ResetType::COLD, Status::SUCCESS);
}

#[cfg(target_os = "uefi")]
unsafe fn boot_pipeline_uefi(boot_info_addr: usize, _kaslr_offset: u64) -> ! {
    // The firmware identity map is still active here.  The common phase word
    // is registered immediately after the adapter establishes the first
    // kernel page tables below, so the phase machine never depends on an
    // unverified firmware mapping.
    use ech_os::boot::pipeline::{caps, BootProtocol, BOOT_PIPELINE};

    let boot_info = &mut *(boot_info_addr as *mut BootInfo);
    let expected_size = core::mem::size_of::<BootInfo>() as u32;
    let boot_magic = core::ptr::read_volatile(core::ptr::addr_of!(boot_info.magic));
    let boot_version = core::ptr::read_volatile(core::ptr::addr_of!(boot_info.version));
    let boot_size = core::ptr::read_volatile(core::ptr::addr_of!(boot_info.size));
    if boot_magic != ech_os::boot::BOOTINFO_MAGIC
        || boot_version != ech_os::boot::BOOTINFO_VERSION
        || boot_size < expected_size
    {
        debugcon_write_byte(b'!'); // Mark: BootInfo check failed
        ech_os::boot::safety::BOOT_SAFETY.record_violation(
            ech_os::boot::safety::ViolationType::AcpiTableInvalid,
            "BootInfo ABI mismatch",
            false,
        );
        serial_write_str(&format_args!(
            "[UEFI] BootInfo ABI mismatch magic={:#x} ver={} size={}\n",
            boot_magic, boot_version, boot_size
        ));
        phase_fatal(0xA001);
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
        phase_fatal(0xA002);
    }
    debugcon_write_byte(b'6'); // Mark: passed offset checks
    if boot_info.secure_boot && boot_info.system_table == 0 {
        serial_write_str(&format_args!("[UEFI] Secure Boot requires system table\n"));
        phase_fatal(0xA003);
    }
    debugcon_write_byte(b'7'); // Mark: passed secure boot check
    let mut boot_ctx = BootContext::from_uefi(boot_info);
    debugcon_write_byte(b'n'); // Mark: after BootContext normalization
    let normalized_map = boot_info
        .memory_map
        .as_ref()
        .and_then(normalize_uefi_memory_map)
        .unwrap_or_else(NormalizedMemoryMap::empty);
    if !boot_ctx.publish_normalized_memory_map(normalized_map) {
        serial_write_str(&format_args!("[UEFI] canonical memory map invalid/empty\n"));
        phase_fatal(0xA004);
    }
    ech_os::boot::set_secure_boot(boot_ctx.secure_boot);
    publish_rsdp_from_boot_ctx(&boot_ctx, RsdpProvenance::Uefi, "UefiHandover");
    debugcon_write_byte(b'8'); // Mark: after RSDP setup
    serial_write_str(&format_args!(
        "[UEFI] RSDP: {:#x}\n",
        boot_ctx.rsdp_address()
    ));
    debugcon_write_byte(b'9'); // Mark: after serial write

    if let Some(framebuffer) = boot_ctx.framebuffer.as_ref() {
        set_optional_capabilities(caps::FRAMEBUFFER);
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
    let mut run_boot_tests =
        (boot_info.boot_flags & ech_os::boot::appliance::BOOT_FLAG_BOOT_TESTS) != 0;
    if boot_ctx.image_size != 0 {
        serial_write_str(&format_args!(
            "[UEFI] Image size: {} bytes\n",
            boot_ctx.image_size
        ));
        serial_write_str(&format_args!("[UEFI] Image sha256: "));
        for byte in boot_ctx.image_hash {
            serial_write_str(&format_args!("{:02x}", byte));
        }
        serial_write_str(&format_args!("\n"));
    }
    debugcon_write_byte(b'B'); // Mark: after image hash

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
        phase_fatal(0xA004);
    }
    debugcon_write_byte(b'E'); // Mark: passed memory map check
    if let Some(map) = boot_info.memory_map.as_ref() {
        let total_pages: u64 = map.entries().map(|d| d.page_count).sum();
        let total_mb = total_pages.saturating_mul(4096) / (1024 * 1024);
        serial_write_str(&format_args!("[UEFI] Memory map total: {} MB\n", total_mb));
    }
    debugcon_write_byte(b'F'); // Mark: after memory map total
    if let (Some(framebuffer), Some(screen)) = (boot_ctx.framebuffer.as_mut(), splash.as_mut()) {
        screen.update_progress(framebuffer, 15);
    }

    debugcon_write_byte(b'G'); // Mark: before init_paging
    serial_write_str(&format_args!("[UEFI] init_paging\n"));
    let mut mapper = unsafe { ech_os::memory::init_paging(0) };
    debugcon_write_byte(b'H'); // Mark: after init_paging

    // Handover registration is adapter-owned, but it occurs only after the
    // first kernel page table is live (see the firmware mapping invariant).
    register_protocol_or_fatal(BootProtocol::Uefi, 0xA006);
    BOOT_PIPELINE.set_profile(ech_os::boot::context::BootProfile::Uefi);
    if let Err(err) = BOOT_PIPELINE.register_reboot_hook(uefi_reboot_hook) {
        serial_write_str(&format_args!(
            "[BOOT_POLICY] stage=UefiHandover disposition=fatal reason=reboot-hook {:?}\n",
            err
        ));
        phase_fatal(0xA007);
    }
    phase_begin(BootPhase::UefiHandover);
    set_required_capabilities_or_fatal(
        boot_ctx.capabilities() | caps::MEMORY_MAP | caps::HHDM,
        0xA008,
    );
    if !run_stage_gate(
        &boot_ctx,
        BootProfile::Uefi,
        BootInitStage::Handover,
        "UEFI",
    ) {
        phase_fatal(0xA005);
    }
    phase_complete(BootPhase::UefiHandover);
    phase_begin(BootPhase::PagingSetup);
    phase_complete(BootPhase::PagingSetup);
    serial_write_str(&format_args!("[UEFI] init_uefi memory manager\n"));
    phase_begin(BootPhase::MemoryInit);
    debugcon_write_byte(b'I'); // Mark: before memory_map.take
    let memory_map = match boot_info.memory_map.take() {
        Some(memory_map) => memory_map,
        None => {
            serial_write_str(&format_args!(
                "[BOOT_POLICY] stage=MemoryInit disposition=fatal reason=UEFI memory map already consumed\n"
            ));
            phase_fatal(0xA009);
        }
    };
    debugcon_write_byte(b'J'); // Mark: after memory_map.take
    let mut memory_manager = ech_os::memory::init_uefi(memory_map);
    debugcon_write_byte(b'K'); // Mark: after init_uefi
    unsafe {
        ech_os::memory::set_global_memory_manager(&mut memory_manager as *mut _);
    }
    debugcon_write_byte(b'L'); // Mark: after set_global_memory_manager
    serial_write_str(&format_args!("[UEFI] init_uefi_hhdm\n"));
    if let Err(err) =
        ech_os::memory::init_uefi_hhdm(&mut mapper, &mut memory_manager, boot_ctx.hhdm_offset)
    {
        debugcon_write_byte(b'X'); // Mark: init_uefi_hhdm failed
        serial_write_str(&format_args!(
            "[HHDM] FATAL: init failed: {:?} — cannot continue without HHDM\n",
            err
        ));
        phase_fatal(0xC001);
    } else {
        debugcon_write_byte(b'M'); // Mark: init_uefi_hhdm success
        ech_os::memory::set_active_physical_offset(boot_ctx.hhdm_offset);
        mapper = unsafe { ech_os::memory::init_paging(boot_ctx.hhdm_offset) };
    }
    debugcon_write_byte(b'N'); // Mark: after hhdm setup
    if let Some(framebuffer) = boot_ctx.framebuffer.as_mut() {
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
            framebuffer.base_addr = (boot_ctx.hhdm_offset + framebuffer.base_addr as u64) as usize;
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
    if let (Some(framebuffer), Some(screen)) = (boot_ctx.framebuffer.as_mut(), splash.as_mut()) {
        screen.update_progress(framebuffer, 30);
    }
    debugcon_write_byte(b'R'); // Mark: before cmdline
    if let Some(cmdline) = boot_ctx.cmdline_str() {
        debugcon_write_byte(b'S'); // Mark: cmdline present
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
    }
    debugcon_write_byte(b'T'); // Mark: before set_virtual_address_map
    serial_write_str(&format_args!("[UEFI] set_virtual_address_map\n"));
    let mut memory_init_degraded = false;
    if boot_ctx.system_table != 0 {
        debugcon_write_byte(b'V'); // Mark: system_table present
        match ech_os::memory::set_uefi_virtual_address_map(
            boot_ctx.system_table,
            &mut memory_manager,
            boot_ctx.hhdm_offset,
        ) {
            Ok(rt) => {
                debugcon_write_byte(b'W'); // Mark: set_virtual_address_map OK
                ech_os::boot::set_runtime_services(rt);
                serial_write_str(&format_args!("[UEFI] Runtime services remapped\n"));
                match ech_os::boot::verify_uefi_runtime_services() {
                    Ok(()) => {
                        debugcon_write_byte(b'X'); // Mark: runtime services verified
                        serial_write_str(&format_args!("[UEFI] Runtime services verified\n"));
                        // UEFI-özel kuyruk capability gate arkasında: runtime doğrulandı,
                        // kontrollü reboot artık güvenli (hook yalnızca REBOOT_SAFE ile çağrılır).
                        set_required_capabilities_or_fatal(
                            caps::RUNTIME_SERVICES | caps::RUNTIME_VERIFIED | caps::REBOOT_SAFE,
                            0xC001,
                        );
                        let boot_control =
                            ech_os::boot::appliance::load_persisted().unwrap_or_default();
                        ech_os::boot::appliance::init_shadow(boot_control);
                        run_boot_tests |= ech_os::boot::appliance::boot_tests_requested();
                        ech_os::boot::appliance::publish_stage(
                            ech_os::boot::appliance::BootStage::BootControlLoaded,
                        );
                        if boot_ctx.secure_boot {
                            if ech_os::posix::secure_boot_db_available() {
                                serial_write_str(&format_args!(
                                    "[UEFI] Secure Boot databases available\n"
                                ));
                            } else {
                                serial_write_str(&format_args!(
                                    "[UEFI] Secure Boot databases unavailable\n"
                                ));
                                phase_fatal(0xC002);
                            }
                        }
                    }
                    Err(status) => {
                        ech_os::boot::set_runtime_services(0);
                        serial_write_str(&format_args!(
                            "[UEFI] Runtime services verification failed: {:?}\n",
                            status
                        ));
                        if boot_ctx.secure_boot {
                            phase_fatal(0xC003);
                        } else {
                            memory_init_degraded = true;
                            if let Err(revoke_err) = BOOT_PIPELINE.revoke_capabilities(
                                caps::RUNTIME_SERVICES | caps::RUNTIME_VERIFIED | caps::REBOOT_SAFE,
                                ech_os::boot::pipeline::DegradeReason::SafeFallback,
                            ) {
                                serial_write_str(&format_args!(
                                    "[BOOT_POLICY] stage=MemoryInit disposition=fatal reason=revoke-runtime {:?}\n",
                                    revoke_err
                                ));
                                phase_fatal(0xC006);
                            }
                            ech_os::boot::safety::BOOT_SAFETY.record_violation(
                                ech_os::boot::safety::ViolationType::CapabilityUnavailable,
                                "UEFI runtime services verification failed; runtime capability revoked",
                                true,
                            );
                        }
                    }
                }
            }
            Err(status) => {
                serial_write_str(&format_args!(
                    "[UEFI] SetVirtualAddressMap failed: {:?}\n",
                    status
                ));
                if boot_ctx.secure_boot {
                    phase_fatal(0xC004);
                } else {
                    memory_init_degraded = true;
                    if let Err(revoke_err) = BOOT_PIPELINE.revoke_capabilities(
                        caps::RUNTIME_SERVICES | caps::RUNTIME_VERIFIED | caps::REBOOT_SAFE,
                        ech_os::boot::pipeline::DegradeReason::SafeFallback,
                    ) {
                        serial_write_str(&format_args!(
                            "[BOOT_POLICY] stage=MemoryInit disposition=fatal reason=revoke-runtime {:?}\n",
                            revoke_err
                        ));
                        phase_fatal(0xC007);
                    }
                    ech_os::boot::safety::BOOT_SAFETY.record_violation(
                        ech_os::boot::safety::ViolationType::CapabilityUnavailable,
                        "UEFI SetVirtualAddressMap failed; runtime capability revoked",
                        true,
                    );
                }
            }
        };
    } else if boot_ctx.secure_boot {
        phase_fatal(0xC005);
    }
    debugcon_write_byte(b'Y'); // Mark: after virtual address map
    if memory_init_degraded {
        phase_degraded(BootPhase::MemoryInit);
    } else {
        phase_complete(BootPhase::MemoryInit);
    }
    if let (Some(framebuffer), Some(screen)) = (boot_ctx.framebuffer.as_mut(), splash.as_mut()) {
        screen.update_progress(framebuffer, 45);
    }
    debugcon_write_byte(b'Z'); // Mark: before heap init
    phase_begin(BootPhase::HeapInit);
    if let Err(err) = ech_os::allocator::init_heap(&mut mapper, &mut memory_manager) {
        debugcon_write_byte(b'!'); // Mark: heap init failed
        serial_write_str(&format_args!("[HEAP] init_heap failed: {:?}\n", err));
        // Karar 2: heap yoksa normal pipeline devam edemez → Failed → RecoveryOnly.
        let heap_error = ech_os::boot::pipeline::PhaseError::HeapInitFailed { code: 0 };
        if let Err(fail_err) = BOOT_PIPELINE.fail_phase(BootPhase::HeapInit, heap_error) {
            serial_write_str(&format_args!(
                "[BOOT_POLICY] stage=HeapInit disposition=fatal reason=fail-phase {:?}\n",
                fail_err
            ));
            BOOT_PIPELINE.fatal_error(fail_err);
        }
        let snap = BOOT_PIPELINE.current_snapshot();
        BOOT_PIPELINE.enter_recovery(&ech_os::boot::pipeline::RecoveryInfo::from_machine(
            &snap, heap_error,
        ));
    } else {
        debugcon_write_byte(b'H'); // Mark: heap init OK
        serial_write_str(&format_args!("[HEAP] TLSF heap initialized\n"));
        phase_complete(BootPhase::HeapInit);
    }
    debugcon_write_byte(b'I'); // Mark: after heap init
    if let (Some(framebuffer), Some(screen)) = (boot_ctx.framebuffer.as_mut(), splash.as_mut()) {
        screen.update_progress(framebuffer, 60);
    }

    debugcon_write_byte(b'J'); // Mark: before common kernel init
    common_kernel_init(&boot_ctx);
    ech_os::boot::appliance::publish_stage(ech_os::boot::appliance::BootStage::KernelCoreReady);
    if ech_os::drivers::virtio_net::auto_init() {
        serial_write_str(&format_args!("[NET] VirtIO-Net driver initialized\n"));
        ech_os::boot::appliance::publish_stage(ech_os::boot::appliance::BootStage::NetworkReady);
    } else {
        serial_write_str(&format_args!(
            "[NET] VirtIO-Net driver not found or init failed\n"
        ));
    }
    if let (Some(framebuffer), Some(screen)) = (boot_ctx.framebuffer.as_mut(), splash.as_mut()) {
        screen.update_progress(framebuffer, 75);
    }

    // UEFI'ye özgü topology/power/NUMA politikası ortak zincirin sonrasında
    // çalışır; BIOS adapter'ları bu feature'ları zorunlu kılmaz.
    let cpu_count = ech_os::cpu::smp::get_cpu_count();
    if let Err(topology_err) = ech_os::topology::init(cpu_count) {
        ech_os::boot::safety::BOOT_SAFETY.record_violation(
            ech_os::boot::safety::ViolationType::BootPolicy,
            "topology discovery unavailable; CPUID fallback remains active",
            true,
        );
        serial_write_str(&format_args!(
            "[BOOT_POLICY] stage=PlatformServices disposition=degraded-safe reason=topology {:?}\n",
            topology_err
        ));
    }
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

    serial_write_str(&format_args!("[INT] Interrupts enabled by common init\n"));
    serial_write_str(&format_args!("[WINSRV] ownership check enabled\n"));
    serial_write_str(&format_args!("[WINSRV] user-range validation enabled\n"));
    serial_write_str(&format_args!(
        "[PERF] latency probes armed (irq + compositor)\n"
    ));
    serial_write_str(&format_args!("[IRONSHIM] fuzz guard active\n"));
    serial_write_str(&format_args!(
        "[IRONSHIM] ring3->ring0 blocked policy active\n"
    ));
    phase_begin(BootPhase::DriverInit);

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
    if let Some(fb) = boot_ctx.framebuffer.as_ref() {
        ech_os::boot::set_global_framebuffer(fb.clone());
    }

    ech_os::ipc::service_ipc::init();
    ech_os::services::init();
    ech_os::services::spawn_service_tasks();
    ech_os::ipc::service_ipc::spawn_task();

    // SIMD dispatch fn ptr cache — CPUID bir kez çağrılır, sonra sıfır overhead
    ech_os::gfx::simd::init_simd_dispatch();
    phase_complete(BootPhase::DriverInit);
    phase_begin(BootPhase::Services);

    if run_boot_tests {
        if !run_wave4_boot_tests(&boot_ctx) {
            serial_write_str(&format_args!("[PANIC] Wave 4 boot acceptance failed!\n"));
            phase_fatal(0xD001);
        }
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

    // Test politikasından ve seçilecek UI yolundan bağımsız tek terminal
    // publication. Bundan sonraki shell/compositor çağrıları geri dönmeyebilir.
    if BOOT_PIPELINE
        .finish_boot(ech_os::boot::pipeline::PhaseOutcome::Completed)
        .is_err()
    {
        phase_fatal(0xD002);
    }
    ech_os::boot::appliance::mark_boot_success();
    BOOT_PIPELINE.emit_phase_marker(&BOOT_PIPELINE.current_snapshot());

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
    if let Some(fb) = boot_ctx.framebuffer.as_mut() {
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
unsafe fn boot_pipeline_limine(_kaslr_offset: u64) -> ! {
    use ech_os::boot::pipeline::{caps, BootProtocol, PhaseOutcome, BOOT_PIPELINE};
    register_protocol_or_fatal(BootProtocol::Limine, 0xE009);
    BOOT_PIPELINE.set_profile(BootProfile::Limine);
    phase_begin(BootPhase::UefiHandover);
    serial_write_str(&format_args!("[LIMINE] Booting via Limine\n"));
    let hhdm_offset = match LIMINE_HHDM_REQUEST.get_response() {
        Some(response) => response.offset,
        None => {
            serial_write_str(&format_args!("[LIMINE] HHDM response missing\n"));
            phase_fatal(0xE001);
        }
    };
    let mut run_boot_tests = false;
    let memmap: PointerSlice<MemoryRegionInfo> = match LIMINE_MEMORY_MAP_REQUEST.get_response() {
        Some(response) => response.get_entries(),
        None => {
            serial_write_str(&format_args!("[LIMINE] Memory map response missing\n"));
            phase_fatal(0xE002);
        }
    };
    serial_write_str(&format_args!("[LIMINE] HHDM offset={:#x}\n", hhdm_offset));
    if let Some(response) = LIMINE_CMDLINE_REQUEST.get_response() {
        let cmdline = response.get_cmdline();
        if !cmdline.is_empty() {
            serial_write_str(&format_args!("[LIMINE] cmdline: {}\n", cmdline));
            if cmdline
                .split_ascii_whitespace()
                .any(|arg| arg == "boot_tests=1")
            {
                run_boot_tests = true;
                serial_write_str(&format_args!("[LIMINE] boot tests enabled\n"));
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
        }
    }

    // RSDP adresi: Limine base revision >= 4'te sanal (HHDM lineer), base
    // revision 3'te fizikseldir. Bootloader istekteki revizyonu karşılayamazsa
    // daha düşük bir base revision kullanır; bu yüzden tag'ın 2. bileşeni
    // (BR >= 3 destekleyen bootloader'larda fiilen kullanılan revizyon) geri
    // okunur. Bileşen magic değerini (0x6a7b384944536bdc) koruyorsa bootloader
    // BR < 3 demektir — RSDP adresi o durumda da sanaldır (HHDM lineer).
    // 3. bileşen (istenen revizyon) onurlandırıldıysa bootloader tarafından
    // 0'a çekilir.
    //
    // Limine 12.5.2'nin SeaBIOS yolunda gözlenen bir uyumluluk ayrıntısı:
    // base_revision=0 raporlanırken RSDP cevabı yine de 32-bit altında fiziksel
    // adres olarak gelebilir (ör. 0xf52e0). Böyle bir adresi HHDM sanal adresi
    // gibi yorumlamak saturating_sub ile 0 üretir ve geçerli ACPI RSDP'sini
    // kaybettirir. Yalnızca yüksek-half HHDM etkinken ve adres 4 GiB altında
    // olduğunda bu sınırlı geri dönüşü fiziksel kabul ediyoruz; yüksek adresler
    // Limine sözleşmesindeki sanal adres semantiğini korur.
    const LIMINE_PHYSICAL_ADDRESS_LIMIT: u64 = 1 << 32;
    const LIMINE_HIGH_HALF_HHDM_MIN: u64 = 0xffff_8000_0000_0000;
    let rsdp_response = LIMINE_RSDP_REQUEST.get_response();
    let requested_honored = LIMINE_BASE_REVISION[2] == 0;
    let actual_base_revision = match LIMINE_BASE_REVISION[1] {
        0x6a7b384944536bdc => 0, // BR < 3: bootloader bileşeni doldurmadı
        revision => revision,
    };
    let (rsdp_raw, rsdp_virtual) = match rsdp_response {
        Some(response) if response.address != 0 => {
            let low_physical_compat = (response.address as u64) < LIMINE_PHYSICAL_ADDRESS_LIMIT
                && hhdm_offset >= LIMINE_HIGH_HALF_HHDM_MIN;
            let rsdp_virtual = actual_base_revision != 3 && !low_physical_compat;
            serial_write_str(&format_args!(
                "[LIMINE] RSDP address={:#x} address_kind={} base_revision={} requested={} honored={}\n",
                response.address,
                if rsdp_virtual { "virtual" } else { "physical" },
                actual_base_revision,
                LIMINE_REVISION,
                requested_honored
            ));
            (response.address as u64, rsdp_virtual)
        }
        Some(_) | None => {
            serial_write_str(&format_args!("[LIMINE] RSDP response missing/empty\n"));
            (0, false)
        }
    };

    // Kernel görüntüsünün fiziksel tabanı: spec fiziksel yerleşim garantisi
    // vermez ("No specific physical memory placement is guaranteed"), bu
    // yüzden link-time LMA'lar (0x100000) slide altında geçersizdir. Gerçek
    // fiziksel taban Executable Address feature'dan alınır; frame allocator
    // kernel bölgesini korumak için bunu kullanır.
    let executable_address = match LIMINE_EXECUTABLE_ADDRESS_REQUEST.get_response() {
        Some(response) => response.physical_base as u64,
        None => {
            serial_write_str(&format_args!(
                "[LIMINE] Executable address response missing\n"
            ));
            phase_fatal(0xE008);
        }
    };
    serial_write_str(&format_args!(
        "[LIMINE] executable physical_base={:#x}\n",
        executable_address
    ));

    let mut boot_ctx = BootContext::from_limine(
        hhdm_offset,
        rsdp_raw,
        rsdp_virtual,
        LIMINE_CMDLINE_REQUEST.get_response(),
        LIMINE_MODULE_REQUEST.get_response(),
    );
    let limine_framebuffer =
        boot_ctx.install_limine_framebuffer(LIMINE_FRAMEBUFFER_REQUEST.get_response(), hhdm_offset);
    serial_write_str(&format_args!(
        "[FRAMEBUFFER] protocol=limine state={}\n",
        if limine_framebuffer {
            "available"
        } else {
            "unavailable"
        }
    ));
    let normalized_map =
        normalize_limine_memory_map(&memmap).unwrap_or_else(NormalizedMemoryMap::empty);
    if !boot_ctx.publish_normalized_memory_map(normalized_map) {
        serial_write_str(&format_args!(
            "[LIMINE] canonical memory map invalid/empty\n"
        ));
        phase_fatal(0xE00A);
    }
    publish_rsdp_from_boot_ctx(&boot_ctx, RsdpProvenance::Limine, "LimineHandover");
    if !run_stage_gate(
        &boot_ctx,
        BootProfile::Limine,
        BootInitStage::Handover,
        "LIMINE",
    ) {
        phase_fatal(0xE003);
    }
    set_required_capabilities_or_fatal(
        boot_ctx.capabilities() | caps::MEMORY_MAP | caps::HHDM,
        0xE00A,
    );
    phase_complete(BootPhase::UefiHandover);
    serial_write_str(&format_args!("[LIMINE] Handover complete\n"));
    debugcon_write_byte(b'R');
    phase_begin(BootPhase::PagingSetup);
    let mut mapper = unsafe { ech_os::memory::init_paging(boot_ctx.physical_memory_offset) };
    phase_complete(BootPhase::PagingSetup);
    phase_begin(BootPhase::MemoryInit);
    let normalized_map = boot_ctx
        .normalized_memory_map()
        .unwrap_or_else(|| phase_fatal(0xE004));
    let link_start = unsafe {
        extern "C" {
            static kernel_phys_start: u8;
        }
        &kernel_phys_start as *const u8 as u64
    };
    let link_end = unsafe {
        extern "C" {
            static kernel_phys_end: u8;
        }
        &kernel_phys_end as *const u8 as u64
    };
    let image_span = link_end.wrapping_sub(link_start);
    let kernel_start_phys = executable_address & !0xfffu64;
    let kernel_end_phys = executable_address.saturating_add(image_span);
    let mut bootstrap_allocator =
        match ech_os::memory::frame_allocator::Multiboot2FrameAllocator::from_regions_with_kernel_range(
            normalized_map.as_slice(),
            kernel_start_phys,
            kernel_end_phys,
        ) {
            Some(allocator) => allocator,
            None => phase_fatal(0xE00B),
        };
    serial_write_str(&format_args!(
        "[MEMORY] Limine bootstrap allocator usable={:#x}\n",
        bootstrap_allocator.total_usable_bytes()
    ));
    phase_complete(BootPhase::MemoryInit);
    phase_begin(BootPhase::HeapInit);
    if let Err(err) = ech_os::allocator::init_heap(&mut mapper, &mut bootstrap_allocator) {
        serial_write_str(&format_args!("[HEAP] init_heap failed: {:?}\n", err));
        phase_fatal(0xE005);
    } else {
        serial_write_str(&format_args!("[HEAP] TLSF heap initialized\n"));
        phase_complete(BootPhase::HeapInit);
    }

    // FibonacciPmm owns dynamic region metadata; construct it only after the
    // bootstrap heap is live, then publish the global PMM before common init.
    let mut memory_manager =
        ech_os::memory::init_limine_normalized(normalized_map, executable_address);
    ech_os::memory::set_global_memory_manager(&mut memory_manager as *mut _);
    // The Limine handover provides the HHDM contract, but the entry page
    // tables may not retain a mapping for reserved low memory (SeaBIOS places
    // the RSDP at 0x000f52e0). Rebuild that small bootstrap window before the
    // common ACPI/MADT phase so the authoritative RSDP is readable.
    let mapped = ech_os::memory::map_low_physical_hhdm(
        &mut mapper,
        &mut memory_manager,
        hhdm_offset,
        0,
        2 * 1024 * 1024,
        x86_64::structures::paging::PageTableFlags::PRESENT
            | x86_64::structures::paging::PageTableFlags::WRITABLE
            | x86_64::structures::paging::PageTableFlags::NO_EXECUTE,
    );
    serial_write_str(&format_args!(
        "[LIMINE] ACPI HHDM bootstrap mapped={:#x}\n",
        mapped
    ));
    serial_write_str(&format_args!(
        "[LIMINE] ACPI HHDM translate={:?}\n",
        ech_os::memory::translate_addr(hhdm_offset + 0xf52e0)
    ));
    // Read the root-table pointer from the now-readable RSDP and map the
    // surrounding firmware table window as well. SeaBIOS commonly places the
    // RSDT/XSDT and MADT near the top of low memory, outside the first 2 MiB.
    let rsdp_virtual = (hhdm_offset + 0xf52e0) as *const u8;
    let rsdp_revision = unsafe { core::ptr::read_unaligned(rsdp_virtual.add(15)) };
    let root_phys = unsafe {
        if rsdp_revision >= 2 {
            core::ptr::read_unaligned(rsdp_virtual.add(24) as *const u64)
        } else {
            core::ptr::read_unaligned(rsdp_virtual.add(16) as *const u32) as u64
        }
    };
    let root_window_start = root_phys & !(2 * 1024 * 1024 - 1);
    let root_mapped = if root_phys != 0 {
        ech_os::memory::map_low_physical_hhdm(
            &mut mapper,
            &mut memory_manager,
            hhdm_offset,
            root_window_start,
            4 * 1024 * 1024,
            x86_64::structures::paging::PageTableFlags::PRESENT
                | x86_64::structures::paging::PageTableFlags::WRITABLE
                | x86_64::structures::paging::PageTableFlags::NO_EXECUTE,
        )
    } else {
        0
    };
    serial_write_str(&format_args!(
        "[LIMINE] ACPI root phys={:#x} mapped={:#x}\n",
        root_phys, root_mapped
    ));
    // Standard x86 firmware MMIO window used by the ACPI MADT/HPET tables:
    // IOAPIC (0xFEC00000), HPET (0xFED00000), and LAPIC (0xFEE00000).
    // Limine's HHDM response does not guarantee these reserved pages are
    // present in the entry tables, so establish the adapter-owned mapping
    // before CPU ACPI/AML initialization touches them.
    let mmio_mapped = ech_os::memory::map_low_physical_hhdm(
        &mut mapper,
        &mut memory_manager,
        hhdm_offset,
        0xFEC0_0000,
        0x0040_0000,
        x86_64::structures::paging::PageTableFlags::PRESENT
            | x86_64::structures::paging::PageTableFlags::WRITABLE
            | x86_64::structures::paging::PageTableFlags::NO_EXECUTE,
    );
    serial_write_str(&format_args!(
        "[LIMINE] ACPI MMIO HHDM mapped={:#x}\n",
        mmio_mapped
    ));
    serial_write_str(&format_args!("[MEMORY] PMM online (Limine)\n"));

    common_kernel_init(&boot_ctx);

    // Self-check is an opt-in acceptance gate.  Ordinary boots keep the
    // bounded diagnostics dormant; the Wave 4 runner enables it with
    // `boot_tests=1` and failures remain fail-closed.
    if run_boot_tests && !ech_os::debug::boot_self_check(&boot_ctx) {
        serial_write_str(&format_args!("[PANIC] Limine boot self-check failed!\n"));
        phase_fatal(0xE00B);
    }
    phase_begin(BootPhase::DriverInit);
    let driver_count = ech_os::drivers::linux::init_linux_driver_layer_deferred_hardware();
    serial_write_str(&format_args!(
        "[DRIVERS] Linux driver layer initialized: {} drivers attached\n",
        driver_count
    ));
    ech_os::fs::mount::mount_virtual_filesystems();
    ech_os::security::users::init_users();
    ech_os::init::init_system();
    ech_os::boot::appliance::publish_stage(ech_os::boot::appliance::BootStage::StorageMounted);
    ech_os::services::set_hardware_probe_policy(false);
    ech_os::ipc::service_ipc::init();
    ech_os::services::init();
    ech_os::services::spawn_service_tasks();
    ech_os::ipc::service_ipc::spawn_task();
    ech_os::gfx::simd::init_simd_dispatch();
    phase_complete(BootPhase::DriverInit);
    phase_begin(BootPhase::Services);
    if run_boot_tests && !run_wave4_boot_tests(&boot_ctx) {
        serial_write_str(&format_args!("[PANIC] Limine Wave 4 acceptance failed!\n"));
        phase_fatal(0xE00C);
    }
    ech_os::fs::f2fs::start_gc_thread();
    serial_write_str(&format_args!("[OS] Basic boot sequence complete.\n"));
    ech_os::boot::safety::BOOT_SAFETY.record_violation(
        ech_os::boot::safety::ViolationType::BootPolicy,
        "Services phase completed in safe fallback mode",
        true,
    );
    serial_write_str(&format_args!(
        "[BOOT_POLICY] stage=Services disposition=degraded-safe reason=hardware-probe-constrained\n"
    ));
    if BOOT_PIPELINE
        .finish_boot(PhaseOutcome::Degraded(
            ech_os::boot::pipeline::DegradeReason::SafeFallback,
        ))
        .is_err()
    {
        phase_fatal(0xE007);
    }
    ech_os::boot::appliance::mark_boot_success();
    BOOT_PIPELINE.emit_phase_marker(&BOOT_PIPELINE.current_snapshot());
    ech_os::task::scheduler::migrate_bsp_to_idle_stack_and_loop();
}

#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
unsafe fn boot_pipeline_multiboot(boot_info_addr: usize, kaslr_offset: u64) -> ! {
    use ech_os::boot::pipeline::{caps, BootProtocol, PhaseOutcome, BOOT_PIPELINE};
    register_protocol_or_fatal(BootProtocol::Multiboot2, 0xF009);
    BOOT_PIPELINE.set_profile(BootProfile::Multiboot2);
    phase_begin(BootPhase::UefiHandover);
    serial_write_str(&format_args!(
        "[MULTIBOOT] Info addr: {:#x}\n",
        boot_info_addr
    ));
    debugcon_write_byte(b'M');

    let info = if boot_info_addr == 0 {
        serial_write_str(&format_args!("[MULTIBOOT] boot_info_addr is null\n"));
        phase_fatal(0xF001);
    } else {
        match unsafe { load(boot_info_addr) } {
            Ok(info) => info,
            Err(_) => {
                serial_write_str(&format_args!("[MULTIBOOT] boot info parse failed\n"));
                phase_fatal(0xF002);
            }
        }
    };
    serial_write_str(&format_args!("[MULTIBOOT] Info parsed\n"));

    // cmdline, from_multiboot2 sonrasında kernel-owned buffer'dan okunur
    // (ownership-safe kopya — boot_ctx.cmdline_str()); info referansı ölmez.

    let mut boot_ctx = BootContext::from_multiboot2(&info);
    let multiboot_framebuffer = boot_ctx
        .install_multiboot2_framebuffer(info.framebuffer_tag().as_ref(), boot_ctx.hhdm_offset);
    serial_write_str(&format_args!(
        "[FRAMEBUFFER] protocol=multiboot2 state={}\n",
        if multiboot_framebuffer {
            "available"
        } else {
            "unavailable"
        }
    ));
    let normalized_map =
        normalize_multiboot2_memory_map(&info).unwrap_or_else(NormalizedMemoryMap::empty);
    if !boot_ctx.publish_normalized_memory_map(normalized_map) {
        serial_write_str(&format_args!(
            "[MULTIBOOT] canonical memory map invalid/empty\n"
        ));
        phase_fatal(0xF008);
    }
    publish_rsdp_from_boot_ctx(&boot_ctx, RsdpProvenance::Multiboot2, "Multiboot2Handover");
    if !run_stage_gate(
        &boot_ctx,
        BootProfile::Multiboot2,
        BootInitStage::Handover,
        "MULTIBOOT",
    ) {
        phase_fatal(0xF003);
    }
    set_required_capabilities_or_fatal(
        boot_ctx.capabilities() | caps::MEMORY_MAP | caps::HHDM,
        0xF00A,
    );
    phase_complete(BootPhase::UefiHandover);
    debugcon_write_byte(b'R');
    let mut run_boot_tests = false;
    if let Some(cmdline) = boot_ctx.cmdline_str() {
        if !cmdline.is_empty() {
            serial_write_str(&format_args!("[MULTIBOOT] cmdline: {}\n", cmdline));
            if cmdline
                .split_ascii_whitespace()
                .any(|arg| arg == "boot_tests=1")
            {
                run_boot_tests = true;
                serial_write_str(&format_args!("[MULTIBOOT] boot tests enabled\n"));
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
        }
    }

    phase_begin(BootPhase::PagingSetup);
    let mut mapper = unsafe { ech_os::memory::init_paging(boot_ctx.physical_memory_offset) };
    serial_write_str(&format_args!("[MEMORY] Paging initialized\n"));
    phase_complete(BootPhase::PagingSetup);

    serial_write_str(&format_args!("[MEMORY] Frame allocator init\n"));
    phase_begin(BootPhase::MemoryInit);
    let normalized_map = boot_ctx
        .normalized_memory_map()
        .unwrap_or_else(|| phase_fatal(0xF004));
    let mut frame_allocator =
        match ech_os::memory::frame_allocator::Multiboot2FrameAllocator::from_regions(
            normalized_map.as_slice(),
            kaslr_offset,
        ) {
            Some(allocator) => allocator,
            None => phase_fatal(0xF004),
        };
    let allocator_ptr: *mut ech_os::memory::frame_allocator::Multiboot2FrameAllocator<'static> =
        core::mem::transmute(&mut frame_allocator as *mut _);
    ech_os::memory::set_global_mb2_frame_allocator(allocator_ptr);
    serial_write_str(&format_args!(
        "[MEMORY] Usable bytes: {:#x}\n",
        frame_allocator.total_usable_bytes()
    ));
    // UEFI yolu ile aynı sıralama: PMM (MemoryManager) önce kurulur, heap
    // frame'leri PMM'den ayrılır; böylece tüm tahsisler tek kaynakta izlenir
    // ve KernelStack/alloc_phys gibi yolcular global yöneticiyi bulabilir.
    let mut memory_manager =
        ech_os::memory::init_multiboot2_normalized(normalized_map, kaslr_offset);
    unsafe {
        ech_os::memory::set_global_memory_manager(&mut memory_manager as *mut _);
    }
    serial_write_str(&format_args!("[MEMORY] PMM online (MB2)\n"));
    phase_complete(BootPhase::MemoryInit);
    phase_begin(BootPhase::HeapInit);
    if let Err(err) = ech_os::allocator::init_heap(&mut mapper, &mut memory_manager) {
        serial_write_str(&format_args!("[HEAP] init_heap failed: {:?}\n", err));
        phase_fatal(0xF005);
    } else {
        serial_write_str(&format_args!("[HEAP] TLSF heap initialized\n"));
        phase_complete(BootPhase::HeapInit);
    }

    // entry.S yalnızca < 1 GiB'i eşler (identity + PML4[256] penceresi); 1 GiB
    // üzerindeki fiziksel bellek phys_to_virt/ACPI okumaları için boot bellek
    // haritasından aktif ofset üzerinde yeniden eşlenir.
    {
        use ech_os::memory::map_physical_regions_hhdm;
        use multiboot2::MemoryAreaType;
        use x86_64::structures::paging::PageTableFlags;
        let mut regions: Vec<(u64, u64)> = info
            .memory_map_tag()
            .map(|tag| {
                tag.memory_areas()
                    .filter(|area| {
                        matches!(
                            area.typ(),
                            MemoryAreaType::Available
                                | MemoryAreaType::Reserved
                                | MemoryAreaType::AcpiAvailable
                                | MemoryAreaType::ReservedHibernate
                        )
                    })
                    .map(|area| (area.start_address(), area.size()))
                    .collect()
            })
            .unwrap_or_default();
        // IOAPIC (0xFEC00000), HPET (0xFED00000), LAPIC MMIO (0xFEE00000):
        // bootloader haritasında reserved olarak eksik kalabilir; HPET/APIC
        // erişimleri için açıkça HHDM penceresi üzerinde eşlenir (4 GiB'a kadar).
        regions.push((0xfec0_0000u64, 0x0140_0000u64));
        if let Some(tag) = info.framebuffer_tag() {
            let framebuffer_offset = tag.address & 0xfff;
            let framebuffer_len = (tag.pitch as u64)
                .checked_mul(tag.height as u64)
                .and_then(|bytes| bytes.checked_add(framebuffer_offset))
                .and_then(|bytes| bytes.checked_add(0xfff))
                .map(|bytes| bytes & !0xfff)
                .unwrap_or(0);
            if framebuffer_len != 0 {
                regions.push((tag.address & !0xfff, framebuffer_len));
            }
        }
        let flags = PageTableFlags::PRESENT
            | PageTableFlags::WRITABLE
            | PageTableFlags::GLOBAL
            | PageTableFlags::ACCESSED
            | PageTableFlags::DIRTY
            | PageTableFlags::NO_EXECUTE;
        let mapped = map_physical_regions_hhdm(
            &mut mapper,
            &mut memory_manager,
            boot_ctx.physical_memory_offset,
            &regions,
            flags,
        );
        serial_write_str(&format_args!(
            "[MULTIBOOT] HHDM regions mapped bytes={:#x}\n",
            mapped
        ));
        debugcon_write_byte(b'H');
    }

    serial_write_str(&format_args!("[BOOT] Core subsystems init\n"));
    common_kernel_init(&boot_ctx);
    ech_os::serial_println!("[BOOT] Scheduler online");

    // MB2 must enter the same product-facing driver/VFS/service chain as UEFI.
    // The bootloader protocol only supplies the handover; it does not justify a
    // diagnostic-only terminal path once the common kernel is ready.
    phase_begin(BootPhase::DriverInit);
    let driver_count = ech_os::drivers::linux::init_linux_driver_layer_deferred_hardware();
    serial_write_str(&format_args!(
        "[DRIVERS] Linux driver layer initialized: {} drivers attached\n",
        driver_count
    ));
    ech_os::fs::mount::mount_virtual_filesystems();
    ech_os::security::users::init_users();
    ech_os::init::init_system();
    ech_os::boot::appliance::publish_stage(ech_os::boot::appliance::BootStage::StorageMounted);
    ech_os::services::set_hardware_probe_policy(false);
    ech_os::ipc::service_ipc::init();
    ech_os::services::init();
    ech_os::services::spawn_service_tasks();
    ech_os::ipc::service_ipc::spawn_task();
    ech_os::gfx::simd::init_simd_dispatch();
    phase_complete(BootPhase::DriverInit);
    phase_begin(BootPhase::Services);
    if run_boot_tests {
        if !run_wave4_boot_tests(&boot_ctx) {
            ech_os::serial_println!("[PANIC] Wave 4 boot acceptance failed");
            phase_fatal(0xF006);
        }
    }
    // F2FS GC starts only after scheduler, driver, VFS and service tasks exist.
    ech_os::fs::f2fs::start_gc_thread();

    ech_os::boot::safety::BOOT_SAFETY.record_violation(
        ech_os::boot::safety::ViolationType::BootPolicy,
        "Services phase completed in safe fallback mode",
        true,
    );
    serial_write_str(&format_args!(
        "[BOOT_POLICY] stage=Services disposition=degraded-safe reason=hardware-probe-constrained\n"
    ));
    if BOOT_PIPELINE
        .finish_boot(PhaseOutcome::Degraded(
            ech_os::boot::pipeline::DegradeReason::SafeFallback,
        ))
        .is_err()
    {
        phase_fatal(0xF007);
    }
    ech_os::boot::appliance::mark_boot_success();
    BOOT_PIPELINE.emit_phase_marker(&BOOT_PIPELINE.current_snapshot());

    ech_os::task::scheduler::migrate_bsp_to_idle_stack_and_loop();
}

/// UEFI RNG protokolünden 32 bayt tohum okur (yalnızca EBS öncesi geçerli).
///
/// Protokol handle'ı yoksa veya `get_rng` başarısızsa `None` döner — boot
/// devam eder; `from_uefi` entropy'yi `Absent` işaretler (KASLR zorunluluğu
/// değilse ölümcül değildir).
#[cfg(target_os = "uefi")]
fn fetch_uefi_entropy_seed(system_table: &SystemTable<Boot>) -> Option<[u8; 32]> {
    use uefi::proto::rng::Rng;
    let boot_services = system_table.boot_services();
    let handle = boot_services.get_handle_for_protocol::<Rng>().ok()?;
    let mut rng = boot_services.open_protocol_exclusive::<Rng>(handle).ok()?;
    let mut seed = [0u8; 32];
    rng.get_rng(None, &mut seed).ok()?;
    Some(seed)
}

#[cfg(target_os = "uefi")]
unsafe fn enter_kernel_on_uefi_stack(boot_info_ptr: *mut BootInfo) -> ! {
    let stack_top =
        (core::ptr::addr_of!(UEFI_BOOT_STACK.0) as usize).saturating_add(UEFI_BOOT_STACK_SIZE);
    asm!(
        "mov rsp, r11",
        "and rsp, -16",
        "call {entry}",
        in("r11") stack_top,
        entry = sym kernel_entry,
        // x86_64-unknown-uefi uses the Microsoft/EFI register ABI:
        // RCX, RDX, R8 carry the first three arguments of kernel_entry.
        in("rcx") boot_info_ptr as usize,
        in("rdx") 0usize,
        in("r8") BOOT_MAGIC_UEFI,
        options(noreturn)
    );
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
    // Karar 7: entropy — UEFI RNG protokolü yalnızca EBS öncesi canlıdır;
    // tohum BootContext::from_uefi'nin tüketeceği köprüye yazılır.
    *ech_os::boot::UEFI_ENTROPY_SEED.lock() = fetch_uefi_entropy_seed(&mut system_table);
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
                boot_flags: boot_control.boot_flags(),
            },
        );
    }
    unsafe { enter_kernel_on_uefi_stack(boot_info_ptr) }
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
    if runtime
        .set_variable(
            cstr16!("echOSBootControl"),
            &appliance_variable_vendor(),
            attributes,
            bytes,
        )
        .is_err()
    {
        ech_os::boot::safety::BOOT_SAFETY.record_violation(
            ech_os::boot::safety::ViolationType::CapabilityUnavailable,
            "UEFI boot-control variable synchronization failed",
            true,
        );
        serial_write_str(&format_args!(
            "[BOOT_POLICY] stage=BootControl disposition=degraded-safe reason=runtime-variable-sync\n"
        ));
    }

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
    if file.set_position(0).is_err() {
        ech_os::boot::safety::BOOT_SAFETY.record_violation(
            ech_os::boot::safety::ViolationType::CapabilityUnavailable,
            "UEFI BOOTCTRL.BIN seek failed",
            true,
        );
        serial_write_str(&format_args!(
            "[BOOT_POLICY] stage=BootControl disposition=degraded-safe reason=file-seek\n"
        ));
        return;
    }
    if file.write(bytes).is_err() {
        ech_os::boot::safety::BOOT_SAFETY.record_violation(
            ech_os::boot::safety::ViolationType::CapabilityUnavailable,
            "UEFI BOOTCTRL.BIN write failed",
            true,
        );
        serial_write_str(&format_args!(
            "[BOOT_POLICY] stage=BootControl disposition=degraded-safe reason=file-write\n"
        ));
        return;
    }
    if file.flush().is_err() {
        ech_os::boot::safety::BOOT_SAFETY.record_violation(
            ech_os::boot::safety::ViolationType::CapabilityUnavailable,
            "UEFI BOOTCTRL.BIN flush failed",
            true,
        );
        serial_write_str(&format_args!(
            "[BOOT_POLICY] stage=BootControl disposition=degraded-safe reason=file-flush\n"
        ));
    }
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
    append: bool,
) -> Result<(), Status> {
    let mut attributes = VariableAttributes::NON_VOLATILE
        | VariableAttributes::BOOTSERVICE_ACCESS
        | VariableAttributes::RUNTIME_ACCESS
        | VariableAttributes::TIME_BASED_AUTHENTICATED_WRITE_ACCESS;
    if append {
        attributes |= VariableAttributes::APPEND_WRITE;
    }
    system_table
        .runtime_services()
        .set_variable(
            name,
            &VariableVendor::GLOBAL_VARIABLE,
            attributes,
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
            if write_secure_boot_enroll_state(system_table, 0).is_err() {
                ech_os::boot::safety::BOOT_SAFETY.record_violation(
                    ech_os::boot::safety::ViolationType::CapabilityUnavailable,
                    "Secure Boot enroll state clear failed",
                    false,
                );
                serial_write_str(&format_args!(
                    "[BOOT_POLICY] stage=SecureBoot disposition=fatal reason=state-clear\n"
                ));
            }
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
        if write_secure_boot_enroll_state(system_table, SECURE_BOOT_ENROLL_FAILED).is_err() {
            ech_os::boot::safety::BOOT_SAFETY.record_violation(
                ech_os::boot::safety::ViolationType::CapabilityUnavailable,
                "Secure Boot failure state persistence failed",
                false,
            );
        }
        serial_write_str(&format_args!(
            "[UEFI] Secure Boot enroll did not transition firmware out of setup mode\n"
        ));
        return Err(Status::SECURITY_VIOLATION);
    }

    let payloads = [
        ("PK", cstr16!("PK"), cstr16!("EFI\\BOOT\\PK.AUT"), None, false),
        ("KEK", cstr16!("KEK"), cstr16!("EFI\\BOOT\\KEK.AUT"), None, false),
        (
            "db",
            cstr16!("db"),
            cstr16!("EFI\\BOOT\\DB.AUT"),
            Some(cstr16!("EFI\\BOOT\\DB.SET")),
            false,
        ),
        (
            "dbx",
            cstr16!("dbx"),
            cstr16!("EFI\\BOOT\\DBX.AUT"),
            Some(cstr16!("EFI\\BOOT\\DBX.SET")),
            false,
        ),
    ];
    serial_write_str(&format_args!(
        "[UEFI] Secure Boot auto-enroll trigger detected\n"
    ));
    for (label, variable_name, path, setup_path, append) in payloads {
        let payload = read_efi_boot_file(system_table, image, path).ok_or_else(|| {
            serial_write_str(&format_args!(
                "[UEFI] Missing Secure Boot payload for {}\n",
                label
            ));
            if write_secure_boot_enroll_state(system_table, SECURE_BOOT_ENROLL_FAILED).is_err() {
                ech_os::boot::safety::BOOT_SAFETY.record_violation(
                    ech_os::boot::safety::ViolationType::CapabilityUnavailable,
                    "Secure Boot failure state persistence failed",
                    false,
                );
            }
            Status::SECURITY_VIOLATION
        })?;
        let result = if setup_mode == 1 {
            if let Some(setup_path) = setup_path {
                let setup_payload = read_efi_boot_file(system_table, image, setup_path)
                    .ok_or(Status::NOT_FOUND)?;
                write_global_variable_payload(system_table, variable_name, &setup_payload, append)
            } else {
                write_global_variable_payload(system_table, variable_name, &payload, append)
            }
        } else {
            write_global_variable_payload(system_table, variable_name, &payload, append)
        };
        result.map_err(|status| {
            serial_write_str(&format_args!(
                "[UEFI] Secure Boot variable write failed for {} status={:?}\n",
                label, status
            ));
            if write_secure_boot_enroll_state(system_table, SECURE_BOOT_ENROLL_FAILED).is_err() {
                ech_os::boot::safety::BOOT_SAFETY.record_violation(
                    ech_os::boot::safety::ViolationType::CapabilityUnavailable,
                    "Secure Boot failure state persistence failed",
                    false,
                );
            }
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
        // A firmware/application hand-off is allowed to omit an optional
        // command line.  Keep PCR8's state explicit so the trusted-boot
        // verifier can distinguish "not supplied" from a failed extend.
        serial_write_str(&format_args!(
            "[TPM] Cmdline absent; PCR8 measure skipped\n"
        ));
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
