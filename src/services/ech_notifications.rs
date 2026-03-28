//! Notification service for shell toasts and history.

use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

use crate::gui::protocol::{
    DesktopPermission, NotificationEntry, NotificationRequest, PermissionState,
};
use crate::ipc::request_shell_sync;
use crate::services::display_atomic::MailboxRing;
use crate::services::ech_shell::{ShellCommand, ShellResponse};

const MAX_NOTIFICATIONS: usize = 32;
const NOTIFICATION_COMMAND_QUEUE_CAPACITY: usize = 128;
const NOTIFICATION_RESPONSE_QUEUE_CAPACITY: usize = 128;

#[derive(Clone, Debug)]
pub enum NotificationCommand {
    Push(NotificationRequest),
    List {
        app_id: AppId,
        include_read: bool,
        max_items: usize,
    },
    MarkRead {
        app_id: AppId,
        id: u64,
    },
    Clear {
        app_id: AppId,
    },
}

#[derive(Clone, Debug)]
pub enum NotificationResponse {
    Ack,
    NotificationId(u64),
    Notifications(Vec<NotificationEntry>),
    Error(String),
}

pub struct EchNotifications {
    running: AtomicBool,
    next_id: AtomicU64,
    entries: Mutex<Vec<NotificationEntry>>,
    command_queue: MailboxRing<NotificationCommand>,
    response_queue: MailboxRing<NotificationResponse>,
}

impl EchNotifications {
    pub fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            next_id: AtomicU64::new(1),
            entries: Mutex::new(Vec::new()),
            command_queue: MailboxRing::with_capacity_pow2(NOTIFICATION_COMMAND_QUEUE_CAPACITY),
            response_queue: MailboxRing::with_capacity_pow2(NOTIFICATION_RESPONSE_QUEUE_CAPACITY),
        }
    }

    pub fn start(&self) {
        self.running.store(true, Ordering::SeqCst);
        crate::serial_println!("[ECHNOTIFY] service started");
    }

    pub fn send_command(&self, command: NotificationCommand) -> bool {
        self.command_queue.try_push(command).is_ok()
    }

    pub fn receive_response(&self) -> Option<NotificationResponse> {
        self.response_queue.pop()
    }

    pub fn process_command(&self, command: NotificationCommand) -> NotificationResponse {
        match command {
            NotificationCommand::Push(request) => {
                if !permission_granted(request.app_id, DesktopPermission::Notifications) {
                    return NotificationResponse::Error(String::from(
                        "notification permission denied",
                    ));
                }
                let id = self.next_id.fetch_add(1, Ordering::SeqCst);
                let mut entries = self.entries.lock();
                entries.push(NotificationEntry {
                    id,
                    app_id: request.app_id,
                    source_name: resolve_source_name(request.app_id),
                    title: request.title,
                    message: request.message,
                    level: request.level,
                    read: false,
                    timestamp_ticks: id,
                    action_label: request.action_label,
                });
                while entries.len() > MAX_NOTIFICATIONS {
                    entries.remove(0);
                }
                let _ = request_shell_sync(
                    request.app_id,
                    ShellCommand::NoteNotification {
                        app_id: request.app_id,
                    },
                );
                NotificationResponse::NotificationId(id)
            }
            NotificationCommand::List {
                app_id: _,
                include_read,
                max_items,
            } => {
                let max_items = max_items.max(1);
                let entries = self
                    .entries
                    .lock()
                    .iter()
                    .filter(|entry| include_read || !entry.read)
                    .rev()
                    .take(max_items)
                    .cloned()
                    .collect();
                NotificationResponse::Notifications(entries)
            }
            NotificationCommand::MarkRead { app_id, id } => {
                let mut entries = self.entries.lock();
                if let Some(entry) = entries.iter_mut().find(|entry| entry.id == id) {
                    entry.read = true;
                    let _ = request_shell_sync(
                        app_id,
                        ShellCommand::ClearNotifications {
                            app_id: Some(app_id),
                        },
                    );
                    NotificationResponse::Ack
                } else {
                    NotificationResponse::Error(String::from("notification not found"))
                }
            }
            NotificationCommand::Clear { app_id } => {
                self.entries.lock().clear();
                let _ = request_shell_sync(
                    app_id,
                    ShellCommand::ClearNotifications {
                        app_id: if app_id == 1 { None } else { Some(app_id) },
                    },
                );
                NotificationResponse::Ack
            }
        }
    }

    pub fn run_service(&self) {
        while self.running.load(Ordering::SeqCst) {
            while let Some(command) = self.command_queue.pop() {
                let response = self.process_command(command);
                let _ = self.response_queue.push_overwrite(response);
            }

            for _ in 0..1000 {
                core::hint::spin_loop();
            }
        }
    }
}

lazy_static::lazy_static! {
    static ref ECH_NOTIFICATIONS: Arc<EchNotifications> = Arc::new(EchNotifications::new());
}

pub fn init() {
    ECH_NOTIFICATIONS.start();
    crate::serial_println!("[ECHNOTIFY] initialized");
}

pub fn get_notifications_service() -> Arc<EchNotifications> {
    Arc::clone(&ECH_NOTIFICATIONS)
}

pub fn service_task() -> ! {
    let svc = get_notifications_service();
    svc.run_service();
    loop {
        core::hint::spin_loop();
    }
}

use crate::gui::protocol::AppId;

fn permission_granted(app_id: AppId, permission: DesktopPermission) -> bool {
    matches!(
        request_shell_sync(app_id, ShellCommand::GetPermission { app_id, permission }),
        Some(ShellResponse::Permission(PermissionState::Granted))
    )
}

fn resolve_source_name(app_id: AppId) -> String {
    match request_shell_sync(app_id, ShellCommand::ListApps) {
        Some(ShellResponse::Apps(apps)) => apps
            .into_iter()
            .find(|entry| entry.app_id == app_id)
            .map(|entry| entry.name)
            .unwrap_or_else(|| format!("App {}", app_id)),
        _ => format!("App {}", app_id),
    }
}
