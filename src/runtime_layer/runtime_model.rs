use super::super::gui::launch_pipeline::{
    AppIdentity, AppManifest, CapabilityProfile, LaunchSession, LaunchSource,
};
use super::super::gui::protocol::{AppId, SurfaceId, WindowId, WorkspaceId};
use super::super::ipc::ServiceId;
use alloc::string::String;
use alloc::vec::Vec;

pub type RuntimeHandleId = u64;
pub type CapabilityTokenId = u64;
pub type ProcessBrokerTicket = u64;

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
    pub resume_token: Option<u64>,
    pub image_path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowSessionSpec {
    pub app_id: AppId,
    pub workspace_id: WorkspaceId,
    pub shell_owned: bool,
    pub window_id: WindowId,
    pub surface_id: SurfaceId,
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
