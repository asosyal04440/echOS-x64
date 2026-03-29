//! Legacy service compatibility shell.
pub const SERVICE_API_COMPATIBILITY_SURFACES: &[&str] = &[
    "ipc",
    "package_registry_contract",
    "process_broker_contract",
    "service_parity_contract",
];

#[deprecated(
    note = "use explicit runtime_layer contracts or ipc service entrypoints instead of the broad service_api facade for new internal code"
)]
pub use super::super::ipc::{
    endpoint_generation_for_service, open_service_handle, publish_service_endpoint,
    request_audio_sync, request_capture_sync, request_clipboard_sync, request_dialog_sync,
    request_display_sync, request_input_sync, request_notification_sync, request_shell_sync,
    request_store_sync, ServiceEndpointRegistration, ServiceId,
};
#[deprecated(
    note = "use service_parity_contract, package_registry_contract, or process_broker_contract instead of the broad service_api control-plane facade"
)]
pub use super::package_registry_contract::request_package_registry_sync;
#[deprecated(
    note = "use service_parity_contract, package_registry_contract, or process_broker_contract instead of the broad service_api control-plane facade"
)]
pub use super::package_registry_contract::{PackageRegistryCommand, PackageRegistryResponse};
#[deprecated(
    note = "use service_parity_contract, package_registry_contract, or process_broker_contract instead of the broad service_api control-plane facade"
)]
pub use super::process_broker_contract::request_process_broker_sync;
#[deprecated(
    note = "use service_parity_contract, package_registry_contract, or process_broker_contract instead of the broad service_api control-plane facade"
)]
pub use super::process_broker_contract::{ProcessBrokerCommand, ProcessBrokerResponse};
#[deprecated(
    note = "use service_parity_contract, package_registry_contract, or process_broker_contract instead of the broad service_api control-plane facade"
)]
pub use super::service_parity_contract::{
    describe_service, refresh_full_parity_mode, request_directory_sync, service_parity_status,
    strict_full_parity_mode_enabled, DirectoryCommand, DirectoryResponse, ServiceClass,
    ServiceDescriptor, ServiceParityStatus,
};
