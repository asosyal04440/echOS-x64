//! # echOS Keyboard Buffer
//!
//! Keyboard input için ring buffer.
//! Interrupt handler'dan gelen tuşları saklar.

use alloc::collections::VecDeque;
use core::sync::atomic::{AtomicBool, Ordering};
use pc_keyboard::DecodedKey;
use spin::Mutex;
use x86_64::instructions::interrupts;

/// Buffer kapasitesi
const BUFFER_SIZE: usize = 128;

/// TTY'nin initialize edilip edilmediğini takip eden flag
static TTY_READY: AtomicBool = AtomicBool::new(false);

/// TTY'nin hazır olduğunu işaretle
pub fn mark_tty_ready() {
    TTY_READY.store(true, Ordering::SeqCst);
}

/// Keyboard tuş buffer'ı
pub struct KeyboardBuffer {
    buffer: VecDeque<DecodedKey>,
}

impl KeyboardBuffer {
    /// Yeni boş buffer oluşturur.
    pub fn new() -> Self {
        Self {
            buffer: VecDeque::with_capacity(BUFFER_SIZE),
        }
    }

    /// Buffer'a tuş ekler.
    pub fn push(&mut self, key: DecodedKey) {
        if self.buffer.len() < BUFFER_SIZE {
            self.buffer.push_back(key);
        }
    }

    /// Buffer'dan tuş çeker (FIFO).
    pub fn pop(&mut self) -> Option<DecodedKey> {
        self.buffer.pop_front()
    }

    /// Buffer boş mu kontrol eder.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }
}

lazy_static::lazy_static! {
    /// Global keyboard buffer
    static ref KEYBOARD_BUFFER: Mutex<KeyboardBuffer> = Mutex::new(KeyboardBuffer::new());
}

/// Interrupt handler'dan çağrılır - tuşu buffer'a ekler.
pub fn push_key(key: DecodedKey) {
    // TTY Line Discipline'e yolla - sadece TTY hazır olduğunda
    // Bu, lazy_static initialization sırasında PAGE FAULT'u önler
    if TTY_READY.load(Ordering::SeqCst) {
        crate::tty::DEFAULT_TTY.receive_key(key.clone());
    }

    interrupts::without_interrupts(|| {
        KEYBOARD_BUFFER.lock().push(key);
    });
}

/// Buffer'dan tuş okur (non-blocking).
pub fn read_key() -> Option<DecodedKey> {
    interrupts::without_interrupts(|| KEYBOARD_BUFFER.lock().pop())
}

/// Buffer'da tuş var mı kontrol eder.
pub fn has_key() -> bool {
    interrupts::without_interrupts(|| !KEYBOARD_BUFFER.lock().is_empty())
}
