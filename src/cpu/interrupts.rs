//! # echOS Kesme (Interrupt) Tanımları
//!
//! Donanım kesme işleyicileri, PIC ofseti sabitleri ve vektör dizin enum'u.
//!
//! ## PIC (Programlanabilir Kesme Denetleyicisi) Hakkında
//! x86 PC'lerde geleneksel donanım kesmeleri 8259A PIC çipi üzerinden iletilir.
//! İki çip zincirlenmiştir (Master + Slave) → toplam 16 IRQ hattı.
//! İşletim sistemi, kesmeleri işledikten sonra mutlaka EOI (End of Interrupt) sinyali
//! göndermelidir; aksi hâlde aynı IRQ hattı tekrar ateşlenmez.

use pic8259::ChainedPics;
use spin::Mutex;
use x86_64::structures::idt::InterruptStackFrame;

/// Ana PIC (Master Programmable Interrupt Controller) vektör ofseti.
/// x86 mimari kuralı: ilk 32 vektör (0-31) CPU istisnalarına ayrılmıştır.
/// Bu yüzden donanım kesmeleri 32'den başlamalı; aksi takdirde çift-hata (double fault) oluşur.
pub const PIC_1_OFFSET: u8 = 32;
/// Yardımcı PIC (Slave/Chained) vektör ofseti — ana PIC'in hemen arkasından gelir.
/// PIC zinciri: IRQ 0-7 → Master PIC (ofset 32-39), IRQ 8-15 → Slave PIC (ofset 40-47).
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

/// Global Programlanabilir Kesme Denetleyicisi (PIC) örneği.
/// `ChainedPics`, iki adet 8259A PIC'i zincirleyerek 16 IRQ hattı sağlar.
/// `unsafe` blok: donanım adresi sabitleriyle ham başlatma yapılır.
pub static PICS: Mutex<ChainedPics> =
    Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

/// Desteklenen donanım kesmelerine ait vektör numaraları.
/// Her değişken, IDT (Interrupt Descriptor Table) içindeki slota karşılık gelir.
/// `#[repr(u8)]` ile enum değerleri doğrudan u8 olarak kullanılabilir.
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

/// Zamanlayıcı Kesme İşleyicisi (Timer IRQ 0 — vektör 32).
///
/// PIT (Programlanabilir Aralık Zamanlayıcı) veya LAPIC timer her atışında çağrılır.
/// Görev: PIC'e "sону bildirimi" (EOI) göndererek bir sonraki kesmenin önünü açmak.
///
/// Akış:
///   [PIT/LAPIC ateşlenir]
///       │
///       ▼
///   [CPU IDT vektör 32'ye atlar → timer_interrupt_handler]
///       │
///       ▼
///   [PICS.notify_end_of_interrupt(32)]  ← PIC IRQ hattını serbest bırakır
pub extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Timer.as_u8());
    }
}

/// Klavye Kesme İşleyicisi (Keyboard IRQ 1 — vektör 33).
///
/// PS/2 klavye her tuş basımında/bırakımında IRQ 1 üretir.
/// Önemli: Port 0x60 okunmadan PIC buffer dolmaya devam eder ve yeni kesmeler gelmez.
/// Gerçek klavye işleme polling tabanlı `drivers/ps2.rs` üzerinden yürütülür;
/// bu handler yalnızca PIC tamponunu temizler ve EOI gönderir.
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
