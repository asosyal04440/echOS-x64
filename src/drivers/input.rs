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

use crate::drivers::gesture::Gesture;
use crate::drivers::spsc::SpscQueue;
use alloc::collections::VecDeque;
use lazy_static::lazy_static;
use pc_keyboard::{DecodedKey, KeyState};
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
    Keyboard {
        decoded: Option<DecodedKey>,
        scan_code: u16,
        modifiers: u8,
        state: KeyState,
    },
    /// PS/2 protokolüyle tam olarak ayrıştırılmış mouse paketi
    Mouse(MousePacket),
    /// Ham PS/2 mouse byte'ı (Hızlı geçiş / Fast-Path ISR'dan gelir)
    MouseByte(u8),
    /// Gesture (el hareketi) olayı
    Gesture(Gesture),
}

// ============================================================================
// ABSOLUTE ZERO INPUT PIPELINE (SPSC)
// ============================================================================

/// SPSC Lock-Free Input Queue. Capacity 4096 (Power of Two).
/// Zero Mutex. Zero Contention.
static INPUT_SPSC: spin::Lazy<SpscQueue<InputEvent, 4096>> = spin::Lazy::new(|| SpscQueue::new());

/// Kuyruk maksimum boyutu.
const MAX_INPUT_EVENTS: usize = 4096;

/// Event'i SPSC kuyruğuna ekler (Lock-Free).
pub fn push_event(event: InputEvent) {
    let _ = INPUT_SPSC.push(event);
}

/// SPSC kuyruğundan bir event çeker (Lock-Free).
pub fn pop_event() -> Option<InputEvent> {
    INPUT_SPSC.pop()
}
