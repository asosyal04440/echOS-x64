use super::super::gui::launch_pipeline::AppResolution;
use super::super::ipc::service_ipc::{
    get_service_ipc, CapabilityRights, ServiceId, ServiceMessage, ServiceResponse,
};
use super::super::security::capability::CapRights;
use super::{
    launch_contract, native_scene_contract, package_registry_contract, process_broker_contract,
};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceParityStatus {
    pub required_services: u32,
    pub packaged_service_slots: u32,
    pub live_user_process_slots: u32,
    pub published_user_process_slots: u32,
    pub strict_mode_enabled: bool,
    pub full_parity_ready: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceClass {
    Directory,
    Ui,
    Input,
    Media,
    Storage,
    Session,
    Integration,
    Runtime,
    Package,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceDescriptor {
    pub id: ServiceId,
    pub name: String,
    pub class: ServiceClass,
    pub control_plane: bool,
    pub bulk_data_out_of_band: bool,
    pub openable_rights: CapabilityRights,
    pub bulk_region_classes: Vec<String>,
    pub service_process_available: bool,
    pub runtime_isolation: Option<launch_contract::IsolationDomain>,
    pub runtime_task_id: Option<u64>,
    pub runtime_image_path: Option<String>,
    pub user_published_endpoint: bool,
}

#[derive(Clone, Debug)]
pub enum DirectoryCommand {
    ListServices,
    DescribeService(ServiceId),
}

#[derive(Clone, Debug)]
pub enum DirectoryResponse {
    Services(Vec<ServiceDescriptor>),
    Service(Option<ServiceDescriptor>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackageRegistryCommand {
    ListEntries,
    DescribeQuery(String),
    ResolveFileAssociation(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackageRegistryResponse {
    Entries(Vec<package_registry_contract::PackageRegistryEntry>),
    Entry(Option<package_registry_contract::PackageRegistryEntry>),
    Resolution(Option<AppResolution>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProcessBrokerCommand {
    DescribeLaunch(process_broker_contract::ProcessBrokerTicket),
    DescribeChildren(process_broker_contract::ProcessBrokerTicket),
    DescribeRuntimeTask(u64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProcessBrokerResponse {
    Launch(Option<process_broker_contract::BrokeredLaunch>),
    Children(Vec<process_broker_contract::ProcessBrokerTicket>),
}

const REQUIRED_PARITY_SERVICES: [ServiceId; 9] = [
    ServiceId::EchDisplay,
    ServiceId::EchInput,
    ServiceId::EchAudio,
    ServiceId::EchStore,
    ServiceId::EchShell,
    ServiceId::EchNotifications,
    ServiceId::EchClipboard,
    ServiceId::EchDialogs,
    ServiceId::EchCapture,
];

static FULL_PARITY_STRICT_MODE: AtomicBool = AtomicBool::new(false);

pub(crate) fn dispatch_directory_command(message: ServiceMessage) -> ServiceResponse {
    match message {
        ServiceMessage::DirectoryCommand(command) => {
            ServiceResponse::DirectoryResponse(match command {
                DirectoryCommand::ListServices => DirectoryResponse::Services(service_directory()),
                DirectoryCommand::DescribeService(service_id) => {
                    DirectoryResponse::Service(service_descriptor(service_id))
                }
            })
        }
        _ => ServiceResponse::DirectoryResponse(DirectoryResponse::Service(None)),
    }
}

pub(crate) fn dispatch_package_registry_command(message: ServiceMessage) -> ServiceResponse {
    match message {
        ServiceMessage::PackageRegistryCommand(command) => {
            let registry_records =
                super::super::gfx::velvet_glove_registry::desktop_launch_registry();
            let registry =
                package_registry_contract::RuntimePackageRegistry::new(&registry_records);
            ServiceResponse::PackageRegistryResponse(match command {
                PackageRegistryCommand::ListEntries => {
                    PackageRegistryResponse::Entries(registry.entries())
                }
                PackageRegistryCommand::DescribeQuery(query) => {
                    PackageRegistryResponse::Entry(registry.describe(query.as_str()))
                }
                PackageRegistryCommand::ResolveFileAssociation(path) => {
                    PackageRegistryResponse::Resolution(
                        registry.resolve_file_association(path.as_str()),
                    )
                }
            })
        }
        _ => ServiceResponse::PackageRegistryResponse(PackageRegistryResponse::Entry(None)),
    }
}

pub(crate) fn dispatch_process_broker_command(message: ServiceMessage) -> ServiceResponse {
    match message {
        ServiceMessage::ProcessBrokerCommand(command) => {
            ServiceResponse::ProcessBrokerResponse(match command {
                ProcessBrokerCommand::DescribeLaunch(ticket) => {
                    ProcessBrokerResponse::Launch(process_broker_contract::brokered_launch(ticket))
                }
                ProcessBrokerCommand::DescribeChildren(ticket) => ProcessBrokerResponse::Children(
                    process_broker_contract::brokered_launch_children(ticket),
                ),
                ProcessBrokerCommand::DescribeRuntimeTask(task_id) => {
                    ProcessBrokerResponse::Launch(
                        native_scene_contract::runtime_handle_for_task(task_id).and_then(
                            |runtime| {
                                process_broker_contract::brokered_launch(runtime.broker_ticket)
                            },
                        ),
                    )
                }
            })
        }
        _ => ServiceResponse::ProcessBrokerResponse(ProcessBrokerResponse::Launch(None)),
    }
}

pub fn describe_service(service: ServiceId) -> Option<ServiceDescriptor> {
    service_descriptor(service)
}

pub fn service_parity_status() -> ServiceParityStatus {
    compute_service_parity_status()
}

pub fn refresh_full_parity_mode() -> ServiceParityStatus {
    let status = compute_service_parity_status();
    let strict = status.packaged_service_slots == status.required_services;
    FULL_PARITY_STRICT_MODE.store(strict, Ordering::Release);
    ServiceParityStatus {
        strict_mode_enabled: strict,
        ..status
    }
}

pub fn strict_full_parity_mode_enabled() -> bool {
    FULL_PARITY_STRICT_MODE.load(Ordering::Acquire)
}

pub(crate) fn set_full_parity_strict_mode_for_tests(strict: bool) {
    FULL_PARITY_STRICT_MODE.store(strict, Ordering::Release);
}

pub fn request_directory_sync(app_id: u32, command: DirectoryCommand) -> Option<DirectoryResponse> {
    match get_service_ipc().request_sync_compat(
        app_id,
        ServiceId::Directory,
        ServiceMessage::DirectoryCommand(command),
        "request_directory_sync",
    ) {
        ServiceResponse::DirectoryResponse(response) => Some(response),
        _ => None,
    }
}

pub fn request_package_registry_sync(
    app_id: u32,
    command: PackageRegistryCommand,
) -> Option<PackageRegistryResponse> {
    match get_service_ipc().request_sync_compat(
        app_id,
        ServiceId::PackageRegistry,
        ServiceMessage::PackageRegistryCommand(command),
        "request_package_registry_sync",
    ) {
        ServiceResponse::PackageRegistryResponse(response) => Some(response),
        _ => None,
    }
}

pub fn request_process_broker_sync(
    app_id: u32,
    command: ProcessBrokerCommand,
) -> Option<ProcessBrokerResponse> {
    match get_service_ipc().request_sync_compat(
        app_id,
        ServiceId::ProcessBroker,
        ServiceMessage::ProcessBrokerCommand(command),
        "request_process_broker_sync",
    ) {
        ServiceResponse::ProcessBrokerResponse(response) => Some(response),
        _ => None,
    }
}

fn service_directory() -> Vec<ServiceDescriptor> {
    fn runtime_fields(
        service_id: ServiceId,
        service_slug: &str,
    ) -> (
        bool,
        Option<launch_contract::IsolationDomain>,
        Option<u64>,
        Option<String>,
        bool,
    ) {
        let available = launch_contract::service_process_available(service_slug);
        let runtime = process_broker_contract::runtime_handle_for_service(service_id);
        let published = get_service_ipc()
            .published_user_endpoint(service_id)
            .is_some();
        (
            available,
            runtime.as_ref().map(|handle| handle.isolation_domain),
            runtime.as_ref().and_then(|handle| handle.task_id),
            runtime.and_then(|handle| handle.image_path),
            published,
        )
    }

    let (display_process, display_iso, display_task, display_image, display_published) =
        runtime_fields(ServiceId::EchDisplay, "ech_display");
    let (input_process, input_iso, input_task, input_image, input_published) =
        runtime_fields(ServiceId::EchInput, "ech_input");
    let (audio_process, audio_iso, audio_task, audio_image, audio_published) =
        runtime_fields(ServiceId::EchAudio, "ech_audio");
    let (store_process, store_iso, store_task, store_image, store_published) =
        runtime_fields(ServiceId::EchStore, "ech_store");
    let (shell_process, shell_iso, shell_task, shell_image, shell_published) =
        runtime_fields(ServiceId::EchShell, "ech_shell");
    let (
        notifications_process,
        notifications_iso,
        notifications_task,
        notifications_image,
        notifications_published,
    ) = runtime_fields(ServiceId::EchNotifications, "ech_notifications");
    let (clipboard_process, clipboard_iso, clipboard_task, clipboard_image, clipboard_published) =
        runtime_fields(ServiceId::EchClipboard, "ech_clipboard");
    let (dialogs_process, dialogs_iso, dialogs_task, dialogs_image, dialogs_published) =
        runtime_fields(ServiceId::EchDialogs, "ech_dialogs");
    let (capture_process, capture_iso, capture_task, capture_image, capture_published) =
        runtime_fields(ServiceId::EchCapture, "ech_capture");

    vec![
        ServiceDescriptor {
            id: ServiceId::Directory,
            name: String::from("ServiceDirectory"),
            class: ServiceClass::Directory,
            control_plane: true,
            bulk_data_out_of_band: false,
            openable_rights: CapRights::READ,
            bulk_region_classes: Vec::new(),
            service_process_available: false,
            runtime_isolation: Some(launch_contract::IsolationDomain::KernelTask),
            runtime_task_id: None,
            runtime_image_path: None,
            user_published_endpoint: false,
        },
        ServiceDescriptor {
            id: ServiceId::PackageRegistry,
            name: String::from("PackageRegistry"),
            class: ServiceClass::Package,
            control_plane: true,
            bulk_data_out_of_band: false,
            openable_rights: CapRights::READ,
            bulk_region_classes: Vec::new(),
            service_process_available: false,
            runtime_isolation: Some(launch_contract::IsolationDomain::KernelTask),
            runtime_task_id: None,
            runtime_image_path: None,
            user_published_endpoint: false,
        },
        ServiceDescriptor {
            id: ServiceId::ProcessBroker,
            name: String::from("ProcessBroker"),
            class: ServiceClass::Runtime,
            control_plane: true,
            bulk_data_out_of_band: false,
            openable_rights: CapRights::READ,
            bulk_region_classes: Vec::new(),
            service_process_available: false,
            runtime_isolation: Some(launch_contract::IsolationDomain::KernelTask),
            runtime_task_id: None,
            runtime_image_path: None,
            user_published_endpoint: false,
        },
        service_descriptor_entry(
            ServiceId::EchDisplay,
            "EchDisplay",
            ServiceClass::Ui,
            true,
            CapRights::READ_WRITE,
            vec![String::from("surface")],
            display_process,
            display_iso,
            display_task,
            display_image,
            display_published,
        ),
        service_descriptor_entry(
            ServiceId::EchInput,
            "EchInput",
            ServiceClass::Input,
            false,
            CapRights::READ_WRITE,
            Vec::new(),
            input_process,
            input_iso,
            input_task,
            input_image,
            input_published,
        ),
        service_descriptor_entry(
            ServiceId::EchAudio,
            "EchAudio",
            ServiceClass::Media,
            true,
            CapRights::READ_WRITE,
            vec![String::from("audio-stream")],
            audio_process,
            audio_iso,
            audio_task,
            audio_image,
            audio_published,
        ),
        service_descriptor_entry(
            ServiceId::EchStore,
            "EchStore",
            ServiceClass::Storage,
            true,
            CapRights::READ_WRITE,
            vec![String::from("file-io")],
            store_process,
            store_iso,
            store_task,
            store_image,
            store_published,
        ),
        service_descriptor_entry(
            ServiceId::EchShell,
            "EchShell",
            ServiceClass::Session,
            false,
            CapRights::READ_WRITE,
            Vec::new(),
            shell_process,
            shell_iso,
            shell_task,
            shell_image,
            shell_published,
        ),
        service_descriptor_entry(
            ServiceId::EchNotifications,
            "EchNotifications",
            ServiceClass::Session,
            false,
            CapRights::READ_WRITE,
            Vec::new(),
            notifications_process,
            notifications_iso,
            notifications_task,
            notifications_image,
            notifications_published,
        ),
        service_descriptor_entry(
            ServiceId::EchClipboard,
            "EchClipboard",
            ServiceClass::Session,
            true,
            CapRights::READ_WRITE,
            vec![String::from("clipboard-payload")],
            clipboard_process,
            clipboard_iso,
            clipboard_task,
            clipboard_image,
            clipboard_published,
        ),
        service_descriptor_entry(
            ServiceId::EchDialogs,
            "EchDialogs",
            ServiceClass::Session,
            false,
            CapRights::READ_WRITE,
            Vec::new(),
            dialogs_process,
            dialogs_iso,
            dialogs_task,
            dialogs_image,
            dialogs_published,
        ),
        service_descriptor_entry(
            ServiceId::EchCapture,
            "EchCapture",
            ServiceClass::Integration,
            true,
            CapRights::READ_WRITE,
            vec![String::from("screenshot")],
            capture_process,
            capture_iso,
            capture_task,
            capture_image,
            capture_published,
        ),
    ]
}

fn service_descriptor(service_id: ServiceId) -> Option<ServiceDescriptor> {
    service_directory()
        .into_iter()
        .find(|descriptor| descriptor.id == service_id)
}

fn compute_service_parity_status() -> ServiceParityStatus {
    let descriptors = service_directory();
    let required_services = REQUIRED_PARITY_SERVICES.len() as u32;
    let mut packaged_service_slots = 0u32;
    let mut live_user_process_slots = 0u32;
    let mut published_user_process_slots = 0u32;

    for service_id in REQUIRED_PARITY_SERVICES {
        let Some(descriptor) = descriptors
            .iter()
            .find(|descriptor| descriptor.id == service_id)
        else {
            continue;
        };
        if descriptor.service_process_available {
            packaged_service_slots = packaged_service_slots.saturating_add(1);
        }
        if descriptor.runtime_isolation == Some(launch_contract::IsolationDomain::UserProcess) {
            live_user_process_slots = live_user_process_slots.saturating_add(1);
        }
        if descriptor.user_published_endpoint {
            published_user_process_slots = published_user_process_slots.saturating_add(1);
        }
    }

    ServiceParityStatus {
        required_services,
        packaged_service_slots,
        live_user_process_slots,
        published_user_process_slots,
        strict_mode_enabled: FULL_PARITY_STRICT_MODE.load(Ordering::Acquire),
        full_parity_ready: published_user_process_slots == required_services,
    }
}

fn service_descriptor_entry(
    id: ServiceId,
    name: &'static str,
    class: ServiceClass,
    bulk_data_out_of_band: bool,
    openable_rights: CapabilityRights,
    bulk_region_classes: Vec<String>,
    service_process_available: bool,
    runtime_isolation: Option<launch_contract::IsolationDomain>,
    runtime_task_id: Option<u64>,
    runtime_image_path: Option<String>,
    user_published_endpoint: bool,
) -> ServiceDescriptor {
    ServiceDescriptor {
        id,
        name: String::from(name),
        class,
        control_plane: true,
        bulk_data_out_of_band,
        openable_rights,
        bulk_region_classes,
        service_process_available,
        runtime_isolation,
        runtime_task_id,
        runtime_image_path,
        user_published_endpoint,
    }
}
