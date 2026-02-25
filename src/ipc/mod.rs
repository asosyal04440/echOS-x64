//! # echOS IPC (Inter-Process Communication)
//!
//! Task'lar arası iletişim altyapısı.
//! Mesaj gönderme ve alma ile channel-based iletişim.

/// Mesaj yapısı ve gönderme/alma fonksiyonları
pub mod message;

/// İletişim kanalları
pub mod channel;

pub use message::{has_message, receive_message, send_message, Message};
