//! hello_world - echOS Kullanıcı Alanı Temel Doğrulama Testi
//!
//! Bu dosya; echOS'un kullanıcı alanı çalışma ortamını doğrulamak amacıyla
//! yazılmış en minimal ELF ikili dosyasıdır. Başarıyla çalışması; sistem çağrısı
//! altyapısının, kullanıcı alanı bellek eşlemesinin ve ELF yükleyicisinin
//! düzgün çalıştığını teyit eder.

#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;

const SYS_WRITE: usize = 1;
const SYS_EXIT: usize = 60;

unsafe fn syscall3(n: usize, a: usize, b: usize, c: usize) -> isize {
    let ret: isize;
    asm!(
        "syscall",
        inlateout("rax") n as isize => ret,
        in("rdi") a as isize,
        in("rsi") b as isize,
        in("rdx") c as isize,
        lateout("rcx") _,
        lateout("r11") _,
    );
    ret
}

unsafe fn syscall1(n: usize, a: usize) -> isize {
    let ret: isize;
    asm!(
        "syscall",
        inlateout("rax") n as isize => ret,
        in("rdi") a as isize,
        lateout("rcx") _,
        lateout("r11") _,
    );
    ret
}

fn write_all(bytes: &[u8]) {
    unsafe {
        let _ = syscall3(SYS_WRITE, 1, bytes.as_ptr() as usize, bytes.len());
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    write_all(b"hello_world.elf OK\n");
    unsafe {
        let _ = syscall1(SYS_EXIT, 0);
    }
    loop {}
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
