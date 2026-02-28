//! # echOS Input Event Kuyruğu
//!
//! Bu modül, keyboard ve mouse event'lerini toplayan merkezi kuyruk yapısını sağlar.
//!
//! ## Veri Akışı
//!
//! ```
//!   [PS/2 IRQ1]  -> push_event(Keyboard(key))  \
//!                                                +-> INPUT_QUEUE (VecDeque)
//!   [PS/2 IRQ12] -> push_event(Mouse(paket))   /         |
//!                                                         v
//!                                               Compositor main loop
//!                                               pop_event() -> işle
//! ```
//!
//! ## Tasarım Kararları
//!
//! - Interrupt handler'lar (IRQ1, IRQ12) event'leri kuyruğa ekler.
//! - Ana döngü (compositor/scheduler) kuyruğu boşaltır.
//! - Kuyruk dolunca en eski event çıkarılır (ring-buffer benzeri davranış).
//! - MAX_INPUT_EVENTS = 2048: 60 Hz'de ~34 saniyelik tampon.

use alloc::collections::VecDeque;
use lazy_static::lazy_static;
use pc_keyboard::DecodedKey;
use spin::Mutex;

// ============================================================================
// EVENT TÜRLERİ (EVENT TYPES)
// ============================================================================

/// PS/2 mouse paket formatları.
/// Standart PS/2 mouse 3 byte gönderirken, IntelliMouse (scroll destekli) 4 byte gönderir.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MousePacket {
    /// Standart 3-byte PS/2 paketi: butonlar + X/Y delta
    Standard { buttons: u8, x: i32, y: i32 },
    /// IntelliMouse 4-byte paketi: butonlar + X/Y delta + Z (scroll tekerleği)
    Intelli { buttons: u8, x: i32, y: i32, z: i32 },
}

/// Input event türleri: keyboard tuşu, işlenmiş mouse paketi ya da ham PS/2 byte.
///
/// Ham byte (MouseByte) path'i, kesme işleyicisinin minimal kod yürütmesi gerektiği
/// kritik bölümlerde kullanılır; byte toplandıktan sonra ana döngüde işlenir.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputEvent {
    /// Keyboard tuş basma veya bırakma olayı (pc_keyboard crate'i ile decode edilmiş)
    Keyboard(DecodedKey),
    /// PS/2 protokolüyle tam olarak ayrıştırılmış mouse paketi
    Mouse(MousePacket),
    /// Ham PS/2 mouse byte'ı (Hızlı geçiş / Fast-Path ISR'dan gelir)
    MouseByte(u8),
}

// ============================================================================
// EVENT KUYRUĞU (EVENT QUEUE)
// ============================================================================

lazy_static! {
    /// Sistem geneli tek global input event kuyruğu.
    /// spin::Mutex ile korunur: interrupt handler'dan güvenle erişilebilir.
    static ref INPUT_QUEUE: Mutex<VecDeque<InputEvent>> = Mutex::new(VecDeque::new());
}

/// Kuyruk maksimum boyutu. Aşılırsa en eski event silinir.
/// 2048 event: 60 Hz compositor ile ~34 saniyelik tampon.
const MAX_INPUT_EVENTS: usize = 2048;

/// Event'i kuyruğa ekler.
///
/// Interrupt handler'lardan (IRQ1 klavye, IRQ12 mouse) çağrılır.
/// `without_interrupts` bloğu deadlock'u önler: lock tutulurken
/// yeni interrupt gelip aynı lock'u almaya çalışmasının önüne geçer.
pub fn push_event(event: InputEvent) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut q = INPUT_QUEUE.lock();
        if q.len() >= MAX_INPUT_EVENTS {
            // Kuyruk doldu: en eski event'i çıkar (ring-buffer davranışı)
            let _ = q.pop_front();
        }
        q.push_back(event);
    });
}

/// Kuyruktan bir event çeker; kuyruk boşsa None döner.
///
/// Compositor/görev zamanlayıcı main loop'undan çağrılır.
/// `without_interrupts` bloğu: okuma sırasında yeni event gelmesini engeller.
pub fn pop_event() -> Option<InputEvent> {
    x86_64::instructions::interrupts::without_interrupts(|| INPUT_QUEUE.lock().pop_front())
}
