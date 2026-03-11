//! # AP (Application Processor) Başlatıcı
//!
//! Bu modül, çok işlemcili (SMP) sistemlerde BSP (Bootstrap Processor) tarafından
//! uyandırılan AP CPU'larının Rust giriş noktasını içerir.
//!
//! Her AP, `ap_startup.asm` içindeki real mod → protected mod → long mod
//! geçiş zincirini tamamladıktan sonra bu modüldeki `ap_entry` fonksiyonuna atlar.
//! Başlatma sırası GDT, IDT, LAPIC, GS_BASE ve sonunda `idle_loop`'tur.

use crate::syscall::{init_cpu_data, CpuData};
use core::arch::{asm, naked_asm};
use x86_64::VirtAddr;

#[inline(always)]
fn ap_marker(byte: u8) {
    unsafe {
        x86_64::instructions::port::PortWriteOnly::<u8>::new(0xE9).write(byte);
    }
}

/// AP Park Modu — sonsuz döngüde bekle (interrupt kapalı)
/// Bu fonksiyon ASLA dönmemeli.
#[cfg(not(target_os = "windows"))]
#[unsafe(naked)]
unsafe extern "C" fn park_secondary_cpu() -> ! {
    naked_asm!(
        "cli",
        "1:",
        "hlt",
        "jmp 1b"
    );
}

#[cfg(target_os = "windows")]
unsafe extern "C" fn park_secondary_cpu() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

/// AP (Application Processor — Uygulama İşlemcisi) giriş noktası.
///
/// ## AP Nedir?
/// Çok işlemcili sistemlerde başlangıçta yalnızca bir CPU (BSP — Bootstrap Processor) çalışır.
/// Diğer tüm CPU'lar (AP'ler) BSP tarafından IPI (Inter-Processor Interrupt) ile uyandırılır.
/// Her AP, real mod → protected mod → long mod geçiş zincirini ap_startup.asm'de tamamladıktan
/// sonra bu Rust fonksiyonuna atlar.
///
/// ## Başlatma Sırası (KRİTİK — değiştirilmemeli)
/// ```text
///  1. init_cpu_data()   → CPU'ya özgü veri alanı (GS segment base) kurulur
///  2. gdt::init()       → Bu CPU için GDT yüklenir (CS=0x08 sağlanır)
///  3. idt::init()       → Bu CPU için IDT yüklenir
///  4. lapic::init()     → Yerel APIC başlatılır (kesme ileticisi aktif edilir)
///  5. GS_BASE MSR       → Çekirdeğin CPU-yerel veri işaretçisi ayarlanır
///  6. mark_cpu_online() → SMP durumu güncellenir; BSP bu CPU'yu hazır sayar
///  7. interrupts::enable() → Kesmeler açılır
///  8. idle_loop()       → CPU görev sırası boşken burada bekler
/// ```
#[no_mangle]
pub extern "sysv64" fn ap_entry(cpu_data: &'static mut CpuData) -> ! {
    // KRİTİK: CPU'ya özgü veri alanı EN ÖNCE başlatılmalı (CPU-yerel erişim için zorunlu)
    unsafe {
        init_cpu_data(cpu_data as *mut CpuData);
    }

    let cpu_id = cpu_data.cpu_id;

    // KRİTİK SIRALAMA: GDT mutlaka IDT'den ÖNCE yüklenmelidir!
    //
    // Neden bu sıra önemlidir?
    //   Erken boot AP trampoleni CS=0x18 ile çalışır.
    //   Çekirdek GDT'si CS=0x08 kullanır ve 0x18 ofsetini TSS olarak tanımlar.
    //   Eğer IDT CS=0x18 aktifken kurulursa, tüm kesme kapıları (interrupt gates)
    //   yeni GDT'deki TSS seçicisini gösterecek ve herhangi bir kesmede #GP(0x18) hatası üretilecektir.
    crate::gdt::init_for_cpu(cpu_id, VirtAddr::new(cpu_data.kernel_stack_top));

    unsafe {
        // Bu CPU için IDT'yi IDT tablosundan yükle — her AP kendi IDT kaydını alır
        let idt = crate::interrupts::init_idt_for_cpu(cpu_id);
        idt.load();
    }

    if crate::apic::lapic::init().is_err() {
        // Hata durumunda bile devam etmeye çalış, log bas
        ap_marker(b'E');
    }
    
    crate::apic::lapic::mask_timer();
    crate::cpu::init_secondary_cpu();
    crate::syscall::init();
    
    // Online işaretini vermeden önce stack/instruction stream'in temiz olduğundan emin ol
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    
    // AP'yi online işaretle
    crate::cpu::smp::mark_cpu_online(cpu_id, cpu_id);
    
    // Doğrudan park moduna geç - return yok, stack kullanımı yok
    unsafe { park_secondary_cpu() }
}
