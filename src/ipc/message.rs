//! # echOS Mesaj Kuyrukları (IPC — Message Passing)
//!
//! Task'lar arası mesajlaşma (Message Passing) altyapısı.
//! Mesaj gönderme, alma ve kuyruk yönetimi.
//!
//! ## Mailbox Mimarisi
//!
//! ```text
//!  REGISTRY (global, Mutex korumalı)
//!  ┌────────────────────────────────────────────────────────┐
//!  │  mailboxes: Vec<TaskMailbox>                           │
//!  │                                                        │
//!  │  ┌────────────────────────────────┐                    │
//!  │  │ TaskMailbox { task_id: 1 }     │                    │
//!  │  │   messages: [Msg1, Msg2, ...]  │ ← FIFO, max 32    │
//!  │  └────────────────────────────────┘                    │
//!  │  ┌────────────────────────────────┐                    │
//!  │  │ TaskMailbox { task_id: 2 }     │                    │
//!  │  │   messages: [Msg3]             │                    │
//!  │  └────────────────────────────────┘                    │
//!  │  ...                                                   │
//!  └────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Mesaj Yapısı
//!
//! ```text
//!  Message {
//!    sender: TaskId   — Mesajı kimin gönderdiği (yanıt için gerekli)
//!    data: Vec<u8>    — Ham byte verisi (text, binary, struct serialize)
//!  }
//! ```
//!
//! ## API Kullanımı
//!
//! ```text
//!  Task A                              Task B (target_id)
//!  ──────                              ─────────────────
//!  send_message(B_id, A_id, data) ──► B'nin mailbox'ına eklenir
//!
//!  (Task B çalışınca)
//!  has_message(B_id)  → true
//!  receive_message(B_id) → Some(Message { sender: A_id, data })
//!
//!  (Kuyruğu tamamen boşalt)
//!  while let Some(msg) = receive_message(B_id) { ... }
//! ```
//!
//! ## Kapasite Sınırı (Backpressure)
//!
//! ```text
//!  MAX_QUEUE_SIZE = 32 mesaj / mailbox
//!
//!  Kuyruk dolduğunda send_message() → false döner.
//!  Alıcı task'ın mesajları zamanında tüketmesi beklenir.
//!  Bu sayede bellek tükenmesi (OOM) önlenir.
//! ```

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use lazy_static::lazy_static;
use spin::Mutex;

use crate::task::task::TaskId;

/// Task başına maksimum mesaj sayısı (kuyruk dolarsa gönderim başarısız olur)
const MAX_QUEUE_SIZE: usize = 32;

/// Task'lar arası gönderilen mesaj.
#[derive(Debug, Clone)]
pub struct Message {
    /// Gönderen task ID'si
    pub sender: TaskId,
    /// Mesaj verisi (byte array)
    pub data: Vec<u8>,
}

impl Message {
    /// Yeni mesaj oluşturur.
    pub fn new(sender: TaskId, data: Vec<u8>) -> Self {
        Self { sender, data }
    }

    /// String'den mesaj oluşturur.
    pub fn from_str(sender: TaskId, s: &str) -> Self {
        Self::new(sender, s.as_bytes().to_vec())
    }

    /// Mesajı string olarak okur (UTF-8 geçerliyse).
    pub fn as_str(&self) -> Option<&str> {
        core::str::from_utf8(&self.data).ok()
    }
}

/// Tek bir task için mesaj kutusu (mailbox).
struct TaskMailbox {
    task_id: TaskId,
    messages: VecDeque<Message>,
}

impl TaskMailbox {
    fn new(task_id: TaskId) -> Self {
        Self {
            task_id,
            messages: VecDeque::new(),
        }
    }
}

/// Global mesaj kayıt defteri. Tüm task'ların mailbox'larını tutar.
struct MessageRegistry {
    mailboxes: Vec<TaskMailbox>,
}

impl MessageRegistry {
    const fn new() -> Self {
        Self {
            mailboxes: Vec::new(),
        }
    }

    /// Task için mailbox'ı getirir veya yoksa oluşturur.
    fn get_or_create_mailbox(&mut self, task_id: TaskId) -> &mut TaskMailbox {
        if let Some(pos) = self.mailboxes.iter().position(|m| m.task_id == task_id) {
            return &mut self.mailboxes[pos];
        }

        self.mailboxes.push(TaskMailbox::new(task_id));
        self.mailboxes.last_mut().unwrap()
    }

    /// Hedef task'a mesaj gönderir.
    /// Kuyruk doluysa false döner.
    fn send(&mut self, target: TaskId, message: Message) -> bool {
        let mailbox = self.get_or_create_mailbox(target);

        if mailbox.messages.len() >= MAX_QUEUE_SIZE {
            return false; // Kuyruk dolu
        }

        mailbox.messages.push_back(message);
        true
    }

    /// Task için bir mesaj alır (FIFO).
    fn receive(&mut self, task_id: TaskId) -> Option<Message> {
        let mailbox = self.get_or_create_mailbox(task_id);
        mailbox.messages.pop_front()
    }

    /// Bekleyen mesaj var mı kontrol eder.
    fn has_message(&mut self, task_id: TaskId) -> bool {
        let mailbox = self.get_or_create_mailbox(task_id);
        !mailbox.messages.is_empty()
    }
}

lazy_static! {
    /// Global Thread-Safe mesaj registry
    static ref REGISTRY: Mutex<MessageRegistry> = Mutex::new(MessageRegistry::new());
}

// ============================================================================
// PUBLIC API — Dışarıya açık mesajlaşma fonksiyonları
//
// Tüm fonksiyonlar REGISTRY Mutex'ini kısa süreliğine kilitler.
// Interrupt context'ten değil, normal task context'ten çağrılmalıdır.
//
// send_message(target, sender, data) → Byte slice gönder
// send_str(target, sender, s)        → String mesaj gönder (UTF-8)
// receive_message(task_id)           → Bir mesaj al (non-blocking, FIFO)
// has_message(task_id)               → Bekleyen mesaj var mı?
// message_count(task_id)             → Kuyruktaki mesaj sayısı
// ============================================================================

/// Hedef task'a mesaj gönderir.
pub fn send_message(target: TaskId, sender: TaskId, data: Vec<u8>) -> bool {
    let message = Message::new(sender, data);
    REGISTRY.lock().send(target, message)
}

/// Hedef task'a string mesaj gönderir.
pub fn send_str(target: TaskId, sender: TaskId, s: &str) -> bool {
    let message = Message::from_str(sender, s);
    REGISTRY.lock().send(target, message)
}

/// Mevcut task için mesaj alır (non-blocking).
pub fn receive_message(task_id: TaskId) -> Option<Message> {
    REGISTRY.lock().receive(task_id)
}

/// Bekleyen mesaj var mı kontrol eder.
pub fn has_message(task_id: TaskId) -> bool {
    REGISTRY.lock().has_message(task_id)
}

/// Bekleyen mesaj sayısını döndürür.
pub fn message_count(task_id: TaskId) -> usize {
    let mut registry = REGISTRY.lock();
    let mailbox = registry.get_or_create_mailbox(task_id);
    mailbox.messages.len()
}
