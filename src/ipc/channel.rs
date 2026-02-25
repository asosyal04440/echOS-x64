//! # echOS MPMC Channel (IPC)
//!
//! Çoklu Üretici - Çoklu Tüketici (Multi-Producer Multi-Consumer) kanal yapısı.
//! Ring-0 SIP task'ları arasında referans paylaşımı ile çalışır.

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use spin::Mutex;

/// Basit MPMC kanalı.
/// T tipi veri taşır.
pub struct Channel<T> {
    #[allow(dead_code)]
    queue: Arc<Mutex<VecDeque<T>>>,
}

impl<T> Channel<T> {
    /// Yeni bir kanal oluşturur.
    /// Dönen Sender ve Receiver uçları ile iletişim sağlanır.
    pub fn new() -> (Sender<T>, Receiver<T>) {
        let queue = Arc::new(Mutex::new(VecDeque::new()));
        (
            Sender {
                queue: queue.clone(),
            },
            Receiver { queue },
        )
    }
}

/// Kanalın gönderici ucu.
pub struct Sender<T> {
    queue: Arc<Mutex<VecDeque<T>>>,
}

impl<T> Sender<T> {
    /// Mesaj gönderir.
    pub fn send(&self, msg: T) {
        self.queue.lock().push_back(msg);
    }
}

/// Kanalın alıcı ucu.
pub struct Receiver<T> {
    queue: Arc<Mutex<VecDeque<T>>>,
}

impl<T> Receiver<T> {
    /// Mesaj almaya çalışır (Non-blocking).
    /// Mesaj yoksa `None` döner.
    pub fn try_recv(&self) -> Option<T> {
        self.queue.lock().pop_front()
    }

    // Gelecekte: blocking recv() metodu eklenebilir (Scheduler::sleep kullanarak)
}

// Clone implementasyonları (çoklu sender/receiver desteği için)

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Sender {
            queue: self.queue.clone(),
        }
    }
}

impl<T> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        Receiver {
            queue: self.queue.clone(),
        }
    }
}
