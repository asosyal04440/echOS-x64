//! # echOS PIC Sürücüsü (8259 Programmable Interrupt Controller)
//!
//! 8259 Programmable Interrupt Controller yapılandırması.
//! Master (IRQ 0-7) ve Slave (IRQ 8-15) PIC'leri yönetir.
//!
//! ## 8259 PIC Donanım Yapısı
//!
//! ```text
//!  CPU                    Master 8259A           Slave 8259A
//!  ───                    ────────────           ───────────
//!  INTA# ◄────────────── INT                    INT
//!  D[0:7] ◄──────────── D[0:7]       IRQ8  ──► IR0
//!                        │            IRQ9  ──► IR1
//!  Port 0x20 (komut) ───► │            IRQ10 ──► IR2
//!  Port 0x21 (veri)  ───► │            IRQ11 ──► IR3
//!                         │            IRQ12 ──► IR4  (Mouse)
//!  IRQ0 (Timer)  ──► IR0  │            IRQ13 ──► IR5  (FPU)
//!  IRQ1 (Keybd)  ──► IR1  │            IRQ14 ──► IR6  (HDD Primary)
//!  IRQ2 (Cascade)──► IR2 ─┼──► SLAVE  IRQ15 ──► IR7  (HDD Secondary)
//!  IRQ3 (COM2)   ──► IR3  │
//!  IRQ4 (COM1)   ──► IR4  │   Port 0xA0 (komut) ──► Slave
//!  IRQ5 (LPT2)   ──► IR5  │   Port 0xA1 (veri)  ──► Slave mask
//!  IRQ6 (Floppy) ──► IR6  │
//!  IRQ7 (LPT1)   ──► IR7  │
//! ```
//!
//! ## IDT Vektör Eşleşmesi
//!
//! ```text
//!  PIC IRQ  │ IDT Vektör │ Kullanım
//!  ─────────┼────────────┼──────────────────────
//!  IRQ0     │  0x20 (32) │ Timer (PIT / LAPIC)
//!  IRQ1     │  0x21 (33) │ PS/2 Klavye
//!  IRQ2     │  0x22 (34) │ Cascade (Slave bağlantısı)
//!  IRQ3     │  0x23 (35) │ COM2 (Seri Port)
//!  IRQ4     │  0x24 (36) │ COM1 (Seri Port)
//!  IRQ5     │  0x25 (37) │ LPT2 / Ses Kartı
//!  IRQ6     │  0x26 (38) │ Disket Sürücü
//!  IRQ7     │  0x27 (39) │ LPT1 (Paralel Port)
//!  IRQ8     │  0x28 (40) │ CMOS Real-Time Clock
//!  IRQ9     │  0x29 (41) │ ACPI / Ağ
//!  IRQ10    │  0x2A (42) │ USB / Ağ
//!  IRQ11    │  0x2B (43) │ USB / AGP
//!  IRQ12    │  0x2C (44) │ PS/2 Fare
//!  IRQ13    │  0x2D (45) │ x87 FPU
//!  IRQ14    │  0x2E (46) │ ATA Primary (IDE/SATA)
//!  IRQ15    │  0x2F (47) │ ATA Secondary (IDE/SATA)
//! ```
//!
//! ## PIC Başlatma Sırası (ICW — Initialization Command Words)
//!
//! ```text
//!  ICW1 → 0x11 : Cascade mode, ICW4 gerekli, edge-triggered
//!  ICW2 (Master) : interrupt vektör offset = 0x20 (32)
//!  ICW2 (Slave)  : interrupt vektör offset = 0x28 (40)
//!  ICW3 (Master) : Slave IR2 hattında → 0x04
//!  ICW3 (Slave)  : Cascade kimliği = 0x02
//!  ICW4 → 0x01  : 8086/88 modu, normal EOI
//! ```
//!
//! ## Maske Ayarları (echOS varsayılanı)
//!
//! ```text
//!  Master mask = 0xF8  →  0b11111000
//!    Bit 0 (IRQ0=Timer)   : 0 = AÇ
//!    Bit 1 (IRQ1=Keyboard): 0 = AÇ
//!    Bit 2 (IRQ2=Cascade) : 0 = AÇ (Slave için gerekli)
//!    Bit 3..7 (IRQ3..7)   : 1 = KAPALI
//!  Slave mask  = 0xFF  →  0b11111111
//!    Tüm Slave IRQ'ları kapalı (Master+Mouse doğrudan)
//! ```

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
///
/// `without_interrupts` bloğu içinde çalışır çünkü PIC başlatma
/// sırasında gelen spurious interrupt sistemi bozabilir.
/// `pics.initialize()` → ICW1-4 sırasını gönderir.
/// `pics.write_masks(0xF8, 0xFF)` → Yalnızca IRQ0,1,2'yi açar.
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
