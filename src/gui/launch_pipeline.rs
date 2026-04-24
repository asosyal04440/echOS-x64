//! Canonical desktop launch pipeline contract for echOS shell surfaces.

use crate::gui::protocol::{AppId, DesktopPermission, WorkspaceId};
use alloc::string::{String, ToString};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchSource {
    ServiceInit,
    DesktopShortcut,
    CommandPalette,
    Notification,
    ShellShortcut,
    TaskStrip,
    Launcher,
    FileAssociation,
    ContextMenu,
    DocumentOpen,
    FaultRecovery,
}

impl LaunchSource {
    pub const fn label(self) -> &'static str {
        match self {
            Self::ServiceInit => "service-init",
            Self::DesktopShortcut => "desktop-shortcut",
            Self::CommandPalette => "command-palette",
            Self::Notification => "notification",
            Self::ShellShortcut => "shell-shortcut",
            Self::TaskStrip => "task-strip",
            Self::Launcher => "launcher",
            Self::FileAssociation => "file-association",
            Self::ContextMenu => "context-menu",
            Self::DocumentOpen => "document-open",
            Self::FaultRecovery => "fault-recovery",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoaderDispatch {
    Native,
    Pe,
    Elf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbiPersonality {
    Native,
    Win32,
    Posix,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppPresentation {
    Windowed,
    ShellOwned,
    SpecialAction,
    Headless,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeBootstrap {
    NativeWindowed,
    NativeSpecialAction,
    NativeHeadless,
    Win32Bridge,
    PosixBridge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppInstallRoot {
    SystemApps,
    UserApps,
    UserData,
    ExternalImage,
    Service,
}

impl AppInstallRoot {
    pub const fn path_prefix(self) -> &'static str {
        match self {
            Self::SystemApps => "/system/apps",
            Self::UserApps => "/apps",
            Self::UserData => "/data/appdata",
            Self::ExternalImage => "/downloads",
            Self::Service => "/system/services",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppTrust {
    Platform,
    Installed,
    External,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateContract {
    Stateless,
    WarmSuspend,
    ColdResume,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnifiedEventLoopContract {
    DesktopWindowed,
    ShellOwnedExternal,
    DesktopSpecialAction,
    HeadlessService,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CapabilityProfile {
    pub file_system: bool,
    pub file_dialogs: bool,
    pub notifications: bool,
}

impl CapabilityProfile {
    pub const fn service_defaults() -> Self {
        Self {
            file_system: false,
            file_dialogs: false,
            notifications: false,
        }
    }

    pub const fn shell_defaults() -> Self {
        Self {
            file_system: false,
            file_dialogs: false,
            notifications: true,
        }
    }

    pub const fn file_worker() -> Self {
        Self {
            file_system: true,
            file_dialogs: true,
            notifications: true,
        }
    }

    pub const fn permissions(self) -> [Option<DesktopPermission>; 3] {
        [
            if self.file_system {
                Some(DesktopPermission::FileSystem)
            } else {
                None
            },
            if self.file_dialogs {
                Some(DesktopPermission::FileDialogs)
            } else {
                None
            },
            if self.notifications {
                Some(DesktopPermission::Notifications)
            } else {
                None
            },
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppIdentity {
    pub app_id: AppId,
    pub package_id: &'static str,
    pub slug: &'static str,
    pub title: &'static str,
    pub install_root: AppInstallRoot,
    pub trust: AppTrust,
}

impl AppIdentity {
    pub const fn new(
        app_id: AppId,
        package_id: &'static str,
        slug: &'static str,
        title: &'static str,
        install_root: AppInstallRoot,
        trust: AppTrust,
    ) -> Self {
        Self {
            app_id,
            package_id,
            slug,
            title,
            install_root,
            trust,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppManifest {
    pub identity: AppIdentity,
    pub loader: LoaderDispatch,
    pub abi: AbiPersonality,
    pub presentation: AppPresentation,
    pub capabilities: CapabilityProfile,
    pub file_associations: &'static [&'static str],
    pub state_contract: StateContract,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppDescriptor {
    pub app_id: AppId,
    pub package_id: &'static str,
    pub slug: &'static str,
    pub title: &'static str,
    pub loader: LoaderDispatch,
    pub abi: AbiPersonality,
    pub presentation: AppPresentation,
    pub capabilities: CapabilityProfile,
    pub install_root: AppInstallRoot,
    pub trust: AppTrust,
    pub file_associations: &'static [&'static str],
    pub state_contract: StateContract,
}

impl AppDescriptor {
    pub const fn new(
        app_id: AppId,
        slug: &'static str,
        title: &'static str,
        loader: LoaderDispatch,
        abi: AbiPersonality,
        presentation: AppPresentation,
        capabilities: CapabilityProfile,
    ) -> Self {
        Self {
            app_id,
            package_id: slug,
            slug,
            title,
            loader,
            abi,
            presentation,
            capabilities,
            install_root: default_install_root(loader, presentation),
            trust: default_trust(loader, presentation),
            file_associations: &[],
            state_contract: StateContract::Stateless,
        }
    }

    pub fn with_package_id(mut self, package_id: &'static str) -> Self {
        self.package_id = package_id;
        self
    }

    pub fn with_install_root(mut self, install_root: AppInstallRoot) -> Self {
        self.install_root = install_root;
        self
    }

    pub fn with_trust(mut self, trust: AppTrust) -> Self {
        self.trust = trust;
        self
    }

    pub fn with_file_associations(mut self, file_associations: &'static [&'static str]) -> Self {
        self.file_associations = file_associations;
        self
    }

    pub fn with_state_contract(mut self, state_contract: StateContract) -> Self {
        self.state_contract = state_contract;
        self
    }

    pub const fn identity(self) -> AppIdentity {
        AppIdentity::new(
            self.app_id,
            self.package_id,
            self.slug,
            self.title,
            self.install_root,
            self.trust,
        )
    }

    pub const fn manifest(self) -> AppManifest {
        AppManifest {
            identity: self.identity(),
            loader: self.loader,
            abi: self.abi,
            presentation: self.presentation,
            capabilities: self.capabilities,
            file_associations: self.file_associations,
            state_contract: self.state_contract,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecutionContext {
    pub source: LaunchSource,
    pub workspace_id: WorkspaceId,
    pub route_label: &'static str,
}

impl ExecutionContext {
    pub const fn new(
        source: LaunchSource,
        workspace_id: WorkspaceId,
        route_label: &'static str,
    ) -> Self {
        Self {
            source,
            workspace_id,
            route_label,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LaunchIntent {
    pub descriptor: AppDescriptor,
    pub context: ExecutionContext,
}

impl LaunchIntent {
    pub const fn new(descriptor: AppDescriptor, context: ExecutionContext) -> Self {
        Self {
            descriptor,
            context,
        }
    }

    pub const fn canonical_session(self) -> LaunchSession {
        LaunchSession::new(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DpiVirtualizationMode {
    Native,
    BitmapScale,
    SystemAware,
    PerMonitorAware,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExternalDisplayContract {
    pub output_id: u32,
    pub ui_scale_100x: u16,
    pub text_scale_100x: u16,
    pub cursor_scale_100x: u16,
    pub dpi_virtualization: DpiVirtualizationMode,
}

impl Default for ExternalDisplayContract {
    fn default() -> Self {
        Self {
            output_id: 0,
            ui_scale_100x: 100,
            text_scale_100x: 100,
            cursor_scale_100x: 100,
            dpi_virtualization: DpiVirtualizationMode::Native,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessContract {
    pub loader: LoaderDispatch,
    pub abi: AbiPersonality,
    pub bootstrap: RuntimeBootstrap,
    pub shell_owned: bool,
    pub external_display: ExternalDisplayContract,
}

impl ProcessContract {
    pub const fn from_descriptor(descriptor: AppDescriptor) -> Self {
        let bootstrap = match (descriptor.loader, descriptor.abi, descriptor.presentation) {
            (LoaderDispatch::Native, AbiPersonality::Native, AppPresentation::Windowed) => {
                RuntimeBootstrap::NativeWindowed
            }
            (LoaderDispatch::Native, AbiPersonality::Native, AppPresentation::SpecialAction) => {
                RuntimeBootstrap::NativeSpecialAction
            }
            (LoaderDispatch::Native, AbiPersonality::Native, AppPresentation::Headless) => {
                RuntimeBootstrap::NativeHeadless
            }
            (LoaderDispatch::Pe, AbiPersonality::Win32, AppPresentation::ShellOwned) => {
                RuntimeBootstrap::Win32Bridge
            }
            (LoaderDispatch::Elf, AbiPersonality::Posix, AppPresentation::ShellOwned) => {
                RuntimeBootstrap::PosixBridge
            }
            _ => RuntimeBootstrap::NativeSpecialAction,
        };
        Self {
            loader: descriptor.loader,
            abi: descriptor.abi,
            bootstrap,
            shell_owned: matches!(descriptor.presentation, AppPresentation::ShellOwned),
            external_display: ExternalDisplayContract {
                output_id: 0,
                ui_scale_100x: 100,
                text_scale_100x: 100,
                cursor_scale_100x: 100,
                dpi_virtualization: DpiVirtualizationMode::Native,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowEndpointContract {
    pub app_id: AppId,
    pub workspace_id: WorkspaceId,
    pub presentation: AppPresentation,
    pub shell_owned: bool,
}

impl WindowEndpointContract {
    pub const fn from_intent(intent: LaunchIntent) -> Self {
        Self {
            app_id: intent.descriptor.app_id,
            workspace_id: intent.context.workspace_id,
            presentation: intent.descriptor.presentation,
            shell_owned: matches!(intent.descriptor.presentation, AppPresentation::ShellOwned),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LaunchSession {
    pub intent: LaunchIntent,
    pub process: ProcessContract,
    pub window: WindowEndpointContract,
    pub event_loop: UnifiedEventLoopContract,
}

impl LaunchSession {
    pub const fn new(intent: LaunchIntent) -> Self {
        let process = ProcessContract::from_descriptor(intent.descriptor);
        let window = WindowEndpointContract::from_intent(intent);
        let event_loop = match intent.descriptor.presentation {
            AppPresentation::Windowed => UnifiedEventLoopContract::DesktopWindowed,
            AppPresentation::ShellOwned => UnifiedEventLoopContract::ShellOwnedExternal,
            AppPresentation::SpecialAction => UnifiedEventLoopContract::DesktopSpecialAction,
            AppPresentation::Headless => UnifiedEventLoopContract::HeadlessService,
        };
        Self {
            intent,
            process,
            window,
            event_loop,
        }
    }

    pub const fn with_external_display_contract(
        mut self,
        contract: ExternalDisplayContract,
    ) -> Self {
        self.process.external_display = contract;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppResolution {
    BuiltIn(AppDescriptor),
    ExternalPath {
        descriptor: AppDescriptor,
        path: String,
    },
    MissingExternalPath {
        descriptor: AppDescriptor,
        candidates: &'static [&'static str],
    },
}

impl AppResolution {
    pub fn descriptor(&self) -> AppDescriptor {
        match self {
            Self::BuiltIn(descriptor) => *descriptor,
            Self::ExternalPath { descriptor, .. } => *descriptor,
            Self::MissingExternalPath { descriptor, .. } => *descriptor,
        }
    }

    pub fn path(&self) -> Option<&str> {
        match self {
            Self::BuiltIn(_) => None,
            Self::ExternalPath { path, .. } => Some(path.as_str()),
            Self::MissingExternalPath { .. } => None,
        }
    }

    pub const fn missing_candidates(&self) -> Option<&'static [&'static str]> {
        match self {
            Self::MissingExternalPath { candidates, .. } => Some(candidates),
            _ => None,
        }
    }

    pub fn launch_intent(&self, context: ExecutionContext) -> LaunchIntent {
        LaunchIntent::new(self.descriptor(), context)
    }

    pub fn identity(&self) -> AppIdentity {
        self.descriptor().identity()
    }

    pub fn manifest(&self) -> AppManifest {
        self.descriptor().manifest()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PackageRecord {
    pub aliases: &'static [&'static str],
    pub descriptor: AppDescriptor,
    pub external_candidates: &'static [&'static str],
}

pub fn resolve_launch_query(query: &str, registry: &[PackageRecord]) -> Option<AppResolution> {
    resolve_launch_query_with_probe(query, registry, |_| false)
}

pub fn resolve_launch_query_with_probe<F>(
    query: &str,
    registry: &[PackageRecord],
    mut path_exists: F,
) -> Option<AppResolution>
where
    F: FnMut(&str) -> bool,
{
    let normalized = query.trim();
    if normalized.is_empty() {
        return None;
    }
    if let Some(path_resolution) = resolve_external_image(normalized) {
        return Some(path_resolution);
    }
    let lowered = normalized.to_ascii_lowercase();
    for record in registry {
        if record.matches_query(lowered.as_str()) {
            if !record.external_candidates.is_empty() {
                for candidate in record.external_candidates {
                    if path_exists(candidate) {
                        return Some(AppResolution::ExternalPath {
                            descriptor: record.descriptor,
                            path: (*candidate).to_string(),
                        });
                    }
                }
                return Some(AppResolution::MissingExternalPath {
                    descriptor: record.descriptor,
                    candidates: record.external_candidates,
                });
            }
            return Some(AppResolution::BuiltIn(record.descriptor));
        }
    }
    None
}

pub fn resolve_file_association(path: &str, registry: &[PackageRecord]) -> Option<AppResolution> {
    let normalized = path.trim();
    let extension = normalized
        .rsplit_once('.')
        .map(|(_, ext)| alloc::format!(".{}", ext.to_ascii_lowercase()))?;
    registry
        .iter()
        .find(|record| record.matches_file_association(extension.as_str()))
        .map(|record| AppResolution::BuiltIn(record.descriptor))
}

pub fn resolve_external_image(path: &str) -> Option<AppResolution> {
    let normalized = path.trim();
    let descriptor = descriptor_for_external_path(normalized)?;
    Some(AppResolution::ExternalPath {
        descriptor,
        path: normalized.to_string(),
    })
}

pub fn looks_like_external_image_query(query: &str) -> bool {
    descriptor_for_external_path(query.trim()).is_some()
}

fn descriptor_for_external_path(path: &str) -> Option<AppDescriptor> {
    let normalized = path.trim();
    if normalized.is_empty() {
        return None;
    }
    let file_name = normalized
        .rsplit(['/', '\\'])
        .next()
        .filter(|segment| !segment.is_empty())
        .unwrap_or(normalized);
    let lowered = file_name.to_ascii_lowercase();
    if lowered.ends_with(".exe") {
        return Some(
            AppDescriptor::new(
                external_app_id(normalized, LoaderDispatch::Pe),
                "external-pe",
                "Windows App",
                LoaderDispatch::Pe,
                AbiPersonality::Win32,
                AppPresentation::ShellOwned,
                CapabilityProfile::shell_defaults(),
            )
            .with_package_id("external.pe")
            .with_install_root(AppInstallRoot::ExternalImage)
            .with_trust(AppTrust::External),
        );
    }
    if lowered.ends_with(".elf") || lowered.ends_with(".bin") {
        return Some(
            AppDescriptor::new(
                external_app_id(normalized, LoaderDispatch::Elf),
                "external-elf",
                "POSIX App",
                LoaderDispatch::Elf,
                AbiPersonality::Posix,
                AppPresentation::ShellOwned,
                CapabilityProfile::shell_defaults(),
            )
            .with_package_id("external.elf")
            .with_install_root(AppInstallRoot::ExternalImage)
            .with_trust(AppTrust::External),
        );
    }
    None
}

impl PackageRecord {
    pub fn matches_query(&self, query: &str) -> bool {
        self.descriptor.package_id.eq_ignore_ascii_case(query)
            || self
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(query))
    }

    pub fn matches_file_association(&self, extension: &str) -> bool {
        self.descriptor
            .file_associations
            .iter()
            .any(|association| association.eq_ignore_ascii_case(extension))
    }
}

const fn default_install_root(
    loader: LoaderDispatch,
    presentation: AppPresentation,
) -> AppInstallRoot {
    match presentation {
        AppPresentation::Headless => AppInstallRoot::Service,
        AppPresentation::ShellOwned => match loader {
            LoaderDispatch::Pe | LoaderDispatch::Elf => AppInstallRoot::ExternalImage,
            LoaderDispatch::Native => AppInstallRoot::UserApps,
        },
        AppPresentation::Windowed | AppPresentation::SpecialAction => AppInstallRoot::SystemApps,
    }
}

const fn default_trust(loader: LoaderDispatch, presentation: AppPresentation) -> AppTrust {
    match presentation {
        AppPresentation::Headless => AppTrust::Platform,
        AppPresentation::ShellOwned => match loader {
            LoaderDispatch::Pe | LoaderDispatch::Elf => AppTrust::External,
            LoaderDispatch::Native => AppTrust::Installed,
        },
        AppPresentation::Windowed | AppPresentation::SpecialAction => AppTrust::Platform,
    }
}

fn external_app_id(path: &str, loader: LoaderDispatch) -> AppId {
    let mut hash = 0x811C_9DC5u32;
    for byte in path.as_bytes() {
        hash ^= *byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    let loader_tag = match loader {
        LoaderDispatch::Native => 0x1000_0000,
        LoaderDispatch::Pe => 0x5000_0000,
        LoaderDispatch::Elf => 0x6000_0000,
    };
    loader_tag | (hash & 0x0FFF_FFFF)
}

#[cfg(test)]
mod tests {
    use super::{
        looks_like_external_image_query, resolve_external_image, resolve_file_association,
        resolve_launch_query, resolve_launch_query_with_probe, AbiPersonality, AppDescriptor,
        AppInstallRoot, AppPresentation, AppTrust, CapabilityProfile, DpiVirtualizationMode,
        ExecutionContext, ExternalDisplayContract, LaunchIntent, LaunchSource, LoaderDispatch,
        PackageRecord, RuntimeBootstrap, StateContract, UnifiedEventLoopContract,
    };

    #[test]
    fn native_windowed_descriptor_carries_loader_and_abi_contract() {
        let descriptor = AppDescriptor::new(
            42,
            "terminal",
            "Terminal",
            LoaderDispatch::Native,
            AbiPersonality::Native,
            AppPresentation::Windowed,
            CapabilityProfile::shell_defaults(),
        );
        let intent = LaunchIntent::new(
            descriptor,
            ExecutionContext::new(LaunchSource::DesktopShortcut, 0, "desktop"),
        );
        assert_eq!(intent.descriptor.loader, LoaderDispatch::Native);
        assert_eq!(intent.descriptor.abi, AbiPersonality::Native);
        assert_eq!(intent.descriptor.presentation, AppPresentation::Windowed);
        assert_eq!(intent.context.source, LaunchSource::DesktopShortcut);
        assert_eq!(intent.descriptor.install_root, AppInstallRoot::SystemApps);
        assert_eq!(intent.descriptor.trust, AppTrust::Platform);
    }

    #[test]
    fn file_worker_capability_profile_exports_expected_permissions() {
        let permissions = CapabilityProfile::file_worker().permissions();
        assert!(permissions[0].is_some());
        assert!(permissions[1].is_some());
        assert!(permissions[2].is_some());
    }

    #[test]
    fn exe_query_resolves_to_pe_win32_shell_owned_descriptor() {
        let resolution = resolve_external_image("/workspace/demo.exe").expect("pe resolution");
        let descriptor = resolution.descriptor();
        assert_eq!(descriptor.loader, LoaderDispatch::Pe);
        assert_eq!(descriptor.abi, AbiPersonality::Win32);
        assert_eq!(descriptor.presentation, AppPresentation::ShellOwned);
        assert_eq!(resolution.path(), Some("/workspace/demo.exe"));
    }

    #[test]
    fn elf_query_detection_accepts_shell_launch_paths() {
        assert!(looks_like_external_image_query("/workspace/hello.elf"));
        assert!(looks_like_external_image_query("build/app.bin"));
        assert!(!looks_like_external_image_query("settings"));
    }

    #[test]
    fn launch_session_projects_runtime_bootstrap_and_event_loop() {
        let descriptor = AppDescriptor::new(
            64,
            "demo-pe",
            "Demo PE",
            LoaderDispatch::Pe,
            AbiPersonality::Win32,
            AppPresentation::ShellOwned,
            CapabilityProfile::shell_defaults(),
        );
        let session = LaunchIntent::new(
            descriptor,
            ExecutionContext::new(LaunchSource::CommandPalette, 3, "demo-pe"),
        )
        .canonical_session();
        assert_eq!(session.process.bootstrap, RuntimeBootstrap::Win32Bridge);
        assert!(session.process.shell_owned);
        assert_eq!(session.window.workspace_id, 3);
        assert_eq!(
            session.event_loop,
            UnifiedEventLoopContract::ShellOwnedExternal
        );
    }

    #[test]
    fn launch_session_accepts_external_display_contract_for_bridge_runtimes() {
        let descriptor = AppDescriptor::new(
            65,
            "demo-pe",
            "Demo PE",
            LoaderDispatch::Pe,
            AbiPersonality::Win32,
            AppPresentation::ShellOwned,
            CapabilityProfile::shell_defaults(),
        );
        let session = LaunchIntent::new(
            descriptor,
            ExecutionContext::new(LaunchSource::CommandPalette, 2, "demo-pe"),
        )
        .canonical_session()
        .with_external_display_contract(ExternalDisplayContract {
            output_id: 3,
            ui_scale_100x: 150,
            text_scale_100x: 175,
            cursor_scale_100x: 125,
            dpi_virtualization: DpiVirtualizationMode::BitmapScale,
        });
        assert_eq!(session.process.external_display.output_id, 3);
        assert_eq!(session.process.external_display.ui_scale_100x, 150);
        assert_eq!(
            session.process.external_display.dpi_virtualization,
            DpiVirtualizationMode::BitmapScale
        );
    }

    #[test]
    fn registry_query_resolves_builtin_alias_before_falling_back() {
        let descriptor = AppDescriptor::new(
            7,
            "web",
            "Web",
            LoaderDispatch::Native,
            AbiPersonality::Native,
            AppPresentation::Windowed,
            CapabilityProfile::file_worker(),
        )
        .with_package_id("echos.web")
        .with_file_associations(&[".html", ".htm", ".url"])
        .with_state_contract(StateContract::WarmSuspend);
        let registry = [PackageRecord {
            aliases: &["web", "browser"],
            descriptor,
            external_candidates: &[],
        }];
        let resolution = resolve_launch_query("browser", &registry).expect("registry resolution");
        assert_eq!(resolution.descriptor().app_id, 7);
        assert_eq!(
            resolution.descriptor().presentation,
            AppPresentation::Windowed
        );
        assert_eq!(resolution.descriptor().package_id, "echos.web");
        assert_eq!(
            resolution.manifest().state_contract,
            StateContract::WarmSuspend
        );
    }

    #[test]
    fn external_alias_resolution_prefers_first_existing_candidate() {
        let descriptor = AppDescriptor::new(
            8,
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
        let registry = [PackageRecord {
            aliases: &["firefox", "browser-firefox"],
            descriptor,
            external_candidates: &[
                "/downloads/firefox/firefox.exe",
                "/programs/firefox/firefox.exe",
            ],
        }];
        let resolution = resolve_launch_query_with_probe("firefox", &registry, |path| {
            path == "/programs/firefox/firefox.exe"
        })
        .expect("resolution");
        assert_eq!(resolution.path(), Some("/programs/firefox/firefox.exe"));
        assert_eq!(resolution.descriptor().title, "Firefox");
        assert_eq!(resolution.identity().package_id, "org.mozilla.firefox");
    }

    #[test]
    fn external_alias_resolution_reports_missing_candidates_when_not_installed() {
        let descriptor = AppDescriptor::new(
            9,
            "chromium",
            "Chromium",
            LoaderDispatch::Pe,
            AbiPersonality::Win32,
            AppPresentation::ShellOwned,
            CapabilityProfile::shell_defaults(),
        )
        .with_package_id("org.chromium.browser")
        .with_install_root(AppInstallRoot::UserApps)
        .with_trust(AppTrust::Installed);
        let registry = [PackageRecord {
            aliases: &["chromium", "chrome"],
            descriptor,
            external_candidates: &[
                "/downloads/chromium/chrome.exe",
                "/programs/chromium/chrome.exe",
            ],
        }];
        let resolution =
            resolve_launch_query_with_probe("chrome", &registry, |_| false).expect("resolution");
        assert!(resolution.path().is_none());
        assert_eq!(
            resolution.missing_candidates(),
            Some(
                &[
                    "/downloads/chromium/chrome.exe",
                    "/programs/chromium/chrome.exe",
                ][..]
            )
        );
        assert_eq!(resolution.descriptor().title, "Chromium");
    }

    #[test]
    fn file_association_resolution_uses_manifest_contract() {
        let descriptor = AppDescriptor::new(
            12,
            "editor",
            "Editor",
            LoaderDispatch::Native,
            AbiPersonality::Native,
            AppPresentation::Windowed,
            CapabilityProfile::file_worker(),
        )
        .with_package_id("echos.editor")
        .with_file_associations(&[".txt", ".md", ".log"])
        .with_state_contract(StateContract::ColdResume);
        let registry = [PackageRecord {
            aliases: &["editor"],
            descriptor,
            external_candidates: &[],
        }];
        let resolution = resolve_file_association("/workspace/notes.md", &registry).expect("assoc");
        assert_eq!(resolution.identity().package_id, "echos.editor");
        assert_eq!(
            resolution.manifest().state_contract,
            StateContract::ColdResume
        );
    }
}
