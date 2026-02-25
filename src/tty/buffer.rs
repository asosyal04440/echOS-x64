//! TTY Lock-Free Ring Buffer
//! Interrupt handler'lar (Klavye) ile user-space (sys_read) arasında 
//! veri kopyalamak için kullanılan asenkron, kilitsiz (lock-free) dairesel tampon.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};

pub const TTY_BUF_SIZE: usize = 4096;

/// SPSC (Single Producer, Single Consumer) tarzı sıfır kopya (zero copy) 
/// veya asgari kopya ring buffer yapısı.
pub struct TtyBuffer {
    data: UnsafeCell<[u8; TTY_BUF_SIZE]>,
    head: AtomicUsize,
    tail: AtomicUsize,
}

// Interrupt ve çekirdekler arası güvenli geçiş için
unsafe impl Sync for TtyBuffer {}
unsafe impl Send for TtyBuffer {}

impl TtyBuffer {
    pub const fn new() -> Self {
        Self {
            data: UnsafeCell::new([0; TTY_BUF_SIZE]),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    /// TTY buffer'ına karakter yazar. Buffer doluysa Err(()) döner.
    pub fn push(&self, val: u8) -> Result<(), ()> {
        let head = self.head.load(Ordering::Relaxed);
        let next_head = (head + 1) % TTY_BUF_SIZE;
        
        if next_head == self.tail.load(Ordering::Acquire) {
            return Err(()); // Buffer dolu (overflow)
        }
        
        unsafe {
            (*self.data.get())[head] = val;
        }
        self.head.store(next_head, Ordering::Release);
        Ok(())
    }

    /// TTY buffer'ından bir karakter çeker. Buffer boşsa None döner.
    pub fn pop(&self) -> Option<u8> {
        let tail = self.tail.load(Ordering::Relaxed);
        if tail == self.head.load(Ordering::Acquire) {
            return None; // Buffer boş
        }
        
        let val = unsafe { (*self.data.get())[tail] };
        self.tail.store((tail + 1) % TTY_BUF_SIZE, Ordering::Release);
        Some(val)
    }

    /// Buffer'ın en son yazılan karakterini siler (Backspace fonksiyonu için).
    pub fn unpush(&self) -> bool {
        loop {
            let head = self.head.load(Ordering::Acquire);
            let tail = self.tail.load(Ordering::Acquire);
            if head == tail {
                return false; // Silebilecek karakter yok
            }
            
            let prev_head = if head == 0 { TTY_BUF_SIZE - 1 } else { head - 1 };
            
            // Atomik olarak head'i bir geri alıyoruz (CAS işlemi önerilir ama 
            // tekil üretici varsa directly store yeterli olabilir, yine de CAS daha güvenli)
            if self.head.compare_exchange_weak(head, prev_head, Ordering::Release, Ordering::Relaxed).is_ok() {
                return true;
            }
        }
    }
    
    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::Acquire) == self.tail.load(Ordering::Acquire)
    }
}
