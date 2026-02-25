//! # echOS Input Event Kuyruğu
//!
//! Bu modül, keyboard ve mouse event'lerini toplayan queue yapısını sağlar.
//! Interrupt handler'lar (IRQ1, IRQ12) event'leri buraya push eder,
//! Compositor main loop bunları pop ederek işler.

use alloc::collections::VecDeque;
use lazy_static::lazy_static;
use pc_keyboard::DecodedKey;
use spin::Mutex;

// ============================================================================
// EVENT TYPES
// ============================================================================

/// Mouse paketi formatları
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MousePacket {
    /// Standart 3-byte PS/2 paketi
    Standard { buttons: u8, x: i32, y: i32 },
    /// IntelliMouse 4-byte paketi (scroll wheel)
    Intelli { buttons: u8, x: i32, y: i32, z: i32 },
}

/// Input event türleri
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputEvent {
    /// Keyboard tuş basma/bırakma
    Keyboard(DecodedKey),
    /// İşlenmiş mouse paketi
    Mouse(MousePacket),
    /// Ham mouse byte (Fast-Path ISR'dan)
    MouseByte(u8),
}

// ============================================================================
// EVENT QUEUE
// ============================================================================

lazy_static! {
    /// Global input event kuyruğu
    static ref INPUT_QUEUE: Mutex<VecDeque<InputEvent>> = Mutex::new(VecDeque::new());
}

/// Event'i kuyruğa ekler.
/// Interrupt handler'lardan çağrılır.
pub fn push_event(event: InputEvent) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        INPUT_QUEUE.lock().push_back(event);
    });
}

/// Kuyruktan event çeker.
/// Compositor main loop'tan çağrılır.
pub fn pop_event() -> Option<InputEvent> {
    x86_64::instructions::interrupts::without_interrupts(|| INPUT_QUEUE.lock().pop_front())
}
