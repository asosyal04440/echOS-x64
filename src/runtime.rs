//! Compatibility surface for the canonical runtime spine.
pub const RUNTIME_COMPATIBILITY_SURFACES: &[&str] = &[
    "runtime_layer::capability_contract",
    "runtime_layer::launch_contract",
    "runtime_layer::native_scene_contract",
    "runtime_layer::package_registry_contract",
    "runtime_layer::process_broker_contract",
    "runtime_layer::window_session_contract",
    "runtime_layer::runtime_model",
    "runtime_layer::runtime_state",
];

pub use super::runtime_layer::capability_contract::{
    runtime_address_space_for_pid, task_allows_native_capability,
};
pub use super::runtime_layer::launch_contract::{
    register_launch_session, register_launch_session_with_parent, service_process_available,
    spawn_elf_runtime, spawn_native_runtime, spawn_pe_runtime, spawn_service_process_runtime,
    spawn_service_runtime, BrokeredLaunch, CapabilityToken, CapabilityTokenId, IsolationDomain,
    ProcessBrokerTicket, RuntimeHandle, RuntimeHandleId,
};
pub use super::runtime_layer::native_scene_contract::runtime_handle_for_task;
pub use super::runtime_layer::package_registry_contract::{
    PackageRegistryEntry, RuntimePackageRegistry,
};
pub use super::runtime_layer::process_broker_contract::{
    brokered_launch, brokered_launch_children, runtime_handle_for_service, ProcessBroker,
};
pub use super::runtime_layer::runtime_model::{
    ProcessClass, RegistryEntrySource, WindowSessionSpec,
};
pub(crate) use super::runtime_layer::runtime_spawn::format_pe_launch_diagnostics;
pub use super::runtime_layer::runtime_state::{
    annotate_runtime_handle, runtime_handle, window_session, RuntimeCoordinator,
};
pub use super::runtime_layer::window_session_contract::{
    attach_window_session, forget_window_session, WindowSessionHandle,
};

#[cfg(test)]
mod tests {
    use super::super::gui::launch_pipeline::{
        AbiPersonality, AppDescriptor, AppInstallRoot, AppPresentation, AppTrust,
        CapabilityProfile, ExecutionContext, LaunchIntent, LaunchSource, LoaderDispatch,
        PackageRecord, RuntimeBootstrap, StateContract, UnifiedEventLoopContract,
    };
    use super::super::pe_loader::{PeImportFailure, PeImportResolutionReport, PeLaunchDiagnostics};
    use super::super::security::package::{self, InstalledPackagedApp, PackageTrustLevel};
    use super::{
        attach_window_session, brokered_launch, forget_window_session,
        format_pe_launch_diagnostics, register_launch_session, runtime_handle, window_session,
        ProcessBroker, RegistryEntrySource, RuntimePackageRegistry,
    };
    use alloc::string::String;
    use alloc::vec;
    use echos_manifest::{
        AppPresentation as ManifestPresentation, AppRuntime, AppStateContract, CompiledAppManifest,
        DefaultWindow, NativeCapability, RestartPolicy, SourceAppManifest, TrustDomain,
    };

    fn register_demo_installed_app() {
        package::clear_test_installed_apps();
        let source = SourceAppManifest {
            app_id: alloc::format!("org.echos.browser"),
            name: String::from("Browser"),
            version: String::from("1.0.0"),
            entry: String::from("browser.exe"),
            sdk_version: 1,
            runtime: AppRuntime::Pe,
            presentation: ManifestPresentation::ShellOwned,
            capabilities: vec![NativeCapability::NotificationsPost],
            default_window: DefaultWindow {
                title: String::from("Browser"),
                width: 1280,
                height: 720,
            },
            state_contract: AppStateContract::WarmSuspend,
            restart_policy: RestartPolicy::bounded_retry(1),
        };
        let compiled = CompiledAppManifest::from_source(&source, [0x33; 32]).expect("compiled");
        package::register_test_installed_app(InstalledPackagedApp {
            runtime_app_id: 0x5000_0042,
            manifest_app_id: "org.echos.browser",
            package_id: "org.echos.browser",
            title: "Browser",
            bundle_root: "/apps/org.echos.browser",
            bundle_path: "/apps/org.echos.browser/browser.app",
            entry_path: "/apps/org.echos.browser/browser.exe",
            compiled_manifest_path: "/apps/org.echos.browser/app.manifest.bin",
            compiled_manifest: compiled,
            capability_set: vec![NativeCapability::NotificationsPost],
            package_digest: [0x11; 32],
            manifest_digest: [0x22; 32],
            entry_digest: [0x33; 32],
            trust_level: PackageTrustLevel::Developer,
            trust_domain: TrustDomain::Developer,
            signer_key_id: "dev-root-v1",
            revocation_epoch: 1,
        });
    }

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
        assert_eq!(
            resolved.manifest().state_contract,
            StateContract::ColdResume
        );
    }

    #[test]
    fn package_registry_entries_unify_built_in_and_installed_truth_surface() {
        register_demo_installed_app();
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
        .with_file_associations(&[".txt", ".md"]);
        let records = [PackageRecord {
            aliases: &["editor"],
            descriptor,
            external_candidates: &[],
        }];
        let registry = RuntimePackageRegistry::new(&records);
        let entries = registry.entries();
        assert!(entries.iter().any(|entry| {
            entry.identity().package_id == "echos.editor"
                && entry.source == RegistryEntrySource::BuiltIn
        }));
        assert!(entries.iter().any(|entry| {
            entry.identity().package_id == "org.echos.browser"
                && entry.source == RegistryEntrySource::InstalledPackage
                && entry.entry_path.as_deref() == Some("/apps/org.echos.browser/browser.exe")
        }));
        package::clear_test_installed_apps();
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
        assert_eq!(
            grant.image_path.as_deref(),
            Some("/programs/firefox/firefox.exe")
        );
    }

    #[test]
    fn process_broker_records_child_tree_under_parent_ticket() {
        let descriptor = AppDescriptor::new(
            201,
            "terminal",
            "Terminal",
            LoaderDispatch::Native,
            AbiPersonality::Native,
            AppPresentation::Windowed,
            CapabilityProfile::shell_defaults(),
        );
        let child_descriptor = AppDescriptor::new(
            202,
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
        let mut broker = ProcessBroker::new();
        let parent =
            broker.authorize_launch(parent_session, None, super::IsolationDomain::KernelTask);
        let child = broker.authorize_child_launch(
            parent.ticket,
            child_session,
            Some("/downloads/firefox/firefox.exe"),
            super::IsolationDomain::UserProcess,
        );
        let described_parent = broker.launch(parent.ticket).expect("parent launch");
        assert_eq!(child.parent_ticket, Some(parent.ticket));
        assert_eq!(described_parent.child_tickets, vec![child.ticket]);
        assert_eq!(broker.children(parent.ticket), vec![child.ticket]);
    }

    #[test]
    fn pe_launch_diagnostic_names_primary_missing_import() {
        let message = format_pe_launch_diagnostics(
            &PeLaunchDiagnostics {
                imported_modules: vec![String::from("browserhelper.dll")],
                import_report: PeImportResolutionReport {
                    total: 2,
                    resolved: 0,
                    unresolved: 2,
                },
                unresolved_imports: vec![
                    PeImportFailure {
                        dll_name: String::from("browserhelper.dll"),
                        symbol_name: String::from("CreateSandboxBroker"),
                    },
                    PeImportFailure {
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
