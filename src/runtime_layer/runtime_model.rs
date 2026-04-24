use super::super::gui::launch_pipeline::{
    AppIdentity, AppManifest, CapabilityProfile, ExternalDisplayContract, LaunchSession,
    LaunchSource,
};
use super::super::gui::protocol::{AppId, SurfaceId, WindowId, WorkspaceId};
use super::super::ipc::ServiceId;
use alloc::string::String;
use alloc::vec::Vec;

pub type RuntimeHandleId = u64;
pub type CapabilityTokenId = u64;
pub type ProcessBrokerTicket = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalRuntimeKind {
    GenericPe,
    BrowserShell,
    BrowserHelper,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalRuntimeStage {
    Reserved,
    ImportPreflightBlocked,
    Spawned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeGraphBoundaryState {
    Closed,
    Open,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalRuntimeHelperRole {
    SandboxBroker,
    GpuHelper,
    RendererHelper,
    NetworkHelper,
    CrashReporter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalRuntimeHelperState {
    Expected,
    BlockedByImportGraph,
    ReadyToSpawn,
    BrokerReserved,
    BridgeAttached,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalRuntimeHelper {
    pub role: ExternalRuntimeHelperRole,
    pub state: ExternalRuntimeHelperState,
    pub blocker_import: Option<String>,
    pub broker_ticket: Option<ProcessBrokerTicket>,
    pub runtime_id: Option<RuntimeHandleId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalRuntimeWorkflow {
    pub image_path: Option<String>,
    pub working_directory: Option<String>,
    pub download_root: Option<String>,
    pub open_folder_root: Option<String>,
    pub command_line_contract_state: RuntimeGraphBoundaryState,
    pub environment_contract_state: RuntimeGraphBoundaryState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalImportBoundary {
    pub dll_name: String,
    pub symbol_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalRuntimeGraph {
    pub kind: ExternalRuntimeKind,
    pub stage: ExternalRuntimeStage,
    pub import_graph_closed: bool,
    pub helper_graph_state: RuntimeGraphBoundaryState,
    pub imported_modules: Vec<String>,
    pub unresolved_imports: Vec<ExternalImportBoundary>,
    pub primary_blocker: Option<String>,
    pub boundary_reason: Option<String>,
    pub helpers: Vec<ExternalRuntimeHelper>,
    pub workflow: ExternalRuntimeWorkflow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessClass {
    NativeWindowed,
    NativeSpecialAction,
    NativeHeadless,
    ExternalPe,
    ExternalElf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IsolationDomain {
    KernelTask,
    UserProcess,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilityToken {
    pub id: CapabilityTokenId,
    pub app_id: AppId,
    pub package_id: &'static str,
    pub capabilities: CapabilityProfile,
    pub native_capability_bits: u64,
    pub source: LaunchSource,
    pub shell_owned: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistryEntrySource {
    BuiltIn,
    InstalledPackage,
    ExternalCandidate,
    ExternalImage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageRegistryEntry {
    pub manifest: AppManifest,
    pub source: RegistryEntrySource,
    pub aliases: Vec<String>,
    pub entry_path: Option<String>,
    pub external_candidates: Vec<String>,
}

impl PackageRegistryEntry {
    pub fn identity(&self) -> AppIdentity {
        self.manifest.identity
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrokeredLaunch {
    pub ticket: ProcessBrokerTicket,
    pub parent_ticket: Option<ProcessBrokerTicket>,
    pub child_tickets: Vec<ProcessBrokerTicket>,
    pub process_class: ProcessClass,
    pub isolation_domain: IsolationDomain,
    pub identity: AppIdentity,
    pub token: CapabilityToken,
    pub address_space_handle: u64,
    pub session_contract: u64,
    pub external_display: ExternalDisplayContract,
    pub resume_token: Option<u64>,
    pub image_path: Option<String>,
    pub external_runtime_graph: Option<ExternalRuntimeGraph>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowSessionSpec {
    pub app_id: AppId,
    pub workspace_id: WorkspaceId,
    pub shell_owned: bool,
    pub window_id: WindowId,
    pub surface_id: SurfaceId,
    pub external_display: ExternalDisplayContract,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowSessionHandle {
    pub runtime_id: Option<RuntimeHandleId>,
    pub spec: WindowSessionSpec,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeHandle {
    pub id: RuntimeHandleId,
    pub session: LaunchSession,
    pub identity: AppIdentity,
    pub broker_ticket: ProcessBrokerTicket,
    pub capability_token: CapabilityToken,
    pub isolation_domain: IsolationDomain,
    pub service_id: Option<ServiceId>,
    pub task_id: Option<u64>,
    pub image_path: Option<String>,
    pub window: Option<WindowSessionHandle>,
}
