//! # Kurtarma Modülleri
//!
//! Modüle özgü kurtarma uygulamaları.

pub mod memory;
pub mod driver;
pub mod fs;
pub mod network;

use core::sync::atomic::{AtomicBool, Ordering};

pub fn init() {
    crate::serial_println!("[RECOVERY_MODULES] Başlatıldı");
}
