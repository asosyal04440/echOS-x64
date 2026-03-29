pub use super::super::services::{ClipboardCommand, ClipboardResponse};

#[inline]
pub fn request_clipboard_sync(app_id: u32, command: ClipboardCommand) -> Option<ClipboardResponse> {
    super::super::ipc::request_clipboard_sync(app_id, command)
}
