pub use super::super::services::{CaptureCommand, CaptureResponse};

#[inline]
pub fn request_capture_sync(app_id: u32, command: CaptureCommand) -> Option<CaptureResponse> {
    super::super::ipc::request_capture_sync(app_id, command)
}
