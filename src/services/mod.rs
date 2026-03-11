//! # echOS Sistem Servisleri (System Services)
//!
//! Faz 3'te tanıtılan sistem servisleri. Her servis ayrı bir kernel görevi olarak çalışır.
//!
//! ## Servisler
//!
//! - **EchDisplay**: Compositor servisi, pencere çizimi ve GPU yönetimi
//! - **EchInput**: Girdi servisi, odaklanmış giriş dağılımı
//! - **EchAudio**: Ses servisi, VirtIO ses desteği
//! - **EchStore**: Depolama servisi, FAT32 VFS entegrasyonu
//!
//! ## Mimari
//!
//! Her servis:
//! - Ayrı kernel görevi olarak çalışır
//! - IPC üzerinden uygulamalarla iletişim kurar
//! - Mutex ile çoklu erişim güvenliği sağlar
//! - Lazy static ile başlatılır

pub mod ech_audio;
pub mod at_spi;
pub mod display_atomic;
pub mod ech_capture;
pub mod ech_clipboard;
pub mod ech_dialogs;
pub mod ech_display;
pub mod ech_input;
pub mod ech_notifications;
pub mod ech_shell;
pub mod ech_store;

pub use ech_audio::{
    AudioChannel, AudioCommand, AudioEffect, AudioFormat, AudioResponse, EchAudio,
};
pub use at_spi::{get_bridge as get_at_spi_bridge, AtSpiBridge, AtSpiEvent};
pub use ech_capture::{CaptureCommand, CaptureResponse, EchCapture};
pub use ech_clipboard::{ClipboardCommand, ClipboardResponse, EchClipboard};
pub use ech_dialogs::{DialogCommand, DialogResponse, EchDialogs};
pub use ech_display::{DisplayCommand, DisplayResponse, EchDisplay};
pub use ech_input::{EchInput, InputCommand, InputResponse};
pub use ech_notifications::{EchNotifications, NotificationCommand, NotificationResponse};
pub use ech_shell::{EchShell, ShellCommand, ShellResponse};
pub use ech_store::{get_store, EchStore, FileEntry, StoreCommand, StoreResponse};

/// Sistem servislerini başlatır
pub fn init() {
    crate::serial_println!("[SERVICES] Initializing system services...");

    ech_display::init();
    ech_input::init();
    ech_audio::init();
    at_spi::init();
    ech_capture::init();
    ech_clipboard::init();
    ech_dialogs::init();
    ech_notifications::init();
    ech_shell::init();
    ech_store::init();

    crate::serial_println!("[SERVICES] System services initialized");
}

pub fn spawn_service_tasks() {
    crate::task::scheduler::spawn_with_priority(
        ech_display::service_task,
        crate::task::task::Priority::Low,
        "ech_display",
    );
    crate::task::scheduler::spawn_with_priority(
        ech_input::service_task,
        crate::task::task::Priority::Low,
        "ech_input",
    );
    crate::task::scheduler::spawn_with_priority(
        ech_audio::service_task,
        crate::task::task::Priority::Low,
        "ech_audio",
    );
    crate::task::scheduler::spawn_with_priority(
        ech_capture::service_task,
        crate::task::task::Priority::Low,
        "ech_capture",
    );
    crate::task::scheduler::spawn_with_priority(
        ech_clipboard::service_task,
        crate::task::task::Priority::Low,
        "ech_clipboard",
    );
    crate::task::scheduler::spawn_with_priority(
        ech_dialogs::service_task,
        crate::task::task::Priority::Low,
        "ech_dialogs",
    );
    crate::task::scheduler::spawn_with_priority(
        ech_notifications::service_task,
        crate::task::task::Priority::Low,
        "ech_notifications",
    );
    crate::task::scheduler::spawn_with_priority(
        ech_shell::service_task,
        crate::task::task::Priority::Low,
        "ech_shell",
    );
    crate::task::scheduler::spawn_with_priority(
        ech_store::service_task,
        crate::task::task::Priority::Low,
        "ech_store",
    );
}
