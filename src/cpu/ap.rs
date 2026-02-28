//! # AP (Application Processor) Başlatıcı
//!
//! Bu modül, çok işlemcili (SMP) sistemlerde BSP (Bootstrap Processor) tarafından
//! uyandırılan AP CPU'larının Rust giriş noktasını içerir.
//!
//! Her AP, `ap_startup.asm` içindeki real mod → protected mod → long mod
//! geçiş zincirini tamamladıktan sonra bu modüldeki `ap_entry` fonksiyonuna atlar.
//! Başlatma sırası GDT, IDT, LAPIC, GS_BASE ve sonunda `idle_loop`'tur.

use crate::syscall::{init_cpu_data, CpuData};
use x86_64::VirtAddr;

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

    // Bu noktadan itibaren istisna (exception) oluşturabilecek işlemler güvenli
    // HAM UART HATA AYIKLAMA: COM1 (0x3F8) portuna 'A' karakteri yaz — AP'nin bu noktaya ulaştığını doğrular
    unsafe {
        x86_64::instructions::port::Port::<u8>::new(0x3f8).write(b'A');
    }
    // HAM UART HATA AYIKLAMA: COM1'e 'B' yaz — GDT/IDT kurulumunun tamamlandığını gösterir
    unsafe {
        x86_64::instructions::port::Port::<u8>::new(0x3f8).write(b'B');
    }

    // CPU kimliği zaten cpu_data içinde mevcut.
    // APIC ID'yi erken başlangıçta MMIO/MSR okuyarak değil, global SMP durumundan al —
    // bu daha güvenli ve SMP durum yapısıyla tutarlıdır.
    let apic_id = {
        let mut id = cpu_id;
        if let Some(state) = crate::cpu::smp::SMP_STATE.try_lock() {
            id = state.cpu_apic_ids.get(cpu_id as usize).copied().unwrap_or(cpu_id);
        }
        id
    };

    // HAM UART HATA AYIKLAMA: COM1'e 'C' yaz — APIC ID alındı
    unsafe {
        x86_64::instructions::port::Port::<u8>::new(0x3f8).write(b'C');
    }
    unsafe {
        crate::debug::serial::EMERGENCY_SERIAL.write_byte(b'D');
    }
    // Local APIC'i başlat — bu CPU'nun kesme alabilmesi ve IPI gönderebilmesi için gerekli
    let _ = crate::apic::lapic::init();
    unsafe {
        crate::debug::serial::EMERGENCY_SERIAL.write_byte(b'E');
    }
    // GS_BASE MSR'a cpu_data işaretçisini yaz — çekirdek CPU-yerel veriye bu yolla erişir.
    // GS taban adresi, "fs:0" veya "gs:0" gibi segment-göreli erişimler için kullanılır.
    unsafe {
        use x86_64::registers::model_specific::Msr;
        // MSR 0xC0000101 = IA32_GS_BASE — GS segmentinin kernel mod taban adresi
        Msr::new(0xC0000101).write(cpu_data as *mut CpuData as u64);
    }
    // Not: GDT ve IDT başta yüklendi — init_per_cpu() yeniden çağrılmamalı
    // Not: IDT başta yüklendi — tekrar yüklemeye gerek yok
    unsafe {
        crate::debug::serial::EMERGENCY_SERIAL.write_byte(b'G');
        x86_64::instructions::port::Port::<u8>::new(0x3f8).write(b'W');
    }
    // Bu CPU'yu çevrimiçi (online) olarak işaretle — BSP tarafından beklenen onay sinyali
    crate::cpu::smp::mark_cpu_online(cpu_id, apic_id);
    unsafe {
        crate::debug::serial::EMERGENCY_SERIAL.write_byte(b'H');
    }
    // Kesmeleri etkinleştir — bu noktadan itibaren zamanlayıcı ve donanım kesmeleri işlenir
    unsafe {
        x86_64::instructions::interrupts::enable();
    }
    unsafe {
        crate::debug::serial::EMERGENCY_SERIAL.write_byte(b'I');
    }
    // Boşta döngüsüne gir — görev sırası boşken CPU burada HLT ile düşük güç modunda bekler
    crate::task::scheduler::idle_loop();
}
