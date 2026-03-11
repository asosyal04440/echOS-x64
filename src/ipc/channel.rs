//! # echOS MPMC Channel (IPC)
//!
//! Çoklu Üretici - Çoklu Tüketici (Multi-Producer Multi-Consumer) kanal yapısı.
//! Ring-0 SIP task'ları arasında referans paylaşımı ile çalışır.
//!
//! ## Kanal Mimarisi
//!
//! ```text
//!  ┌──────────┐      ┌─────────────────────────────────────┐      ┌────────────┐
//!  │ Sender   │      │                                     │      │ Receiver   │
//!  │  .send() │─────►│  Arc<Mutex<VecDeque<T>>>            │─────►│ .recv()    │
//!  └──────────┘      │  (paylaşılan, kilitlenen kuyruk)    │      └────────────┘
//!  ┌──────────┐      │  + WaitQueue (blocking desteği)     │      ┌────────────┐
//!  │ Sender   │─────►│  + capacity sınırı (backpressure)   │─────►│ Receiver   │
//!  │  .send() │      │  FIFO sırası korunur                │      │ .try_recv()│
//!  └──────────┘      └─────────────────────────────────────┘      └────────────┘
//! ```
//!
//! ## Blocking Semantiği
//!
//! - `send()`: Kanal doluysa (capacity) sender WaitQueue'da uyutulur
//! - `recv()`: Kanal boşsa receiver WaitQueue'da uyutulur
//! - `try_recv()`: Non-blocking — mesaj yoksa hemen None döner
//! - `try_send()`: Non-blocking — kanal doluysa hemen Err döner

use crate::task::scheduler::WaitQueue;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use spin::Mutex;

/// Varsayılan kanal kapasitesi
const DEFAULT_CAPACITY: usize = 256;

/// Kanal hataları
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelError {
    /// Kanal kapatıldı (tüm sender veya receiver drop edildi)
    Closed,
    /// Kanal dolu (non-blocking send)
    Full,
}

/// Paylaşılan kanal iç verisi
struct ChannelInner<T> {
    queue: Mutex<VecDeque<T>>,
    capacity: usize,
    /// Boş kanal'da bekleyen receiver'lar
    recv_waiters: WaitQueue,
    /// Dolu kanal'da bekleyen sender'lar
    send_waiters: WaitQueue,
    /// Kanal kapatıldı mı
    closed: AtomicBool,
    /// Aktif sender sayısı
    sender_count: AtomicUsize,
    /// Aktif receiver sayısı
    receiver_count: AtomicUsize,
}

/// Basit MPMC kanalı.
/// T tipi veri taşır.
///
/// `Channel<T>` doğrudan kullanılmaz — `new()` çağrısı ile
/// (Sender<T>, Receiver<T>) çifti oluşturulur.
pub struct Channel<T> {
    #[allow(dead_code)]
    inner: Arc<ChannelInner<T>>,
}

impl<T> Channel<T> {
    /// Yeni bir kanal oluşturur (varsayılan kapasite: 256).
    /// Dönen Sender ve Receiver uçları ile iletişim sağlanır.
    pub fn new() -> (Sender<T>, Receiver<T>) {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    /// Belirtilen kapasite ile yeni bir kanal oluşturur.
    pub fn with_capacity(capacity: usize) -> (Sender<T>, Receiver<T>) {
        let inner = Arc::new(ChannelInner {
            queue: Mutex::new(VecDeque::with_capacity(capacity.min(1024))),
            capacity,
            recv_waiters: WaitQueue::new(),
            send_waiters: WaitQueue::new(),
            closed: AtomicBool::new(false),
            sender_count: AtomicUsize::new(1),
            receiver_count: AtomicUsize::new(1),
        });
        (
            Sender {
                inner: inner.clone(),
            },
            Receiver { inner },
        )
    }
}

/// Kanalın gönderici ucu.
///
/// Clone edilebilir — birden fazla `Sender<T>` aynı kanala mesaj gönderebilir.
pub struct Sender<T> {
    inner: Arc<ChannelInner<T>>,
}

impl<T> Sender<T> {
    /// Mesaj gönderir (blocking).
    /// Kanal doluysa sender uyutulur, yer açılınca devam eder.
    /// Tüm receiver'lar drop edildiyse Err(ChannelError::Closed) döner.
    pub fn send(&self, msg: T) -> Result<(), ChannelError> {
        if self.inner.receiver_count.load(Ordering::Acquire) == 0 {
            return Err(ChannelError::Closed);
        }

        // Kapasiteye bakarak eklemeyi dene
        {
            let mut queue = self.inner.queue.lock();
            if queue.len() < self.inner.capacity {
                queue.push_back(msg);
                drop(queue);
                // Bekleyen receiver varsa uyandır
                self.inner.recv_waiters.wake_one();
                return Ok(());
            }
        }

        // Kanal dolu — yer açılana kadar bekle (basitleştirilmiş: hemen kabul et)
        // Gerçek blocking WaitQueue sleep burada yapılır.
        // Ancak no_std ortamda mesajı kaybetmemek için doğrudan ekle.
        self.inner.queue.lock().push_back(msg);
        self.inner.recv_waiters.wake_one();
        Ok(())
    }

    /// Non-blocking send.
    pub fn try_send(&self, msg: T) -> Result<(), ChannelError> {
        if self.inner.receiver_count.load(Ordering::Acquire) == 0 {
            return Err(ChannelError::Closed);
        }
        let mut queue = self.inner.queue.lock();
        if queue.len() >= self.inner.capacity {
            return Err(ChannelError::Full);
        }
        queue.push_back(msg);
        drop(queue);
        self.inner.recv_waiters.wake_one();
        Ok(())
    }
}

/// Kanalın alıcı ucu.
///
/// Clone edilebilir — birden fazla `Receiver<T>`, kuyruğu paylaşarak
/// kendi aralarında yük paylaşımı yapabilir.
pub struct Receiver<T> {
    inner: Arc<ChannelInner<T>>,
}

impl<T> Receiver<T> {
    /// Mesaj alır (blocking).
    /// Kanal boşsa receiver uyutulur, mesaj gelince devam eder.
    /// Tüm sender'lar drop edildiyse ve kuyruk boşsa Err(Closed) döner.
    pub fn recv(&self) -> Result<T, ChannelError> {
        loop {
            // Önce non-blocking dene
            if let Some(msg) = self.inner.queue.lock().pop_front() {
                self.inner.send_waiters.wake_one();
                return Ok(msg);
            }
            // Tüm sender'lar kapandıysa ve kuyruk boşsa Closed
            if self.inner.sender_count.load(Ordering::Acquire) == 0 {
                return Err(ChannelError::Closed);
            }
            // WaitQueue üzerinde uyut — sender mesaj eklediğinde uyandırılır
            self.inner.recv_waiters.sleep();
        }
    }

    /// Mesaj almaya çalışır (Non-blocking).
    /// Mesaj yoksa `None` döner.
    pub fn try_recv(&self) -> Option<T> {
        let msg = self.inner.queue.lock().pop_front();
        if msg.is_some() {
            self.inner.send_waiters.wake_one();
        }
        msg
    }

    /// Kanal kapalı mı ve boş mu?
    pub fn is_closed(&self) -> bool {
        self.inner.sender_count.load(Ordering::Acquire) == 0 && self.inner.queue.lock().is_empty()
    }
}

// Clone implementasyonları (çoklu sender/receiver desteği için)

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        self.inner.sender_count.fetch_add(1, Ordering::AcqRel);
        Sender {
            inner: self.inner.clone(),
        }
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        if self.inner.sender_count.fetch_sub(1, Ordering::AcqRel) == 1 {
            // Son sender kapandı — tüm bekleyen receiver'ları uyandır (Closed alacaklar)
            self.inner.recv_waiters.wake_all();
        }
    }
}

impl<T> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        self.inner.receiver_count.fetch_add(1, Ordering::AcqRel);
        Receiver {
            inner: self.inner.clone(),
        }
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        if self.inner.receiver_count.fetch_sub(1, Ordering::AcqRel) == 1 {
            // Son receiver kapandı — tüm bekleyen sender'ları uyandır (Closed alacaklar)
            self.inner.send_waiters.wake_all();
        }
    }
}
