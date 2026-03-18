//! Active desktop session runtime for the native echOS shell.

use crate::font::vga_font;
use crate::gfx::shell_invalidation::{ShellFramePlan, ShellInvalidationState};
use crate::gfx::shell_scene::raster_surface_scene;
use crate::gop::framebuffer::Framebuffer;
use crate::gui::animation::{
    add_animation, get_animation_value, get_time_ns, remove_animation, update_animations,
    Animation, AnimationTarget, AnimationTargetType,
};
use crate::gui::client::{ClientWindow, DesktopClient};
use crate::gui::layout::{layout_flex, EdgeInsets, FlexDirection, FlexItem};
use crate::gui::protocol::{
    AccessibilityNode, AccessibilityRole, AppHealth, ClipboardPayload, CommandPaletteAction,
    DamageLane, DesktopPermission, DialogKind, DialogSelection, InputEvent, InvalidationReason,
    InvalidationTarget, KeyState, LayerRole, NotificationEntry, NotificationLevel,
    PermissionState, Point, Rect, RenderObjectKind, SceneNodeId, SessionPowerState,
    SessionSnapshot, ShellAppEntry, ShellShortcut, ShellState, StageSet, WindowFlags, WindowId,
    WindowInputEvent, WorkspaceLayout, WorkspaceRule, MOD_CTRL,
};
use crate::gui::scene::SceneGraph;
use crate::gui::text::{TextStyle, TextSystem};
use crate::gui::theme::{ShellLayoutProfile, Theme, ThemeMode};
use crate::gui::window_manager::{
    BORDER_THICKNESS, MIN_CONTENT_HEIGHT, MIN_CONTENT_WIDTH, TITLEBAR_HEIGHT,
};
use crate::personalization::{hybrid_windowing, virtual_desktops};
use crate::security::users::USER_DB;
use crate::services::FileEntry;
use crate::tty::pty::{
    configure_pty_for_shell, execute_command_on_pty_with_shell, pty_has_output,
    write_welcome_message, PtyPair, Winsize, PTY_MANAGER,
};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::cmp::{max, min};
use x86_64::instructions::port::PortWriteOnly;

const SHELL_APP_ID: u32 = 1;
const TERMINAL_APP_ID: u32 = 10;
const FILES_APP_ID: u32 = 11;
const SETTINGS_APP_ID: u32 = 12;
const EDITOR_APP_ID: u32 = 13;
const WORKSPACE_COUNT: u8 = 8;
const SCRATCHPAD_WORKSPACE: u8 = WORKSPACE_COUNT;

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
    Settings,
    Editor,
}

impl AppKind {
    const ALL: [Self; 4] = [Self::Terminal, Self::Files, Self::Settings, Self::Editor];

    fn app_id(self) -> u32 {
        match self {
            Self::Terminal => TERMINAL_APP_ID,
            Self::Files => FILES_APP_ID,
            Self::Settings => SETTINGS_APP_ID,
            Self::Editor => EDITOR_APP_ID,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Terminal => "Terminal",
            Self::Files => "Files",
            Self::Settings => "Settings",
            Self::Editor => "Editor",
        }
    }

    fn accent(self) -> u32 {
        match self {
            Self::Terminal => ACCENT_MINT,
            Self::Files => ACCENT_BLUE,
            Self::Settings => ACCENT_GOLD,
            Self::Editor => ACCENT_CORAL,
        }
    }

    fn shortcut(self) -> char {
        match self {
            Self::Terminal => '1',
            Self::Files => '2',
            Self::Settings => '3',
            Self::Editor => '4',
        }
    }

    fn dock_label(self) -> &'static str {
        match self {
            Self::Terminal => "T",
            Self::Files => "F",
            Self::Settings => "S",
            Self::Editor => "W",
        }
    }
}

fn app_kind_from_id(app_id: u32) -> Option<AppKind> {
    match app_id {
        TERMINAL_APP_ID => Some(AppKind::Terminal),
        FILES_APP_ID => Some(AppKind::Files),
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

    fn update_from_info(&mut self, info: &crate::gui::protocol::WindowInfo) -> bool {
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

fn set_shell_surface_visibility(
    client: &DesktopClient,
    window: &mut SessionWindow,
    visible: bool,
) {
    let _ = client.set_visibility(window.window_id, visible);
    window.visible = visible;
    window.desired_visible = visible;
    window.opacity = if visible { 1.0 } else { 0.0 };
    window.fading_out = false;
}

fn task_strip_window_rect(screen: Rect) -> Rect {
    let available = screen.width.saturating_sub(48);
    let width = if available >= 354 {
        354
    } else if available >= 260 {
        available
    } else {
        available.max(200)
    };
    Rect::new(
        screen.x + ((screen.width.saturating_sub(width)) / 2) as i32,
        screen.bottom() - (Theme::PULSE_DOCK_HEIGHT as i32 + 56),
        width,
        Theme::PULSE_DOCK_HEIGHT as u32,
    )
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
    pending_dialog: Option<crate::gui::protocol::DialogRequest>,
    dialog_input: String,
    command_query: String,
    command_selection: usize,
    context_target: Option<AppKind>,
    switcher_index: usize,
    notification_index: usize,
    auth_input: String,
    logged_in: bool,
    invalidation: ShellInvalidationState,
}

struct TerminalApp {
    client: DesktopClient,
    window: Option<SessionWindow>,
    workspace_id: u8,
    lines: Vec<String>,
    input: String,
    shell: crate::shell::Shell,
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
    settings: SettingsApp,
    editor: EditorApp,
    appliance_auto_login_pending: bool,
    desktop_ready_published: bool,
    app_basket_committed: bool,
    last_tick_ns: u64,
}

pub struct VelvetGloveCompositor;

impl VelvetGloveCompositor {
    pub fn run(fb: &mut Framebuffer) -> ! {
        crate::serial_println!("[DESKTOP] native session runtime active");
        debug_marker(b"VG0\n");
        let screen = Rect::new(0, 0, fb.width as u32, fb.height as u32);
        let mut session = match DesktopSession::new(screen) {
            Ok(session) => session,
            Err(err) => {
                crate::serial_println!("[DESKTOP] session bootstrap failed: {}", err);
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

    fn commit_shell_surface(
        &self,
        window: &SessionWindow,
        width: usize,
        height: usize,
        pixels: Vec<u32>,
    ) -> Result<(), String> {
        self.shell.client.commit_scene(
            window.window_id,
            raster_surface_scene(window.window_id, width, height, pixels, DamageLane::Shell),
        )
    }

    fn new(screen: Rect) -> Result<Self, String> {
        debug_marker(b"N00\n");
        crate::serial_println!("[DESKTOP] session bootstrap step=connect");
        let shell_client = DesktopClient::connect(SHELL_APP_ID)?;
        let terminal_client = DesktopClient::connect(TERMINAL_APP_ID)?;
        let files_client = DesktopClient::connect(FILES_APP_ID)?;
        let settings_client = DesktopClient::connect(SETTINGS_APP_ID)?;
        let editor_client = DesktopClient::connect(EDITOR_APP_ID)?;
        let _ = shell_client.register_shortcut_sink();

        debug_marker(b"N10\n");
        crate::serial_println!("[DESKTOP] session bootstrap step=shell-windows");
        let top_bar_window = shell_client.create_layer_window(
            "Top Bar",
            screen.x + 18,
            screen.y + 18,
            screen.width.saturating_sub(36),
            Theme::HALO_BAR_HEIGHT as u32,
            0,
            LayerRole::TopBar,
            shell_layer_flags(),
        )?;
        let task_strip_rect = task_strip_window_rect(screen);
        let task_strip_window = shell_client.create_layer_window(
            "Task Strip",
            task_strip_rect.x,
            task_strip_rect.y,
            task_strip_rect.width,
            task_strip_rect.height,
            0,
            LayerRole::Dock,
            shell_layer_flags(),
        )?;
        let launcher_window = shell_client.create_layer_window(
            "Session Shell",
            screen.x + 32,
            screen.y + 108,
            min(392, screen.width.saturating_sub(96)),
            min(332, screen.height.saturating_sub(172)),
            0,
            LayerRole::Overlay,
            shell_layer_flags(),
        )?;
        let notifications_window = shell_client.create_layer_window(
            "Notifications",
            screen.right() - 364,
            screen.y + 108,
            320,
            220,
            0,
            LayerRole::Overlay,
            shell_layer_flags(),
        )?;
        let quick_settings_window = shell_client.create_layer_window(
            "Quick Settings",
            screen.right() - 364,
            screen.y + 348,
            320,
            232,
            0,
            LayerRole::Overlay,
            shell_layer_flags(),
        )?;
        let command_palette_window = shell_client.create_layer_window(
            "Command Palette",
            screen.x + (screen.width as i32 / 2) - 310,
            screen.y + 136,
            620,
            312,
            0,
            LayerRole::Overlay,
            shell_layer_flags(),
        )?;
        let stage_rail_window = shell_client.create_layer_window(
            "Workspace Overview",
            screen.x + 18,
            screen.y + 108,
            236,
            264,
            0,
            LayerRole::Overlay,
            shell_layer_flags(),
        )?;
        let dialog_window = shell_client.create_layer_window(
            "Dialog Broker",
            screen.x + (screen.width as i32 / 2) - 180,
            screen.y + (screen.height as i32 / 2) - 120,
            360,
            190,
            0,
            LayerRole::Modal,
            shell_layer_flags(),
        )?;
        let context_menu_window = shell_client.create_layer_window(
            "Context Menu",
            screen.right() - 310,
            screen.bottom() - 290,
            240,
            224,
            0,
            LayerRole::Overlay,
            shell_layer_flags(),
        )?;
        let switcher_window = shell_client.create_layer_window(
            "App Switcher",
            screen.x + (screen.width as i32 / 2) - 220,
            screen.y + (screen.height as i32 / 2) - 120,
            440,
            240,
            0,
            LayerRole::Overlay,
            shell_layer_flags(),
        )?;
        let lock_window = shell_client.create_layer_window(
            "Login",
            screen.x + (screen.width as i32 / 2) - 240,
            screen.y + (screen.height as i32 / 2) - 160,
            480,
            320,
            0,
            LayerRole::Modal,
            shell_layer_flags(),
        )?;
        let _ = shell_client.set_visibility(dialog_window.window_id, false);
        let _ = shell_client.set_visibility(context_menu_window.window_id, false);
        let _ = shell_client.set_visibility(switcher_window.window_id, false);
        let _ = shell_client.set_visibility(notifications_window.window_id, false);
        let _ = shell_client.set_visibility(quick_settings_window.window_id, false);
        let _ = shell_client.set_visibility(command_palette_window.window_id, false);
        debug_marker(b"N20\n");
        crate::serial_println!("[DESKTOP] session bootstrap step=register-apps");
        let _ = terminal_client.register_shell_app("Terminal");
        let _ = files_client.register_shell_app("Files");
        let _ = settings_client.register_shell_app("Settings");
        let _ = editor_client.register_shell_app("Editor");
        crate::serial_println!("[DESKTOP] session bootstrap step=permissions");
        grant_default_permissions(&shell_client, true);
        grant_default_permissions(&terminal_client, true);
        grant_default_permissions(&files_client, true);
        grant_default_permissions(&editor_client, true);
        grant_default_permissions(&settings_client, true);
        grant_default_file_access(&terminal_client, &["/workspace", "/"]);
        grant_default_file_access(&files_client, &["/", "/workspace", "/system"]);
        grant_default_file_access(&editor_client, &["/workspace"]);
        grant_default_file_access(&settings_client, &["/system", "/workspace"]);
        grant_default_file_access(&shell_client, &["/", "/workspace", "/system"]);
        let _ = shell_client.set_power_state(SessionPowerState::Active);
        let _ = shell_client.set_theme_mode(Theme::default_mode());
        for workspace_id in 0..WORKSPACE_COUNT {
            let rule = default_workspace_rule(workspace_id);
            let _ = shell_client.set_workspace_rule(workspace_id, rule);
            let _ = shell_client.set_workspace_layout(workspace_id, rule.layout);
            let _ = virtual_desktops().lock().set_profile(
                workspace_id,
                crate::personalization::DesktopProfile {
                    wallpaper_id: workspace_id as u32,
                    icon_pack: rule.default_name_str(),
                },
            );
        }
        let _ = shell_client.set_workspace_rule(
            SCRATCHPAD_WORKSPACE,
            default_workspace_rule(SCRATCHPAD_WORKSPACE),
        );
        debug_marker(b"N30\n");
        crate::serial_println!("[DESKTOP] session bootstrap step=session-struct");
        let mut session = Self {
            screen,
            shell: ShellRuntime {
                client: shell_client,
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
                shell: crate::shell::Shell::new(),
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
            appliance_auto_login_pending: crate::boot::appliance::auto_login_requested(),
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
        crate::serial_println!("[DESKTOP] session bootstrap step=login-visible");
        session.set_login_visibility(true);
        let _ = session
            .shell
            .client
            .focus_window(session.shell.lock_screen.window_id);
        session.push_notice(String::from("Login required: password echos"));
        debug_marker(b"N50\n");
        crate::serial_println!("[DESKTOP] session bootstrap step=render-shell");
        session.render_shell()?;
        debug_marker(b"N60\n");
        crate::serial_println!("[DESKTOP] session bootstrap step=render-apps");
        session.render_apps()?;
        debug_marker(b"N70\n");
        crate::serial_println!("[DESKTOP] session bootstrap step=ready");
        if session.appliance_auto_login_pending {
            crate::serial_println!("[DESKTOP] appliance auto-login armed");
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

        let mut commands = Vec::new();
        if let Ok(events) = self.shell.client.poll_input(32) {
            self.handle_shell_events(events, &mut commands);
        }
        if !self.is_locked() {
            if let Ok(events) = self.terminal.client.poll_input(32) {
                self.terminal.handle_events(events, &mut commands);
            }
            if let Ok(events) = self.files.client.poll_input(32) {
                self.files.handle_events(events, &mut commands);
            }
            if let Ok(events) = self.settings.client.poll_input(32) {
                self.settings.handle_events(events);
            }
            if let Ok(events) = self.editor.client.poll_input(32) {
                self.editor.handle_events(events);
            }
            self.terminal.poll_platform();
            self.editor.poll_platform();
        }

        for command in commands {
            self.apply_command(command);
        }

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

    fn run_appliance_auto_login(&mut self) {
        if !self.appliance_auto_login_pending
            || self.shell.logged_in
            || !self.shell.lock_screen.visible
        {
            return;
        }

        self.appliance_auto_login_pending = false;
        self.shell.auth_input.clear();
        self.shell.auth_input.push_str("echos");
        self.invalidate_shell(InvalidationTarget::LockScreen, InvalidationReason::StateChanged);
        crate::serial_println!("[DESKTOP] appliance auto-login attempt");
        self.attempt_login();
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
                    self.invalidate_shell(InvalidationTarget::LockScreen, InvalidationReason::StateChanged);
                } else if let Some(ch) = printable_key(input) {
                    self.shell.auth_input.push(ch);
                    self.invalidate_shell(InvalidationTarget::LockScreen, InvalidationReason::StateChanged);
                }
                continue;
            }

            if self.is_locked() {
                continue;
            }

            if event.window_id == self.shell.top_bar.window_id {
                match input {
                    InputEvent::PointerButton {
                        button: crate::gui::protocol::PointerButton::Left,
                        state: KeyState::Pressed,
                        ..
                    } => {
                        if let Some(local) = event.local_position {
                            if top_bar_power_hit(
                                local,
                                self.shell.top_bar.content_rect.width as i32,
                            ) {
                                self.toggle_power_state();
                            } else if top_bar_command_hit(
                                local,
                                self.shell.top_bar.content_rect.width as i32,
                            ) {
                                self.toggle_command_palette();
                            } else if top_bar_quick_settings_hit(
                                local,
                                self.shell.top_bar.content_rect.width as i32,
                            ) {
                                self.toggle_quick_settings();
                            }
                        }
                    }
                    InputEvent::PointerButton {
                        button: crate::gui::protocol::PointerButton::Right,
                        state: KeyState::Pressed,
                        ..
                    } => self.cycle_switcher(),
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
                        button: crate::gui::protocol::PointerButton::Left,
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
                    self.invalidate_shell(InvalidationTarget::CommandPalette, InvalidationReason::StateChanged);
                } else if key_scan_pressed(input, 0x24) {
                    self.shell.command_selection = self.shell.command_selection.saturating_add(1);
                    self.invalidate_shell(InvalidationTarget::CommandPalette, InvalidationReason::StateChanged);
                } else if key_scan_pressed(input, 0x25) {
                    self.shell.command_selection = self.shell.command_selection.saturating_sub(1);
                    self.invalidate_shell(InvalidationTarget::CommandPalette, InvalidationReason::StateChanged);
                } else if let Some(ch) = printable_key(input) {
                    if self.shell.command_query.len() < 48 {
                        self.shell.command_query.push(ch);
                    }
                    self.shell.command_selection = 0;
                    self.invalidate_shell(InvalidationTarget::CommandPalette, InvalidationReason::StateChanged);
                }
            }

            if event.window_id == self.shell.quick_settings.window_id {
                match input {
                    InputEvent::PointerButton {
                        button: crate::gui::protocol::PointerButton::Left,
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
                        button: crate::gui::protocol::PointerButton::Left,
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
                            if let Some(workspace_id) = task_strip_workspace_hit(
                                local,
                                self.shell.task_strip.content_rect.width as usize,
                            ) {
                                commands.push(SessionCommand::SwitchWorkspace(workspace_id));
                            } else if let Some(kind) = task_strip_app_hit(
                                local,
                                self.shell.task_strip.content_rect.width as usize,
                            ) {
                                match button {
                                    crate::gui::protocol::PointerButton::Right => {
                                        self.open_context_menu(kind)
                                    }
                                    _ => commands.push(SessionCommand::Activate(kind)),
                                }
                            }
                        }
                    }
                    _ => {}
                }

                if let Some(kind) = app_shortcut_from_input(input) {
                    commands.push(SessionCommand::Activate(kind));
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
                                    crate::gui::protocol::PointerButton::Right => {
                                        self.open_context_menu(kind)
                                    }
                                    _ => commands.push(SessionCommand::Activate(kind)),
                                }
                            }
                        }
                    }
                    _ => {}
                }

                if let Some(kind) = app_shortcut_from_input(input) {
                    commands.push(SessionCommand::Activate(kind));
                } else if key_scan_pressed(input, 0x0F) {
                    self.cycle_switcher();
                }
            }

            if event.window_id == self.shell.notifications.window_id {
                match input {
                    InputEvent::PointerButton {
                        button: crate::gui::protocol::PointerButton::Left,
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
                                                commands.push(SessionCommand::Activate(kind));
                                            }
                                        }
                                        self.push_notice(format!(
                                            "Notification {} acknowledged",
                                            entry.id
                                        ));
                                        self.invalidate_shell(InvalidationTarget::NotificationCenter, InvalidationReason::StateChanged);
                                        self.invalidate_shell(InvalidationTarget::TopBar, InvalidationReason::StateChanged);
                                        self.invalidate_shell(InvalidationTarget::Dock, InvalidationReason::StateChanged);
                                        self.invalidate_shell(InvalidationTarget::Launcher, InvalidationReason::StateChanged);
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }

                if key_scan_pressed(input, 0x24) {
                    self.shell.notification_index = self.shell.notification_index.saturating_add(1);
                    self.invalidate_shell(InvalidationTarget::NotificationCenter, InvalidationReason::StateChanged);
                } else if key_scan_pressed(input, 0x25) {
                    self.shell.notification_index = self.shell.notification_index.saturating_sub(1);
                    self.invalidate_shell(InvalidationTarget::NotificationCenter, InvalidationReason::StateChanged);
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
                                    commands.push(SessionCommand::Activate(kind));
                                }
                            }
                            self.invalidate_shell(InvalidationTarget::NotificationCenter, InvalidationReason::StateChanged);
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
                    self.invalidate_shell(InvalidationTarget::Dialog, InvalidationReason::StateChanged);
                } else if is_escape_key(input) {
                    self.resolve_pending_dialog(false);
                } else if let Some(ch) = printable_key(input) {
                    self.shell.dialog_input.push(ch);
                    self.invalidate_shell(InvalidationTarget::Dialog, InvalidationReason::StateChanged);
                }
            }
        }
    }

    fn apply_command(&mut self, command: SessionCommand) {
        match command {
            SessionCommand::Activate(kind) => {
                if let Err(err) = self.activate_app(kind) {
                    self.push_notice(format!("{} launch failed: {}", kind.title(), err));
                }
            }
            SessionCommand::SwitchWorkspace(workspace_id) => {
                if let Err(err) = self.switch_workspace(workspace_id) {
                    self.push_notice(format!("workspace switch failed: {}", err));
                }
            }
            SessionCommand::Notify(message) => self.push_notice(message),
            SessionCommand::OpenEditorPath(path) => {
                if let Err(err) = self.activate_app(AppKind::Editor) {
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
                    let _ = self.activate_app(AppKind::Terminal);
                }
            }
        }
    }

    fn is_locked(&self) -> bool {
        matches!(
            self.shell.client.session_snapshot(),
            Ok(snapshot) if snapshot.power_state == SessionPowerState::Locked
        ) || !self.shell.logged_in
    }

    fn set_login_visibility(&mut self, visible: bool) {
        let stage_rail_visible = false;
        set_shell_surface_visibility(
            &self.shell.client,
            &mut self.shell.lock_screen,
            visible,
        );
        self.invalidate_shell(InvalidationTarget::LockScreen, InvalidationReason::StateChanged);

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

        self.invalidate_shell(InvalidationTarget::QuickSettings, InvalidationReason::StateChanged);
        self.invalidate_shell(InvalidationTarget::CommandPalette, InvalidationReason::StateChanged);
        self.invalidate_shell(InvalidationTarget::Overview, InvalidationReason::StateChanged);

        for window in [
            self.terminal.window.as_ref(),
            self.files.window.as_ref(),
            self.settings.window.as_ref(),
            self.editor.window.as_ref(),
        ] {
            if let Some(window) = window {
                let _ = self.shell.client.set_visibility(window.window_id, !visible);
            }
        }
    }

    fn unlock_session(&mut self) {
        self.shell.logged_in = true;
        let _ = self.shell.client.set_power_state(SessionPowerState::Active);
        self.set_login_visibility(false);
        let _ = apply_workspace_visibility(
            &self.terminal.client,
            &mut self.terminal.window,
            self.terminal.workspace_id,
            self.shell.active_workspace,
        );
        let _ = apply_workspace_visibility(
            &self.files.client,
            &mut self.files.window,
            self.files.workspace_id,
            self.shell.active_workspace,
        );
        let _ = apply_workspace_visibility(
            &self.settings.client,
            &mut self.settings.window,
            self.settings.workspace_id,
            self.shell.active_workspace,
        );
        let _ = apply_workspace_visibility(
            &self.editor.client,
            &mut self.editor.window,
            self.editor.workspace_id,
            self.shell.active_workspace,
        );
        let _ = self.shell.client.focus_window(self.shell.top_bar.window_id);
        self.push_notice(String::from("Session unlocked"));
        self.mark_shell_dirty();
    }

    fn evaluate_boot_readiness(&mut self) {
        if self.shell.logged_in && !self.desktop_ready_published {
            crate::boot::appliance::publish_stage(crate::boot::appliance::BootStage::DesktopReady);
            self.desktop_ready_published = true;
        }

        let app_basket_ready = self.shell.logged_in
            && self.shell.top_bar.visible
            && self.shell.task_strip.visible
            && !self.shell.lock_screen.visible;

        if app_basket_ready && !self.app_basket_committed {
            crate::boot::appliance::publish_stage(
                crate::boot::appliance::BootStage::AppBasketReady,
            );
            crate::boot::appliance::mark_boot_success();
            self.app_basket_committed = true;
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
            self.invalidate_shell(InvalidationTarget::LockScreen, InvalidationReason::StateChanged);
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
                    let _ = self.activate_app(kind);
                    let _ = self
                        .shell
                        .client
                        .clear_app_attention(Some("auto-restored after fault"));
                    self.push_notice(format!("{} auto-restored", kind.title()));
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
        self.settings.publish_accessibility();
        self.editor.publish_accessibility();
    }

    fn activate_app(&mut self, kind: AppKind) -> Result<(), String> {
        let has_window = match kind {
            AppKind::Terminal => self.terminal.window.is_some(),
            AppKind::Files => self.files.window.is_some(),
            AppKind::Settings => self.settings.window.is_some(),
            AppKind::Editor => self.editor.window.is_some(),
        };
        let target_workspace = self.app_workspace(kind);
        if has_window && target_workspace != self.shell.active_workspace {
            self.switch_workspace(target_workspace)?;
        }

        let outcome = match kind {
            AppKind::Terminal => {
                if self.terminal.window.is_none() {
                    self.terminal.workspace_id = self.shell.active_workspace;
                }
                self.terminal.ensure_window(self.screen)?
            }
            AppKind::Files => {
                if self.files.window.is_none() {
                    self.files.workspace_id = self.shell.active_workspace;
                }
                self.files.ensure_window(self.screen)?
            }
            AppKind::Settings => {
                if self.settings.window.is_none() {
                    self.settings.workspace_id = self.shell.active_workspace;
                }
                self.settings.ensure_window(self.screen)?
            }
            AppKind::Editor => {
                if self.editor.window.is_none() {
                    self.editor.workspace_id = self.shell.active_workspace;
                }
                self.editor.ensure_window(self.screen)?
            }
        };

        let verb = match outcome {
            LaunchResult::Launched => "launched",
            LaunchResult::Restored => "restored",
            LaunchResult::Focused => "focused",
        };
        let _ = self.relayout_workspace(self.shell.active_workspace);
        self.rebuild_stage_sets();
        self.push_notice(format!("{} {}", kind.title(), verb));
        self.mark_shell_dirty();
        Ok(())
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
        self.invalidate_shell(InvalidationTarget::NotificationCenter, InvalidationReason::StateChanged);
        self.invalidate_shell(InvalidationTarget::Launcher, InvalidationReason::StateChanged);
        self.invalidate_shell(InvalidationTarget::TopBar, InvalidationReason::StateChanged);
        self.invalidate_shell(InvalidationTarget::Dock, InvalidationReason::StateChanged);
    }

    fn mark_shell_dirty(&mut self) {
        self.invalidate_shell(InvalidationTarget::TopBar, InvalidationReason::StateChanged);
        self.invalidate_shell(InvalidationTarget::Dock, InvalidationReason::StateChanged);
        self.invalidate_shell(InvalidationTarget::Launcher, InvalidationReason::StateChanged);
        self.invalidate_shell(InvalidationTarget::NotificationCenter, InvalidationReason::StateChanged);
        self.invalidate_shell(InvalidationTarget::QuickSettings, InvalidationReason::StateChanged);
        self.invalidate_shell(InvalidationTarget::CommandPalette, InvalidationReason::StateChanged);
        self.invalidate_shell(InvalidationTarget::Overview, InvalidationReason::StateChanged);
        self.invalidate_shell(InvalidationTarget::Dialog, InvalidationReason::StateChanged);
        self.invalidate_shell(InvalidationTarget::ContextMenu, InvalidationReason::StateChanged);
        self.invalidate_shell(InvalidationTarget::Switcher, InvalidationReason::StateChanged);
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
            AppKind::Settings => self.settings.workspace_id,
            AppKind::Editor => self.editor.workspace_id,
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
        self.invalidate_shell(InvalidationTarget::Switcher, InvalidationReason::StateChanged);
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
            self.invalidate_shell(InvalidationTarget::Switcher, InvalidationReason::StateChanged);
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
        let _ = self.activate_app(selected);
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
        self.invalidate_shell(InvalidationTarget::CommandPalette, InvalidationReason::StateChanged);
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
            self.invalidate_shell(InvalidationTarget::CommandPalette, InvalidationReason::StateChanged);
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
        self.invalidate_shell(InvalidationTarget::QuickSettings, InvalidationReason::StateChanged);
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
                self.invalidate_shell(InvalidationTarget::QuickSettings, InvalidationReason::StateChanged);
            }
            2 => {
                self.settings.animations = !self.settings.animations;
                self.push_notice(format!(
                    "Animations {}",
                    if self.settings.animations {
                        "enabled"
                    } else {
                        "reduced"
                    }
                ));
                self.settings.dirty = true;
                self.invalidate_shell(InvalidationTarget::QuickSettings, InvalidationReason::StateChanged);
            }
            3 => self.toggle_power_state(),
            4 => {
                let _ = self.shell.client.clear_notifications();
                self.push_notice(String::from("Notifications cleared"));
                self.invalidate_shell(InvalidationTarget::NotificationCenter, InvalidationReason::StateChanged);
                self.invalidate_shell(InvalidationTarget::QuickSettings, InvalidationReason::StateChanged);
            }
            5 => {
                commands.push(SessionCommand::SwitchWorkspace(0));
            }
            _ => {}
        }
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
        actions
    }

    fn filtered_palette_actions(&self) -> Vec<CommandPaletteAction> {
        let query = self.shell.command_query.trim().to_ascii_lowercase();
        if query.is_empty() {
            return self.command_palette_actions();
        }

        self.command_palette_actions()
            .into_iter()
            .filter(|action| {
                action.enabled
                    && (action.title.to_ascii_lowercase().contains(&query)
                        || action.category.to_ascii_lowercase().contains(&query))
            })
            .collect()
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
            1 => commands.push(SessionCommand::Activate(AppKind::Terminal)),
            2 => commands.push(SessionCommand::Activate(AppKind::Files)),
            3 => commands.push(SessionCommand::Activate(AppKind::Settings)),
            4 => commands.push(SessionCommand::Activate(AppKind::Editor)),
            10..=17 => commands.push(SessionCommand::SwitchWorkspace((selected.id - 10) as u8)),
            20 => self.apply_quick_settings_toggle(0, commands),
            21 => self.toggle_quick_settings(),
            22 => self.toggle_power_state(),
            23 => self.apply_quick_settings_toggle(4, commands),
            25 => self.toggle_overview(),
            26 => self.toggle_terminal_scratchpad(),
            24 => {
                if let Ok(entry) = self.shell.client.capture_screen("palette") {
                    self.push_notice(format!("Captured screen {}", entry.id));
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
            self.invalidate_shell(InvalidationTarget::Overview, InvalidationReason::StateChanged);
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
        self.invalidate_shell(InvalidationTarget::Overview, InvalidationReason::StateChanged);
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
        self.invalidate_shell(InvalidationTarget::Overview, InvalidationReason::StateChanged);
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
            let _ = self.activate_app(AppKind::Terminal);
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

    fn toggle_power_state(&mut self) {
        let next = match self.shell.client.session_snapshot() {
            Ok(snapshot) if snapshot.power_state == SessionPowerState::Active => {
                SessionPowerState::Locked
            }
            _ => SessionPowerState::Active,
        };
        let _ = self.shell.client.set_power_state(next);
        if next == SessionPowerState::Locked {
            self.set_login_visibility(true);
            let _ = self
                .shell
                .client
                .focus_window(self.shell.lock_screen.window_id);
        } else {
            self.unlock_session();
        }
        self.push_notice(format!("Session {}", power_state_label(next)));
        self.invalidate_shell(InvalidationTarget::TopBar, InvalidationReason::StateChanged);
        self.invalidate_shell(InvalidationTarget::Launcher, InvalidationReason::StateChanged);
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
        self.invalidate_shell(InvalidationTarget::ContextMenu, InvalidationReason::StateChanged);
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
            self.invalidate_shell(InvalidationTarget::ContextMenu, InvalidationReason::StateChanged);
        }
    }

    fn apply_context_action(&mut self, action: ContextAction) {
        let Some(kind) = self.shell.context_target else {
            return;
        };

        let result = match action {
            ContextAction::Focus => self.activate_app(kind),
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
                self.invalidate_shell(InvalidationTarget::Launcher, InvalidationReason::StateChanged);
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
                self.invalidate_shell(InvalidationTarget::NotificationCenter, InvalidationReason::StateChanged);
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
                self.invalidate_shell(InvalidationTarget::QuickSettings, InvalidationReason::StateChanged);
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
                self.invalidate_shell(InvalidationTarget::CommandPalette, InvalidationReason::StateChanged);
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
                self.invalidate_shell(InvalidationTarget::Overview, InvalidationReason::StateChanged);
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
                self.invalidate_shell(InvalidationTarget::ContextMenu, InvalidationReason::StateChanged);
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
                self.invalidate_shell(InvalidationTarget::Switcher, InvalidationReason::StateChanged);
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
                self.invalidate_shell(InvalidationTarget::LockScreen, InvalidationReason::StateChanged);
                self.push_notice(String::from("Login restored"));
            }
        }

        if self.terminal.sync(self.shell.active_workspace) {
            self.mark_shell_dirty();
        }
        if self.files.sync(self.shell.active_workspace) {
            self.mark_shell_dirty();
        }
        if self.settings.sync(self.shell.active_workspace) {
            self.mark_shell_dirty();
        }
        if self.editor.sync(self.shell.active_workspace) {
            self.mark_shell_dirty();
        }
    }

    fn service_dialog_queue(&mut self) {
        if self.shell.pending_dialog.is_none() {
            let Ok(requests) = self.shell.client.list_pending_dialogs(1) else {
                return;
            };

            if let Some(request) = requests.into_iter().next() {
                self.close_context_menu();
                self.close_switcher();
                self.shell.dialog_input = dialog_default_path(&request);
                self.shell.pending_dialog = Some(request);
                animate_shell_surface(
                    &self.shell.client,
                    &mut self.shell.dialog,
                    true,
                    self.settings.animations,
                    0.16,
                );
                let _ = self.shell.client.focus_window(self.shell.dialog.window_id);
                self.invalidate_shell(InvalidationTarget::Dialog, InvalidationReason::StateChanged);
                self.push_notice(String::from("Dialog awaiting shell decision"));
            }
        }

        if self.shell.pending_dialog.is_none()
            && (self.shell.dialog.visible || self.shell.dialog.desired_visible)
        {
            animate_shell_surface(
                &self.shell.client,
                &mut self.shell.dialog,
                false,
                self.settings.animations,
                0.12,
            );
            self.invalidate_shell(InvalidationTarget::Dialog, InvalidationReason::StateChanged);
        }
    }

    fn resolve_pending_dialog(&mut self, accept: bool) {
        let Some(request) = self.shell.pending_dialog.take() else {
            return;
        };

        let selection = if accept {
            let path = if self.shell.dialog_input.is_empty() {
                dialog_default_path(&request)
            } else {
                self.shell.dialog_input.clone()
            };
            if matches!(
                request.kind,
                DialogKind::OpenFile | DialogKind::SaveFile | DialogKind::PickFolder
            ) {
                let temp_client = DesktopClient::connect(request.app_id);
                if let Ok(client) = temp_client {
                    let _ = client.grant_file_access(&path, false);
                }
            }
            DialogSelection::Accepted(path)
        } else {
            DialogSelection::Cancelled
        };
        let _ = self.shell.client.resolve_dialog(request.id, selection);
        self.shell.dialog_input.clear();
        animate_shell_surface(
            &self.shell.client,
            &mut self.shell.dialog,
            false,
            self.settings.animations,
            0.12,
        );
        self.invalidate_shell(InvalidationTarget::Dialog, InvalidationReason::StateChanged);
        self.push_notice(format!("Dialog resolved for app {}", request.app_id));
    }

    fn render_shell(&mut self) -> Result<(), String> {
        let Some(frame_plan) = self.shell.invalidation.take_frame_plan() else {
            return Ok(());
        };

        self.shell.layout_profile = Theme::layout_profile(self.screen.width);
        let session_snapshot = self
            .shell
            .client
            .session_snapshot()
            .unwrap_or(SessionSnapshot {
                workspace_id: self.shell.active_workspace,
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
                locale: String::from("en-US"),
                theme_variant: String::from("hybrid-titan"),
                shell_state: self.shell_state(),
            });
        let snapshots = self.app_snapshots();
        let theme_mode = self.shell.theme_mode;
        let _pending_reasons = frame_plan.pending.as_slice();

        if frame_plan.touches(InvalidationTarget::TopBar) {
            let width = self.shell.top_bar.content_rect.width as usize;
            let height = self.shell.top_bar.content_rect.height as usize;
            self.shell.client.commit_scene(
                self.shell.top_bar.window_id,
                build_top_bar_scene(
                    self.shell.top_bar.window_id,
                    &mut self.text_system,
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
            self.shell.client.commit_scene(
                self.shell.task_strip.window_id,
                build_task_strip_scene(
                    self.shell.task_strip.window_id,
                    &mut self.text_system,
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
            let slide_px =
                (((1.0 - self.shell.launcher.opacity.clamp(0.0, 1.0)) * 14.0) + 0.5) as i32;
            self.shell.client.commit_scene(
                self.shell.launcher.window_id,
                apply_scene_overlay_transform(
                    build_launcher_scene(
                        self.shell.launcher.window_id,
                        &mut self.text_system,
                        width,
                        height,
                        &snapshots,
                        &session_snapshot,
                        &self.shell.notices,
                        theme_mode,
                        self.shell.layout_profile,
                    ),
                    self.shell.launcher.opacity,
                    slide_px,
                ),
            )?;
        }

        if frame_plan.touches(InvalidationTarget::NotificationCenter) && self.shell.notifications.visible
        {
            let entries = self.shell.client.list_notifications(6).unwrap_or_else(|_| Vec::new());
            let width = self.shell.notifications.content_rect.width as usize;
            let height = self.shell.notifications.content_rect.height as usize;
            let slide_px =
                (((1.0 - self.shell.notifications.opacity.clamp(0.0, 1.0)) * 14.0) + 0.5) as i32;
            self.shell.client.commit_scene(
                self.shell.notifications.window_id,
                apply_scene_overlay_transform(
                    build_notifications_scene(
                        self.shell.notifications.window_id,
                        &mut self.text_system,
                        width,
                        height,
                        &entries,
                        self.shell.notification_index,
                        theme_mode,
                    ),
                    self.shell.notifications.opacity,
                    slide_px,
                ),
            )?;
        }

        if frame_plan.touches(InvalidationTarget::QuickSettings) && self.shell.quick_settings.visible
        {
            let width = self.shell.quick_settings.content_rect.width as usize;
            let height = self.shell.quick_settings.content_rect.height as usize;
            let slide_px =
                (((1.0 - self.shell.quick_settings.opacity.clamp(0.0, 1.0)) * 14.0) + 0.5) as i32;
            self.shell.client.commit_scene(
                self.shell.quick_settings.window_id,
                apply_scene_overlay_transform(
                    build_quick_settings_scene(
                        self.shell.quick_settings.window_id,
                        &mut self.text_system,
                        width,
                        height,
                        theme_mode,
                        self.settings.notifications,
                        self.settings.animations,
                    ),
                    self.shell.quick_settings.opacity,
                    slide_px,
                ),
            )?;
        }

        if frame_plan.touches(InvalidationTarget::CommandPalette)
            && self.shell.command_palette.visible
        {
            let actions = self.filtered_palette_actions();
            let width = self.shell.command_palette.content_rect.width as usize;
            let height = self.shell.command_palette.content_rect.height as usize;
            let slide_px =
                (((1.0 - self.shell.command_palette.opacity.clamp(0.0, 1.0)) * 14.0) + 0.5)
                    as i32;
            self.shell.client.commit_scene(
                self.shell.command_palette.window_id,
                apply_scene_overlay_transform(
                    build_command_palette_scene(
                        self.shell.command_palette.window_id,
                        &mut self.text_system,
                        width,
                        height,
                        &actions,
                        &self.shell.command_query,
                        self.shell.command_selection,
                        theme_mode,
                    ),
                    self.shell.command_palette.opacity,
                    slide_px,
                ),
            )?;
        }

        if frame_plan.touches(InvalidationTarget::Overview) && self.shell.stage_rail.visible {
            let width = self.shell.stage_rail.content_rect.width as usize;
            let height = self.shell.stage_rail.content_rect.height as usize;
            let slide_px =
                (((1.0 - self.shell.stage_rail.opacity.clamp(0.0, 1.0)) * 14.0) + 0.5) as i32;
            self.shell.client.commit_scene(
                self.shell.stage_rail.window_id,
                apply_scene_overlay_transform(
                    build_stage_rail_scene(
                        self.shell.stage_rail.window_id,
                        &mut self.text_system,
                        width,
                        height,
                        &self.shell.stage_sets,
                        self.shell.active_stage_set,
                        theme_mode,
                    ),
                    self.shell.stage_rail.opacity,
                    slide_px,
                ),
            )?;
        }

        if frame_plan.touches(InvalidationTarget::Dialog) && self.shell.dialog.visible {
            let width = self.shell.dialog.content_rect.width as usize;
            let height = self.shell.dialog.content_rect.height as usize;
            let slide_px =
                (((1.0 - self.shell.dialog.opacity.clamp(0.0, 1.0)) * 14.0) + 0.5) as i32;
            self.shell.client.commit_scene(
                self.shell.dialog.window_id,
                apply_scene_overlay_transform(
                    build_dialog_scene(
                        self.shell.dialog.window_id,
                        &mut self.text_system,
                        width,
                        height,
                        self.shell.pending_dialog.as_ref(),
                        &self.shell.dialog_input,
                        theme_mode,
                    ),
                    self.shell.dialog.opacity,
                    slide_px,
                ),
            )?;
        }

        if frame_plan.touches(InvalidationTarget::ContextMenu) && self.shell.context_menu.visible {
            let width = self.shell.context_menu.content_rect.width as usize;
            let height = self.shell.context_menu.content_rect.height as usize;
            let slide_px =
                (((1.0 - self.shell.context_menu.opacity.clamp(0.0, 1.0)) * 14.0) + 0.5) as i32;
            self.shell.client.commit_scene(
                self.shell.context_menu.window_id,
                apply_scene_overlay_transform(
                    build_context_menu_scene(
                        self.shell.context_menu.window_id,
                        &mut self.text_system,
                        width,
                        height,
                        self.shell.context_target,
                        theme_mode,
                    ),
                    self.shell.context_menu.opacity,
                    slide_px,
                ),
            )?;
        }

        if frame_plan.touches(InvalidationTarget::Switcher) && self.shell.switcher.visible {
            let candidates = self.switcher_candidates();
            let width = self.shell.switcher.content_rect.width as usize;
            let height = self.shell.switcher.content_rect.height as usize;
            let slide_px =
                (((1.0 - self.shell.switcher.opacity.clamp(0.0, 1.0)) * 14.0) + 0.5) as i32;
            self.shell.client.commit_scene(
                self.shell.switcher.window_id,
                apply_scene_overlay_transform(
                    build_switcher_scene(
                        self.shell.switcher.window_id,
                        &mut self.text_system,
                        width,
                        height,
                        &candidates,
                        self.shell.switcher_index,
                        self.shell.active_workspace,
                        theme_mode,
                    ),
                    self.shell.switcher.opacity,
                    slide_px,
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
        self.settings.render()?;
        self.editor.render()?;
        Ok(())
    }

    fn app_snapshots(&self) -> Vec<AppSnapshot> {
        let mut snapshots = vec![
            self.terminal.snapshot(),
            self.files.snapshot(),
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
    Activate(AppKind),
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

fn decode_terminal_output(raw: &[u8]) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut index = 0;

    while index < raw.len() {
        match raw[index] {
            0x1B => {
                if index + 1 < raw.len() && raw[index + 1] == b'[' {
                    index += 2;
                    let mut final_byte = 0;
                    while index < raw.len() {
                        let byte = raw[index];
                        if (byte as char).is_ascii_alphabetic() {
                            final_byte = byte;
                            break;
                        }
                        index += 1;
                    }
                    if final_byte == b'J' {
                        lines.push(String::from("__CLEAR__"));
                        current.clear();
                    }
                }
            }
            b'\r' => {}
            b'\n' => {
                if !current.trim_end().is_empty() {
                    lines.push(current.trim_end().to_string());
                }
                current.clear();
            }
            0x08 => {
                current.pop();
            }
            byte if byte.is_ascii_graphic() || byte == b' ' => current.push(byte as char),
            _ => {}
        }
        index += 1;
    }

    if !current.trim_end().is_empty() {
        lines.push(current.trim_end().to_string());
    }

    lines
}

fn parent_path(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "/" {
        return String::from("/");
    }
    let mut parts = trimmed.rsplitn(2, '/');
    let _name = parts.next();
    let parent = parts.next().unwrap_or("");
    if parent.is_empty() {
        String::from("/")
    } else {
        format!("/{}", parent.trim_start_matches('/'))
    }
}

fn join_path(base: &str, name: &str) -> String {
    if base == "/" {
        format!("/{}", name.trim_start_matches('/'))
    } else {
        format!(
            "{}/{}",
            base.trim_end_matches('/'),
            name.trim_start_matches('/')
        )
    }
}

fn entry_launch_kind(path: &str) -> Option<AppKind> {
    match path {
        "/proc" | "/sys" | "/dev" => Some(AppKind::Files),
        "/settings" => Some(AppKind::Settings),
        _ => None,
    }
}

fn file_association_kind(path: &str) -> Option<AppKind> {
    if let Some(kind) = entry_launch_kind(path) {
        return Some(kind);
    }
    match path.rsplit('.').next() {
        Some("txt" | "rs" | "md" | "cfg" | "json" | "toml" | "log") => Some(AppKind::Editor),
        Some("png" | "jpg" | "jpeg" | "bmp") => Some(AppKind::Files),
        _ => Some(AppKind::Editor),
    }
}

fn file_association_label(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("txt" | "rs" | "md" | "cfg" | "json" | "toml" | "log") => "editor",
        Some("png" | "jpg" | "jpeg" | "bmp") => "preview",
        _ => "open",
    }
}

fn thumbnail_color_for_path(path: &str) -> u32 {
    match path.rsplit('.').next() {
        Some("png" | "jpg" | "jpeg" | "bmp") => ACCENT_CORAL,
        Some("rs" | "toml") => ACCENT_BLUE,
        Some("cfg" | "json" | "log") => ACCENT_GOLD,
        _ => ACCENT_SOFT,
    }
}

fn thumbnail_label_for_path(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("png" | "jpg" | "jpeg" | "bmp") => "I",
        Some("rs") => "R",
        Some("toml" | "cfg" | "json") => "C",
        _ => "F",
    }
}
impl TerminalApp {
    fn ensure_window(&mut self, screen: Rect) -> Result<LaunchResult, String> {
        self.ensure_backend()?;
        let result = ensure_window_visible(
            &self.client,
            &mut self.window,
            "Terminal",
            Rect::new(screen.x + 86, screen.y + 126, 720, 420),
            self.workspace_id,
        )?;
        self.sync_winsize();
        self.dirty = true;
        Ok(result)
    }

    fn ensure_backend(&mut self) -> Result<(), String> {
        if self.pty.is_some() {
            return Ok(());
        }

        let pair = PTY_MANAGER
            .create_pair()
            .map_err(|err| format!("pty create failed: {:?}", err))?;
        configure_pty_for_shell(&pair);
        write_welcome_message(&pair);
        self.pty = Some(pair);
        self.pull_pty_output();
        Ok(())
    }

    fn sync(&mut self, active_workspace: u8) -> bool {
        match sync_window_state(
            &self.client,
            &mut self.window,
            self.workspace_id,
            active_workspace,
        ) {
            WindowSync::Changed => {
                self.sync_winsize();
                self.dirty = true;
                true
            }
            WindowSync::Closed => {
                let _ = self.client.mark_app_exited(true, "window closed");
                self.dirty = true;
                true
            }
            WindowSync::Unchanged => false,
        }
    }

    fn handle_events(&mut self, events: Vec<WindowInputEvent>, commands: &mut Vec<SessionCommand>) {
        let Some(window_id) = self.window.as_ref().map(|window| window.window_id) else {
            return;
        };

        for event in events {
            if event.window_id != window_id {
                continue;
            }
            if is_backspace_key(&event.event) {
                self.input.pop();
                self.dirty = true;
            } else if is_enter_key(&event.event) {
                self.submit_line(commands);
            } else if let Some(ch) = printable_key(&event.event) {
                self.input.push(ch);
                self.dirty = true;
            }
        }
    }

    fn submit_line(&mut self, commands: &mut Vec<SessionCommand>) {
        let command = self.input.trim().to_string();
        if command.is_empty() {
            self.input.clear();
            self.dirty = true;
            return;
        }

        match command.as_str() {
            "help" => {
                self.lines.push(String::from(
                    "local: clear | open terminal|files|settings|editor | copy <text> | paste | open-file | save-file | pick-folder | screenshot | grants | accessibility",
                ));
                self.lines.push(String::from(
                    "shell: pwd | cd <dir> | ls [path] | tree [path] | find [path] -name <glob> | stat <path> | cp <src> <dst> | mv | rm | mkdir | touch | head | tail | wc | grep | sort | uniq | env | history | alias | which | command",
                ));
            }
            "clear" => {
                self.lines.clear();
                self.lines.push(String::from("screen cleared"));
            }
            "open terminal" => commands.push(SessionCommand::Activate(AppKind::Terminal)),
            "open files" => commands.push(SessionCommand::Activate(AppKind::Files)),
            "open settings" => commands.push(SessionCommand::Activate(AppKind::Settings)),
            "open editor" => commands.push(SessionCommand::Activate(AppKind::Editor)),
            "paste" => match self.client.clipboard_get() {
                Ok(ClipboardPayload::Text(text)) => self.lines.push(format!("clipboard: {}", text)),
                Ok(ClipboardPayload::Files(paths)) => {
                    self.lines.push(format!("clipboard files: {}", paths.len()))
                }
                Ok(ClipboardPayload::Empty) => self.lines.push(String::from("clipboard empty")),
                Err(_) => self.lines.push(String::from("clipboard unavailable")),
            },
            "open-file" => {
                if let Ok(dialog_id) = self
                    .client
                    .open_file_dialog("Open File", "/workspace/demo.txt")
                {
                    self.pending_dialogs.push(dialog_id);
                    self.lines.push(String::from("file dialog requested"));
                }
            }
            "save-file" => {
                if let Ok(dialog_id) = self
                    .client
                    .save_file_dialog("Save File", "/workspace/output.txt")
                {
                    self.pending_dialogs.push(dialog_id);
                    self.lines.push(String::from("save dialog requested"));
                }
            }
            "pick-folder" => {
                if let Ok(dialog_id) = self.client.pick_folder_dialog("Pick Folder", "/workspace") {
                    self.pending_dialogs.push(dialog_id);
                    self.lines.push(String::from("folder dialog requested"));
                }
            }
            other if other.starts_with("notify ") => {
                commands.push(SessionCommand::Notify(other[7..].trim().to_string()));
            }
            other if other.starts_with("copy ") => {
                let text = other[5..].trim();
                let _ = self
                    .client
                    .clipboard_set(ClipboardPayload::Text(String::from(text)));
                self.lines.push(String::from("clipboard updated"));
            }
            "screenshot" => match self.client.capture_screen("terminal-request") {
                Ok(entry) => self.lines.push(format!(
                    "capture {} {}x{}",
                    entry.id, entry.width, entry.height
                )),
                Err(err) => self.lines.push(format!("capture failed: {}", err)),
            },
            "screenshot-save" => match self.client.capture_screen("terminal-save") {
                Ok(entry) => {
                    let path = format!("/workspace/capture-{}.ppm", entry.id);
                    match self.client.save_capture_ppm(entry.id, &path) {
                        Ok(()) => self.lines.push(format!("saved {}", path)),
                        Err(err) => self.lines.push(format!("save failed: {}", err)),
                    }
                }
                Err(err) => self.lines.push(format!("capture failed: {}", err)),
            },
            "grants" => match self.client.list_file_grants() {
                Ok(grants) => {
                    for grant in grants {
                        self.lines.push(format!("grant {}", grant.path_prefix));
                    }
                }
                Err(err) => self.lines.push(format!("grant list failed: {}", err)),
            },
            "accessibility" => match self.client.accessibility_tree() {
                Ok(nodes) => self.lines.push(format!("a11y nodes: {}", nodes.len())),
                Err(err) => self.lines.push(format!("a11y failed: {}", err)),
            },
            _ => {
                if self.execute_pty_command(&command).is_err() {
                    self.lines.push(String::from("command execution failed"));
                }
            }
        }

        while self.lines.len() > 22 {
            self.lines.remove(0);
        }
        self.input.clear();
        self.dirty = true;
    }

    fn execute_pty_command(&mut self, command: &str) -> Result<(), String> {
        self.ensure_backend()?;
        let Some(pair) = self.pty.as_ref() else {
            return Err(String::from("pty unavailable"));
        };

        let _ = pair.master.write(command.as_bytes());
        let _ = pair.master.write(b"\n");
        let _ = execute_command_on_pty_with_shell(pair, &mut self.shell, command);
        let _ = pair.slave.write(b"$ ");
        self.pull_pty_output();
        Ok(())
    }

    fn pull_pty_output(&mut self) {
        let Some(pair) = self.pty.as_ref() else {
            return;
        };
        if !pty_has_output(pair) {
            return;
        }

        let mut raw = Vec::new();
        let mut chunk = [0u8; 512];
        loop {
            let Ok(read) = pair.master.read(&mut chunk) else {
                break;
            };
            if read == 0 {
                break;
            }
            raw.extend_from_slice(&chunk[..read]);
            if read < chunk.len() {
                break;
            }
        }

        if raw.is_empty() {
            return;
        }

        for line in decode_terminal_output(&raw) {
            if line == "__CLEAR__" {
                self.lines.clear();
            } else {
                self.lines.push(line);
            }
        }
        while self.lines.len() > 128 {
            self.lines.remove(0);
        }
        self.dirty = true;
    }

    fn poll_platform(&mut self) {
        let mut completed = Vec::new();
        for dialog_id in self.pending_dialogs.iter().copied() {
            if let Ok(Some(result)) = self.client.poll_dialog_result(dialog_id) {
                match result.selection {
                    DialogSelection::Accepted(path) => {
                        self.lines.push(format!("dialog accepted: {}", path));
                    }
                    DialogSelection::Cancelled => {
                        self.lines.push(String::from("dialog cancelled"));
                    }
                }
                completed.push(dialog_id);
                self.dirty = true;
            }
        }
        self.pending_dialogs
            .retain(|dialog_id| !completed.iter().any(|done| done == dialog_id));
        self.pull_pty_output();
    }

    fn sync_winsize(&mut self) {
        let Some(pair) = self.pty.as_ref() else {
            return;
        };
        let Some(window) = self.window else {
            return;
        };

        let cols = max((window.content_rect.width as i32 - 36) / FONT_WIDTH, 20) as u16;
        let rows = max((window.content_rect.height as i32 - 118) / 18, 8) as u16;
        pair.slave.set_winsize(Winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: window.content_rect.width as u16,
            ws_ypixel: window.content_rect.height as u16,
        });
    }

    fn render(&mut self) -> Result<(), String> {
        let Some(window) = self.window else {
            return Ok(());
        };
        if !window.visible || !self.dirty {
            return Ok(());
        }

        let mut canvas = Canvas::new(
            window.content_rect.width as usize,
            window.content_rect.height as usize,
            WINDOW_BG,
        );
        canvas.fill_rect(Rect::new(0, 0, window.content_rect.width, 56), PANEL_BG);
        canvas.fill_rect(Rect::new(0, 56, window.content_rect.width, 1), BORDER);
        canvas.draw_text(18, 18, "Native Terminal", TEXT_PRIMARY);
        canvas.draw_text(18, 72, "Commands", TEXT_MUTED);

        let available_rows = ((window.content_rect.height as i32 - 138).max(0) / 18) as usize;
        let start = self.lines.len().saturating_sub(available_rows);
        let mut y = 96;
        for line in self.lines.iter().skip(start) {
            canvas.draw_text(18, y, line, TEXT_SECONDARY);
            y += 18;
        }

        let footer_y = max(window.content_rect.height as i32 - 34, 0);
        canvas.fill_rect(
            Rect::new(0, footer_y, window.content_rect.width, 34),
            PANEL_BG,
        );
        canvas.fill_rect(Rect::new(0, footer_y, window.content_rect.width, 1), BORDER);
        canvas.draw_text(18, footer_y + 10, ">", ACCENT_MINT);
        canvas.draw_text(34, footer_y + 10, &self.input, TEXT_PRIMARY);

        self.client
            .present(window.window_id, &canvas.into_pixels())?;
        self.dirty = false;
        Ok(())
    }

    fn snapshot(&self) -> AppSnapshot {
        snapshot_for_window(
            AppKind::Terminal,
            self.window,
            self.workspace_id,
            format!("pty {} lines", self.lines.len().saturating_sub(1)),
        )
    }

    fn publish_accessibility(&self) {
        let Some(window) = self.window else {
            return;
        };
        let nodes = vec![
            AccessibilityNode {
                id: 1,
                app_id: self.client.app_id(),
                role: AccessibilityRole::Window,
                label: String::from("Terminal"),
                description: String::from("pty terminal window"),
                focused: window.focused,
                bounds: window.content_rect,
            },
            AccessibilityNode {
                id: 2,
                app_id: self.client.app_id(),
                role: AccessibilityRole::Input,
                label: String::from("Command Input"),
                description: self.input.clone(),
                focused: window.focused,
                bounds: Rect::new(
                    0,
                    window.content_rect.height as i32 - 34,
                    window.content_rect.width,
                    34,
                ),
            },
        ];
        let _ = self.client.publish_accessibility_tree(nodes);
    }
}

impl FilesApp {
    fn ensure_window(&mut self, screen: Rect) -> Result<LaunchResult, String> {
        let result = ensure_window_visible(
            &self.client,
            &mut self.window,
            "Files",
            Rect::new(screen.x + 232, screen.y + 168, 580, 360),
            self.workspace_id,
        )?;
        if crate::boot::appliance::auto_login_requested() && self.entries.is_empty() {
            self.status = format!("Deferred appliance scan for {}", self.current_path);
            self.dirty = true;
            return Ok(result);
        }
        self.refresh()?;
        self.dirty = true;
        Ok(result)
    }

    fn sync(&mut self, active_workspace: u8) -> bool {
        match sync_window_state(
            &self.client,
            &mut self.window,
            self.workspace_id,
            active_workspace,
        ) {
            WindowSync::Changed => {
                self.dirty = true;
                true
            }
            WindowSync::Closed => {
                let _ = self.client.mark_app_exited(true, "window closed");
                self.dirty = true;
                true
            }
            WindowSync::Unchanged => false,
        }
    }

    fn handle_events(&mut self, events: Vec<WindowInputEvent>, commands: &mut Vec<SessionCommand>) {
        let Some(window_id) = self.window.as_ref().map(|window| window.window_id) else {
            return;
        };

        for event in events {
            if event.window_id != window_id {
                continue;
            }
            let input = &event.event;
            match input {
                InputEvent::PointerButton {
                    state: KeyState::Pressed,
                    ..
                } => {
                    if let Some(local) = event.local_position {
                        if let Some(index) = files_hit(local, self.entries.len()) {
                            self.selected = index.min(self.entries.len().saturating_sub(1));
                            self.dirty = true;
                            self.activate_selected(commands);
                        }
                    }
                }
                InputEvent::Key { .. } => {
                    if ctrl_scan_pressed(input, 0x13) {
                        let _ = self.refresh();
                        continue;
                    }
                    if key_scan_pressed(input, 0x50) || key_scan_pressed(input, 0x24) {
                        self.selected =
                            (self.selected + 1).min(self.entries.len().saturating_sub(1));
                        self.dirty = true;
                    } else if key_scan_pressed(input, 0x48) || key_scan_pressed(input, 0x25) {
                        self.selected = self.selected.saturating_sub(1);
                        self.dirty = true;
                    } else if is_enter_key(input) {
                        self.activate_selected(commands);
                    } else if key_scan_pressed(input, 0x23) || is_backspace_key(input) {
                        let _ = self.navigate_up();
                    } else if key_scan_pressed(input, 0x20) {
                        let _ = self.delete_selected();
                    } else if key_scan_pressed(input, 0x32) {
                        let _ = self.rename_selected();
                    } else if key_scan_pressed(input, 0x31) {
                        let _ = self.create_directory("new-folder");
                    } else if key_scan_pressed(input, 0x13) {
                        let _ = self.refresh();
                    }
                }
                _ => {}
            }
        }
    }

    fn refresh(&mut self) -> Result<(), String> {
        let entries = self.client.list_directory(&self.current_path)?;
        self.entries = entries;
        self.selected = self.selected.min(self.entries.len().saturating_sub(1));
        self.status = format!("{} items in {}", self.entries.len(), self.current_path);
        self.dirty = true;
        Ok(())
    }

    fn activate_selected(&mut self, commands: &mut Vec<SessionCommand>) {
        let Some(entry) = self.entries.get(self.selected).cloned() else {
            return;
        };

        if entry.is_directory {
            self.current_path = entry.path;
            if let Err(err) = self.refresh() {
                self.status = err;
                self.dirty = true;
            }
            return;
        }

        if let Some(kind) = file_association_kind(&entry.path) {
            commands.push(SessionCommand::Activate(kind));
            return;
        }

        commands.push(SessionCommand::OpenEditorPath(entry.path));
    }

    fn navigate_up(&mut self) -> Result<(), String> {
        self.current_path = parent_path(&self.current_path);
        self.refresh()
    }

    fn delete_selected(&mut self) -> Result<(), String> {
        let Some(entry) = self.entries.get(self.selected).cloned() else {
            return Ok(());
        };

        if entry.is_directory {
            self.client.delete_file(&entry.path)?;
        } else {
            self.client.delete_file(&entry.path)?;
        }
        self.status = format!("Removed {}", entry.name);
        self.refresh()
    }

    fn create_directory(&mut self, name: &str) -> Result<(), String> {
        let path = join_path(&self.current_path, name);
        self.client.create_directory(&path)?;
        self.status = format!("Created {}", path);
        self.refresh()
    }

    fn rename_selected(&mut self) -> Result<(), String> {
        let Some(entry) = self.entries.get(self.selected).cloned() else {
            return Ok(());
        };
        if entry.is_directory {
            self.status = String::from("directory rename unsupported");
            self.dirty = true;
            return Ok(());
        }
        let new_name = if let Some((stem, ext)) = entry.name.rsplit_once('.') {
            format!("{}-renamed.{}", stem, ext)
        } else {
            format!("{}-renamed", entry.name)
        };
        let new_path = join_path(&self.current_path, &new_name);
        let data = self.client.read_file(&entry.path)?;
        self.client.write_file(&new_path, &data)?;
        self.client.delete_file(&entry.path)?;
        self.status = format!("Renamed {} -> {}", entry.name, new_name);
        self.refresh()
    }

    fn render(&mut self) -> Result<(), String> {
        let Some(window) = self.window else {
            return Ok(());
        };
        if !window.visible || !self.dirty {
            return Ok(());
        }

        let mut canvas = Canvas::new(
            window.content_rect.width as usize,
            window.content_rect.height as usize,
            WINDOW_BG,
        );
        canvas.fill_rect(Rect::new(0, 0, window.content_rect.width, 60), PANEL_BG);
        canvas.fill_rect(Rect::new(0, 60, window.content_rect.width, 1), BORDER);
        canvas.draw_text(18, 18, "Files", TEXT_PRIMARY);
        canvas.draw_text(18, 38, &self.current_path, TEXT_MUTED);

        let mut y = 86;
        for (index, entry) in self.entries.iter().enumerate() {
            let selected = index == self.selected;
            let rect = Rect::new(18, y - 8, window.content_rect.width.saturating_sub(36), 48);
            canvas.fill_rect(rect, if selected { PANEL_ALT } else { PANEL_BG });
            let accent = if entry.is_directory {
                ACCENT_BLUE
            } else {
                thumbnail_color_for_path(&entry.path)
            };
            canvas.stroke_rect(rect, if selected { accent } else { BORDER });
            canvas.fill_rect(Rect::new(rect.x + 12, rect.y + 12, 18, 18), accent);
            canvas.draw_text(
                rect.x + 16,
                rect.y + 18,
                thumbnail_label_for_path(&entry.path),
                WINDOW_BG,
            );
            canvas.draw_text(34, y + 4, &entry.name, TEXT_PRIMARY);
            let detail = if entry.is_directory {
                String::from("directory")
            } else {
                format!(
                    "{} bytes  {}",
                    entry.size,
                    file_association_label(&entry.path)
                )
            };
            canvas.draw_text(34, y + 22, &detail, TEXT_SECONDARY);
            y += 58;
            if y > window.content_rect.height as i32 - 56 {
                break;
            }
        }

        let footer_y = max(window.content_rect.height as i32 - 34, 0);
        canvas.fill_rect(
            Rect::new(0, footer_y, window.content_rect.width, 34),
            PANEL_BG,
        );
        canvas.fill_rect(Rect::new(0, footer_y, window.content_rect.width, 1), BORDER);
        canvas.draw_text(18, footer_y + 10, &self.status, TEXT_MUTED);

        self.client
            .present(window.window_id, &canvas.into_pixels())?;
        self.dirty = false;
        Ok(())
    }

    fn snapshot(&self) -> AppSnapshot {
        snapshot_for_window(
            AppKind::Files,
            self.window,
            self.workspace_id,
            format!("{} [{}]", self.current_path, self.entries.len()),
        )
    }

    fn publish_accessibility(&self) {
        let Some(window) = self.window else {
            return;
        };
        let mut nodes = vec![AccessibilityNode {
            id: 1,
            app_id: self.client.app_id(),
            role: AccessibilityRole::Window,
            label: String::from("Files"),
            description: self.current_path.clone(),
            focused: window.focused,
            bounds: window.content_rect,
        }];
        for (index, entry) in self.entries.iter().take(12).enumerate() {
            nodes.push(AccessibilityNode {
                id: (index + 2) as u64,
                app_id: self.client.app_id(),
                role: AccessibilityRole::ListItem,
                label: entry.name.clone(),
                description: entry.path.clone(),
                focused: index == self.selected,
                bounds: Rect::new(
                    18,
                    78 + index as i32 * 58,
                    window.content_rect.width.saturating_sub(36),
                    48,
                ),
            });
        }
        let _ = self.client.publish_accessibility_tree(nodes);
    }
}
impl SettingsApp {
    fn ensure_window(&mut self, screen: Rect) -> Result<LaunchResult, String> {
        let result = ensure_window_visible(
            &self.client,
            &mut self.window,
            "Settings",
            Rect::new(screen.x + 314, screen.y + 108, 480, 520),
            self.workspace_id,
        )?;
        self.dirty = true;
        Ok(result)
    }

    fn sync(&mut self, active_workspace: u8) -> bool {
        match sync_window_state(
            &self.client,
            &mut self.window,
            self.workspace_id,
            active_workspace,
        ) {
            WindowSync::Changed => {
                self.dirty = true;
                true
            }
            WindowSync::Closed => {
                let _ = self.client.mark_app_exited(true, "window closed");
                self.dirty = true;
                true
            }
            WindowSync::Unchanged => false,
        }
    }

    fn handle_events(&mut self, events: Vec<WindowInputEvent>) {
        let Some(window_id) = self.window.as_ref().map(|window| window.window_id) else {
            return;
        };

        for event in events {
            if event.window_id != window_id {
                continue;
            }
            let input = &event.event;
            match input {
                InputEvent::PointerButton {
                    state: KeyState::Pressed,
                    ..
                } => {
                    if let Some(local) = event.local_position {
                        if let Some(index) = settings_hit(local) {
                            self.toggle(index);
                        }
                    }
                }
                InputEvent::Key { .. } => match digit_key_pressed(input) {
                    Some(1) => self.toggle(0),
                    Some(2) => self.toggle(1),
                    Some(3) => self.toggle(2),
                    Some(4) => self.toggle(3),
                    Some(5) => self.toggle(4),
                    Some(6) => self.toggle(5),
                    _ => {}
                },
                _ => {}
            }
        }
    }

    fn toggle(&mut self, index: usize) {
        match index {
            0 => self.focus_mode = !self.focus_mode,
            1 => self.animations = !self.animations,
            2 => self.notifications = !self.notifications,
            3 => {
                let next = toggle_permission(
                    self.client
                        .permission_state(DesktopPermission::ClipboardRead),
                );
                let _ = self
                    .client
                    .set_permission(DesktopPermission::ClipboardRead, next);
            }
            4 => {
                let next =
                    toggle_permission(self.client.permission_state(DesktopPermission::FileDialogs));
                let _ = self
                    .client
                    .set_permission(DesktopPermission::FileDialogs, next);
            }
            5 => {
                let next = match self.client.session_snapshot() {
                    Ok(snapshot) if snapshot.power_state == SessionPowerState::Active => {
                        SessionPowerState::Locked
                    }
                    _ => SessionPowerState::Active,
                };
                let _ = self.client.set_power_state(next);
            }
            _ => {}
        }
        self.dirty = true;
    }

    fn render(&mut self) -> Result<(), String> {
        let Some(window) = self.window else {
            return Ok(());
        };
        if !window.visible || !self.dirty {
            return Ok(());
        }

        let mut canvas = Canvas::new(
            window.content_rect.width as usize,
            window.content_rect.height as usize,
            WINDOW_BG,
        );
        canvas.fill_rect(Rect::new(0, 0, window.content_rect.width, 60), PANEL_BG);
        canvas.fill_rect(Rect::new(0, 60, window.content_rect.width, 1), BORDER);
        canvas.draw_text(18, 18, "Settings", TEXT_PRIMARY);
        let session = self.client.session_snapshot().unwrap_or(SessionSnapshot {
            workspace_id: self.workspace_id,
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
            locale: String::from("en-US"),
            theme_variant: String::from("hybrid-titan"),
            shell_state: ShellState::DesktopReady,
        });
        let clipboard_permission = self
            .client
            .permission_state(DesktopPermission::ClipboardRead)
            .unwrap_or(PermissionState::Ask);
        let dialog_permission = self
            .client
            .permission_state(DesktopPermission::FileDialogs)
            .unwrap_or(PermissionState::Ask);
        canvas.draw_text(
            18,
            38,
            &format!(
                "{}  {} running  {} faults",
                power_state_label(session.power_state),
                session.apps_running,
                session.apps_crashed
            ),
            TEXT_MUTED,
        );

        let rows = [
            ("Focus mode", self.focus_mode, ACCENT_GOLD),
            ("Animation pacing", self.animations, ACCENT_BLUE),
            ("Notification surface", self.notifications, ACCENT_SOFT),
            (
                "Clipboard read permission",
                clipboard_permission == PermissionState::Granted,
                ACCENT_MINT,
            ),
            (
                "File dialog permission",
                dialog_permission == PermissionState::Granted,
                ACCENT_BLUE,
            ),
            (
                "Session lock state",
                session.power_state == SessionPowerState::Locked,
                ACCENT_CORAL,
            ),
        ];

        let mut y = 88;
        for (index, (label, enabled, accent)) in rows.iter().enumerate() {
            let rect = Rect::new(20, y - 8, window.content_rect.width.saturating_sub(40), 54);
            canvas.fill_rect(rect, PANEL_BG);
            canvas.stroke_rect(rect, if *enabled { *accent } else { BORDER });
            canvas.draw_text(34, y + 4, label, TEXT_PRIMARY);
            canvas.draw_text(
                34,
                y + 24,
                match index {
                    3 => permission_state_label(clipboard_permission),
                    4 => permission_state_label(dialog_permission),
                    5 => power_state_label(session.power_state),
                    _ => {
                        if *enabled {
                            "Enabled"
                        } else {
                            "Disabled"
                        }
                    }
                },
                if *enabled { *accent } else { TEXT_SECONDARY },
            );
            canvas.draw_text(
                rect.right() - 54,
                y + 14,
                &(index + 1).to_string(),
                TEXT_MUTED,
            );
            y += 64;
        }

        self.client
            .present(window.window_id, &canvas.into_pixels())?;
        self.dirty = false;
        Ok(())
    }

    fn snapshot(&self) -> AppSnapshot {
        let enabled = [self.focus_mode, self.animations, self.notifications]
            .iter()
            .filter(|flag| **flag)
            .count();
        snapshot_for_window(
            AppKind::Settings,
            self.window,
            self.workspace_id,
            format!("{} toggles on, shell policy live", enabled),
        )
    }

    fn publish_accessibility(&self) {
        let Some(window) = self.window else {
            return;
        };
        let nodes = vec![
            AccessibilityNode {
                id: 1,
                app_id: self.client.app_id(),
                role: AccessibilityRole::Window,
                label: String::from("Settings"),
                description: String::from("desktop policy settings"),
                focused: window.focused,
                bounds: window.content_rect,
            },
            AccessibilityNode {
                id: 2,
                app_id: self.client.app_id(),
                role: AccessibilityRole::Button,
                label: String::from("Focus mode"),
                description: self.focus_mode.to_string(),
                focused: false,
                bounds: Rect::new(20, 80, 420, 54),
            },
        ];
        let _ = self.client.publish_accessibility_tree(nodes);
    }
}

impl EditorApp {
    fn ensure_window(&mut self, screen: Rect) -> Result<LaunchResult, String> {
        let result = ensure_window_visible(
            &self.client,
            &mut self.window,
            "Editor",
            Rect::new(screen.x + 162, screen.y + 148, 620, 400),
            self.workspace_id,
        )?;
        self.dirty = true;
        Ok(result)
    }

    fn sync(&mut self, active_workspace: u8) -> bool {
        match sync_window_state(
            &self.client,
            &mut self.window,
            self.workspace_id,
            active_workspace,
        ) {
            WindowSync::Changed => {
                self.dirty = true;
                true
            }
            WindowSync::Closed => {
                let _ = self.client.mark_app_exited(true, "window closed");
                self.dirty = true;
                true
            }
            WindowSync::Unchanged => false,
        }
    }

    fn handle_events(&mut self, events: Vec<WindowInputEvent>) {
        let Some(window_id) = self.window.as_ref().map(|window| window.window_id) else {
            return;
        };

        for event in events {
            if event.window_id != window_id {
                continue;
            }
            if ctrl_scan_pressed(&event.event, 0x1F) {
                let _ = self.save_document();
                continue;
            }
            if ctrl_scan_pressed(&event.event, 0x18) {
                if let Ok(dialog_id) = self
                    .client
                    .open_file_dialog("Open File", self.path.as_deref().unwrap_or("/"))
                {
                    self.pending_dialogs.push(EditorDialog {
                        id: dialog_id,
                        kind: EditorDialogKind::Open,
                    });
                }
                continue;
            }

            if is_backspace_key(&event.event) {
                self.text.pop();
                self.document_dirty = true;
                self.dirty = true;
            } else if is_enter_key(&event.event) {
                self.text.push('\n');
                self.document_dirty = true;
                self.dirty = true;
            } else if let Some(ch) = printable_key(&event.event) {
                self.text.push(ch);
                self.document_dirty = true;
                self.dirty = true;
            }
        }
    }

    fn open_document(&mut self, path: &str) -> Result<(), String> {
        let data = self.client.read_file(path)?;
        let text = String::from_utf8_lossy(&data).to_string();
        self.text = text;
        self.path = Some(String::from(path));
        self.status = format!("Opened {}", path);
        self.document_dirty = false;
        self.dirty = true;
        Ok(())
    }

    fn save_document(&mut self) -> Result<(), String> {
        if let Some(path) = self.path.clone() {
            self.client.write_file(&path, self.text.as_bytes())?;
            self.status = format!("Saved {}", path);
            self.document_dirty = false;
            self.dirty = true;
            return Ok(());
        }

        let dialog_id = self.client.save_file_dialog("Save File", "/notes.txt")?;
        self.pending_dialogs.push(EditorDialog {
            id: dialog_id,
            kind: EditorDialogKind::Save,
        });
        self.status = String::from("Waiting for save path");
        self.dirty = true;
        Ok(())
    }

    fn poll_platform(&mut self) {
        let mut completed = Vec::new();
        let pending = self.pending_dialogs.clone();
        for dialog in pending.iter() {
            if let Ok(Some(result)) = self.client.poll_dialog_result(dialog.id) {
                match result.selection {
                    DialogSelection::Accepted(path) => match dialog.kind {
                        EditorDialogKind::Open => {
                            let _ = self.open_document(&path);
                        }
                        EditorDialogKind::Save => {
                            self.path = Some(path.clone());
                            let _ = self.save_document();
                        }
                    },
                    DialogSelection::Cancelled => {
                        self.status = String::from("Dialog cancelled");
                    }
                }
                completed.push(dialog.id);
                self.dirty = true;
            }
        }
        self.pending_dialogs
            .retain(|dialog| !completed.iter().any(|done| done == &dialog.id));
    }

    fn render(&mut self) -> Result<(), String> {
        let Some(window) = self.window else {
            return Ok(());
        };
        if !window.visible || !self.dirty {
            return Ok(());
        }

        let mut canvas = Canvas::new(
            window.content_rect.width as usize,
            window.content_rect.height as usize,
            WINDOW_BG,
        );
        canvas.fill_rect(Rect::new(0, 0, window.content_rect.width, 54), PANEL_BG);
        canvas.fill_rect(Rect::new(0, 54, window.content_rect.width, 1), BORDER);
        canvas.draw_text(18, 18, "Text Editor", TEXT_PRIMARY);
        let title = self.path.as_deref().unwrap_or("Scratch buffer");
        canvas.draw_text(18, 34, title, TEXT_MUTED);
        canvas.draw_multiline_text(
            20,
            72,
            window.content_rect.width as i32 - 40,
            &self.text,
            TEXT_PRIMARY,
        );
        let footer_y = max(window.content_rect.height as i32 - 30, 0);
        canvas.fill_rect(
            Rect::new(0, footer_y, window.content_rect.width, 30),
            PANEL_BG,
        );
        canvas.fill_rect(Rect::new(0, footer_y, window.content_rect.width, 1), BORDER);
        let status = if self.document_dirty {
            format!("{} (modified)", self.status)
        } else {
            self.status.clone()
        };
        canvas.draw_text(18, footer_y + 8, &status, TEXT_SECONDARY);

        self.client
            .present(window.window_id, &canvas.into_pixels())?;
        self.dirty = false;
        Ok(())
    }

    fn snapshot(&self) -> AppSnapshot {
        snapshot_for_window(
            AppKind::Editor,
            self.window,
            self.workspace_id,
            if let Some(path) = self.path.as_ref() {
                format!("{}{}", path, if self.document_dirty { " *" } else { "" })
            } else {
                format!("scratch {} chars", self.text.chars().count())
            },
        )
    }

    fn publish_accessibility(&self) {
        let Some(window) = self.window else {
            return;
        };
        let nodes = vec![
            AccessibilityNode {
                id: 1,
                app_id: self.client.app_id(),
                role: AccessibilityRole::Window,
                label: String::from("Editor"),
                description: self.status.clone(),
                focused: window.focused,
                bounds: window.content_rect,
            },
            AccessibilityNode {
                id: 2,
                app_id: self.client.app_id(),
                role: AccessibilityRole::Input,
                label: String::from("Document"),
                description: self.path.clone().unwrap_or_else(|| String::from("scratch")),
                focused: window.focused,
                bounds: Rect::new(
                    0,
                    60,
                    window.content_rect.width,
                    window.content_rect.height.saturating_sub(60),
                ),
            },
        ];
        let _ = self.client.publish_accessibility_tree(nodes);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WindowSync {
    Unchanged,
    Changed,
    Closed,
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
        Some(3) => Some(AppKind::Settings),
        Some(4) => Some(AppKind::Editor),
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

fn dialog_default_path(request: &crate::gui::protocol::DialogRequest) -> String {
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

fn grant_default_permissions(client: &DesktopClient, enabled: bool) {
    let state = if enabled {
        PermissionState::Granted
    } else {
        PermissionState::Denied
    };
    for permission in [
        DesktopPermission::ClipboardRead,
        DesktopPermission::ClipboardWrite,
        DesktopPermission::Notifications,
        DesktopPermission::FileDialogs,
        DesktopPermission::FileSystem,
        DesktopPermission::ScreenCapture,
    ] {
        let _ = client.set_permission(permission, state);
    }
}

fn grant_default_file_access(client: &DesktopClient, prefixes: &[&str]) {
    for prefix in prefixes {
        let _ = client.grant_file_access(prefix, false);
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
    workspace_start: i32,
    command_rect: Rect,
    quick_rect: Rect,
    power_rect: Rect,
    status_start: i32,
}

fn top_bar_layout(width: i32) -> TopBarLayout {
    let width = width.max(320);
    let power_rect = Rect::new(width.saturating_sub(74), 10, 58, 38);
    let quick_rect = Rect::new(power_rect.x.saturating_sub(116), 10, 104, 38);
    let command_rect = Rect::new(width.saturating_div(2).saturating_sub(103), 10, 206, 38);
    let workspace_start = command_rect.x.saturating_sub(292).max(210);
    let status_start = quick_rect.x.saturating_sub(118);

    TopBarLayout {
        workspace_start,
        command_rect,
        quick_rect,
        power_rect,
        status_start,
    }
}

fn top_bar_workspace_rect(index: u8, width: i32) -> Rect {
    let layout = top_bar_layout(width);
    if index >= 4 {
        return Rect::new(-10_000, -10_000, 1, 1);
    }
    let gap = 10;
    let button_width = 56;
    Rect::new(
        layout.workspace_start + index as i32 * (button_width + gap),
        10,
        button_width as u32,
        38,
    )
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
        Rect::new(dock_rect.x + 19, dock_rect.y + 14, dock_rect.width.saturating_sub(38), 52),
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

fn top_bar_power_hit(local: Point, width: i32) -> bool {
    top_bar_layout(width).power_rect.contains(local)
}

fn top_bar_command_hit(local: Point, width: i32) -> bool {
    top_bar_layout(width).command_rect.contains(local)
}

fn top_bar_quick_settings_hit(local: Point, width: i32) -> bool {
    top_bar_layout(width).quick_rect.contains(local)
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

fn push_scene_rect(scene: &mut SceneGraph, parent: SceneNodeId, bounds: Rect, color: u32) {
    push_scene_round_rect(scene, parent, bounds, color, 0);
}

fn push_scene_round_rect(
    scene: &mut SceneGraph,
    parent: SceneNodeId,
    bounds: Rect,
    color: u32,
    corner_radius: u16,
) {
    let _ = scene.push_render_object(
        parent,
        bounds,
        DamageLane::Shell,
        RenderObjectKind::SolidRect {
            color,
            corner_radius,
        },
    );
}

fn push_scene_panel(
    scene: &mut SceneGraph,
    parent: SceneNodeId,
    bounds: Rect,
    fill: u32,
    border: u32,
    corner_radius: u16,
    top_accent: Option<u32>,
) {
    push_scene_round_rect(scene, parent, bounds, fill, corner_radius);
    push_scene_outline(scene, parent, bounds, border);
    if let Some(accent) = top_accent {
        let inset = (corner_radius as i32 / 2).max(0);
        let width = bounds
            .width
            .saturating_sub((inset as u32).saturating_mul(2))
            .max(1);
        push_scene_round_rect(
            scene,
            parent,
            Rect::new(bounds.x + inset, bounds.y, width, 1),
            accent,
            1,
        );
    }
}

fn push_scene_icon(
    scene: &mut SceneGraph,
    parent: SceneNodeId,
    kind: AppKind,
    rect: Rect,
    accent: u32,
) {
    let inset = 10;
    let inner = Rect::new(
        rect.x + inset,
        rect.y + inset,
        rect.width.saturating_sub((inset as u32) * 2),
        rect.height.saturating_sub((inset as u32) * 2),
    );
    match kind {
        AppKind::Terminal => {
            push_scene_round_rect(scene, parent, inner, accent, 8);
            push_scene_rect(
                scene,
                parent,
                Rect::new(inner.x + 4, inner.y + 5, inner.width.saturating_sub(8), 3),
                0xFF0A131E,
            );
            push_scene_rect(scene, parent, Rect::new(inner.x + 6, inner.y + 13, 10, 3), 0xFF0A131E);
            push_scene_rect(
                scene,
                parent,
                Rect::new(inner.x + 18, inner.y + 19, inner.width.saturating_sub(24), 3),
                0xFF0A131E,
            );
        }
        AppKind::Files => {
            let tab = Rect::new(inner.x + 4, inner.y + 5, inner.width / 3, 6);
            let body = Rect::new(
                inner.x + 3,
                inner.y + 10,
                inner.width.saturating_sub(6),
                inner.height.saturating_sub(13),
            );
            push_scene_round_rect(scene, parent, body, accent, 7);
            push_scene_round_rect(scene, parent, tab, accent, 4);
        }
        AppKind::Settings => {
            push_scene_round_rect(
                scene,
                parent,
                Rect::new(inner.x + 4, inner.y + 6, inner.width.saturating_sub(8), 5),
                accent,
                3,
            );
            push_scene_round_rect(
                scene,
                parent,
                Rect::new(inner.x + 4, inner.y + 14, inner.width.saturating_sub(8), 5),
                accent,
                3,
            );
            push_scene_round_rect(
                scene,
                parent,
                Rect::new(inner.x + 4, inner.y + 22, inner.width.saturating_sub(8), 5),
                accent,
                3,
            );
            push_scene_round_rect(scene, parent, Rect::new(inner.x + 9, inner.y + 4, 7, 9), 0xFF0A131E, 3);
            push_scene_round_rect(scene, parent, Rect::new(inner.x + 17, inner.y + 12, 7, 9), 0xFF0A131E, 3);
            push_scene_round_rect(scene, parent, Rect::new(inner.x + 13, inner.y + 20, 7, 9), 0xFF0A131E, 3);
        }
        AppKind::Editor => {
            push_scene_round_rect(scene, parent, inner, accent, 8);
            push_scene_rect(
                scene,
                parent,
                Rect::new(inner.x + 6, inner.y + 6, 3, inner.height.saturating_sub(12)),
                0xFF0A131E,
            );
            push_scene_rect(
                scene,
                parent,
                Rect::new(inner.x + 14, inner.y + 8, inner.width.saturating_sub(20), 3),
                0xFF0A131E,
            );
            push_scene_rect(
                scene,
                parent,
                Rect::new(inner.x + 14, inner.y + 15, inner.width.saturating_sub(24), 3),
                0xFF0A131E,
            );
            push_scene_rect(
                scene,
                parent,
                Rect::new(inner.x + 14, inner.y + 22, inner.width.saturating_sub(16), 3),
                0xFF0A131E,
            );
        }
    }
}

fn push_scene_outline(scene: &mut SceneGraph, parent: SceneNodeId, bounds: Rect, color: u32) {
    if bounds.width == 0 || bounds.height == 0 {
        return;
    }
    push_scene_rect(scene, parent, Rect::new(bounds.x, bounds.y, bounds.width, 1), color);
    push_scene_rect(
        scene,
        parent,
        Rect::new(bounds.x, bounds.bottom().saturating_sub(1), bounds.width, 1),
        color,
    );
    push_scene_rect(scene, parent, Rect::new(bounds.x, bounds.y, 1, bounds.height), color);
    push_scene_rect(
        scene,
        parent,
        Rect::new(bounds.right().saturating_sub(1), bounds.y, 1, bounds.height),
        color,
    );
}

fn push_scene_text(
    scene: &mut SceneGraph,
    text_system: &mut TextSystem,
    parent: SceneNodeId,
    x: i32,
    y: i32,
    max_width: u32,
    text: &str,
    color: u32,
) {
    let blob = text_system.layout_text_with_style(text, max_width.max(1), TextStyle::ui(), color);
    let _ = scene.push_render_object(
        parent,
        Rect::new(x, y, blob.width_px.max(1), blob.height_px.max(1)),
        DamageLane::Text,
        RenderObjectKind::Raster {
            width: blob.width_px.max(1),
            height: blob.height_px.max(1),
            pixels: blob.pixels,
        },
    );
}

fn apply_scene_overlay_transform(
    mut scene: crate::gui::protocol::SceneUpdate,
    opacity: f32,
    slide_y: i32,
) -> crate::gui::protocol::SceneUpdate {
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
) -> crate::gui::protocol::SceneUpdate {
    let palette = hybrid_titan_palette(theme_mode);
    let bounds = Rect::new(0, 0, width as u32, height as u32);
    let mut scene = SceneGraph::new(bounds);
    scene.set_semantic_root(Some(window_id));
    let root = scene.root();
    let layout = top_bar_layout(width as i32);

    push_scene_panel(
        &mut scene,
        root,
        Rect::new(0, 0, width as u32, height as u32),
        0xD60A131E,
        palette.border,
        20,
        Some(0x2E60B8FF),
    );

    push_scene_text(
        &mut scene,
        text_system,
        root,
        18,
        17,
        180,
        "echOS",
        palette.text_primary,
    );

    let workspace_start = ((active_workspace as usize) / 4) * 4;
    let workspace_rects = layout_flex(
        Rect::new(layout.workspace_start, 10, 266, 38),
        FlexDirection::Row,
        EdgeInsets {
            left: 0,
            top: 1,
            right: 0,
            bottom: 1,
        },
        10,
        &[FlexItem::fixed(56), FlexItem::fixed(56), FlexItem::fixed(56), FlexItem::fixed(56)],
    );
    for (index, rect) in workspace_rects.iter().enumerate() {
        let workspace_id = (workspace_start + index).min(WORKSPACE_COUNT as usize - 1) as u8;
        let active = workspace_id == active_workspace;
        push_scene_panel(
            &mut scene,
            root,
            *rect,
            if active { 0xF2142433 } else { 0xDB0D1724 },
            palette.border,
            14,
            if active { Some(palette.accent_mint) } else { None },
        );
        push_scene_text(
            &mut scene,
            text_system,
            root,
            rect.x + ((rect.width as i32 - 10) / 2),
            rect.y + 11,
            16,
            &(workspace_id + 1).to_string(),
            if active {
                palette.text_primary
            } else {
                palette.text_muted
            },
        );
    }

    push_scene_panel(
        &mut scene,
        root,
        layout.command_rect,
        0xC7091018,
        palette.border,
        14,
        None,
    );
    push_scene_text(
        &mut scene,
        text_system,
        root,
        layout.command_rect.x + 15,
        layout.command_rect.y + 10,
        layout.command_rect.width.saturating_sub(30),
        "Search / Command",
        palette.text_muted,
    );

    let right_labels = ["NET", "AUD", "PWR"];
    let mut status_x = layout.status_start;
    for label in right_labels {
        push_scene_text(
            &mut scene,
            text_system,
            root,
            status_x,
            17,
            30,
            label,
            palette.text_secondary,
        );
        status_x += 34;
    }
    push_scene_panel(
        &mut scene,
        root,
        layout.quick_rect,
        0xD00E1723,
        palette.border,
        14,
        Some(palette.accent_blue),
    );
    push_scene_text(
        &mut scene,
        text_system,
        root,
        layout.quick_rect.x + 16,
        layout.quick_rect.y + 10,
        layout.quick_rect.width.saturating_sub(32),
        "Panel",
        palette.text_primary,
    );
    push_scene_panel(
        &mut scene,
        root,
        layout.power_rect,
        0xD00E1723,
        palette.border,
        14,
        Some(palette.accent_gold),
    );
    push_scene_text(
        &mut scene,
        text_system,
        root,
        layout.power_rect.x + 14,
        layout.power_rect.y + 10,
        layout.power_rect.width.saturating_sub(28),
        "Lock",
        palette.text_primary,
    );
    push_scene_text(
        &mut scene,
        text_system,
        root,
        width as i32 - 120,
        17,
        44,
        "12:00",
        palette.text_secondary,
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
) -> crate::gui::protocol::SceneUpdate {
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
        Rect::new(dock_rect.x + 19, dock_rect.y + 14, dock_rect.width.saturating_sub(38), 52),
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
        (None, if snapshots.iter().any(|s| s.needs_attention) { palette.accent_gold } else { palette.text_secondary }),
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
            push_scene_icon(&mut scene, root, kind, *rect, if active { palette.text_primary } else { accent });
        } else {
            let badge = Rect::new(rect.x + 14, rect.y + 12, rect.width.saturating_sub(28), rect.height.saturating_sub(24));
            push_scene_round_rect(&mut scene, root, badge, accent, 12);
            push_scene_round_rect(&mut scene, root, Rect::new(rect.x + rect.width as i32 / 2 - 3, rect.y + 14, 6, 6), 0xFF0A131E, 3);
            push_scene_round_rect(&mut scene, root, Rect::new(rect.x + rect.width as i32 / 2 - 3, rect.bottom() - 20, 6, 6), 0xFF0A131E, 3);
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
) -> crate::gui::protocol::SceneUpdate {
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

    let app_rects = crate::gui::layout::layout_grid(
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
        push_scene_text(
            &mut scene,
            text_system,
            root,
            rect.x + 18,
            rect.y + 18,
            rect.width.saturating_sub(36),
            kind.title(),
            palette.text_primary,
        );
        push_scene_text(
            &mut scene,
            text_system,
            root,
            rect.x + 18,
            rect.y + 42,
            rect.width.saturating_sub(36),
            snapshot
                .map(|s| app_health_label(s.health, s.visible, s.focused))
                .unwrap_or("ready"),
            if active { palette.text_secondary } else { palette.text_muted },
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
        let rect = Rect::new(18, 292 + index as i32 * 54, width.saturating_sub(36) as u32, 44);
        push_scene_panel(
            &mut scene,
            root,
            rect,
            0xE00E1723,
            palette.border,
            14,
            if index == 0 { Some(palette.accent_blue) } else { None },
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
    notifications: bool,
    animations: bool,
) -> crate::gui::protocol::SceneUpdate {
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
    let rows = [
        (
            "Wi-Fi",
            if notifications { "On" } else { "Idle" },
            palette.accent_blue,
        ),
        (
            "Audio",
            if animations { "36%" } else { "Muted" },
            palette.accent_mint,
        ),
        ("Theme", theme_mode_label(theme_mode), palette.accent_gold),
        ("Desktop", "Hybrid", palette.accent_soft),
        ("Notifications", if notifications { "Open" } else { "Muted" }, palette.text_secondary),
    ];
    for (index, (label, value, accent)) in rows.iter().enumerate() {
        let rect = Rect::new(18, 86 + index as i32 * 58, width.saturating_sub(36) as u32, 46);
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
) -> crate::gui::protocol::SceneUpdate {
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
    let cards = crate::gui::layout::layout_grid(
        Rect::new(18, 82, width.saturating_sub(36) as u32, height.saturating_sub(100) as u32),
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
            if selected { Some(palette.accent_mint) } else { None },
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
        let thumb = Rect::new(rect.x + 16, rect.y + 78, rect.width.saturating_sub(32), rect.height.saturating_sub(94));
        push_scene_panel(
            &mut scene,
            root,
            thumb,
            0xD00C1722,
            palette.border,
            16,
            None,
        );
        let left = Rect::new(thumb.x + 14, thumb.y + 16, thumb.width / 2, thumb.height.saturating_sub(32));
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
            let right_bottom = Rect::new(right_top.x, right_top.bottom() + 10, right_top.width, right_top.height);
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
) -> crate::gui::protocol::SceneUpdate {
    let palette = hybrid_titan_palette(theme_mode);
    let bounds = Rect::new(0, 0, width as u32, height as u32);
    let mut scene = SceneGraph::new(bounds);
    scene.set_semantic_root(Some(window_id));
    let root = scene.root();
    push_scene_rect(&mut scene, root, bounds, palette.window_bg);
    push_scene_rect(&mut scene, root, Rect::new(0, 0, width as u32, 36), palette.panel_bg);
    push_scene_rect(&mut scene, root, Rect::new(0, 36, width as u32, 1), palette.border);
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
) -> crate::gui::protocol::SceneUpdate {
    let palette = hybrid_titan_palette(theme_mode);
    let bounds = Rect::new(0, 0, width as u32, height as u32);
    let mut scene = SceneGraph::new(bounds);
    scene.set_semantic_root(Some(window_id));
    let root = scene.root();
    push_scene_rect(&mut scene, root, bounds, palette.window_bg);
    push_scene_rect(&mut scene, root, Rect::new(0, 0, width as u32, 44), palette.panel_bg);
    push_scene_rect(&mut scene, root, Rect::new(0, 44, width as u32, 1), palette.border);
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
            if selected { palette.panel_alt } else { palette.panel_bg },
        );
        push_scene_outline(
            &mut scene,
            root,
            rect,
            if selected { kind.accent() } else { palette.border },
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
    pending_dialog: Option<&crate::gui::protocol::DialogRequest>,
    dialog_input: &str,
    theme_mode: ThemeMode,
) -> crate::gui::protocol::SceneUpdate {
    let palette = hybrid_titan_palette(theme_mode);
    let bounds = Rect::new(0, 0, width as u32, height as u32);
    let mut scene = SceneGraph::new(bounds);
    scene.set_semantic_root(Some(window_id));
    let root = scene.root();
    push_scene_rect(&mut scene, root, bounds, palette.window_bg);
    push_scene_rect(&mut scene, root, Rect::new(0, 0, width as u32, 58), palette.panel_bg);
    push_scene_rect(&mut scene, root, Rect::new(0, 58, width as u32, 1), palette.border);
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
) -> crate::gui::protocol::SceneUpdate {
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
            if index == selected_index { 0xF2142433 } else { 0xE00E1723 },
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
            if notice.read { palette.text_muted } else { accent },
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
) -> crate::gui::protocol::SceneUpdate {
    let palette = hybrid_titan_palette(theme_mode);
    let bounds = Rect::new(0, 0, width as u32, height as u32);
    let mut scene = SceneGraph::new(bounds);
    scene.set_semantic_root(Some(window_id));
    let root = scene.root();
    push_scene_rect(&mut scene, root, bounds, palette.window_bg);
    push_scene_rect(&mut scene, root, Rect::new(0, 0, width as u32, 64), palette.panel_bg);
    push_scene_rect(&mut scene, root, Rect::new(0, 64, width as u32, 1), palette.border);
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
) -> crate::gui::protocol::SceneUpdate {
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
        if query.is_empty() { "Type to filter actions" } else { query },
        if query.is_empty() { palette.text_muted } else { palette.text_primary },
    );
    for (index, action) in actions.iter().take(6).enumerate() {
        let selected = index == selected_index.min(actions.len().saturating_sub(1));
        let rect = Rect::new(18, 138 + index as i32 * 54, width.saturating_sub(36) as u32, 44);
        push_scene_panel(
            &mut scene,
            root,
            rect,
            if selected { 0xF2142433 } else { 0xE00E1723 },
            palette.border,
            14,
            if selected { Some(palette.accent_blue) } else { None },
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
    let profile = Theme::layout_profile(width as u32);
    let layout = top_bar_layout(width as i32);
    let mut canvas = Canvas::new(width, height, palette.panel_bg);
    canvas.fill_rect(
        Rect::new(0, 0, width as u32, height as u32),
        palette.panel_bg,
    );
    canvas.fill_rect(
        Rect::new(0, height as i32 - 1, width as u32, 1),
        palette.border,
    );
    canvas.draw_text(18, 10, "echOS Desktop", palette.text_primary);

    for index in 0..WORKSPACE_COUNT {
        let rect = top_bar_workspace_rect(index, width as i32);
        let active = index == active_workspace;
        canvas.fill_rect(
            rect,
            if active {
                palette.panel_alt
            } else {
                palette.window_bg
            },
        );
        canvas.stroke_rect(
            rect,
            if active {
                palette.accent_blue
            } else {
                palette.border
            },
        );
        canvas.draw_text(
            rect.x + 8,
            rect.y + 7,
            &(index.saturating_add(1)).to_string(),
            if active {
                palette.text_primary
            } else {
                palette.text_secondary
            },
        );
    }

    let status = format!(
        "{} unread  {} live  {} fault  {}",
        snapshot.unread_notifications,
        snapshot.apps_running,
        snapshot.apps_crashed,
        if snapshot.boot_clean_desktop {
            "clean"
        } else {
            "active"
        }
    );
    let status_x = match profile {
        ShellLayoutProfile::Desktop => width as i32 - 278,
        ShellLayoutProfile::Compact => width as i32 - 252,
    };
    canvas.draw_text(status_x, 10, &status, palette.text_secondary);
    if matches!(profile, ShellLayoutProfile::Desktop) && height > 46 {
        canvas.draw_text(
            width as i32 - 278,
            34,
            &format!(
                "{}  {}  Super+Space palette  Super+, settings  Super+` overview",
                power_state_label(snapshot.power_state),
                if snapshot.shell_ready {
                    "shell-ready"
                } else {
                    "shell-syncing"
                }
            ),
            palette.text_muted,
        );
    }
    let command_rect = layout.command_rect;
    canvas.fill_rect(command_rect, palette.window_bg);
    canvas.stroke_rect(command_rect, palette.border);
    canvas.draw_text(
        command_rect.x + 12,
        command_rect.y + 8,
        "Search / Command",
        palette.text_muted,
    );
    let quick_rect = layout.quick_rect;
    canvas.fill_rect(quick_rect, palette.panel_alt);
    canvas.stroke_rect(quick_rect, palette.accent_blue);
    canvas.draw_text(
        quick_rect.x + 8,
        quick_rect.y + 8,
        "Panel",
        palette.text_primary,
    );
    let power_rect = layout.power_rect;
    canvas.fill_rect(power_rect, palette.panel_alt);
    canvas.stroke_rect(power_rect, palette.accent_gold);
    canvas.draw_text(
        power_rect.x + 18,
        power_rect.y + 8,
        "Lock",
        palette.text_primary,
    );
    canvas.into_pixels()
}

fn paint_task_strip_surface(
    width: usize,
    height: usize,
    snapshots: &[AppSnapshot],
    active_workspace: u8,
    theme_mode: ThemeMode,
) -> Vec<u32> {
    let palette = hybrid_titan_palette(theme_mode);
    let mut canvas = Canvas::new(width, height, palette.panel_alt);
    canvas.fill_rect(
        Rect::new(0, 0, width as u32, height as u32),
        palette.panel_alt,
    );
    canvas.fill_rect(Rect::new(0, 0, width as u32, 1), palette.border);
    canvas.draw_text(18, 10, "Spaces", palette.text_muted);

    for index in 0..WORKSPACE_COUNT {
        let rect = task_strip_workspace_rect(index);
        let active = index == active_workspace;
        canvas.fill_rect(
            rect,
            if active {
                palette.panel_bg
            } else {
                palette.window_bg
            },
        );
        canvas.stroke_rect(
            rect,
            if active {
                palette.accent_mint
            } else {
                palette.border
            },
        );
        canvas.draw_text(
            rect.x + 8,
            rect.y + 10,
            &(index.saturating_add(1)).to_string(),
            if active {
                palette.text_primary
            } else {
                palette.text_secondary
            },
        );
    }

    canvas.draw_text(246, 10, "Apps", palette.text_muted);
    for (index, snapshot) in snapshots.iter().enumerate() {
        let rect = Rect::new(246 + index as i32 * 118, 12, 106, 48);
        let on_active_workspace = snapshot.workspace_id == active_workspace;
        let border = if snapshot.focused {
            snapshot.kind.accent()
        } else if snapshot.needs_attention {
            palette.accent_gold
        } else if snapshot.health == AppHealth::Crashed {
            palette.accent_coral
        } else if on_active_workspace && snapshot.visible {
            palette.accent_soft
        } else {
            palette.border
        };
        canvas.fill_rect(
            rect,
            if on_active_workspace {
                palette.panel_bg
            } else {
                palette.window_bg
            },
        );
        canvas.stroke_rect(rect, border);
        canvas.fill_rect(
            Rect::new(rect.x + 10, rect.y + 14, 10, 10),
            snapshot.kind.accent(),
        );
        canvas.draw_text(
            rect.x + 28,
            rect.y + 8,
            snapshot.kind.title(),
            palette.text_primary,
        );
        canvas.draw_text(
            rect.x + 28,
            rect.y + 24,
            app_health_label(snapshot.health, snapshot.visible, snapshot.focused),
            if on_active_workspace {
                palette.text_secondary
            } else {
                palette.text_muted
            },
        );
    }

    canvas.draw_text(
        width as i32 - 208,
        28,
        "Super+1..8 spaces  Super+S scratch",
        palette.text_muted,
    );
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
    pending_dialog: Option<&crate::gui::protocol::DialogRequest>,
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
    animations_enabled: bool,
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
            format!(
                "Animations {}",
                if animations_enabled { "On" } else { "Reduced" }
            ),
            if animations_enabled {
                palette.accent_mint
            } else {
                palette.border
            },
        ),
        (String::from("Lock Session"), palette.accent_gold),
        (String::from("Clear Notifications"), palette.accent_coral),
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

