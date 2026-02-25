//! # echOS PIC Sürücüsü
//!
//! 8259 Programmable Interrupt Controller yapılandırması.
//! Master (IRQ 0-7) ve Slave (IRQ 8-15) PIC'leri yönetir.

use pic8259::ChainedPics;
use spin::Mutex;

/// Master PIC interrupt offset (IDT'de 32-39)
pub const PIC_1_OFFSET: u8 = 32;
/// Slave PIC interrupt offset (IDT'de 40-47)
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

/// Global PIC instance (Master + Slave zinciri)
pub static PICS: Mutex<ChainedPics> =
    Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

/// PIC'leri başlatır ve interrupt mask'leri ayarlar.
///
/// - Master: Timer (0), Keyboard (1), Cascade (2) açık
/// - Slave: Tümü kapalı (Mouse polling mode kullanıyor)
pub fn init() {
    crate::serial_println!("DEBUG: Entering init_pics");
    crate::serial_println!("DEBUG: Attempting to lock PICS...");
    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        let mut pics = PICS.lock();
        crate::serial_println!("DEBUG: PICS lock acquired!");
        crate::serial_println!("DEBUG: Sending PIC remap commands (unsafe)...");
        pics.initialize();
        crate::serial_println!("DEBUG: PICS initialized successfully");
        pics.write_masks(0xF8, 0xFF);
    });
    crate::serial_println!("PIC initialized");
}
