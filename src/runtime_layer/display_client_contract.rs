use alloc::string::String;

use super::super::gui::theme::ThemeMode;
pub use super::super::services::{DisplayCommand, DisplayResponse};

#[inline]
pub fn request_display_sync(app_id: u32, command: DisplayCommand) -> Option<DisplayResponse> {
    super::super::ipc::request_display_sync(app_id, command)
}

pub fn set_theme_mode(mode: ThemeMode) -> Result<(), String> {
    match request_display_sync(0, DisplayCommand::SetThemeMode { mode }) {
        Some(DisplayResponse::Ack) => Ok(()),
        Some(DisplayResponse::Error(err)) => Err(err),
        _ => Err(String::from("display service unavailable")),
    }
}
