//! Dialog request broker for shell-managed file and message dialogs.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

use crate::gui::protocol::{
    AppId, DesktopPermission, DialogId, DialogKind, DialogRequest, DialogResult, DialogSelection,
    PermissionState,
};
use crate::services::ech_shell::{get_shell_service, ShellCommand, ShellResponse};

#[derive(Clone, Debug)]
pub enum DialogCommand {
    Request {
        app_id: AppId,
        kind: DialogKind,
        title: String,
        message: String,
        path_hint: String,
    },
    ListPending {
        max_items: usize,
    },
    Resolve {
        dialog_id: DialogId,
        selection: DialogSelection,
    },
    PollResult {
        app_id: AppId,
        dialog_id: DialogId,
    },
}

#[derive(Clone, Debug)]
pub enum DialogResponse {
    Requested(DialogId),
    Pending(Vec<DialogRequest>),
    Result(Option<DialogResult>),
    Ack,
    Error(String),
}

pub struct EchDialogs {
    running: AtomicBool,
    next_id: AtomicU64,
    pending: Mutex<Vec<DialogRequest>>,
    resolved: Mutex<Vec<DialogResult>>,
    command_queue: Mutex<Vec<DialogCommand>>,
    response_queue: Mutex<Vec<DialogResponse>>,
}

impl EchDialogs {
    pub fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            next_id: AtomicU64::new(1),
            pending: Mutex::new(Vec::new()),
            resolved: Mutex::new(Vec::new()),
            command_queue: Mutex::new(Vec::new()),
            response_queue: Mutex::new(Vec::new()),
        }
    }

    pub fn start(&self) {
        self.running.store(true, Ordering::SeqCst);
        crate::serial_println!("[ECHDIALOGS] service started");
    }

    pub fn send_command(&self, command: DialogCommand) {
        self.command_queue.lock().push(command);
    }

    pub fn receive_response(&self) -> Option<DialogResponse> {
        self.response_queue.lock().pop()
    }

    pub fn process_command(&self, command: DialogCommand) -> DialogResponse {
        match command {
            DialogCommand::Request {
                app_id,
                kind,
                title,
                message,
                path_hint,
            } => {
                if matches!(
                    kind,
                    DialogKind::OpenFile | DialogKind::SaveFile | DialogKind::PickFolder
                ) && !permission_granted(app_id, DesktopPermission::FileDialogs)
                {
                    return DialogResponse::Error(String::from("file dialog permission denied"));
                }
                let id = self.next_id.fetch_add(1, Ordering::SeqCst);
                self.pending.lock().push(DialogRequest {
                    id,
                    app_id,
                    kind,
                    title,
                    message,
                    path_hint,
                });
                DialogResponse::Requested(id)
            }
            DialogCommand::ListPending { max_items } => {
                let max_items = max_items.max(1);
                let requests = self
                    .pending
                    .lock()
                    .iter()
                    .take(max_items)
                    .cloned()
                    .collect();
                DialogResponse::Pending(requests)
            }
            DialogCommand::Resolve {
                dialog_id,
                selection,
            } => {
                let mut pending = self.pending.lock();
                let Some(index) = pending.iter().position(|request| request.id == dialog_id) else {
                    return DialogResponse::Error(String::from("dialog request not found"));
                };
                let request = pending.remove(index);
                self.resolved.lock().push(DialogResult {
                    id: dialog_id,
                    app_id: request.app_id,
                    selection,
                });
                DialogResponse::Ack
            }
            DialogCommand::PollResult { app_id, dialog_id } => {
                let mut resolved = self.resolved.lock();
                let result = resolved
                    .iter()
                    .position(|result| result.id == dialog_id && result.app_id == app_id)
                    .map(|index| resolved.remove(index));
                DialogResponse::Result(result)
            }
        }
    }

    pub fn run_service(&self) {
        while self.running.load(Ordering::SeqCst) {
            let commands = {
                let mut queue = self.command_queue.lock();
                core::mem::take(&mut *queue)
            };

            for command in commands {
                let response = self.process_command(command);
                self.response_queue.lock().push(response);
            }

            for _ in 0..1000 {
                core::hint::spin_loop();
            }
        }
    }
}

lazy_static::lazy_static! {
    static ref ECH_DIALOGS: Arc<EchDialogs> = Arc::new(EchDialogs::new());
}

pub fn init() {
    ECH_DIALOGS.start();
    crate::serial_println!("[ECHDIALOGS] initialized");
}

pub fn get_dialogs_service() -> Arc<EchDialogs> {
    Arc::clone(&ECH_DIALOGS)
}

pub fn service_task() -> ! {
    let svc = get_dialogs_service();
    svc.run_service();
    loop {
        core::hint::spin_loop();
    }
}

fn permission_granted(app_id: AppId, permission: DesktopPermission) -> bool {
    matches!(
        get_shell_service().process_command(ShellCommand::GetPermission { app_id, permission }),
        ShellResponse::Permission(PermissionState::Granted)
    )
}
