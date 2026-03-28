//! Canonical runtime spine for whole-system launch/bootstrap/window coordination.

use crate::gui::launch_pipeline::{
    resolve_file_association, resolve_launch_query, AbiPersonality, AppDescriptor, AppIdentity,
    AppInstallRoot, AppPresentation, AppResolution, AppTrust, CapabilityProfile,
    ExecutionContext, LaunchIntent, LaunchSession, LaunchSource, LoaderDispatch, PackageRecord,
    RuntimeBootstrap, StateContract, UnifiedEventLoopContract, WindowEndpointContract,
};
use crate::memory::AddressSpace;
use crate::ipc::ServiceId;
use crate::gui::protocol::{AppId, SurfaceId, WindowId, WorkspaceId};
use crate::task::task::Priority;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use core::sync::atomic::{AtomicU64, Ordering};
use echos_manifest::{AppRuntime, NativeCapability};
use lazy_static::lazy_static;
use spin::Mutex;

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrokeredLaunch {
    pub ticket: ProcessBrokerTicket,
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

pub struct RuntimePackageRegistry<'a> {
    records: &'a [PackageRecord],
}

impl<'a> RuntimePackageRegistry<'a> {
    pub fn new(records: &'a [PackageRecord]) -> Self {
        Self { records }
    }

    pub fn resolve(&self, query: &str) -> Option<AppResolution> {
        resolve_installed_packaged(query).or_else(|| resolve_launch_query(query, self.records))
    }

    pub fn resolve_with_probe<F>(&self, query: &str, path_exists: F) -> Option<AppResolution>
    where
        F: FnMut(&str) -> bool,
    {
        resolve_installed_packaged(query).or_else(|| {
            crate::gui::launch_pipeline::resolve_launch_query_with_probe(
                query,
                self.records,
                path_exists,
            )
        })
    }

    pub fn records(&self) -> &'a [PackageRecord] {
        self.records
    }

    pub fn resolve_file_association(&self, path: &str) -> Option<AppResolution> {
        resolve_file_association(path, self.records)
    }
}

#[derive(Default)]
pub struct RuntimeCoordinator {
    handles: BTreeMap<RuntimeHandleId, RuntimeHandle>,
    windows: BTreeMap<WindowId, WindowSessionHandle>,
    tasks: BTreeMap<u64, RuntimeHandleId>,
    services: BTreeMap<ServiceId, RuntimeHandleId>,
    address_spaces: BTreeMap<u64, Arc<Mutex<AddressSpace>>>,
}

#[derive(Default)]
pub struct ProcessBroker {
    launches: BTreeMap<ProcessBrokerTicket, BrokeredLaunch>,
}

impl RuntimeCoordinator {
    pub fn new() -> Self {
        Self {
            handles: BTreeMap::new(),
            windows: BTreeMap::new(),
            tasks: BTreeMap::new(),
            services: BTreeMap::new(),
            address_spaces: BTreeMap::new(),
        }
    }

    fn allocate_handle_id() -> RuntimeHandleId {
        NEXT_RUNTIME_HANDLE_ID.fetch_add(1, Ordering::Relaxed)
    }

    fn runtime_for_window_spec(&self, spec: &WindowSessionSpec) -> Option<RuntimeHandleId> {
        self.handles
            .values()
            .rev()
            .find(|handle| {
                let window = handle.session.window;
                window.app_id == spec.app_id
                    && window.workspace_id == spec.workspace_id
                    && window.shell_owned == spec.shell_owned
            })
            .map(|handle| handle.id)
    }

    fn register_runtime(
        &mut self,
        session: LaunchSession,
        task_id: Option<u64>,
        image_path: Option<String>,
        address_space: Option<Arc<Mutex<AddressSpace>>>,
        isolation_domain: IsolationDomain,
        service_id: Option<ServiceId>,
    ) -> RuntimeHandle {
        let grant = PROCESS_BROKER
            .lock()
            .authorize_launch(session, image_path.as_deref(), isolation_domain);
        let id = Self::allocate_handle_id();
        let handle = RuntimeHandle {
            id,
            session,
            identity: grant.identity,
            broker_ticket: grant.ticket,
            capability_token: grant.token.clone(),
            isolation_domain,
            service_id,
            task_id,
            image_path,
            window: None,
        };
        self.handles.insert(id, handle.clone());
        if let Some(service_id) = service_id {
            self.services.insert(service_id, id);
        }
        if let Some(space) = address_space {
            self.address_spaces
                .insert(handle.identity.app_id as u64, Arc::clone(&space));
            if let Some(task_id) = task_id {
                self.address_spaces.insert(task_id, space);
            }
        }
        if let Some(task_id) = task_id {
            self.tasks.insert(task_id, id);
        }
        handle
    }

    fn attach_window(
        &mut self,
        spec: WindowSessionSpec,
        preferred_runtime: Option<RuntimeHandleId>,
    ) -> WindowSessionHandle {
        let runtime_id = preferred_runtime.or_else(|| self.runtime_for_window_spec(&spec));
        let handle = WindowSessionHandle { runtime_id, spec };
        self.windows.insert(handle.spec.window_id, handle.clone());
        if let Some(runtime_id) = handle.runtime_id {
            if let Some(runtime) = self.handles.get_mut(&runtime_id) {
                runtime.window = Some(handle.clone());
            }
        }
        handle
    }

    fn forget_window(&mut self, window_id: WindowId) {
        let Some(window) = self.windows.remove(&window_id) else {
            return;
        };
        if let Some(runtime_id) = window.runtime_id {
            if let Some(runtime) = self.handles.get_mut(&runtime_id) {
                if runtime
                    .window
                    .as_ref()
                    .map(|entry| entry.spec.window_id == window_id)
                    .unwrap_or(false)
                {
                    runtime.window = None;
                }
            }
        }
    }

    pub fn runtime(&self, id: RuntimeHandleId) -> Option<RuntimeHandle> {
        self.handles.get(&id).cloned()
    }

    pub fn window(&self, window_id: WindowId) -> Option<WindowSessionHandle> {
        self.windows.get(&window_id).cloned()
    }

    pub fn runtime_for_task(&self, task_id: u64) -> Option<RuntimeHandle> {
        let handle_id = *self.tasks.get(&task_id)?;
        self.handles.get(&handle_id).cloned()
    }

    pub fn runtime_for_service(&self, service_id: ServiceId) -> Option<RuntimeHandle> {
        let handle_id = *self.services.get(&service_id)?;
        self.handles.get(&handle_id).cloned()
    }

    pub fn annotate_runtime(
        &mut self,
        id: RuntimeHandleId,
        isolation_domain: IsolationDomain,
        service_id: Option<ServiceId>,
    ) -> Option<RuntimeHandle> {
        let runtime = self.handles.get_mut(&id)?;
        runtime.isolation_domain = isolation_domain;
        runtime.service_id = service_id;
        Some(runtime.clone())
    }

    pub fn address_space_for_pid(&self, pid: u64) -> Option<Arc<Mutex<AddressSpace>>> {
        self.address_spaces.get(&pid).cloned()
    }
}

impl ProcessBroker {
    pub fn new() -> Self {
        Self {
            launches: BTreeMap::new(),
        }
    }

    fn classify(session: LaunchSession) -> ProcessClass {
        match session.process.bootstrap {
            RuntimeBootstrap::NativeWindowed => ProcessClass::NativeWindowed,
            RuntimeBootstrap::NativeSpecialAction => ProcessClass::NativeSpecialAction,
            RuntimeBootstrap::NativeHeadless => ProcessClass::NativeHeadless,
            RuntimeBootstrap::Win32Bridge => ProcessClass::ExternalPe,
            RuntimeBootstrap::PosixBridge => ProcessClass::ExternalElf,
        }
    }

    fn allocate_ticket() -> ProcessBrokerTicket {
        NEXT_PROCESS_BROKER_TICKET.fetch_add(1, Ordering::Relaxed)
    }

    fn allocate_token() -> CapabilityTokenId {
        NEXT_CAPABILITY_TOKEN_ID.fetch_add(1, Ordering::Relaxed)
    }

    fn allocate_address_space_handle() -> u64 {
        NEXT_ADDRESS_SPACE_HANDLE.fetch_add(1, Ordering::Relaxed)
    }

    pub fn authorize_launch(
        &mut self,
        session: LaunchSession,
        image_path: Option<&str>,
        isolation_domain: IsolationDomain,
    ) -> BrokeredLaunch {
        let ticket = Self::allocate_ticket();
        let identity = session.intent.descriptor.identity();
        let installed = crate::security::package::resolve_installed_app(identity.package_id)
            .or_else(|| image_path.and_then(crate::security::package::resolve_installed_app));
        let native_capability_bits = installed
            .as_ref()
            .map(|installed| installed.compiled_manifest.capability_bits)
            .unwrap_or(0);
        let token = CapabilityToken {
            id: Self::allocate_token(),
            app_id: identity.app_id,
            package_id: identity.package_id,
            capabilities: session.intent.descriptor.capabilities,
            native_capability_bits,
            source: session.intent.context.source,
            shell_owned: session.process.shell_owned,
        };
        let grant = BrokeredLaunch {
            ticket,
            process_class: Self::classify(session),
            isolation_domain,
            identity,
            token,
            address_space_handle: Self::allocate_address_space_handle(),
            session_contract: encode_session_contract(session),
            resume_token: installed
                .as_ref()
                .and_then(crate::runtime_supervisor::resume_token_for_app),
            image_path: image_path.map(|value| value.to_string()),
        };
        self.launches.insert(ticket, grant.clone());
        grant
    }

    pub fn launch(&self, ticket: ProcessBrokerTicket) -> Option<BrokeredLaunch> {
        self.launches.get(&ticket).cloned()
    }
}

static NEXT_RUNTIME_HANDLE_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_PROCESS_BROKER_TICKET: AtomicU64 = AtomicU64::new(1);
static NEXT_CAPABILITY_TOKEN_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_ADDRESS_SPACE_HANDLE: AtomicU64 = AtomicU64::new(1);

lazy_static! {
    static ref RUNTIME_COORDINATOR: Mutex<RuntimeCoordinator> =
        Mutex::new(RuntimeCoordinator::new());
    static ref PROCESS_BROKER: Mutex<ProcessBroker> = Mutex::new(ProcessBroker::new());
}

pub fn register_launch_session(
    session: LaunchSession,
    task_id: Option<u64>,
    image_path: Option<String>,
    address_space: Option<Arc<Mutex<AddressSpace>>>,
    isolation_domain: IsolationDomain,
    service_id: Option<ServiceId>,
) -> RuntimeHandle {
    let app_id = session.intent.descriptor.app_id as u64;
    crate::security::capability::init_process(app_id);
    if let Some(task_id) = task_id {
        crate::security::capability::init_process(task_id);
    }
    RUNTIME_COORDINATOR
        .lock()
        .register_runtime(
            session,
            task_id,
            image_path,
            address_space,
            isolation_domain,
            service_id,
        )
}

pub fn attach_window_session(
    app_id: AppId,
    workspace_id: WorkspaceId,
    shell_owned: bool,
    window_id: WindowId,
    surface_id: SurfaceId,
) -> WindowSessionHandle {
    let spec = WindowSessionSpec {
        app_id,
        workspace_id,
        shell_owned,
        window_id,
        surface_id,
    };
    RUNTIME_COORDINATOR.lock().attach_window(spec, None)
}

pub fn forget_window_session(window_id: WindowId) {
    RUNTIME_COORDINATOR.lock().forget_window(window_id);
}

pub fn runtime_handle(id: RuntimeHandleId) -> Option<RuntimeHandle> {
    RUNTIME_COORDINATOR.lock().runtime(id)
}

pub fn annotate_runtime_handle(
    id: RuntimeHandleId,
    isolation_domain: IsolationDomain,
    service_id: Option<ServiceId>,
) -> Option<RuntimeHandle> {
    RUNTIME_COORDINATOR
        .lock()
        .annotate_runtime(id, isolation_domain, service_id)
}

pub fn runtime_handle_for_task(task_id: u64) -> Option<RuntimeHandle> {
    RUNTIME_COORDINATOR.lock().runtime_for_task(task_id)
}

pub fn runtime_handle_for_service(service_id: ServiceId) -> Option<RuntimeHandle> {
    RUNTIME_COORDINATOR.lock().runtime_for_service(service_id)
}

pub fn window_session(window_id: WindowId) -> Option<WindowSessionHandle> {
    RUNTIME_COORDINATOR.lock().window(window_id)
}

pub fn brokered_launch(ticket: ProcessBrokerTicket) -> Option<BrokeredLaunch> {
    PROCESS_BROKER.lock().launch(ticket)
}

pub fn runtime_address_space_for_pid(pid: u64) -> Option<Arc<Mutex<AddressSpace>>> {
    RUNTIME_COORDINATOR.lock().address_space_for_pid(pid)
}

pub fn spawn_service_runtime(
    service_id: ServiceId,
    service_name: &'static str,
    title: &'static str,
    entry: fn() -> !,
    priority: Priority,
) -> RuntimeHandle {
    let descriptor = AppDescriptor::new(
        hash_runtime_app_id(service_name, LoaderDispatch::Native),
        service_name,
        title,
        LoaderDispatch::Native,
        AbiPersonality::Native,
        AppPresentation::Headless,
        CapabilityProfile::service_defaults(),
    );
    let session = LaunchIntent::new(
        descriptor,
        ExecutionContext::new(LaunchSource::ServiceInit, 0, service_name),
    )
    .canonical_session();
    let address_space = crate::memory::create_address_space(&[]);
    let task_id = crate::task::scheduler::spawn_with_priority_in_address_space(
        entry,
        priority,
        service_name,
        Some(address_space.clone()),
    ) as u64;
    register_launch_session(
        session,
        Some(task_id),
        None,
        Some(address_space),
        IsolationDomain::KernelTask,
        Some(service_id),
    )
}

pub fn service_process_available(service_name: &str) -> bool {
    crate::security::package::resolve_installed_app(service_name)
        .map(|installed| {
            validate_service_packaged_contract(&installed, service_name).is_ok()
        })
        .unwrap_or(false)
}

pub fn spawn_service_process_runtime(
    service_id: ServiceId,
    service_name: &'static str,
    title: &'static str,
    priority: Priority,
) -> Result<RuntimeHandle, String> {
    let installed = crate::security::package::resolve_installed_app(service_name)
        .ok_or_else(|| alloc::format!("service package '{}' not installed", service_name))?;
    validate_service_packaged_contract(&installed, service_name)?;
    let (loader, abi, presentation) = runtime_contract_for(
        installed.compiled_manifest.runtime,
        installed.compiled_manifest.presentation,
    )
    .ok_or_else(|| String::from("service package runtime unsupported"))?;
    if presentation != AppPresentation::Headless {
        return Err(String::from("service package must be headless"));
    }
    let descriptor = AppDescriptor::new(
        installed.runtime_app_id,
        installed.package_id,
        title,
        loader,
        abi,
        AppPresentation::Headless,
        capability_profile_for_packaged(&installed),
    )
    .with_package_id(installed.package_id)
    .with_install_root(AppInstallRoot::Service)
    .with_trust(AppTrust::Platform)
    .with_state_contract(match installed.compiled_manifest.state_contract {
        echos_manifest::AppStateContract::Stateless => StateContract::Stateless,
        echos_manifest::AppStateContract::WarmSuspend => StateContract::WarmSuspend,
        echos_manifest::AppStateContract::ColdResume => StateContract::ColdResume,
    });
    let session = LaunchIntent::new(
        descriptor,
        ExecutionContext::new(LaunchSource::ServiceInit, 0, service_name),
    )
    .canonical_session();
    match installed.compiled_manifest.runtime {
        AppRuntime::Native => {
            let handle = spawn_native_runtime(session, priority, service_name, installed.entry_path)?;
            annotate_runtime_handle(
                handle.id,
                IsolationDomain::UserProcess,
                Some(service_id),
            )
            .ok_or_else(|| String::from("service runtime annotation failed"))
        }
        AppRuntime::Elf => {
            let handle =
                spawn_elf_runtime(session, &[], priority, service_name, Some(installed.entry_path))?;
            annotate_runtime_handle(
                handle.id,
                IsolationDomain::UserProcess,
                Some(service_id),
            )
            .ok_or_else(|| String::from("service runtime annotation failed"))
        }
        AppRuntime::Pe => {
            let handle =
                spawn_pe_runtime(session, &[], priority, service_name, Some(installed.entry_path))?;
            annotate_runtime_handle(
                handle.id,
                IsolationDomain::UserProcess,
                Some(service_id),
            )
            .ok_or_else(|| String::from("service runtime annotation failed"))
        }
        AppRuntime::Special => Err(String::from("special runtime cannot host a service process")),
    }
}

pub fn spawn_elf_runtime(
    session: LaunchSession,
    image: &[u8],
    priority: Priority,
    task_name: &'static str,
    image_path: Option<&str>,
) -> Result<RuntimeHandle, String> {
    let verified = image_path
        .and_then(|path| crate::security::package::verify_packaged_launch(path).ok());
    if let Some(ref verified) = verified {
        validate_packaged_runtime_identity(&session, verified, AppRuntime::Elf)?;
    }
    let image = verified
        .as_ref()
        .map(|verified| verified.entry_image.as_slice())
        .unwrap_or(image);
    let (task_id, address_space) =
        crate::task::scheduler::spawn_user_image_task_with_address_space(
            image,
            priority,
            task_name,
        )
        .map_err(|_| String::from("ELF runtime spawn failed"))?;
    let handle = register_launch_session(
        session,
        Some(task_id as u64),
        image_path.map(|value| value.to_string()),
        Some(address_space),
        IsolationDomain::UserProcess,
        None,
    );
    prepare_bridge_for_runtime(&handle);
    Ok(handle)
}

pub fn spawn_native_runtime(
    session: LaunchSession,
    priority: Priority,
    task_name: &'static str,
    image_path: &str,
) -> Result<RuntimeHandle, String> {
    let verified = crate::security::package::verify_native_launch(image_path)
        .map_err(|err| alloc::format!("native package verify failed: {}", err))?;
    if verified.installed.runtime_app_id != session.intent.descriptor.app_id
        || verified.installed.package_id != session.intent.descriptor.package_id
    {
        return Err(String::from("native launch identity mismatch"));
    }
    let (task_id, address_space) =
        crate::task::scheduler::spawn_user_image_task_with_address_space(
        &verified.entry_image,
        priority,
        task_name,
    )
    .map_err(|_| String::from("native runtime spawn failed"))?;
    let handle = register_launch_session(
        session,
        Some(task_id as u64),
        Some(image_path.to_string()),
        Some(address_space),
        IsolationDomain::UserProcess,
        None,
    );
    prepare_bridge_for_runtime(&handle);
    Ok(handle)
}

fn format_pe_launch_diagnostics(
    diagnostics: &crate::pe_loader::PeLaunchDiagnostics,
    image_path: Option<&str>,
) -> String {
    let target = image_path.unwrap_or("<memory>");
    let missing_count = diagnostics.unresolved_imports.len();
    let Some(primary) = diagnostics.primary_failure() else {
        return alloc::format!(
            "{} preflight blocked: unresolved import graph without named failure",
            target
        );
    };
    let extra = missing_count.saturating_sub(1);
    if extra == 0 {
        alloc::format!(
            "{} missing import {}!{}",
            target,
            primary.dll_name,
            primary.symbol_name
        )
    } else {
        alloc::format!(
            "{} missing import {}!{} (+{} more)",
            target,
            primary.dll_name,
            primary.symbol_name,
            extra
        )
    }
}

pub fn spawn_pe_runtime(
    session: LaunchSession,
    image: &[u8],
    priority: Priority,
    task_name: &'static str,
    image_path: Option<&str>,
) -> Result<RuntimeHandle, String> {
    let verified = image_path
        .and_then(|path| crate::security::package::verify_packaged_launch(path).ok());
    if let Some(ref verified) = verified {
        validate_packaged_runtime_identity(&session, verified, AppRuntime::Pe)?;
    }
    let image = verified
        .as_ref()
        .map(|verified| verified.entry_image.as_slice())
        .unwrap_or(image);
    let diagnostics = crate::pe_loader::preflight_launch_diagnostics(image)
        .map_err(|err| alloc::format!("PE preflight failed: {:?}", err))?;
    if !diagnostics.can_launch() {
        return Err(format_pe_launch_diagnostics(&diagnostics, image_path));
    }
    let (_, task_id) = crate::pe_loader::spawn_process_task_from_payload(
        image, priority, task_name,
    )
    .map_err(|err| {
        if err == crate::pe_loader::PeError::ImportNotFound {
            format_pe_launch_diagnostics(&diagnostics, image_path)
        } else {
            alloc::format!("PE runtime spawn failed: {:?}", err)
        }
    })?;
    let handle = register_launch_session(
        session,
        Some(task_id as u64),
        image_path.map(|value| value.to_string()),
        None,
        IsolationDomain::UserProcess,
        None,
    );
    prepare_bridge_for_runtime(&handle);
    Ok(handle)
}

fn hash_runtime_app_id(name: &str, loader: LoaderDispatch) -> AppId {
    let mut hash = 0x811C_9DC5u32;
    for byte in name.as_bytes() {
        hash ^= *byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    let tag = match loader {
        LoaderDispatch::Native => 0x1000_0000,
        LoaderDispatch::Pe => 0x5000_0000,
        LoaderDispatch::Elf => 0x6000_0000,
    };
    tag | (hash & 0x0FFF_FFFF)
}

pub fn task_allows_native_capability(task_id: u64, capability: NativeCapability) -> bool {
    runtime_handle_for_task(task_id)
        .map(|runtime| runtime.capability_token.native_capability_bits & capability.bit() != 0)
        .unwrap_or(false)
}

fn resolve_installed_native(query: &str) -> Option<AppResolution> {
    let installed = crate::security::package::resolve_installed_native_app(query)?;
    packaged_resolution_for(&installed)
}

fn resolve_installed_packaged(query: &str) -> Option<AppResolution> {
    let installed = crate::security::package::resolve_installed_app(query)?;
    packaged_resolution_for(&installed)
}

fn packaged_resolution_for(
    installed: &crate::security::package::InstalledPackagedApp,
) -> Option<AppResolution> {
    let (loader, abi, presentation) = runtime_contract_for(
        installed.compiled_manifest.runtime,
        installed.compiled_manifest.presentation,
    )?;
    let descriptor = AppDescriptor::new(
        installed.runtime_app_id,
        installed.package_id,
        installed.title,
        loader,
        abi,
        presentation,
        capability_profile_for_packaged(installed),
    )
    .with_package_id(installed.package_id)
    .with_install_root(AppInstallRoot::UserApps)
    .with_trust(match installed.trust_level {
        crate::security::package::PackageTrustLevel::Platform => AppTrust::Platform,
        crate::security::package::PackageTrustLevel::Store
        | crate::security::package::PackageTrustLevel::Developer => AppTrust::Installed,
    })
    .with_state_contract(match installed.compiled_manifest.state_contract {
        echos_manifest::AppStateContract::Stateless => StateContract::Stateless,
        echos_manifest::AppStateContract::WarmSuspend => StateContract::WarmSuspend,
        echos_manifest::AppStateContract::ColdResume => StateContract::ColdResume,
    });
    Some(AppResolution::ExternalPath {
        descriptor,
        path: installed.entry_path.to_string(),
    })
}

fn capability_profile_for_packaged(
    installed: &crate::security::package::InstalledPackagedApp,
) -> CapabilityProfile {
    let mut profile = CapabilityProfile::service_defaults();
    for capability in &installed.capability_set {
        match capability {
            NativeCapability::FsRead | NativeCapability::FsWrite => profile.file_system = true,
            NativeCapability::DialogsOpen | NativeCapability::DialogsSave => {
                profile.file_dialogs = true
            }
            NativeCapability::NotificationsPost => profile.notifications = true,
            NativeCapability::ClipboardRead
            | NativeCapability::ClipboardWrite
            | NativeCapability::CaptureFrame => {}
        }
    }
    profile
}

fn runtime_contract_for(
    runtime: AppRuntime,
    presentation: echos_manifest::AppPresentation,
) -> Option<(LoaderDispatch, AbiPersonality, AppPresentation)> {
    match runtime {
        AppRuntime::Native => Some((
            LoaderDispatch::Native,
            AbiPersonality::Native,
            match presentation {
                echos_manifest::AppPresentation::Windowed => AppPresentation::Windowed,
                echos_manifest::AppPresentation::ShellOwned => AppPresentation::ShellOwned,
                echos_manifest::AppPresentation::SpecialAction => AppPresentation::SpecialAction,
                echos_manifest::AppPresentation::Headless => AppPresentation::Headless,
            },
        )),
        AppRuntime::Pe => Some((
            LoaderDispatch::Pe,
            AbiPersonality::Win32,
            AppPresentation::ShellOwned,
        )),
        AppRuntime::Elf => Some((
            LoaderDispatch::Elf,
            AbiPersonality::Posix,
            AppPresentation::ShellOwned,
        )),
        AppRuntime::Special => None,
    }
}

fn encode_session_contract(session: LaunchSession) -> u64 {
    ((session.window.app_id as u64) << 32) | (session.window.workspace_id as u64)
}

fn validate_packaged_runtime_identity(
    session: &LaunchSession,
    verified: &crate::security::package::VerifiedPackagedLaunch,
    expected_runtime: AppRuntime,
) -> Result<(), String> {
    if verified.installed.compiled_manifest.runtime != expected_runtime {
        return Err(String::from("packaged runtime personality mismatch"));
    }
    if verified.installed.runtime_app_id != session.intent.descriptor.app_id
        || verified.installed.package_id != session.intent.descriptor.package_id
    {
        return Err(String::from("packaged launch identity mismatch"));
    }
    Ok(())
}

fn validate_service_packaged_contract(
    installed: &crate::security::package::InstalledPackagedApp,
    service_name: &str,
) -> Result<(), String> {
    if installed.compiled_manifest.presentation != echos_manifest::AppPresentation::Headless {
        return Err(alloc::format!(
            "service package '{}' is not headless",
            service_name
        ));
    }
    if installed.trust_level != crate::security::package::PackageTrustLevel::Platform {
        return Err(alloc::format!(
            "service package '{}' is not platform-trusted",
            service_name
        ));
    }
    Ok(())
}

fn prepare_bridge_for_runtime(handle: &RuntimeHandle) {
    let Some(grant) = brokered_launch(handle.broker_ticket) else {
        return;
    };
    match handle.session.intent.descriptor.abi {
        AbiPersonality::Native => {}
        AbiPersonality::Win32 | AbiPersonality::Posix => {
            let _ = crate::ironshim_app::prepare_packaged_bridge(
                &grant,
                handle.session.intent.descriptor.abi,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        attach_window_session, brokered_launch, forget_window_session,
        format_pe_launch_diagnostics, register_launch_session, runtime_handle, window_session,
        RuntimePackageRegistry,
    };
    use crate::gui::launch_pipeline::{
        AbiPersonality, AppDescriptor, AppInstallRoot, AppPresentation, AppTrust,
        CapabilityProfile, ExecutionContext, LaunchIntent, LaunchSource, LoaderDispatch,
        PackageRecord, RuntimeBootstrap, StateContract, UnifiedEventLoopContract,
    };
    use alloc::string::String;
    use alloc::vec;

    #[test]
    fn headless_service_session_uses_native_headless_runtime() {
        let descriptor = AppDescriptor::new(
            77,
            "ech_display",
            "EchDisplay",
            LoaderDispatch::Native,
            AbiPersonality::Native,
            AppPresentation::Headless,
            CapabilityProfile::service_defaults(),
        );
        let session = LaunchIntent::new(
            descriptor,
            ExecutionContext::new(LaunchSource::ServiceInit, 0, "ech_display"),
        )
        .canonical_session();
        assert_eq!(session.process.bootstrap, RuntimeBootstrap::NativeHeadless);
        assert_eq!(
            session.event_loop,
            UnifiedEventLoopContract::HeadlessService
        );
    }

    #[test]
    fn runtime_registry_tracks_window_attachment_by_app_identity() {
        let descriptor = AppDescriptor::new(
            91,
            "terminal",
            "Terminal",
            LoaderDispatch::Native,
            AbiPersonality::Native,
            AppPresentation::Windowed,
            CapabilityProfile::shell_defaults(),
        );
        let session = LaunchIntent::new(
            descriptor,
            ExecutionContext::new(LaunchSource::DesktopShortcut, 2, "desktop"),
        )
        .canonical_session();
        let runtime = register_launch_session(
            session,
            Some(1001),
            None,
            None,
            super::IsolationDomain::KernelTask,
            None,
        );
        let window = attach_window_session(91, 2, false, 41, 9);
        let attached = runtime_handle(runtime.id).expect("runtime");
        assert_eq!(window.runtime_id, Some(runtime.id));
        assert_eq!(attached.window.expect("window").spec.window_id, 41);
        forget_window_session(41);
        assert!(window_session(41).is_none());
    }

    #[test]
    fn package_registry_prefers_alias_resolution() {
        let descriptor = AppDescriptor::new(
            9,
            "browser",
            "Web",
            LoaderDispatch::Native,
            AbiPersonality::Native,
            AppPresentation::Windowed,
            CapabilityProfile::file_worker(),
        )
        .with_package_id("echos.web")
        .with_file_associations(&[".html", ".htm", ".url"]);
        let records = [PackageRecord {
            aliases: &["web", "browser"],
            descriptor,
            external_candidates: &[],
        }];
        let registry = RuntimePackageRegistry::new(&records);
        let resolved = registry.resolve("browser").expect("resolution");
        assert_eq!(resolved.descriptor().app_id, 9);
        assert_eq!(
            resolved.descriptor().presentation,
            AppPresentation::Windowed
        );
        assert_eq!(resolved.identity().package_id, "echos.web");
    }

    #[test]
    fn package_registry_resolves_file_association_into_app_identity() {
        let descriptor = AppDescriptor::new(
            14,
            "editor",
            "Editor",
            LoaderDispatch::Native,
            AbiPersonality::Native,
            AppPresentation::Windowed,
            CapabilityProfile::file_worker(),
        )
        .with_package_id("echos.editor")
        .with_file_associations(&[".txt", ".md"])
        .with_state_contract(StateContract::ColdResume);
        let records = [PackageRecord {
            aliases: &["editor"],
            descriptor,
            external_candidates: &[],
        }];
        let registry = RuntimePackageRegistry::new(&records);
        let resolved = registry
            .resolve_file_association("/workspace/notes.md")
            .expect("association resolution");
        assert_eq!(resolved.identity().package_id, "echos.editor");
        assert_eq!(resolved.manifest().state_contract, StateContract::ColdResume);
    }

    #[test]
    fn register_launch_session_issues_broker_ticket_and_capability_token() {
        let descriptor = AppDescriptor::new(
            110,
            "files",
            "Files",
            LoaderDispatch::Native,
            AbiPersonality::Native,
            AppPresentation::Windowed,
            CapabilityProfile::file_worker(),
        )
        .with_package_id("echos.files")
        .with_install_root(AppInstallRoot::SystemApps)
        .with_trust(AppTrust::Platform);
        let session = LaunchIntent::new(
            descriptor,
            ExecutionContext::new(LaunchSource::Launcher, 1, "launcher"),
        )
        .canonical_session();
        let runtime = register_launch_session(
            session,
            Some(2002),
            None,
            None,
            super::IsolationDomain::KernelTask,
            None,
        );
        assert_eq!(runtime.identity.package_id, "echos.files");
        assert!(runtime.capability_token.capabilities.file_system);
        let grant = brokered_launch(runtime.broker_ticket).expect("broker grant");
        assert_eq!(grant.identity.package_id, "echos.files");
        assert_eq!(grant.token.package_id, "echos.files");
        assert_eq!(grant.token.source, LaunchSource::Launcher);
    }

    #[test]
    fn process_broker_classifies_external_win32_launches() {
        let descriptor = AppDescriptor::new(
            111,
            "firefox",
            "Firefox",
            LoaderDispatch::Pe,
            AbiPersonality::Win32,
            AppPresentation::ShellOwned,
            CapabilityProfile::shell_defaults(),
        )
        .with_package_id("org.mozilla.firefox")
        .with_install_root(AppInstallRoot::UserApps)
        .with_trust(AppTrust::Installed);
        let session = LaunchIntent::new(
            descriptor,
            ExecutionContext::new(LaunchSource::CommandPalette, 4, "firefox"),
        )
        .canonical_session();
        let runtime = register_launch_session(
            session,
            Some(2003),
            Some(String::from("/programs/firefox/firefox.exe")),
            None,
            super::IsolationDomain::UserProcess,
            None,
        );
        let grant = brokered_launch(runtime.broker_ticket).expect("broker grant");
        assert_eq!(grant.process_class, super::ProcessClass::ExternalPe);
        assert_eq!(grant.image_path.as_deref(), Some("/programs/firefox/firefox.exe"));
    }

    #[test]
    fn pe_launch_diagnostic_names_primary_missing_import() {
        let message = format_pe_launch_diagnostics(
            &crate::pe_loader::PeLaunchDiagnostics {
                imported_modules: vec![String::from("browserhelper.dll")],
                import_report: crate::pe_loader::PeImportResolutionReport {
                    total: 2,
                    resolved: 0,
                    unresolved: 2,
                },
                unresolved_imports: vec![
                    crate::pe_loader::PeImportFailure {
                        dll_name: String::from("browserhelper.dll"),
                        symbol_name: String::from("CreateSandboxBroker"),
                    },
                    crate::pe_loader::PeImportFailure {
                        dll_name: String::from("browserhelper.dll"),
                        symbol_name: String::from("LaunchGpuHelper"),
                    },
                ],
            },
            Some("/downloads/firefox/firefox.exe"),
        );
        assert_eq!(
            message,
            "/downloads/firefox/firefox.exe missing import browserhelper.dll!CreateSandboxBroker (+1 more)"
        );
    }
}
