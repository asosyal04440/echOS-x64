use alloc::string::String;

pub use super::super::services::{InputCommand, InputResponse};

#[inline]
pub fn request_input_sync(app_id: u32, command: InputCommand) -> Option<InputResponse> {
    super::super::ipc::request_input_sync(app_id, command)
}

pub fn register_shortcut_sink(app_id: u32) -> Result<(), String> {
    match request_input_sync(app_id, InputCommand::RegisterShortcutSink { app_id }) {
        Some(InputResponse::Ack) => Ok(()),
        Some(InputResponse::Error(err)) => Err(err),
        _ => Err(String::from("input service unavailable")),
    }
}
