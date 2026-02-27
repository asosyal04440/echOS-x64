//! echsh - echOS Yerel Kabuk (Shell) Uygulaması
//!
//! Bu dosya; echOS'un Ring 3 kullanıcı alanında çalışan, syscall tabanlı
//! minimal bir kabuk uygulamasının giriş noktasını içerir. TTY'den satır
//! satır okuma yaparak temel komut ayrıştırma işlemi gerçekleştirir.

#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;

const SYS_READ: usize = 0;
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

fn write_str(value: &str) {
    write_all(value.as_bytes());
}

fn read_stdin(buf: &mut [u8]) -> usize {
    unsafe {
        let ret = syscall3(SYS_READ, 0, buf.as_mut_ptr() as usize, buf.len());
        if ret < 0 {
            0
        } else {
            ret as usize
        }
    }
}

fn exit(code: i32) -> ! {
    unsafe {
        let _ = syscall1(SYS_EXIT, code as usize);
    }
    loop {}
}

fn panic_exit(message: &str) -> ! {
    write_str("\n[echsh] ÖLÜMCÜL HATA: ");
    write_str(message);
    write_str("\n");
    exit(1);
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    write_str("\n================================================\n");
    write_str("   >>> echOS Modern Terminal (echsh) v0.1 <<<   \n");
    write_str("================================================\n\n");

    write_str("Ring 3 Kullanıcı Alanında Çalışıyor!\n");
    write_str("Komutlar için 'help' yazın.\n\n");

    let mut buf = [0u8; 128];

    loop {
        write_str("echOS> ");

        let mut line_len = 0;

        // Gerekirse yield/spin döngüsü syscall'ı kullanarak bloke okuma simülasyonu
        // echOS'ta TTY üzerinden sys_read şu an bloke olmayabilir.
        // Yeni satır görene kadar sürekli okuyalım.
        loop {
            let read_bytes = read_stdin(&mut buf[line_len..line_len+1]);
            if read_bytes > 0 {
                let c = buf[line_len];
                line_len += 1;
                if c == b'\n' {
                    break;
                }
            } else {
                // Dönerken Kullanıcı Alanında %100 CPU kullanımından kaçınmak için:
                // İdeal olarak sys_sched_yield (SYS_SCHED_YIELD = 24) çağrılmalı
                unsafe { syscall1(24, 0); }
            }
        }

        if line_len > 0 {
            // Komutu yankıla
            write_str("Alınan komut: ");
            write_all(&buf[..line_len]);

            // Temel komut ayrıştırıcı
            if line_len >= 4 && &buf[0..4] == b"help" {
                write_str("Kullanılabilir komutlar: help, clear, exit, about\n");
            } else if line_len >= 4 && &buf[0..4] == b"exit" {
                write_str("Kabuktan çıkılıyor...\n");
                exit(0);
            } else if line_len >= 5 && &buf[0..5] == b"about" {
                write_str("echsh - echOS Asenkron Yerel Kabuğu\n");
            }
        }
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    panic_exit("echsh içinde panik");
}
