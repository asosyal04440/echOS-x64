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
#[cfg(not(target_os = "uefi"))]
use core::arch::global_asm;
use core::arch::x86_64::_rdtsc;
use core::fmt::{self, Write};
#[cfg(target_os = "uefi")]
use core::mem::MaybeUninit;
#[cfg(target_os = "uefi")]
use ech_os::boot::BootInfo;
#[cfg(target_os = "uefi")]
use ech_os::gop::framebuffer::Framebuffer;
#[cfg(target_os = "uefi")]
use ech_os::splash::Splash;
#[cfg(not(target_os = "uefi"))]
use ech_os::memory::frame_allocator::{LimineFrameAllocator, LimineMemmapEntry};
#[cfg(not(target_os = "uefi"))]
use limine_protocol_for_rust::{
    requests::executable_cmdline::ExecutableCmdlineRequest,
    requests::hhdm::HigherHalfDirectMapRequest,
    requests::memory_map::{MemoryMapRequest, MemoryRegionInfo, MemoryRegionType},
    requests::LimineRequest,
    use_base_revision,
    util::PointerSlice,
    REQUEST_END_MARKER, REQUEST_START_MARKER,
};
#[cfg(not(target_os = "uefi"))]
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
use uefi::proto::tcg::v2::{HashLogExtendEventFlags, PcrEventInputs, Tcg};
#[cfg(target_os = "uefi")]
use uefi::proto::tcg::{EventType, PcrIndex};
#[cfg(target_os = "uefi")]
use uefi::table::boot::MemoryType;
#[cfg(target_os = "uefi")]
use uefi::table::runtime::VariableVendor;
#[cfg(target_os = "uefi")]
use uefi::CStr16;

const COM1: u16 = 0x3F8;
const BOOT_MAGIC_UEFI: u64 = 0x55454649;
const BOOT_MAGIC_MB2: u64 = 0x36d76289;
#[cfg(target_os = "uefi")]
const CMDLINE_MAX_LEN: usize = 4096;
#[cfg(not(target_os = "uefi"))]
const LIMINE_REVISION: u64 = 4;

#[cfg(not(target_os = "uefi"))]
#[used]
#[link_section = ".limine_reqs"]
static LIMINE_BASE_REVISION: [u64; 4] = use_base_revision(LIMINE_REVISION);

#[cfg(not(target_os = "uefi"))]
#[used]
#[link_section = ".limine_req_start"]
static LIMINE_REQUEST_START_MARKER: [u64; 4] = REQUEST_START_MARKER;

#[cfg(not(target_os = "uefi"))]
#[used]
#[link_section = ".limine_reqs"]
static LIMINE_MEMORY_MAP_REQUEST: MemoryMapRequest = MemoryMapRequest::new(LIMINE_REVISION);

#[cfg(not(target_os = "uefi"))]
#[used]
#[link_section = ".limine_reqs"]
static LIMINE_HHDM_REQUEST: HigherHalfDirectMapRequest =
    HigherHalfDirectMapRequest::new(LIMINE_REVISION);

#[cfg(not(target_os = "uefi"))]
#[used]
#[link_section = ".limine_reqs"]
static LIMINE_CMDLINE_REQUEST: ExecutableCmdlineRequest =
    ExecutableCmdlineRequest::new(LIMINE_REVISION);

#[cfg(not(target_os = "uefi"))]
#[used]
#[link_section = ".limine_req_end"]
static LIMINE_REQUEST_END_MARKER: [u64; 2] = REQUEST_END_MARKER;

#[cfg(not(target_os = "uefi"))]
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

fn serial_write_byte(byte: u8) {
    unsafe {
        let mut spins = 1_000_000u32;
        while (inb(COM1 + 5) & 0x20) == 0 {
            if spins == 0 { break; }
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
            serial_write_byte(byte);
        }
        Ok(())
    }
}

fn serial_write_str(args: &fmt::Arguments) {
    let mut port = SerialPort;
    let _ = port.write_fmt(*args);
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
    loop {
        unsafe {
            asm!("hlt");
        }
    }
}

#[no_mangle]
pub extern "C" fn kernel_entry(boot_info_addr: usize, kaslr_offset: u64, boot_magic: u64) -> ! {
    unsafe {
        debugcon_write_byte(b'k');
    }
    kernel_main(boot_info_addr, kaslr_offset, boot_magic)
}

#[no_mangle]
pub extern "C" fn kernel_main(boot_info_addr: usize, kaslr_offset: u64, boot_magic: u64) -> ! {
    unsafe {
        debugcon_write_byte(b'K');
    }
    unsafe {
        serial_init();
        debugcon_write_byte(b'S');
    }
    unsafe { debugcon_write_byte(b'M'); }  // Mark: after serial_init
    ech_os::memory::set_kaslr_offset(kaslr_offset);
    let mut seed = unsafe { _rdtsc() };
    seed ^= boot_info_addr as u64;
    seed ^= kaslr_offset;
    seed ^= boot_magic;
    seed ^= seed >> 32;
    ech_os::random::init(seed as u32);
    unsafe { debugcon_write_byte(b'R'); }  // Mark: after random init
    serial_write_str(&format_args!("[KASLR] Offset: {:#x}\n", kaslr_offset));
    serial_write_str(&format_args!("[BOOT] Magic: {:#x}\n", boot_magic));
    unsafe { debugcon_write_byte(b'B'); debugcon_write_hex(boot_magic); }  // Mark: boot magic value

    #[cfg(not(target_os = "uefi"))]
    if limine_available() {
        unsafe { boot_pipeline_limine(kaslr_offset) };
    }

    if boot_magic == BOOT_MAGIC_UEFI {
        #[cfg(target_os = "uefi")]
        unsafe {
            debugcon_write_byte(b'U');  // Mark: entering UEFI pipeline
            boot_pipeline_uefi(boot_info_addr, kaslr_offset);
        }
        #[cfg(not(target_os = "uefi"))]
        {
            serial_write_str(&format_args!(
                "[BOOT] UEFI magic on non-UEFI target, halting\n"
            ));
        }
    } else if boot_magic == BOOT_MAGIC_MB2 {
        #[cfg(not(target_os = "uefi"))]
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

#[cfg(not(target_os = "uefi"))]
fn limine_available() -> bool {
    LIMINE_MEMORY_MAP_REQUEST.get_response().is_some()
}

#[cfg(target_os = "uefi")]
unsafe fn boot_pipeline_uefi(boot_info_addr: usize, _kaslr_offset: u64) -> ! {
    debugcon_write_byte(b'1');  // Mark: entered boot_pipeline_uefi
    // Initialize boot safety system FIRST
    ech_os::boot::safety::init();
    debugcon_write_byte(b'2');  // Mark: after safety init
    ech_os::boot::safety::BOOT_SAFETY.enter_phase(ech_os::boot::safety::BootPhase::UefiHandover);
    debugcon_write_byte(b'3');  // Mark: after enter_phase
    
    let boot_info = &mut *(boot_info_addr as *mut BootInfo);
    debugcon_write_byte(b'4');  // Mark: after boot_info cast
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
        debugcon_write_byte(b'!');  // Mark: BootInfo check failed
        ech_os::boot::safety::BOOT_SAFETY.record_violation(
            ech_os::boot::safety::ViolationType::AcpiTableInvalid,
            "BootInfo ABI mismatch",
            false
        );
        serial_write_str(&format_args!(
            "[UEFI] BootInfo ABI mismatch magic={:#x} ver={} size={}\n",
            boot_info.magic, boot_info.version, boot_info.size
        ));
        loop {
            asm!("hlt");
        }
    }
    debugcon_write_byte(b'5');  // Mark: passed BootInfo checks
    if boot_info.physical_memory_offset == 0 || boot_info.hhdm_offset == 0 {
        debugcon_write_byte(b'Z');  // Mark: zero offset
        ech_os::boot::safety::BOOT_SAFETY.record_violation(
            ech_os::boot::safety::ViolationType::MemoryMapInvalid,
            "Invalid memory offsets",
            false
        );
        serial_write_str(&format_args!(
            "[UEFI] Invalid memory offsets phys={:#x} hhdm={:#x}\n",
            boot_info.physical_memory_offset, boot_info.hhdm_offset
        ));
        loop {
            asm!("hlt");
        }
    }
    debugcon_write_byte(b'6');  // Mark: passed offset checks
    if boot_info.secure_boot && boot_info.system_table == 0 {
        serial_write_str(&format_args!("[UEFI] Secure Boot requires system table\n"));
        loop {
            asm!("hlt");
        }
    }
    debugcon_write_byte(b'7');  // Mark: passed secure boot check
    ech_os::boot::set_secure_boot(boot_info.secure_boot);
    ech_os::cpu::acpi::set_uefi_rsdp_address(boot_info.rsdp_address);
    ech_os::acpi::set_rsdp_address(boot_info.rsdp_address);
    debugcon_write_byte(b'8');  // Mark: after RSDP setup
    serial_write_str(&format_args!(
        "[UEFI] RSDP: {:#x}\n",
        boot_info.rsdp_address
    ));
    debugcon_write_byte(b'9');  // Mark: after serial write

    if let Some(framebuffer) = boot_info.framebuffer.as_ref() {
        serial_write_str(&format_args!(
            "[UEFI] FB base={:#x} {}x{} stride={}\n",
            framebuffer.base_addr,
            framebuffer.width,
            framebuffer.height,
            framebuffer.pixels_per_scan_line
        ));
    }
    debugcon_write_byte(b'A');  // Mark: after framebuffer check
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
    debugcon_write_byte(b'B');  // Mark: after image hash

    let _boot_ctx = ech_os::KernelBootContext {
        physical_memory_offset: boot_info.physical_memory_offset as u64,
    };
    debugcon_write_byte(b'C');  // Mark: after boot context

    let memory_map_present = boot_info
        .memory_map
        .as_ref()
        .map(|map| map.entries().next().is_some())
        .unwrap_or(false);
    debugcon_write_byte(if memory_map_present { b'Y' } else { b'N' });  // Mark: memory map present?
    debugcon_write_byte(b'D');  // Mark: after memory_map_present check
    if !memory_map_present {
        debugcon_write_byte(b'!');  // Mark: memory map empty
        ech_os::boot::safety::BOOT_SAFETY.record_violation(
            ech_os::boot::safety::ViolationType::MemoryMapInvalid,
            "Empty memory map",
            false
        );
        serial_write_str(&format_args!("[UEFI] Empty memory map\n"));
        loop {
            asm!("hlt");
        }
    }
    debugcon_write_byte(b'E');  // Mark: passed memory map check
    if let Some(map) = boot_info.memory_map.as_ref() {
        let total_pages: u64 = map.entries().map(|d| d.page_count).sum();
        let total_mb = total_pages.saturating_mul(4096) / (1024 * 1024);
        serial_write_str(&format_args!("[UEFI] Memory map total: {} MB\n", total_mb));
    }
    debugcon_write_byte(b'F');  // Mark: after memory map total
    if let (Some(framebuffer), Some(screen)) =
        (boot_info.framebuffer.as_mut(), splash.as_mut())
    {
        screen.update_progress(framebuffer, 15);
    }

    debugcon_write_byte(b'G');  // Mark: before init_paging
    serial_write_str(&format_args!("[UEFI] init_paging\n"));
    ech_os::boot::safety::BOOT_SAFETY.enter_phase(ech_os::boot::safety::BootPhase::PagingSetup);
    let mut mapper = unsafe { ech_os::memory::init_paging(0) };
    debugcon_write_byte(b'H');  // Mark: after init_paging
    serial_write_str(&format_args!("[UEFI] init_uefi memory manager\n"));
    ech_os::boot::safety::BOOT_SAFETY.enter_phase(ech_os::boot::safety::BootPhase::MemoryInit);
    debugcon_write_byte(b'I');  // Mark: before memory_map.take
    let memory_map = boot_info
        .memory_map
        .take()
        .expect("[UEFI] memory map already consumed");
    debugcon_write_byte(b'J');  // Mark: after memory_map.take
    let mut memory_manager = ech_os::memory::init_uefi(memory_map);
    debugcon_write_byte(b'K');  // Mark: after init_uefi
    unsafe {
        ech_os::memory::set_global_memory_manager(&mut memory_manager as *mut _);
    }
    debugcon_write_byte(b'L');  // Mark: after set_global_memory_manager
    serial_write_str(&format_args!("[UEFI] init_uefi_hhdm\n"));
    if let Err(err) =
        ech_os::memory::init_uefi_hhdm(&mut mapper, &mut memory_manager, boot_info.hhdm_offset)
    {
        debugcon_write_byte(b'X');  // Mark: init_uefi_hhdm failed
        serial_write_str(&format_args!("[HHDM] init failed: {:?}\n", err));
    } else {
        debugcon_write_byte(b'M');  // Mark: init_uefi_hhdm success
        ech_os::memory::set_active_physical_offset(boot_info.hhdm_offset);
        mapper = unsafe { ech_os::memory::init_paging(boot_info.hhdm_offset) };
    }
    debugcon_write_byte(b'N');  // Mark: after hhdm setup
    if let Some(framebuffer) = boot_info.framebuffer.as_mut() {
        debugcon_write_byte(b'P');  // Mark: framebuffer present
        let size = framebuffer
            .pixels_per_scan_line
            .saturating_mul(framebuffer.height)
            .saturating_mul(4);
        debugcon_write_byte(b'a');  // Mark: before map_mmio
        let mapped = ech_os::memory::map_mmio(framebuffer.base_addr as u64, size);
        debugcon_write_byte(b'b');  // Mark: after map_mmio
        if !mapped.is_null() {
            framebuffer.base_addr = mapped as usize;
        } else {
            framebuffer.base_addr =
                (boot_info.hhdm_offset + framebuffer.base_addr as u64) as usize;
        }
        debugcon_write_byte(b'c');  // Mark: before Splash::new
        let mut screen = Splash::new(framebuffer);
        debugcon_write_byte(b'd');  // Mark: after Splash::new
        screen.update_progress(framebuffer, 5);
        splash = Some(screen);
    } else {
        debugcon_write_byte(b'p');  // Mark: no framebuffer
    }
    debugcon_write_byte(b'Q');  // Mark: after framebuffer setup
    if let (Some(framebuffer), Some(screen)) =
        (boot_info.framebuffer.as_mut(), splash.as_mut())
    {
        screen.update_progress(framebuffer, 30);
    }
    debugcon_write_byte(b'R');  // Mark: before cmdline
    if boot_info.cmdline_len > 0 && boot_info.cmdline_ptr != 0 {
        debugcon_write_byte(b'S');  // Mark: cmdline present
        if boot_info.cmdline_len > isize::MAX as u64 {
            serial_write_str(&format_args!("[UEFI] cmdline too large\n"));
        } else {
            let cmdline_ptr = ech_os::memory::phys_to_virt(boot_info.cmdline_ptr as usize)
                as *const u8;
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
    debugcon_write_byte(b'T');  // Mark: before set_virtual_address_map
    serial_write_str(&format_args!("[UEFI] set_virtual_address_map\n"));
    if boot_info.system_table != 0 {
        debugcon_write_byte(b'V');  // Mark: system_table present
        match ech_os::memory::set_uefi_virtual_address_map(
            boot_info.system_table,
            &mut memory_manager,
            boot_info.hhdm_offset,
        ) {
            Ok(runtime_services) => {
                debugcon_write_byte(b'W');  // Mark: set_virtual_address_map OK
                ech_os::boot::set_runtime_services(runtime_services);
                serial_write_str(&format_args!("[UEFI] Runtime services remapped\n"));
                match ech_os::boot::verify_uefi_runtime_services() {
                    Ok(()) => {
                        debugcon_write_byte(b'X');  // Mark: runtime services verified
                        serial_write_str(&format_args!("[UEFI] Runtime services verified\n"));
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
    debugcon_write_byte(b'Y');  // Mark: after virtual address map
    if let (Some(framebuffer), Some(screen)) =
        (boot_info.framebuffer.as_mut(), splash.as_mut())
    {
        screen.update_progress(framebuffer, 45);
    }
    debugcon_write_byte(b'Z');  // Mark: before heap init
    if let Err(err) = ech_os::allocator::init_heap(&mut mapper, &mut memory_manager) {
        debugcon_write_byte(b'!');  // Mark: heap init failed
        serial_write_str(&format_args!("[HEAP] init_heap failed: {:?}\n", err));
    } else {
        debugcon_write_byte(b'H');  // Mark: heap init OK
        serial_write_str(&format_args!("[HEAP] TLSF heap initialized\n"));
    }
    debugcon_write_byte(b'I');  // Mark: after heap init
    if let (Some(framebuffer), Some(screen)) =
        (boot_info.framebuffer.as_mut(), splash.as_mut())
    {
        screen.update_progress(framebuffer, 60);
    }

    debugcon_write_byte(b'J');  // Mark: before gdt::init
    ech_os::gdt::init();
    debugcon_write_byte(b'K');  // Mark: after gdt::init
    ech_os::boot::safety::BOOT_SAFETY.enter_phase(ech_os::boot::safety::BootPhase::GdtSetup);
    debugcon_write_byte(b'L');  // Mark: before cpu::init
    ech_os::cpu::init();
    debugcon_write_byte(b'M');  // Mark: after cpu::init
    debugcon_write_byte(b's');  // Mark: before security::init
    ech_os::security::init();
    debugcon_write_byte(b'n');  // Mark: after security::init
    debugcon_write_byte(b'N');  // Mark: after security::init
    ech_os::interrupts::init();
    serial_write_str(&format_args!("[INT] Interrupts initialized\n"));
    debugcon_write_byte(b'O');  // Mark: after interrupts::init
    ech_os::boot::safety::BOOT_SAFETY.enter_phase(ech_os::boot::safety::BootPhase::IdtSetup);
    ech_os::vdso::init();
    // TTY alt sistemini başlat - klavye interrupt'ları öncesinde!
    ech_os::tty::init();
    
    // VirtIO-Net driver'ı başlat
    if ech_os::drivers::virtio_net::auto_init() {
        serial_write_str(&format_args!("[NET] VirtIO-Net driver initialized\n"));
    } else {
        serial_write_str(&format_args!("[NET] VirtIO-Net driver not found or init failed\n"));
    }
    
    if let (Some(framebuffer), Some(screen)) =
        (boot_info.framebuffer.as_mut(), splash.as_mut())
    {
        screen.update_progress(framebuffer, 75);
    }

    let cpu_acpi_ok = ech_os::cpu::acpi::init();
    ech_os::boot::safety::BOOT_SAFETY.enter_phase(ech_os::boot::safety::BootPhase::AcpiInit);
    if cpu_acpi_ok {
        serial_write_str(&format_args!("[SMP] CPU ACPI tables parsed\n"));
    } else {
        serial_write_str(&format_args!(
            "[SMP] CPU ACPI init failed, using CPUID topology\n"
        ));
    }
    let iommu_ok = ech_os::memory::init_iommu();
    if iommu_ok {
        serial_write_str(&format_args!(
            "[IOMMU] DMAR parsed and domains initialized\n"
        ));
    } else {
        serial_write_str(&format_args!("[IOMMU] DMAR not available\n"));
    }

    // CRITICAL: Scheduler ve Workers SMP'den ÖNCE init edilmeli!
    // AP'ler başlatıldığında scheduler kullanıma hazır olmalı
    ech_os::task::scheduler::init();
    ech_os::interrupts::kick_irq_worker();
    ech_os::task::worker::init_workers(4);
    ech_os::boot::safety::BOOT_SAFETY.enter_phase(ech_os::boot::safety::BootPhase::SmpInit);
    // TODO: SMP temporarily disabled for debugging
    // ech_os::cpu::smp::init();
    serial_write_str(&format_args!("[SMP] Skipped for debugging\n"));
    ech_os::boot::safety::BOOT_SAFETY.enter_phase(ech_os::boot::safety::BootPhase::DriverInit);
    x86_64::instructions::interrupts::enable();
    serial_write_str(&format_args!("[INT] Interrupts enabled\n"));
    serial_write_str(&format_args!("[WINSRV] ownership check enabled\n"));
    serial_write_str(&format_args!("[WINSRV] user-range validation enabled\n"));
    serial_write_str(&format_args!("[PERF] latency probes armed (irq + compositor)\n"));
    serial_write_str(&format_args!("[IRONSHIM] fuzz guard active\n"));
    serial_write_str(&format_args!("[IRONSHIM] ring3->ring0 blocked policy active\n"));

    // Global framebuffer'ı kaydet - shell için
    if let Some(fb) = boot_info.framebuffer.as_ref() {
        ech_os::boot::set_global_framebuffer(*fb);
    }

    // Shell yerine yeni compositor tabanlı GUI'yi başlat
    serial_write_str(&format_args!("[BOOT] Starting GUI compositor...\n"));
    if let Some(fb) = boot_info.framebuffer.as_mut() {
        ech_os::gfx::compositor::run(fb);
    } else {
        serial_write_str(&format_args!("[BOOT] No framebuffer, starting shell...\n"));
        ech_os::shell::run_shell();
    }

    // // ech_os::debug::init_telemetry();
    
    // // Smoke tests
    // // let self_ok = ech_os::debug::boot_self_check();
    let self_ok = true;
    if !self_ok {
        serial_write_str(&format_args!("[PANIC] Self-check failed!\n"));
        loop {
            unsafe { asm!("hlt") };
        }
    }

    serial_write_str(&format_args!("[OS] Basic boot sequence complete.\n"));
    ech_os::boot::safety::BootWatchdog::complete();
    ech_os::boot::safety::BOOT_SAFETY.enter_phase(ech_os::boot::safety::BootPhase::UserspaceReady);
    
    // Report boot safety status
    let report = ech_os::boot::safety::get_report();
    serial_write_str(&format_args!(
        "[BOOT_SAFETY] Complete - violations: {}, heap_corruptions: {}, smp_failures: {}\n",
        report.violation_count, report.heap_corruptions, report.smp_failures
    ));
    
    // Initialize fault management system
    ech_os::fault::init();
    serial_write_str(&format_args!("[FAULT] Anti-crash system initialized\n"));
    
    // // Gelişmiş testler
    // // ech_os::debug::run_ring3_smoketest();
    
    // // Stress testleri (sadece DEBUG mode'da ve istenirse)
    #[cfg(feature = "stress_test")]
    {
        // ech_os::debug::run_vm_security_tests();
        // ech_os::debug::run_vm_stress_tests();
        // ech_os::debug::run_irq_stress_tests();
        // ech_os::debug::run_long_stability_checks();
    }

    ech_os::task::scheduler::idle_loop();
}

#[cfg(not(target_os = "uefi"))]
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
    ech_os::cpu::init();
    ech_os::security::init();
    ech_os::interrupts::init();
    ech_os::vdso::init();
    // TTY alt sistemini başlat - klavye interrupt'ları öncesinde!
    ech_os::tty::init();

    let cpu_acpi_ok = ech_os::cpu::acpi::init();
    if cpu_acpi_ok {
        serial_write_str(&format_args!("[SMP] CPU ACPI tables parsed\n"));
    } else {
        serial_write_str(&format_args!(
            "[SMP] CPU ACPI init failed, using CPUID topology\n"
        ));
    }
    let iommu_ok = ech_os::memory::init_iommu();
    if iommu_ok {
        serial_write_str(&format_args!(
            "[IOMMU] DMAR parsed and domains initialized\n"
        ));
    } else {
        serial_write_str(&format_args!("[IOMMU] DMAR not available\n"));
    }

    // CRITICAL: Scheduler ve Workers SMP'den ÖNCE init edilmeli!
    ech_os::task::scheduler::init();
    ech_os::interrupts::kick_irq_worker();
    ech_os::task::worker::init_workers(4);
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

#[cfg(not(target_os = "uefi"))]
unsafe fn boot_pipeline_multiboot(boot_info_addr: usize, kaslr_offset: u64) -> ! {
    serial_write_str(&format_args!(
        "[MULTIBOOT] Info addr: {:#x}\n",
        boot_info_addr
    ));
    debugcon_write_byte(b'M');

    let info = if boot_info_addr != 0 {
        unsafe { load(boot_info_addr).unwrap() }
    } else {
        panic!("[MULTIBOOT] boot_info_addr is null");
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
    ech_os::cpu::init();
    ech_os::security::init();
    ech_os::interrupts::init();
    // TTY alt sistemini başlat - klavye interrupt'ları öncesinde!
    ech_os::tty::init();

    let cpu_acpi_ok = ech_os::cpu::acpi::init();
    if cpu_acpi_ok {
        serial_write_str(&format_args!("[SMP] CPU ACPI tables parsed\n"));
    } else {
        serial_write_str(&format_args!(
            "[SMP] CPU ACPI init failed, using CPUID topology\n"
        ));
    }

    // CRITICAL: Scheduler ve Workers SMP'den ÖNCE init edilmeli!
    ech_os::task::scheduler::init();
    ech_os::interrupts::kick_irq_worker();
    ech_os::task::worker::init_workers(4);
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
pub extern "efiapi" fn efi_main(
    image: Handle,
    mut system_table: SystemTable<Boot>,
) -> Status {
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
        gop.as_mut().map(|gop| Framebuffer::new(gop))
    };
    
    serial_write_str(&format_args!("[UEFI] Finding ACPI table...\n"));
    let rsdp_address =
        ech_os::acpi::find_acpi_table(system_table.config_table()).unwrap_or(0) as u64;
        
    serial_write_str(&format_args!("[UEFI] Detecting secure boot...\n"));
    let secure_boot = detect_secure_boot(&system_table);
    
    serial_write_str(&format_args!("[UEFI] Inspecting loaded image...\n"));
    let (image_hash, image_size) =
        match inspect_loaded_image(&mut system_table, image, secure_boot) {
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
    report_tpm_event_log(&mut system_table);
    let boot_info_ptr = match system_table
        .boot_services()
        .allocate_pool(MemoryType::LOADER_DATA, core::mem::size_of::<BootInfo>())
    {
        Ok(ptr) => ptr as *mut BootInfo,
        Err(_) => return Status::OUT_OF_RESOURCES,
    };
    let (runtime_table, memory_map) = system_table.exit_boot_services();
    let runtime_services =
        unsafe { runtime_table.runtime_services() as *const _ as usize };
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
