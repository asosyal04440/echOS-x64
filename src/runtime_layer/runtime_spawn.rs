use super::super::gui::launch_pipeline::{
    AbiPersonality, AppDescriptor, AppInstallRoot, AppPresentation, AppTrust, CapabilityProfile,
    ExecutionContext, LaunchIntent, LaunchSession, LaunchSource, LoaderDispatch, StateContract,
};
use super::super::gui::protocol::AppId;
use super::super::ipc::ServiceId;
use super::super::kernel::memory as kernel_memory;
use super::super::kernel::tasking;
use super::super::kernel::tasking::task::Priority;
use alloc::string::{String, ToString};
use echos_manifest::{AppRuntime, NativeCapability};

use super::runtime_model::{IsolationDomain, RuntimeHandle};
use super::runtime_registry::{capability_profile_for_packaged, runtime_contract_for};
use super::runtime_state::{
    annotate_runtime_handle, brokered_launch, register_launch_session, runtime_handle_for_task,
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
    let diagnostics = super::super::pe_loader::preflight_launch_diagnostics(image)
        .map_err(|err| alloc::format!("PE preflight failed: {:?}", err))?;
    if !diagnostics.can_launch() {
        return Err(format_pe_launch_diagnostics(&diagnostics, image_path));
    }
    let (_, task_id) =
        super::super::pe_loader::spawn_process_task_from_payload(image, priority, task_name)
            .map_err(|err| {
                if err == super::super::pe_loader::PeError::ImportNotFound {
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
