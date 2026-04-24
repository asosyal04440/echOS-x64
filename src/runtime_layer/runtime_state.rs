use super::super::gui::launch_pipeline::ExternalDisplayContract;
use super::super::gui::launch_pipeline::{LaunchSession, RuntimeBootstrap};
use super::super::gui::protocol::{AppId, SurfaceId, WindowId, WorkspaceId};
use super::super::ipc::ServiceId;
use super::super::kernel::memory::AddressSpace;
use super::super::kernel::tasking;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use lazy_static::lazy_static;
use spin::Mutex;

use super::runtime_model::{
    BrokeredLaunch, CapabilityToken, ExternalRuntimeGraph, IsolationDomain, ProcessBrokerTicket,
    ProcessClass, RuntimeHandle, RuntimeHandleId, WindowSessionHandle, WindowSessionSpec,
};

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
        self.register_runtime_with_parent(
            session,
            task_id,
            image_path,
            address_space,
            isolation_domain,
            service_id,
            None,
        )
    }

    fn register_runtime_with_parent(
        &mut self,
        session: LaunchSession,
        task_id: Option<u64>,
        image_path: Option<String>,
        address_space: Option<Arc<Mutex<AddressSpace>>>,
        isolation_domain: IsolationDomain,
        service_id: Option<ServiceId>,
        parent_ticket: Option<ProcessBrokerTicket>,
    ) -> RuntimeHandle {
        let grant = if let Some(parent_ticket) = parent_ticket {
            PROCESS_BROKER.lock().authorize_child_launch(
                parent_ticket,
                session,
                image_path.as_deref(),
                isolation_domain,
            )
        } else {
            PROCESS_BROKER
                .lock()
                .authorize_launch(session, image_path.as_deref(), isolation_domain)
        };
        self.register_runtime_from_grant(
            session,
            task_id,
            image_path,
            address_space,
            isolation_domain,
            service_id,
            grant,
        )
    }

    fn register_runtime_from_ticket(
        &mut self,
        session: LaunchSession,
        task_id: Option<u64>,
        image_path: Option<String>,
        address_space: Option<Arc<Mutex<AddressSpace>>>,
        isolation_domain: IsolationDomain,
        service_id: Option<ServiceId>,
        broker_ticket: ProcessBrokerTicket,
    ) -> Option<RuntimeHandle> {
        let grant = PROCESS_BROKER.lock().launch(broker_ticket)?;
        Some(self.register_runtime_from_grant(
            session,
            task_id,
            image_path,
            address_space,
            isolation_domain,
            service_id,
            grant,
        ))
    }

    fn register_runtime_from_grant(
        &mut self,
        session: LaunchSession,
        task_id: Option<u64>,
        image_path: Option<String>,
        address_space: Option<Arc<Mutex<AddressSpace>>>,
        isolation_domain: IsolationDomain,
        service_id: Option<ServiceId>,
        grant: BrokeredLaunch,
    ) -> RuntimeHandle {
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

    pub fn runtime_for_broker_ticket(
        &self,
        broker_ticket: ProcessBrokerTicket,
    ) -> Option<RuntimeHandle> {
        self.handles
            .values()
            .find(|handle| handle.broker_ticket == broker_ticket)
            .cloned()
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

    fn allocate_token() -> u64 {
        NEXT_CAPABILITY_TOKEN_ID.fetch_add(1, Ordering::Relaxed)
    }

    fn allocate_address_space_handle() -> u64 {
        NEXT_ADDRESS_SPACE_HANDLE.fetch_add(1, Ordering::Relaxed)
    }

    fn authorize_launch_internal(
        &mut self,
        session: LaunchSession,
        image_path: Option<&str>,
        isolation_domain: IsolationDomain,
        parent_ticket: Option<ProcessBrokerTicket>,
    ) -> BrokeredLaunch {
        let ticket = Self::allocate_ticket();
        let identity = session.intent.descriptor.identity();
        let installed = super::super::security::package::resolve_installed_app(identity.package_id)
            .or_else(|| {
                image_path.and_then(super::super::security::package::resolve_installed_app)
            });
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
            parent_ticket,
            child_tickets: Vec::new(),
            process_class: Self::classify(session),
            isolation_domain,
            identity,
            token,
            address_space_handle: Self::allocate_address_space_handle(),
            session_contract: encode_session_contract(session),
            external_display: session.process.external_display,
            resume_token: installed
                .as_ref()
                .and_then(super::super::runtime_supervisor::resume_token_for_app),
            image_path: image_path.map(|value| value.to_string()),
            external_runtime_graph: None,
        };
        self.launches.insert(ticket, grant.clone());
        if let Some(parent_ticket) = parent_ticket {
            if let Some(parent) = self.launches.get_mut(&parent_ticket) {
                if !parent.child_tickets.contains(&ticket) {
                    parent.child_tickets.push(ticket);
                }
            }
        }
        grant
    }

    pub fn authorize_launch(
        &mut self,
        session: LaunchSession,
        image_path: Option<&str>,
        isolation_domain: IsolationDomain,
    ) -> BrokeredLaunch {
        self.authorize_launch_internal(
            session,
            image_path,
            isolation_domain,
            current_parent_broker_ticket(),
        )
    }

    pub fn authorize_child_launch(
        &mut self,
        parent_ticket: ProcessBrokerTicket,
        session: LaunchSession,
        image_path: Option<&str>,
        isolation_domain: IsolationDomain,
    ) -> BrokeredLaunch {
        self.authorize_launch_internal(session, image_path, isolation_domain, Some(parent_ticket))
    }

    pub fn launch(&self, ticket: ProcessBrokerTicket) -> Option<BrokeredLaunch> {
        self.launches.get(&ticket).cloned()
    }

    pub fn children(&self, ticket: ProcessBrokerTicket) -> Vec<ProcessBrokerTicket> {
        self.launches
            .get(&ticket)
            .map(|launch| launch.child_tickets.clone())
            .unwrap_or_default()
    }

    pub fn annotate_external_runtime_graph(
        &mut self,
        ticket: ProcessBrokerTicket,
        graph: ExternalRuntimeGraph,
    ) -> Option<BrokeredLaunch> {
        let launch = self.launches.get_mut(&ticket)?;
        launch.external_runtime_graph = Some(graph);
        Some(launch.clone())
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
    super::super::security::capability::init_process(app_id);
    if let Some(task_id) = task_id {
        super::super::security::capability::init_process(task_id);
    }
    RUNTIME_COORDINATOR.lock().register_runtime(
        session,
        task_id,
        image_path,
        address_space,
        isolation_domain,
        service_id,
    )
}

pub fn register_launch_session_with_parent(
    session: LaunchSession,
    task_id: Option<u64>,
    image_path: Option<String>,
    address_space: Option<Arc<Mutex<AddressSpace>>>,
    isolation_domain: IsolationDomain,
    service_id: Option<ServiceId>,
    parent_ticket: ProcessBrokerTicket,
) -> RuntimeHandle {
    let app_id = session.intent.descriptor.app_id as u64;
    super::super::security::capability::init_process(app_id);
    if let Some(task_id) = task_id {
        super::super::security::capability::init_process(task_id);
    }
    RUNTIME_COORDINATOR.lock().register_runtime_with_parent(
        session,
        task_id,
        image_path,
        address_space,
        isolation_domain,
        service_id,
        Some(parent_ticket),
    )
}

pub fn reserve_launch_grant(
    session: LaunchSession,
    image_path: Option<&str>,
    isolation_domain: IsolationDomain,
) -> BrokeredLaunch {
    PROCESS_BROKER
        .lock()
        .authorize_launch(session, image_path, isolation_domain)
}

pub fn reserve_child_launch_grant(
    parent_ticket: ProcessBrokerTicket,
    session: LaunchSession,
    image_path: Option<&str>,
    isolation_domain: IsolationDomain,
) -> BrokeredLaunch {
    PROCESS_BROKER.lock().authorize_child_launch(
        parent_ticket,
        session,
        image_path,
        isolation_domain,
    )
}

pub fn register_launch_session_from_grant(
    session: LaunchSession,
    task_id: Option<u64>,
    image_path: Option<String>,
    address_space: Option<Arc<Mutex<AddressSpace>>>,
    isolation_domain: IsolationDomain,
    service_id: Option<ServiceId>,
    broker_ticket: ProcessBrokerTicket,
) -> Option<RuntimeHandle> {
    let app_id = session.intent.descriptor.app_id as u64;
    super::super::security::capability::init_process(app_id);
    if let Some(task_id) = task_id {
        super::super::security::capability::init_process(task_id);
    }
    RUNTIME_COORDINATOR.lock().register_runtime_from_ticket(
        session,
        task_id,
        image_path,
        address_space,
        isolation_domain,
        service_id,
        broker_ticket,
    )
}

pub fn attach_window_session(
    app_id: AppId,
    workspace_id: WorkspaceId,
    shell_owned: bool,
    window_id: WindowId,
    surface_id: SurfaceId,
) -> WindowSessionHandle {
    attach_window_session_with_display(
        app_id,
        workspace_id,
        shell_owned,
        window_id,
        surface_id,
        ExternalDisplayContract::default(),
    )
}

pub fn attach_window_session_with_display(
    app_id: AppId,
    workspace_id: WorkspaceId,
    shell_owned: bool,
    window_id: WindowId,
    surface_id: SurfaceId,
    external_display: ExternalDisplayContract,
) -> WindowSessionHandle {
    let spec = WindowSessionSpec {
        app_id,
        workspace_id,
        shell_owned,
        window_id,
        surface_id,
        external_display,
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

pub fn runtime_handle_for_broker_ticket(
    broker_ticket: ProcessBrokerTicket,
) -> Option<RuntimeHandle> {
    RUNTIME_COORDINATOR
        .lock()
        .runtime_for_broker_ticket(broker_ticket)
}

pub fn window_session(window_id: WindowId) -> Option<WindowSessionHandle> {
    RUNTIME_COORDINATOR.lock().window(window_id)
}

pub fn brokered_launch(ticket: ProcessBrokerTicket) -> Option<BrokeredLaunch> {
    PROCESS_BROKER.lock().launch(ticket)
}

pub fn brokered_launch_children(ticket: ProcessBrokerTicket) -> Vec<ProcessBrokerTicket> {
    PROCESS_BROKER.lock().children(ticket)
}

pub fn annotate_brokered_launch_runtime_graph(
    ticket: ProcessBrokerTicket,
    graph: ExternalRuntimeGraph,
) -> Option<BrokeredLaunch> {
    PROCESS_BROKER
        .lock()
        .annotate_external_runtime_graph(ticket, graph)
}

pub fn runtime_address_space_for_pid(pid: u64) -> Option<Arc<Mutex<AddressSpace>>> {
    RUNTIME_COORDINATOR.lock().address_space_for_pid(pid)
}

fn current_parent_broker_ticket() -> Option<ProcessBrokerTicket> {
    if cfg!(test) {
        return None;
    }
    let task_id = tasking::scheduler::current_task_id() as u64;
    if task_id == 0 {
        return None;
    }
    runtime_handle_for_task(task_id).map(|runtime| runtime.broker_ticket)
}

fn encode_session_contract(session: LaunchSession) -> u64 {
    ((session.window.app_id as u64) << 32) | (session.window.workspace_id as u64)
}
