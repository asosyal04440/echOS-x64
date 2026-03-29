use super::*;
use lazy_static::lazy_static;

use super::super::super::kernel::tasking;
use super::super::super::runtime_layer::service_control;
use super::super::super::serial_println;
use super::super::super::services::{
    AudioCommand, AudioResponse, CaptureCommand, CaptureResponse, ClipboardCommand,
    ClipboardResponse, DialogCommand, DialogResponse, DisplayCommand, DisplayResponse,
    InputCommand, InputResponse, NotificationCommand, NotificationResponse, ShellCommand,
    ShellResponse, StoreCommand, StoreResponse,
};

lazy_static! {
    static ref SERVICE_IPC: ServiceIpcManager = ServiceIpcManager::new();
}

pub fn init() {
    SERVICE_IPC.bind_runtime_endpoints();
    serial_println!("[SERVICE_IPC] Initialized");
}

pub fn get_service_ipc() -> &'static ServiceIpcManager {
    &SERVICE_IPC
}

pub fn bind_service_endpoints() {
    get_service_ipc().bind_runtime_endpoints();
}

pub fn publish_service_endpoint(service: ServiceId, endpoint: ServiceEndpointRegistration) {
    get_service_ipc().register_endpoint(service, endpoint);
}

pub fn register_service_runtime_task(service: ServiceId, task_id: u64) {
    get_service_ipc().register_service_runtime_task(service, task_id);
}

pub fn open_service_handle(
    pid: u32,
    service: ServiceId,
    rights: CapabilityRights,
) -> Result<ServiceHandleDescriptor, ServiceError> {
    get_service_ipc().open_service_handle(pid, service, rights)
}

pub fn describe_service(service: ServiceId) -> Option<ServiceDescriptor> {
    service_control::describe_service(service)
}

pub fn service_parity_status() -> ServiceParityStatus {
    service_control::service_parity_status()
}

pub fn refresh_full_parity_mode() -> ServiceParityStatus {
    service_control::refresh_full_parity_mode()
}

pub fn strict_full_parity_mode_enabled() -> bool {
    service_control::strict_full_parity_mode_enabled()
}

pub fn legacy_sync_metrics() -> LegacySyncMetrics {
    get_service_ipc().legacy_sync_metrics()
}

pub fn migrated_legacy_sync_clear() -> bool {
    get_service_ipc().migrated_legacy_sync_clear()
}

pub fn endpoint_generation_for_service(service: ServiceId) -> EndpointGeneration {
    get_service_ipc().endpoint_generation(service)
}

pub fn grant_service_mailbox_regions(
    pid: u32,
    service: ServiceId,
) -> Result<ServiceMailboxLease, ServiceError> {
    get_service_ipc().grant_service_mailbox_regions(pid, service)
}

pub fn publish_user_service_endpoint(
    pid: u32,
    service: ServiceId,
    request_region_handle: UserHandle,
    response_region_handle: UserHandle,
) -> Result<UserPublishedEndpointDescriptor, ServiceError> {
    get_service_ipc().publish_user_endpoint(
        pid,
        service,
        request_region_handle,
        response_region_handle,
    )
}

pub fn heartbeat_user_service_endpoint(
    pid: u32,
    service: ServiceId,
) -> Result<UserPublishedEndpointDescriptor, ServiceError> {
    get_service_ipc().heartbeat_user_endpoint(pid, service)
}

pub fn published_user_service_endpoint(
    service: ServiceId,
) -> Option<UserPublishedEndpointDescriptor> {
    get_service_ipc().published_user_endpoint(service)
}

pub fn receive_notification_user_request(
    pid: u32,
) -> Result<Option<(u64, NotificationCommand)>, ServiceError> {
    let ipc = get_service_ipc();
    let _ = ipc.published_endpoint_owned_by(pid, ServiceId::EchNotifications)?;
    Ok(ipc
        .notification_user_requests
        .pop()
        .map(|request| (request.request_id, request.command)))
}

pub fn send_notification_user_response(
    pid: u32,
    request_id: u64,
    response: NotificationResponse,
) -> Result<(), ServiceError> {
    let ipc = get_service_ipc();
    let _ = ipc.published_endpoint_owned_by(pid, ServiceId::EchNotifications)?;
    ipc.notification_user_responses
        .try_push(NotificationUserResponse {
            request_id,
            response,
        })
        .map_err(|_| ServiceError::QueueFull)
}

pub fn grant_shared_region_handle(
    pid: u32,
    region_id: u64,
) -> Result<SharedRegionDescriptor, ServiceError> {
    get_service_ipc().grant_shared_region_handle(pid, region_id)
}

pub fn send_request(
    pid: u32,
    handle: UserHandle,
    message: ServiceMessage,
) -> Result<RequestTokenDescriptor, ServiceError> {
    get_service_ipc().send_request(pid, handle, message)
}

pub fn take_response(
    pid: u32,
    token: RequestToken,
) -> Result<Option<ServiceResponse>, ServiceError> {
    get_service_ipc().take_response(pid, token)
}

pub fn revoke_handle(pid: u32, handle: UserHandle) -> Result<(), ServiceError> {
    get_service_ipc().revoke_handle(pid, handle)
}

pub fn map_shared_region(pid: u32, region_handle: UserHandle) -> Result<UserMapping, ServiceError> {
    get_service_ipc().map_shared_region(pid, region_handle)
}

pub fn unmap_shared_region(pid: u32, region_handle: UserHandle) -> Result<(), ServiceError> {
    get_service_ipc().unmap_shared_region(pid, region_handle)
}

pub fn send_to_display(app_id: u32, command: DisplayCommand) -> u64 {
    get_service_ipc().send_to_service(
        app_id,
        ServiceId::EchDisplay,
        ServiceMessage::DisplayCommand(command),
    )
}

pub fn request_directory_sync(app_id: u32, command: DirectoryCommand) -> Option<DirectoryResponse> {
    service_control::request_directory_sync(app_id, command)
}

pub fn request_package_registry_sync(
    app_id: u32,
    command: PackageRegistryCommand,
) -> Option<PackageRegistryResponse> {
    service_control::request_package_registry_sync(app_id, command)
}

pub fn request_process_broker_sync(
    app_id: u32,
    command: ProcessBrokerCommand,
) -> Option<ProcessBrokerResponse> {
    service_control::request_process_broker_sync(app_id, command)
}

pub fn request_display_sync(app_id: u32, command: DisplayCommand) -> Option<DisplayResponse> {
    match get_service_ipc().request_sync_compat(
        app_id,
        ServiceId::EchDisplay,
        ServiceMessage::DisplayCommand(command),
        "request_display_sync",
    ) {
        ServiceResponse::DisplayResponse(response) => Some(response),
        _ => None,
    }
}

pub fn request_input_sync(app_id: u32, command: InputCommand) -> Option<InputResponse> {
    match get_service_ipc().request_sync_compat(
        app_id,
        ServiceId::EchInput,
        ServiceMessage::InputCommand(command),
        "request_input_sync",
    ) {
        ServiceResponse::InputResponse(response) => Some(response),
        _ => None,
    }
}

pub fn send_to_input(app_id: u32, command: InputCommand) -> u64 {
    get_service_ipc().send_to_service(
        app_id,
        ServiceId::EchInput,
        ServiceMessage::InputCommand(command),
    )
}

pub fn send_to_audio(app_id: u32, command: AudioCommand) -> u64 {
    get_service_ipc().send_to_service(
        app_id,
        ServiceId::EchAudio,
        ServiceMessage::AudioCommand(command),
    )
}

pub fn request_audio_sync(app_id: u32, command: AudioCommand) -> Option<AudioResponse> {
    match get_service_ipc().request_sync_compat(
        app_id,
        ServiceId::EchAudio,
        ServiceMessage::AudioCommand(command),
        "request_audio_sync",
    ) {
        ServiceResponse::AudioResponse(response) => Some(response),
        _ => None,
    }
}

pub fn send_to_store(app_id: u32, command: StoreCommand) -> u64 {
    get_service_ipc().send_to_service(
        app_id,
        ServiceId::EchStore,
        ServiceMessage::StoreCommand(command),
    )
}

pub fn request_store_sync(app_id: u32, command: StoreCommand) -> Option<StoreResponse> {
    match get_service_ipc().request_sync_compat(
        app_id,
        ServiceId::EchStore,
        ServiceMessage::StoreCommand(command),
        "request_store_sync",
    ) {
        ServiceResponse::StoreResponse(response) => Some(response),
        _ => None,
    }
}

pub fn request_shell_sync(app_id: u32, command: ShellCommand) -> Option<ShellResponse> {
    match get_service_ipc().request_sync_compat(
        app_id,
        ServiceId::EchShell,
        ServiceMessage::ShellCommand(command),
        "request_shell_sync",
    ) {
        ServiceResponse::ShellResponse(response) => Some(response),
        _ => None,
    }
}

pub fn request_notification_sync(
    app_id: u32,
    command: NotificationCommand,
) -> Option<NotificationResponse> {
    match get_service_ipc().request_sync_compat(
        app_id,
        ServiceId::EchNotifications,
        ServiceMessage::NotificationCommand(command),
        "request_notification_sync",
    ) {
        ServiceResponse::NotificationResponse(response) => Some(response),
        _ => None,
    }
}

pub fn request_clipboard_sync(app_id: u32, command: ClipboardCommand) -> Option<ClipboardResponse> {
    match get_service_ipc().request_sync_compat(
        app_id,
        ServiceId::EchClipboard,
        ServiceMessage::ClipboardCommand(command),
        "request_clipboard_sync",
    ) {
        ServiceResponse::ClipboardResponse(response) => Some(response),
        _ => None,
    }
}

pub fn request_dialog_sync(app_id: u32, command: DialogCommand) -> Option<DialogResponse> {
    match get_service_ipc().request_sync_compat(
        app_id,
        ServiceId::EchDialogs,
        ServiceMessage::DialogCommand(command),
        "request_dialog_sync",
    ) {
        ServiceResponse::DialogResponse(response) => Some(response),
        _ => None,
    }
}

pub fn request_capture_sync(app_id: u32, command: CaptureCommand) -> Option<CaptureResponse> {
    match get_service_ipc().request_sync_compat(
        app_id,
        ServiceId::EchCapture,
        ServiceMessage::CaptureCommand(command),
        "request_capture_sync",
    ) {
        ServiceResponse::CaptureResponse(response) => Some(response),
        _ => None,
    }
}

pub fn receive_response() -> Option<ResponseEnvelope> {
    get_service_ipc().receive_from_service()
}

pub fn process_messages() {
    get_service_ipc().process_pending_messages();
}

pub fn service_task() -> ! {
    loop {
        process_messages();
        for _ in 0..1000 {
            core::hint::spin_loop();
        }
    }
}

pub fn spawn_task() {
    tasking::scheduler::spawn_with_priority(
        service_task,
        tasking::task::Priority::Low,
        "service_ipc",
    );
}
