pub use super::super::services::{NotificationCommand, NotificationResponse};

#[inline]
pub fn request_notification_sync(
    app_id: u32,
    command: NotificationCommand,
) -> Option<NotificationResponse> {
    super::super::ipc::request_notification_sync(app_id, command)
}
