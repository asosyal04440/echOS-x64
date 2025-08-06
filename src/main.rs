#![no_main]
#![no_std]

use core::arch::asm;
use uefi::prelude::*;

mod gdt;
mod pagging;
mod vga_buffer;


#[unsafe(no_mangle)]
extern "C" fn kernel_main() -> ! {
    vga_buffer::clear();
    vga_buffer::write_str("64-bit kernel online!\n> ");
    loop {
        unsafe { asm!("hlt"); }
    }
}

#[entry]
fn efi_main() -> Status {
    // GDT + paging
    gdt::GdtTable::new().load();
    unsafe { pagging::setup_paging(); }

    // kernel_main’e geç
    kernel_main();
}

