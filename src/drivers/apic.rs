//! # echOS Local APIC Sürücüsü
//! 
//! Advanced Programmable Interrupt Controller (APIC) sürücüsü.
//! Timer ve interrupt yönlendirme işlemlerini yönetir.

use x86_64::instructions::port::Port;
use x86_64::registers::model_specific::Msr;
use core::ptr::{read_volatile, write_volatile};

// APIC Register Offsetleri
const LAPIC_ID: usize = 0x020;
const LAPIC_EOI: usize = 0x0B0;
const LAPIC_SPURIOUS: usize = 0x0F0;
const LAPIC_ICR_LOW: usize = 0x300;
const LAPIC_ICR_HIGH: usize = 0x310;
const LAPIC_LVT_TIMER: usize = 0x320;
const LAPIC_TIMER_INIT: usize = 0x380;
const LAPIC_TIMER_CURRENT: usize = 0x390;
const LAPIC_TIMER_DIV: usize = 0x3E0;

/// Local APIC Base adresi.
/// Standart olarak 0xFEE00000, ancak safe access için MSR'dan okunmalı.
/// Şimdilik hardcoded 0xFEE00400 (QEMU için).
/// DİKKAT: Bu adresin map edilmiş olması gerekir!
static mut LAPIC_BASE: u64 = 0xFEE00400;

/// Local APIC'i başlatır.
/// 
/// 1. 8259 PIC'i devre dışı bırakır.
/// 2. APIC Base adresini öğrenir.
/// 3. Spurious Interrupt Vector'ü ayarlar.
/// 4. Timer'ı yapılandırır (10ms civarı).
pub unsafe fn init() {
    // 1. 8259 PIC Devre Dışı Bırak
    disable_pic();

    // 2. APIC Base'i MSR'den al (IA32_APIC_BASE)
    let apic_base_msr = Msr::new(0x1B);
    let base = apic_base_msr.read();
    let addr = base & 0xFFFFF000;
    LAPIC_BASE = addr;
    
    // 3. APIC Logic'i etkinleştir
    // Spurious Vector 255 (0xFF) + Enable Bit (8)
    write_reg(LAPIC_SPURIOUS, 0x1FF);

    // 4. Timer Kurulumu
    // Divider: 16'ya böl (0xB)
    write_reg(LAPIC_TIMER_DIV, 0xB);
    
    // Sayım başlangıç değeri (yaklaşık 10ms)
    // Kalibrasyon yapılmadan tahmini değer.
    write_reg(LAPIC_TIMER_INIT, 10_000_000); 

    // LVT Timer Register
    // Vector 32 (IDT'de Timer interrupt index'i)
    // Periodic Mode (Bit 17)
    write_reg(LAPIC_LVT_TIMER, 32 | 0x20000); 
    
    crate::serial_println!("Local APIC Timer Başlatıldı.");
}

/// Eski PIC'i (Programmable Interrupt Controller) devre dışı bırakır.
unsafe fn disable_pic() {
    let mut p1 = Port::<u8>::new(0x21);
    let mut p2 = Port::<u8>::new(0xA1);
    // Tüm interrupt'ları maskele (0xFF)
    p1.write(0xFF);
    p2.write(0xFF);
}

/// APIC registerına yazar.
unsafe fn write_reg(offset: usize, value: u32) {
    let ptr = (LAPIC_BASE + offset as u64) as *mut u32;
    write_volatile(ptr, value);
}

/// APIC registerından okur.
unsafe fn read_reg(offset: usize) -> u32 {
    let ptr = (LAPIC_BASE + offset as u64) as *const u32;
    read_volatile(ptr)
}

/// End of Interrupt (EOI) sinyali gönderir.
/// Interrupt handler bitiminde çağrılmalıdır.
pub unsafe fn eoi() {
    write_reg(LAPIC_EOI, 0);
}
