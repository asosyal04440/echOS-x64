use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use lazy_static::lazy_static;
use spin::Mutex;

use crate::memory::shared_region::{self, UserMapping};
use crate::security::capability::{self, CapRights, CapabilityError, UserHandle as RawUserHandle};
use crate::services::display_atomic::MailboxRing;
use crate::services::{
    AudioResponse, CaptureResponse, ClipboardResponse, DialogResponse, DisplayResponse, EchAudio,
    EchCapture, EchClipboard, EchDialogs, EchDisplay, EchInput, EchNotifications, EchShell,
    EchStore, InputResponse, NotificationResponse, ShellResponse, StoreResponse,
};

const SERVICE_IPC_OUTGOING_CAPACITY: usize = 256;
const SERVICE_IPC_INCOMING_CAPACITY: usize = 256;
const SERVICE_RESPONSE_SPINS: usize = 200_000;

pub use crate::security::capability::UserHandle;
pub type KernelCapabilityId = crate::security::capability::CapId;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceParityStatus {
    pub required_services: u32,
    pub packaged_service_slots: u32,
    pub live_user_process_slots: u32,
    pub published_user_process_slots: u32,
    pub strict_mode_enabled: bool,
    pub full_parity_ready: bool,
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
    EchDisplay = 1,
    EchInput = 2,
    EchAudio = 3,
    EchStore = 4,
    EchShell = 5,
    EchNotifications = 6,
    EchClipboard = 7,
    EchDialogs = 8,
    EchCapture = 9,
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
    pub runtime_isolation: Option<crate::runtime::IsolationDomain>,
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

#[derive(Clone, Debug)]
pub enum ServiceMessage {
    DirectoryCommand(DirectoryCommand),
    DisplayCommand(crate::services::DisplayCommand),
    InputCommand(crate::services::InputCommand),
    AudioCommand(crate::services::AudioCommand),
    StoreCommand(crate::services::StoreCommand),
    ShellCommand(crate::services::ShellCommand),
    NotificationCommand(crate::services::NotificationCommand),
    ClipboardCommand(crate::services::ClipboardCommand),
    DialogCommand(crate::services::DialogCommand),
    CaptureCommand(crate::services::CaptureCommand),
}

#[derive(Clone, Debug)]
pub enum ServiceResponse {
    DirectoryResponse(DirectoryResponse),
    DisplayResponse(crate::services::DisplayResponse),
    InputResponse(crate::services::InputResponse),
    AudioResponse(crate::services::AudioResponse),
    StoreResponse(crate::services::StoreResponse),
    ShellResponse(crate::services::ShellResponse),
    NotificationResponse(crate::services::NotificationResponse),
    ClipboardResponse(crate::services::ClipboardResponse),
    DialogResponse(crate::services::DialogResponse),
    CaptureResponse(crate::services::CaptureResponse),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BlockingMode {
    Async,
    Sync,
}

static REQUEST_ID: AtomicU64 = AtomicU64::new(1);
static FULL_PARITY_STRICT_MODE: AtomicBool = AtomicBool::new(false);
static HEARTBEAT_EPOCH: AtomicU64 = AtomicU64::new(1);

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

#[derive(Clone)]
enum BoundServiceEndpoint {
    Display(Arc<EchDisplay>),
    Input(Arc<EchInput>),
    Audio(Arc<EchAudio>),
    Store(Arc<EchStore>),
    Shell(Arc<EchShell>),
    Notifications(Arc<EchNotifications>),
    Clipboard(Arc<EchClipboard>),
    Dialogs(Arc<EchDialogs>),
    Capture(Arc<EchCapture>),
}

#[derive(Clone)]
pub enum ServiceEndpointRegistration {
    Display(Arc<EchDisplay>),
    Input(Arc<EchInput>),
    Audio(Arc<EchAudio>),
    Store(Arc<EchStore>),
    Shell(Arc<EchShell>),
    Notifications(Arc<EchNotifications>),
    Clipboard(Arc<EchClipboard>),
    Dialogs(Arc<EchDialogs>),
    Capture(Arc<EchCapture>),
}

impl ServiceEndpointRegistration {
    fn into_bound(self) -> BoundServiceEndpoint {
        match self {
            Self::Display(endpoint) => BoundServiceEndpoint::Display(endpoint),
            Self::Input(endpoint) => BoundServiceEndpoint::Input(endpoint),
            Self::Audio(endpoint) => BoundServiceEndpoint::Audio(endpoint),
            Self::Store(endpoint) => BoundServiceEndpoint::Store(endpoint),
            Self::Shell(endpoint) => BoundServiceEndpoint::Shell(endpoint),
            Self::Notifications(endpoint) => BoundServiceEndpoint::Notifications(endpoint),
            Self::Clipboard(endpoint) => BoundServiceEndpoint::Clipboard(endpoint),
            Self::Dialogs(endpoint) => BoundServiceEndpoint::Dialogs(endpoint),
            Self::Capture(endpoint) => BoundServiceEndpoint::Capture(endpoint),
        }
    }
}

impl BoundServiceEndpoint {
    fn dispatch_sync(&self, message: ServiceMessage) -> Result<ServiceResponse, ServiceError> {
        match self {
            Self::Display(display) => match message {
                ServiceMessage::DisplayCommand(cmd) => {
                    Ok(ServiceResponse::DisplayResponse(display.process_command(cmd)))
                }
                _ => Err(ServiceError::WrongService),
            },
            Self::Input(input) => match message {
                ServiceMessage::InputCommand(cmd) => {
                    Ok(ServiceResponse::InputResponse(input.process_command(cmd)))
                }
                _ => Err(ServiceError::WrongService),
            },
            Self::Audio(audio) => match message {
                ServiceMessage::AudioCommand(cmd) => {
                    Ok(ServiceResponse::AudioResponse(audio.process_command(cmd)))
                }
                _ => Err(ServiceError::WrongService),
            },
            Self::Store(store) => match message {
                ServiceMessage::StoreCommand(cmd) => {
                    Ok(ServiceResponse::StoreResponse(store.process_command(cmd)))
                }
                _ => Err(ServiceError::WrongService),
            },
            Self::Shell(shell) => match message {
                ServiceMessage::ShellCommand(cmd) => {
                    Ok(ServiceResponse::ShellResponse(shell.process_command(cmd)))
                }
                _ => Err(ServiceError::WrongService),
            },
            Self::Notifications(notifications) => match message {
                ServiceMessage::NotificationCommand(cmd) => Ok(ServiceResponse::NotificationResponse(
                    notifications.process_command(cmd),
                )),
                _ => Err(ServiceError::WrongService),
            },
            Self::Clipboard(clipboard) => match message {
                ServiceMessage::ClipboardCommand(cmd) => Ok(ServiceResponse::ClipboardResponse(
                    clipboard.process_command(cmd),
                )),
                _ => Err(ServiceError::WrongService),
            },
            Self::Dialogs(dialogs) => match message {
                ServiceMessage::DialogCommand(cmd) => {
                    Ok(ServiceResponse::DialogResponse(dialogs.process_command(cmd)))
                }
                _ => Err(ServiceError::WrongService),
            },
            Self::Capture(capture) => match message {
                ServiceMessage::CaptureCommand(cmd) => {
                    Ok(ServiceResponse::CaptureResponse(capture.process_command(cmd)))
                }
                _ => Err(ServiceError::WrongService),
            },
        }
    }

    fn enqueue(&self, message: ServiceMessage) -> Result<(), ServiceError> {
        match self {
            Self::Display(display) => match message {
                ServiceMessage::DisplayCommand(cmd) => {
                    display.send_command(cmd).then_some(()).ok_or(ServiceError::QueueFull)
                }
                _ => Err(ServiceError::WrongService),
            },
            Self::Input(input) => match message {
                ServiceMessage::InputCommand(cmd) => {
                    input.send_command(cmd).then_some(()).ok_or(ServiceError::QueueFull)
                }
                _ => Err(ServiceError::WrongService),
            },
            Self::Audio(audio) => match message {
                ServiceMessage::AudioCommand(cmd) => {
                    audio.send_command(cmd).then_some(()).ok_or(ServiceError::QueueFull)
                }
                _ => Err(ServiceError::WrongService),
            },
            Self::Store(store) => match message {
                ServiceMessage::StoreCommand(cmd) => {
                    store.send_command(cmd).then_some(()).ok_or(ServiceError::QueueFull)
                }
                _ => Err(ServiceError::WrongService),
            },
            Self::Shell(shell) => match message {
                ServiceMessage::ShellCommand(cmd) => {
                    shell.send_command(cmd).then_some(()).ok_or(ServiceError::QueueFull)
                }
                _ => Err(ServiceError::WrongService),
            },
            Self::Notifications(notifications) => match message {
                ServiceMessage::NotificationCommand(cmd) => notifications
                    .send_command(cmd)
                    .then_some(())
                    .ok_or(ServiceError::QueueFull),
                _ => Err(ServiceError::WrongService),
            },
            Self::Clipboard(clipboard) => match message {
                ServiceMessage::ClipboardCommand(cmd) => clipboard
                    .send_command(cmd)
                    .then_some(())
                    .ok_or(ServiceError::QueueFull),
                _ => Err(ServiceError::WrongService),
            },
            Self::Dialogs(dialogs) => match message {
                ServiceMessage::DialogCommand(cmd) => {
                    dialogs.send_command(cmd).then_some(()).ok_or(ServiceError::QueueFull)
                }
                _ => Err(ServiceError::WrongService),
            },
            Self::Capture(capture) => match message {
                ServiceMessage::CaptureCommand(cmd) => {
                    capture.send_command(cmd).then_some(()).ok_or(ServiceError::QueueFull)
                }
                _ => Err(ServiceError::WrongService),
            },
        }
    }

    fn try_receive(&self) -> Option<ServiceResponse> {
        match self {
            Self::Display(display) => {
                display.receive_response().map(ServiceResponse::DisplayResponse)
            }
            Self::Input(input) => input.receive_response().map(ServiceResponse::InputResponse),
            Self::Audio(audio) => audio.receive_response().map(ServiceResponse::AudioResponse),
            Self::Store(store) => store.receive_response().map(ServiceResponse::StoreResponse),
            Self::Shell(shell) => shell.receive_response().map(ServiceResponse::ShellResponse),
            Self::Notifications(notifications) => notifications
                .receive_response()
                .map(ServiceResponse::NotificationResponse),
            Self::Clipboard(clipboard) => clipboard
                .receive_response()
                .map(ServiceResponse::ClipboardResponse),
            Self::Dialogs(dialogs) => dialogs.receive_response().map(ServiceResponse::DialogResponse),
            Self::Capture(capture) => capture.receive_response().map(ServiceResponse::CaptureResponse),
        }
    }
}

pub struct ServiceIpcManager {
    outgoing: MailboxRing<MessageEnvelope>,
    incoming: MailboxRing<ResponseEnvelope>,
    endpoints: Mutex<BTreeMap<ServiceId, BoundServiceEndpoint>>,
    endpoint_generations: Mutex<BTreeMap<ServiceId, EndpointGeneration>>,
    inflight_requests: Mutex<BTreeMap<ServiceId, VecDeque<MessageEnvelope>>>,
    pending: Mutex<BTreeMap<u64, PendingRequest>>,
    active_requests: Mutex<BTreeMap<ServiceId, ActiveServiceRequest>>,
    service_runtime_tasks: Mutex<BTreeMap<u64, ServiceId>>,
    mailbox_regions: Mutex<BTreeMap<ServiceId, (u64, u64)>>,
    published_user_endpoints: Mutex<BTreeMap<ServiceId, UserPublishedEndpointDescriptor>>,
}

impl ServiceIpcManager {
    pub fn new() -> Self {
        Self {
            outgoing: MailboxRing::with_capacity_pow2(SERVICE_IPC_OUTGOING_CAPACITY),
            incoming: MailboxRing::with_capacity_pow2(SERVICE_IPC_INCOMING_CAPACITY),
            endpoints: Mutex::new(BTreeMap::new()),
            endpoint_generations: Mutex::new(BTreeMap::new()),
            inflight_requests: Mutex::new(BTreeMap::new()),
            pending: Mutex::new(BTreeMap::new()),
            active_requests: Mutex::new(BTreeMap::new()),
            service_runtime_tasks: Mutex::new(BTreeMap::new()),
            mailbox_regions: Mutex::new(BTreeMap::new()),
            published_user_endpoints: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn register_service_runtime_task(&self, service: ServiceId, task_id: u64) {
        self.service_runtime_tasks.lock().insert(task_id, service);
    }

    pub fn bind_runtime_endpoints(&self) {}

    pub fn register_endpoint(&self, service: ServiceId, endpoint: ServiceEndpointRegistration) {
        let mut endpoints = self.endpoints.lock();
        let mut generations = self.endpoint_generations.lock();
        publish_endpoint(&mut endpoints, &mut generations, service, endpoint.into_bound());
    }

    pub fn grant_service_mailbox_regions(
        &self,
        pid: u32,
        service: ServiceId,
    ) -> Result<ServiceMailboxLease, ServiceError> {
        let service_pid = pid as u64;
        let (request_region_id, response_region_id) = {
            let mut regions = self.mailbox_regions.lock();
            if let Some(existing) = regions.get(&service).copied() {
                existing
            } else {
                let request = shared_region::create_ipc_region(
                    service_pid,
                    service_mailbox_region_name(service, "request").as_str(),
                    64 * 1024,
                    true,
                );
                let response = shared_region::create_ipc_region(
                    service_pid,
                    service_mailbox_region_name(service, "response").as_str(),
                    64 * 1024,
                    true,
                );
                let ids = (request.id, response.id);
                regions.insert(service, ids);
                ids
            }
        };
        let request_region = self.grant_shared_region_handle(pid, request_region_id)?;
        let response_region = self.grant_shared_region_handle(pid, response_region_id)?;
        Ok(ServiceMailboxLease {
            request_region,
            response_region,
        })
    }

    pub fn publish_user_endpoint(
        &self,
        pid: u32,
        service: ServiceId,
        request_region_handle: UserHandle,
        response_region_handle: UserHandle,
    ) -> Result<UserPublishedEndpointDescriptor, ServiceError> {
        let task_id = crate::task::scheduler::current_task_id() as u64;
        let current_service = self.current_service_context().ok_or(ServiceError::RightsDenied)?;
        if current_service != service {
            return Err(ServiceError::RightsDenied);
        }
        let request_region = capability::resolve_shared_region_handle(
            pid as u64,
            request_region_handle,
            CapRights::READ_WRITE,
        )
        .map_err(map_capability_error)?;
        let response_region = capability::resolve_shared_region_handle(
            pid as u64,
            response_region_handle,
            CapRights::READ_WRITE,
        )
        .map_err(map_capability_error)?;
        if !shared_region::region_generation_matches(
            request_region.region_id,
            request_region.region_generation,
        ) || !shared_region::region_generation_matches(
            response_region.region_id,
            response_region.region_generation,
        ) {
            return Err(ServiceError::StaleGeneration);
        }
        let descriptor = UserPublishedEndpointDescriptor {
            owner_pid: pid as u64,
            task_id,
            request_region_id: request_region.region_id,
            request_generation: request_region.region_generation,
            response_region_id: response_region.region_id,
            response_generation: response_region.region_generation,
            heartbeat_epoch: HEARTBEAT_EPOCH.fetch_add(1, Ordering::Relaxed),
        };
        self.published_user_endpoints
            .lock()
            .insert(service, descriptor);
        Ok(descriptor)
    }

    pub fn heartbeat_user_endpoint(
        &self,
        pid: u32,
        service: ServiceId,
    ) -> Result<UserPublishedEndpointDescriptor, ServiceError> {
        let current_service = self.current_service_context().ok_or(ServiceError::RightsDenied)?;
        if current_service != service {
            return Err(ServiceError::RightsDenied);
        }
        let mut endpoints = self.published_user_endpoints.lock();
        let descriptor = endpoints
            .get_mut(&service)
            .ok_or(ServiceError::ServiceUnavailable)?;
        if descriptor.owner_pid != pid as u64 {
            return Err(ServiceError::RightsDenied);
        }
        descriptor.heartbeat_epoch = HEARTBEAT_EPOCH.fetch_add(1, Ordering::Relaxed);
        Ok(*descriptor)
    }

    pub fn published_user_endpoint(
        &self,
        service: ServiceId,
    ) -> Option<UserPublishedEndpointDescriptor> {
        self.published_user_endpoints.lock().get(&service).copied()
    }

    fn ensure_runtime_endpoint(&self, service: ServiceId) -> Option<BoundServiceEndpoint> {
        if service == ServiceId::Directory {
            return None;
        }
        self.endpoints.lock().get(&service).cloned()
    }

    fn endpoint_generation(&self, service: ServiceId) -> EndpointGeneration {
        if service == ServiceId::Directory {
            return 1;
        }
        self.endpoint_generations
            .lock()
            .get(&service)
            .copied()
            .unwrap_or(0)
    }

    fn current_service_context(&self) -> Option<ServiceId> {
        let task_id = crate::task::scheduler::current_task_id() as u64;
        self.service_runtime_tasks.lock().get(&task_id).copied()
    }

    fn current_active_request(&self) -> Option<ActiveServiceRequest> {
        let service = self.current_service_context()?;
        self.active_requests.lock().get(&service).copied()
    }

    fn service_has_runtime_task(&self, service: ServiceId) -> bool {
        self.service_runtime_tasks
            .lock()
            .values()
            .any(|registered| *registered == service)
    }

    fn submit_to_runtime_queue(
        &self,
        endpoint: &BoundServiceEndpoint,
        envelope: MessageEnvelope,
    ) -> Result<(), ServiceError> {
        self.active_requests.lock().insert(
            envelope.to_service,
            ActiveServiceRequest {
                request_token: envelope.request_token,
                origin_endpoint: envelope.origin_endpoint.or(Some(envelope.to_service)),
            },
        );
        if let Err(error) = endpoint.enqueue(envelope.message.clone()) {
            self.active_requests.lock().remove(&envelope.to_service);
            return Err(error);
        }
        self.inflight_requests
            .lock()
            .entry(envelope.to_service)
            .or_default()
            .push_back(envelope);
        Ok(())
    }

    fn complete_immediate_response(&self, envelope: &MessageEnvelope, response: ServiceResponse) {
        {
            let mut pending = self.pending.lock();
            if let Some(entry) = pending.get_mut(&envelope.id) {
                entry.response = Some(response.clone());
            }
        }
        let _ = self.incoming.try_push(ResponseEnvelope {
            message_id: envelope.id,
            request_token: envelope.request_token,
            response,
        });
    }

    fn poll_runtime_responses(&self) {
        let services = self
            .inflight_requests
            .lock()
            .keys()
            .copied()
            .collect::<Vec<_>>();
        for service in services {
            let Some(endpoint) = self.ensure_runtime_endpoint(service) else {
                continue;
            };
            while let Some(response) = endpoint.try_receive() {
                let envelope = {
                    let mut inflight = self.inflight_requests.lock();
                    let Some(queue) = inflight.get_mut(&service) else {
                        break;
                    };
                    let envelope = queue.pop_front();
                    if queue.is_empty() {
                        inflight.remove(&service);
                    }
                    envelope
                };
                let Some(envelope) = envelope else {
                    break;
                };
                self.active_requests.lock().remove(&service);
                self.complete_immediate_response(&envelope, response);
            }
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

    pub fn open_service_handle(
        &self,
        pid: u32,
        service: ServiceId,
        rights: CapabilityRights,
    ) -> Result<ServiceHandleDescriptor, ServiceError> {
        if service != ServiceId::Directory && self.ensure_runtime_endpoint(service).is_none() {
            return Err(ServiceError::ServiceUnavailable);
        }
        let endpoint_generation = self.endpoint_generation(service);
        if endpoint_generation == 0 {
            return Err(ServiceError::ServiceUnavailable);
        }
        let handle = capability::open_service_handle(
            pid as u64,
            service as u64,
            rights,
            endpoint_generation,
            self.current_service_context().map(|service| service as u64),
        )
        .map_err(map_capability_error)?;
        Ok(ServiceHandleDescriptor {
            handle,
            service_id: service,
            endpoint_generation,
            rights,
        })
    }

    pub fn grant_shared_region_handle(
        &self,
        pid: u32,
        region_id: u64,
    ) -> Result<SharedRegionDescriptor, ServiceError> {
        let lease =
            shared_region::snapshot_ipc_region(region_id).ok_or(ServiceError::SharedRegionUnavailable)?;
        let handle = capability::grant_shared_region_handle(
            pid as u64,
            lease.id,
            lease.generation,
            lease.len,
            lease.writable,
        )
        .map_err(map_capability_error)?;
        Ok(SharedRegionDescriptor {
            handle,
            region_id: lease.id,
            generation: lease.generation,
            len: lease.len,
            writable: lease.writable,
        })
    }

    pub fn map_shared_region(
        &self,
        pid: u32,
        region_handle: UserHandle,
    ) -> Result<UserMapping, ServiceError> {
        let descriptor = capability::resolve_shared_region_handle(
            pid as u64,
            region_handle,
            CapRights::READ,
        )
        .map_err(map_capability_error)?;
        if !shared_region::region_generation_matches(
            descriptor.region_id,
            descriptor.region_generation,
        ) {
            return Err(ServiceError::StaleGeneration);
        }
        if let Some(space) = crate::runtime::runtime_address_space_for_pid(pid as u64) {
            shared_region::map_ipc_region_into_space(pid as u64, descriptor.region_id, &space)
                .ok_or(ServiceError::SharedRegionUnavailable)
        } else {
            shared_region::map_ipc_region(pid as u64, descriptor.region_id)
                .ok_or(ServiceError::SharedRegionUnavailable)
        }
    }

    pub fn revoke_handle(&self, pid: u32, handle: RawUserHandle) -> Result<(), ServiceError> {
        capability::revoke_handle(pid as u64, handle).map_err(map_capability_error)
    }

    pub fn unmap_shared_region(
        &self,
        pid: u32,
        region_handle: UserHandle,
    ) -> Result<(), ServiceError> {
        let descriptor = capability::resolve_shared_region_handle(
            pid as u64,
            region_handle,
            CapRights::READ,
        )
        .map_err(map_capability_error)?;
        if !shared_region::region_generation_matches(
            descriptor.region_id,
            descriptor.region_generation,
        ) {
            return Err(ServiceError::StaleGeneration);
        }
        if shared_region::unmap_ipc_region(pid as u64, descriptor.region_id) {
            Ok(())
        } else {
            Err(ServiceError::SharedRegionUnavailable)
        }
    }

    pub fn send_request(
        &self,
        pid: u32,
        handle: UserHandle,
        message: ServiceMessage,
    ) -> Result<RequestTokenDescriptor, ServiceError> {
        self.enqueue_request(pid, handle, message, BlockingMode::Async)
    }

    fn enqueue_request(
        &self,
        pid: u32,
        handle: UserHandle,
        message: ServiceMessage,
        blocking_mode: BlockingMode,
    ) -> Result<RequestTokenDescriptor, ServiceError> {
        let record = capability::resolve_service_handle(pid as u64, handle, CapRights::WRITE)
            .map_err(map_capability_error)?;
        let service = service_id_from_u64(record.service_id).ok_or(ServiceError::WrongService)?;
        let current_generation = self.endpoint_generation(service);
        if current_generation == 0 {
            return Err(ServiceError::ServiceUnavailable);
        }
        if current_generation != record.endpoint_generation {
            return Err(ServiceError::EndpointRestarted);
        }
        let request_id = REQUEST_ID.fetch_add(1, Ordering::SeqCst);
        let token_handle = capability::grant_request_handle(
            pid as u64,
            request_id,
            record.service_id,
            record.endpoint_generation,
            record.owner_endpoint,
        )
        .map_err(map_capability_error)?;
        let current_service = self.current_service_context();
        let active_request = self.current_active_request();
        let descriptor = RequestTokenDescriptor {
            token: RequestToken(token_handle),
            request_id,
            service_id: service,
            endpoint_generation: record.endpoint_generation,
        };
        self.pending.lock().insert(
            request_id,
            PendingRequest {
                owner_pid: pid as u64,
                request_token: descriptor.token,
                response: None,
            },
        );
        let envelope = MessageEnvelope {
            id: request_id,
            owner_pid: pid as u64,
            from_app: pid,
            request_token: descriptor.token,
            to_service: service,
            endpoint_generation: record.endpoint_generation,
            origin_endpoint: active_request
                .and_then(|request| request.origin_endpoint)
                .or(current_service),
            blocking_mode,
            causal_parent_token: active_request.map(|request| request.request_token),
            message,
        };
        if self.outgoing.try_push(envelope).is_err() {
            let _ = self.pending.lock().remove(&request_id);
            let _ = capability::revoke_handle(pid as u64, token_handle);
            return Err(ServiceError::QueueFull);
        }
        Ok(descriptor)
    }

    pub fn take_response(
        &self,
        pid: u32,
        token: RequestToken,
    ) -> Result<Option<ServiceResponse>, ServiceError> {
        let request = capability::resolve_request_handle(pid as u64, token.0)
            .map_err(map_capability_error)?;
        let response = self
            .pending
            .lock()
            .get(&request.request_id)
            .and_then(|pending| pending.response.clone());
        if let Some(response) = response {
            self.discard_buffered_response(request.request_id);
            self.complete_request(request.request_id);
            Ok(Some(response))
        } else {
            Ok(None)
        }
    }

    pub fn request_sync(
        &self,
        pid: u32,
        handle: UserHandle,
        message: ServiceMessage,
    ) -> Result<ServiceResponse, ServiceError> {
        let service = capability::resolve_service_handle(pid as u64, handle, CapRights::WRITE)
            .map_err(map_capability_error)?
            .service_id;
        let service = service_id_from_u64(service).ok_or(ServiceError::WrongService)?;
        if self.sync_cycle_risk(service) {
            return Err(ServiceError::SyncCycleRisk);
        }
        let descriptor = self.enqueue_request(pid, handle, message, BlockingMode::Sync)?;
        for _ in 0..SERVICE_RESPONSE_SPINS {
            self.process_pending_messages();
            if let Some(response) = self.take_response(pid, descriptor.token)? {
                return Ok(response);
            }
            core::hint::spin_loop();
        }
        Err(ServiceError::ServiceUnavailable)
    }

    pub fn send_to_service(&self, app_id: u32, service: ServiceId, message: ServiceMessage) -> u64 {
        let Ok(handle) = self.open_service_handle(app_id, service, default_rights_for(service)) else {
            return 0;
        };
        match self.send_request(app_id, handle.handle, message) {
            Ok(descriptor) => descriptor.request_id,
            Err(_) => {
                let _ = self.revoke_handle(app_id, handle.handle);
                0
            }
        }
    }

    pub fn receive_from_service(&self) -> Option<ResponseEnvelope> {
        let response = self.incoming.pop()?;
        self.complete_request(response.message_id);
        Some(response)
    }

    pub fn receive_from_service_timeout(&self, _timeout_ms: u32) -> Option<ResponseEnvelope> {
        self.receive_from_service()
    }

    pub fn request_sync_legacy(
        &self,
        app_id: u32,
        service: ServiceId,
        message: ServiceMessage,
    ) -> ServiceResponse {
        let Ok(handle) = self.open_service_handle(app_id, service, default_rights_for(service)) else {
            return service_unavailable_response(service);
        };
        let response = self
            .request_sync(app_id, handle.handle, message)
            .unwrap_or_else(|_| service_unavailable_response(service));
        let _ = self.revoke_handle(app_id, handle.handle);
        response
    }

    pub fn take_response_for(&self, message_id: u64) -> Option<ResponseEnvelope> {
        let request_token = self
            .pending
            .lock()
            .get(&message_id)
            .map(|pending| pending.request_token)?;
        let response = self
            .pending
            .lock()
            .get(&message_id)
            .and_then(|pending| pending.response.clone())?;
        self.discard_buffered_response(message_id);
        self.complete_request(message_id);
        Some(ResponseEnvelope {
            message_id,
            request_token,
            response,
        })
    }

    pub fn process_pending_messages(&self) {
        while let Some(envelope) = self.outgoing.pop() {
            if envelope.to_service == ServiceId::Directory {
                self.complete_immediate_response(
                    &envelope,
                    dispatch_directory_command(envelope.message.clone()),
                );
                continue;
            }
            let current_generation = self.endpoint_generation(envelope.to_service);
            if current_generation == 0 {
                self.complete_immediate_response(
                    &envelope,
                    service_unavailable_response(envelope.to_service),
                );
                continue;
            }
            if current_generation != envelope.endpoint_generation {
                self.complete_immediate_response(
                    &envelope,
                    error_response(envelope.to_service, ServiceError::EndpointRestarted),
                );
                continue;
            }
            let Some(endpoint) = self.ensure_runtime_endpoint(envelope.to_service) else {
                self.complete_immediate_response(
                    &envelope,
                    service_unavailable_response(envelope.to_service),
                );
                continue;
            };
            if self.service_has_runtime_task(envelope.to_service) {
                let response = self
                    .submit_to_runtime_queue(&endpoint, envelope.clone())
                    .err()
                    .map(|error| error_response(envelope.to_service, error));
                if let Some(response) = response {
                    self.complete_immediate_response(&envelope, response);
                }
            } else {
                let response = self.dispatch_to_service(&envelope);
                self.complete_immediate_response(&envelope, response);
            }
        }
        self.poll_runtime_responses();
    }

    fn dispatch_to_service(&self, envelope: &MessageEnvelope) -> ServiceResponse {
        if envelope.to_service == ServiceId::Directory {
            return dispatch_directory_command(envelope.message.clone());
        }
        let current_generation = self.endpoint_generation(envelope.to_service);
        if current_generation == 0 {
            return service_unavailable_response(envelope.to_service);
        }
        if current_generation != envelope.endpoint_generation {
            return error_response(envelope.to_service, ServiceError::EndpointRestarted);
        }
        let Some(endpoint) = self.ensure_runtime_endpoint(envelope.to_service) else {
            return service_unavailable_response(envelope.to_service);
        };
        self.active_requests.lock().insert(
            envelope.to_service,
            ActiveServiceRequest {
                request_token: envelope.request_token,
                origin_endpoint: envelope.origin_endpoint.or(Some(envelope.to_service)),
            },
        );
        let response = endpoint
            .dispatch_sync(envelope.message.clone())
            .unwrap_or_else(|error| error_response(envelope.to_service, error));
        self.active_requests.lock().remove(&envelope.to_service);
        response
    }

    fn discard_buffered_response(&self, message_id: u64) {
        let mut buffered = Vec::new();
        while let Some(response) = self.incoming.pop() {
            if response.message_id != message_id {
                buffered.push(response);
            }
        }
        for response in buffered {
            let _ = self.incoming.try_push(response);
        }
    }

    fn complete_request(&self, message_id: u64) {
        let pending = self.pending.lock().remove(&message_id);
        if let Some(pending) = pending {
            let _ = capability::revoke_handle(pending.owner_pid, pending.request_token.0);
        }
    }
}

fn publish_endpoint(
    endpoints: &mut BTreeMap<ServiceId, BoundServiceEndpoint>,
    generations: &mut BTreeMap<ServiceId, EndpointGeneration>,
    service: ServiceId,
    endpoint: BoundServiceEndpoint,
) {
    endpoints.insert(service, endpoint);
    let generation = generations.entry(service).or_insert(0);
    *generation = generation.saturating_add(1).max(1);
}

fn blocking_sync_allowed(current: ServiceId, target: ServiceId) -> bool {
    matches!(
        (current, target),
        (ServiceId::EchInput, ServiceId::EchDisplay)
            | (ServiceId::EchCapture, ServiceId::EchDisplay)
            | (ServiceId::EchCapture, ServiceId::EchShell)
            | (ServiceId::EchClipboard, ServiceId::EchShell)
            | (ServiceId::EchDialogs, ServiceId::EchShell)
            | (ServiceId::EchNotifications, ServiceId::EchShell)
    )
}

fn map_capability_error(error: CapabilityError) -> ServiceError {
    match error {
        CapabilityError::ProcessNotInitialized | CapabilityError::InvalidHandle => {
            ServiceError::InvalidHandle
        }
        CapabilityError::Revoked => ServiceError::Revoked,
        CapabilityError::RightsDenied => ServiceError::RightsDenied,
        CapabilityError::WrongKind => ServiceError::WrongService,
        CapabilityError::StaleGeneration => ServiceError::StaleGeneration,
    }
}

fn default_rights_for(service: ServiceId) -> CapabilityRights {
    match service {
        ServiceId::Directory => CapRights::READ,
        _ => CapRights::READ_WRITE,
    }
}

fn service_id_from_u64(value: u64) -> Option<ServiceId> {
    match value {
        0 => Some(ServiceId::Directory),
        1 => Some(ServiceId::EchDisplay),
        2 => Some(ServiceId::EchInput),
        3 => Some(ServiceId::EchAudio),
        4 => Some(ServiceId::EchStore),
        5 => Some(ServiceId::EchShell),
        6 => Some(ServiceId::EchNotifications),
        7 => Some(ServiceId::EchClipboard),
        8 => Some(ServiceId::EchDialogs),
        9 => Some(ServiceId::EchCapture),
        _ => None,
    }
}

fn dispatch_directory_command(message: ServiceMessage) -> ServiceResponse {
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
        ServiceId::Directory => ServiceResponse::DirectoryResponse(DirectoryResponse::Service(None)),
        ServiceId::EchDisplay => ServiceResponse::DisplayResponse(DisplayResponse::Error(message)),
        ServiceId::EchInput => ServiceResponse::InputResponse(InputResponse::Error(message)),
        ServiceId::EchAudio => ServiceResponse::AudioResponse(AudioResponse::Error(message)),
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

fn service_directory() -> Vec<ServiceDescriptor> {
    fn runtime_fields(
        service_id: ServiceId,
        service_slug: &str,
    ) -> (
        bool,
        Option<crate::runtime::IsolationDomain>,
        Option<u64>,
        Option<String>,
        bool,
    ) {
        let available = crate::runtime::service_process_available(service_slug);
        let runtime = crate::runtime::runtime_handle_for_service(service_id);
        let published = get_service_ipc().published_user_endpoint(service_id).is_some();
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
    ) =
        runtime_fields(ServiceId::EchNotifications, "ech_notifications");
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
            runtime_isolation: Some(crate::runtime::IsolationDomain::KernelTask),
            runtime_task_id: None,
            runtime_image_path: None,
            user_published_endpoint: false,
        },
        ServiceDescriptor {
            id: ServiceId::EchDisplay,
            name: String::from("EchDisplay"),
            class: ServiceClass::Ui,
            control_plane: true,
            bulk_data_out_of_band: true,
            openable_rights: CapRights::READ_WRITE,
            bulk_region_classes: vec![String::from("surface")],
            service_process_available: display_process,
            runtime_isolation: display_iso,
            runtime_task_id: display_task,
            runtime_image_path: display_image,
            user_published_endpoint: display_published,
        },
        ServiceDescriptor {
            id: ServiceId::EchInput,
            name: String::from("EchInput"),
            class: ServiceClass::Input,
            control_plane: true,
            bulk_data_out_of_band: false,
            openable_rights: CapRights::READ_WRITE,
            bulk_region_classes: Vec::new(),
            service_process_available: input_process,
            runtime_isolation: input_iso,
            runtime_task_id: input_task,
            runtime_image_path: input_image,
            user_published_endpoint: input_published,
        },
        ServiceDescriptor {
            id: ServiceId::EchAudio,
            name: String::from("EchAudio"),
            class: ServiceClass::Media,
            control_plane: true,
            bulk_data_out_of_band: true,
            openable_rights: CapRights::READ_WRITE,
            bulk_region_classes: vec![String::from("audio-stream")],
            service_process_available: audio_process,
            runtime_isolation: audio_iso,
            runtime_task_id: audio_task,
            runtime_image_path: audio_image,
            user_published_endpoint: audio_published,
        },
        ServiceDescriptor {
            id: ServiceId::EchStore,
            name: String::from("EchStore"),
            class: ServiceClass::Storage,
            control_plane: true,
            bulk_data_out_of_band: true,
            openable_rights: CapRights::READ_WRITE,
            bulk_region_classes: vec![String::from("file-io")],
            service_process_available: store_process,
            runtime_isolation: store_iso,
            runtime_task_id: store_task,
            runtime_image_path: store_image,
            user_published_endpoint: store_published,
        },
        ServiceDescriptor {
            id: ServiceId::EchShell,
            name: String::from("EchShell"),
            class: ServiceClass::Session,
            control_plane: true,
            bulk_data_out_of_band: false,
            openable_rights: CapRights::READ_WRITE,
            bulk_region_classes: Vec::new(),
            service_process_available: shell_process,
            runtime_isolation: shell_iso,
            runtime_task_id: shell_task,
            runtime_image_path: shell_image,
            user_published_endpoint: shell_published,
        },
        ServiceDescriptor {
            id: ServiceId::EchNotifications,
            name: String::from("EchNotifications"),
            class: ServiceClass::Session,
            control_plane: true,
            bulk_data_out_of_band: false,
            openable_rights: CapRights::READ_WRITE,
            bulk_region_classes: Vec::new(),
            service_process_available: notifications_process,
            runtime_isolation: notifications_iso,
            runtime_task_id: notifications_task,
            runtime_image_path: notifications_image,
            user_published_endpoint: notifications_published,
        },
        ServiceDescriptor {
            id: ServiceId::EchClipboard,
            name: String::from("EchClipboard"),
            class: ServiceClass::Session,
            control_plane: true,
            bulk_data_out_of_band: true,
            openable_rights: CapRights::READ_WRITE,
            bulk_region_classes: vec![String::from("clipboard-payload")],
            service_process_available: clipboard_process,
            runtime_isolation: clipboard_iso,
            runtime_task_id: clipboard_task,
            runtime_image_path: clipboard_image,
            user_published_endpoint: clipboard_published,
        },
        ServiceDescriptor {
            id: ServiceId::EchDialogs,
            name: String::from("EchDialogs"),
            class: ServiceClass::Session,
            control_plane: true,
            bulk_data_out_of_band: false,
            openable_rights: CapRights::READ_WRITE,
            bulk_region_classes: Vec::new(),
            service_process_available: dialogs_process,
            runtime_isolation: dialogs_iso,
            runtime_task_id: dialogs_task,
            runtime_image_path: dialogs_image,
            user_published_endpoint: dialogs_published,
        },
        ServiceDescriptor {
            id: ServiceId::EchCapture,
            name: String::from("EchCapture"),
            class: ServiceClass::Integration,
            control_plane: true,
            bulk_data_out_of_band: true,
            openable_rights: CapRights::READ_WRITE,
            bulk_region_classes: vec![String::from("screenshot")],
            service_process_available: capture_process,
            runtime_isolation: capture_iso,
            runtime_task_id: capture_task,
            runtime_image_path: capture_image,
            user_published_endpoint: capture_published,
        },
    ]
}

fn service_descriptor(service_id: ServiceId) -> Option<ServiceDescriptor> {
    service_directory()
        .into_iter()
        .find(|descriptor| descriptor.id == service_id)
}

fn service_mailbox_region_name(service: ServiceId, lane: &str) -> String {
    let slug = match service {
        ServiceId::Directory => "service_directory",
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

fn compute_service_parity_status() -> ServiceParityStatus {
    let descriptors = service_directory();
    let required_services = REQUIRED_PARITY_SERVICES.len() as u32;
    let mut packaged_service_slots = 0u32;
    let mut live_user_process_slots = 0u32;
    let mut published_user_process_slots = 0u32;

    for service_id in REQUIRED_PARITY_SERVICES {
        let Some(descriptor) = descriptors.iter().find(|descriptor| descriptor.id == service_id) else {
            continue;
        };
        if descriptor.service_process_available {
            packaged_service_slots = packaged_service_slots.saturating_add(1);
        }
        if descriptor.runtime_isolation == Some(crate::runtime::IsolationDomain::UserProcess) {
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

lazy_static! {
    static ref SERVICE_IPC: ServiceIpcManager = ServiceIpcManager::new();
}

pub fn init() {
    SERVICE_IPC.bind_runtime_endpoints();
    crate::serial_println!("[SERVICE_IPC] Initialized");
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
    get_service_ipc().publish_user_endpoint(pid, service, request_region_handle, response_region_handle)
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

pub fn map_shared_region(
    pid: u32,
    region_handle: UserHandle,
) -> Result<UserMapping, ServiceError> {
    get_service_ipc().map_shared_region(pid, region_handle)
}

pub fn unmap_shared_region(pid: u32, region_handle: UserHandle) -> Result<(), ServiceError> {
    get_service_ipc().unmap_shared_region(pid, region_handle)
}

pub fn send_to_display(app_id: u32, command: crate::services::DisplayCommand) -> u64 {
    get_service_ipc().send_to_service(
        app_id,
        ServiceId::EchDisplay,
        ServiceMessage::DisplayCommand(command),
    )
}

pub fn request_directory_sync(app_id: u32, command: DirectoryCommand) -> Option<DirectoryResponse> {
    match get_service_ipc().request_sync_legacy(
        app_id,
        ServiceId::Directory,
        ServiceMessage::DirectoryCommand(command),
    ) {
        ServiceResponse::DirectoryResponse(response) => Some(response),
        _ => None,
    }
}

pub fn request_display_sync(
    app_id: u32,
    command: crate::services::DisplayCommand,
) -> Option<crate::services::DisplayResponse> {
    match get_service_ipc().request_sync_legacy(
        app_id,
        ServiceId::EchDisplay,
        ServiceMessage::DisplayCommand(command),
    ) {
        ServiceResponse::DisplayResponse(response) => Some(response),
        _ => None,
    }
}

pub fn request_input_sync(
    app_id: u32,
    command: crate::services::InputCommand,
) -> Option<crate::services::InputResponse> {
    match get_service_ipc().request_sync_legacy(
        app_id,
        ServiceId::EchInput,
        ServiceMessage::InputCommand(command),
    ) {
        ServiceResponse::InputResponse(response) => Some(response),
        _ => None,
    }
}

pub fn send_to_input(app_id: u32, command: crate::services::InputCommand) -> u64 {
    get_service_ipc().send_to_service(
        app_id,
        ServiceId::EchInput,
        ServiceMessage::InputCommand(command),
    )
}

pub fn send_to_audio(app_id: u32, command: crate::services::AudioCommand) -> u64 {
    get_service_ipc().send_to_service(
        app_id,
        ServiceId::EchAudio,
        ServiceMessage::AudioCommand(command),
    )
}

pub fn request_audio_sync(
    app_id: u32,
    command: crate::services::AudioCommand,
) -> Option<crate::services::AudioResponse> {
    match get_service_ipc().request_sync_legacy(
        app_id,
        ServiceId::EchAudio,
        ServiceMessage::AudioCommand(command),
    ) {
        ServiceResponse::AudioResponse(response) => Some(response),
        _ => None,
    }
}

pub fn send_to_store(app_id: u32, command: crate::services::StoreCommand) -> u64 {
    get_service_ipc().send_to_service(
        app_id,
        ServiceId::EchStore,
        ServiceMessage::StoreCommand(command),
    )
}

pub fn request_store_sync(
    app_id: u32,
    command: crate::services::StoreCommand,
) -> Option<crate::services::StoreResponse> {
    match get_service_ipc().request_sync_legacy(
        app_id,
        ServiceId::EchStore,
        ServiceMessage::StoreCommand(command),
    ) {
        ServiceResponse::StoreResponse(response) => Some(response),
        _ => None,
    }
}

pub fn request_shell_sync(
    app_id: u32,
    command: crate::services::ShellCommand,
) -> Option<crate::services::ShellResponse> {
    match get_service_ipc().request_sync_legacy(
        app_id,
        ServiceId::EchShell,
        ServiceMessage::ShellCommand(command),
    ) {
        ServiceResponse::ShellResponse(response) => Some(response),
        _ => None,
    }
}

pub fn request_notification_sync(
    app_id: u32,
    command: crate::services::NotificationCommand,
) -> Option<crate::services::NotificationResponse> {
    match get_service_ipc().request_sync_legacy(
        app_id,
        ServiceId::EchNotifications,
        ServiceMessage::NotificationCommand(command),
    ) {
        ServiceResponse::NotificationResponse(response) => Some(response),
        _ => None,
    }
}

pub fn request_clipboard_sync(
    app_id: u32,
    command: crate::services::ClipboardCommand,
) -> Option<crate::services::ClipboardResponse> {
    match get_service_ipc().request_sync_legacy(
        app_id,
        ServiceId::EchClipboard,
        ServiceMessage::ClipboardCommand(command),
    ) {
        ServiceResponse::ClipboardResponse(response) => Some(response),
        _ => None,
    }
}

pub fn request_dialog_sync(
    app_id: u32,
    command: crate::services::DialogCommand,
) -> Option<crate::services::DialogResponse> {
    match get_service_ipc().request_sync_legacy(
        app_id,
        ServiceId::EchDialogs,
        ServiceMessage::DialogCommand(command),
    ) {
        ServiceResponse::DialogResponse(response) => Some(response),
        _ => None,
    }
}

pub fn request_capture_sync(
    app_id: u32,
    command: crate::services::CaptureCommand,
) -> Option<crate::services::CaptureResponse> {
    match get_service_ipc().request_sync_legacy(
        app_id,
        ServiceId::EchCapture,
        ServiceMessage::CaptureCommand(command),
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
    crate::task::scheduler::spawn_with_priority(
        service_task,
        crate::task::task::Priority::Low,
        "service_ipc",
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::sync::Arc;

    #[test]
    fn directory_lists_core_service_bus_endpoints() {
        let response = request_directory_sync(0, DirectoryCommand::ListServices)
            .expect("directory response");
        let DirectoryResponse::Services(services) = response else {
            unreachable!("unexpected directory response");
        };
        assert!(services.iter().any(|entry| entry.id == ServiceId::EchShell));
        assert!(services.iter().any(|entry| entry.id == ServiceId::EchDisplay));
        assert!(services.iter().any(|entry| entry.id == ServiceId::EchCapture));
    }

    #[test]
    fn async_directory_round_trip_uses_handle_and_token() {
        let ipc = ServiceIpcManager::new();
        let handle = ipc
            .open_service_handle(7, ServiceId::Directory, CapRights::READ)
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
        let ServiceResponse::DirectoryResponse(DirectoryResponse::Service(Some(service))) = response
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
        let notifications = Arc::new(crate::services::EchNotifications::new());
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
                ServiceMessage::NotificationCommand(crate::services::NotificationCommand::Clear {
                    app_id: 1,
                }),
            )
            .expect("queued request");
        ipc.process_pending_messages();
        assert!(
            ipc.take_response(31, token.token)
                .expect("response lookup")
                .is_none()
        );
        assert_eq!(
            ipc.inflight_requests
                .lock()
                .get(&ServiceId::EchNotifications)
                .map(|queue| queue.len()),
            Some(1)
        );
    }

    #[test]
    fn bootstrap_fallback_still_returns_direct_response_before_runtime_task() {
        let ipc = ServiceIpcManager::new();
        let notifications = Arc::new(crate::services::EchNotifications::new());
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
                ServiceMessage::NotificationCommand(crate::services::NotificationCommand::Clear {
                    app_id: 1,
                }),
            )
            .expect("queued request");
        ipc.process_pending_messages();
        let response = ipc
            .take_response(32, token.token)
            .expect("response lookup")
            .expect("completed response");
        assert!(matches!(
            response,
            ServiceResponse::NotificationResponse(
                crate::services::NotificationResponse::Ack
            )
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
