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

pub mod at_spi;
pub mod display_atomic;
pub mod ech_audio;
pub mod ech_capture;
pub mod ech_clipboard;
pub mod ech_dialogs;
pub mod ech_display;
pub mod ech_input;
pub mod ech_notifications;
pub mod ech_shell;
pub mod ech_store;

pub use at_spi::{get_bridge as get_at_spi_bridge, AtSpiBridge, AtSpiEvent};
pub use ech_audio::{
    AudioChannel, AudioCommand, AudioEffect, AudioFormat, AudioResponse, EchAudio,
};
pub use ech_capture::{CaptureCommand, CaptureResponse, EchCapture};
pub use ech_clipboard::{ClipboardCommand, ClipboardResponse, EchClipboard};
pub use ech_dialogs::{DialogCommand, DialogResponse, EchDialogs};
pub use ech_display::{DisplayCommand, DisplayResponse, EchDisplay};
pub use ech_input::{EchInput, InputCommand, InputResponse};
pub use ech_notifications::{EchNotifications, NotificationCommand, NotificationResponse};
pub use ech_shell::{EchShell, ShellCommand, ShellResponse};
pub use ech_store::{get_store, EchStore, FileEntry, StoreCommand, StoreResponse};

use super::ipc::{publish_service_endpoint, ServiceEndpointRegistration, ServiceId};
use super::runtime_layer::{launch_contract, service_parity_contract};
use super::{serial_println, task};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ServiceDeploymentMode {
    KernelResident,
    IsolatedProcess,
}

#[derive(Clone, Copy)]
struct ServiceSpawnSpec {
    id: ServiceId,
    slug: &'static str,
    title: &'static str,
    kernel_entry: fn() -> !,
    ensure_kernel_ready: fn(),
}

fn deployment_mode(service_name: &str) -> ServiceDeploymentMode {
    if launch_contract::service_process_available(service_name) {
        ServiceDeploymentMode::IsolatedProcess
    } else {
        ServiceDeploymentMode::KernelResident
    }
}

fn ensure_display_kernel_ready() {
    ech_display::init();
    if let Some(display) = ech_display::get_display().lock().clone() {
        publish_service_endpoint(
            ServiceId::EchDisplay,
            ServiceEndpointRegistration::Display(display),
        );
    }
}

fn ensure_input_kernel_ready() {
    ech_input::init();
    publish_service_endpoint(
        ServiceId::EchInput,
        ServiceEndpointRegistration::Input(ech_input::get_input()),
    );
}

fn ensure_audio_kernel_ready() {
    ech_audio::init();
    publish_service_endpoint(
        ServiceId::EchAudio,
        ServiceEndpointRegistration::Audio(ech_audio::get_audio()),
    );
}

fn ensure_store_kernel_ready() {
    ech_store::init();
    publish_service_endpoint(
        ServiceId::EchStore,
        ServiceEndpointRegistration::Store(ech_store::get_store()),
    );
}

fn ensure_shell_kernel_ready() {
    ech_shell::init();
    publish_service_endpoint(
        ServiceId::EchShell,
        ServiceEndpointRegistration::Shell(ech_shell::get_shell_service()),
    );
}

fn ensure_notifications_kernel_ready() {
    ech_notifications::init();
    publish_service_endpoint(
        ServiceId::EchNotifications,
        ServiceEndpointRegistration::Notifications(ech_notifications::get_notifications_service()),
    );
}

fn ensure_clipboard_kernel_ready() {
    ech_clipboard::init();
    publish_service_endpoint(
        ServiceId::EchClipboard,
        ServiceEndpointRegistration::Clipboard(ech_clipboard::get_clipboard_service()),
    );
}

fn ensure_dialogs_kernel_ready() {
    ech_dialogs::init();
    publish_service_endpoint(
        ServiceId::EchDialogs,
        ServiceEndpointRegistration::Dialogs(ech_dialogs::get_dialogs_service()),
    );
}

fn ensure_capture_kernel_ready() {
    ech_capture::init();
    publish_service_endpoint(
        ServiceId::EchCapture,
        ServiceEndpointRegistration::Capture(ech_capture::get_capture_service()),
    );
}

fn spawn_service_slot(
    spec: ServiceSpawnSpec,
    priority: task::task::Priority,
) -> Option<launch_contract::RuntimeHandle> {
    let strict_full_parity = service_parity_contract::strict_full_parity_mode_enabled();
    match deployment_mode(spec.slug) {
        ServiceDeploymentMode::IsolatedProcess => {
            match launch_contract::spawn_service_process_runtime(
                spec.id, spec.slug, spec.title, priority,
            ) {
                Ok(runtime) => Some(runtime),
                Err(error) => {
                    serial_println!(
                        "[SERVICES] {} process bootstrap failed ({})",
                        spec.title,
                        error
                    );
                    if strict_full_parity {
                        serial_println!(
                            "[SERVICES] strict full-parity mode denies kernel fallback for {}",
                            spec.title
                        );
                        None
                    } else {
                        serial_println!(
                            "[SERVICES] reverting {} to kernel-resident path",
                            spec.title
                        );
                        (spec.ensure_kernel_ready)();
                        Some(launch_contract::spawn_service_runtime(
                            spec.id,
                            spec.slug,
                            spec.title,
                            spec.kernel_entry,
                            priority,
                        ))
                    }
                }
            }
        }
        ServiceDeploymentMode::KernelResident => {
            if strict_full_parity {
                serial_println!(
                    "[SERVICES] strict full-parity mode suppresses kernel-resident {}",
                    spec.title
                );
                None
            } else {
                Some(launch_contract::spawn_service_runtime(
                    spec.id,
                    spec.slug,
                    spec.title,
                    spec.kernel_entry,
                    priority,
                ))
            }
        }
    }
}

/// Sistem servislerini başlatır
pub fn init() {
    serial_println!("[SERVICES] Initializing system services...");
    let parity = service_parity_contract::refresh_full_parity_mode();
    serial_println!(
        "[SERVICES] full-parity strict={} packaged={}/{} live_user_process={}/{}",
        parity.strict_mode_enabled,
        parity.packaged_service_slots,
        parity.required_services,
        parity.live_user_process_slots,
        parity.required_services
    );

    at_spi::init();
    for (service_name, title, ensure_ready) in [
        (
            "ech_display",
            "EchDisplay",
            ensure_display_kernel_ready as fn(),
        ),
        ("ech_input", "EchInput", ensure_input_kernel_ready as fn()),
        ("ech_audio", "EchAudio", ensure_audio_kernel_ready as fn()),
        (
            "ech_capture",
            "EchCapture",
            ensure_capture_kernel_ready as fn(),
        ),
        (
            "ech_clipboard",
            "EchClipboard",
            ensure_clipboard_kernel_ready as fn(),
        ),
        (
            "ech_dialogs",
            "EchDialogs",
            ensure_dialogs_kernel_ready as fn(),
        ),
        (
            "ech_notifications",
            "EchNotifications",
            ensure_notifications_kernel_ready as fn(),
        ),
        ("ech_shell", "EchShell", ensure_shell_kernel_ready as fn()),
        ("ech_store", "EchStore", ensure_store_kernel_ready as fn()),
    ] {
        if deployment_mode(service_name) == ServiceDeploymentMode::KernelResident {
            ensure_ready();
        } else {
            serial_println!(
                "[SERVICES] {} reserved for isolated service-process bootstrap",
                title
            );
        }
    }

    serial_println!("[SERVICES] System services initialized");
}

pub fn spawn_service_tasks() {
    let parity = service_parity_contract::refresh_full_parity_mode();
    serial_println!(
        "[SERVICES] spawning service tasks with strict full-parity mode={}",
        parity.strict_mode_enabled
    );
    let priority = task::task::Priority::Low;
    for spec in [
        ServiceSpawnSpec {
            id: ServiceId::EchDisplay,
            slug: "ech_display",
            title: "EchDisplay",
            kernel_entry: ech_display::service_task,
            ensure_kernel_ready: ensure_display_kernel_ready,
        },
        ServiceSpawnSpec {
            id: ServiceId::EchInput,
            slug: "ech_input",
            title: "EchInput",
            kernel_entry: ech_input::service_task,
            ensure_kernel_ready: ensure_input_kernel_ready,
        },
        ServiceSpawnSpec {
            id: ServiceId::EchAudio,
            slug: "ech_audio",
            title: "EchAudio",
            kernel_entry: ech_audio::service_task,
            ensure_kernel_ready: ensure_audio_kernel_ready,
        },
        ServiceSpawnSpec {
            id: ServiceId::EchCapture,
            slug: "ech_capture",
            title: "EchCapture",
            kernel_entry: ech_capture::service_task,
            ensure_kernel_ready: ensure_capture_kernel_ready,
        },
        ServiceSpawnSpec {
            id: ServiceId::EchClipboard,
            slug: "ech_clipboard",
            title: "EchClipboard",
            kernel_entry: ech_clipboard::service_task,
            ensure_kernel_ready: ensure_clipboard_kernel_ready,
        },
        ServiceSpawnSpec {
            id: ServiceId::EchDialogs,
            slug: "ech_dialogs",
            title: "EchDialogs",
            kernel_entry: ech_dialogs::service_task,
            ensure_kernel_ready: ensure_dialogs_kernel_ready,
        },
        ServiceSpawnSpec {
            id: ServiceId::EchNotifications,
            slug: "ech_notifications",
            title: "EchNotifications",
            kernel_entry: ech_notifications::service_task,
            ensure_kernel_ready: ensure_notifications_kernel_ready,
        },
        ServiceSpawnSpec {
            id: ServiceId::EchShell,
            slug: "ech_shell",
            title: "EchShell",
            kernel_entry: ech_shell::service_task,
            ensure_kernel_ready: ensure_shell_kernel_ready,
        },
        ServiceSpawnSpec {
            id: ServiceId::EchStore,
            slug: "ech_store",
            title: "EchStore",
            kernel_entry: ech_store::service_task,
            ensure_kernel_ready: ensure_store_kernel_ready,
        },
    ] {
        if let Some(runtime) = spawn_service_slot(spec, priority) {
            if let Some(task_id) = runtime.task_id {
                super::ipc::register_service_runtime_task(spec.id, task_id);
            }
        }
    }
}
