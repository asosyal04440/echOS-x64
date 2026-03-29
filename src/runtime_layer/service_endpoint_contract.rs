pub const SERVICE_ENDPOINT_CONTRACT_ROOTS: &[&str] = &["ipc"];

pub use super::super::ipc::{
    describe_service, endpoint_generation_for_service, grant_service_mailbox_regions,
    heartbeat_user_service_endpoint, map_shared_region, open_service_handle,
    publish_user_service_endpoint, receive_notification_user_request,
    send_notification_user_response, CapabilityRights, ServiceError, ServiceId,
};
