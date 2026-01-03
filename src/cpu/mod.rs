//! # echOS CPU Modülü
//! 
//! CPU yapılandırması: GDT, IDT ve SSE/AVX etkinleştirme.

/// Interrupt Descriptor Table
pub mod idt;

/// Interrupt handlers
pub mod interrupts;

/// Global Descriptor Table
pub mod gdt;

/// CPU özelliklerini etkinleştirir (SSE, AVX).
pub fn init() {
    enable_sse();
}

/// SSE (Streaming SIMD Extensions) talimatlarını etkinleştirir.
/// CR0 ve CR4 register'larını yapılandırır.
fn enable_sse() {
    use x86_64::registers::control::{Cr0, Cr0Flags, Cr4, Cr4Flags};
    
    unsafe {
        // CR0: EM bitini temizle, MP bitini ayarla
        let mut cr0 = Cr0::read();
        cr0.remove(Cr0Flags::from_bits_truncate(0x4));
        cr0.insert(Cr0Flags::MONITOR_COPROCESSOR);
        Cr0::write(cr0);
        
        // CR4: OSFXSR ve OSXMMEXCPT bitlerini ayarla
        let mut cr4 = Cr4::read();
        cr4.insert(Cr4Flags::OSFXSR);
        cr4.insert(Cr4Flags::OSXMMEXCPT_ENABLE);
        Cr4::write(cr4);
    }
    crate::serial_println!("SSE Enabled");
}
