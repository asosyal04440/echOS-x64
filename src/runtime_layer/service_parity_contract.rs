pub const SERVICE_PARITY_CONTRACT_ROOTS: &[&str] = &["service_control"];

pub use super::service_control::{
    describe_service, refresh_full_parity_mode, request_directory_sync, service_parity_status,
    strict_full_parity_mode_enabled, DirectoryCommand, DirectoryResponse, ServiceClass,
    ServiceDescriptor, ServiceParityStatus,
};
