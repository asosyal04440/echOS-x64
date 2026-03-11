//! # echOS IPC (Inter-Process Communication — Süreçler Arası İletişim)
//!
//! Task'lar arası iletişim altyapısı.
//! Mesaj gönderme ve alma ile channel-based iletişim.
//!
//! ## IPC Nedir?
//!
//! IPC (Süreçlerarası İletişim), birbirinden izole çalışan task'ların
//! bilgi paylaşabilmesini sağlayan mekanizmaların genel adıdır.
//! echOS iki yöntemi destekler:
//!
//! ### 1. Mesaj Kuyrukları (Message Queues) — `message` modülü
//!
//! ```text
//!  Gönderen Task (sender_id)          Alıcı Task (target_id)
//!  ──────────────────────             ──────────────────────
//!  send_message(target, data)  ──►   REGISTRY[target].mailbox
//!                                           │
//!                                           ▼
//!                                    receive_message(my_id)
//!                                    ◄── Message { sender, data }
//! ```
//!
//! Mailbox (posta kutusu) modeli: Her task'ın kendi mesaj kutusundaki
//! mesajlar FIFO sırasıyla alınır. Kuyruk dolduğunda (MAX_QUEUE_SIZE=32)
//! gönderim başarısız olur (backpressure).
//!
//! ### 2. MPMC Kanalları (Multi-Producer Multi-Consumer) — `channel` modülü
//!
//! ```text
//!  Sender1 ─┐                    ┌─► Receiver1
//!  Sender2 ─┼──► Arc<Mutex<VecDeque>> ──┤
//!  Sender3 ─┘   (paylaşılan kuyruk)    └─► Receiver2
//! ```
//!
//! Rust'ın sahiplik sistemi sayesinde birden fazla Sender ve Receiver
//! aynı kuyruktan güvenle veri alıp gönderebilir.
//!
//! ## Hangi Yöntemi Seçmeli?
//!
//! ```text
//!  Durum                           │ Öneri
//!  ────────────────────────────────┼────────────────────
//!  Task ID ile belirli alıcıya     │ send_message()
//!  Typed veri, generic kanal       │ Channel<T>
//!  Non-blocking kontrol            │ has_message() / try_recv()
//!  Çoklu üretici/tüketici          │ Channel<T> clone
//! ```

/// Mesaj yapısı ve gönderme/alma fonksiyonları
pub mod message;

/// MPMC kanalları (Multi-Producer Multi-Consumer)
pub mod channel;

/// Servis IPC arayüzü (Faz 3)
pub mod service_ipc;

/// Service IPC türleri (Faz 3)
pub use service_ipc::*;

/// Eventfd / Signalfd / Timerfd — Linux IPC dosya tanımlayıcıları
pub mod event_fd;

pub use message::{has_message, receive_message, send_message, Message};
