//! Service-backed clipboard for the native desktop stack.

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

use crate::gui::protocol::{AppId, ClipboardPayload, DesktopPermission, PermissionState};
use crate::ipc::request_shell_sync;
use crate::services::display_atomic::MailboxRing;
use crate::services::ech_shell::{get_shell_service, ShellCommand, ShellResponse};

const MAX_HISTORY_ITEMS: usize = 16;
const CLIPBOARD_COMMAND_QUEUE_CAPACITY: usize = 128;
const CLIPBOARD_RESPONSE_QUEUE_CAPACITY: usize = 128;

#[derive(Clone, Debug)]
pub enum ClipboardCommand {
    Set {
        app_id: AppId,
        payload: ClipboardPayload,
    },
    GetCurrent {
        app_id: AppId,
    },
    GetHistory {
        app_id: AppId,
        max_items: usize,
    },
    Clear {
        app_id: AppId,
    },
}

#[derive(Clone, Debug)]
pub enum ClipboardResponse {
    Ack,
    Current(ClipboardPayload),
    History(Vec<ClipboardPayload>),
    Error(String),
}

pub struct EchClipboard {
    running: AtomicBool,
    current: Mutex<ClipboardPayload>,
    history: Mutex<VecDeque<ClipboardPayload>>,
    command_queue: MailboxRing<ClipboardCommand>,
    response_queue: MailboxRing<ClipboardResponse>,
}

impl EchClipboard {
    pub fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            current: Mutex::new(ClipboardPayload::Empty),
            history: Mutex::new(VecDeque::new()),
            command_queue: MailboxRing::with_capacity_pow2(CLIPBOARD_COMMAND_QUEUE_CAPACITY),
            response_queue: MailboxRing::with_capacity_pow2(CLIPBOARD_RESPONSE_QUEUE_CAPACITY),
        }
    }

    pub fn start(&self) {
        self.running.store(true, Ordering::SeqCst);
        crate::serial_println!("[ECHCLIPBOARD] service started");
    }

    pub fn send_command(&self, command: ClipboardCommand) -> bool {
        self.command_queue.try_push(command).is_ok()
    }

    pub fn receive_response(&self) -> Option<ClipboardResponse> {
        self.response_queue.pop()
    }

    pub fn process_command(&self, command: ClipboardCommand) -> ClipboardResponse {
        match command {
            ClipboardCommand::Set { app_id, payload } => {
                if matches!(payload, ClipboardPayload::Empty) {
                    return ClipboardResponse::Error(String::from("clipboard payload is empty"));
                }
                if !permission_granted(app_id, DesktopPermission::ClipboardWrite) {
                    return ClipboardResponse::Error(String::from(
                        "clipboard write permission denied",
                    ));
                }
                *self.current.lock() = payload.clone();
                let mut history = self.history.lock();
                history.push_front(payload);
                while history.len() > MAX_HISTORY_ITEMS {
                    history.pop_back();
                }
                let _ = get_shell_service().process_command(ShellCommand::SetClipboardHistoryLen {
                    len: history.len() as u32,
                });
                ClipboardResponse::Ack
            }
            ClipboardCommand::GetCurrent { app_id } => {
                if !permission_granted(app_id, DesktopPermission::ClipboardRead) {
                    return ClipboardResponse::Error(String::from(
                        "clipboard read permission denied",
                    ));
                }
                ClipboardResponse::Current(self.current.lock().clone())
            }
            ClipboardCommand::GetHistory { app_id, max_items } => {
                if !permission_granted(app_id, DesktopPermission::ClipboardRead) {
                    return ClipboardResponse::Error(String::from(
                        "clipboard read permission denied",
                    ));
                }
                let max_items = max_items.max(1);
                let items = self
                    .history
                    .lock()
                    .iter()
                    .take(max_items)
                    .cloned()
                    .collect();
                ClipboardResponse::History(items)
            }
            ClipboardCommand::Clear { app_id } => {
                if !permission_granted(app_id, DesktopPermission::ClipboardWrite) {
                    return ClipboardResponse::Error(String::from(
                        "clipboard clear permission denied",
                    ));
                }
                *self.current.lock() = ClipboardPayload::Empty;
                self.history.lock().clear();
                let _ = get_shell_service()
                    .process_command(ShellCommand::SetClipboardHistoryLen { len: 0 });
                ClipboardResponse::Ack
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

static ECH_CLIPBOARD: spin::Lazy<Arc<EchClipboard>> = spin::Lazy::new(|| Arc::new(EchClipboard::new()));

pub fn init() {
    ECH_CLIPBOARD.start();
    crate::serial_println!("[ECHCLIPBOARD] initialized");
}

pub fn get_clipboard_service() -> Arc<EchClipboard> {
    Arc::clone(&ECH_CLIPBOARD)
}

pub fn service_task() -> ! {
    let svc = get_clipboard_service();
    svc.run_service();
    loop {
        core::hint::spin_loop();
    }
}

fn permission_granted(app_id: AppId, permission: DesktopPermission) -> bool {
    matches!(
        request_shell_sync(app_id, ShellCommand::GetPermission { app_id, permission }),
        Some(ShellResponse::Permission(PermissionState::Granted))
    )
}
