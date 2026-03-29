//! Active desktop session runtime for the native echOS shell.

mod app_runtime;
mod bootstrap;
mod launch;
mod session_runtime;

use super::super::boot::appliance::{auto_login_requested, suspend_resume_smoke_requested};
use super::super::drivers::audio::get_mixer;
use super::super::drivers::audio_jail::primary_controller_status;
use super::super::drivers::rtc::get_cached_datetime;
use super::super::font::vga_font;
use super::super::fs::f2fs::{open_entry, F2fsEntry};
use super::super::gop::framebuffer::Framebuffer;
use super::super::gui::animation::{
    add_animation, get_animation_value, get_time_ns, remove_animation, update_animations,
    Animation, AnimationTarget, AnimationTargetType,
};
use super::super::gui::client::{ClientWindow, DesktopClient};
use super::super::gui::icon_pack::{emit_desktop_icon_rects, DesktopIconKind};
use super::super::gui::launch_pipeline::{
    AbiPersonality, AppDescriptor, AppInstallRoot, AppPresentation, AppResolution, AppTrust,
    CapabilityProfile, ExecutionContext, LaunchIntent, LaunchSession, LaunchSource, LoaderDispatch,
    PackageRecord, RuntimeBootstrap, StateContract, UnifiedEventLoopContract,
};
use super::super::gui::layout::layout_grid;
use super::super::gui::layout::{layout_flex, EdgeInsets, FlexDirection, FlexItem};
use super::super::gui::protocol::{
    AccessibilityNode, AccessibilityProfile, AccessibilityRole, AppHealth, ClipboardPayload,
    CommandPaletteAction, DamageLane, DesktopPermission, DialogKind, DialogRequest,
    DialogSelection, DisplayProfile, HdrPolicy, InputEvent, InvalidationReason, InvalidationTarget,
    KeyState, LayerRole, MotionProfile, NotificationEntry, NotificationLevel, PermissionState,
    Point, PointerButton, Rect, RenderObjectKind, RestoreDisposition, SceneNodeId, SceneUpdate,
    SessionPowerState, SessionSnapshot, ShellAppEntry, ShellDensityProfile, ShellShortcut,
    ShellState, StageSet, StageSetPolicy, VrrPolicy, WindowFlags, WindowId, WindowInfo,
    WindowInputEvent, WorkspaceLayout, WorkspaceRule, MOD_CTRL,
};
use super::super::gui::scene::SceneGraph;
use super::super::gui::text::{TextStyle, TextSystem};
use super::super::gui::theme::{ShellLayoutProfile, Theme, ThemeMode};
use super::super::gui::window_manager::{
    BORDER_THICKNESS, MIN_CONTENT_HEIGHT, MIN_CONTENT_WIDTH, TITLEBAR_HEIGHT,
};
use super::super::net::http::{HttpClient, HttpUrl};
use super::super::net::smoltcp_driver::{get_gateway, get_ip};
use super::super::net::wireguard::runtime_status as wireguard_runtime_status;
use super::super::personalization::{hybrid_windowing, virtual_desktops};
use super::super::runtime_layer::package_registry_contract::RuntimePackageRegistry;
use super::super::security::users::USER_DB;
use super::super::serial_println;
use super::super::services::FileEntry;
use super::super::shell::Shell;
use super::super::task::scheduler::get_cpu_load;
use super::super::tty::pty::{
    configure_pty_for_shell, execute_command_on_pty_with_shell, pty_has_output,
    write_welcome_message, PtyPair, Winsize, PTY_MANAGER,
};
use super::shell_invalidation::{ShellFramePlan, ShellInvalidationState};
use super::shell_scene::{
    push_scene_outline, push_scene_panel, push_scene_rect, push_scene_round_rect, push_scene_text,
    raster_surface_scene,
};
use super::velvet_glove_registry::{desktop_launch_registry, CEF_BINARY_CANDIDATES};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use app_runtime::*;
use bootstrap::{
    configure_shell_environment, connect_clients, create_shell_windows, desktop_surface_rect,
    register_bootstrap_apps, task_strip_window_rect, DesktopBootstrapClients, DesktopShellWindows,
};
use core::cmp::{max, min};
use htmlparser::{ElementEnd as HtmlElementEnd, Token as HtmlToken, Tokenizer as HtmlTokenizer};
use session_runtime::session_snapshot_or_fallback;
use x86_64::instructions::port::PortWriteOnly;

const SHELL_APP_ID: u32 = 1;
const TERMINAL_APP_ID: u32 = 10;
const FILES_APP_ID: u32 = 11;
const SETTINGS_APP_ID: u32 = 12;
const EDITOR_APP_ID: u32 = 13;
const BROWSER_APP_ID: u32 = 14;
const RECYCLE_SHORTCUT_APP_ID: u32 = 15;
const WORKSPACE_COUNT: u8 = 8;
const SCRATCHPAD_WORKSPACE: u8 = WORKSPACE_COUNT;
const DESKTOP_SHORTCUT_ICON_SIZE: i32 = 56;
const DESKTOP_SHORTCUT_STEP_Y: i32 = 102;
const FONT_WIDTH: i32 = 8;
const FONT_HEIGHT: i32 = 16;
const WINDOW_BG: u32 = 0xFF131C27;
const PANEL_BG: u32 = 0xFF1C2836;
const PANEL_ALT: u32 = 0xFF223245;
const BORDER: u32 = 0xFF3B5168;
const TEXT_PRIMARY: u32 = 0xFFF2F7FA;
const TEXT_SECONDARY: u32 = 0xFFA3B8C8;
const TEXT_MUTED: u32 = 0xFF7890A4;
const ACCENT_MINT: u32 = 0xFF26E6C6;
const ACCENT_BLUE: u32 = 0xFF5AB3FF;
const ACCENT_GOLD: u32 = 0xFFFFB84D;
const ACCENT_CORAL: u32 = 0xFFFF7866;
const ACCENT_SOFT: u32 = 0xFF7FE6A6;

fn shell_layer_flags() -> WindowFlags {
    WindowFlags::layer_shell()
}

fn scratchpad_flags() -> WindowFlags {
    WindowFlags {
        scratchpad: true,
        ..WindowFlags::layer_shell()
    }
}

fn default_workspace_rule(workspace_id: u8) -> WorkspaceRule {
    let mut name = [0u8; 16];
    let label = match workspace_id {
        0 => "Prime",
        1 => "Build",
        2 => "Observe",
        3 => "Docs",
        4 => "Net",
        5 => "Media",
        6 => "Lab",
        7 => "Ops",
        _ => "Scratchpad",
    };
    let bytes = label.as_bytes();
    let len = bytes.len().min(name.len());
    name[..len].copy_from_slice(&bytes[..len]);
    WorkspaceRule::new(name, WorkspaceLayout::Dwindle)
}

fn top_bar_workspace_label(workspace_id: u8) -> &'static str {
    match workspace_id {
        0 => "Prime",
        1 => "Build",
        2 => "Observe",
        3 => "Docs",
        4 => "Net",
        5 => "Media",
        6 => "Lab",
        7 => "Ops",
        _ => "Scratch",
    }
}

#[derive(Clone, Copy)]
struct UiPalette {
    window_bg: u32,
    panel_bg: u32,
    panel_alt: u32,
    border: u32,
    text_primary: u32,
    text_secondary: u32,
    text_muted: u32,
    accent_mint: u32,
    accent_blue: u32,
    accent_gold: u32,
    accent_coral: u32,
    accent_soft: u32,
}

fn hybrid_titan_palette(theme_mode: ThemeMode) -> UiPalette {
    let _ = theme_mode;
    UiPalette {
        window_bg: 0xFF0C1722,
        panel_bg: 0xFF0A131E,
        panel_alt: 0xFF111C29,
        border: 0xFF2B4156,
        text_primary: 0xFFEDF4FB,
        text_secondary: 0xFFA6B7CA,
        text_muted: 0xFF6D8196,
        accent_mint: 0xFF29E4C6,
        accent_blue: 0xFF60B8FF,
        accent_gold: 0xFFFFBB57,
        accent_coral: 0xFFFF7474,
        accent_soft: 0xFF5DE1A4,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AppKind {
    Terminal,
    Files,
    Browser,
    Settings,
    Editor,
}

impl AppKind {
    const ALL: [Self; 5] = [
        Self::Terminal,
        Self::Files,
        Self::Browser,
        Self::Settings,
        Self::Editor,
    ];

    fn app_id(self) -> u32 {
        match self {
            Self::Terminal => TERMINAL_APP_ID,
            Self::Files => FILES_APP_ID,
            Self::Browser => BROWSER_APP_ID,
            Self::Settings => SETTINGS_APP_ID,
            Self::Editor => EDITOR_APP_ID,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Terminal => "Terminal",
            Self::Files => "Files",
            Self::Browser => "Web",
            Self::Settings => "Settings",
            Self::Editor => "Editor",
        }
    }

    fn accent(self) -> u32 {
        match self {
            Self::Terminal => ACCENT_MINT,
            Self::Files => ACCENT_BLUE,
            Self::Browser => ACCENT_SOFT,
            Self::Settings => ACCENT_GOLD,
            Self::Editor => ACCENT_CORAL,
        }
    }

    fn shortcut(self) -> char {
        match self {
            Self::Terminal => '1',
            Self::Files => '2',
            Self::Browser => '3',
            Self::Settings => '4',
            Self::Editor => '5',
        }
    }

    fn icon(self) -> DesktopIconKind {
        match self {
            Self::Terminal => DesktopIconKind::Terminal,
            Self::Files => DesktopIconKind::Files,
            Self::Browser => DesktopIconKind::Browser,
            Self::Settings => DesktopIconKind::Settings,
            Self::Editor => DesktopIconKind::Editor,
        }
    }

    fn descriptor(self) -> AppDescriptor {
        let capabilities = match self {
            Self::Terminal => CapabilityProfile::shell_defaults(),
            Self::Files => CapabilityProfile::file_worker(),
            Self::Browser => CapabilityProfile::file_worker(),
            Self::Settings => CapabilityProfile::shell_defaults(),
            Self::Editor => CapabilityProfile::file_worker(),
        };
        let descriptor = AppDescriptor::new(
            self.app_id(),
            self.title(),
            self.title(),
            LoaderDispatch::Native,
            AbiPersonality::Native,
            AppPresentation::Windowed,
            capabilities,
        )
        .with_install_root(AppInstallRoot::SystemApps)
        .with_trust(AppTrust::Platform);
        match self {
            Self::Browser => descriptor
                .with_package_id("echos.web")
                .with_file_associations(&[".html", ".htm", ".url"])
                .with_state_contract(StateContract::WarmSuspend),
            Self::Editor => descriptor
                .with_package_id("echos.editor")
                .with_file_associations(&[".txt", ".md", ".log"])
                .with_state_contract(StateContract::ColdResume),
            Self::Files => descriptor
                .with_package_id("echos.files")
                .with_state_contract(StateContract::WarmSuspend),
            Self::Terminal => descriptor
                .with_package_id("echos.terminal")
                .with_state_contract(StateContract::WarmSuspend),
            Self::Settings => descriptor
                .with_package_id("echos.settings")
                .with_state_contract(StateContract::WarmSuspend),
        }
    }
}

fn app_kind_from_id(app_id: u32) -> Option<AppKind> {
    match app_id {
        TERMINAL_APP_ID => Some(AppKind::Terminal),
        FILES_APP_ID => Some(AppKind::Files),
        BROWSER_APP_ID => Some(AppKind::Browser),
        SETTINGS_APP_ID => Some(AppKind::Settings),
        EDITOR_APP_ID => Some(AppKind::Editor),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug)]
enum LaunchResult {
    Launched,
    Restored,
    Focused,
}

impl LaunchResult {
    fn as_label(self) -> &'static str {
        match self {
            Self::Launched => "launched",
            Self::Restored => "restored",
            Self::Focused => "focused",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DesktopShortcutKind {
    Terminal,
    Files,
    Web,
    Settings,
    RecycleBin,
}

impl DesktopShortcutKind {
    const ALL: [Self; 5] = [
        Self::Terminal,
        Self::Files,
        Self::Web,
        Self::Settings,
        Self::RecycleBin,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Terminal => "Terminal",
            Self::Files => "Files",
            Self::Web => "Web",
            Self::Settings => "Settings",
            Self::RecycleBin => "Recycle Bin",
        }
    }

    fn icon(self) -> DesktopIconKind {
        match self {
            Self::Terminal => DesktopIconKind::Terminal,
            Self::Files => DesktopIconKind::Files,
            Self::Web => DesktopIconKind::Browser,
            Self::Settings => DesktopIconKind::Settings,
            Self::RecycleBin => DesktopIconKind::Recycle,
        }
    }

    fn accent(self) -> u32 {
        match self {
            Self::Terminal => ACCENT_MINT,
            Self::Files => ACCENT_BLUE,
            Self::Web => ACCENT_SOFT,
            Self::Settings => ACCENT_GOLD,
            Self::RecycleBin => 0xFF536476,
        }
    }

    fn app_kind(self) -> Option<AppKind> {
        match self {
            Self::Terminal => Some(AppKind::Terminal),
            Self::Files => Some(AppKind::Files),
            Self::Web => Some(AppKind::Browser),
            Self::Settings => Some(AppKind::Settings),
            Self::RecycleBin => None,
        }
    }

    fn route_label(self) -> &'static str {
        match self {
            Self::Terminal => "desktop-terminal",
            Self::Files => "desktop-files",
            Self::Web => "desktop-web",
            Self::Settings => "desktop-settings",
            Self::RecycleBin => "desktop-recycle-bin",
        }
    }

    fn descriptor(self) -> AppDescriptor {
        match self {
            Self::Terminal | Self::Files | Self::Web | Self::Settings => self
                .app_kind()
                .expect("desktop native shortcut")
                .descriptor(),
            Self::RecycleBin => AppDescriptor::new(
                RECYCLE_SHORTCUT_APP_ID,
                "recycle-bin",
                "Recycle Bin",
                LoaderDispatch::Native,
                AbiPersonality::Native,
                AppPresentation::SpecialAction,
                CapabilityProfile::shell_defaults(),
            )
            .with_package_id("echos.recycle-bin")
            .with_install_root(AppInstallRoot::SystemApps)
            .with_trust(AppTrust::Platform),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct DesktopShortcutEntry {
    kind: DesktopShortcutKind,
    icon_rect: Rect,
    label_x: i32,
    label_y: i32,
    hit_rect: Rect,
}

#[derive(Clone, Debug, Default)]
struct TopBarStatusSummary {
    net: String,
    cpu: String,
    vpn: String,
    aud: String,
    time: String,
    power: String,
}

#[derive(Clone, Copy, Debug, Default)]
struct SessionWindow {
    window_id: WindowId,
    content_rect: Rect,
    visible: bool,
    focused: bool,
    opacity: f32,
    desired_visible: bool,
    fading_out: bool,
}

impl SessionWindow {
    fn from_client_window(window: ClientWindow) -> Self {
        Self {
            window_id: window.window_id,
            content_rect: window.content_rect,
            visible: true,
            focused: false,
            opacity: 1.0,
            desired_visible: true,
            fading_out: false,
        }
    }

    fn update_from_info(&mut self, info: &WindowInfo) -> bool {
        let changed = self.content_rect != info.content_rect
            || self.visible != info.visible
            || self.focused != info.focused;
        self.content_rect = info.content_rect;
        self.visible = info.visible;
        self.focused = info.focused;
        changed
    }
}

fn animation_window_id(window_id: WindowId) -> u32 {
    window_id as u32
}

fn window_opacity_target(window_id: WindowId) -> AnimationTarget {
    AnimationTarget {
        target_type: AnimationTargetType::WindowOpacity,
        id: animation_window_id(window_id),
    }
}

fn animate_shell_surface(
    client: &DesktopClient,
    window: &mut SessionWindow,
    visible: bool,
    animated: bool,
    duration: f64,
) {
    let target = window_opacity_target(window.window_id);
    remove_animation(&target);

    if visible {
        let _ = client.set_visibility(window.window_id, true);
        window.visible = true;
        window.desired_visible = true;
        window.fading_out = false;
        if animated {
            window.opacity = 0.0;
            let _ = add_animation(Animation::opacity(
                animation_window_id(window.window_id),
                0.0,
                1.0,
                duration,
            ));
        } else {
            window.opacity = 1.0;
        }
    } else if animated && window.visible {
        window.desired_visible = false;
        window.fading_out = true;
        let start = window.opacity.clamp(0.0, 1.0);
        let _ = add_animation(Animation::opacity(
            animation_window_id(window.window_id),
            start,
            0.0,
            duration,
        ));
    } else {
        let _ = client.set_visibility(window.window_id, false);
        window.visible = false;
        window.desired_visible = false;
        window.fading_out = false;
        window.opacity = 0.0;
    }
}

fn refresh_shell_surface_animation(client: &DesktopClient, window: &mut SessionWindow) -> bool {
    let target = window_opacity_target(window.window_id);
    let mut changed = false;

    if let Some(value) = get_animation_value(&target) {
        let next = value.clamp(0.0, 1.0);
        if (window.opacity - next).abs() > 0.01 {
            window.opacity = next;
            changed = true;
        }
    } else if window.desired_visible && window.opacity < 1.0 {
        window.opacity = 1.0;
        changed = true;
    }

    if window.fading_out && window.opacity <= 0.01 {
        let _ = client.set_visibility(window.window_id, false);
        window.visible = false;
        window.fading_out = false;
        window.opacity = 0.0;
        changed = true;
    }

    changed
}

fn set_shell_surface_visibility(client: &DesktopClient, window: &mut SessionWindow, visible: bool) {
    let _ = client.set_visibility(window.window_id, visible);
    window.visible = visible;
    window.desired_visible = visible;
    window.opacity = if visible { 1.0 } else { 0.0 };
    window.fading_out = false;
}

fn desktop_visibility_recovery_needed(
    power_state: SessionPowerState,
    shell_logged_in: bool,
    lock_screen_visible: bool,
    top_bar_visible: bool,
    task_strip_visible: bool,
    appliance_auto_login_pending: bool,
    desktop_ready_published: bool,
) -> bool {
    if power_state != SessionPowerState::Active {
        return false;
    }

    let should_be_on_desktop =
        shell_logged_in || appliance_auto_login_pending || desktop_ready_published;
    should_be_on_desktop
        && (!shell_logged_in || lock_screen_visible || !top_bar_visible || !task_strip_visible)
}

#[derive(Clone)]
struct AppSnapshot {
    kind: AppKind,
    window_id: Option<WindowId>,
    visible: bool,
    focused: bool,
    workspace_id: u8,
    detail: String,
    health: AppHealth,
    launch_count: u32,
    crash_count: u32,
    needs_attention: bool,
}

struct ShellRuntime {
    client: DesktopClient,
    desktop: SessionWindow,
    top_bar: SessionWindow,
    task_strip: SessionWindow,
    launcher: SessionWindow,
    notifications: SessionWindow,
    quick_settings: SessionWindow,
    command_palette: SessionWindow,
    stage_rail: SessionWindow,
    dialog: SessionWindow,
    context_menu: SessionWindow,
    switcher: SessionWindow,
    lock_screen: SessionWindow,
    notices: Vec<String>,
    active_workspace: u8,
    theme_mode: ThemeMode,
    layout_profile: ShellLayoutProfile,
    stage_sets: Vec<StageSet>,
    active_stage_set: usize,
    pending_dialog: Option<DialogRequest>,
    dialog_input: String,
    command_query: String,
    command_selection: usize,
    context_target: Option<AppKind>,
    switcher_index: usize,
    notification_index: usize,
    auth_input: String,
    logged_in: bool,
    selected_shortcut: Option<DesktopShortcutKind>,
    last_shortcut_click: Option<(DesktopShortcutKind, u64)>,
    invalidation: ShellInvalidationState,
}

struct TerminalApp {
    client: DesktopClient,
    window: Option<SessionWindow>,
    workspace_id: u8,
    lines: Vec<String>,
    input: String,
    shell: Shell,
    pty: Option<Arc<PtyPair>>,
    pending_dialogs: Vec<u64>,
    dirty: bool,
}

struct FilesApp {
    client: DesktopClient,
    window: Option<SessionWindow>,
    workspace_id: u8,
    current_path: String,
    entries: Vec<FileEntry>,
    selected: usize,
    status: String,
    dirty: bool,
}

struct SettingsApp {
    client: DesktopClient,
    window: Option<SessionWindow>,
    workspace_id: u8,
    focus_mode: bool,
    animations: bool,
    notifications: bool,
    dirty: bool,
}

struct BrowserApp {
    client: DesktopClient,
    window: Option<SessionWindow>,
    workspace_id: u8,
    address_input: String,
    current_url: Option<String>,
    content_type: String,
    preview_lines: Vec<String>,
    links: Vec<BrowserLink>,
    status: String,
    dirty: bool,
}

#[derive(Clone, Debug, Default)]
struct BrowserLink {
    label: String,
    url: String,
}

struct EditorApp {
    client: DesktopClient,
    window: Option<SessionWindow>,
    workspace_id: u8,
    path: Option<String>,
    text: String,
    pending_dialogs: Vec<EditorDialog>,
    status: String,
    document_dirty: bool,
    dirty: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditorDialogKind {
    Open,
    Save,
}

#[derive(Clone, Debug)]
struct EditorDialog {
    id: u64,
    kind: EditorDialogKind,
}

struct DesktopSession {
    screen: Rect,
    shell: ShellRuntime,
    text_system: TextSystem,
    terminal: TerminalApp,
    files: FilesApp,
    browser: BrowserApp,
    settings: SettingsApp,
    editor: EditorApp,
    appliance_auto_login_pending: bool,
    suspend_resume_smoke_pending: bool,
    desktop_ready_published: bool,
    app_basket_committed: bool,
    last_tick_ns: u64,
}

pub struct VelvetGloveCompositor;

#[derive(Default)]
struct UnifiedEventLoopBatch {
    shell_events: Vec<WindowInputEvent>,
    terminal_events: Vec<WindowInputEvent>,
    files_events: Vec<WindowInputEvent>,
    browser_events: Vec<WindowInputEvent>,
    settings_events: Vec<WindowInputEvent>,
    editor_events: Vec<WindowInputEvent>,
}

impl VelvetGloveCompositor {
    pub fn run(fb: &mut Framebuffer) -> ! {
        serial_println!("[DESKTOP] native session runtime active");
        debug_marker(b"VG0\n");
        let screen = Rect::new(0, 0, fb.width as u32, fb.height as u32);
        let mut session = match DesktopSession::new(screen) {
            Ok(session) => session,
            Err(err) => {
                serial_println!("[DESKTOP] session bootstrap failed: {}", err);
                loop {
                    unsafe {
                        core::arch::asm!("hlt");
                    }
                }
            }
        };

        loop {
            session.tick();
            unsafe {
                core::arch::asm!("hlt");
            }
        }
    }
}

impl DesktopSession {
    fn poll_unified_event_loop(&self) -> UnifiedEventLoopBatch {
        UnifiedEventLoopBatch {
            shell_events: self.shell.client.poll_input(32).unwrap_or_default(),
            terminal_events: self.terminal.client.poll_input(32).unwrap_or_default(),
            files_events: self.files.client.poll_input(32).unwrap_or_default(),
            browser_events: self.browser.client.poll_input(32).unwrap_or_default(),
            settings_events: self.settings.client.poll_input(32).unwrap_or_default(),
            editor_events: self.editor.client.poll_input(32).unwrap_or_default(),
        }
    }

    fn drive_unified_event_loop(
        &mut self,
        batch: UnifiedEventLoopBatch,
        commands: &mut Vec<SessionCommand>,
    ) {
        self.handle_shell_events(batch.shell_events, commands);
        if self.is_locked() {
            return;
        }
        self.terminal.handle_events(batch.terminal_events, commands);
        self.files.handle_events(batch.files_events, commands);
        self.browser.handle_events(batch.browser_events, commands);
        self.settings.handle_events(batch.settings_events);
        self.editor.handle_events(batch.editor_events);
        self.terminal.poll_platform();
        self.browser.poll_platform();
        self.editor.poll_platform();
    }

    fn invalidate_shell(&mut self, target: InvalidationTarget, reason: InvalidationReason) {
        self.shell.invalidation.mark(target, reason);
    }

    fn invalidate_shell_many(
        &mut self,
        targets: &[InvalidationTarget],
        reason: InvalidationReason,
    ) {
        self.shell.invalidation.mark_many(targets, reason);
    }

    fn shell_state(&self) -> ShellState {
        if self.is_locked() {
            ShellState::Locked
        } else if self.shell.quick_settings.visible
            || self.shell.command_palette.visible
            || self.shell.stage_rail.visible
            || self.shell.dialog.visible
            || self.shell.context_menu.visible
            || self.shell.switcher.visible
        {
            ShellState::OverlayInteractive
        } else {
            ShellState::DesktopReady
        }
    }

    fn commit_shell_surface(&self, window: &SessionWindow, pixels: Vec<u32>) -> Result<(), String> {
        self.shell.client.present(window.window_id, &pixels)
    }

    fn new(screen: Rect) -> Result<Self, String> {
        debug_marker(b"N00\n");
        let bootstrap_clients = connect_clients()?;

        debug_marker(b"N10\n");
        let DesktopShellWindows {
            desktop_window,
            top_bar_window,
            task_strip_window,
            launcher_window,
            notifications_window,
            quick_settings_window,
            command_palette_window,
            stage_rail_window,
            dialog_window,
            context_menu_window,
            switcher_window,
            lock_window,
        } = create_shell_windows(screen, &bootstrap_clients.shell_client)?;
        debug_marker(b"N20\n");
        register_bootstrap_apps(&bootstrap_clients);
        configure_shell_environment(&bootstrap_clients);
        let DesktopBootstrapClients {
            shell_client,
            terminal_client,
            files_client,
            browser_client,
            settings_client,
            editor_client,
        } = bootstrap_clients;
        debug_marker(b"N30\n");
        serial_println!("[DESKTOP] session bootstrap step=session-struct");
        let mut session = Self {
            screen,
            shell: ShellRuntime {
                client: shell_client,
                desktop: SessionWindow::from_client_window(desktop_window),
                top_bar: SessionWindow::from_client_window(top_bar_window),
                task_strip: SessionWindow::from_client_window(task_strip_window),
                launcher: SessionWindow::from_client_window(launcher_window),
                notifications: SessionWindow::from_client_window(notifications_window),
                quick_settings: SessionWindow::from_client_window(quick_settings_window),
                command_palette: SessionWindow::from_client_window(command_palette_window),
                stage_rail: SessionWindow::from_client_window(stage_rail_window),
                dialog: SessionWindow::from_client_window(dialog_window),
                context_menu: SessionWindow::from_client_window(context_menu_window),
                switcher: SessionWindow::from_client_window(switcher_window),
                lock_screen: SessionWindow::from_client_window(lock_window),
                notices: vec![
                    String::from("Session shell ready"),
                    String::from("Launcher uses click or keys 1-4"),
                ],
                active_workspace: 0,
                theme_mode: Theme::default_mode(),
                layout_profile: Theme::layout_profile(screen.width),
                stage_sets: Vec::new(),
                active_stage_set: 0,
                pending_dialog: None,
                dialog_input: String::new(),
                command_query: String::new(),
                command_selection: 0,
                context_target: None,
                switcher_index: 0,
                notification_index: 0,
                auth_input: String::new(),
                logged_in: false,
                selected_shortcut: None,
                last_shortcut_click: None,
                invalidation: ShellInvalidationState::bootstrap_shell(),
            },
            text_system: TextSystem::new(),
            terminal: TerminalApp {
                client: terminal_client,
                window: None,
                workspace_id: 0,
                lines: vec![
                    String::from("echOS native terminal"),
                    String::from("Type pwd, cd, ls, cp, or open <app>"),
                ],
                input: String::new(),
                shell: Shell::new(),
                pty: None,
                pending_dialogs: Vec::new(),
                dirty: true,
            },
            files: FilesApp {
                client: files_client,
                window: None,
                workspace_id: 0,
                current_path: String::from("/"),
                entries: Vec::new(),
                selected: 0,
                status: String::from("Ready"),
                dirty: true,
            },
            browser: BrowserApp {
                client: browser_client,
                window: None,
                workspace_id: 0,
                address_input: String::from("http://example.com/"),
                current_url: None,
                content_type: String::from("text/plain"),
                preview_lines: vec![
                    String::from("Enter a URL, then press Enter or click Open."),
                    String::from("Use Download to store binaries under /downloads."),
                ],
                links: Vec::new(),
                status: String::from("Browser ready"),
                dirty: true,
            },
            settings: SettingsApp {
                client: settings_client,
                window: None,
                workspace_id: 0,
                focus_mode: false,
                animations: true,
                notifications: true,
                dirty: true,
            },
            editor: EditorApp {
                client: editor_client,
                window: None,
                workspace_id: 0,
                path: None,
                text: String::from("echOS editor draft\n\nBuild the desktop around native apps."),
                pending_dialogs: Vec::new(),
                status: String::from("Scratch buffer"),
                document_dirty: false,
                dirty: true,
            },
            appliance_auto_login_pending: auto_login_requested(),
            suspend_resume_smoke_pending: suspend_resume_smoke_requested(),
            desktop_ready_published: false,
            app_basket_committed: false,
            last_tick_ns: get_time_ns(),
        };
        session.shell.dialog.visible = false;
        session.shell.context_menu.visible = false;
        session.shell.switcher.visible = false;
        session.shell.notifications.visible = false;
        session.shell.quick_settings.visible = false;
        session.shell.command_palette.visible = false;
        session.shell.stage_rail.visible = false;
        session.shell.lock_screen.visible = true;
        for window in [
            &mut session.shell.notifications,
            &mut session.shell.dialog,
            &mut session.shell.context_menu,
            &mut session.shell.switcher,
            &mut session.shell.quick_settings,
            &mut session.shell.command_palette,
            &mut session.shell.stage_rail,
        ] {
            window.opacity = 0.0;
            window.desired_visible = false;
            window.fading_out = false;
        }
        debug_marker(b"N40\n");
        serial_println!("[DESKTOP] session bootstrap step=login-visible");
        session.set_login_visibility(true);
        let _ = session
            .shell
            .client
            .focus_window(session.shell.lock_screen.window_id);
        session.push_notice(String::from("Login required: password echos"));
        debug_marker(b"N50\n");
        serial_println!("[DESKTOP] session bootstrap step=render-shell");
        session.render_shell()?;
        debug_marker(b"N60\n");
        serial_println!("[DESKTOP] session bootstrap step=render-apps");
        session.render_apps()?;
        debug_marker(b"N70\n");
        serial_println!("[DESKTOP] session bootstrap step=ready");
        if session.appliance_auto_login_pending {
            serial_println!("[DESKTOP] appliance auto-login armed");
        }
        Ok(session)
    }

    fn tick(&mut self) {
        let now = get_time_ns();
        let dt =
            ((now.saturating_sub(self.last_tick_ns)).min(100_000_000) as f64) / 1_000_000_000.0;
        self.last_tick_ns = now;
        if self.settings.animations && update_animations(dt.max(1.0 / 240.0)) {
            self.refresh_shell_surface_animations();
        } else {
            self.refresh_shell_surface_animations();
        }
        self.sync_window_states();
        self.rebuild_stage_sets();
        self.service_dialog_queue();
        self.handle_shell_shortcuts();
        self.restore_faulted_apps();
        self.run_appliance_auto_login();
        self.enforce_desktop_visibility_contract();

        let mut commands = Vec::new();
        let batch = self.poll_unified_event_loop();
        self.drive_unified_event_loop(batch, &mut commands);

        for command in commands {
            self.apply_command(command);
        }
        self.enforce_desktop_visibility_contract();

        self.publish_accessibility();
        let _ = self.render_shell();
        let _ = self.render_apps();
        self.evaluate_boot_readiness();

        // Flush cursor position and all committed surface damage to the framebuffer
        // synchronously on every tick.
        //
        // Root-cause: EchDisplay::run_service() is spawned at Priority::Low but the
        // VelvetGlove main loop never calls schedule() or preemptible_schedule(), so
        // the service task never receives a CPU time-slice. Without this call, damage
        // marked by update_cursor() (driven by mouse IRQ12 → drain_raw_input →
        // PointerMove → dispatch_input_event) accumulates in the DamageTracker but is
        // never consumed; the cursor stays frozen at its initial (24, 24) position for
        // the entire session. present() exits immediately when there is no damage, so
        // the overhead on quiet frames is a single Mutex::lock + is_empty() check.
        let _ = self.shell.client.present_frame();
    }

    fn refresh_shell_surface_animations(&mut self) {
        if refresh_shell_surface_animation(&self.shell.client, &mut self.shell.quick_settings) {
            self.invalidate_shell(
                InvalidationTarget::QuickSettings,
                InvalidationReason::AnimationAdvanced,
            );
        }
        if refresh_shell_surface_animation(&self.shell.client, &mut self.shell.command_palette) {
            self.invalidate_shell(
                InvalidationTarget::CommandPalette,
                InvalidationReason::AnimationAdvanced,
            );
        }
        if refresh_shell_surface_animation(&self.shell.client, &mut self.shell.stage_rail) {
            self.invalidate_shell(
                InvalidationTarget::Overview,
                InvalidationReason::AnimationAdvanced,
            );
        }
        if refresh_shell_surface_animation(&self.shell.client, &mut self.shell.dialog) {
            self.invalidate_shell(
                InvalidationTarget::Dialog,
                InvalidationReason::AnimationAdvanced,
            );
        }
        if refresh_shell_surface_animation(&self.shell.client, &mut self.shell.context_menu) {
            self.invalidate_shell(
                InvalidationTarget::ContextMenu,
                InvalidationReason::AnimationAdvanced,
            );
        }
        if refresh_shell_surface_animation(&self.shell.client, &mut self.shell.switcher) {
            self.invalidate_shell(
                InvalidationTarget::Switcher,
                InvalidationReason::AnimationAdvanced,
            );
        }
    }

    fn handle_shell_events(
        &mut self,
        events: Vec<WindowInputEvent>,
        commands: &mut Vec<SessionCommand>,
    ) {
        for event in events {
            let input = &event.event;

            if event.window_id == self.shell.lock_screen.window_id {
                if is_enter_key(input) {
                    self.attempt_login();
                } else if is_backspace_key(input) {
                    self.shell.auth_input.pop();
                    self.invalidate_shell(
                        InvalidationTarget::LockScreen,
                        InvalidationReason::StateChanged,
                    );
                } else if let Some(ch) = printable_key(input) {
                    self.shell.auth_input.push(ch);
                    self.invalidate_shell(
                        InvalidationTarget::LockScreen,
                        InvalidationReason::StateChanged,
                    );
                }
                continue;
            }

            if self.is_locked() {
                continue;
            }

            if event.window_id == self.shell.desktop.window_id {
                match input {
                    InputEvent::PointerButton {
                        button: PointerButton::Left,
                        state: KeyState::Pressed,
                        ..
                    } => {
                        let now = get_time_ns();
                        if let Some(local) = event.local_position {
                            if let Some(shortcut) = desktop_shortcut_hit(
                                local,
                                self.shell.desktop.content_rect.width as usize,
                                self.shell.desktop.content_rect.height as usize,
                            ) {
                                self.select_desktop_shortcut(Some(shortcut));
                                self.shell.last_shortcut_click = Some((shortcut, now));
                                self.activate_desktop_shortcut(shortcut, commands);
                            } else {
                                self.select_desktop_shortcut(None);
                                self.shell.last_shortcut_click = None;
                            }
                        }
                    }
                    InputEvent::PointerButton {
                        button: PointerButton::Right,
                        state: KeyState::Pressed,
                        ..
                    } => {
                        if let Some(local) = event.local_position {
                            if let Some(shortcut) = desktop_shortcut_hit(
                                local,
                                self.shell.desktop.content_rect.width as usize,
                                self.shell.desktop.content_rect.height as usize,
                            ) {
                                self.select_desktop_shortcut(Some(shortcut));
                                self.shell.last_shortcut_click = Some((shortcut, get_time_ns()));
                                self.open_desktop_shortcut_context(shortcut, commands);
                            } else {
                                self.select_desktop_shortcut(None);
                            }
                        }
                    }
                    _ => {}
                }

                if is_enter_key(input) {
                    if let Some(shortcut) = self.shell.selected_shortcut {
                        self.activate_desktop_shortcut(shortcut, commands);
                    }
                } else if is_escape_key(input) {
                    self.select_desktop_shortcut(None);
                }
            }

            if event.window_id == self.shell.top_bar.window_id {
                match input {
                    InputEvent::PointerButton {
                        button: PointerButton::Left,
                        state: KeyState::Pressed,
                        ..
                    } => {
                        if let Some(local) = event.local_position {
                            let top_bar_width = self.shell.top_bar.content_rect.width as i32;
                            if top_bar_apps_hit(local, top_bar_width) {
                                let _ = self.open_files_path("/programs");
                            } else if let Some(workspace_id) = top_bar_workspace_hit(
                                local,
                                top_bar_width,
                                self.shell.active_workspace,
                            ) {
                                commands.push(SessionCommand::SwitchWorkspace(workspace_id));
                            } else if top_bar_command_hit(local, top_bar_width) {
                                let _ = self.activate_app(AppKind::Terminal);
                            } else if let Some(kind) = top_bar_status_hit(local, top_bar_width) {
                                match kind {
                                    TopBarStatusKind::Cpu => self.push_notice(String::from(
                                        "CPU telemetry panel is not wired yet; use Settings from the dock",
                                    )),
                                    TopBarStatusKind::Power => self.push_notice(String::from(
                                        "Power controls are temporarily routed off the top bar to avoid lock-screen misfires",
                                    )),
                                    TopBarStatusKind::Net => self.push_notice(String::from(
                                        "Network status is live; controls will return after the top-bar panel path is hardened",
                                    )),
                                    TopBarStatusKind::Vpn => self.push_notice(String::from(
                                        "VPN status is live; controls will return after the top-bar panel path is hardened",
                                    )),
                                    TopBarStatusKind::Aud => self.push_notice(String::from(
                                        "Audio status is live; controls will return after the top-bar panel path is hardened",
                                    )),
                                }
                            } else if top_bar_time_hit(local, top_bar_width) {
                                self.push_notice(String::from(
                                    "Clock and session controls are temporarily top-bar read-only",
                                ));
                            }
                        }
                    }
                    InputEvent::PointerButton {
                        button: PointerButton::Right,
                        state: KeyState::Pressed,
                        ..
                    } => self.push_notice(String::from(
                        "Top bar right click reserved; use Alt+Tab or the dock",
                    )),
                    _ => {}
                }

                if matches!(digit_key_pressed(input), Some(1)) {
                    commands.push(SessionCommand::SwitchWorkspace(0));
                } else if matches!(digit_key_pressed(input), Some(2)) {
                    commands.push(SessionCommand::SwitchWorkspace(1));
                } else if matches!(digit_key_pressed(input), Some(3)) {
                    commands.push(SessionCommand::SwitchWorkspace(2));
                } else if key_scan_pressed(input, 0x0F) {
                    self.cycle_switcher();
                } else if key_scan_pressed(input, 0x26) {
                    self.toggle_power_state();
                } else if key_scan_pressed(input, 0x19) {
                    if let Ok(entry) = self.shell.client.capture_screen("top-bar") {
                        self.push_notice(format!("Captured screen {}", entry.id));
                    }
                }
            }

            if event.window_id == self.shell.command_palette.window_id {
                match input {
                    InputEvent::PointerButton {
                        button: PointerButton::Left,
                        state: KeyState::Pressed,
                        ..
                    } => {
                        if let Some(local) = event.local_position {
                            let action_count = self.filtered_palette_actions().len();
                            if let Some(index) = command_palette_hit(
                                local,
                                self.shell.command_palette.content_rect.width as usize,
                                action_count,
                            ) {
                                self.shell.command_selection = index;
                                self.execute_command_palette_selection(commands);
                            }
                        }
                    }
                    _ => {}
                }

                if is_escape_key(input) {
                    self.close_command_palette();
                } else if is_enter_key(input) {
                    self.execute_command_palette_selection(commands);
                } else if is_backspace_key(input) {
                    self.shell.command_query.pop();
                    self.shell.command_selection = 0;
                    self.invalidate_shell(
                        InvalidationTarget::CommandPalette,
                        InvalidationReason::StateChanged,
                    );
                } else if key_scan_pressed(input, 0x24) {
                    self.shell.command_selection = self.shell.command_selection.saturating_add(1);
                    self.invalidate_shell(
                        InvalidationTarget::CommandPalette,
                        InvalidationReason::StateChanged,
                    );
                } else if key_scan_pressed(input, 0x25) {
                    self.shell.command_selection = self.shell.command_selection.saturating_sub(1);
                    self.invalidate_shell(
                        InvalidationTarget::CommandPalette,
                        InvalidationReason::StateChanged,
                    );
                } else if let Some(ch) = printable_key(input) {
                    if self.shell.command_query.len() < 48 {
                        self.shell.command_query.push(ch);
                    }
                    self.shell.command_selection = 0;
                    self.invalidate_shell(
                        InvalidationTarget::CommandPalette,
                        InvalidationReason::StateChanged,
                    );
                }
            }

            if event.window_id == self.shell.quick_settings.window_id {
                match input {
                    InputEvent::PointerButton {
                        button: PointerButton::Left,
                        state: KeyState::Pressed,
                        ..
                    } => {
                        if let Some(local) = event.local_position {
                            if let Some(index) = quick_settings_hit(
                                local,
                                self.shell.quick_settings.content_rect.width as usize,
                                5,
                            ) {
                                self.apply_quick_settings_toggle(index, commands);
                            }
                        }
                    }
                    _ => {}
                }

                match digit_key_pressed(input) {
                    Some(1) => self.apply_quick_settings_toggle(0, commands),
                    Some(2) => self.apply_quick_settings_toggle(1, commands),
                    Some(3) => self.apply_quick_settings_toggle(2, commands),
                    Some(4) => self.apply_quick_settings_toggle(3, commands),
                    Some(5) => self.apply_quick_settings_toggle(4, commands),
                    _ if is_escape_key(input) => self.toggle_quick_settings(),
                    _ => {}
                }
            }

            if event.window_id == self.shell.stage_rail.window_id {
                match input {
                    InputEvent::PointerButton {
                        button: PointerButton::Left,
                        state: KeyState::Pressed,
                        ..
                    } => {
                        if let Some(local) = event.local_position {
                            if let Some(index) = stage_rail_hit(
                                local,
                                self.shell.stage_rail.content_rect.width as usize,
                                self.shell.stage_sets.len(),
                            ) {
                                self.activate_stage_set(index);
                            }
                        }
                    }
                    _ => {}
                }

                if let Some(index) = digit_key_pressed(input) {
                    if index > 0 {
                        self.activate_stage_set(index.saturating_sub(1) as usize);
                    }
                }
            }

            if event.window_id == self.shell.switcher.window_id {
                match input {
                    InputEvent::PointerButton {
                        state: KeyState::Pressed,
                        ..
                    } => {
                        if let Some(local) = event.local_position {
                            if let Some(index) = switcher_hit(local) {
                                self.shell.switcher_index = index;
                                self.activate_switcher_selection();
                            }
                        }
                    }
                    _ => {}
                }

                if key_scan_pressed(input, 0x0F) {
                    self.cycle_switcher();
                } else if is_enter_key(input) {
                    self.activate_switcher_selection();
                } else if is_escape_key(input) {
                    self.close_switcher();
                }
            }

            if event.window_id == self.shell.context_menu.window_id {
                match input {
                    InputEvent::PointerButton {
                        state: KeyState::Pressed,
                        ..
                    } => {
                        if let Some(local) = event.local_position {
                            match context_menu_hit(local) {
                                Some(action) => self.apply_context_action(action),
                                None => self.close_context_menu(),
                            }
                        }
                    }
                    _ => {}
                }

                match digit_key_pressed(input) {
                    Some(1) => self.apply_context_action(ContextAction::Focus),
                    Some(2) => self.apply_context_action(ContextAction::Minimize),
                    Some(3) => self.apply_context_action(ContextAction::SnapLeft),
                    Some(4) => self.apply_context_action(ContextAction::SnapRight),
                    Some(5) => self.apply_context_action(ContextAction::Maximize),
                    Some(6) => self.apply_context_action(ContextAction::MoveNextWorkspace),
                    Some(7) => self.apply_context_action(ContextAction::Close),
                    _ if is_escape_key(input) => self.close_context_menu(),
                    _ => {}
                }
            }

            if event.window_id == self.shell.task_strip.window_id {
                match input {
                    InputEvent::PointerButton {
                        button,
                        state: KeyState::Pressed,
                        ..
                    } => {
                        if let Some(local) = event.local_position {
                            if let Some(kind) = task_strip_app_hit(
                                local,
                                self.shell.task_strip.content_rect.width as usize,
                            ) {
                                match button {
                                    PointerButton::Right => self.open_context_menu(kind),
                                    _ => commands.push(SessionCommand::Launch(
                                        self.launch_intent_for_app(kind, LaunchSource::TaskStrip),
                                    )),
                                }
                            }
                        }
                    }
                    _ => {}
                }

                if let Some(kind) = app_shortcut_from_input(input) {
                    commands.push(SessionCommand::Launch(
                        self.launch_intent_for_app(kind, LaunchSource::TaskStrip),
                    ));
                } else {
                    match digit_key_pressed(input) {
                        _ if key_scan_pressed(input, 0x0F) => self.cycle_switcher(),
                        _ => {}
                    }
                }
            }

            if event.window_id == self.shell.launcher.window_id {
                match input {
                    InputEvent::PointerButton {
                        button,
                        state: KeyState::Pressed,
                        ..
                    } => {
                        if let Some(local) = event.local_position {
                            if let Some(kind) = launcher_hit(local) {
                                match button {
                                    PointerButton::Right => self.open_context_menu(kind),
                                    _ => commands.push(SessionCommand::Launch(
                                        self.launch_intent_for_app(kind, LaunchSource::Launcher),
                                    )),
                                }
                            }
                        }
                    }
                    _ => {}
                }

                if let Some(kind) = app_shortcut_from_input(input) {
                    commands.push(SessionCommand::Launch(
                        self.launch_intent_for_app(kind, LaunchSource::Launcher),
                    ));
                } else if key_scan_pressed(input, 0x0F) {
                    self.cycle_switcher();
                }
            }

            if event.window_id == self.shell.notifications.window_id {
                match input {
                    InputEvent::PointerButton {
                        button: PointerButton::Left,
                        state: KeyState::Pressed,
                        ..
                    } => {
                        if let Some(local) = event.local_position {
                            if let Ok(entries) = self.shell.client.list_notifications(6) {
                                let display_entries: Vec<_> = entries.into_iter().rev().collect();
                                if let Some(index) = notification_hit(local) {
                                    if let Some(entry) = display_entries.get(index) {
                                        let _ = self.shell.client.mark_notification_read(entry.id);
                                        if entry.action_label.is_some() {
                                            if let Some(kind) = app_kind_from_id(entry.app_id) {
                                                commands.push(SessionCommand::Launch(
                                                    self.launch_intent_for_app(
                                                        kind,
                                                        LaunchSource::Notification,
                                                    ),
                                                ));
                                            }
                                        }
                                        self.push_notice(format!(
                                            "Notification {} acknowledged",
                                            entry.id
                                        ));
                                        self.invalidate_shell(
                                            InvalidationTarget::NotificationCenter,
                                            InvalidationReason::StateChanged,
                                        );
                                        self.invalidate_shell(
                                            InvalidationTarget::TopBar,
                                            InvalidationReason::StateChanged,
                                        );
                                        self.invalidate_shell(
                                            InvalidationTarget::Dock,
                                            InvalidationReason::StateChanged,
                                        );
                                        self.invalidate_shell(
                                            InvalidationTarget::Launcher,
                                            InvalidationReason::StateChanged,
                                        );
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }

                if key_scan_pressed(input, 0x24) {
                    self.shell.notification_index = self.shell.notification_index.saturating_add(1);
                    self.invalidate_shell(
                        InvalidationTarget::NotificationCenter,
                        InvalidationReason::StateChanged,
                    );
                } else if key_scan_pressed(input, 0x25) {
                    self.shell.notification_index = self.shell.notification_index.saturating_sub(1);
                    self.invalidate_shell(
                        InvalidationTarget::NotificationCenter,
                        InvalidationReason::StateChanged,
                    );
                } else if key_scan_pressed(input, 0x18) {
                    if let Ok(entries) = self.shell.client.list_notifications(6) {
                        let display_entries: Vec<_> = entries.into_iter().rev().collect();
                        if let Some(entry) = display_entries.get(
                            self.shell
                                .notification_index
                                .min(display_entries.len().saturating_sub(1)),
                        ) {
                            let _ = self.shell.client.mark_notification_read(entry.id);
                            if entry.action_label.is_some() {
                                if let Some(kind) = app_kind_from_id(entry.app_id) {
                                    commands.push(SessionCommand::Launch(
                                        self.launch_intent_for_app(
                                            kind,
                                            LaunchSource::Notification,
                                        ),
                                    ));
                                }
                            }
                            self.invalidate_shell(
                                InvalidationTarget::NotificationCenter,
                                InvalidationReason::StateChanged,
                            );
                        }
                    }
                } else if key_scan_pressed(input, 0x2E) {
                    self.shell.notices.clear();
                    let _ = self.shell.client.clear_notifications();
                    self.push_notice(String::from("Notifications cleared"));
                }
            }

            if event.window_id == self.shell.dialog.window_id {
                match input {
                    InputEvent::PointerButton {
                        state: KeyState::Pressed,
                        ..
                    } => {
                        if let Some(local) = event.local_position {
                            match dialog_button_hit(local) {
                                Some(true) => self.resolve_pending_dialog(true),
                                Some(false) => self.resolve_pending_dialog(false),
                                None => {}
                            }
                        }
                    }
                    _ => {}
                }

                if is_enter_key(input) {
                    self.resolve_pending_dialog(true);
                } else if is_backspace_key(input) {
                    self.shell.dialog_input.pop();
                    self.invalidate_shell(
                        InvalidationTarget::Dialog,
                        InvalidationReason::StateChanged,
                    );
                } else if is_escape_key(input) {
                    self.resolve_pending_dialog(false);
                } else if let Some(ch) = printable_key(input) {
                    self.shell.dialog_input.push(ch);
                    self.invalidate_shell(
                        InvalidationTarget::Dialog,
                        InvalidationReason::StateChanged,
                    );
                }
            }
        }
    }

    fn apply_command(&mut self, command: SessionCommand) {
        match command {
            SessionCommand::Launch(intent) => {
                if let Err(err) = self.dispatch_launch_intent(intent) {
                    self.push_notice(format!(
                        "{} launch failed: {}",
                        intent.descriptor.title, err
                    ));
                }
            }
            SessionCommand::LaunchExternal(intent, path) => {
                if let Err(err) = self.dispatch_external_launch_intent(intent, &path) {
                    self.push_notice(format!(
                        "{} launch failed: {}",
                        intent.descriptor.title, err
                    ));
                }
            }
            SessionCommand::SwitchWorkspace(workspace_id) => {
                if let Err(err) = self.switch_workspace(workspace_id) {
                    self.push_notice(format!("workspace switch failed: {}", err));
                }
            }
            SessionCommand::Notify(message) => self.push_notice(message),
            SessionCommand::OpenEditorPath(path) => {
                let intent =
                    self.launch_intent_for_app(AppKind::Editor, LaunchSource::DocumentOpen);
                if let Err(err) = self.dispatch_launch_intent(intent) {
                    self.push_notice(format!("Editor launch failed: {}", err));
                    return;
                }
                if let Err(err) = self.editor.open_document(&path) {
                    self.push_notice(format!("Editor open failed: {}", err));
                } else {
                    self.push_notice(format!("Opened {}", path));
                }
            }
        }
    }

    fn handle_shell_shortcuts(&mut self) {
        let Ok(shortcuts) = self.shell.client.poll_shortcuts(16) else {
            return;
        };

        for shortcut in shortcuts {
            match shortcut {
                ShellShortcut::AppSwitcherNext => self.cycle_switcher(),
                ShellShortcut::AppSwitcherConfirm => self.activate_switcher_selection(),
                ShellShortcut::AppSwitcherCancel => self.close_switcher(),
                ShellShortcut::Workspace(workspace_id) => {
                    let _ = self.switch_workspace(workspace_id);
                }
                ShellShortcut::ToggleCommandPalette => self.toggle_command_palette(),
                ShellShortcut::ToggleQuickSettings => self.toggle_quick_settings(),
                ShellShortcut::ToggleOverview => self.toggle_overview(),
                ShellShortcut::ToggleScratchpad => self.toggle_terminal_scratchpad(),
                ShellShortcut::LaunchTerminal => {
                    let intent =
                        self.launch_intent_for_app(AppKind::Terminal, LaunchSource::ShellShortcut);
                    let _ = self.dispatch_launch_intent(intent);
                }
            }
        }
    }

    fn set_login_visibility(&mut self, visible: bool) {
        let stage_rail_visible = false;
        set_shell_surface_visibility(&self.shell.client, &mut self.shell.lock_screen, visible);
        self.invalidate_shell(
            InvalidationTarget::LockScreen,
            InvalidationReason::StateChanged,
        );

        set_shell_surface_visibility(&self.shell.client, &mut self.shell.top_bar, !visible);
        set_shell_surface_visibility(&self.shell.client, &mut self.shell.task_strip, !visible);
        set_shell_surface_visibility(&self.shell.client, &mut self.shell.launcher, false);
        set_shell_surface_visibility(&self.shell.client, &mut self.shell.notifications, false);
        set_shell_surface_visibility(&self.shell.client, &mut self.shell.quick_settings, false);
        set_shell_surface_visibility(&self.shell.client, &mut self.shell.command_palette, false);
        set_shell_surface_visibility(
            &self.shell.client,
            &mut self.shell.stage_rail,
            stage_rail_visible,
        );
        set_shell_surface_visibility(&self.shell.client, &mut self.shell.dialog, false);
        set_shell_surface_visibility(&self.shell.client, &mut self.shell.context_menu, false);
        set_shell_surface_visibility(&self.shell.client, &mut self.shell.switcher, false);

        self.invalidate_shell(
            InvalidationTarget::QuickSettings,
            InvalidationReason::StateChanged,
        );
        self.invalidate_shell(
            InvalidationTarget::CommandPalette,
            InvalidationReason::StateChanged,
        );
        self.invalidate_shell(
            InvalidationTarget::Overview,
            InvalidationReason::StateChanged,
        );

        for window in [
            self.terminal.window.as_ref(),
            self.files.window.as_ref(),
            self.browser.window.as_ref(),
            self.settings.window.as_ref(),
            self.editor.window.as_ref(),
        ] {
            if let Some(window) = window {
                let _ = self.shell.client.set_visibility(window.window_id, !visible);
            }
        }
    }

    fn attempt_login(&mut self) {
        if USER_DB
            .login("operator", self.shell.auth_input.trim())
            .is_ok()
        {
            self.shell.auth_input.clear();
            self.unlock_session();
        } else {
            self.shell.auth_input.clear();
            self.push_notice(String::from("Authentication failed"));
            self.invalidate_shell(
                InvalidationTarget::LockScreen,
                InvalidationReason::StateChanged,
            );
        }
    }

    fn restore_faulted_apps(&mut self) {
        if self.is_locked() {
            return;
        }
        let Ok(entries) = self.shell.client.list_shell_apps() else {
            return;
        };
        for entry in entries {
            if entry.health == AppHealth::Crashed && entry.auto_restore {
                if let Some(kind) = app_kind_from_id(entry.app_id) {
                    let intent = self.launch_intent_for_app(kind, LaunchSource::FaultRecovery);
                    if self.dispatch_launch_intent(intent).is_ok() {
                        let _ = self
                            .shell
                            .client
                            .clear_app_attention(Some("auto-restored after fault"));
                        self.push_notice(format!("{} auto-restored", kind.title()));
                    }
                }
            }
        }
    }

    fn publish_accessibility(&mut self) {
        let mut nodes = Vec::new();
        nodes.push(AccessibilityNode {
            id: 1,
            app_id: SHELL_APP_ID,
            role: AccessibilityRole::Toolbar,
            label: String::from("Top Bar"),
            description: String::from("workspace and power controls"),
            focused: self.shell.top_bar.focused,
            bounds: self.shell.top_bar.content_rect,
        });
        nodes.push(AccessibilityNode {
            id: 2,
            app_id: SHELL_APP_ID,
            role: AccessibilityRole::List,
            label: String::from("Task Strip"),
            description: String::from("running applications and spaces"),
            focused: self.shell.task_strip.focused,
            bounds: self.shell.task_strip.content_rect,
        });
        nodes.push(AccessibilityNode {
            id: 3,
            app_id: SHELL_APP_ID,
            role: AccessibilityRole::Window,
            label: String::from("Session Shell"),
            description: String::from("launcher and shell notices"),
            focused: self.shell.launcher.focused,
            bounds: self.shell.launcher.content_rect,
        });
        nodes.push(AccessibilityNode {
            id: 4,
            app_id: SHELL_APP_ID,
            role: AccessibilityRole::List,
            label: String::from("Workspace Overview"),
            description: String::from("workspace switcher and overview"),
            focused: self.shell.stage_rail.focused,
            bounds: self.shell.stage_rail.content_rect,
        });
        if self.shell.quick_settings.visible {
            nodes.push(AccessibilityNode {
                id: 5,
                app_id: SHELL_APP_ID,
                role: AccessibilityRole::Dialog,
                label: String::from("Quick Settings"),
                description: String::from("session toggles"),
                focused: self.shell.quick_settings.focused,
                bounds: self.shell.quick_settings.content_rect,
            });
        }
        if self.shell.command_palette.visible {
            nodes.push(AccessibilityNode {
                id: 6,
                app_id: SHELL_APP_ID,
                role: AccessibilityRole::Dialog,
                label: String::from("Command Palette"),
                description: String::from("global shell command graph"),
                focused: self.shell.command_palette.focused,
                bounds: self.shell.command_palette.content_rect,
            });
        }
        if self.shell.lock_screen.visible {
            nodes.push(AccessibilityNode {
                id: 7,
                app_id: SHELL_APP_ID,
                role: AccessibilityRole::Dialog,
                label: String::from("Login"),
                description: String::from("authenticate to unlock desktop"),
                focused: true,
                bounds: self.shell.lock_screen.content_rect,
            });
        }
        let _ = self.shell.client.publish_accessibility_tree(nodes);
        self.terminal.publish_accessibility();
        self.files.publish_accessibility();
        self.browser.publish_accessibility();
        self.settings.publish_accessibility();
        self.editor.publish_accessibility();
    }

    fn push_notice(&mut self, notice: String) {
        if self.settings.notifications {
            let _ = self
                .shell
                .client
                .notify("Session", &notice, NotificationLevel::Info);
        }
        self.shell.notices.push(notice);
        while self.shell.notices.len() > 8 {
            self.shell.notices.remove(0);
        }
        self.invalidate_shell(
            InvalidationTarget::NotificationCenter,
            InvalidationReason::StateChanged,
        );
        self.invalidate_shell(
            InvalidationTarget::Launcher,
            InvalidationReason::StateChanged,
        );
        self.invalidate_shell(InvalidationTarget::TopBar, InvalidationReason::StateChanged);
        self.invalidate_shell(InvalidationTarget::Dock, InvalidationReason::StateChanged);
    }

    fn select_desktop_shortcut(&mut self, selection: Option<DesktopShortcutKind>) {
        if self.shell.selected_shortcut != selection {
            self.shell.selected_shortcut = selection;
            self.invalidate_shell(
                InvalidationTarget::Wallpaper,
                InvalidationReason::FocusChanged,
            );
        }
    }

    fn activate_desktop_shortcut(
        &mut self,
        shortcut: DesktopShortcutKind,
        commands: &mut Vec<SessionCommand>,
    ) {
        commands.push(SessionCommand::Launch(
            self.launch_intent_for_shortcut(shortcut, LaunchSource::DesktopShortcut),
        ));
    }

    fn open_desktop_shortcut_context(
        &mut self,
        shortcut: DesktopShortcutKind,
        commands: &mut Vec<SessionCommand>,
    ) {
        if let Some(kind) = shortcut.app_kind() {
            self.open_context_menu(kind);
            return;
        }

        match shortcut {
            DesktopShortcutKind::Web => {
                commands.push(SessionCommand::Launch(
                    self.launch_intent_for_shortcut(shortcut, LaunchSource::ContextMenu),
                ));
            }
            DesktopShortcutKind::RecycleBin => {
                commands.push(SessionCommand::Launch(
                    self.launch_intent_for_shortcut(shortcut, LaunchSource::ContextMenu),
                ));
            }
            DesktopShortcutKind::Terminal
            | DesktopShortcutKind::Files
            | DesktopShortcutKind::Settings => {
                self.activate_desktop_shortcut(shortcut, commands);
            }
        }
    }

    fn mark_shell_dirty(&mut self) {
        self.invalidate_shell(
            InvalidationTarget::Wallpaper,
            InvalidationReason::StateChanged,
        );
        self.invalidate_shell(InvalidationTarget::TopBar, InvalidationReason::StateChanged);
        self.invalidate_shell(InvalidationTarget::Dock, InvalidationReason::StateChanged);
        self.invalidate_shell(
            InvalidationTarget::Launcher,
            InvalidationReason::StateChanged,
        );
        self.invalidate_shell(
            InvalidationTarget::NotificationCenter,
            InvalidationReason::StateChanged,
        );
        self.invalidate_shell(
            InvalidationTarget::QuickSettings,
            InvalidationReason::StateChanged,
        );
        self.invalidate_shell(
            InvalidationTarget::CommandPalette,
            InvalidationReason::StateChanged,
        );
        self.invalidate_shell(
            InvalidationTarget::Overview,
            InvalidationReason::StateChanged,
        );
        self.invalidate_shell(InvalidationTarget::Dialog, InvalidationReason::StateChanged);
        self.invalidate_shell(
            InvalidationTarget::ContextMenu,
            InvalidationReason::StateChanged,
        );
        self.invalidate_shell(
            InvalidationTarget::Switcher,
            InvalidationReason::StateChanged,
        );
    }

    fn switch_workspace(&mut self, workspace_id: u8) -> Result<(), String> {
        self.close_context_menu();
        self.close_switcher();
        self.close_command_palette();
        self.shell.active_workspace = workspace_id.min(WORKSPACE_COUNT.saturating_sub(1));
        self.shell
            .client
            .set_workspace(self.shell.active_workspace)?;
        if let Some(index) = self
            .shell
            .stage_sets
            .iter()
            .position(|set| set.id as u8 == self.shell.active_workspace)
        {
            self.shell.active_stage_set = index;
        }

        apply_workspace_visibility(
            &self.terminal.client,
            &mut self.terminal.window,
            self.terminal.workspace_id,
            self.shell.active_workspace,
        )?;
        apply_workspace_visibility(
            &self.files.client,
            &mut self.files.window,
            self.files.workspace_id,
            self.shell.active_workspace,
        )?;
        apply_workspace_visibility(
            &self.browser.client,
            &mut self.browser.window,
            self.browser.workspace_id,
            self.shell.active_workspace,
        )?;
        apply_workspace_visibility(
            &self.settings.client,
            &mut self.settings.window,
            self.settings.workspace_id,
            self.shell.active_workspace,
        )?;
        apply_workspace_visibility(
            &self.editor.client,
            &mut self.editor.window,
            self.editor.workspace_id,
            self.shell.active_workspace,
        )?;
        let _ = self.relayout_active_workspace();
        self.rebuild_stage_sets();

        let _ = self
            .shell
            .client
            .focus_window(self.shell.launcher.window_id);
        self.push_notice(format!(
            "Workspace {} active",
            self.shell.active_workspace.saturating_add(1)
        ));
        self.mark_shell_dirty();
        Ok(())
    }

    fn app_workspace(&self, kind: AppKind) -> u8 {
        match kind {
            AppKind::Terminal => self.terminal.workspace_id,
            AppKind::Files => self.files.workspace_id,
            AppKind::Browser => self.browser.workspace_id,
            AppKind::Settings => self.settings.workspace_id,
            AppKind::Editor => self.editor.workspace_id,
        }
    }

    fn app_has_window(&self, kind: AppKind) -> bool {
        match kind {
            AppKind::Terminal => self.terminal.window.is_some(),
            AppKind::Files => self.files.window.is_some(),
            AppKind::Browser => self.browser.window.is_some(),
            AppKind::Settings => self.settings.window.is_some(),
            AppKind::Editor => self.editor.window.is_some(),
        }
    }

    fn cycle_switcher(&mut self) {
        self.close_context_menu();
        self.close_command_palette();
        let candidates = self.switcher_candidates();
        if candidates.is_empty() {
            return;
        }

        if !self.shell.switcher.desired_visible {
            self.shell.switcher_index = 0;
            animate_shell_surface(
                &self.shell.client,
                &mut self.shell.switcher,
                true,
                self.settings.animations,
                0.14,
            );
        } else {
            self.shell.switcher_index = (self.shell.switcher_index + 1) % candidates.len();
        }

        let _ = self
            .shell
            .client
            .focus_window(self.shell.switcher.window_id);
        self.invalidate_shell(
            InvalidationTarget::Switcher,
            InvalidationReason::StateChanged,
        );
    }

    fn close_switcher(&mut self) {
        if self.shell.switcher.visible || self.shell.switcher.desired_visible {
            animate_shell_surface(
                &self.shell.client,
                &mut self.shell.switcher,
                false,
                self.settings.animations,
                0.12,
            );
            self.invalidate_shell(
                InvalidationTarget::Switcher,
                InvalidationReason::StateChanged,
            );
        }
    }

    fn activate_switcher_selection(&mut self) {
        let candidates = self.switcher_candidates();
        if candidates.is_empty() {
            self.close_switcher();
            return;
        }
        let selected = candidates[self.shell.switcher_index.min(candidates.len() - 1)];
        self.close_switcher();
        let intent = self.launch_intent_for_app(selected, LaunchSource::ShellShortcut);
        let _ = self.dispatch_launch_intent(intent);
    }

    fn toggle_command_palette(&mut self) {
        if self.is_locked() {
            return;
        }
        self.close_switcher();
        self.close_context_menu();
        if self.shell.command_palette.desired_visible {
            self.close_command_palette();
            return;
        }
        animate_shell_surface(
            &self.shell.client,
            &mut self.shell.command_palette,
            true,
            self.settings.animations,
            0.16,
        );
        let _ = self
            .shell
            .client
            .focus_window(self.shell.command_palette.window_id);
        self.shell.command_selection = 0;
        self.invalidate_shell(
            InvalidationTarget::CommandPalette,
            InvalidationReason::StateChanged,
        );
    }

    fn close_command_palette(&mut self) {
        if self.shell.command_palette.visible || self.shell.command_palette.desired_visible {
            animate_shell_surface(
                &self.shell.client,
                &mut self.shell.command_palette,
                false,
                self.settings.animations,
                0.12,
            );
            self.shell.command_query.clear();
            self.shell.command_selection = 0;
            self.invalidate_shell(
                InvalidationTarget::CommandPalette,
                InvalidationReason::StateChanged,
            );
        }
    }

    fn toggle_quick_settings(&mut self) {
        if self.is_locked() {
            return;
        }
        self.close_command_palette();
        let next_visible = !self.shell.quick_settings.desired_visible;
        animate_shell_surface(
            &self.shell.client,
            &mut self.shell.quick_settings,
            next_visible,
            self.settings.animations,
            0.14,
        );
        if next_visible {
            let _ = self
                .shell
                .client
                .focus_window(self.shell.quick_settings.window_id);
        }
        self.invalidate_shell(
            InvalidationTarget::QuickSettings,
            InvalidationReason::StateChanged,
        );
    }

    fn toggle_notifications_center(&mut self) {
        if self.is_locked() {
            return;
        }
        self.close_command_palette();
        let next_visible = !self.shell.notifications.desired_visible;
        animate_shell_surface(
            &self.shell.client,
            &mut self.shell.notifications,
            next_visible,
            self.settings.animations,
            0.14,
        );
        if next_visible {
            let _ = self
                .shell
                .client
                .focus_window(self.shell.notifications.window_id);
        }
        self.invalidate_shell(
            InvalidationTarget::NotificationCenter,
            InvalidationReason::StateChanged,
        );
    }

    fn cycle_motion_profile(&mut self) {
        let current = self
            .shell
            .client
            .motion_profile()
            .unwrap_or(MotionProfile::Standard);
        let next = next_motion_profile(current);
        if self.shell.client.set_motion_profile(next).is_ok() {
            self.settings.animations = next != MotionProfile::Reduced;
            self.push_notice(format!("Motion profile {}", motion_profile_label(next)));
            self.settings.dirty = true;
            self.mark_shell_dirty();
        }
    }

    fn cycle_shell_density_profile(&mut self) {
        let current = self
            .shell
            .client
            .shell_density()
            .unwrap_or(ShellDensityProfile::Balanced);
        let next = next_shell_density_profile(current);
        if self.shell.client.set_shell_density(next).is_ok() {
            self.push_notice(format!("Shell density {}", shell_density_label(next)));
            self.settings.dirty = true;
            self.mark_shell_dirty();
        }
    }

    fn cycle_primary_display_scale(&mut self) {
        let Ok(mut profile) = self.shell.client.display_profile() else {
            self.push_notice(String::from("Display profile unavailable"));
            return;
        };
        let capability_scales = profile.capability.supported_scales_100x.clone();
        if let Some(output) = profile
            .outputs
            .iter_mut()
            .find(|output| output.output_id == profile.primary_output)
        {
            output.scale_100x = next_supported_scale(&capability_scales, output.scale_100x);
            output.text_scale_100x = output.scale_100x;
        }
        match self.shell.client.set_display_profile(profile) {
            Ok(updated) => {
                self.push_notice(format!("Display scale {}", display_scale_label(&updated)));
                self.settings.dirty = true;
                self.mark_shell_dirty();
            }
            Err(err) => self.push_notice(format!("Display scale failed: {}", err)),
        }
    }

    fn toggle_screen_reader(&mut self) {
        let mut profile = self
            .shell
            .client
            .accessibility_profile()
            .unwrap_or_default();
        profile.screen_reader = !profile.screen_reader;
        if self.shell.client.set_accessibility_profile(profile).is_ok() {
            self.push_notice(format!(
                "Screen reader {}",
                if profile.screen_reader {
                    "enabled"
                } else {
                    "disabled"
                }
            ));
            self.settings.dirty = true;
            self.mark_shell_dirty();
        }
    }

    fn apply_quick_settings_toggle(&mut self, index: usize, commands: &mut Vec<SessionCommand>) {
        match index {
            0 => {
                self.shell.theme_mode = match self.shell.theme_mode {
                    ThemeMode::Dark => ThemeMode::Light,
                    ThemeMode::Light => ThemeMode::Auto,
                    ThemeMode::Auto => ThemeMode::Dark,
                };
                let resolved = Theme::resolve_mode(self.shell.theme_mode, false);
                let _ = self.shell.client.set_theme_mode(resolved);
                self.push_notice(format!(
                    "Theme set to {}",
                    theme_mode_label(self.shell.theme_mode)
                ));
                self.mark_shell_dirty();
                self.settings.dirty = true;
            }
            1 => {
                self.settings.notifications = !self.settings.notifications;
                self.push_notice(format!(
                    "Notifications {}",
                    if self.settings.notifications {
                        "enabled"
                    } else {
                        "muted"
                    }
                ));
                self.settings.dirty = true;
                self.invalidate_shell(
                    InvalidationTarget::QuickSettings,
                    InvalidationReason::StateChanged,
                );
            }
            2 => self.cycle_motion_profile(),
            3 => self.cycle_primary_display_scale(),
            4 => self.toggle_screen_reader(),
            _ => {}
        }
        let _ = commands;
    }

    fn command_palette_actions(&self) -> Vec<CommandPaletteAction> {
        let mut actions = Vec::new();
        actions.push(CommandPaletteAction {
            id: 1,
            title: String::from("Open Terminal"),
            category: String::from("Apps"),
            shortcut: String::from("1"),
            enabled: true,
        });
        actions.push(CommandPaletteAction {
            id: 2,
            title: String::from("Open Files"),
            category: String::from("Apps"),
            shortcut: String::from("2"),
            enabled: true,
        });
        actions.push(CommandPaletteAction {
            id: 3,
            title: String::from("Open Settings"),
            category: String::from("Apps"),
            shortcut: String::from("3"),
            enabled: true,
        });
        actions.push(CommandPaletteAction {
            id: 4,
            title: String::from("Open Editor"),
            category: String::from("Apps"),
            shortcut: String::from("4"),
            enabled: true,
        });
        actions.push(CommandPaletteAction {
            id: 5,
            title: String::from("Open Web"),
            category: String::from("Apps"),
            shortcut: String::from("5"),
            enabled: true,
        });
        actions.push(CommandPaletteAction {
            id: 6,
            title: String::from("Open Recycle Bin"),
            category: String::from("Apps"),
            shortcut: String::from("6"),
            enabled: true,
        });
        for workspace_id in 0..WORKSPACE_COUNT {
            actions.push(CommandPaletteAction {
                id: 10 + workspace_id as u64,
                title: format!("Switch Workspace {}", workspace_id.saturating_add(1)),
                category: String::from("Workspace"),
                shortcut: format!("Super+{}", workspace_id.saturating_add(1)),
                enabled: true,
            });
        }
        actions.push(CommandPaletteAction {
            id: 20,
            title: String::from("Toggle Theme"),
            category: String::from("Appearance"),
            shortcut: String::from("QS-1"),
            enabled: true,
        });
        actions.push(CommandPaletteAction {
            id: 21,
            title: String::from("Toggle Quick Settings"),
            category: String::from("Shell"),
            shortcut: String::from("Super+,"),
            enabled: true,
        });
        actions.push(CommandPaletteAction {
            id: 22,
            title: String::from("Lock Session"),
            category: String::from("Session"),
            shortcut: String::from("Lock"),
            enabled: true,
        });
        actions.push(CommandPaletteAction {
            id: 23,
            title: String::from("Clear Notifications"),
            category: String::from("Shell"),
            shortcut: String::from("C"),
            enabled: true,
        });
        actions.push(CommandPaletteAction {
            id: 24,
            title: String::from("Capture Screen"),
            category: String::from("Tools"),
            shortcut: String::from("P"),
            enabled: true,
        });
        actions.push(CommandPaletteAction {
            id: 25,
            title: String::from("Toggle Workspace Overview"),
            category: String::from("Workspace"),
            shortcut: String::from("Super+`"),
            enabled: true,
        });
        actions.push(CommandPaletteAction {
            id: 26,
            title: String::from("Toggle Terminal Scratchpad"),
            category: String::from("Workspace"),
            shortcut: String::from("Super+S"),
            enabled: true,
        });
        actions.push(CommandPaletteAction {
            id: 27,
            title: String::from("Cycle Motion Profile"),
            category: String::from("Display"),
            shortcut: String::from("QS-3"),
            enabled: true,
        });
        actions.push(CommandPaletteAction {
            id: 28,
            title: String::from("Cycle Display Scale"),
            category: String::from("Display"),
            shortcut: String::from("QS-4"),
            enabled: true,
        });
        actions.push(CommandPaletteAction {
            id: 29,
            title: String::from("Toggle Screen Reader"),
            category: String::from("Accessibility"),
            shortcut: String::from("QS-5"),
            enabled: true,
        });
        actions.push(CommandPaletteAction {
            id: 30,
            title: String::from("Cycle Shell Density"),
            category: String::from("Appearance"),
            shortcut: String::from("Settings-3"),
            enabled: true,
        });
        actions
    }

    fn filtered_palette_actions(&self) -> Vec<CommandPaletteAction> {
        let query = self.shell.command_query.trim().to_ascii_lowercase();
        if query.is_empty() {
            return self.command_palette_actions();
        }

        let mut actions: Vec<_> = self
            .command_palette_actions()
            .into_iter()
            .filter(|action| {
                action.enabled
                    && (action.title.to_ascii_lowercase().contains(&query)
                        || action.category.to_ascii_lowercase().contains(&query))
            })
            .collect();
        if let Some(action) = self.external_palette_action(self.shell.command_query.trim()) {
            actions.push(action);
        }
        actions
    }

    fn external_palette_action(&self, query: &str) -> Option<CommandPaletteAction> {
        let resolution = self.launch_resolution(query)?;
        let descriptor = resolution.descriptor();
        let path = resolution.path().unwrap_or(descriptor.title);
        let category = if resolution.missing_candidates().is_some() {
            String::from("Win32 Bridge Missing")
        } else {
            String::from(match descriptor.loader {
                LoaderDispatch::Pe => "Win32 Bridge",
                LoaderDispatch::Elf => "POSIX Bridge",
                LoaderDispatch::Native => "Native",
            })
        };
        Some(CommandPaletteAction {
            id: 90,
            title: format!("Launch {} ({})", path, descriptor.title),
            category,
            shortcut: String::from("Enter"),
            enabled: true,
        })
    }

    fn execute_command_palette_selection(&mut self, commands: &mut Vec<SessionCommand>) {
        let actions = self.filtered_palette_actions();
        if actions.is_empty() {
            return;
        }
        let index = self
            .shell
            .command_selection
            .min(actions.len().saturating_sub(1));
        let selected = &actions[index];
        match selected.id {
            1 => commands.push(SessionCommand::Launch(
                self.launch_intent_for_app(AppKind::Terminal, LaunchSource::CommandPalette),
            )),
            2 => commands.push(SessionCommand::Launch(
                self.launch_intent_for_app(AppKind::Files, LaunchSource::CommandPalette),
            )),
            3 => commands.push(SessionCommand::Launch(
                self.launch_intent_for_app(AppKind::Settings, LaunchSource::CommandPalette),
            )),
            4 => commands.push(SessionCommand::Launch(
                self.launch_intent_for_app(AppKind::Editor, LaunchSource::CommandPalette),
            )),
            5 => commands.push(SessionCommand::Launch(self.launch_intent_for_shortcut(
                DesktopShortcutKind::Web,
                LaunchSource::CommandPalette,
            ))),
            6 => commands.push(SessionCommand::Launch(self.launch_intent_for_shortcut(
                DesktopShortcutKind::RecycleBin,
                LaunchSource::CommandPalette,
            ))),
            10..=17 => commands.push(SessionCommand::SwitchWorkspace((selected.id - 10) as u8)),
            20 => self.apply_quick_settings_toggle(0, commands),
            21 => self.toggle_quick_settings(),
            22 => self.toggle_power_state(),
            23 => {
                let _ = self.shell.client.clear_notifications();
                self.push_notice(String::from("Notifications cleared"));
                self.invalidate_shell(
                    InvalidationTarget::NotificationCenter,
                    InvalidationReason::StateChanged,
                );
                self.invalidate_shell(
                    InvalidationTarget::QuickSettings,
                    InvalidationReason::StateChanged,
                );
            }
            25 => self.toggle_overview(),
            26 => self.toggle_terminal_scratchpad(),
            27 => self.cycle_motion_profile(),
            28 => self.cycle_primary_display_scale(),
            29 => self.toggle_screen_reader(),
            30 => self.cycle_shell_density_profile(),
            24 => {
                if let Ok(entry) = self.shell.client.capture_screen("palette") {
                    self.push_notice(format!("Captured screen {}", entry.id));
                }
            }
            90 => {
                let query = self.shell.command_query.trim();
                if let Some(resolution) = self.launch_resolution(query) {
                    if let Some(candidates) = resolution.missing_candidates() {
                        self.push_notice(format!(
                            "{} binary not found; searched {}",
                            resolution.descriptor().title,
                            candidates.join(", ")
                        ));
                        self.close_command_palette();
                        return;
                    }
                    let intent = resolution.launch_intent(ExecutionContext::new(
                        LaunchSource::CommandPalette,
                        self.shell.active_workspace,
                        "command-palette-external",
                    ));
                    if let Some(path) = resolution.path() {
                        commands.push(SessionCommand::LaunchExternal(intent, path.to_string()));
                    } else {
                        commands.push(SessionCommand::Launch(intent));
                    }
                }
            }
            _ => {}
        }
        self.close_command_palette();
    }

    fn rebuild_stage_sets(&mut self) {
        let snapshots = self.app_snapshots();
        let mut sets = Vec::new();
        for workspace_id in 0..WORKSPACE_COUNT {
            let mut window_ids = Vec::new();
            for snapshot in snapshots.iter().filter(|s| s.workspace_id == workspace_id) {
                if let Some(window_id) = snapshot.window_id {
                    window_ids.push(window_id);
                }
            }
            sets.push(StageSet {
                id: workspace_id as u64,
                name: self
                    .shell
                    .client
                    .workspace_rule(workspace_id)
                    .map(|rule| rule.default_name_str())
                    .unwrap_or_else(|_| format!("Workspace {}", workspace_id.saturating_add(1))),
                window_ids,
                pinned: workspace_id == self.shell.active_workspace,
            });
        }

        if sets != self.shell.stage_sets {
            self.shell.stage_sets = sets;
            self.shell.active_stage_set = self
                .shell
                .stage_sets
                .iter()
                .position(|set| set.id as u8 == self.shell.active_workspace)
                .unwrap_or(0);
            self.invalidate_shell(
                InvalidationTarget::Overview,
                InvalidationReason::StateChanged,
            );
        }
    }

    fn cycle_stage_set(&mut self) {
        if self.is_locked() {
            return;
        }
        if !self.shell.stage_rail.visible {
            self.toggle_overview();
            return;
        }
        if self.shell.stage_sets.is_empty() {
            return;
        }
        let next = (self.shell.active_stage_set + 1) % self.shell.stage_sets.len();
        self.activate_stage_set(next);
    }

    fn activate_stage_set(&mut self, index: usize) {
        if self.is_locked() {
            return;
        }
        if self.shell.stage_sets.is_empty() {
            return;
        }
        let selected = self
            .shell
            .stage_sets
            .get(index.min(self.shell.stage_sets.len().saturating_sub(1)))
            .cloned();
        let Some(stage) = selected else {
            return;
        };
        let _ = self.switch_workspace(stage.id as u8);
        if let Some(window_id) = stage.window_ids.first().copied() {
            let _ = self.shell.client.focus_window(window_id);
        }
        self.shell.active_stage_set = index.min(self.shell.stage_sets.len().saturating_sub(1));
        self.invalidate_shell(
            InvalidationTarget::Overview,
            InvalidationReason::StateChanged,
        );
    }

    fn toggle_overview(&mut self) {
        if self.is_locked() {
            return;
        }
        let visible = self.shell.client.toggle_overview().unwrap_or(false);
        animate_shell_surface(
            &self.shell.client,
            &mut self.shell.stage_rail,
            visible,
            self.settings.animations,
            0.18,
        );
        if visible {
            self.rebuild_stage_sets();
            let _ = self
                .shell
                .client
                .focus_window(self.shell.stage_rail.window_id);
        }
        self.invalidate_shell(
            InvalidationTarget::Overview,
            InvalidationReason::StateChanged,
        );
        self.push_notice(if visible {
            String::from("Workspace overview open")
        } else {
            String::from("Workspace overview closed")
        });
    }

    fn toggle_terminal_scratchpad(&mut self) {
        if self.is_locked() {
            return;
        }
        if self.terminal.window.is_none() {
            self.terminal.workspace_id = self.shell.active_workspace;
            let intent = self.launch_intent_for_app(AppKind::Terminal, LaunchSource::ShellShortcut);
            let _ = self.dispatch_launch_intent(intent);
        }
        let Some(window) = self.terminal.window.as_mut() else {
            return;
        };

        let visible = self.shell.client.toggle_scratchpad().unwrap_or(false);
        let target_workspace = SCRATCHPAD_WORKSPACE;
        let _ = hybrid_windowing()
            .lock()
            .toggle_scratchpad(target_workspace, window.window_id);
        let _ = self
            .terminal
            .client
            .move_window_to_workspace(window.window_id, target_workspace);
        let _ = self.terminal.client.set_window_meta(
            window.window_id,
            target_workspace,
            LayerRole::WorkspaceScratchpad,
            scratchpad_flags(),
        );
        self.terminal.workspace_id = target_workspace;

        if visible {
            let rect = Rect::new(
                self.screen.x + (self.screen.width as i32 / 2) - 380,
                self.screen.y + 110,
                760,
                420,
            );
            let _ = self
                .terminal
                .client
                .move_window(window.window_id, rect.x, rect.y);
            let _ = self
                .terminal
                .client
                .resize_window(window.window_id, rect.width, rect.height);
            let _ = self.terminal.client.set_visibility(window.window_id, true);
            let _ = self.terminal.client.focus_window(window.window_id);
        } else {
            let _ = self.terminal.client.set_visibility(window.window_id, false);
        }

        let _ = sync_shell_window(&self.terminal.client, window);
        self.rebuild_stage_sets();
        self.mark_shell_dirty();
        self.push_notice(if visible {
            String::from("Terminal scratchpad shown")
        } else {
            String::from("Terminal scratchpad hidden")
        });
    }

    fn open_context_menu(&mut self, kind: AppKind) {
        self.close_switcher();
        self.close_command_palette();
        self.shell.context_target = Some(kind);
        if !self.shell.context_menu.desired_visible {
            animate_shell_surface(
                &self.shell.client,
                &mut self.shell.context_menu,
                true,
                self.settings.animations,
                0.14,
            );
        }
        let _ = self
            .shell
            .client
            .focus_window(self.shell.context_menu.window_id);
        self.invalidate_shell(
            InvalidationTarget::ContextMenu,
            InvalidationReason::StateChanged,
        );
    }

    fn close_context_menu(&mut self) {
        self.shell.context_target = None;
        if self.shell.context_menu.visible || self.shell.context_menu.desired_visible {
            animate_shell_surface(
                &self.shell.client,
                &mut self.shell.context_menu,
                false,
                self.settings.animations,
                0.12,
            );
            self.invalidate_shell(
                InvalidationTarget::ContextMenu,
                InvalidationReason::StateChanged,
            );
        }
    }

    fn apply_context_action(&mut self, action: ContextAction) {
        let Some(kind) = self.shell.context_target else {
            return;
        };

        let result = match action {
            ContextAction::Focus => {
                let intent = self.launch_intent_for_app(kind, LaunchSource::ContextMenu);
                self.dispatch_launch_intent(intent)
            }
            ContextAction::Minimize => self.minimize_app(kind),
            ContextAction::SnapLeft => self.snap_app(kind, SnapLayout::Left),
            ContextAction::SnapRight => self.snap_app(kind, SnapLayout::Right),
            ContextAction::Maximize => self.snap_app(kind, SnapLayout::Maximize),
            ContextAction::MoveNextWorkspace => self.move_app_to_next_workspace(kind),
            ContextAction::Close => self.close_app(kind),
        };

        if let Err(err) = result {
            self.push_notice(format!("{} action failed: {}", kind.title(), err));
        }
        self.close_context_menu();
        self.mark_shell_dirty();
    }

    fn minimize_app(&mut self, kind: AppKind) -> Result<(), String> {
        match kind {
            AppKind::Terminal => minimize_app_window(
                &self.terminal.client,
                &mut self.terminal.window,
                self.terminal.workspace_id,
            )?,
            AppKind::Files => minimize_app_window(
                &self.files.client,
                &mut self.files.window,
                self.files.workspace_id,
            )?,
            AppKind::Browser => minimize_app_window(
                &self.browser.client,
                &mut self.browser.window,
                self.browser.workspace_id,
            )?,
            AppKind::Settings => minimize_app_window(
                &self.settings.client,
                &mut self.settings.window,
                self.settings.workspace_id,
            )?,
            AppKind::Editor => minimize_app_window(
                &self.editor.client,
                &mut self.editor.window,
                self.editor.workspace_id,
            )?,
        }
        self.push_notice(format!("{} minimized", kind.title()));
        Ok(())
    }

    fn close_app(&mut self, kind: AppKind) -> Result<(), String> {
        let workspace_id = self.app_workspace(kind);
        match kind {
            AppKind::Terminal => close_app_window(
                &self.terminal.client,
                &mut self.terminal.window,
                &mut self.terminal.dirty,
                self.terminal.workspace_id,
            )?,
            AppKind::Files => close_app_window(
                &self.files.client,
                &mut self.files.window,
                &mut self.files.dirty,
                self.files.workspace_id,
            )?,
            AppKind::Browser => close_app_window(
                &self.browser.client,
                &mut self.browser.window,
                &mut self.browser.dirty,
                self.browser.workspace_id,
            )?,
            AppKind::Settings => close_app_window(
                &self.settings.client,
                &mut self.settings.window,
                &mut self.settings.dirty,
                self.settings.workspace_id,
            )?,
            AppKind::Editor => close_app_window(
                &self.editor.client,
                &mut self.editor.window,
                &mut self.editor.dirty,
                self.editor.workspace_id,
            )?,
        }
        let _ = self.relayout_workspace(workspace_id);
        self.rebuild_stage_sets();
        self.push_notice(format!("{} closed", kind.title()));
        Ok(())
    }

    fn move_app_to_next_workspace(&mut self, kind: AppKind) -> Result<(), String> {
        let previous_workspace = self.app_workspace(kind);
        match kind {
            AppKind::Terminal => move_app_workspace(
                &self.terminal.client,
                &mut self.terminal.window,
                &mut self.terminal.workspace_id,
                self.shell.active_workspace,
            )?,
            AppKind::Files => move_app_workspace(
                &self.files.client,
                &mut self.files.window,
                &mut self.files.workspace_id,
                self.shell.active_workspace,
            )?,
            AppKind::Browser => move_app_workspace(
                &self.browser.client,
                &mut self.browser.window,
                &mut self.browser.workspace_id,
                self.shell.active_workspace,
            )?,
            AppKind::Settings => move_app_workspace(
                &self.settings.client,
                &mut self.settings.window,
                &mut self.settings.workspace_id,
                self.shell.active_workspace,
            )?,
            AppKind::Editor => move_app_workspace(
                &self.editor.client,
                &mut self.editor.window,
                &mut self.editor.workspace_id,
                self.shell.active_workspace,
            )?,
        }
        let _ = self.relayout_workspace(previous_workspace);
        let _ = self.relayout_active_workspace();
        self.rebuild_stage_sets();
        self.push_notice(format!("{} moved to next space", kind.title()));
        Ok(())
    }

    fn snap_app(&mut self, kind: AppKind, layout: SnapLayout) -> Result<(), String> {
        let work_area = self.work_area_rect();
        match kind {
            AppKind::Terminal => snap_app_window(
                &self.terminal.client,
                &mut self.terminal.window,
                work_area,
                layout,
            )?,
            AppKind::Files => snap_app_window(
                &self.files.client,
                &mut self.files.window,
                work_area,
                layout,
            )?,
            AppKind::Browser => snap_app_window(
                &self.browser.client,
                &mut self.browser.window,
                work_area,
                layout,
            )?,
            AppKind::Settings => snap_app_window(
                &self.settings.client,
                &mut self.settings.window,
                work_area,
                layout,
            )?,
            AppKind::Editor => snap_app_window(
                &self.editor.client,
                &mut self.editor.window,
                work_area,
                layout,
            )?,
        }
        self.push_notice(format!("{} {}", kind.title(), layout.label()));
        Ok(())
    }

    fn work_area_rect(&self) -> Rect {
        let top = self.screen.y + 18 + Theme::HALO_BAR_HEIGHT as i32 + 32;
        let dock_top = task_strip_window_rect(self.screen).y;
        let bottom = dock_top - 22;
        Rect::new(
            self.screen.x + 18,
            top,
            self.screen.width.saturating_sub(36),
            bottom.saturating_sub(top).max(120) as u32,
        )
    }

    fn relayout_active_workspace(&mut self) -> Result<(), String> {
        self.relayout_workspace(self.shell.active_workspace)
    }

    fn relayout_workspace(&mut self, workspace_id: u8) -> Result<(), String> {
        let layout = self
            .shell
            .client
            .workspace_layout(workspace_id)
            .unwrap_or(WorkspaceLayout::Dwindle);
        let rule = self
            .shell
            .client
            .workspace_rule(workspace_id)
            .unwrap_or_else(|_| default_workspace_rule(workspace_id));
        let windows = self.shell.client.list_windows()?;
        let plans = {
            let mut orchestrator = hybrid_windowing().lock();
            orchestrator.set_workspace_rule(workspace_id, rule);
            orchestrator.set_workspace_layout(workspace_id, layout);
            orchestrator.plan_workspace(&windows, workspace_id, self.work_area_rect())
        };

        for plan in plans {
            let Some(window) = windows.iter().find(|window| window.id == plan.window_id) else {
                continue;
            };
            self.apply_window_plan(window.app_id, plan.window_id, plan.workspace_id, plan.rect)?;
        }
        Ok(())
    }

    fn apply_window_plan(
        &self,
        app_id: u32,
        window_id: WindowId,
        workspace_id: u8,
        rect: Rect,
    ) -> Result<(), String> {
        let client = match app_kind_from_id(app_id) {
            Some(AppKind::Terminal) => &self.terminal.client,
            Some(AppKind::Files) => &self.files.client,
            Some(AppKind::Browser) => &self.browser.client,
            Some(AppKind::Settings) => &self.settings.client,
            Some(AppKind::Editor) => &self.editor.client,
            None => return Ok(()),
        };
        client.move_window_to_workspace(window_id, workspace_id)?;
        client.set_window_meta(
            window_id,
            workspace_id,
            LayerRole::Window,
            WindowFlags::default(),
        )?;
        client.move_window(window_id, rect.x, rect.y)?;
        client.resize_window(window_id, rect.width, rect.height)?;
        Ok(())
    }

    fn switcher_candidates(&self) -> Vec<AppKind> {
        let snapshots = self.app_snapshots();
        let mut running = Vec::new();
        let mut available = Vec::new();
        for snapshot in snapshots {
            if snapshot.workspace_id == self.shell.active_workspace && snapshot.window_id.is_some()
            {
                running.push(snapshot.kind);
            } else {
                available.push(snapshot.kind);
            }
        }
        running.extend(available);
        running
    }

    fn sync_window_states(&mut self) {
        if sync_shell_window(&self.shell.client, &mut self.shell.desktop).is_none() {
            if restore_shell_window(
                &self.shell.client,
                &mut self.shell.desktop,
                "Desktop Shortcuts",
                desktop_surface_rect(self.screen),
                0,
                LayerRole::Background,
                true,
            )
            .is_ok()
            {
                self.invalidate_shell(
                    InvalidationTarget::Wallpaper,
                    InvalidationReason::StateChanged,
                );
                self.push_notice(String::from("Desktop shortcuts restored"));
            }
        }

        if sync_shell_window(&self.shell.client, &mut self.shell.top_bar).is_none() {
            if restore_shell_window(
                &self.shell.client,
                &mut self.shell.top_bar,
                "Top Bar",
                Rect::new(
                    self.screen.x + 18,
                    self.screen.y + 18,
                    self.screen.width.saturating_sub(36),
                    Theme::HALO_BAR_HEIGHT as u32,
                ),
                0,
                LayerRole::TopBar,
                true,
            )
            .is_ok()
            {
                self.invalidate_shell(InvalidationTarget::TopBar, InvalidationReason::StateChanged);
                self.push_notice(String::from("Top Bar restored"));
            }
        }

        if sync_shell_window(&self.shell.client, &mut self.shell.task_strip).is_none() {
            let rect = task_strip_window_rect(self.screen);
            if restore_shell_window(
                &self.shell.client,
                &mut self.shell.task_strip,
                "Task Strip",
                rect,
                0,
                LayerRole::Dock,
                true,
            )
            .is_ok()
            {
                self.invalidate_shell(InvalidationTarget::Dock, InvalidationReason::StateChanged);
                self.push_notice(String::from("Task Strip restored"));
            }
        }

        if sync_shell_window(&self.shell.client, &mut self.shell.launcher).is_none() {
            if restore_shell_window(
                &self.shell.client,
                &mut self.shell.launcher,
                "Session Shell",
                Rect::new(
                    self.screen.x + 42,
                    self.screen.y + 74,
                    min(392, self.screen.width.saturating_sub(96)),
                    min(332, self.screen.height.saturating_sub(172)),
                ),
                0,
                LayerRole::Overlay,
                true,
            )
            .is_ok()
            {
                self.invalidate_shell(
                    InvalidationTarget::Launcher,
                    InvalidationReason::StateChanged,
                );
                self.push_notice(String::from("Session Shell restored"));
            }
        }

        if sync_shell_window(&self.shell.client, &mut self.shell.notifications).is_none() {
            if restore_shell_window(
                &self.shell.client,
                &mut self.shell.notifications,
                "Notifications",
                Rect::new(self.screen.right() - 364, self.screen.y + 74, 320, 220),
                0,
                LayerRole::Overlay,
                true,
            )
            .is_ok()
            {
                self.invalidate_shell(
                    InvalidationTarget::NotificationCenter,
                    InvalidationReason::StateChanged,
                );
                self.push_notice(String::from("Notifications restored"));
            }
        }

        if sync_shell_window(&self.shell.client, &mut self.shell.quick_settings).is_none() {
            if restore_shell_window(
                &self.shell.client,
                &mut self.shell.quick_settings,
                "Quick Settings",
                Rect::new(self.screen.right() - 364, self.screen.y + 348, 320, 232),
                0,
                LayerRole::Overlay,
                false,
            )
            .is_ok()
            {
                self.invalidate_shell(
                    InvalidationTarget::QuickSettings,
                    InvalidationReason::StateChanged,
                );
                self.push_notice(String::from("Quick Settings restored"));
            }
        }

        if sync_shell_window(&self.shell.client, &mut self.shell.command_palette).is_none() {
            if restore_shell_window(
                &self.shell.client,
                &mut self.shell.command_palette,
                "Command Palette",
                Rect::new(
                    self.screen.x + (self.screen.width as i32 / 2) - 310,
                    self.screen.y + 136,
                    620,
                    312,
                ),
                0,
                LayerRole::Overlay,
                false,
            )
            .is_ok()
            {
                self.invalidate_shell(
                    InvalidationTarget::CommandPalette,
                    InvalidationReason::StateChanged,
                );
                self.push_notice(String::from("Command Palette restored"));
            }
        }

        if sync_shell_window(&self.shell.client, &mut self.shell.stage_rail).is_none() {
            let stage_rail_visible = self.shell.stage_rail.visible && !self.is_locked();
            if restore_shell_window(
                &self.shell.client,
                &mut self.shell.stage_rail,
                "Workspace Overview",
                Rect::new(self.screen.x + 18, self.screen.y + 108, 236, 264),
                0,
                LayerRole::Overlay,
                stage_rail_visible,
            )
            .is_ok()
            {
                self.invalidate_shell(
                    InvalidationTarget::Overview,
                    InvalidationReason::StateChanged,
                );
                self.push_notice(String::from("Workspace Overview restored"));
            }
        }

        if sync_shell_window(&self.shell.client, &mut self.shell.dialog).is_none() {
            if restore_shell_window(
                &self.shell.client,
                &mut self.shell.dialog,
                "Dialog Broker",
                Rect::new(
                    self.screen.x + (self.screen.width as i32 / 2) - 180,
                    self.screen.y + (self.screen.height as i32 / 2) - 120,
                    360,
                    190,
                ),
                0,
                LayerRole::Modal,
                self.shell.pending_dialog.is_some(),
            )
            .is_ok()
            {
                self.invalidate_shell(InvalidationTarget::Dialog, InvalidationReason::StateChanged);
                self.push_notice(String::from("Dialog Broker restored"));
            }
        }

        if sync_shell_window(&self.shell.client, &mut self.shell.context_menu).is_none() {
            if restore_shell_window(
                &self.shell.client,
                &mut self.shell.context_menu,
                "Context Menu",
                Rect::new(
                    self.screen.right() - 310,
                    self.screen.bottom() - 290,
                    240,
                    224,
                ),
                0,
                LayerRole::Overlay,
                self.shell.context_target.is_some(),
            )
            .is_ok()
            {
                self.invalidate_shell(
                    InvalidationTarget::ContextMenu,
                    InvalidationReason::StateChanged,
                );
                self.push_notice(String::from("Context Menu restored"));
            }
        }

        if sync_shell_window(&self.shell.client, &mut self.shell.switcher).is_none() {
            let should_show = self.shell.switcher.visible;
            if restore_shell_window(
                &self.shell.client,
                &mut self.shell.switcher,
                "App Switcher",
                Rect::new(
                    self.screen.x + (self.screen.width as i32 / 2) - 220,
                    self.screen.y + (self.screen.height as i32 / 2) - 120,
                    440,
                    240,
                ),
                0,
                LayerRole::Overlay,
                should_show,
            )
            .is_ok()
            {
                self.invalidate_shell(
                    InvalidationTarget::Switcher,
                    InvalidationReason::StateChanged,
                );
                self.push_notice(String::from("App Switcher restored"));
            }
        }

        if sync_shell_window(&self.shell.client, &mut self.shell.lock_screen).is_none() {
            let lock_visible = self.is_locked();
            if restore_shell_window(
                &self.shell.client,
                &mut self.shell.lock_screen,
                "Login",
                Rect::new(
                    self.screen.x + (self.screen.width as i32 / 2) - 240,
                    self.screen.y + (self.screen.height as i32 / 2) - 160,
                    480,
                    320,
                ),
                0,
                LayerRole::Modal,
                lock_visible,
            )
            .is_ok()
            {
                self.invalidate_shell(
                    InvalidationTarget::LockScreen,
                    InvalidationReason::StateChanged,
                );
                self.push_notice(String::from("Login restored"));
            }
        }

        if self.terminal.sync(self.shell.active_workspace) {
            self.mark_shell_dirty();
        }
        if self.files.sync(self.shell.active_workspace) {
            self.mark_shell_dirty();
        }
        if self.browser.sync(self.shell.active_workspace) {
            self.mark_shell_dirty();
        }
        if self.settings.sync(self.shell.active_workspace) {
            self.mark_shell_dirty();
        }
        if self.editor.sync(self.shell.active_workspace) {
            self.mark_shell_dirty();
        }
    }

    fn render_shell(&mut self) -> Result<(), String> {
        let Some(frame_plan) = self.shell.invalidation.take_frame_plan() else {
            return Ok(());
        };

        self.shell.layout_profile = Theme::layout_profile(self.screen.width);
        let session_snapshot = session_snapshot_or_fallback(self, false, 1, 1, self.shell_state());
        let snapshots = self.app_snapshots();
        let theme_mode = self.shell.theme_mode;
        let _pending_reasons = frame_plan.pending.as_slice();

        if frame_plan.touches(InvalidationTarget::Wallpaper) {
            let width = self.shell.desktop.content_rect.width as usize;
            let height = self.shell.desktop.content_rect.height as usize;
            self.shell.client.commit_scene(
                self.shell.desktop.window_id,
                raster_surface_scene(
                    self.shell.desktop.window_id,
                    width,
                    height,
                    paint_desktop_shortcuts_surface(
                        width,
                        height,
                        theme_mode,
                        self.shell.selected_shortcut,
                    ),
                    DamageLane::Shell,
                ),
            )?;
        }

        if frame_plan.touches(InvalidationTarget::TopBar) {
            let width = self.shell.top_bar.content_rect.width as usize;
            let height = self.shell.top_bar.content_rect.height as usize;
            self.commit_shell_surface(
                &self.shell.top_bar,
                paint_top_bar_surface(
                    width,
                    height,
                    self.shell.active_workspace,
                    &session_snapshot,
                    theme_mode,
                ),
            )?;
        }

        if frame_plan.touches(InvalidationTarget::Dock) {
            let width = self.shell.task_strip.content_rect.width as usize;
            let height = self.shell.task_strip.content_rect.height as usize;
            self.commit_shell_surface(
                &self.shell.task_strip,
                paint_task_strip_surface(
                    width,
                    height,
                    &snapshots,
                    self.shell.active_workspace,
                    theme_mode,
                ),
            )?;
        }

        if frame_plan.touches(InvalidationTarget::Launcher)
            && (self.shell.launcher.visible
                || self.shell.launcher.desired_visible
                || self.shell.launcher.fading_out)
        {
            let width = self.shell.launcher.content_rect.width as usize;
            let height = self.shell.launcher.content_rect.height as usize;
            self.commit_shell_surface(
                &self.shell.launcher,
                paint_launcher_surface(
                    width,
                    height,
                    &snapshots,
                    &session_snapshot,
                    &self.shell.notices,
                    theme_mode,
                    self.shell.layout_profile,
                ),
            )?;
        }

        if frame_plan.touches(InvalidationTarget::NotificationCenter)
            && self.shell.notifications.visible
        {
            let entries = self
                .shell
                .client
                .list_notifications(6)
                .unwrap_or_else(|_| Vec::new());
            let width = self.shell.notifications.content_rect.width as usize;
            let height = self.shell.notifications.content_rect.height as usize;
            self.commit_shell_surface(
                &self.shell.notifications,
                paint_notifications_surface(
                    width,
                    height,
                    &entries,
                    self.shell.notification_index,
                    theme_mode,
                ),
            )?;
        }

        if frame_plan.touches(InvalidationTarget::QuickSettings)
            && self.shell.quick_settings.visible
        {
            let width = self.shell.quick_settings.content_rect.width as usize;
            let height = self.shell.quick_settings.content_rect.height as usize;
            let session = session_snapshot_or_fallback(
                self,
                self.shell.stage_rail.visible,
                1,
                100,
                ShellState::DesktopReady,
            );
            self.commit_shell_surface(
                &self.shell.quick_settings,
                paint_quick_settings_surface(
                    width,
                    height,
                    theme_mode,
                    self.settings.notifications,
                    &session,
                ),
            )?;
        }

        if frame_plan.touches(InvalidationTarget::CommandPalette)
            && self.shell.command_palette.visible
        {
            let actions = self.filtered_palette_actions();
            let width = self.shell.command_palette.content_rect.width as usize;
            let height = self.shell.command_palette.content_rect.height as usize;
            self.commit_shell_surface(
                &self.shell.command_palette,
                paint_command_palette_surface(
                    width,
                    height,
                    &actions,
                    &self.shell.command_query,
                    self.shell.command_selection,
                    theme_mode,
                ),
            )?;
        }

        if frame_plan.touches(InvalidationTarget::Overview) && self.shell.stage_rail.visible {
            let width = self.shell.stage_rail.content_rect.width as usize;
            let height = self.shell.stage_rail.content_rect.height as usize;
            self.commit_shell_surface(
                &self.shell.stage_rail,
                paint_stage_rail_surface(
                    width,
                    height,
                    &self.shell.stage_sets,
                    self.shell.active_stage_set,
                    theme_mode,
                ),
            )?;
        }

        if frame_plan.touches(InvalidationTarget::Dialog) && self.shell.dialog.visible {
            let width = self.shell.dialog.content_rect.width as usize;
            let height = self.shell.dialog.content_rect.height as usize;
            self.commit_shell_surface(
                &self.shell.dialog,
                paint_dialog_surface(
                    width,
                    height,
                    self.shell.pending_dialog.as_ref(),
                    &self.shell.dialog_input,
                    theme_mode,
                ),
            )?;
        }

        if frame_plan.touches(InvalidationTarget::ContextMenu) && self.shell.context_menu.visible {
            let width = self.shell.context_menu.content_rect.width as usize;
            let height = self.shell.context_menu.content_rect.height as usize;
            self.commit_shell_surface(
                &self.shell.context_menu,
                paint_context_menu_surface(width, height, self.shell.context_target, theme_mode),
            )?;
        }

        if frame_plan.touches(InvalidationTarget::Switcher) && self.shell.switcher.visible {
            let candidates = self.switcher_candidates();
            let width = self.shell.switcher.content_rect.width as usize;
            let height = self.shell.switcher.content_rect.height as usize;
            self.commit_shell_surface(
                &self.shell.switcher,
                paint_switcher_surface(
                    width,
                    height,
                    &candidates,
                    self.shell.switcher_index,
                    self.shell.active_workspace,
                    theme_mode,
                ),
            )?;
        }

        if frame_plan.touches(InvalidationTarget::LockScreen) && self.shell.lock_screen.visible {
            let width = self.shell.lock_screen.content_rect.width as usize;
            let height = self.shell.lock_screen.content_rect.height as usize;
            self.shell.client.commit_scene(
                self.shell.lock_screen.window_id,
                build_lock_scene(
                    self.shell.lock_screen.window_id,
                    &mut self.text_system,
                    width,
                    height,
                    &self.shell.auth_input,
                    self.shell.logged_in,
                    theme_mode,
                ),
            )?;
        }

        Ok(())
    }

    fn render_apps(&mut self) -> Result<(), String> {
        self.terminal.render()?;
        self.files.render()?;
        self.browser.render()?;
        self.settings.render()?;
        self.editor.render()?;
        Ok(())
    }

    fn app_snapshots(&self) -> Vec<AppSnapshot> {
        let mut snapshots = vec![
            self.terminal.snapshot(),
            self.files.snapshot(),
            self.browser.snapshot(),
            self.settings.snapshot(),
            self.editor.snapshot(),
        ];

        if let Ok(entries) = self.shell.client.list_shell_apps() {
            for entry in entries {
                if let Some(kind) = app_kind_from_id(entry.app_id) {
                    if let Some(snapshot) =
                        snapshots.iter_mut().find(|snapshot| snapshot.kind == kind)
                    {
                        apply_shell_entry(snapshot, &entry);
                    }
                }
            }
        }

        snapshots
    }
}

#[derive(Clone)]
enum SessionCommand {
    Launch(LaunchIntent),
    LaunchExternal(LaunchIntent, String),
    SwitchWorkspace(u8),
    Notify(String),
    OpenEditorPath(String),
}

#[derive(Clone, Copy)]
enum SnapLayout {
    Left,
    Right,
    Maximize,
}

impl SnapLayout {
    fn label(self) -> &'static str {
        match self {
            Self::Left => "snapped left",
            Self::Right => "snapped right",
            Self::Maximize => "maximized",
        }
    }
}

#[derive(Clone, Copy)]
enum ContextAction {
    Focus,
    Minimize,
    SnapLeft,
    SnapRight,
    Maximize,
    MoveNextWorkspace,
    Close,
}

fn ensure_window_visible(
    client: &DesktopClient,
    window: &mut Option<SessionWindow>,
    title: &str,
    rect: Rect,
    workspace_id: u8,
) -> Result<LaunchResult, String> {
    if let Some(current) = window.as_mut() {
        if let Ok(info) = client.window_info(current.window_id) {
            let was_visible = info.visible;
            current.update_from_info(&info);
            let _ = client.move_window_to_workspace(current.window_id, workspace_id);
            let _ = client.set_window_meta(
                current.window_id,
                workspace_id,
                LayerRole::Window,
                WindowFlags::default(),
            );
            if !was_visible {
                client.set_visibility(current.window_id, true)?;
            }
            client.focus_window(current.window_id)?;
            if let Ok(info) = client.window_info(current.window_id) {
                current.update_from_info(&info);
            }
            let _ = client.update_shell_window(
                Some(current.window_id),
                current.visible,
                current.focused,
                workspace_id,
            );
            let _ = client.clear_app_attention(Some("window restored"));
            return Ok(if was_visible {
                LaunchResult::Focused
            } else {
                LaunchResult::Restored
            });
        }
        *window = None;
    }

    let created = client.create_layer_window(
        title,
        rect.x,
        rect.y,
        rect.width,
        rect.height,
        workspace_id,
        LayerRole::Window,
        WindowFlags::default(),
    )?;
    client.focus_window(created.window_id)?;
    *window = Some(SessionWindow::from_client_window(created));
    let _ = client.mark_app_launched(title);
    let _ = client.update_shell_window(Some(created.window_id), true, true, workspace_id);
    Ok(LaunchResult::Launched)
}

fn sync_window_state(
    client: &DesktopClient,
    window: &mut Option<SessionWindow>,
    workspace_id: u8,
    active_workspace: u8,
) -> WindowSync {
    let Some(current) = window.as_mut() else {
        return WindowSync::Unchanged;
    };

    match client.window_info(current.window_id) {
        Ok(info) => {
            let changed = current.update_from_info(&info);
            let visible = current.visible && workspace_id == active_workspace;
            if changed {
                let _ = client.update_shell_window(
                    Some(current.window_id),
                    visible,
                    current.focused && visible,
                    workspace_id,
                );
                WindowSync::Changed
            } else {
                let _ = client.update_shell_window(
                    Some(current.window_id),
                    visible,
                    current.focused && visible,
                    workspace_id,
                );
                WindowSync::Unchanged
            }
        }
        Err(_) => {
            *window = None;
            let _ = client.update_shell_window(None, false, false, workspace_id);
            WindowSync::Closed
        }
    }
}

fn sync_shell_window(client: &DesktopClient, window: &mut SessionWindow) -> Option<()> {
    let info = client.window_info(window.window_id).ok()?;
    let _ = window.update_from_info(&info);
    Some(())
}

fn restore_shell_window(
    client: &DesktopClient,
    window: &mut SessionWindow,
    title: &str,
    rect: Rect,
    workspace_id: u8,
    layer_role: LayerRole,
    visible: bool,
) -> Result<(), String> {
    let created = client.create_layer_window(
        title,
        rect.x,
        rect.y,
        rect.width,
        rect.height,
        workspace_id,
        layer_role,
        shell_layer_flags(),
    )?;
    if !visible {
        client.set_visibility(created.window_id, false)?;
    }
    *window = SessionWindow::from_client_window(created);
    window.visible = visible;
    window.desired_visible = visible;
    window.opacity = if visible { 1.0 } else { 0.0 };
    window.fading_out = false;
    Ok(())
}

fn apply_workspace_visibility(
    client: &DesktopClient,
    window: &mut Option<SessionWindow>,
    workspace_id: u8,
    active_workspace: u8,
) -> Result<(), String> {
    let Some(current) = window.as_mut() else {
        return Ok(());
    };

    let should_be_visible = workspace_id == active_workspace;
    if current.visible != should_be_visible {
        client.set_visibility(current.window_id, should_be_visible)?;
        if let Ok(info) = client.window_info(current.window_id) {
            current.update_from_info(&info);
        } else {
            current.visible = should_be_visible;
        }
        current.desired_visible = should_be_visible;
        current.opacity = if should_be_visible { 1.0 } else { 0.0 };
        current.fading_out = false;
    }

    let _ = client.update_shell_window(
        Some(current.window_id),
        should_be_visible,
        current.focused && should_be_visible,
        workspace_id,
    );
    Ok(())
}

fn minimize_app_window(
    client: &DesktopClient,
    window: &mut Option<SessionWindow>,
    workspace_id: u8,
) -> Result<(), String> {
    let Some(current) = window.as_mut() else {
        return Ok(());
    };
    client.set_visibility(current.window_id, false)?;
    if let Ok(info) = client.window_info(current.window_id) {
        current.update_from_info(&info);
    } else {
        current.visible = false;
    }
    current.desired_visible = false;
    current.opacity = 0.0;
    current.fading_out = false;
    let _ = client.update_shell_window(Some(current.window_id), false, false, workspace_id);
    Ok(())
}

fn close_app_window(
    client: &DesktopClient,
    window: &mut Option<SessionWindow>,
    dirty: &mut bool,
    workspace_id: u8,
) -> Result<(), String> {
    let Some(current) = window.take() else {
        return Ok(());
    };
    client.destroy_window(current.window_id)?;
    let _ = client.mark_app_exited(true, "closed by shell");
    let _ = client.update_shell_window(None, false, false, workspace_id);
    *dirty = true;
    Ok(())
}

fn move_app_workspace(
    client: &DesktopClient,
    window: &mut Option<SessionWindow>,
    workspace_id: &mut u8,
    active_workspace: u8,
) -> Result<(), String> {
    *workspace_id = (*workspace_id + 1) % WORKSPACE_COUNT;
    if let Some(current) = window.as_ref() {
        client.move_window_to_workspace(current.window_id, *workspace_id)?;
        client.set_window_meta(
            current.window_id,
            *workspace_id,
            LayerRole::Window,
            WindowFlags::default(),
        )?;
    }
    apply_workspace_visibility(client, window, *workspace_id, active_workspace)
}

fn snap_app_window(
    client: &DesktopClient,
    window: &mut Option<SessionWindow>,
    work_area: Rect,
    layout: SnapLayout,
) -> Result<(), String> {
    let Some(current) = window.as_mut() else {
        return Ok(());
    };

    let target_frame = match layout {
        SnapLayout::Left => Rect::new(
            work_area.x,
            work_area.y,
            work_area.width / 2,
            work_area.height,
        ),
        SnapLayout::Right => Rect::new(
            work_area.x + (work_area.width as i32 / 2),
            work_area.y,
            work_area.width / 2,
            work_area.height,
        ),
        SnapLayout::Maximize => work_area,
    };

    let content_width = target_frame
        .width
        .saturating_sub(BORDER_THICKNESS.saturating_mul(2))
        .max(MIN_CONTENT_WIDTH);
    let chrome_height = TITLEBAR_HEIGHT.saturating_add(BORDER_THICKNESS);
    let content_height = target_frame
        .height
        .saturating_sub(chrome_height)
        .max(MIN_CONTENT_HEIGHT);

    client.move_window(current.window_id, target_frame.x, target_frame.y)?;
    client.resize_window(current.window_id, content_width, content_height)?;
    if let Ok(info) = client.window_info(current.window_id) {
        current.update_from_info(&info);
    }
    Ok(())
}

fn snapshot_for_window(
    kind: AppKind,
    window: Option<SessionWindow>,
    workspace_id: u8,
    detail: String,
) -> AppSnapshot {
    if let Some(window) = window {
        AppSnapshot {
            kind,
            window_id: Some(window.window_id),
            visible: window.visible,
            focused: window.focused,
            workspace_id,
            detail,
            health: if window.visible {
                AppHealth::Running
            } else {
                AppHealth::Idle
            },
            launch_count: 0,
            crash_count: 0,
            needs_attention: false,
        }
    } else {
        AppSnapshot {
            kind,
            window_id: None,
            visible: false,
            focused: false,
            workspace_id,
            detail,
            health: AppHealth::Idle,
            launch_count: 0,
            crash_count: 0,
            needs_attention: false,
        }
    }
}

fn apply_shell_entry(snapshot: &mut AppSnapshot, entry: &ShellAppEntry) {
    snapshot.window_id = entry.window_id.or(snapshot.window_id);
    snapshot.visible = entry.visible;
    snapshot.focused = entry.focused;
    snapshot.workspace_id = entry.workspace_id;
    snapshot.health = entry.health;
    snapshot.launch_count = entry.launch_count;
    snapshot.crash_count = entry.crash_count;
    snapshot.needs_attention = entry.needs_attention;
    snapshot.detail = if entry.status_line.is_empty() {
        snapshot.detail.clone()
    } else {
        entry.status_line.clone()
    };
}
struct FilesEntry {
    name: &'static str,
    detail: &'static str,
    accent: u32,
    launch: Option<AppKind>,
}

const FILES_ENTRIES: [FilesEntry; 4] = [
    FilesEntry {
        name: "workspace/README.txt",
        detail: "Open the native editor scratchpad",
        accent: ACCENT_CORAL,
        launch: Some(AppKind::Editor),
    },
    FilesEntry {
        name: "system/session.cfg",
        detail: "Inspect shell controls in Settings",
        accent: ACCENT_GOLD,
        launch: Some(AppKind::Settings),
    },
    FilesEntry {
        name: "var/log/terminal.log",
        detail: "Jump back to the terminal surface",
        accent: ACCENT_MINT,
        launch: Some(AppKind::Terminal),
    },
    FilesEntry {
        name: "assets/overview",
        detail: "Thumbnail and preview pipeline queued",
        accent: ACCENT_BLUE,
        launch: None,
    },
];

fn blend_pixel(background: u32, foreground: u32, opacity: f32) -> u32 {
    let opacity = opacity.clamp(0.0, 1.0);
    let br = ((background >> 16) & 0xFF) as f32;
    let bg = ((background >> 8) & 0xFF) as f32;
    let bb = (background & 0xFF) as f32;

    let fr = ((foreground >> 16) & 0xFF) as f32;
    let fg = ((foreground >> 8) & 0xFF) as f32;
    let fb = (foreground & 0xFF) as f32;

    let r = (br * (1.0 - opacity) + fr * opacity) as u32;
    let g = (bg * (1.0 - opacity) + fg * opacity) as u32;
    let b = (bb * (1.0 - opacity) + fb * opacity) as u32;
    0xFF00_0000 | (r << 16) | (g << 8) | b
}

fn transform_overlay_surface(
    pixels: Vec<u32>,
    width: usize,
    height: usize,
    background: u32,
    opacity: f32,
    slide_px: i32,
) -> Vec<u32> {
    let opacity = opacity.clamp(0.0, 1.0);
    if width == 0 || height == 0 {
        return pixels;
    }

    let shift = slide_px.max(0) as usize;
    let mut transformed = vec![background; width.saturating_mul(height)];

    if shift >= height {
        return transformed;
    }

    for row in 0..height.saturating_sub(shift) {
        let src = row * width;
        let dst = (row + shift) * width;
        transformed[dst..dst + width].copy_from_slice(&pixels[src..src + width]);
    }

    if opacity >= 0.995 {
        return transformed;
    }

    for pixel in &mut transformed {
        *pixel = blend_pixel(background, *pixel, opacity);
    }
    transformed
}

fn launcher_hit(local: Point) -> Option<AppKind> {
    let row_top = 92;
    let row_height = 54;
    let row_gap = 12;
    if local.x < 18 || local.x > 344 {
        return None;
    }
    for (index, kind) in AppKind::ALL.iter().enumerate() {
        let top = row_top + index as i32 * (row_height + row_gap);
        let rect = Rect::new(18, top, 332, row_height as u32);
        if rect.contains(local) {
            return Some(*kind);
        }
    }
    None
}

fn task_strip_workspace_hit(_local: Point, _width: usize) -> Option<u8> {
    None
}

fn task_strip_app_hit(local: Point, width: usize) -> Option<AppKind> {
    if local.y < 10 || local.y > 70 {
        return None;
    }
    for (index, kind) in AppKind::ALL.iter().enumerate() {
        let rect = task_strip_icon_rect(index, width);
        if rect.contains(local) {
            return Some(*kind);
        }
    }
    None
}

fn dialog_button_hit(local: Point) -> Option<bool> {
    let accept = Rect::new(190, 126, 134, 40);
    if accept.contains(local) {
        return Some(true);
    }
    let cancel = Rect::new(36, 126, 134, 40);
    if cancel.contains(local) {
        return Some(false);
    }
    None
}

fn switcher_hit(local: Point) -> Option<usize> {
    if local.x < 24 || local.x > 404 {
        return None;
    }
    for index in 0..AppKind::ALL.len() {
        let top = 72 + index as i32 * 36;
        let rect = Rect::new(24, top, 392, 30);
        if rect.contains(local) {
            return Some(index);
        }
    }
    None
}

fn context_menu_hit(local: Point) -> Option<ContextAction> {
    let actions = [
        ContextAction::Focus,
        ContextAction::Minimize,
        ContextAction::SnapLeft,
        ContextAction::SnapRight,
        ContextAction::Maximize,
        ContextAction::MoveNextWorkspace,
        ContextAction::Close,
    ];
    for (index, action) in actions.into_iter().enumerate() {
        let top = 52 + index as i32 * 24;
        let rect = Rect::new(16, top, 208, 22);
        if rect.contains(local) {
            return Some(action);
        }
    }
    None
}

fn files_hit(local: Point, entry_count: usize) -> Option<usize> {
    let row_top = 78;
    let row_height = 58;
    if local.x < 18 {
        return None;
    }
    for index in 0..entry_count {
        let top = row_top + index as i32 * row_height;
        let rect = Rect::new(18, top, 420, 48);
        if rect.contains(local) {
            return Some(index);
        }
    }
    None
}

fn settings_hit(local: Point) -> Option<usize> {
    let row_top = 80;
    let row_height = 64;
    for index in 0..6 {
        let top = row_top + index as i32 * row_height;
        let rect = Rect::new(20, top, 420, 54);
        if rect.contains(local) {
            return Some(index);
        }
    }
    None
}

fn key_press(input: &InputEvent) -> Option<(Option<char>, u16, u8)> {
    match input {
        InputEvent::Key {
            unicode,
            scan_code,
            modifiers,
            state: KeyState::Pressed,
        } => Some((*unicode, *scan_code, *modifiers)),
        _ => None,
    }
}

fn key_scan_pressed(input: &InputEvent, scan_code: u16) -> bool {
    matches!(key_press(input), Some((_, code, _)) if code == scan_code)
}

fn ctrl_scan_pressed(input: &InputEvent, scan_code: u16) -> bool {
    matches!(key_press(input), Some((_, code, modifiers)) if code == scan_code && modifiers & MOD_CTRL != 0)
}

fn printable_key(input: &InputEvent) -> Option<char> {
    match key_press(input) {
        Some((Some(ch), _, _)) if is_printable(ch) => Some(ch),
        _ => None,
    }
}

fn digit_key_pressed(input: &InputEvent) -> Option<u8> {
    match key_press(input) {
        Some((Some(ch @ '1'..='9'), _, _)) => Some(ch as u8 - b'0'),
        Some((_, scan_code @ 0x02..=0x0A, _)) => Some((scan_code - 0x01) as u8),
        Some((Some('0'), _, _)) | Some((_, 0x0B, _)) => Some(0),
        _ => None,
    }
}

fn is_enter_key(input: &InputEvent) -> bool {
    matches!(
        key_press(input),
        Some((Some('\n' | '\r'), _, _)) | Some((_, 0x1C, _))
    )
}

fn is_backspace_key(input: &InputEvent) -> bool {
    matches!(
        key_press(input),
        Some((Some('\u{8}'), _, _)) | Some((_, 0x0E, _))
    )
}

fn is_escape_key(input: &InputEvent) -> bool {
    matches!(
        key_press(input),
        Some((Some('\u{1b}'), _, _)) | Some((_, 0x01, _))
    )
}

fn app_shortcut_from_input(input: &InputEvent) -> Option<AppKind> {
    match digit_key_pressed(input) {
        Some(1) => Some(AppKind::Terminal),
        Some(2) => Some(AppKind::Files),
        Some(3) => Some(AppKind::Browser),
        Some(4) => Some(AppKind::Settings),
        Some(5) => Some(AppKind::Editor),
        _ => None,
    }
}

fn is_printable(ch: char) -> bool {
    !ch.is_control()
}

fn debug_marker(bytes: &[u8]) {
    let mut port = PortWriteOnly::<u8>::new(0xE9);
    for &byte in bytes {
        unsafe { port.write(byte) };
    }
}

fn dialog_default_path(request: &DialogRequest) -> String {
    if !request.path_hint.is_empty() {
        return request.path_hint.clone();
    }

    match request.kind {
        DialogKind::OpenFile => String::from("/workspace/demo.txt"),
        DialogKind::SaveFile => String::from("/workspace/output.txt"),
        DialogKind::PickFolder => String::from("/workspace"),
        DialogKind::Message => String::from("Acknowledged"),
    }
}

fn normalize_dialog_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return String::from("/");
    }
    let without_trailing = trimmed.trim_end_matches('/');
    if without_trailing.is_empty() {
        return String::from("/");
    }
    if without_trailing.starts_with('/') {
        String::from(without_trailing)
    } else {
        format!("/{}", without_trailing)
    }
}

fn dialog_parent_path(path: &str) -> String {
    let normalized = normalize_dialog_path(path);
    if normalized == "/" {
        return normalized;
    }
    match normalized.rfind('/') {
        Some(0) | None => String::from("/"),
        Some(index) => normalized[..index].to_string(),
    }
}

fn dialog_entry_exists(path: &str) -> Option<F2fsEntry> {
    open_entry(&normalize_dialog_path(path)).ok()
}

fn validate_dialog_selection(request: &DialogRequest, path: &str) -> Result<String, String> {
    let normalized = normalize_dialog_path(path);
    match request.kind {
        DialogKind::OpenFile => match dialog_entry_exists(&normalized) {
            Some(entry) if !entry.is_dir => Ok(normalized),
            Some(_) => Err(String::from("Open file dialog bir dosya bekliyor")),
            None => Err(String::from("Secilen dosya bulunamadi")),
        },
        DialogKind::SaveFile => {
            if matches!(dialog_entry_exists(&normalized), Some(entry) if entry.is_dir) {
                return Err(String::from("Save file dialog bir dosya yolu bekliyor"));
            }
            let parent = dialog_parent_path(&normalized);
            match dialog_entry_exists(&parent) {
                Some(entry) if entry.is_dir => Ok(normalized),
                _ => Err(String::from("Hedef dizin bulunamadi")),
            }
        }
        DialogKind::PickFolder => match dialog_entry_exists(&normalized) {
            Some(entry) if entry.is_dir => Ok(normalized),
            Some(_) => Err(String::from("Pick folder dialog bir dizin bekliyor")),
            None => Err(String::from("Secilen dizin bulunamadi")),
        },
        DialogKind::Message => Ok(normalized),
    }
}

fn app_health_label(health: AppHealth, visible: bool, focused: bool) -> &'static str {
    match health {
        AppHealth::Crashed => "crashed",
        AppHealth::Attention => "attention",
        AppHealth::Running if focused => "focused",
        AppHealth::Running if visible => "running",
        AppHealth::Running => "background",
        AppHealth::Idle => "available",
    }
}

fn app_health_color(health: AppHealth, needs_attention: bool, accent: u32) -> u32 {
    if needs_attention {
        return ACCENT_GOLD;
    }
    match health {
        AppHealth::Crashed => ACCENT_CORAL,
        AppHealth::Attention => ACCENT_GOLD,
        AppHealth::Running => accent,
        AppHealth::Idle => BORDER,
    }
}

fn workspace_layout_label(layout: WorkspaceLayout) -> &'static str {
    match layout {
        WorkspaceLayout::Dwindle => "dwindle",
        WorkspaceLayout::Master => "master",
        WorkspaceLayout::Floating => "floating",
        WorkspaceLayout::Overview => "overview",
    }
}

fn notification_level_label(level: NotificationLevel) -> &'static str {
    match level {
        NotificationLevel::Info => "info",
        NotificationLevel::Success => "ok",
        NotificationLevel::Warning => "warn",
        NotificationLevel::Error => "error",
    }
}

fn power_state_label(state: SessionPowerState) -> &'static str {
    match state {
        SessionPowerState::Active => "active",
        SessionPowerState::Locked => "locked",
        SessionPowerState::Suspended => "suspended",
    }
}

fn permission_state_label(state: PermissionState) -> &'static str {
    match state {
        PermissionState::Ask => "Ask",
        PermissionState::Granted => "Granted",
        PermissionState::Denied => "Denied",
    }
}

fn toggle_permission(state: Result<PermissionState, String>) -> PermissionState {
    match state.unwrap_or(PermissionState::Ask) {
        PermissionState::Granted => PermissionState::Denied,
        PermissionState::Denied | PermissionState::Ask => PermissionState::Granted,
    }
}

#[derive(Clone, Copy)]
struct TopBarLayout {
    apps_rect: Rect,
    workspace_rect: Rect,
    command_rect: Rect,
    status_rects: [Rect; 5],
    time_rect: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TopBarStatusKind {
    Net,
    Cpu,
    Vpn,
    Aud,
    Power,
}

fn top_bar_layout(width: i32) -> TopBarLayout {
    let width = width.max(1240);
    let row_y = 12;
    let row_h = 34;
    let apps_rect = Rect::new(120, row_y, 54, row_h as u32);
    let workspace_rect = Rect::new(
        apps_rect.right().saturating_add(16),
        row_y,
        234,
        row_h as u32,
    );
    let time_rect = Rect::new(width.saturating_sub(136), row_y, 118, row_h as u32);
    let status_widths = [48, 52, 58, 50, 50];
    let mut cursor = time_rect.x.saturating_sub(12);
    let mut status_rects = [Rect::new(-10_000, -10_000, 1, 1); 5];
    for index in (0..status_rects.len()).rev() {
        let width_px = status_widths[index];
        cursor = cursor.saturating_sub(width_px);
        status_rects[index] = Rect::new(cursor, row_y, width_px as u32, row_h as u32);
        cursor = cursor.saturating_sub(10);
    }
    let command_left = workspace_rect.right().saturating_add(26);
    let command_right = status_rects[0].x.saturating_sub(18);
    let command_span = command_right.saturating_sub(command_left);
    let command_width = if command_span >= 120 {
        command_span.min(180)
    } else {
        command_span
    };
    let command_rect = Rect::new(
        command_left + command_span.saturating_sub(command_width) / 2,
        row_y,
        command_width as u32,
        row_h as u32,
    );

    TopBarLayout {
        apps_rect,
        workspace_rect,
        command_rect,
        status_rects,
        time_rect,
    }
}

fn top_bar_workspace_rect(index: u8, width: i32) -> Rect {
    let layout = top_bar_layout(width);
    if index >= 4 {
        return Rect::new(-10_000, -10_000, 1, 1);
    }
    let gap = 10;
    let button_width = 48i32;
    Rect::new(
        layout.workspace_rect.x + index as i32 * (button_width + gap),
        layout.workspace_rect.y,
        button_width as u32,
        layout.workspace_rect.height,
    )
}

fn top_bar_workspace_hit(local: Point, width: i32, active_workspace: u8) -> Option<u8> {
    let workspace_start = ((active_workspace as usize) / 4) as u8 * 4;
    for slot in 0..4u8 {
        let workspace_id = workspace_start.saturating_add(slot);
        if workspace_id >= WORKSPACE_COUNT {
            break;
        }
        if top_bar_workspace_rect(slot, width).contains(local) {
            return Some(workspace_id);
        }
    }
    None
}

fn top_bar_apps_hit(local: Point, width: i32) -> bool {
    top_bar_layout(width).apps_rect.contains(local)
}

fn task_strip_workspace_rect(index: u8) -> Rect {
    let gap = 4;
    let button_width = 25;
    Rect::new(
        18 + index as i32 * (button_width + gap),
        24,
        button_width as u32,
        36,
    )
}

fn task_strip_icon_rect(index: usize, width: usize) -> Rect {
    let dock_width = 354u32.min(width as u32);
    let dock_rect = Rect::new(
        ((width as i32 - dock_width as i32) / 2).max(0),
        0,
        dock_width,
        Theme::PULSE_DOCK_HEIGHT as u32,
    );
    let icons = layout_flex(
        Rect::new(
            dock_rect.x + 19,
            dock_rect.y + 14,
            dock_rect.width.saturating_sub(38),
            52,
        ),
        FlexDirection::Row,
        EdgeInsets::default(),
        14,
        &[
            FlexItem::fixed(52),
            FlexItem::fixed(52),
            FlexItem::fixed(52),
            FlexItem::fixed(52),
            FlexItem::fixed(52),
        ],
    );
    icons
        .get(index)
        .copied()
        .unwrap_or(Rect::new(-10_000, -10_000, 1, 1))
}

fn desktop_shortcut_label_width(label: &str) -> i32 {
    (label.len() as i32).saturating_mul(FONT_WIDTH)
}

fn desktop_shortcut_entries(width: usize, height: usize) -> [DesktopShortcutEntry; 5] {
    let work = Rect::new(0, 0, width as u32, height as u32).inset(18, 14, 18, 18);
    let left = work.x + 18;
    let top = work.y + 10;
    DesktopShortcutKind::ALL.map(|kind| {
        let index = match kind {
            DesktopShortcutKind::Terminal => 0,
            DesktopShortcutKind::Files => 1,
            DesktopShortcutKind::Web => 2,
            DesktopShortcutKind::Settings => 3,
            DesktopShortcutKind::RecycleBin => 4,
        } as i32;
        let icon_rect = Rect::new(
            left,
            top + DESKTOP_SHORTCUT_STEP_Y * index,
            DESKTOP_SHORTCUT_ICON_SIZE as u32,
            DESKTOP_SHORTCUT_ICON_SIZE as u32,
        );
        let label = kind.label();
        let label_width = desktop_shortcut_label_width(label);
        let label_x = icon_rect.x + ((icon_rect.width as i32 - label_width) / 2);
        let label_y = icon_rect.y + DESKTOP_SHORTCUT_ICON_SIZE + 10;
        let hit_x = (label_x.min(icon_rect.x) - 8).max(0);
        let hit_right = (label_x + label_width).max(icon_rect.right()) + 8;
        let hit_rect = Rect::new(
            hit_x,
            icon_rect.y - 8,
            hit_right.saturating_sub(hit_x) as u32,
            (label_y + FONT_HEIGHT - (icon_rect.y - 8)) as u32,
        );
        DesktopShortcutEntry {
            kind,
            icon_rect,
            label_x,
            label_y,
            hit_rect,
        }
    })
}

fn desktop_shortcut_hit(local: Point, width: usize, height: usize) -> Option<DesktopShortcutKind> {
    desktop_shortcut_entries(width, height)
        .into_iter()
        .find(|entry| entry.hit_rect.contains(local))
        .map(|entry| entry.kind)
}

fn quick_settings_row_rect(index: usize, width: usize) -> Rect {
    Rect::new(
        18,
        86 + index as i32 * 58,
        width.saturating_sub(36) as u32,
        46,
    )
}

fn command_palette_row_rect(index: usize, width: usize) -> Rect {
    Rect::new(
        18,
        138 + index as i32 * 54,
        width.saturating_sub(36) as u32,
        44,
    )
}

fn stage_rail_row_rect(index: usize, width: usize) -> Rect {
    let card_width = width.saturating_sub(54).saturating_div(2);
    let x = 18 + (index % 2) as i32 * (card_width as i32 + 18);
    let y = 82 + (index / 2) as i32 * 214;
    Rect::new(x, y, card_width as u32, 196)
}

fn top_bar_command_hit(local: Point, width: i32) -> bool {
    top_bar_layout(width).command_rect.contains(local)
}

fn top_bar_status_hit(local: Point, width: i32) -> Option<TopBarStatusKind> {
    top_bar_layout(width)
        .status_rects
        .iter()
        .enumerate()
        .find_map(|(index, rect)| {
            if !rect.contains(local) {
                return None;
            }
            Some(match index {
                0 => TopBarStatusKind::Net,
                1 => TopBarStatusKind::Cpu,
                2 => TopBarStatusKind::Vpn,
                3 => TopBarStatusKind::Aud,
                _ => TopBarStatusKind::Power,
            })
        })
}

fn top_bar_time_hit(local: Point, width: i32) -> bool {
    top_bar_layout(width).time_rect.contains(local)
}

fn notification_hit(local: Point) -> Option<usize> {
    if local.x < 18 || local.x > 302 || local.y < 66 {
        return None;
    }
    Some(((local.y - 66) / 42).max(0) as usize)
}

fn quick_settings_hit(local: Point, width: usize, item_count: usize) -> Option<usize> {
    for index in 0..item_count {
        if quick_settings_row_rect(index, width).contains(local) {
            return Some(index);
        }
    }
    None
}

fn command_palette_hit(local: Point, width: usize, item_count: usize) -> Option<usize> {
    for index in 0..item_count.min(6) {
        if command_palette_row_rect(index, width).contains(local) {
            return Some(index);
        }
    }
    None
}

fn stage_rail_hit(local: Point, width: usize, set_count: usize) -> Option<usize> {
    for index in 0..set_count {
        if stage_rail_row_rect(index, width).contains(local) {
            return Some(index);
        }
    }
    None
}

fn theme_mode_label(mode: ThemeMode) -> &'static str {
    match mode {
        ThemeMode::Dark => "Dark",
        ThemeMode::Light => "Light",
        ThemeMode::Auto => "Auto",
    }
}

fn shell_density_label(profile: ShellDensityProfile) -> &'static str {
    match profile {
        ShellDensityProfile::Comfort => "Comfort",
        ShellDensityProfile::Balanced => "Balanced",
        ShellDensityProfile::Compact => "Compact",
    }
}

fn next_shell_density_profile(profile: ShellDensityProfile) -> ShellDensityProfile {
    match profile {
        ShellDensityProfile::Comfort => ShellDensityProfile::Balanced,
        ShellDensityProfile::Balanced => ShellDensityProfile::Compact,
        ShellDensityProfile::Compact => ShellDensityProfile::Comfort,
    }
}

fn motion_profile_label(profile: MotionProfile) -> &'static str {
    match profile {
        MotionProfile::Calm => "Calm",
        MotionProfile::Standard => "Standard",
        MotionProfile::Expressive => "Expressive",
        MotionProfile::Reduced => "Reduced",
    }
}

fn next_motion_profile(profile: MotionProfile) -> MotionProfile {
    match profile {
        MotionProfile::Calm => MotionProfile::Standard,
        MotionProfile::Standard => MotionProfile::Expressive,
        MotionProfile::Expressive => MotionProfile::Reduced,
        MotionProfile::Reduced => MotionProfile::Calm,
    }
}

fn restore_disposition_label(disposition: RestoreDisposition) -> &'static str {
    match disposition {
        RestoreDisposition::NoRestore => "Manual",
        RestoreDisposition::RestoreIfClean => "Clean",
        RestoreDisposition::RestoreIfPinned => "Pinned",
        RestoreDisposition::ForceRestoreShellOwned => "Shell First",
    }
}

fn next_restore_disposition(disposition: RestoreDisposition) -> RestoreDisposition {
    match disposition {
        RestoreDisposition::NoRestore => RestoreDisposition::RestoreIfClean,
        RestoreDisposition::RestoreIfClean => RestoreDisposition::RestoreIfPinned,
        RestoreDisposition::RestoreIfPinned => RestoreDisposition::ForceRestoreShellOwned,
        RestoreDisposition::ForceRestoreShellOwned => RestoreDisposition::NoRestore,
    }
}

fn vrr_policy_label(policy: VrrPolicy) -> &'static str {
    match policy {
        VrrPolicy::Off => "VRR Off",
        VrrPolicy::On => "VRR On",
        VrrPolicy::Auto => "VRR Auto",
    }
}

fn hdr_policy_label(policy: HdrPolicy) -> &'static str {
    match policy {
        HdrPolicy::Off => "HDR Off",
        HdrPolicy::On => "HDR On",
        HdrPolicy::Auto => "HDR Auto",
    }
}

fn accessibility_profile_label(profile: AccessibilityProfile) -> String {
    if profile.screen_reader {
        String::from("Screen reader")
    } else if profile.magnifier {
        String::from("Magnifier")
    } else if profile.captions_enabled {
        String::from("Captions")
    } else if profile.reduced_motion {
        String::from("Reduced motion")
    } else if profile.contrast_theme {
        String::from("Contrast")
    } else {
        String::from("Standard")
    }
}

fn next_supported_scale(scales: &[u16], current: u16) -> u16 {
    if scales.is_empty() {
        return current.max(100);
    }
    let current_index = scales
        .iter()
        .position(|scale| *scale == current)
        .unwrap_or(0);
    scales[(current_index + 1) % scales.len()]
}

fn display_scale_label(profile: &DisplayProfile) -> String {
    if let Some(output) = profile
        .outputs
        .iter()
        .find(|output| output.output_id == profile.primary_output)
    {
        format!("{}% @ {}Hz", output.scale_100x, output.refresh_hz)
    } else {
        String::from("100% @ 60Hz")
    }
}

fn month_label(month: u8) -> &'static str {
    match month {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => "--",
    }
}

fn top_bar_status_summary(power_state: SessionPowerState) -> TopBarStatusSummary {
    let cpu_load = get_cpu_load(0);
    let cpu = if cpu_load.is_sign_negative() {
        0
    } else {
        (cpu_load + 0.5) as u32
    };
    let net = if get_ip().is_some() {
        String::from("NET UP")
    } else if let Some(gateway) = get_gateway() {
        format!("GW {:02}", gateway[3])
    } else {
        String::from("NET OFF")
    };
    let wg = wireguard_runtime_status();
    let vpn = if wg.active_devices > 0 {
        if wg.established_peers > 0 {
            String::from("VPN ON")
        } else if wg.devices > 0 {
            String::from("VPN IDLE")
        } else {
            String::from("VPN ON")
        }
    } else {
        String::from("VPN OFF")
    };
    let aud = if let Some(mixer) = get_mixer() {
        if mixer.master_muted {
            String::from("MUTED")
        } else {
            format!("VOL {}", mixer.master_volume.min(99))
        }
    } else if let Some((volume, muted)) = primary_controller_status() {
        if muted {
            String::from("MUTED")
        } else {
            format!("VOL {}", volume.min(99))
        }
    } else {
        String::from("AUD --")
    };
    let dt = get_cached_datetime();
    TopBarStatusSummary {
        net,
        cpu: format!("CPU {}%", cpu.min(99)),
        vpn,
        aud,
        time: format!(
            "{:02}:{:02}  {:02} {}",
            dt.hour,
            dt.minute,
            dt.day,
            month_label(dt.month)
        ),
        power: match power_state {
            SessionPowerState::Active => String::from("PWR ON"),
            SessionPowerState::Locked => String::from("LOCK"),
            SessionPowerState::Suspended => String::from("SLEEP"),
        },
    }
}

fn push_scene_icon(
    scene: &mut SceneGraph,
    parent: SceneNodeId,
    kind: AppKind,
    rect: Rect,
    accent: u32,
) {
    let inset = 8;
    let inner = Rect::new(
        rect.x + inset,
        rect.y + inset,
        rect.width.saturating_sub((inset as u32) * 2),
        rect.height.saturating_sub((inset as u32) * 2),
    );
    emit_desktop_icon_rects(kind.icon(), inner, |segment| {
        push_scene_round_rect(scene, parent, segment, accent, 2);
    });
}

fn apply_scene_overlay_transform(
    mut scene: SceneUpdate,
    opacity: f32,
    slide_y: i32,
) -> SceneUpdate {
    let alpha = (opacity.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    for object in scene.render_objects.iter_mut() {
        object.opacity = ((object.opacity as u16 * alpha as u16) / 255) as u8;
        object.bounds.y = object.bounds.y.saturating_add(slide_y);
        if let Some(clip) = object.clip.as_mut() {
            clip.y = clip.y.saturating_add(slide_y);
        }
    }
    for rect in scene.damage_hint.iter_mut() {
        rect.y = rect.y.saturating_add(slide_y);
    }
    scene
}

fn build_top_bar_scene(
    window_id: WindowId,
    text_system: &mut TextSystem,
    width: usize,
    height: usize,
    active_workspace: u8,
    snapshot: &SessionSnapshot,
    theme_mode: ThemeMode,
) -> SceneUpdate {
    let palette = hybrid_titan_palette(theme_mode);
    let bounds = Rect::new(0, 0, width as u32, height as u32);
    let mut scene = SceneGraph::new(bounds);
    scene.set_semantic_root(Some(window_id));
    let root = scene.root();
    let layout = top_bar_layout(width as i32);
    let status = top_bar_status_summary(snapshot.power_state);

    push_scene_panel(
        &mut scene,
        root,
        Rect::new(0, 0, width as u32, height as u32),
        0xE1081018,
        0x12243446,
        0,
        None,
    );

    push_scene_text(
        &mut scene,
        text_system,
        root,
        24,
        22,
        180,
        "echOS",
        palette.text_primary,
    );
    push_scene_rect(
        &mut scene,
        root,
        Rect::new(
            layout.apps_rect.x,
            layout.apps_rect.bottom() - 2,
            layout.apps_rect.width,
            2,
        ),
        palette.accent_blue,
    );
    push_scene_text(
        &mut scene,
        text_system,
        root,
        layout.apps_rect.x,
        layout.apps_rect.y + 12,
        layout.apps_rect.width,
        "Apps",
        palette.text_primary,
    );

    let workspace_start = ((active_workspace as usize) / 4) * 4;
    let workspace_rects = layout_flex(
        layout.workspace_rect,
        FlexDirection::Row,
        EdgeInsets {
            left: 0,
            top: 1,
            right: 0,
            bottom: 1,
        },
        10,
        &[
            FlexItem::fixed(48),
            FlexItem::fixed(48),
            FlexItem::fixed(48),
            FlexItem::fixed(48),
        ],
    );
    for (index, rect) in workspace_rects.iter().enumerate() {
        let workspace_id = (workspace_start + index).min(WORKSPACE_COUNT as usize - 1) as u8;
        let active = workspace_id == active_workspace;
        if active {
            push_scene_rect(
                &mut scene,
                root,
                Rect::new(
                    rect.x - 4,
                    rect.y + 4,
                    rect.width + 8,
                    rect.height.saturating_sub(8),
                ),
                0x7A101A25,
            );
        }
        push_scene_rect(
            &mut scene,
            root,
            Rect::new(rect.x, rect.bottom() - 2, rect.width, 2),
            if active {
                palette.accent_mint
            } else {
                0xFF1C2B39
            },
        );
        push_scene_text(
            &mut scene,
            text_system,
            root,
            rect.x + 2,
            rect.y + 12,
            rect.width.saturating_sub(4),
            top_bar_workspace_label(workspace_id),
            if active {
                palette.text_primary
            } else {
                palette.text_muted
            },
        );
    }

    push_scene_text(
        &mut scene,
        text_system,
        root,
        layout.command_rect.x,
        layout.command_rect.y + 12,
        layout.command_rect.width,
        "Desktop",
        palette.text_muted,
    );

    let right_labels = [
        (status.net.as_str(), palette.accent_blue),
        (status.cpu.as_str(), palette.accent_soft),
        (status.vpn.as_str(), palette.accent_blue),
        (status.aud.as_str(), palette.accent_mint),
        (status.power.as_str(), palette.accent_gold),
    ];
    for (rect, (label, accent)) in layout.status_rects.iter().zip(right_labels.iter()) {
        push_scene_rect(
            &mut scene,
            root,
            Rect::new(rect.x, rect.y + 10, 1, rect.height.saturating_sub(20)),
            0xFF213445,
        );
        push_scene_rect(
            &mut scene,
            root,
            Rect::new(rect.x + 8, rect.y + 15, 4, 4),
            *accent,
        );
        push_scene_text(
            &mut scene,
            text_system,
            root,
            rect.x + 16,
            rect.y + 12,
            rect.width.saturating_sub(18),
            label,
            if *accent == palette.accent_gold {
                palette.text_primary
            } else {
                palette.text_secondary
            },
        );
    }
    push_scene_rect(
        &mut scene,
        root,
        Rect::new(
            layout.time_rect.x,
            layout.time_rect.bottom() - 2,
            layout.time_rect.width,
            2,
        ),
        palette.accent_gold,
    );
    push_scene_text(
        &mut scene,
        text_system,
        root,
        layout.time_rect.x + 16,
        layout.time_rect.y + 12,
        layout.time_rect.width.saturating_sub(32),
        &status.time,
        palette.text_primary,
    );

    let mut update = scene.snapshot(window_id);
    update.damage_hint = vec![bounds];
    update
}

fn build_task_strip_scene(
    window_id: WindowId,
    text_system: &mut TextSystem,
    width: usize,
    height: usize,
    snapshots: &[AppSnapshot],
    active_workspace: u8,
    theme_mode: ThemeMode,
) -> SceneUpdate {
    let palette = hybrid_titan_palette(theme_mode);
    let bounds = Rect::new(0, 0, width as u32, height as u32);
    let mut scene = SceneGraph::new(bounds);
    scene.set_semantic_root(Some(window_id));
    let root = scene.root();

    let dock_width = 354u32.min(width as u32);
    let dock_rect = Rect::new(
        ((width as i32 - dock_width as i32) / 2).max(0),
        0,
        dock_width,
        height as u32,
    );
    push_scene_panel(
        &mut scene,
        root,
        dock_rect,
        0xD1091018,
        palette.border,
        22,
        None,
    );

    let icons_rect = layout_flex(
        Rect::new(
            dock_rect.x + 19,
            dock_rect.y + 14,
            dock_rect.width.saturating_sub(38),
            52,
        ),
        FlexDirection::Row,
        EdgeInsets::default(),
        14,
        &[
            FlexItem::fixed(52),
            FlexItem::fixed(52),
            FlexItem::fixed(52),
            FlexItem::fixed(52),
            FlexItem::fixed(52),
        ],
    );
    let mut focused_kind = None;
    for snapshot in snapshots.iter() {
        if snapshot.focused || (snapshot.workspace_id == active_workspace && snapshot.visible) {
            focused_kind = Some(snapshot.kind);
            if snapshot.focused {
                break;
            }
        }
    }
    let dock_items = [
        (Some(AppKind::Terminal), palette.accent_mint),
        (Some(AppKind::Files), palette.accent_blue),
        (Some(AppKind::Settings), palette.accent_gold),
        (Some(AppKind::Editor), palette.accent_coral),
        (
            None,
            if snapshots.iter().any(|s| s.needs_attention) {
                palette.accent_gold
            } else {
                palette.text_secondary
            },
        ),
    ];
    for (index, rect) in icons_rect.iter().enumerate() {
        let (kind, accent) = dock_items[index];
        let active = kind.is_some() && kind == focused_kind;
        push_scene_panel(
            &mut scene,
            root,
            *rect,
            0xF1111C29,
            palette.border,
            16,
            if active { Some(accent) } else { None },
        );
        if let Some(kind) = kind {
            push_scene_icon(
                &mut scene,
                root,
                kind,
                *rect,
                if active { palette.text_primary } else { accent },
            );
        } else {
            let badge = Rect::new(
                rect.x + 10,
                rect.y + 10,
                rect.width.saturating_sub(20),
                rect.height.saturating_sub(20),
            );
            emit_desktop_icon_rects(DesktopIconKind::Alerts, badge, |segment| {
                push_scene_round_rect(&mut scene, root, segment, accent, 2);
            });
        }
    }

    let mut update = scene.snapshot(window_id);
    update.damage_hint = vec![bounds];
    update
}

fn build_launcher_scene(
    window_id: WindowId,
    text_system: &mut TextSystem,
    width: usize,
    height: usize,
    snapshots: &[AppSnapshot],
    session: &SessionSnapshot,
    notices: &[String],
    theme_mode: ThemeMode,
    layout_profile: ShellLayoutProfile,
) -> SceneUpdate {
    let palette = hybrid_titan_palette(theme_mode);
    let bounds = Rect::new(0, 0, width as u32, height as u32);
    let mut scene = SceneGraph::new(bounds);
    scene.set_semantic_root(Some(window_id));
    let root = scene.root();
    let _ = layout_profile;
    push_scene_panel(
        &mut scene,
        root,
        bounds,
        0xE80B121C,
        palette.border,
        24,
        Some(palette.accent_blue),
    );
    push_scene_text(
        &mut scene,
        text_system,
        root,
        18,
        18,
        width.saturating_sub(36) as u32,
        "Launcher",
        palette.text_primary,
    );
    push_scene_text(
        &mut scene,
        text_system,
        root,
        18,
        40,
        width.saturating_sub(36) as u32,
        "Dock-first launch, recent actions, and app restore.",
        palette.text_muted,
    );

    let search_rect = Rect::new(18, 74, width.saturating_sub(36) as u32, 42);
    push_scene_panel(
        &mut scene,
        root,
        search_rect,
        0xC7091018,
        palette.border,
        14,
        None,
    );
    push_scene_text(
        &mut scene,
        text_system,
        root,
        search_rect.x + 14,
        search_rect.y + 12,
        search_rect.width.saturating_sub(28),
        "Search apps, files, commands, sessions",
        palette.text_muted,
    );

    let app_rects = layout_grid(
        Rect::new(18, 134, width.saturating_sub(36) as u32, 104),
        EdgeInsets::default(),
        1,
        0,
        12,
        4,
        92,
    );
    for (rect, kind) in app_rects.iter().zip(AppKind::ALL.iter()) {
        let snapshot = snapshots.iter().find(|snapshot| snapshot.kind == *kind);
        let active = snapshot
            .map(|s| s.focused || (s.visible && s.workspace_id == session.workspace_id))
            .unwrap_or(false);
        push_scene_panel(
            &mut scene,
            root,
            *rect,
            0xE0111C29,
            palette.border,
            18,
            if active { Some(kind.accent()) } else { None },
        );
        let icon_badge = Rect::new(rect.x + 16, rect.y + 12, 34, 34);
        push_scene_round_rect(&mut scene, root, icon_badge, 0xD00B1520, 10);
        push_scene_outline(
            &mut scene,
            root,
            icon_badge,
            if active {
                kind.accent()
            } else {
                palette.border
            },
        );
        push_scene_icon(
            &mut scene,
            root,
            *kind,
            Rect::new(icon_badge.x + 3, icon_badge.y + 3, 28, 28),
            if active {
                kind.accent()
            } else {
                palette.text_secondary
            },
        );
        push_scene_text(
            &mut scene,
            text_system,
            root,
            rect.x + 62,
            rect.y + 18,
            rect.width.saturating_sub(80),
            kind.title(),
            palette.text_primary,
        );
        push_scene_text(
            &mut scene,
            text_system,
            root,
            rect.x + 62,
            rect.y + 42,
            rect.width.saturating_sub(80),
            snapshot
                .map(|s| app_health_label(s.health, s.visible, s.focused))
                .unwrap_or("ready"),
            if active {
                palette.text_secondary
            } else {
                palette.text_muted
            },
        );
    }

    push_scene_text(
        &mut scene,
        text_system,
        root,
        18,
        262,
        width.saturating_sub(36) as u32,
        "Recent actions",
        palette.text_primary,
    );
    let recents = [
        "Launch Terminal",
        "Open Quick Settings",
        "Switch to Workspace 2",
        "Find build logs",
        "Toggle scratchpad",
    ];
    for (index, label) in recents.iter().enumerate() {
        let rect = Rect::new(
            18,
            292 + index as i32 * 54,
            width.saturating_sub(36) as u32,
            44,
        );
        push_scene_panel(
            &mut scene,
            root,
            rect,
            0xE00E1723,
            palette.border,
            14,
            if index == 0 {
                Some(palette.accent_blue)
            } else {
                None
            },
        );
        push_scene_text(
            &mut scene,
            text_system,
            root,
            rect.x + 14,
            rect.y + 13,
            rect.width.saturating_sub(110),
            label,
            palette.text_secondary,
        );
        push_scene_text(
            &mut scene,
            text_system,
            root,
            rect.right() - 64,
            rect.y + 13,
            50,
            if index == 0 { "Enter" } else { "Open" },
            palette.text_muted,
        );
    }

    if let Some(last) = notices.last() {
        push_scene_text(
            &mut scene,
            text_system,
            root,
            18,
            max(height as i32 - 34, 0),
            width.saturating_sub(36) as u32,
            last,
            palette.text_muted,
        );
    }
    scene.snapshot(window_id)
}

fn build_quick_settings_scene(
    window_id: WindowId,
    text_system: &mut TextSystem,
    width: usize,
    height: usize,
    theme_mode: ThemeMode,
    snapshot: &SessionSnapshot,
    notifications: bool,
) -> SceneUpdate {
    let palette = hybrid_titan_palette(theme_mode);
    let bounds = Rect::new(0, 0, width as u32, height as u32);
    let mut scene = SceneGraph::new(bounds);
    scene.set_semantic_root(Some(window_id));
    let root = scene.root();
    push_scene_panel(
        &mut scene,
        root,
        bounds,
        0xE80B121C,
        palette.border,
        24,
        Some(palette.accent_blue),
    );
    push_scene_text(
        &mut scene,
        text_system,
        root,
        18,
        18,
        width.saturating_sub(36) as u32,
        "Quick Settings",
        palette.text_primary,
    );
    push_scene_text(
        &mut scene,
        text_system,
        root,
        18,
        42,
        width.saturating_sub(36) as u32,
        "Utility overlays stay compact and quiet.",
        palette.text_muted,
    );
    let rows = vec![
        (
            "Theme",
            theme_mode_label(theme_mode).to_string(),
            palette.accent_blue,
        ),
        (
            "Notifications",
            if notifications {
                String::from("On")
            } else {
                String::from("Muted")
            },
            palette.accent_mint,
        ),
        (
            "Motion",
            motion_profile_label(snapshot.motion_profile).to_string(),
            palette.accent_gold,
        ),
        (
            "Display",
            display_scale_label(&snapshot.display_profile),
            palette.accent_soft,
        ),
        (
            "Screen Reader",
            if snapshot.accessibility_profile.screen_reader {
                String::from("On")
            } else {
                String::from("Off")
            },
            palette.text_secondary,
        ),
    ];
    for (index, (label, value, accent)) in rows.iter().enumerate() {
        let rect = Rect::new(
            18,
            86 + index as i32 * 58,
            width.saturating_sub(36) as u32,
            46,
        );
        push_scene_panel(
            &mut scene,
            root,
            rect,
            0xE00E1723,
            palette.border,
            14,
            Some(*accent),
        );
        push_scene_text(
            &mut scene,
            text_system,
            root,
            rect.x + 14,
            rect.y + 13,
            rect.width.saturating_sub(110),
            label,
            palette.text_primary,
        );
        push_scene_text(
            &mut scene,
            text_system,
            root,
            rect.right() - 96,
            rect.y + 13,
            84,
            value,
            palette.text_secondary,
        );
    }
    scene.snapshot(window_id)
}

fn build_stage_rail_scene(
    window_id: WindowId,
    text_system: &mut TextSystem,
    width: usize,
    height: usize,
    stage_sets: &[StageSet],
    active_stage_set: usize,
    theme_mode: ThemeMode,
) -> SceneUpdate {
    let palette = hybrid_titan_palette(theme_mode);
    let bounds = Rect::new(0, 0, width as u32, height as u32);
    let mut scene = SceneGraph::new(bounds);
    scene.set_semantic_root(Some(window_id));
    let root = scene.root();
    push_scene_panel(
        &mut scene,
        root,
        bounds,
        0xE80B121C,
        palette.border,
        24,
        Some(palette.accent_blue),
    );
    push_scene_text(
        &mut scene,
        text_system,
        root,
        14,
        18,
        width.saturating_sub(28) as u32,
        "Workspaces",
        palette.text_primary,
    );
    push_scene_text(
        &mut scene,
        text_system,
        root,
        14,
        40,
        width.saturating_sub(28) as u32,
        "Overview as a true workspace control surface",
        palette.text_muted,
    );
    let cards = layout_grid(
        Rect::new(
            18,
            82,
            width.saturating_sub(36) as u32,
            height.saturating_sub(100) as u32,
        ),
        EdgeInsets::default(),
        2,
        0,
        18,
        stage_sets.len().max(1).min(4),
        196,
    );
    for (index, stage) in stage_sets.iter().take(cards.len()).enumerate() {
        let rect = cards[index];
        let selected = index == active_stage_set;
        push_scene_panel(
            &mut scene,
            root,
            rect,
            if selected { 0xF2142433 } else { 0xE0111C29 },
            palette.border,
            20,
            if selected {
                Some(palette.accent_mint)
            } else {
                None
            },
        );
        push_scene_text(
            &mut scene,
            text_system,
            root,
            rect.x + 16,
            rect.y + 18,
            rect.width.saturating_sub(32),
            &format!("Workspace {}", index + 1),
            palette.text_primary,
        );
        push_scene_text(
            &mut scene,
            text_system,
            root,
            rect.x + 16,
            rect.y + 42,
            rect.width.saturating_sub(32),
            &format!(
                "{} · layout: {}{}",
                stage.name,
                workspace_layout_label(if selected {
                    WorkspaceLayout::Overview
                } else {
                    WorkspaceLayout::Dwindle
                }),
                if stage.pinned { " · pinned" } else { "" }
            ),
            palette.text_secondary,
        );
        let thumb = Rect::new(
            rect.x + 16,
            rect.y + 78,
            rect.width.saturating_sub(32),
            rect.height.saturating_sub(94),
        );
        push_scene_panel(
            &mut scene,
            root,
            thumb,
            0xD00C1722,
            palette.border,
            16,
            None,
        );
        let left = Rect::new(
            thumb.x + 14,
            thumb.y + 16,
            thumb.width / 2,
            thumb.height.saturating_sub(32),
        );
        push_scene_round_rect(&mut scene, root, left, 0xF018293B, 14);
        if stage.window_ids.len() > 1 {
            let right_top = Rect::new(
                thumb
                    .right()
                    .saturating_sub((thumb.width / 4) as i32)
                    .saturating_sub(14),
                thumb.y + 16,
                thumb.width / 4,
                thumb.height / 2 - 10,
            );
            let right_bottom = Rect::new(
                right_top.x,
                right_top.bottom() + 10,
                right_top.width,
                right_top.height,
            );
            push_scene_round_rect(&mut scene, root, right_top, 0xEE142233, 12);
            push_scene_round_rect(&mut scene, root, right_bottom, 0xEE142233, 12);
        }
    }
    scene.snapshot(window_id)
}

fn build_context_menu_scene(
    window_id: WindowId,
    text_system: &mut TextSystem,
    width: usize,
    height: usize,
    target: Option<AppKind>,
    theme_mode: ThemeMode,
) -> SceneUpdate {
    let palette = hybrid_titan_palette(theme_mode);
    let bounds = Rect::new(0, 0, width as u32, height as u32);
    let mut scene = SceneGraph::new(bounds);
    scene.set_semantic_root(Some(window_id));
    let root = scene.root();
    push_scene_rect(&mut scene, root, bounds, palette.window_bg);
    push_scene_rect(
        &mut scene,
        root,
        Rect::new(0, 0, width as u32, 36),
        palette.panel_bg,
    );
    push_scene_rect(
        &mut scene,
        root,
        Rect::new(0, 36, width as u32, 1),
        palette.border,
    );
    push_scene_text(
        &mut scene,
        text_system,
        root,
        16,
        12,
        width.saturating_sub(128) as u32,
        "Window Actions",
        palette.text_primary,
    );
    push_scene_text(
        &mut scene,
        text_system,
        root,
        width as i32 - 96,
        12,
        92,
        target.map(|kind| kind.title()).unwrap_or("No Target"),
        palette.text_muted,
    );
    let rows = [
        ("1 Focus / Restore", ContextAction::Focus),
        ("2 Minimize", ContextAction::Minimize),
        ("3 Snap Left", ContextAction::SnapLeft),
        ("4 Snap Right", ContextAction::SnapRight),
        ("5 Maximize", ContextAction::Maximize),
        ("6 Move Next Space", ContextAction::MoveNextWorkspace),
        ("7 Close Window", ContextAction::Close),
    ];
    for (index, (label, action)) in rows.iter().enumerate() {
        let rect = Rect::new(16, 52 + index as i32 * 24, 208, 22);
        push_scene_rect(&mut scene, root, rect, palette.panel_bg);
        push_scene_outline(
            &mut scene,
            root,
            rect,
            if matches!(action, ContextAction::Close) {
                palette.accent_coral
            } else {
                palette.border
            },
        );
        push_scene_text(
            &mut scene,
            text_system,
            root,
            rect.x + 10,
            rect.y + 6,
            rect.width.saturating_sub(20),
            label,
            palette.text_secondary,
        );
    }
    scene.snapshot(window_id)
}

fn build_switcher_scene(
    window_id: WindowId,
    text_system: &mut TextSystem,
    width: usize,
    height: usize,
    candidates: &[AppKind],
    selected_index: usize,
    active_workspace: u8,
    theme_mode: ThemeMode,
) -> SceneUpdate {
    let palette = hybrid_titan_palette(theme_mode);
    let bounds = Rect::new(0, 0, width as u32, height as u32);
    let mut scene = SceneGraph::new(bounds);
    scene.set_semantic_root(Some(window_id));
    let root = scene.root();
    push_scene_rect(&mut scene, root, bounds, palette.window_bg);
    push_scene_rect(
        &mut scene,
        root,
        Rect::new(0, 0, width as u32, 44),
        palette.panel_bg,
    );
    push_scene_rect(
        &mut scene,
        root,
        Rect::new(0, 44, width as u32, 1),
        palette.border,
    );
    push_scene_text(
        &mut scene,
        text_system,
        root,
        18,
        14,
        width.saturating_sub(36) as u32,
        "App Switcher",
        palette.text_primary,
    );
    push_scene_text(
        &mut scene,
        text_system,
        root,
        18,
        28,
        width.saturating_sub(36) as u32,
        "Alt+Tab cycles, Alt release confirms",
        palette.text_muted,
    );
    for (index, kind) in candidates.iter().enumerate() {
        let rect = Rect::new(24, 72 + index as i32 * 36, 392, 30);
        let selected = index == selected_index.min(candidates.len().saturating_sub(1));
        push_scene_rect(
            &mut scene,
            root,
            rect,
            if selected {
                palette.panel_alt
            } else {
                palette.panel_bg
            },
        );
        push_scene_outline(
            &mut scene,
            root,
            rect,
            if selected {
                kind.accent()
            } else {
                palette.border
            },
        );
        push_scene_rect(
            &mut scene,
            root,
            Rect::new(rect.x + 10, rect.y + 10, 8, 8),
            kind.accent(),
        );
        push_scene_text(
            &mut scene,
            text_system,
            root,
            rect.x + 28,
            rect.y + 7,
            rect.width.saturating_sub(148),
            kind.title(),
            palette.text_primary,
        );
        push_scene_text(
            &mut scene,
            text_system,
            root,
            rect.right() - 110,
            rect.y + 7,
            100,
            if index == 0 { "current ring" } else { "queued" },
            palette.text_muted,
        );
    }
    push_scene_text(
        &mut scene,
        text_system,
        root,
        18,
        height as i32 - 24,
        width.saturating_sub(36) as u32,
        &format!("workspace {}", active_workspace.saturating_add(1)),
        palette.text_secondary,
    );
    scene.snapshot(window_id)
}

fn build_dialog_scene(
    window_id: WindowId,
    text_system: &mut TextSystem,
    width: usize,
    height: usize,
    pending_dialog: Option<&DialogRequest>,
    dialog_input: &str,
    theme_mode: ThemeMode,
) -> SceneUpdate {
    let palette = hybrid_titan_palette(theme_mode);
    let bounds = Rect::new(0, 0, width as u32, height as u32);
    let mut scene = SceneGraph::new(bounds);
    scene.set_semantic_root(Some(window_id));
    let root = scene.root();
    push_scene_rect(&mut scene, root, bounds, palette.window_bg);
    push_scene_rect(
        &mut scene,
        root,
        Rect::new(0, 0, width as u32, 58),
        palette.panel_bg,
    );
    push_scene_rect(
        &mut scene,
        root,
        Rect::new(0, 58, width as u32, 1),
        palette.border,
    );
    push_scene_text(
        &mut scene,
        text_system,
        root,
        18,
        18,
        width.saturating_sub(36) as u32,
        "Dialog Broker",
        palette.text_primary,
    );
    push_scene_text(
        &mut scene,
        text_system,
        root,
        18,
        36,
        width.saturating_sub(36) as u32,
        "Shell mediates file and message dialogs",
        palette.text_muted,
    );
    if let Some(request) = pending_dialog {
        let kind = match request.kind {
            DialogKind::OpenFile => "Open file",
            DialogKind::SaveFile => "Save file",
            DialogKind::PickFolder => "Pick folder",
            DialogKind::Message => "Message",
        };
        push_scene_text(
            &mut scene,
            text_system,
            root,
            18,
            78,
            width.saturating_sub(36) as u32,
            &request.title,
            palette.text_primary,
        );
        push_scene_text(
            &mut scene,
            text_system,
            root,
            18,
            98,
            width.saturating_sub(36) as u32,
            kind,
            palette.accent_blue,
        );
        if !request.message.is_empty() {
            for (index, line) in request.message.lines().take(4).enumerate() {
                push_scene_text(
                    &mut scene,
                    text_system,
                    root,
                    18,
                    118 + index as i32 * 18,
                    width.saturating_sub(36) as u32,
                    line,
                    palette.text_secondary,
                );
            }
        } else {
            push_scene_text(
                &mut scene,
                text_system,
                root,
                18,
                118,
                width.saturating_sub(36) as u32,
                "Path",
                palette.text_muted,
            );
            let input_rect = Rect::new(18, 136, width as u32 - 36, 26);
            push_scene_rect(&mut scene, root, input_rect, palette.panel_bg);
            push_scene_outline(&mut scene, root, input_rect, palette.accent_mint);
            push_scene_text(
                &mut scene,
                text_system,
                root,
                26,
                144,
                input_rect.width.saturating_sub(16),
                dialog_input,
                palette.text_secondary,
            );
        }
    } else {
        push_scene_text(
            &mut scene,
            text_system,
            root,
            18,
            92,
            width.saturating_sub(36) as u32,
            "No pending dialogs",
            palette.text_secondary,
        );
    }
    let cancel = Rect::new(36, 126, 134, 40);
    let accept = Rect::new(190, 126, 134, 40);
    push_scene_rect(&mut scene, root, cancel, palette.panel_bg);
    push_scene_outline(&mut scene, root, cancel, palette.border);
    push_scene_text(
        &mut scene,
        text_system,
        root,
        cancel.x + 40,
        cancel.y + 12,
        64,
        "Cancel",
        palette.text_secondary,
    );
    push_scene_rect(&mut scene, root, accept, palette.panel_alt);
    push_scene_outline(&mut scene, root, accept, palette.accent_mint);
    push_scene_text(
        &mut scene,
        text_system,
        root,
        accept.x + 40,
        accept.y + 12,
        64,
        "Accept",
        palette.text_primary,
    );
    scene.snapshot(window_id)
}

fn build_notifications_scene(
    window_id: WindowId,
    text_system: &mut TextSystem,
    width: usize,
    height: usize,
    notices: &[NotificationEntry],
    selected_index: usize,
    theme_mode: ThemeMode,
) -> SceneUpdate {
    let palette = hybrid_titan_palette(theme_mode);
    let bounds = Rect::new(0, 0, width as u32, height as u32);
    let mut scene = SceneGraph::new(bounds);
    scene.set_semantic_root(Some(window_id));
    let root = scene.root();
    push_scene_panel(
        &mut scene,
        root,
        bounds,
        0xE80B121C,
        palette.border,
        24,
        Some(palette.accent_blue),
    );
    push_scene_text(
        &mut scene,
        text_system,
        root,
        18,
        18,
        width.saturating_sub(36) as u32,
        "Notifications",
        palette.text_primary,
    );
    push_scene_text(
        &mut scene,
        text_system,
        root,
        18,
        42,
        width.saturating_sub(36) as u32,
        "Recent shell activity",
        palette.text_muted,
    );
    let mut y = 86;
    for (index, notice) in notices.iter().rev().take(6).rev().enumerate() {
        let rect = Rect::new(18, y, width.saturating_sub(36) as u32, 46);
        let accent = match notice.level {
            NotificationLevel::Info => palette.border,
            NotificationLevel::Success => palette.accent_soft,
            NotificationLevel::Warning => palette.accent_gold,
            NotificationLevel::Error => palette.accent_coral,
        };
        push_scene_panel(
            &mut scene,
            root,
            rect,
            if index == selected_index {
                0xF2142433
            } else {
                0xE00E1723
            },
            palette.border,
            14,
            Some(accent),
        );
        push_scene_text(
            &mut scene,
            text_system,
            root,
            rect.x + 14,
            rect.y + 13,
            rect.width.saturating_sub(126),
            &notice.title,
            palette.text_primary,
        );
        push_scene_text(
            &mut scene,
            text_system,
            root,
            rect.right() - 48,
            rect.y + 13,
            40,
            notification_level_label(notice.level),
            if notice.read {
                palette.text_muted
            } else {
                accent
            },
        );
        y += 58;
    }
    scene.snapshot(window_id)
}

fn build_lock_scene(
    window_id: WindowId,
    text_system: &mut TextSystem,
    width: usize,
    height: usize,
    auth_input: &str,
    logged_in: bool,
    theme_mode: ThemeMode,
) -> SceneUpdate {
    let palette = hybrid_titan_palette(theme_mode);
    let bounds = Rect::new(0, 0, width as u32, height as u32);
    let mut scene = SceneGraph::new(bounds);
    scene.set_semantic_root(Some(window_id));
    let root = scene.root();
    push_scene_rect(&mut scene, root, bounds, palette.window_bg);
    push_scene_rect(
        &mut scene,
        root,
        Rect::new(0, 0, width as u32, 64),
        palette.panel_bg,
    );
    push_scene_rect(
        &mut scene,
        root,
        Rect::new(0, 64, width as u32, 1),
        palette.border,
    );
    push_scene_text(
        &mut scene,
        text_system,
        root,
        24,
        20,
        width.saturating_sub(48) as u32,
        if logged_in { "Unlock" } else { "Login" },
        palette.text_primary,
    );
    push_scene_text(
        &mut scene,
        text_system,
        root,
        24,
        40,
        width.saturating_sub(48) as u32,
        "Native session authentication gate",
        palette.text_muted,
    );
    let panel = Rect::new(40, 100, (width as u32).saturating_sub(80), 136);
    push_scene_rect(&mut scene, root, panel, palette.panel_bg);
    push_scene_outline(&mut scene, root, panel, palette.accent_blue);
    push_scene_text(
        &mut scene,
        text_system,
        root,
        60,
        126,
        panel.width.saturating_sub(120),
        "User: operator",
        palette.text_secondary,
    );
    push_scene_text(
        &mut scene,
        text_system,
        root,
        60,
        152,
        panel.width.saturating_sub(120),
        "Password",
        palette.text_primary,
    );
    let input_rect = Rect::new(60, 176, panel.width.saturating_sub(120), 34);
    push_scene_rect(&mut scene, root, input_rect, palette.window_bg);
    push_scene_outline(&mut scene, root, input_rect, palette.accent_mint);
    let masked = "*".repeat(auth_input.len().max(1));
    push_scene_text(
        &mut scene,
        text_system,
        root,
        74,
        186,
        input_rect.width.saturating_sub(20),
        &masked,
        palette.text_primary,
    );
    push_scene_text(
        &mut scene,
        text_system,
        root,
        60,
        224,
        panel.width.saturating_sub(120),
        "Enter = unlock, Backspace = delete",
        palette.text_muted,
    );
    push_scene_text(
        &mut scene,
        text_system,
        root,
        60,
        246,
        panel.width.saturating_sub(120),
        "Default password: echos",
        palette.accent_gold,
    );
    scene.snapshot(window_id)
}

fn build_command_palette_scene(
    window_id: WindowId,
    text_system: &mut TextSystem,
    width: usize,
    height: usize,
    actions: &[CommandPaletteAction],
    query: &str,
    selected_index: usize,
    theme_mode: ThemeMode,
) -> SceneUpdate {
    let palette = hybrid_titan_palette(theme_mode);
    let bounds = Rect::new(0, 0, width as u32, height as u32);
    let mut scene = SceneGraph::new(bounds);
    scene.set_semantic_root(Some(window_id));
    let root = scene.root();
    push_scene_panel(
        &mut scene,
        root,
        bounds,
        0xE80B121C,
        palette.border,
        24,
        Some(palette.accent_blue),
    );
    push_scene_text(
        &mut scene,
        text_system,
        root,
        18,
        18,
        width.saturating_sub(36) as u32,
        "Command Palette",
        palette.text_primary,
    );
    push_scene_text(
        &mut scene,
        text_system,
        root,
        18,
        42,
        width.saturating_sub(36) as u32,
        "One entry point for apps, commands, recent actions, and workspace moves.",
        palette.text_muted,
    );
    let query_rect = Rect::new(18, 82, width.saturating_sub(36) as u32, 42);
    push_scene_panel(
        &mut scene,
        root,
        query_rect,
        0xC7091018,
        palette.border,
        14,
        None,
    );
    push_scene_text(
        &mut scene,
        text_system,
        root,
        query_rect.x + 14,
        query_rect.y + 12,
        query_rect.width.saturating_sub(20),
        if query.is_empty() {
            "Type to filter actions"
        } else {
            query
        },
        if query.is_empty() {
            palette.text_muted
        } else {
            palette.text_primary
        },
    );
    for (index, action) in actions.iter().take(6).enumerate() {
        let selected = index == selected_index.min(actions.len().saturating_sub(1));
        let rect = Rect::new(
            18,
            138 + index as i32 * 54,
            width.saturating_sub(36) as u32,
            44,
        );
        push_scene_panel(
            &mut scene,
            root,
            rect,
            if selected { 0xF2142433 } else { 0xE00E1723 },
            palette.border,
            14,
            if selected {
                Some(palette.accent_blue)
            } else {
                None
            },
        );
        push_scene_text(
            &mut scene,
            text_system,
            root,
            rect.x + 14,
            rect.y + 13,
            108,
            &action.category,
            palette.text_muted,
        );
        push_scene_text(
            &mut scene,
            text_system,
            root,
            rect.x + 126,
            rect.y + 13,
            rect.width.saturating_sub(252),
            &action.title,
            palette.text_primary,
        );
        push_scene_text(
            &mut scene,
            text_system,
            root,
            rect.right() - 120,
            rect.y + 13,
            112,
            &action.shortcut,
            palette.text_secondary,
        );
    }
    if actions.is_empty() {
        push_scene_text(
            &mut scene,
            text_system,
            root,
            24,
            92,
            width.saturating_sub(48) as u32,
            "No matching actions",
            palette.text_secondary,
        );
    }
    scene.snapshot(window_id)
}

fn paint_launcher_surface(
    width: usize,
    height: usize,
    snapshots: &[AppSnapshot],
    session: &SessionSnapshot,
    notices: &[String],
    theme_mode: ThemeMode,
    layout_profile: ShellLayoutProfile,
) -> Vec<u32> {
    let palette = hybrid_titan_palette(theme_mode);
    let mut canvas = Canvas::new(width, height, palette.window_bg);
    canvas.fill_rect(Rect::new(0, 0, width as u32, 64), palette.panel_bg);
    canvas.fill_rect(Rect::new(0, 64, width as u32, 1), palette.border);
    canvas.draw_text(18, 18, "Desktop Launcher", palette.text_primary);
    canvas.draw_text(
        18,
        38,
        if session.boot_clean_desktop {
            "Clean desktop ready. Launch only what you need."
        } else {
            "Launch, focus, or restore native apps"
        },
        palette.text_muted,
    );
    canvas.draw_text(
        width as i32 - 172,
        18,
        match layout_profile {
            ShellLayoutProfile::Desktop => "Layout Desktop",
            ShellLayoutProfile::Compact => "Layout Compact",
        },
        palette.text_muted,
    );

    if session.boot_clean_desktop {
        let hero = Rect::new(18, 92, width.saturating_sub(36) as u32, 74);
        canvas.fill_rect(hero, palette.panel_bg);
        canvas.stroke_rect(hero, palette.accent_mint);
        canvas.draw_text(
            hero.x + 14,
            hero.y + 14,
            "Zero-app startup",
            palette.text_primary,
        );
        canvas.draw_text(
            hero.x + 14,
            hero.y + 34,
            "Workspace 1 is active, overlays are quiet, shell is accepting input.",
            palette.text_secondary,
        );
        canvas.draw_text(
            hero.x + 14,
            hero.y + 52,
            "Super+Enter terminal  Super+Space palette  Super+, settings  Super+` overview",
            palette.text_muted,
        );
    }

    let mut y = if session.boot_clean_desktop { 184 } else { 92 };
    for snapshot in snapshots.iter() {
        let rect = Rect::new(18, y, width.saturating_sub(36) as u32, 54);
        canvas.fill_rect(rect, palette.panel_bg);
        canvas.stroke_rect(
            rect,
            app_health_color(
                snapshot.health,
                snapshot.needs_attention,
                snapshot.kind.accent(),
            ),
        );
        canvas.fill_rect(
            Rect::new(rect.x + 12, rect.y + 12, 10, 10),
            snapshot.kind.accent(),
        );
        canvas.draw_text(
            rect.x + 32,
            rect.y + 10,
            snapshot.kind.title(),
            palette.text_primary,
        );
        canvas.draw_text(
            rect.x + 32,
            rect.y + 28,
            &snapshot.detail,
            palette.text_secondary,
        );
        let state = if snapshot.launch_count == 0 && !snapshot.visible {
            "ready"
        } else {
            app_health_label(snapshot.health, snapshot.visible, snapshot.focused)
        };
        canvas.draw_text(rect.right() - 96, rect.y + 10, state, palette.text_muted);
        canvas.draw_text(
            rect.right() - 112,
            rect.y + 26,
            &format!("L{} C{}", snapshot.launch_count, snapshot.crash_count),
            if snapshot.crash_count > 0 {
                palette.accent_coral
            } else {
                palette.text_muted
            },
        );
        canvas.draw_text(
            rect.right() - 42,
            rect.y + 18,
            &snapshot.kind.shortcut().to_string(),
            palette.text_muted,
        );
        y += 66;
    }

    let footer_y = max(height as i32 - 52, 0);
    canvas.fill_rect(Rect::new(0, footer_y, width as u32, 52), palette.panel_alt);
    canvas.fill_rect(Rect::new(0, footer_y, width as u32, 1), palette.border);
    if let Some(last) = notices.last() {
        canvas.draw_text(18, footer_y + 12, last, palette.text_secondary);
    }
    canvas.draw_text(
        18,
        footer_y + 28,
        if session.shell_ready {
            "Shell ready, clean desktop preserved until explicit launch"
        } else {
            "Shell runtime not ready"
        },
        palette.text_muted,
    );
    canvas.into_pixels()
}

fn paint_top_bar_surface(
    width: usize,
    height: usize,
    active_workspace: u8,
    snapshot: &SessionSnapshot,
    theme_mode: ThemeMode,
) -> Vec<u32> {
    let palette = hybrid_titan_palette(theme_mode);
    let layout = top_bar_layout(width as i32);
    let status = top_bar_status_summary(snapshot.power_state);
    let mut canvas = Canvas::new(width, height, palette.panel_bg);
    canvas.fill_rect(Rect::new(0, 0, width as u32, height as u32), 0xE1081018);
    canvas.fill_rect(
        Rect::new(0, height as i32 - 1, width as u32, 1),
        palette.border,
    );
    canvas_draw_top_bar_text(&mut canvas, 24, 22, "echOS", palette.text_primary);

    canvas.fill_rect(
        Rect::new(
            layout.apps_rect.x,
            layout.apps_rect.bottom() - 2,
            layout.apps_rect.width,
            2,
        ),
        palette.accent_blue,
    );
    canvas_draw_top_bar_centered_text(&mut canvas, layout.apps_rect, "Apps", palette.text_primary);

    let workspace_start = ((active_workspace as usize) / 4) * 4;
    let workspace_rects = layout_flex(
        layout.workspace_rect,
        FlexDirection::Row,
        EdgeInsets {
            left: 0,
            top: 1,
            right: 0,
            bottom: 1,
        },
        10,
        &[
            FlexItem::fixed(48),
            FlexItem::fixed(48),
            FlexItem::fixed(48),
            FlexItem::fixed(48),
        ],
    );
    for (slot, rect) in workspace_rects.iter().enumerate() {
        let workspace_id = (workspace_start + slot).min(WORKSPACE_COUNT as usize - 1) as u8;
        let active = workspace_id == active_workspace;
        if active {
            canvas.fill_rect(
                Rect::new(
                    rect.x - 4,
                    rect.y + 4,
                    rect.width + 8,
                    rect.height.saturating_sub(8),
                ),
                0x7A101A25,
            );
        }
        canvas.fill_rect(
            Rect::new(rect.x, rect.bottom() - 2, rect.width, 2),
            if active {
                palette.accent_mint
            } else {
                0xFF1C2B39
            },
        );
        canvas_draw_top_bar_centered_text(
            &mut canvas,
            *rect,
            top_bar_workspace_label(workspace_id),
            if active {
                palette.text_primary
            } else {
                palette.text_secondary
            },
        );
    }
    let command_rect = layout.command_rect;
    canvas_draw_top_bar_text(
        &mut canvas,
        command_rect.x,
        command_rect.y + ((command_rect.height as i32 - FONT_HEIGHT) / 2),
        "Desktop",
        palette.text_muted,
    );
    let status_pills = [
        (
            layout.status_rects[0],
            status.net.as_str(),
            palette.accent_blue,
        ),
        (
            layout.status_rects[1],
            status.cpu.as_str(),
            palette.accent_soft,
        ),
        (
            layout.status_rects[2],
            status.vpn.as_str(),
            palette.accent_blue,
        ),
        (
            layout.status_rects[3],
            status.aud.as_str(),
            palette.accent_mint,
        ),
        (
            layout.status_rects[4],
            status.power.as_str(),
            palette.accent_gold,
        ),
    ];
    for (rect, label, accent) in status_pills {
        canvas.fill_rect(
            Rect::new(rect.x, rect.y + 10, 1, rect.height.saturating_sub(20)),
            0xFF213445,
        );
        canvas.fill_rect(Rect::new(rect.x + 8, rect.y + 15, 4, 4), accent);
        canvas_draw_top_bar_text(
            &mut canvas,
            rect.x + 16,
            rect.y + ((rect.height as i32 - FONT_HEIGHT) / 2),
            label,
            if accent == palette.accent_gold {
                palette.text_primary
            } else {
                palette.text_secondary
            },
        );
    }
    let time_rect = layout.time_rect;
    canvas.fill_rect(
        Rect::new(time_rect.x, time_rect.bottom() - 2, time_rect.width, 2),
        palette.accent_gold,
    );
    canvas_draw_top_bar_centered_text(&mut canvas, time_rect, &status.time, palette.text_primary);
    canvas.into_pixels()
}

fn paint_desktop_shortcuts_surface(
    width: usize,
    height: usize,
    theme_mode: ThemeMode,
    selected: Option<DesktopShortcutKind>,
) -> Vec<u32> {
    let palette = hybrid_titan_palette(theme_mode);
    let mut canvas = Canvas::new(width, height, 0x0000_0000);
    for entry in desktop_shortcut_entries(width, height) {
        let selected_now = selected == Some(entry.kind);
        if selected_now {
            let highlight = entry.hit_rect.inset(-6, -6, -6, -6);
            canvas.fill_rect(highlight, 0xFF122030);
            canvas.stroke_rect(highlight, entry.kind.accent());
        }
        canvas.fill_rect(entry.icon_rect, entry.kind.accent());
        canvas.stroke_rect(
            entry.icon_rect,
            if selected_now {
                palette.text_primary
            } else {
                palette.border
            },
        );
        let glyph_rect = Rect::new(
            entry.icon_rect.x + 10,
            entry.icon_rect.y + 10,
            entry.icon_rect.width.saturating_sub(20),
            entry.icon_rect.height.saturating_sub(20),
        );
        emit_desktop_icon_rects(entry.kind.icon(), glyph_rect, |segment| {
            canvas.fill_rect(segment, 0xFF09131E);
        });
        canvas.draw_text(
            entry.label_x,
            entry.label_y,
            entry.kind.label(),
            if selected_now {
                palette.text_primary
            } else {
                palette.text_secondary
            },
        );
    }
    canvas.into_pixels()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rects_overlap(a: Rect, b: Rect) -> bool {
        a.x < b.right() && a.right() > b.x && a.y < b.bottom() && a.bottom() > b.y
    }

    #[test]
    fn top_bar_layout_keeps_command_and_status_clusters_disjoint() {
        let layout = top_bar_layout(1600);
        assert!(layout.apps_rect.right() < layout.workspace_rect.x);
        assert!(layout.workspace_rect.right() < layout.command_rect.x);
        assert!(layout.command_rect.right() < layout.status_rects[0].x);
        assert!(layout.status_rects[4].right() < layout.time_rect.x);
        assert!(!rects_overlap(layout.apps_rect, layout.workspace_rect));
        assert!(!rects_overlap(layout.command_rect, layout.status_rects[0]));
        assert!(!rects_overlap(layout.status_rects[4], layout.time_rect));
    }

    #[test]
    fn top_bar_raster_surface_contains_title_and_command_text_pixels() {
        let snapshot = SessionSnapshot {
            workspace_id: 0,
            workspace_layout: WorkspaceLayout::Dwindle,
            power_state: SessionPowerState::Active,
            unread_notifications: 0,
            apps_running: 0,
            apps_crashed: 0,
            overview_active: false,
            scratchpad_visible: false,
            shell_ready: true,
            boot_clean_desktop: true,
            output_scale: 1,
            text_scale: 1,
            clipboard_history_len: 0,
            accessibility_profile: AccessibilityProfile::default(),
            display_profile: DisplayProfile::default(),
            shell_density: ShellDensityProfile::Balanced,
            motion_profile: MotionProfile::Standard,
            restore_state: RestoreDisposition::RestoreIfClean,
            stage_set_policy: StageSetPolicy::default(),
            locale: String::from("en-US"),
            theme_variant: String::from("hybrid-titan"),
            shell_state: ShellState::DesktopReady,
        };
        let width = 1600usize;
        let height = Theme::HALO_BAR_HEIGHT as usize;
        let pixels = paint_top_bar_surface(width, height, 0, &snapshot, ThemeMode::Dark);
        let background = hybrid_titan_palette(ThemeMode::Dark).panel_bg;

        let title_bounds = Rect::new(18, 14, 72, 18);
        let apps_bounds = top_bar_layout(width as i32).apps_rect;
        let command_bounds = top_bar_layout(width as i32).command_rect;
        let metric_bounds = top_bar_layout(width as i32).status_rects[0];
        let time_bounds = top_bar_layout(width as i32).time_rect;
        let title_non_bg = count_non_background_pixels(&pixels, width, title_bounds, background);
        let apps_non_bg = count_non_background_pixels(&pixels, width, apps_bounds, background);
        let command_non_bg =
            count_non_background_pixels(&pixels, width, command_bounds, background);
        let metric_non_bg = count_non_background_pixels(&pixels, width, metric_bounds, background);
        let time_non_bg = count_non_background_pixels(&pixels, width, time_bounds, background);

        assert!(
            title_non_bg > 24,
            "title text should mark visible glyph pixels"
        );
        assert!(apps_non_bg > 48, "apps pill should carry visible pixels");
        assert!(
            command_non_bg > 80,
            "command prompt text should mark visible glyph pixels"
        );
        assert!(
            metric_non_bg > 48,
            "network metric pill should carry visible text or outline pixels"
        );
        assert!(time_non_bg > 48, "clock pill should carry visible pixels");
    }

    #[test]
    fn desktop_shortcut_hit_detects_terminal_and_recycle_bin() {
        let entries = desktop_shortcut_entries(320, 720);
        let terminal = entries[0];
        let recycle = entries[4];
        assert_eq!(
            desktop_shortcut_hit(
                Point::new(terminal.icon_rect.x + 10, terminal.icon_rect.y + 10),
                320,
                720
            ),
            Some(DesktopShortcutKind::Terminal)
        );
        assert_eq!(
            desktop_shortcut_hit(
                Point::new(recycle.label_x + 12, recycle.label_y + 4),
                320,
                720
            ),
            Some(DesktopShortcutKind::RecycleBin)
        );
    }

    #[test]
    fn desktop_shortcut_surface_contains_visible_terminal_pixels() {
        let pixels = paint_desktop_shortcuts_surface(320, 720, ThemeMode::Dark, None);
        let background = 0x0000_0000;
        let terminal_bounds = desktop_shortcut_entries(320, 720)[0].hit_rect;
        let visible = count_non_background_pixels(&pixels, 320, terminal_bounds, background);
        assert!(
            visible > 120,
            "desktop shortcut surface should paint visible terminal shortcut pixels"
        );
    }

    #[test]
    fn desktop_shortcuts_publish_browser_window_and_recycle_special_descriptors() {
        let web = DesktopShortcutKind::Web.descriptor();
        let recycle = DesktopShortcutKind::RecycleBin.descriptor();
        assert_eq!(web.app_id, BROWSER_APP_ID);
        assert_eq!(web.presentation, AppPresentation::Windowed);
        assert_eq!(web.loader, LoaderDispatch::Native);
        assert_eq!(web.abi, AbiPersonality::Native);
        assert_eq!(recycle.presentation, AppPresentation::SpecialAction);
        assert_eq!(recycle.loader, LoaderDispatch::Native);
        assert_eq!(recycle.abi, AbiPersonality::Native);
    }

    #[test]
    fn browser_url_normalization_prefers_https_and_default_home() {
        assert_eq!(normalize_browser_url(""), "https://example.com/");
        assert_eq!(normalize_browser_url("example.org"), "https://example.org");
        assert_eq!(
            normalize_browser_url("http://insecure.test/file.exe"),
            "http://insecure.test/file.exe"
        );
    }

    #[test]
    fn browser_start_page_lines_are_local_and_actionable() {
        let lines = browser_start_page_lines();
        assert_eq!(lines.len(), 4);
        assert!(lines[0].contains("Native browser shell is ready."));
        assert!(lines[1].contains("press Enter"));
        assert!(lines[2].contains("Use Open"));
        assert!(lines[3].contains("/downloads"));
    }

    #[test]
    fn desktop_visibility_recovery_triggers_for_active_session_with_stale_lock_screen() {
        assert!(desktop_visibility_recovery_needed(
            SessionPowerState::Active,
            true,
            true,
            false,
            false,
            false,
            true,
        ));
    }

    #[test]
    fn desktop_visibility_recovery_stays_idle_for_locked_session() {
        assert!(!desktop_visibility_recovery_needed(
            SessionPowerState::Locked,
            false,
            true,
            false,
            false,
            true,
            false,
        ));
    }

    #[test]
    fn browser_document_parser_extracts_title_preview_and_absolute_links() {
        let html = r#"
            <html>
                <head><title>echOS Downloads</title></head>
                <body>
                    <p>Latest nightly builds and release notes.</p>
                    <a href="/downloads/echos-installer.exe">Download installer</a>
                    <a href="docs/release-notes.html">Release notes</a>
                </body>
            </html>
        "#;
        let document = parse_browser_document("https://echos.dev/store/index.html", html);
        assert!(document
            .lines
            .iter()
            .any(|line| line.contains("Title: echOS Downloads")));
        assert!(document
            .lines
            .iter()
            .any(|line| line.contains("Latest nightly builds and release notes.")));
        assert_eq!(document.links.len(), 2);
        assert_eq!(document.links[0].label, "Download installer");
        assert_eq!(
            document.links[0].url,
            "https://echos.dev/downloads/echos-installer.exe"
        );
        assert_eq!(
            document.links[1].url,
            "https://echos.dev/store/docs/release-notes.html"
        );
    }

    #[test]
    fn browser_download_path_preserves_filename_and_derives_fallbacks() {
        assert_eq!(
            browser_download_path(
                "https://downloads.echos.dev/releases/echos-setup.exe",
                "application/octet-stream"
            ),
            "/downloads/echos-setup.exe"
        );
        assert_eq!(
            browser_download_path("https://docs.echos.dev", "text/html"),
            "/downloads/index.html"
        );
        assert_eq!(
            browser_download_path("https://api.echos.dev/latest", "application/json"),
            "/downloads/payload.json"
        );
    }

    #[test]
    fn desktop_launch_registry_resolves_firefox_binary_when_candidate_exists() {
        let registry = desktop_launch_registry();
        let resolution = RuntimePackageRegistry::new(&registry)
            .resolve_with_probe("firefox", |path| path == "/programs/firefox/firefox.exe")
            .expect("resolution");
        assert_eq!(resolution.path(), Some("/programs/firefox/firefox.exe"));
        assert_eq!(resolution.descriptor().title, "Firefox");
        assert_eq!(resolution.descriptor().loader, LoaderDispatch::Pe);
    }

    #[test]
    fn desktop_launch_registry_reports_missing_cef_candidates() {
        let registry = desktop_launch_registry();
        let resolution = RuntimePackageRegistry::new(&registry)
            .resolve_with_probe("cef", |_| false)
            .expect("resolution");
        assert!(resolution.path().is_none());
        assert_eq!(resolution.missing_candidates(), Some(CEF_BINARY_CANDIDATES));
        assert_eq!(resolution.descriptor().title, "CEF Browser");
    }

    #[test]
    fn desktop_web_shortcut_projects_native_windowed_launch_contract() {
        let session = LaunchIntent::new(
            DesktopShortcutKind::Web.descriptor(),
            ExecutionContext::new(LaunchSource::DesktopShortcut, 0, "desktop-web"),
        )
        .canonical_session();

        assert_eq!(session.intent.descriptor.app_id, BROWSER_APP_ID);
        assert_eq!(session.process.bootstrap, RuntimeBootstrap::NativeWindowed);
        assert_eq!(session.window.app_id, BROWSER_APP_ID);
        assert_eq!(
            session.event_loop,
            UnifiedEventLoopContract::DesktopWindowed
        );
    }

    #[test]
    fn launcher_quick_settings_and_palette_raster_surfaces_emit_visible_text_pixels() {
        let snapshot = SessionSnapshot {
            workspace_id: 0,
            workspace_layout: WorkspaceLayout::Dwindle,
            power_state: SessionPowerState::Active,
            unread_notifications: 0,
            apps_running: 0,
            apps_crashed: 0,
            overview_active: false,
            scratchpad_visible: false,
            shell_ready: true,
            boot_clean_desktop: true,
            output_scale: 1,
            text_scale: 1,
            clipboard_history_len: 0,
            accessibility_profile: AccessibilityProfile::default(),
            display_profile: DisplayProfile::default(),
            shell_density: ShellDensityProfile::Balanced,
            motion_profile: MotionProfile::Standard,
            restore_state: RestoreDisposition::RestoreIfClean,
            stage_set_policy: StageSetPolicy::default(),
            locale: String::from("en-US"),
            theme_variant: String::from("hybrid-titan"),
            shell_state: ShellState::DesktopReady,
        };
        let launcher = paint_launcher_surface(
            520,
            320,
            &[],
            &snapshot,
            &[],
            ThemeMode::Dark,
            ShellLayoutProfile::Desktop,
        );
        let quick_settings = paint_quick_settings_surface(
            360,
            240,
            ThemeMode::Dark,
            true,
            &SessionSnapshot {
                workspace_id: 0,
                workspace_layout: WorkspaceLayout::Dwindle,
                power_state: SessionPowerState::Active,
                unread_notifications: 2,
                apps_running: 3,
                apps_crashed: 0,
                overview_active: false,
                scratchpad_visible: false,
                shell_ready: true,
                boot_clean_desktop: true,
                output_scale: 100,
                text_scale: 100,
                clipboard_history_len: 1,
                accessibility_profile: AccessibilityProfile::default(),
                display_profile: DisplayProfile::default(),
                shell_density: ShellDensityProfile::Balanced,
                motion_profile: MotionProfile::Standard,
                restore_state: RestoreDisposition::RestoreIfClean,
                stage_set_policy: StageSetPolicy::default(),
                locale: String::from("en-US"),
                theme_variant: String::from("hybrid-titan"),
                shell_state: ShellState::DesktopReady,
            },
        );
        let palette = paint_command_palette_surface(
            520,
            320,
            &[CommandPaletteAction {
                id: 1,
                title: String::from("Open Web"),
                category: String::from("Launch"),
                shortcut: String::from("Enter"),
                enabled: true,
            }],
            "",
            0,
            ThemeMode::Dark,
        );
        let background = hybrid_titan_palette(ThemeMode::Dark).window_bg;

        assert!(
            count_non_background_pixels(&launcher, 520, Rect::new(0, 0, 520, 320), background)
                > 800,
            "launcher raster should paint visible glyphs and panels"
        );
        assert!(
            count_non_background_pixels(
                &quick_settings,
                360,
                Rect::new(0, 0, 360, 240),
                background
            ) > 600,
            "quick settings raster should paint visible glyphs and rows"
        );
        assert!(
            count_non_background_pixels(&palette, 520, Rect::new(0, 0, 520, 320), background) > 800,
            "command palette raster should paint visible glyphs and rows"
        );
    }

    fn count_non_background_pixels(
        pixels: &[u32],
        width: usize,
        rect: Rect,
        background: u32,
    ) -> usize {
        let mut count = 0usize;
        for y in rect.y.max(0) as usize..rect.bottom().max(0) as usize {
            let row = y.saturating_mul(width);
            for x in rect.x.max(0) as usize..rect.right().max(0) as usize {
                let idx = row.saturating_add(x);
                if let Some(pixel) = pixels.get(idx) {
                    if *pixel != background {
                        count += 1;
                    }
                }
            }
        }
        count
    }
}

fn paint_task_strip_surface(
    width: usize,
    height: usize,
    snapshots: &[AppSnapshot],
    active_workspace: u8,
    theme_mode: ThemeMode,
) -> Vec<u32> {
    let palette = hybrid_titan_palette(theme_mode);
    let dock_width = 354u32.min(width as u32);
    let dock_rect = Rect::new(
        ((width as i32 - dock_width as i32) / 2).max(0),
        0,
        dock_width,
        height as u32,
    );
    let mut canvas = Canvas::new(width, height, 0x00000000);
    canvas.fill_rect(dock_rect, 0xD1091018);
    canvas.stroke_rect(dock_rect, palette.border);

    let mut focused_kind = None;
    for snapshot in snapshots.iter() {
        if snapshot.focused || (snapshot.workspace_id == active_workspace && snapshot.visible) {
            focused_kind = Some(snapshot.kind);
            if snapshot.focused {
                break;
            }
        }
    }

    let dock_items = [
        (Some(AppKind::Terminal), palette.accent_mint),
        (Some(AppKind::Files), palette.accent_blue),
        (Some(AppKind::Settings), palette.accent_gold),
        (Some(AppKind::Editor), palette.accent_coral),
        (
            None,
            if snapshots.iter().any(|s| s.needs_attention) {
                palette.accent_gold
            } else {
                palette.text_secondary
            },
        ),
    ];

    for index in 0..dock_items.len() {
        let rect = task_strip_icon_rect(index, width);
        let (kind, accent) = dock_items[index];
        let active = kind.is_some() && kind == focused_kind;
        canvas.fill_rect(rect, 0xF1111C29);
        canvas.stroke_rect(rect, if active { accent } else { palette.border });

        if let Some(kind) = kind {
            let glyph_rect = Rect::new(
                rect.x + 10,
                rect.y + 10,
                rect.width.saturating_sub(20),
                rect.height.saturating_sub(20),
            );
            emit_desktop_icon_rects(kind.icon(), glyph_rect, |segment| {
                canvas.fill_rect(segment, if active { palette.text_primary } else { accent });
            });
        } else {
            let badge = Rect::new(
                rect.x + 10,
                rect.y + 12,
                rect.width.saturating_sub(20),
                rect.height.saturating_sub(24),
            );
            canvas.fill_rect(badge, 0xFF111821);
            canvas.stroke_rect(badge, accent);
            let text = if snapshots.iter().any(|s| s.needs_attention) {
                "!"
            } else {
                "i"
            };
            canvas.draw_text(
                badge.x + ((badge.width as i32 - FONT_WIDTH) / 2),
                badge.y + 12,
                text,
                accent,
            );
        }
    }

    canvas.into_pixels()
}

fn paint_context_menu_surface(
    width: usize,
    height: usize,
    target: Option<AppKind>,
    theme_mode: ThemeMode,
) -> Vec<u32> {
    let palette = hybrid_titan_palette(theme_mode);
    let mut canvas = Canvas::new(width, height, palette.window_bg);
    canvas.fill_rect(Rect::new(0, 0, width as u32, 36), palette.panel_bg);
    canvas.fill_rect(Rect::new(0, 36, width as u32, 1), palette.border);
    let title = target.map(|kind| kind.title()).unwrap_or("No Target");
    canvas.draw_text(16, 12, "Window Actions", palette.text_primary);
    canvas.draw_text(width as i32 - 96, 12, title, palette.text_muted);

    let rows = [
        ("1 Focus / Restore", ContextAction::Focus),
        ("2 Minimize", ContextAction::Minimize),
        ("3 Snap Left", ContextAction::SnapLeft),
        ("4 Snap Right", ContextAction::SnapRight),
        ("5 Maximize", ContextAction::Maximize),
        ("6 Move Next Space", ContextAction::MoveNextWorkspace),
        ("7 Close Window", ContextAction::Close),
    ];

    for (index, (label, action)) in rows.iter().enumerate() {
        let rect = Rect::new(16, 52 + index as i32 * 24, 208, 22);
        canvas.fill_rect(rect, palette.panel_bg);
        canvas.stroke_rect(
            rect,
            if matches!(action, ContextAction::Close) {
                palette.accent_coral
            } else {
                palette.border
            },
        );
        canvas.draw_text(rect.x + 10, rect.y + 6, label, palette.text_secondary);
    }
    canvas.into_pixels()
}

fn paint_switcher_surface(
    width: usize,
    height: usize,
    candidates: &[AppKind],
    selected_index: usize,
    active_workspace: u8,
    theme_mode: ThemeMode,
) -> Vec<u32> {
    let palette = hybrid_titan_palette(theme_mode);
    let mut canvas = Canvas::new(width, height, palette.window_bg);
    canvas.fill_rect(Rect::new(0, 0, width as u32, 44), palette.panel_bg);
    canvas.fill_rect(Rect::new(0, 44, width as u32, 1), palette.border);
    canvas.draw_text(18, 14, "App Switcher", palette.text_primary);
    canvas.draw_text(
        18,
        28,
        "Alt+Tab cycles, Alt release confirms",
        palette.text_muted,
    );

    for (index, kind) in candidates.iter().enumerate() {
        let rect = Rect::new(24, 72 + index as i32 * 36, 392, 30);
        let selected = index == selected_index.min(candidates.len().saturating_sub(1));
        canvas.fill_rect(
            rect,
            if selected {
                palette.panel_alt
            } else {
                palette.panel_bg
            },
        );
        canvas.stroke_rect(
            rect,
            if selected {
                kind.accent()
            } else {
                palette.border
            },
        );
        canvas.fill_rect(Rect::new(rect.x + 10, rect.y + 10, 8, 8), kind.accent());
        canvas.draw_text(rect.x + 28, rect.y + 7, kind.title(), palette.text_primary);
        canvas.draw_text(
            rect.right() - 110,
            rect.y + 7,
            if index == 0 { "current ring" } else { "queued" },
            palette.text_muted,
        );
    }

    canvas.draw_text(
        18,
        height as i32 - 24,
        &format!("workspace {}", active_workspace.saturating_add(1)),
        palette.text_secondary,
    );
    canvas.into_pixels()
}

fn paint_dialog_surface(
    width: usize,
    height: usize,
    pending_dialog: Option<&DialogRequest>,
    dialog_input: &str,
    theme_mode: ThemeMode,
) -> Vec<u32> {
    let palette = hybrid_titan_palette(theme_mode);
    let mut canvas = Canvas::new(width, height, palette.window_bg);
    canvas.fill_rect(Rect::new(0, 0, width as u32, 58), palette.panel_bg);
    canvas.fill_rect(Rect::new(0, 58, width as u32, 1), palette.border);
    canvas.draw_text(18, 18, "Dialog Broker", palette.text_primary);
    canvas.draw_text(
        18,
        36,
        "Shell mediates file and message dialogs",
        palette.text_muted,
    );

    if let Some(request) = pending_dialog {
        let kind = match request.kind {
            DialogKind::OpenFile => "Open file",
            DialogKind::SaveFile => "Save file",
            DialogKind::PickFolder => "Pick folder",
            DialogKind::Message => "Message",
        };
        canvas.draw_text(18, 78, &request.title, palette.text_primary);
        canvas.draw_text(18, 98, kind, palette.accent_blue);
        if !request.message.is_empty() {
            canvas.draw_multiline_text(
                18,
                118,
                width as i32 - 36,
                &request.message,
                palette.text_secondary,
            );
        } else {
            canvas.draw_text(18, 118, "Path", palette.text_muted);
            let input_rect = Rect::new(18, 136, width as u32 - 36, 26);
            canvas.fill_rect(input_rect, palette.panel_bg);
            canvas.stroke_rect(input_rect, palette.accent_mint);
            canvas.draw_text(26, 144, dialog_input, palette.text_secondary);
        }
    } else {
        canvas.draw_text(18, 92, "No pending dialogs", palette.text_secondary);
    }

    let cancel = Rect::new(36, 126, 134, 40);
    let accept = Rect::new(190, 126, 134, 40);
    canvas.fill_rect(cancel, palette.panel_bg);
    canvas.stroke_rect(cancel, palette.border);
    canvas.draw_text(
        cancel.x + 40,
        cancel.y + 12,
        "Cancel",
        palette.text_secondary,
    );
    canvas.fill_rect(accept, palette.panel_alt);
    canvas.stroke_rect(accept, palette.accent_mint);
    canvas.draw_text(accept.x + 40, accept.y + 12, "Accept", palette.text_primary);
    canvas.into_pixels()
}

fn paint_notifications_surface(
    width: usize,
    height: usize,
    notices: &[NotificationEntry],
    selected_index: usize,
    theme_mode: ThemeMode,
) -> Vec<u32> {
    let palette = hybrid_titan_palette(theme_mode);
    let mut canvas = Canvas::new(width, height, palette.window_bg);
    canvas.fill_rect(Rect::new(0, 0, width as u32, 54), palette.panel_bg);
    canvas.fill_rect(Rect::new(0, 54, width as u32, 1), palette.border);
    canvas.draw_text(18, 18, "Notification Surface", palette.text_primary);
    canvas.draw_text(18, 34, "Recent shell activity", palette.text_muted);

    let mut y = 72;
    for (index, notice) in notices.iter().rev().take(6).rev().enumerate() {
        let rect = Rect::new(18, y - 6, width.saturating_sub(36) as u32, 34);
        canvas.fill_rect(
            rect,
            if index == selected_index {
                palette.panel_alt
            } else {
                palette.panel_bg
            },
        );
        canvas.stroke_rect(
            rect,
            match notice.level {
                NotificationLevel::Info => palette.border,
                NotificationLevel::Success => palette.accent_soft,
                NotificationLevel::Warning => palette.accent_gold,
                NotificationLevel::Error => palette.accent_coral,
            },
        );
        canvas.draw_text(30, y + 2, &notice.title, palette.text_primary);
        canvas.draw_text(
            30,
            y + 16,
            &format!("{}: {}", notice.source_name, notice.message),
            palette.text_secondary,
        );
        canvas.draw_text(
            rect.right() - 70,
            y + 2,
            if notice.read { "read" } else { "new" },
            if notice.read {
                palette.text_muted
            } else {
                palette.accent_soft
            },
        );
        if let Some(action) = notice.action_label.as_ref() {
            canvas.draw_text(rect.right() - 130, y + 16, action, palette.accent_blue);
        }
        y += 42;
    }

    canvas.draw_text(
        18,
        max(height as i32 - 28, 0),
        "Click/o = open  j/k = move  c = clear",
        palette.text_muted,
    );
    canvas.into_pixels()
}

fn paint_lock_surface(
    width: usize,
    height: usize,
    auth_input: &str,
    logged_in: bool,
    theme_mode: ThemeMode,
) -> Vec<u32> {
    let palette = hybrid_titan_palette(theme_mode);
    let mut canvas = Canvas::new(width, height, palette.window_bg);
    canvas.fill_rect(
        Rect::new(0, 0, width as u32, height as u32),
        palette.window_bg,
    );
    canvas.fill_rect(Rect::new(0, 0, width as u32, 64), palette.panel_bg);
    canvas.fill_rect(Rect::new(0, 64, width as u32, 1), palette.border);
    canvas.draw_text(
        24,
        20,
        if logged_in { "Unlock" } else { "Login" },
        palette.text_primary,
    );
    canvas.draw_text(
        24,
        40,
        "Native session authentication gate",
        palette.text_muted,
    );
    let panel = Rect::new(40, 100, (width as u32).saturating_sub(80), 136);
    canvas.fill_rect(panel, palette.panel_bg);
    canvas.stroke_rect(panel, palette.accent_blue);
    canvas.draw_text(60, 126, "User: operator", palette.text_secondary);
    canvas.draw_text(60, 152, "Password", palette.text_primary);
    let masked = "*".repeat(auth_input.len().max(1));
    canvas.fill_rect(
        Rect::new(60, 176, panel.width.saturating_sub(120), 34),
        palette.window_bg,
    );
    canvas.stroke_rect(
        Rect::new(60, 176, panel.width.saturating_sub(120), 34),
        palette.accent_mint,
    );
    canvas.draw_text(74, 186, &masked, palette.text_primary);
    canvas.draw_text(
        60,
        224,
        "Enter = unlock, Backspace = delete",
        palette.text_muted,
    );
    canvas.draw_text(60, 246, "Default password: echos", palette.accent_gold);
    canvas.into_pixels()
}

fn paint_quick_settings_surface(
    width: usize,
    height: usize,
    theme_mode: ThemeMode,
    notifications_enabled: bool,
    snapshot: &SessionSnapshot,
) -> Vec<u32> {
    let palette = hybrid_titan_palette(theme_mode);
    let mut canvas = Canvas::new(width, height, palette.window_bg);
    canvas.fill_rect(Rect::new(0, 0, width as u32, 48), palette.panel_bg);
    canvas.fill_rect(Rect::new(0, 48, width as u32, 1), palette.border);
    canvas.draw_text(18, 14, "Quick Settings", palette.text_primary);
    canvas.draw_text(18, 30, "1-5 or click to toggle", palette.text_muted);

    let rows = [
        (
            format!("Theme {}", theme_mode_label(theme_mode)),
            palette.accent_blue,
        ),
        (
            format!(
                "Notifications {}",
                if notifications_enabled { "On" } else { "Off" }
            ),
            if notifications_enabled {
                palette.accent_soft
            } else {
                palette.border
            },
        ),
        (
            format!("Motion {}", motion_profile_label(snapshot.motion_profile)),
            if snapshot.motion_profile != MotionProfile::Reduced {
                palette.accent_mint
            } else {
                palette.border
            },
        ),
        (
            format!("Scale {}", display_scale_label(&snapshot.display_profile)),
            palette.accent_gold,
        ),
        (
            format!(
                "Screen Reader {}",
                if snapshot.accessibility_profile.screen_reader {
                    "On"
                } else {
                    "Off"
                }
            ),
            if snapshot.accessibility_profile.screen_reader {
                palette.accent_coral
            } else {
                palette.border
            },
        ),
    ];

    for (index, (label, accent)) in rows.iter().enumerate() {
        let rect = quick_settings_row_rect(index, width);
        canvas.fill_rect(rect, palette.panel_bg);
        canvas.stroke_rect(rect, *accent);
        canvas.draw_text(
            rect.x + 10,
            rect.y + 7,
            &(index + 1).to_string(),
            palette.text_muted,
        );
        canvas.draw_text(rect.x + 28, rect.y + 7, label, palette.text_secondary);
    }

    canvas.draw_text(
        18,
        max(height as i32 - 16, 0),
        "Super+, toggles this panel",
        palette.text_muted,
    );
    canvas.into_pixels()
}

fn paint_command_palette_surface(
    width: usize,
    height: usize,
    actions: &[CommandPaletteAction],
    query: &str,
    selected_index: usize,
    theme_mode: ThemeMode,
) -> Vec<u32> {
    let palette = hybrid_titan_palette(theme_mode);
    let mut canvas = Canvas::new(width, height, palette.window_bg);
    canvas.fill_rect(Rect::new(0, 0, width as u32, 56), palette.panel_bg);
    canvas.fill_rect(Rect::new(0, 56, width as u32, 1), palette.border);
    canvas.draw_text(18, 18, "Command Palette", palette.text_primary);
    canvas.draw_text(18, 38, "Super+Space", palette.text_muted);

    let query_rect = Rect::new(168, 14, width.saturating_sub(184) as u32, 34);
    canvas.fill_rect(query_rect, palette.window_bg);
    canvas.stroke_rect(query_rect, palette.accent_blue);
    canvas.draw_text(
        query_rect.x + 10,
        query_rect.y + 10,
        if query.is_empty() {
            "Type to filter actions"
        } else {
            query
        },
        if query.is_empty() {
            palette.text_muted
        } else {
            palette.text_primary
        },
    );

    for (index, action) in actions.iter().take(6).enumerate() {
        let selected = index == selected_index.min(actions.len().saturating_sub(1));
        let rect = command_palette_row_rect(index, width);
        canvas.fill_rect(
            rect,
            if selected {
                palette.panel_alt
            } else {
                palette.panel_bg
            },
        );
        canvas.stroke_rect(
            rect,
            if selected {
                palette.accent_blue
            } else {
                palette.border
            },
        );
        canvas.draw_text(
            rect.x + 10,
            rect.y + 8,
            &action.category,
            palette.text_muted,
        );
        canvas.draw_text(
            rect.x + 126,
            rect.y + 8,
            &action.title,
            palette.text_primary,
        );
        canvas.draw_text(
            rect.right() - 120,
            rect.y + 8,
            &action.shortcut,
            palette.text_secondary,
        );
    }

    if actions.is_empty() {
        canvas.draw_text(24, 92, "No matching actions", palette.text_secondary);
    }

    canvas.draw_text(
        22,
        max(height as i32 - 24, 0),
        "Enter execute | Esc close | j/k navigate",
        palette.text_muted,
    );
    canvas.into_pixels()
}

fn paint_stage_rail_surface(
    width: usize,
    height: usize,
    stage_sets: &[StageSet],
    active_index: usize,
    theme_mode: ThemeMode,
) -> Vec<u32> {
    let palette = hybrid_titan_palette(theme_mode);
    let mut canvas = Canvas::new(width, height, palette.window_bg);
    canvas.fill_rect(Rect::new(0, 0, width as u32, 44), palette.panel_bg);
    canvas.fill_rect(Rect::new(0, 44, width as u32, 1), palette.border);
    canvas.draw_text(14, 14, "Workspace Overview", palette.text_primary);
    canvas.draw_text(14, 30, "Super+` toggle", palette.text_muted);

    for (index, set) in stage_sets.iter().enumerate() {
        let selected = index == active_index.min(stage_sets.len().saturating_sub(1));
        let rect = stage_rail_row_rect(index, width);
        canvas.fill_rect(
            rect,
            if selected {
                palette.panel_alt
            } else {
                palette.panel_bg
            },
        );
        canvas.stroke_rect(
            rect,
            if selected {
                palette.accent_mint
            } else {
                palette.border
            },
        );
        canvas.draw_text(rect.x + 10, rect.y + 8, &set.name, palette.text_primary);
        canvas.draw_text(
            rect.x + 10,
            rect.y + 24,
            &format!(
                "{}",
                if set.window_ids.is_empty() {
                    String::from("empty workspace")
                } else {
                    format!("{} windows", set.window_ids.len())
                }
            ),
            palette.text_secondary,
        );
        if set.pinned {
            canvas.draw_text(rect.right() - 56, rect.y + 8, "Pin", palette.accent_gold);
        }
    }

    canvas.draw_text(
        14,
        max(height as i32 - 20, 0),
        "Click or 1-8 to activate",
        palette.text_muted,
    );
    canvas.into_pixels()
}
struct Canvas {
    width: usize,
    height: usize,
    pixels: Vec<u32>,
}

impl Canvas {
    fn new(width: usize, height: usize, background: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![background; width.saturating_mul(height)],
        }
    }

    fn fill_rect(&mut self, rect: Rect, color: u32) {
        let clip = Rect::new(0, 0, self.width as u32, self.height as u32);
        let Some(clipped) = rect.intersection(&clip) else {
            return;
        };
        for y in clipped.y.max(0) as usize..clipped.bottom().max(0) as usize {
            let row = y * self.width;
            for x in clipped.x.max(0) as usize..clipped.right().max(0) as usize {
                self.pixels[row + x] = color;
            }
        }
    }

    fn stroke_rect(&mut self, rect: Rect, color: u32) {
        if rect.width == 0 || rect.height == 0 {
            return;
        }
        self.fill_rect(Rect::new(rect.x, rect.y, rect.width, 1), color);
        self.fill_rect(
            Rect::new(rect.x, rect.bottom().saturating_sub(1), rect.width, 1),
            color,
        );
        self.fill_rect(Rect::new(rect.x, rect.y, 1, rect.height), color);
        self.fill_rect(
            Rect::new(rect.right().saturating_sub(1), rect.y, 1, rect.height),
            color,
        );
    }

    fn draw_text(&mut self, x: i32, y: i32, text: &str, color: u32) {
        let mut cursor_x = x;
        let mut cursor_y = y;
        for ch in text.chars() {
            match ch {
                '\n' => {
                    cursor_x = x;
                    cursor_y += FONT_HEIGHT;
                }
                _ => {
                    self.draw_char(cursor_x, cursor_y, ch, color);
                    cursor_x += FONT_WIDTH;
                }
            }
        }
    }

    fn draw_multiline_text(&mut self, x: i32, y: i32, max_width: i32, text: &str, color: u32) {
        let mut cursor_x = x;
        let mut cursor_y = y;
        let max_chars = max((max_width / FONT_WIDTH) as usize, 1);
        let mut line_len = 0usize;
        for ch in text.chars() {
            match ch {
                '\n' => {
                    cursor_x = x;
                    cursor_y += FONT_HEIGHT + 2;
                    line_len = 0;
                }
                _ => {
                    if line_len >= max_chars {
                        cursor_x = x;
                        cursor_y += FONT_HEIGHT + 2;
                        line_len = 0;
                    }
                    self.draw_char(cursor_x, cursor_y, ch, color);
                    cursor_x += FONT_WIDTH;
                    line_len += 1;
                }
            }
        }
    }

    fn draw_char(&mut self, x: i32, y: i32, ch: char, color: u32) {
        let glyph = vga_font::get_font_data(ch);
        for (row, byte) in glyph.iter().enumerate() {
            for col in 0..8usize {
                if (byte >> (7 - col)) & 1 == 0 {
                    continue;
                }
                let px = x + col as i32;
                let py = y + row as i32;
                if px < 0 || py < 0 || px >= self.width as i32 || py >= self.height as i32 {
                    continue;
                }
                let index = py as usize * self.width + px as usize;
                self.pixels[index] = color;
            }
        }
    }

    fn into_pixels(self) -> Vec<u32> {
        self.pixels
    }
}

fn canvas_draw_top_bar_text(canvas: &mut Canvas, x: i32, y: i32, text: &str, color: u32) {
    canvas.draw_text(x + 1, y + 1, text, 0xFF081019);
    canvas.draw_text(x, y, text, color);
}

fn canvas_draw_top_bar_centered_text(canvas: &mut Canvas, rect: Rect, text: &str, color: u32) {
    let text_width = (text.len() as i32).saturating_mul(FONT_WIDTH);
    let text_x = rect.x + ((rect.width as i32 - text_width) / 2).max(0);
    let text_y = rect.y + ((rect.height as i32 - FONT_HEIGHT) / 2).max(0);
    canvas_draw_top_bar_text(canvas, text_x, text_y, text, color);
}
