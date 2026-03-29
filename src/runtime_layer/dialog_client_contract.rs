use alloc::string::String;
use alloc::vec::Vec;

use super::super::gui::protocol::{DialogId, DialogRequest, DialogSelection};
pub use super::super::services::{DialogCommand, DialogResponse};

#[inline]
pub fn request_dialog_sync(app_id: u32, command: DialogCommand) -> Option<DialogResponse> {
    super::super::ipc::request_dialog_sync(app_id, command)
}

pub fn list_pending_dialogs(max_items: usize) -> Result<Vec<DialogRequest>, String> {
    match request_dialog_sync(0, DialogCommand::ListPending { max_items }) {
        Some(DialogResponse::Pending(requests)) => Ok(requests),
        Some(DialogResponse::Error(err)) => Err(err),
        _ => Err(String::from("dialog service unavailable")),
    }
}

pub fn resolve_dialog(dialog_id: DialogId, selection: DialogSelection) -> Result<(), String> {
    match request_dialog_sync(
        0,
        DialogCommand::Resolve {
            dialog_id,
            selection,
        },
    ) {
        Some(DialogResponse::Ack) => Ok(()),
        Some(DialogResponse::Error(err)) => Err(err),
        _ => Err(String::from("dialog service unavailable")),
    }
}
