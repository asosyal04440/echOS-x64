use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use spin::Mutex;

use super::super::kernel::memory::shared_region::{self, UserMapping};
use super::super::runtime_layer::launch_contract::{
    register_launch_session, register_launch_session_with_parent, IsolationDomain,
};
use super::super::runtime_layer::service_control::{
    dispatch_directory_command as dispatch_directory_message,
    dispatch_network_broker_command as dispatch_network_broker_message,
    dispatch_package_registry_command as dispatch_package_registry_message,
    dispatch_process_broker_command as dispatch_process_broker_message,
    dispatch_update_installer_command as dispatch_update_installer_message, DirectoryCommand,
    DirectoryResponse, NetworkBrokerCommand, NetworkBrokerResponse, PackageRegistryCommand,
    PackageRegistryResponse, ProcessBrokerCommand, ProcessBrokerResponse, ServiceClass,
    ServiceDescriptor, ServiceParityStatus, UpdateInstallerCommand, UpdateInstallerResponse,
};
use super::super::security::capability::{
    self, CapRights, CapabilityError, UserHandle as RawUserHandle,
};
use super::super::services::display_atomic::MailboxRing;
use super::super::services::{
    AudioCommand, AudioResponse, CaptureCommand, CaptureResponse, ClipboardCommand,
    ClipboardResponse, DialogCommand, DialogResponse, DisplayCommand, DisplayResponse, EchAudio,
    EchCapture, EchClipboard, EchDialogs, EchDisplay, EchInput, EchNotifications, EchShell,
    EchStore, InputCommand, InputResponse, NotificationCommand, NotificationResponse, ShellCommand,
    ShellResponse, StoreCommand, StoreResponse,
};
use endpoints::{blocking_sync_allowed, publish_endpoint, BoundServiceEndpoint};

mod api;
mod compat;
mod endpoints;
mod handles;
mod runtime_bridge;
mod transport;

const SERVICE_IPC_OUTGOING_CAPACITY: usize = 256;
const SERVICE_IPC_INCOMING_CAPACITY: usize = 256;
const SERVICE_RESPONSE_SPINS: usize = 200_000;
const SERVICE_RESPONSE_SCHEDULE_INTERVAL: usize = 64;

pub use super::super::security::capability::UserHandle;
pub use api::*;
pub use endpoints::ServiceEndpointRegistration;
pub type KernelCapabilityId = super::super::security::capability::CapId;
pub type EndpointGeneration = u32;
pub type CapabilityRights = CapRights;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestToken(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceHandleDescriptor {
    pub handle: UserHandle,
    pub service_id: ServiceId,
    pub endpoint_generation: EndpointGeneration,
    pub rights: CapabilityRights,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestTokenDescriptor {
    pub token: RequestToken,
    pub request_id: u64,
    pub service_id: ServiceId,
    pub endpoint_generation: EndpointGeneration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SharedRegionDescriptor {
    pub handle: UserHandle,
    pub region_id: u64,
    pub generation: u64,
    pub len: u64,
    pub writable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BulkDescriptor {
    pub region_handle: UserHandle,
    pub offset: u64,
    pub len: u64,
    pub generation: u64,
    pub writable: bool,
    pub fence_seq: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceMailboxLease {
    pub request_region: SharedRegionDescriptor,
    pub response_region: SharedRegionDescriptor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserPublishedEndpointDescriptor {
    pub owner_pid: u64,
    pub task_id: u64,
    pub request_region_id: u64,
    pub request_generation: u64,
    pub response_region_id: u64,
    pub response_generation: u64,
    pub heartbeat_epoch: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LegacySyncMetrics {
    pub by_service: BTreeMap<ServiceId, u64>,
    pub by_callsite: BTreeMap<&'static str, u64>,
    pub strict_violation_by_service: BTreeMap<ServiceId, u64>,
    pub strict_violation_by_callsite: BTreeMap<&'static str, u64>,
    pub migrated_services_clear: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServiceError {
    InvalidHandle,
    RightsDenied,
    Revoked,
    StaleGeneration,
    QueueFull,
    SyncCycleRisk,
    EndpointRestarted,
    ServiceUnavailable,
    WrongService,
    WrongResponseKind,
    SharedRegionUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ServiceId {
    Directory = 0,
    NetworkBroker = 13,
    EchDisplay = 1,
    EchInput = 2,
    EchAudio = 3,
    EchStore = 4,
    EchShell = 5,
    EchNotifications = 6,
    EchClipboard = 7,
    EchDialogs = 8,
    EchCapture = 9,
    PackageRegistry = 10,
    ProcessBroker = 11,
    UpdateInstaller = 12,
}

#[derive(Clone, Debug)]
pub enum ServiceMessage {
    DirectoryCommand(DirectoryCommand),
    NetworkBrokerCommand(NetworkBrokerCommand),
    PackageRegistryCommand(PackageRegistryCommand),
    ProcessBrokerCommand(ProcessBrokerCommand),
    UpdateInstallerCommand(UpdateInstallerCommand),
    DisplayCommand(DisplayCommand),
    InputCommand(InputCommand),
    AudioCommand(AudioCommand),
    StoreCommand(StoreCommand),
    ShellCommand(ShellCommand),
    NotificationCommand(NotificationCommand),
    ClipboardCommand(ClipboardCommand),
    DialogCommand(DialogCommand),
    CaptureCommand(CaptureCommand),
}

#[derive(Clone, Debug)]
pub enum ServiceResponse {
    DirectoryResponse(DirectoryResponse),
    NetworkBrokerResponse(NetworkBrokerResponse),
    PackageRegistryResponse(PackageRegistryResponse),
    ProcessBrokerResponse(ProcessBrokerResponse),
    UpdateInstallerResponse(UpdateInstallerResponse),
    DisplayResponse(DisplayResponse),
    InputResponse(InputResponse),
    AudioResponse(AudioResponse),
    StoreResponse(StoreResponse),
    ShellResponse(ShellResponse),
    NotificationResponse(NotificationResponse),
    ClipboardResponse(ClipboardResponse),
    DialogResponse(DialogResponse),
    CaptureResponse(CaptureResponse),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BlockingMode {
    Async,
    Sync,
}

static REQUEST_ID: AtomicU64 = AtomicU64::new(1);
static HEARTBEAT_EPOCH: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
struct MessageEnvelope {
    pub id: u64,
    pub owner_pid: u64,
    pub from_app: u32,
    pub request_token: RequestToken,
    pub to_service: ServiceId,
    pub endpoint_generation: EndpointGeneration,
    pub origin_endpoint: Option<ServiceId>,
    pub blocking_mode: BlockingMode,
    pub causal_parent_token: Option<RequestToken>,
    pub message: ServiceMessage,
}

#[derive(Clone, Debug)]
pub struct ResponseEnvelope {
    pub message_id: u64,
    pub request_token: RequestToken,
    pub response: ServiceResponse,
}

#[derive(Clone, Debug)]
struct NotificationUserRequest {
    pub request_id: u64,
    pub command: NotificationCommand,
}

#[derive(Clone, Debug)]
struct NotificationUserResponse {
    pub request_id: u64,
    pub response: NotificationResponse,
}

#[derive(Clone, Debug)]
struct PendingRequest {
    owner_pid: u64,
    request_token: RequestToken,
    response: Option<ServiceResponse>,
}

#[derive(Clone, Copy, Debug)]
struct ActiveServiceRequest {
    request_token: RequestToken,
    origin_endpoint: Option<ServiceId>,
}

pub struct ServiceIpcManager {
    outgoing: MailboxRing<MessageEnvelope>,
    incoming: MailboxRing<ResponseEnvelope>,
    notification_user_requests: MailboxRing<NotificationUserRequest>,
    notification_user_responses: MailboxRing<NotificationUserResponse>,
    endpoints: Mutex<BTreeMap<ServiceId, BoundServiceEndpoint>>,
    endpoint_generations: Mutex<BTreeMap<ServiceId, EndpointGeneration>>,
    inflight_requests: Mutex<BTreeMap<ServiceId, VecDeque<MessageEnvelope>>>,
    pending: Mutex<BTreeMap<u64, PendingRequest>>,
    active_requests: Mutex<BTreeMap<ServiceId, ActiveServiceRequest>>,
    service_runtime_tasks: Mutex<BTreeMap<u64, ServiceId>>,
    mailbox_regions: Mutex<BTreeMap<ServiceId, (u64, u64)>>,
    published_user_endpoints: Mutex<BTreeMap<ServiceId, UserPublishedEndpointDescriptor>>,
    legacy_sync_by_service: Mutex<BTreeMap<ServiceId, u64>>,
    legacy_sync_by_callsite: Mutex<BTreeMap<&'static str, u64>>,
    legacy_sync_strict_violation_by_service: Mutex<BTreeMap<ServiceId, u64>>,
    legacy_sync_strict_violation_by_callsite: Mutex<BTreeMap<&'static str, u64>>,
}

impl ServiceIpcManager {
    pub fn new() -> Self {
        Self {
            outgoing: MailboxRing::with_capacity_pow2(SERVICE_IPC_OUTGOING_CAPACITY),
            incoming: MailboxRing::with_capacity_pow2(SERVICE_IPC_INCOMING_CAPACITY),
            notification_user_requests: MailboxRing::with_capacity_pow2(
                SERVICE_IPC_OUTGOING_CAPACITY,
            ),
            notification_user_responses: MailboxRing::with_capacity_pow2(
                SERVICE_IPC_INCOMING_CAPACITY,
            ),
            endpoints: Mutex::new(BTreeMap::new()),
            endpoint_generations: Mutex::new(BTreeMap::new()),
            inflight_requests: Mutex::new(BTreeMap::new()),
            pending: Mutex::new(BTreeMap::new()),
            active_requests: Mutex::new(BTreeMap::new()),
            service_runtime_tasks: Mutex::new(BTreeMap::new()),
            mailbox_regions: Mutex::new(BTreeMap::new()),
            published_user_endpoints: Mutex::new(BTreeMap::new()),
            legacy_sync_by_service: Mutex::new(BTreeMap::new()),
            legacy_sync_by_callsite: Mutex::new(BTreeMap::new()),
            legacy_sync_strict_violation_by_service: Mutex::new(BTreeMap::new()),
            legacy_sync_strict_violation_by_callsite: Mutex::new(BTreeMap::new()),
        }
    }

    fn sync_cycle_risk(&self, target_service: ServiceId) -> bool {
        let Some(current) = self.current_service_context() else {
            return false;
        };
        if current == target_service {
            return true;
        }
        if let Some(active) = self.current_active_request() {
            if active.origin_endpoint == Some(target_service) {
                return true;
            }
        }
        !blocking_sync_allowed(current, target_service)
    }
}

fn dispatch_directory_command(message: ServiceMessage) -> ServiceResponse {
    dispatch_directory_message(message)
}

fn dispatch_network_broker_command(message: ServiceMessage) -> ServiceResponse {
    dispatch_network_broker_message(message)
}

fn dispatch_package_registry_command(message: ServiceMessage) -> ServiceResponse {
    dispatch_package_registry_message(message)
}

fn dispatch_process_broker_command(message: ServiceMessage) -> ServiceResponse {
    dispatch_process_broker_message(message)
}

fn dispatch_update_installer_command(message: ServiceMessage) -> ServiceResponse {
    dispatch_update_installer_message(message)
}

fn error_response(service: ServiceId, error: ServiceError) -> ServiceResponse {
    let message = match error {
        ServiceError::InvalidHandle => String::from("invalid handle"),
        ServiceError::RightsDenied => String::from("rights denied"),
        ServiceError::Revoked => String::from("handle revoked"),
        ServiceError::StaleGeneration => String::from("stale generation"),
        ServiceError::QueueFull => String::from("service queue full"),
        ServiceError::SyncCycleRisk => String::from("distributed sync cycle risk"),
        ServiceError::EndpointRestarted => String::from("endpoint restarted"),
        ServiceError::ServiceUnavailable => String::from("service unavailable"),
        ServiceError::WrongService => String::from("wrong service message"),
        ServiceError::WrongResponseKind => String::from("wrong response kind"),
        ServiceError::SharedRegionUnavailable => String::from("shared region unavailable"),
    };
    match service {
        ServiceId::Directory => {
            ServiceResponse::DirectoryResponse(DirectoryResponse::Service(None))
        }
        ServiceId::NetworkBroker => ServiceResponse::NetworkBrokerResponse(
            NetworkBrokerResponse::Error(
                super::super::runtime_layer::service_control::NetworkBrokerError::service_unavailable(
                    message,
                ),
            ),
        ),
        ServiceId::PackageRegistry => {
            ServiceResponse::PackageRegistryResponse(PackageRegistryResponse::Error(
                super::super::runtime_layer::service_control::PackageRegistryError::service_unavailable(
                    message,
                ),
            ))
        }
        ServiceId::ProcessBroker => {
            ServiceResponse::ProcessBrokerResponse(ProcessBrokerResponse::Launch(None))
        }
        ServiceId::UpdateInstaller => {
            ServiceResponse::UpdateInstallerResponse(UpdateInstallerResponse::Error(
                super::super::runtime_layer::service_control::UpdateInstallerError::service_unavailable(
                    message,
                ),
            ))
        }
        ServiceId::EchDisplay => ServiceResponse::DisplayResponse(DisplayResponse::Error(message)),
        ServiceId::EchInput => ServiceResponse::InputResponse(InputResponse::Error(message)),
        ServiceId::EchAudio => ServiceResponse::AudioResponse(AudioResponse::Error(
            crate::services::AudioError::service_unavailable(message),
        )),
        ServiceId::EchStore => ServiceResponse::StoreResponse(StoreResponse::Error(message)),
        ServiceId::EchShell => ServiceResponse::ShellResponse(ShellResponse::Error(message)),
        ServiceId::EchNotifications => {
            ServiceResponse::NotificationResponse(NotificationResponse::Error(message))
        }
        ServiceId::EchClipboard => {
            ServiceResponse::ClipboardResponse(ClipboardResponse::Error(message))
        }
        ServiceId::EchDialogs => ServiceResponse::DialogResponse(DialogResponse::Error(message)),
        ServiceId::EchCapture => ServiceResponse::CaptureResponse(CaptureResponse::Error(message)),
    }
}

fn service_unavailable_response(service: ServiceId) -> ServiceResponse {
    error_response(service, ServiceError::ServiceUnavailable)
}

pub(super) fn service_mailbox_region_name(service: ServiceId, lane: &str) -> String {
    let slug = match service {
        ServiceId::Directory => "service_directory",
        ServiceId::NetworkBroker => "network_broker",
        ServiceId::PackageRegistry => "package_registry",
        ServiceId::ProcessBroker => "process_broker",
        ServiceId::UpdateInstaller => "update_installer",
        ServiceId::EchDisplay => "ech_display",
        ServiceId::EchInput => "ech_input",
        ServiceId::EchAudio => "ech_audio",
        ServiceId::EchStore => "ech_store",
        ServiceId::EchShell => "ech_shell",
        ServiceId::EchNotifications => "ech_notifications",
        ServiceId::EchClipboard => "ech_clipboard",
        ServiceId::EchDialogs => "ech_dialogs",
        ServiceId::EchCapture => "ech_capture",
    };
    alloc::format!("svc:{}:{}", slug, lane)
}

#[cfg(test)]
mod tests {
    use super::super::super::gui::launch_pipeline::{
        AbiPersonality, AppDescriptor, AppPresentation, CapabilityProfile, ExecutionContext,
        LaunchIntent, LaunchSource, LoaderDispatch,
    };
    use super::super::super::runtime_layer::service_control::set_full_parity_strict_mode_for_tests;
    use super::super::super::runtime_layer::service_control::{
        NetworkBrokerError, NetworkBrokerErrorKind, PackageLifecycleAction, PackageLifecycleReport,
    };
    use super::*;
    use alloc::string::{String, ToString};
    use alloc::sync::Arc;
    use echos_manifest::{
        AppPresentation as ManifestPresentation, AppRuntime, AppStateContract, CompiledAppManifest,
        DefaultWindow, RestartPolicy, SourceAppManifest, TrustDomain,
    };
    use sha2::{Digest, Sha256};

    fn demo_source_manifest(app_id: &str, entry: &str) -> SourceAppManifest {
        SourceAppManifest {
            app_id: app_id.to_string(),
            name: String::from("Service Package Test"),
            version: String::from("0.1.0"),
            entry: entry.to_string(),
            sdk_version: 1,
            runtime: AppRuntime::Native,
            presentation: ManifestPresentation::Windowed,
            capabilities: alloc::vec![echos_manifest::NativeCapability::NotificationsPost],
            default_window: DefaultWindow {
                title: String::from("Service Package Test"),
                width: 640,
                height: 480,
            },
            state_contract: AppStateContract::ColdResume,
            restart_policy: RestartPolicy::bounded_retry(2),
        }
    }

    #[test]
    fn directory_lists_core_service_bus_endpoints() {
        let response =
            request_directory_sync(0, DirectoryCommand::ListServices).expect("directory response");
        let DirectoryResponse::Services(services) = response else {
            unreachable!("unexpected directory response");
        };
        assert!(services.iter().any(|entry| entry.id == ServiceId::EchShell));
        assert!(services
            .iter()
            .any(|entry| entry.id == ServiceId::EchDisplay));
        assert!(services
            .iter()
            .any(|entry| entry.id == ServiceId::EchCapture));
        assert!(services
            .iter()
            .any(|entry| entry.id == ServiceId::PackageRegistry));
        assert!(services
            .iter()
            .any(|entry| entry.id == ServiceId::ProcessBroker));
        assert!(services
            .iter()
            .any(|entry| entry.id == ServiceId::NetworkBroker));
    }

    #[test]
    fn package_registry_service_lists_built_in_entries() {
        let response = request_package_registry_sync(0, PackageRegistryCommand::ListEntries)
            .expect("package registry response");
        let PackageRegistryResponse::Entries(entries) = response else {
            unreachable!("unexpected package registry payload");
        };
        assert!(entries
            .iter()
            .any(|entry| entry.identity().package_id == "echos.terminal"));
        assert!(entries
            .iter()
            .any(|entry| entry.identity().package_id == "echos.web"));
    }

    #[test]
    fn package_registry_service_runs_install_verify_remove_through_control_plane() {
        let app_id = "org.echos.service.eon";
        let _ = request_package_registry_sync(
            0,
            PackageRegistryCommand::RemovePackage(app_id.to_string()),
        );

        let source = demo_source_manifest(app_id, "service-eon.elf");
        let entry = b"service-package-binary".to_vec();
        let entry_digest: [u8; 32] = Sha256::digest(&entry).into();
        let compiled =
            CompiledAppManifest::from_source(&source, entry_digest).expect("compiled manifest");
        let bundle = crate::security::package::build_signed_bundle(
            &source,
            &compiled,
            &entry,
            TrustDomain::Developer,
        )
        .expect("bundle");
        let install =
            request_package_registry_sync(0, PackageRegistryCommand::InstallBundle(bundle))
                .expect("install response");
        assert!(matches!(
            install,
            PackageRegistryResponse::Lifecycle(PackageLifecycleReport {
                action: PackageLifecycleAction::Install,
                ..
            })
        ));

        let verify = request_package_registry_sync(
            0,
            PackageRegistryCommand::VerifyPackage(app_id.to_string()),
        )
        .expect("verify response");
        assert!(matches!(
            verify,
            PackageRegistryResponse::Lifecycle(PackageLifecycleReport {
                action: PackageLifecycleAction::Verify,
                ..
            })
        ));

        let packages = request_package_registry_sync(0, PackageRegistryCommand::ListPackages)
            .expect("packages response");
        let PackageRegistryResponse::Packages(packages) = packages else {
            unreachable!("unexpected package list response");
        };
        assert!(packages.iter().any(|record| record.name == app_id));

        let remove = request_package_registry_sync(
            0,
            PackageRegistryCommand::RemovePackage(app_id.to_string()),
        )
        .expect("remove response");
        assert!(matches!(
            remove,
            PackageRegistryResponse::Lifecycle(PackageLifecycleReport {
                action: PackageLifecycleAction::Remove,
                ..
            })
        ));
    }

    #[test]
    fn network_broker_returns_typed_invalid_url_error() {
        let response =
            request_network_broker_sync(0, NetworkBrokerCommand::Download(String::from("http://")))
                .expect("network broker response");
        assert!(matches!(
            response,
            NetworkBrokerResponse::Error(NetworkBrokerError {
                kind: NetworkBrokerErrorKind::InvalidUrl,
                ..
            })
        ));
    }

    #[test]
    fn legacy_sync_compat_counters_are_measurable() {
        let manager = ServiceIpcManager::new();

        let strict_violation =
            manager.record_legacy_sync_probe(ServiceId::Directory, "compat-counter-test");

        assert!(!strict_violation);
        let metrics = manager.legacy_sync_metrics();
        assert_eq!(
            metrics.by_service.get(&ServiceId::Directory).copied(),
            Some(1)
        );
        assert_eq!(
            metrics.by_callsite.get("compat-counter-test").copied(),
            Some(1)
        );
        assert!(metrics.migrated_services_clear);
    }

    #[test]
    fn strict_mode_marks_migrated_service_compat_usage_as_violation() {
        let manager = ServiceIpcManager::new();
        set_full_parity_strict_mode_for_tests(true);

        let strict_violation =
            manager.record_legacy_sync_probe(ServiceId::EchDisplay, "strict-compat-test");

        assert!(strict_violation);
        let metrics = manager.legacy_sync_metrics();
        assert_eq!(
            metrics
                .strict_violation_by_service
                .get(&ServiceId::EchDisplay)
                .copied(),
            Some(1)
        );
        assert_eq!(
            metrics
                .strict_violation_by_callsite
                .get("strict-compat-test")
                .copied(),
            Some(1)
        );
        set_full_parity_strict_mode_for_tests(false);
    }

    #[test]
    fn process_broker_service_describes_registered_launch_and_children() {
        let descriptor = AppDescriptor::new(
            410,
            "terminal",
            "Terminal",
            LoaderDispatch::Native,
            AbiPersonality::Native,
            AppPresentation::Windowed,
            CapabilityProfile::shell_defaults(),
        )
        .with_package_id("echos.terminal");
        let child_descriptor = AppDescriptor::new(
            411,
            "firefox",
            "Firefox",
            LoaderDispatch::Pe,
            AbiPersonality::Win32,
            AppPresentation::ShellOwned,
            CapabilityProfile::shell_defaults(),
        )
        .with_package_id("org.mozilla.firefox");
        let parent_session = LaunchIntent::new(
            descriptor,
            ExecutionContext::new(LaunchSource::DesktopShortcut, 0, "desktop"),
        )
        .canonical_session();
        let child_session = LaunchIntent::new(
            child_descriptor,
            ExecutionContext::new(LaunchSource::CommandPalette, 0, "browser"),
        )
        .canonical_session();
        let parent_runtime = register_launch_session(
            parent_session,
            Some(4001),
            None,
            None,
            IsolationDomain::KernelTask,
            None,
        );
        let child_runtime = register_launch_session_with_parent(
            child_session,
            Some(4002),
            Some(String::from("/downloads/firefox/firefox.exe")),
            None,
            IsolationDomain::UserProcess,
            None,
            parent_runtime.broker_ticket,
        );
        let launch_response = request_process_broker_sync(
            0,
            ProcessBrokerCommand::DescribeLaunch(child_runtime.broker_ticket),
        )
        .expect("broker response");
        let ProcessBrokerResponse::Launch(Some(launch)) = launch_response else {
            unreachable!("unexpected broker response");
        };
        assert_eq!(launch.ticket, child_runtime.broker_ticket);
        let child_response = request_process_broker_sync(
            0,
            ProcessBrokerCommand::DescribeChildren(parent_runtime.broker_ticket),
        )
        .expect("children response");
        let ProcessBrokerResponse::Children(children) = child_response else {
            unreachable!("unexpected broker child response");
        };
        assert!(children.contains(&child_runtime.broker_ticket));
    }

    #[test]
    fn async_directory_round_trip_uses_handle_and_token() {
        let ipc = ServiceIpcManager::new();
        let handle = ipc
            .open_service_handle(7, ServiceId::Directory, CapRights::READ_WRITE)
            .expect("handle");
        let token = ipc
            .send_request(
                7,
                handle.handle,
                ServiceMessage::DirectoryCommand(DirectoryCommand::DescribeService(
                    ServiceId::EchShell,
                )),
            )
            .expect("token");
        ipc.process_pending_messages();
        let response = ipc
            .take_response(7, token.token)
            .expect("response lookup")
            .expect("completed response");
        let ServiceResponse::DirectoryResponse(DirectoryResponse::Service(Some(service))) =
            response
        else {
            unreachable!("unexpected directory response payload");
        };
        assert_eq!(service.id, ServiceId::EchShell);
    }

    #[test]
    fn shared_region_handles_track_generation_and_mapping() {
        let ipc = ServiceIpcManager::new();
        let region = shared_region::create_ipc_region(11, "clipboard", 4096, true);
        let handle = ipc
            .grant_shared_region_handle(11, region.id)
            .expect("shared region handle");
        let mapping = ipc.map_shared_region(11, handle.handle).expect("mapping");
        assert_eq!(mapping.region_id, region.id);
        assert_eq!(mapping.generation, region.generation);
    }

    #[test]
    fn revoked_shared_region_handles_go_stale() {
        let ipc = ServiceIpcManager::new();
        let region = shared_region::create_ipc_region(13, "surface", 8192, true);
        let handle = ipc
            .grant_shared_region_handle(13, region.id)
            .expect("shared region handle");
        assert!(ipc.map_shared_region(13, handle.handle).is_ok());
        assert!(shared_region::revoke_ipc_region(region.id));
        assert_eq!(
            ipc.map_shared_region(13, handle.handle)
                .expect_err("revoked region must go stale"),
            ServiceError::StaleGeneration
        );
    }

    #[test]
    fn service_mailbox_leases_are_granted_as_shared_regions() {
        let ipc = ServiceIpcManager::new();
        let lease = ipc
            .grant_service_mailbox_regions(21, ServiceId::EchNotifications)
            .expect("mailbox lease");
        let request = ipc
            .map_shared_region(21, lease.request_region.handle)
            .expect("request mapping");
        let response = ipc
            .map_shared_region(21, lease.response_region.handle)
            .expect("response mapping");
        assert_eq!(request.len, 64 * 1024);
        assert_eq!(response.len, 64 * 1024);
        assert_ne!(request.region_id, response.region_id);
    }

    #[test]
    fn runtime_bound_services_move_requests_into_inflight_queue() {
        let ipc = ServiceIpcManager::new();
        let notifications = Arc::new(EchNotifications::new());
        ipc.register_endpoint(
            ServiceId::EchNotifications,
            ServiceEndpointRegistration::Notifications(notifications),
        );
        ipc.register_service_runtime_task(ServiceId::EchNotifications, 99);
        let handle = ipc
            .open_service_handle(31, ServiceId::EchNotifications, CapRights::READ_WRITE)
            .expect("notifications handle");
        let token = ipc
            .send_request(
                31,
                handle.handle,
                ServiceMessage::NotificationCommand(NotificationCommand::Clear { app_id: 1 }),
            )
            .expect("queued request");
        ipc.process_pending_messages();
        assert!(ipc
            .take_response(31, token.token)
            .expect("response lookup")
            .is_none());
        assert_eq!(
            ipc.inflight_requests
                .lock()
                .get(&ServiceId::EchNotifications)
                .map(|queue| queue.len()),
            Some(1)
        );
    }

    #[test]
    fn published_notification_endpoint_routes_requests_into_user_queue() {
        let ipc = ServiceIpcManager::new();
        let notifications = Arc::new(EchNotifications::new());
        ipc.register_endpoint(
            ServiceId::EchNotifications,
            ServiceEndpointRegistration::Notifications(notifications),
        );
        ipc.register_service_runtime_task(ServiceId::EchNotifications, 100);
        let request_region = shared_region::create_ipc_region(41, "notify-req", 4096, true);
        let response_region = shared_region::create_ipc_region(41, "notify-resp", 4096, true);
        ipc.published_user_endpoints.lock().insert(
            ServiceId::EchNotifications,
            UserPublishedEndpointDescriptor {
                owner_pid: 41,
                task_id: 100,
                request_region_id: request_region.id,
                request_generation: request_region.generation,
                response_region_id: response_region.id,
                response_generation: response_region.generation,
                heartbeat_epoch: 1,
            },
        );
        let handle = ipc
            .open_service_handle(41, ServiceId::EchNotifications, CapRights::READ_WRITE)
            .expect("notifications handle");
        let token = ipc
            .send_request(
                41,
                handle.handle,
                ServiceMessage::NotificationCommand(NotificationCommand::Clear { app_id: 41 }),
            )
            .expect("queued request");
        ipc.process_pending_messages();
        assert!(ipc
            .take_response(41, token.token)
            .expect("response lookup")
            .is_none());
        let queued = ipc
            .notification_user_requests
            .pop()
            .expect("user queue request");
        assert_eq!(queued.request_id, token.request_id);
        assert!(matches!(
            queued.command,
            NotificationCommand::Clear { app_id: 41 }
        ));
        ipc.notification_user_responses
            .try_push(NotificationUserResponse {
                request_id: token.request_id,
                response: NotificationResponse::Ack,
            })
            .expect("queued response");
        ipc.process_pending_messages();
        let response = ipc
            .take_response(41, token.token)
            .expect("response lookup")
            .expect("completed response");
        assert!(matches!(
            response,
            ServiceResponse::NotificationResponse(NotificationResponse::Ack)
        ));
    }

    #[test]
    fn bootstrap_fallback_still_returns_direct_response_before_runtime_task() {
        let ipc = ServiceIpcManager::new();
        let notifications = Arc::new(EchNotifications::new());
        ipc.register_endpoint(
            ServiceId::EchNotifications,
            ServiceEndpointRegistration::Notifications(notifications),
        );
        let handle = ipc
            .open_service_handle(32, ServiceId::EchNotifications, CapRights::READ_WRITE)
            .expect("notifications handle");
        let token = ipc
            .send_request(
                32,
                handle.handle,
                ServiceMessage::NotificationCommand(NotificationCommand::Clear { app_id: 1 }),
            )
            .expect("queued request");
        ipc.process_pending_messages();
        let response = ipc
            .take_response(32, token.token)
            .expect("response lookup")
            .expect("completed response");
        assert!(matches!(
            response,
            ServiceResponse::NotificationResponse(NotificationResponse::Ack)
        ));
    }

    #[test]
    fn sync_cycle_allowlist_is_explicit() {
        assert!(blocking_sync_allowed(
            ServiceId::EchCapture,
            ServiceId::EchShell
        ));
        assert!(!blocking_sync_allowed(
            ServiceId::EchShell,
            ServiceId::EchDisplay
        ));
        assert!(!blocking_sync_allowed(
            ServiceId::EchDisplay,
            ServiceId::EchDisplay
        ));
    }
}
