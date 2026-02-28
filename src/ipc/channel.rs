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
//!  │  .send() │─────►│  Arc<Mutex<VecDeque<T>>>            │─────►│ .try_recv()│
//!  └──────────┘      │  (paylaşılan, kilitlenen kuyruk)    │      └────────────┘
//!  ┌──────────┐      │                                     │      ┌────────────┐
//!  │ Sender   │─────►│  FIFO sırası korunur                │─────►│ Receiver   │
//!  │  .send() │      │  push_back / pop_front              │      │ .try_recv()│
//!  └──────────┘      └─────────────────────────────────────┘      └────────────┘
//! ```
//!
//! ## Neden Arc<Mutex<VecDeque>>?
//!
//! - `Arc`   : Birden fazla Sender/Receiver aynı kuyruğu paylaşır (referans sayımlı)
//! - `Mutex` : Spin kilidi ile thread-safe erişim (no_std uyumlu)
//! - `VecDeque`: O(1) push_back ve O(1) pop_front (gerçek çift-uçlu kuyruk)
//!
//! ## Kullanım Örneği
//!
//! ```rust
//! let (tx, rx) = Channel::<u32>::new();
//! tx.send(42);
//! assert_eq!(rx.try_recv(), Some(42));
//!
//! // Çoklu gönderici:
//! let tx2 = tx.clone();   // aynı kuyruğa iki gönderici
//! tx2.send(100);
//! assert_eq!(rx.try_recv(), Some(100));
//! ```
//!
//! ## Dikkat Edilmesi Gerekenler
//!
//! - `try_recv()` non-blocking'dir; mesaj yoksa `None` döner.
//!   Blocking (bekleme) için TODO: `scheduler::sleep()` ile extend edilebilir.
//! - Kanal kapasitesi sınırsızdır (yalnızca mevcut bellek sınırlar).
//!   Üretici çok hızlıysa backpressure mekanizması yoktur — dikkatli kullan!

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use spin::Mutex;

/// Basit MPMC kanalı.
/// T tipi veri taşır.
///
/// `Channel<T>` doğrudan kullanılmaz — `new()` çağrısı ile
/// (Sender<T>, Receiver<T>) çifti oluşturulur.
pub struct Channel<T> {
    #[allow(dead_code)]
    queue: Arc<Mutex<VecDeque<T>>>,
}

impl<T> Channel<T> {
    /// Yeni bir kanal oluşturur.
    /// Dönen Sender ve Receiver uçları ile iletişim sağlanır.
    ///
    /// İkisi de aynı `Arc<Mutex<VecDeque<T>>>`'yi paylaşır.
    /// Clone ile çoğaltılabilirler (MPMC desteği).
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
///
/// Clone edilebilir — birden fazla `Sender<T>` aynı kanala mesaj gönderebilir.
/// `send()` çağrısı kilidi geçici olarak alır ve `O(1)`'de mesajı kuyruğa ekler.
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
///
/// Clone edilebilir — birden fazla `Receiver<T>`, kuyruğu paylaşarak
/// kendi aralarında yük paylaşımı yapabilir (work-stealing değil, yarışır).
pub struct Receiver<T> {
    queue: Arc<Mutex<VecDeque<T>>>,
}

impl<T> Receiver<T> {
    /// Mesaj almaya çalışır (Non-blocking).
    /// Mesaj yoksa `None` döner.
    ///
    /// Spin kilidi alır, kuyruğun önünden `O(1)` ile bir mesaj çıkarır.
    /// Mesaj olmadığında hemen `None` döner — bloklama gerçekleşmez.
    pub fn try_recv(&self) -> Option<T> {
        self.queue.lock().pop_front()
    }

    // Gelecekte: blocking recv() metodu eklenebilir (Scheduler::sleep kullanarak)
    // Örnek: scheduler::sleep_until(|| !self.queue.lock().is_empty())
}

// Clone implementasyonları (çoklu sender/receiver desteği için)
// Her clone, aynı Arc'ı paylaşır — yeni tahsisat olmaz, sadece referans sayacı artar.

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
