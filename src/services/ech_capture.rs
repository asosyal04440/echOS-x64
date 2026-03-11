//! Screen capture service for native desktop screenshots.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

use crate::gui::protocol::{
    AppId, DesktopPermission, PermissionState, ScreenshotEntry, ScreenshotId,
};
use crate::services::ech_display::{get_display, DisplayCommand, DisplayResponse};
use crate::services::ech_shell::{get_shell_service, ShellCommand, ShellResponse};

const MAX_CAPTURES: usize = 8;

#[derive(Clone, Debug)]
pub enum CaptureCommand {
    CaptureDesktop {
        app_id: AppId,
        label: String,
    },
    ListCaptures {
        app_id: AppId,
        max_items: usize,
    },
    GetCapture {
        app_id: AppId,
        capture_id: ScreenshotId,
    },
}

#[derive(Clone, Debug)]
pub enum CaptureResponse {
    Captured(ScreenshotEntry),
    Captures(Vec<ScreenshotEntry>),
    CaptureData {
        entry: ScreenshotEntry,
        pixels: Vec<u32>,
    },
    Error(String),
}

#[derive(Clone, Debug)]
struct CaptureRecord {
    entry: ScreenshotEntry,
    pixels: Vec<u32>,
}

pub struct EchCapture {
    running: AtomicBool,
    next_id: AtomicU64,
    captures: Mutex<Vec<CaptureRecord>>,
    command_queue: Mutex<Vec<CaptureCommand>>,
    response_queue: Mutex<Vec<CaptureResponse>>,
}

impl EchCapture {
    pub fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            next_id: AtomicU64::new(1),
            captures: Mutex::new(Vec::new()),
            command_queue: Mutex::new(Vec::new()),
            response_queue: Mutex::new(Vec::new()),
        }
    }

    pub fn start(&self) {
        self.running.store(true, Ordering::SeqCst);
        crate::serial_println!("[ECHCAPTURE] service started");
    }

    pub fn process_command(&self, command: CaptureCommand) -> CaptureResponse {
        match command {
            CaptureCommand::CaptureDesktop { app_id, label } => {
                if !permission_granted(app_id, DesktopPermission::ScreenCapture) {
                    return CaptureResponse::Error(String::from(
                        "screen capture permission denied",
                    ));
                }
                let display = match get_display().lock().clone() {
                    Some(display) => display,
                    None => return CaptureResponse::Error(String::from("display unavailable")),
                };
                match display.process_command(DisplayCommand::SnapshotDesktop) {
                    DisplayResponse::DesktopSnapshot {
                        width,
                        height,
                        pixels,
                    } => {
                        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
                        let entry = ScreenshotEntry {
                            id,
                            app_id,
                            label,
                            width,
                            height,
                        };
                        let mut captures = self.captures.lock();
                        captures.push(CaptureRecord {
                            entry: entry.clone(),
                            pixels,
                        });
                        while captures.len() > MAX_CAPTURES {
                            captures.remove(0);
                        }
                        CaptureResponse::Captured(entry)
                    }
                    DisplayResponse::Error(err) => CaptureResponse::Error(err),
                    _ => {
                        CaptureResponse::Error(String::from("display returned unexpected response"))
                    }
                }
            }
            CaptureCommand::ListCaptures { app_id, max_items } => {
                if !permission_granted(app_id, DesktopPermission::ScreenCapture) {
                    return CaptureResponse::Error(String::from(
                        "screen capture permission denied",
                    ));
                }
                let items = self
                    .captures
                    .lock()
                    .iter()
                    .rev()
                    .take(max_items.max(1))
                    .map(|record| record.entry.clone())
                    .collect();
                CaptureResponse::Captures(items)
            }
            CaptureCommand::GetCapture { app_id, capture_id } => {
                if !permission_granted(app_id, DesktopPermission::ScreenCapture) {
                    return CaptureResponse::Error(String::from(
                        "screen capture permission denied",
                    ));
                }
                let captures = self.captures.lock();
                match captures.iter().find(|record| record.entry.id == capture_id) {
                    Some(record) => CaptureResponse::CaptureData {
                        entry: record.entry.clone(),
                        pixels: record.pixels.clone(),
                    },
                    None => CaptureResponse::Error(String::from("capture not found")),
                }
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
    static ref ECH_CAPTURE: Arc<EchCapture> = Arc::new(EchCapture::new());
}

pub fn init() {
    ECH_CAPTURE.start();
    crate::serial_println!("[ECHCAPTURE] initialized");
}

pub fn get_capture_service() -> Arc<EchCapture> {
    Arc::clone(&ECH_CAPTURE)
}

pub fn service_task() -> ! {
    let svc = get_capture_service();
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
