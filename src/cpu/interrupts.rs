//! # echOS Interrupt Tanımları
//!
//! Interrupt handler nesneleri ve indeksleri.

use pic8259::ChainedPics;
use spin::Mutex;
use x86_64::structures::idt::InterruptStackFrame;

/// Ana (Master) PIC kesme vektörü başlangıç ofseti
pub const PIC_1_OFFSET: u8 = 32;
/// Bağımlı (Slave) PIC kesme vektörü başlangıç ofseti
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

/// Global Programmable Interrupt Controller (PIC)
pub static PICS: Mutex<ChainedPics> =
    Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

/// Interrupt türleri için enum
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = PIC_1_OFFSET,
    Keyboard = PIC_1_OFFSET + 1,
}

impl InterruptIndex {
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    pub fn as_usize(self) -> usize {
        usize::from(self.as_u8())
    }
}

/// Timer Interrupt Handler
pub extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Timer.as_u8());
    }
}

/// Keyboard Interrupt Handler
pub extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use x86_64::instructions::port::Port;

    // Scancode'u oku (PIC buffer'ı boşaltmak için gerekli)
    let mut port = Port::new(0x60);
    let _scancode: u8 = unsafe { port.read() };

    // Not: Gerçek klavye işleme drivers/ps2.rs üzerinden polling ile veya
    // task listesine eklenerek yapılabilir. Şimdilik sadece PIC bildirimi yapıyoruz.

    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8());
    }
}
