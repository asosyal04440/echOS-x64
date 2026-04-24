use super::super::gui::launch_pipeline::{
    AbiPersonality, AppDescriptor, AppInstallRoot, AppPresentation, AppTrust, CapabilityProfile,
    ExecutionContext, LaunchIntent, LaunchSession, LaunchSource, LoaderDispatch, StateContract,
};
use super::super::gui::protocol::AppId;
use super::super::ipc::ServiceId;
use super::super::kernel::memory as kernel_memory;
use super::super::kernel::tasking;
use super::super::kernel::tasking::task::Priority;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use echos_manifest::{AppRuntime, NativeCapability};

use super::runtime_model::{
    ExternalImportBoundary, ExternalRuntimeGraph, ExternalRuntimeHelper, ExternalRuntimeHelperRole,
    ExternalRuntimeHelperState, ExternalRuntimeKind, ExternalRuntimeStage, ExternalRuntimeWorkflow,
    IsolationDomain, RuntimeGraphBoundaryState, RuntimeHandle,
};
use super::runtime_registry::{capability_profile_for_packaged, runtime_contract_for};
use super::runtime_state::{
    annotate_brokered_launch_runtime_graph, annotate_runtime_handle, brokered_launch,
    register_launch_session, register_launch_session_from_grant, reserve_child_launch_grant,
    reserve_launch_grant, runtime_handle_for_broker_ticket, runtime_handle_for_task,
};

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
    let address_space = kernel_memory::create_address_space(&[]);
    let task_id = tasking::scheduler::spawn_with_priority_in_address_space(
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
    super::super::security::package::resolve_installed_app(service_name)
        .map(|installed| validate_service_packaged_contract(&installed, service_name).is_ok())
        .unwrap_or(false)
}

pub fn spawn_service_process_runtime(
    service_id: ServiceId,
    service_name: &'static str,
    title: &'static str,
    priority: Priority,
) -> Result<RuntimeHandle, String> {
    let installed = super::super::security::package::resolve_installed_app(service_name)
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
            let handle =
                spawn_native_runtime(session, priority, service_name, installed.entry_path)?;
            annotate_runtime_handle(handle.id, IsolationDomain::UserProcess, Some(service_id))
                .ok_or_else(|| String::from("service runtime annotation failed"))
        }
        AppRuntime::Elf => {
            let handle = spawn_elf_runtime(
                session,
                &[],
                priority,
                service_name,
                Some(installed.entry_path),
            )?;
            annotate_runtime_handle(handle.id, IsolationDomain::UserProcess, Some(service_id))
                .ok_or_else(|| String::from("service runtime annotation failed"))
        }
        AppRuntime::Pe => {
            let handle = spawn_pe_runtime(
                session,
                &[],
                priority,
                service_name,
                Some(installed.entry_path),
            )?;
            annotate_runtime_handle(handle.id, IsolationDomain::UserProcess, Some(service_id))
                .ok_or_else(|| String::from("service runtime annotation failed"))
        }
        AppRuntime::Special => Err(String::from(
            "special runtime cannot host a service process",
        )),
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
        .and_then(|path| super::super::security::package::verify_packaged_launch(path).ok());
    if let Some(ref verified) = verified {
        validate_packaged_runtime_identity(&session, verified, AppRuntime::Elf)?;
    }
    let image = verified
        .as_ref()
        .map(|verified| verified.entry_image.as_slice())
        .unwrap_or(image);
    let (task_id, address_space) =
        tasking::scheduler::spawn_user_image_task_with_address_space(image, priority, task_name)
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
    let verified = super::super::security::package::verify_native_launch(image_path)
        .map_err(|err| alloc::format!("native package verify failed: {}", err))?;
    if verified.installed.runtime_app_id != session.intent.descriptor.app_id
        || verified.installed.package_id != session.intent.descriptor.package_id
    {
        return Err(String::from("native launch identity mismatch"));
    }
    let (task_id, address_space) = tasking::scheduler::spawn_user_image_task_with_address_space(
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

pub(crate) fn format_pe_launch_diagnostics(
    diagnostics: &super::super::pe_loader::PeLaunchDiagnostics,
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

fn browser_shell_runtime_kind(
    session: LaunchSession,
    image_path: Option<&str>,
) -> ExternalRuntimeKind {
    let package_id = session.intent.descriptor.package_id;
    let title = session.intent.descriptor.title;
    let route_label = session.intent.context.route_label;
    let image_path = image_path.unwrap_or("");
    if session.process.shell_owned
        && [
            package_id,
            title,
            route_label,
            image_path,
            session.intent.descriptor.slug,
        ]
        .iter()
        .any(|value| {
            let value = value.to_ascii_lowercase();
            value.contains("browser")
                || value.contains("firefox")
                || value.contains("chrome")
                || value.contains("chromium")
                || value.contains("edge")
                || value.contains("webview")
        })
    {
        ExternalRuntimeKind::BrowserShell
    } else {
        ExternalRuntimeKind::GenericPe
    }
}

fn build_external_runtime_graph(
    session: LaunchSession,
    image_path: Option<&str>,
    diagnostics: &super::super::pe_loader::PeLaunchDiagnostics,
    stage: ExternalRuntimeStage,
) -> ExternalRuntimeGraph {
    let kind = browser_shell_runtime_kind(session, image_path);
    let helpers = infer_external_runtime_helpers(kind, diagnostics);
    let workflow = infer_external_runtime_workflow(kind, image_path);
    let boundary_reason = browser_boundary_reason(kind, &helpers, &workflow);
    ExternalRuntimeGraph {
        kind,
        stage,
        import_graph_closed: diagnostics.can_launch(),
        helper_graph_state: browser_helper_graph_state(kind, &helpers),
        imported_modules: diagnostics.imported_modules.clone(),
        unresolved_imports: diagnostics
            .unresolved_imports
            .iter()
            .map(|failure| ExternalImportBoundary {
                dll_name: failure.dll_name.clone(),
                symbol_name: failure.symbol_name.clone(),
            })
            .collect::<Vec<_>>(),
        primary_blocker: diagnostics
            .primary_failure()
            .map(|failure| alloc::format!("{}!{}", failure.dll_name, failure.symbol_name)),
        boundary_reason,
        helpers,
        workflow,
    }
}

fn infer_external_runtime_helpers(
    kind: ExternalRuntimeKind,
    diagnostics: &super::super::pe_loader::PeLaunchDiagnostics,
) -> Vec<ExternalRuntimeHelper> {
    if kind != ExternalRuntimeKind::BrowserShell {
        return Vec::new();
    }

    let mut helpers = Vec::new();
    collect_browser_helper(
        &mut helpers,
        diagnostics,
        ExternalRuntimeHelperRole::SandboxBroker,
        &["CreateSandboxBroker", "CreateBroker", "SandboxBrokerMain"],
    );
    collect_browser_helper(
        &mut helpers,
        diagnostics,
        ExternalRuntimeHelperRole::GpuHelper,
        &["LaunchGpuHelper", "CreateGpuHelper", "GpuProcessMain"],
    );
    collect_browser_helper(
        &mut helpers,
        diagnostics,
        ExternalRuntimeHelperRole::RendererHelper,
        &[
            "LaunchRendererHelper",
            "CreateRendererHelper",
            "RendererMain",
        ],
    );
    collect_browser_helper(
        &mut helpers,
        diagnostics,
        ExternalRuntimeHelperRole::NetworkHelper,
        &[
            "LaunchNetworkHelper",
            "CreateNetworkHelper",
            "NetworkServiceMain",
        ],
    );
    collect_browser_helper(
        &mut helpers,
        diagnostics,
        ExternalRuntimeHelperRole::CrashReporter,
        &["LaunchCrashReporter", "CreateCrashReporter", "CrashpadMain"],
    );

    helpers
}

fn browser_helper_graph_state(
    kind: ExternalRuntimeKind,
    helpers: &[ExternalRuntimeHelper],
) -> RuntimeGraphBoundaryState {
    match kind {
        ExternalRuntimeKind::GenericPe | ExternalRuntimeKind::BrowserHelper => {
            RuntimeGraphBoundaryState::Closed
        }
        ExternalRuntimeKind::BrowserShell => {
            if !helpers.is_empty()
                && helpers.iter().all(|helper| {
                    helper.state == ExternalRuntimeHelperState::BrokerReserved
                        || helper.state == ExternalRuntimeHelperState::BridgeAttached
                })
            {
                RuntimeGraphBoundaryState::Closed
            } else {
                RuntimeGraphBoundaryState::Open
            }
        }
    }
}

fn collect_browser_helper(
    helpers: &mut Vec<ExternalRuntimeHelper>,
    diagnostics: &super::super::pe_loader::PeLaunchDiagnostics,
    role: ExternalRuntimeHelperRole,
    symbols: &[&str],
) {
    let unresolved = diagnostics
        .unresolved_imports
        .iter()
        .find(|failure| {
            symbols
                .iter()
                .any(|symbol| failure.symbol_name.eq_ignore_ascii_case(symbol))
        })
        .map(|failure| alloc::format!("{}!{}", failure.dll_name, failure.symbol_name));
    let imported_helper_module = diagnostics
        .imported_modules
        .iter()
        .any(|module| module.eq_ignore_ascii_case("browserhelper.dll"));
    if unresolved.is_none() && !imported_helper_module {
        return;
    }
    helpers.push(ExternalRuntimeHelper {
        role,
        state: if unresolved.is_some() {
            ExternalRuntimeHelperState::BlockedByImportGraph
        } else {
            ExternalRuntimeHelperState::ReadyToSpawn
        },
        blocker_import: unresolved,
        broker_ticket: None,
        runtime_id: None,
    });
}

fn infer_external_runtime_workflow(
    kind: ExternalRuntimeKind,
    image_path: Option<&str>,
) -> ExternalRuntimeWorkflow {
    let working_directory = image_path.and_then(parent_directory);
    match kind {
        ExternalRuntimeKind::BrowserShell => ExternalRuntimeWorkflow {
            image_path: image_path.map(String::from),
            working_directory,
            download_root: Some(String::from("/downloads")),
            open_folder_root: Some(String::from("/downloads")),
            command_line_contract_state: RuntimeGraphBoundaryState::Open,
            environment_contract_state: RuntimeGraphBoundaryState::Open,
        },
        ExternalRuntimeKind::GenericPe => ExternalRuntimeWorkflow {
            image_path: image_path.map(String::from),
            working_directory,
            download_root: None,
            open_folder_root: None,
            command_line_contract_state: RuntimeGraphBoundaryState::Closed,
            environment_contract_state: RuntimeGraphBoundaryState::Closed,
        },
        ExternalRuntimeKind::BrowserHelper => ExternalRuntimeWorkflow {
            image_path: image_path.map(String::from),
            working_directory,
            download_root: Some(String::from("/downloads")),
            open_folder_root: Some(String::from("/downloads")),
            command_line_contract_state: RuntimeGraphBoundaryState::Closed,
            environment_contract_state: RuntimeGraphBoundaryState::Closed,
        },
    }
}

fn parent_directory(path: &str) -> Option<String> {
    let trimmed = path.trim_end_matches('/');
    let split = trimmed.rfind('/')?;
    if split == 0 {
        Some(String::from("/"))
    } else {
        Some(String::from(&trimmed[..split]))
    }
}

fn browser_boundary_reason(
    kind: ExternalRuntimeKind,
    helpers: &[ExternalRuntimeHelper],
    workflow: &ExternalRuntimeWorkflow,
) -> Option<String> {
    if kind != ExternalRuntimeKind::BrowserShell {
        return None;
    }
    let mut reasons = Vec::new();
    if helpers.iter().any(|helper| {
        helper.state == ExternalRuntimeHelperState::BlockedByImportGraph
            || helper.state == ExternalRuntimeHelperState::Expected
    }) {
        reasons.push("brokered helper-process graph is still incomplete".to_string());
    } else if helpers
        .iter()
        .any(|helper| helper.state == ExternalRuntimeHelperState::BrokerReserved)
    {
        reasons.push(
            "helper-process graph is reserved in the broker tree, but live bridge attachment or executable spawn parity is still open"
                .to_string(),
        );
    } else if helpers
        .iter()
        .any(|helper| helper.state == ExternalRuntimeHelperState::BridgeAttached)
    {
        reasons.push(
            "helper bridge sessions are attached under the browser parent, but executable child-task spawn/lifecycle parity is still open"
                .to_string(),
        );
    } else if !helpers.is_empty() {
        reasons.push("helper-process graph is projected and ready, but browser runtime must still claim the helpers at execution time".to_string());
    } else {
        reasons.push(
            "browser helper-process topology is not yet projected from the launch corpus"
                .to_string(),
        );
    }
    if workflow.command_line_contract_state == RuntimeGraphBoundaryState::Open
        || workflow.environment_contract_state == RuntimeGraphBoundaryState::Open
    {
        reasons.push("argv/env/cwd propagation contract is not yet closed for shell-owned browser PE launches".to_string());
    }
    reasons.push("sandbox closure remains open".to_string());
    Some(reasons.join("; "))
}

fn browser_helper_descriptor(
    parent: LaunchSession,
    role: ExternalRuntimeHelperRole,
) -> AppDescriptor {
    let (app_id_xor, slug, title, package_id) = match role {
        ExternalRuntimeHelperRole::SandboxBroker => (
            0x1010,
            "browser-sandbox-helper",
            "Browser Sandbox Helper",
            "org.echos.browser.helper.sandbox",
        ),
        ExternalRuntimeHelperRole::GpuHelper => (
            0x1020,
            "browser-gpu-helper",
            "Browser GPU Helper",
            "org.echos.browser.helper.gpu",
        ),
        ExternalRuntimeHelperRole::RendererHelper => (
            0x1030,
            "browser-renderer-helper",
            "Browser Renderer Helper",
            "org.echos.browser.helper.renderer",
        ),
        ExternalRuntimeHelperRole::NetworkHelper => (
            0x1040,
            "browser-network-helper",
            "Browser Network Helper",
            "org.echos.browser.helper.network",
        ),
        ExternalRuntimeHelperRole::CrashReporter => (
            0x1050,
            "browser-crash-helper",
            "Browser Crash Helper",
            "org.echos.browser.helper.crash",
        ),
    };
    AppDescriptor::new(
        parent.intent.descriptor.app_id ^ app_id_xor,
        slug,
        title,
        LoaderDispatch::Pe,
        AbiPersonality::Win32,
        AppPresentation::ShellOwned,
        CapabilityProfile::shell_defaults(),
    )
    .with_package_id(package_id)
    .with_install_root(AppInstallRoot::ExternalImage)
    .with_trust(AppTrust::External)
}

fn reserve_browser_helper_children(
    parent_session: LaunchSession,
    parent_ticket: u64,
    image_path: Option<&str>,
    graph: &mut ExternalRuntimeGraph,
) {
    if graph.kind != ExternalRuntimeKind::BrowserShell {
        return;
    }
    for helper in graph.helpers.iter_mut() {
        if helper.state != ExternalRuntimeHelperState::ReadyToSpawn {
            continue;
        }
        let child_session = LaunchIntent::new(
            browser_helper_descriptor(parent_session, helper.role),
            ExecutionContext::new(
                parent_session.intent.context.source,
                parent_session.intent.context.workspace_id,
                browser_helper_route_label(helper.role),
            ),
        )
        .canonical_session()
        .with_external_display_contract(parent_session.process.external_display);
        let child = reserve_child_launch_grant(
            parent_ticket,
            child_session,
            image_path,
            IsolationDomain::UserProcess,
        );
        let _ = annotate_brokered_launch_runtime_graph(
            child.ticket,
            browser_helper_runtime_graph(helper.role, image_path),
        );
        helper.state = ExternalRuntimeHelperState::BrokerReserved;
        helper.broker_ticket = Some(child.ticket);
    }
    graph.helper_graph_state = browser_helper_graph_state(graph.kind, &graph.helpers);
    graph.boundary_reason = browser_boundary_reason(graph.kind, &graph.helpers, &graph.workflow);
}

fn attach_browser_helper_runtimes(
    parent_session: LaunchSession,
    image_path: Option<&str>,
    graph: &mut ExternalRuntimeGraph,
) {
    if graph.kind != ExternalRuntimeKind::BrowserShell {
        return;
    }
    for helper in graph.helpers.iter_mut() {
        if helper.state != ExternalRuntimeHelperState::BrokerReserved {
            continue;
        }
        let Some(broker_ticket) = helper.broker_ticket else {
            continue;
        };
        let child_session = LaunchIntent::new(
            browser_helper_descriptor(parent_session, helper.role),
            ExecutionContext::new(
                parent_session.intent.context.source,
                parent_session.intent.context.workspace_id,
                browser_helper_route_label(helper.role),
            ),
        )
        .canonical_session()
        .with_external_display_contract(parent_session.process.external_display);
        let Some(handle) = register_launch_session_from_grant(
            child_session,
            None,
            image_path.map(|value| value.to_string()),
            None,
            IsolationDomain::UserProcess,
            None,
            broker_ticket,
        ) else {
            continue;
        };
        prepare_bridge_for_runtime(&handle);
        let _ = annotate_brokered_launch_runtime_graph(
            broker_ticket,
            browser_helper_attached_runtime_graph(helper.role, image_path),
        );
        helper.state = ExternalRuntimeHelperState::BridgeAttached;
        helper.runtime_id = Some(handle.id);
    }
    graph.helper_graph_state = browser_helper_graph_state(graph.kind, &graph.helpers);
    graph.boundary_reason = browser_boundary_reason(graph.kind, &graph.helpers, &graph.workflow);
}

fn browser_helper_route_label(role: ExternalRuntimeHelperRole) -> &'static str {
    match role {
        ExternalRuntimeHelperRole::SandboxBroker => "browser-sandbox-helper",
        ExternalRuntimeHelperRole::GpuHelper => "browser-gpu-helper",
        ExternalRuntimeHelperRole::RendererHelper => "browser-renderer-helper",
        ExternalRuntimeHelperRole::NetworkHelper => "browser-network-helper",
        ExternalRuntimeHelperRole::CrashReporter => "browser-crash-helper",
    }
}

fn browser_helper_runtime_graph(
    role: ExternalRuntimeHelperRole,
    image_path: Option<&str>,
) -> ExternalRuntimeGraph {
    ExternalRuntimeGraph {
        kind: ExternalRuntimeKind::BrowserHelper,
        stage: ExternalRuntimeStage::Reserved,
        import_graph_closed: true,
        helper_graph_state: RuntimeGraphBoundaryState::Closed,
        imported_modules: Vec::new(),
        unresolved_imports: Vec::new(),
        primary_blocker: None,
        boundary_reason: Some(format!(
            "{:?} helper ticket reserved under browser parent; live helper task spawn still owned by the parent browser runtime",
            role
        )),
        helpers: Vec::new(),
        workflow: infer_external_runtime_workflow(ExternalRuntimeKind::BrowserHelper, image_path),
    }
}

fn browser_helper_attached_runtime_graph(
    role: ExternalRuntimeHelperRole,
    image_path: Option<&str>,
) -> ExternalRuntimeGraph {
    ExternalRuntimeGraph {
        kind: ExternalRuntimeKind::BrowserHelper,
        stage: ExternalRuntimeStage::Spawned,
        import_graph_closed: true,
        helper_graph_state: RuntimeGraphBoundaryState::Closed,
        imported_modules: Vec::new(),
        unresolved_imports: Vec::new(),
        primary_blocker: None,
        boundary_reason: Some(format!(
            "{:?} helper bridge session attached under browser parent; executable helper task spawn still remains an open parity boundary",
            role
        )),
        helpers: Vec::new(),
        workflow: infer_external_runtime_workflow(ExternalRuntimeKind::BrowserHelper, image_path),
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
        .and_then(|path| super::super::security::package::verify_packaged_launch(path).ok());
    if let Some(ref verified) = verified {
        validate_packaged_runtime_identity(&session, verified, AppRuntime::Pe)?;
    }
    let image = verified
        .as_ref()
        .map(|verified| verified.entry_image.as_slice())
        .unwrap_or(image);
    let grant = reserve_launch_grant(session, image_path, IsolationDomain::UserProcess);
    let diagnostics = super::super::pe_loader::preflight_launch_diagnostics(image)
        .map_err(|err| alloc::format!("PE preflight failed: {:?}", err))?;
    let reserved_graph = build_external_runtime_graph(
        session,
        image_path,
        &diagnostics,
        ExternalRuntimeStage::Reserved,
    );
    let blocked_graph = build_external_runtime_graph(
        session,
        image_path,
        &diagnostics,
        ExternalRuntimeStage::ImportPreflightBlocked,
    );
    let mut spawned_graph = build_external_runtime_graph(
        session,
        image_path,
        &diagnostics,
        ExternalRuntimeStage::Spawned,
    );
    let blocked_graph_for_spawn = blocked_graph.clone();
    let _ = annotate_brokered_launch_runtime_graph(grant.ticket, reserved_graph);
    if !diagnostics.can_launch() {
        let _ = annotate_brokered_launch_runtime_graph(grant.ticket, blocked_graph);
        return Err(format_pe_launch_diagnostics(&diagnostics, image_path));
    }
    let (_, task_id) =
        super::super::pe_loader::spawn_process_task_from_payload(image, priority, task_name)
            .map_err(|err| {
                if err == super::super::pe_loader::PeError::ImportNotFound {
                    let _ = annotate_brokered_launch_runtime_graph(
                        grant.ticket,
                        blocked_graph_for_spawn.clone(),
                    );
                    format_pe_launch_diagnostics(&diagnostics, image_path)
                } else {
                    alloc::format!("PE runtime spawn failed: {:?}", err)
                }
            })?;
    let handle = register_launch_session_from_grant(
        session,
        Some(task_id as u64),
        image_path.map(|value| value.to_string()),
        None,
        IsolationDomain::UserProcess,
        None,
        grant.ticket,
    )
    .ok_or_else(|| String::from("PE runtime broker grant lost before registration"))?;
    reserve_browser_helper_children(session, grant.ticket, image_path, &mut spawned_graph);
    attach_browser_helper_runtimes(session, image_path, &mut spawned_graph);
    let _ = annotate_brokered_launch_runtime_graph(grant.ticket, spawned_graph);
    prepare_bridge_for_runtime(&handle);
    Ok(handle)
}

pub fn task_allows_native_capability(task_id: u64, capability: NativeCapability) -> bool {
    runtime_handle_for_task(task_id)
        .map(|runtime| runtime.capability_token.native_capability_bits & capability.bit() != 0)
        .unwrap_or(false)
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

fn validate_packaged_runtime_identity(
    session: &LaunchSession,
    verified: &super::super::security::package::VerifiedPackagedLaunch,
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
    installed: &super::super::security::package::InstalledPackagedApp,
    service_name: &str,
) -> Result<(), String> {
    if installed.compiled_manifest.presentation != echos_manifest::AppPresentation::Headless {
        return Err(alloc::format!(
            "service package '{}' is not headless",
            service_name
        ));
    }
    if installed.trust_level != super::super::security::package::PackageTrustLevel::Platform {
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
            let _ = super::super::ironshim_app::prepare_packaged_bridge(
                &grant,
                handle.session.intent.descriptor.abi,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::gui::launch_pipeline::{
        AbiPersonality, AppDescriptor, AppPresentation, CapabilityProfile, ExecutionContext,
        LaunchIntent, LaunchSource, LoaderDispatch,
    };
    use super::super::super::pe_loader::{
        PeImportFailure, PeImportResolutionReport, PeLaunchDiagnostics,
    };
    use super::super::runtime_state::{
        brokered_launch, brokered_launch_children, reserve_launch_grant,
        runtime_handle_for_broker_ticket,
    };
    use super::{
        attach_browser_helper_runtimes, browser_shell_runtime_kind, build_external_runtime_graph,
        reserve_browser_helper_children, ExternalRuntimeHelperRole, ExternalRuntimeHelperState,
        ExternalRuntimeKind, ExternalRuntimeStage, IsolationDomain, RuntimeGraphBoundaryState,
    };
    use alloc::string::String;
    use alloc::vec;
    use alloc::vec::Vec;

    fn shell_owned_session(
        slug: &'static str,
        title: &'static str,
        package_id: &'static str,
    ) -> super::LaunchSession {
        let descriptor = AppDescriptor::new(
            77,
            slug,
            title,
            LoaderDispatch::Pe,
            AbiPersonality::Win32,
            AppPresentation::ShellOwned,
            CapabilityProfile::shell_defaults(),
        )
        .with_package_id(package_id);
        LaunchIntent::new(
            descriptor,
            ExecutionContext::new(LaunchSource::CommandPalette, 2, slug),
        )
        .canonical_session()
    }

    #[test]
    fn browser_shell_launches_publish_open_helper_graph_boundary() {
        let session = shell_owned_session("firefox", "Firefox", "org.mozilla.firefox");
        let diagnostics = PeLaunchDiagnostics {
            imported_modules: vec![
                String::from("browserhelper.dll"),
                String::from("user32.dll"),
            ],
            import_report: PeImportResolutionReport {
                total: 2,
                resolved: 0,
                unresolved: 2,
            },
            unresolved_imports: vec![PeImportFailure {
                dll_name: String::from("browserhelper.dll"),
                symbol_name: String::from("CreateSandboxBroker"),
            }],
        };
        assert_eq!(
            browser_shell_runtime_kind(session, Some("/downloads/firefox/firefox.exe")),
            ExternalRuntimeKind::BrowserShell
        );
        let graph = build_external_runtime_graph(
            session,
            Some("/downloads/firefox/firefox.exe"),
            &diagnostics,
            ExternalRuntimeStage::ImportPreflightBlocked,
        );
        assert_eq!(graph.kind, ExternalRuntimeKind::BrowserShell);
        assert_eq!(graph.stage, ExternalRuntimeStage::ImportPreflightBlocked);
        assert!(!graph.import_graph_closed);
        assert_eq!(graph.helper_graph_state, RuntimeGraphBoundaryState::Open);
        assert_eq!(
            graph.primary_blocker.as_deref(),
            Some("browserhelper.dll!CreateSandboxBroker")
        );
        assert_eq!(
            graph.workflow.working_directory.as_deref(),
            Some("/downloads/firefox")
        );
        assert_eq!(graph.workflow.download_root.as_deref(), Some("/downloads"));
        assert_eq!(
            graph.workflow.command_line_contract_state,
            RuntimeGraphBoundaryState::Open
        );
        assert!(graph.helpers.iter().any(|helper| {
            helper.role == ExternalRuntimeHelperRole::SandboxBroker
                && helper.state == ExternalRuntimeHelperState::BlockedByImportGraph
                && helper.blocker_import.as_deref() == Some("browserhelper.dll!CreateSandboxBroker")
        }));
        assert!(graph
            .boundary_reason
            .as_deref()
            .unwrap_or("")
            .contains("argv/env/cwd"));
    }

    #[test]
    fn generic_pe_launches_stay_closed_when_import_graph_is_resolved() {
        let descriptor = AppDescriptor::new(
            78,
            "demo-pe",
            "Demo PE",
            LoaderDispatch::Pe,
            AbiPersonality::Win32,
            AppPresentation::Windowed,
            CapabilityProfile::shell_defaults(),
        )
        .with_package_id("org.echos.demo");
        let session = LaunchIntent::new(
            descriptor,
            ExecutionContext::new(LaunchSource::Launcher, 1, "demo-pe"),
        )
        .canonical_session();
        let diagnostics = PeLaunchDiagnostics {
            imported_modules: vec![String::from("kernel32.dll")],
            import_report: PeImportResolutionReport {
                total: 1,
                resolved: 1,
                unresolved: 0,
            },
            unresolved_imports: Vec::new(),
        };
        let graph = build_external_runtime_graph(
            session,
            Some("/apps/demo/demo.exe"),
            &diagnostics,
            ExternalRuntimeStage::Spawned,
        );
        assert_eq!(graph.kind, ExternalRuntimeKind::GenericPe);
        assert!(graph.import_graph_closed);
        assert_eq!(graph.helper_graph_state, RuntimeGraphBoundaryState::Closed);
        assert!(graph.primary_blocker.is_none());
        assert!(graph.boundary_reason.is_none());
        assert!(graph.helpers.is_empty());
        assert_eq!(
            graph.workflow.image_path.as_deref(),
            Some("/apps/demo/demo.exe")
        );
        assert_eq!(
            graph.workflow.working_directory.as_deref(),
            Some("/apps/demo")
        );
        assert_eq!(
            graph.workflow.command_line_contract_state,
            RuntimeGraphBoundaryState::Closed
        );
    }

    #[test]
    fn browser_shell_ready_helpers_reserve_child_broker_launches() {
        let session = shell_owned_session("firefox", "Firefox", "org.mozilla.firefox");
        let diagnostics = PeLaunchDiagnostics {
            imported_modules: vec![
                String::from("browserhelper.dll"),
                String::from("user32.dll"),
            ],
            import_report: PeImportResolutionReport {
                total: 2,
                resolved: 2,
                unresolved: 0,
            },
            unresolved_imports: Vec::new(),
        };
        let parent = reserve_launch_grant(
            session,
            Some("/downloads/firefox/firefox.exe"),
            IsolationDomain::UserProcess,
        );
        let mut graph = build_external_runtime_graph(
            session,
            Some("/downloads/firefox/firefox.exe"),
            &diagnostics,
            ExternalRuntimeStage::Spawned,
        );
        reserve_browser_helper_children(
            session,
            parent.ticket,
            Some("/downloads/firefox/firefox.exe"),
            &mut graph,
        );
        attach_browser_helper_runtimes(session, Some("/downloads/firefox/firefox.exe"), &mut graph);
        let children = brokered_launch_children(parent.ticket);
        assert!(!children.is_empty());
        assert_eq!(graph.helper_graph_state, RuntimeGraphBoundaryState::Closed);
        assert!(graph.helpers.iter().all(|helper| {
            helper.state == ExternalRuntimeHelperState::BridgeAttached
                && helper.broker_ticket.is_some()
                && helper.runtime_id.is_some()
        }));
        let sandbox_ticket = graph
            .helpers
            .iter()
            .find(|helper| helper.role == ExternalRuntimeHelperRole::SandboxBroker)
            .and_then(|helper| helper.broker_ticket)
            .expect("sandbox helper ticket");
        assert!(children.contains(&sandbox_ticket));
        let child = brokered_launch(sandbox_ticket).expect("child launch");
        assert_eq!(child.parent_ticket, Some(parent.ticket));
        assert!(child.external_runtime_graph.is_some());
        let attached = runtime_handle_for_broker_ticket(sandbox_ticket).expect("child runtime");
        assert_eq!(attached.broker_ticket, sandbox_ticket);
    }
}
