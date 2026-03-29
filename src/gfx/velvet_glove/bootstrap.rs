use super::*;
use crate::runtime_layer::{display_client_contract, input_client_contract, shell_client_contract};

const DESKTOP_BOOTSTRAP_SERVICE_RETRY_TICKS: usize = 32;

pub(super) struct DesktopBootstrapClients {
    pub shell_client: DesktopClient,
    pub terminal_client: DesktopClient,
    pub files_client: DesktopClient,
    pub browser_client: DesktopClient,
    pub settings_client: DesktopClient,
    pub editor_client: DesktopClient,
}

pub(super) struct DesktopShellWindows {
    pub desktop_window: ClientWindow,
    pub top_bar_window: ClientWindow,
    pub task_strip_window: ClientWindow,
    pub launcher_window: ClientWindow,
    pub notifications_window: ClientWindow,
    pub quick_settings_window: ClientWindow,
    pub command_palette_window: ClientWindow,
    pub stage_rail_window: ClientWindow,
    pub dialog_window: ClientWindow,
    pub context_menu_window: ClientWindow,
    pub switcher_window: ClientWindow,
    pub lock_window: ClientWindow,
}

pub(super) fn bootstrap_retry<T, F>(label: &str, mut op: F) -> Result<T, String>
where
    F: FnMut() -> Result<T, String>,
{
    let mut last_err = None;
    for attempt in 0..DESKTOP_BOOTSTRAP_SERVICE_RETRY_TICKS {
        match op() {
            Ok(value) => return Ok(value),
            Err(err) if err.contains("service unavailable") => {
                last_err = Some(err);
                if attempt + 1 < DESKTOP_BOOTSTRAP_SERVICE_RETRY_TICKS {
                    // Desktop bootstrap runs before the steady-state compositor loop starts
                    // issuing per-frame presents. Yield cooperatively here so service tasks
                    // can register their endpoints without tripping the idle-sleep guard.
                    crate::preempt::preemptible_schedule();
                    continue;
                }
            }
            Err(err) => return Err(format!("{}: {}", label, err)),
        }
    }

    Err(format!(
        "{}: {}",
        label,
        last_err.unwrap_or_else(|| String::from("service unavailable"))
    ))
}

pub(super) fn connect_clients() -> Result<DesktopBootstrapClients, String> {
    crate::serial_println!("[DESKTOP] session bootstrap step=connect");
    let shell_client = bootstrap_retry("shell client connect", || {
        DesktopClient::connect(SHELL_APP_ID)
    })?;
    let terminal_client = bootstrap_retry("terminal client connect", || {
        DesktopClient::connect(TERMINAL_APP_ID)
    })?;
    let files_client = bootstrap_retry("files client connect", || {
        DesktopClient::connect(FILES_APP_ID)
    })?;
    let browser_client = bootstrap_retry("browser client connect", || {
        DesktopClient::connect(BROWSER_APP_ID)
    })?;
    let settings_client = bootstrap_retry("settings client connect", || {
        DesktopClient::connect(SETTINGS_APP_ID)
    })?;
    let editor_client = bootstrap_retry("editor client connect", || {
        DesktopClient::connect(EDITOR_APP_ID)
    })?;
    let _ = bootstrap_retry("shortcut sink registration", || {
        input_client_contract::register_shortcut_sink(SHELL_APP_ID)
    });

    Ok(DesktopBootstrapClients {
        shell_client,
        terminal_client,
        files_client,
        browser_client,
        settings_client,
        editor_client,
    })
}

pub(super) fn desktop_surface_rect(screen: Rect) -> Rect {
    crate::gui::shell::desktop_work_area(screen)
}

pub(super) fn task_strip_window_rect(screen: Rect) -> Rect {
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

pub(super) fn create_shell_windows(
    screen: Rect,
    shell_client: &DesktopClient,
) -> Result<DesktopShellWindows, String> {
    crate::serial_println!("[DESKTOP] session bootstrap step=shell-windows");
    let desktop_rect = desktop_surface_rect(screen);
    let desktop_window = shell_client.create_layer_window(
        "Desktop Shortcuts",
        desktop_rect.x,
        desktop_rect.y,
        desktop_rect.width,
        desktop_rect.height,
        0,
        LayerRole::Background,
        super::shell_layer_flags(),
    )?;
    let top_bar_window = shell_client.create_layer_window(
        "Top Bar",
        screen.x + 18,
        screen.y + 18,
        screen.width.saturating_sub(36),
        Theme::HALO_BAR_HEIGHT as u32,
        0,
        LayerRole::TopBar,
        super::shell_layer_flags(),
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
        super::shell_layer_flags(),
    )?;
    let launcher_window = shell_client.create_layer_window(
        "Session Shell",
        screen.x + 32,
        screen.y + 108,
        min(392, screen.width.saturating_sub(96)),
        min(332, screen.height.saturating_sub(172)),
        0,
        LayerRole::Overlay,
        super::shell_layer_flags(),
    )?;
    let notifications_window = shell_client.create_layer_window(
        "Notifications",
        screen.right() - 364,
        screen.y + 108,
        320,
        220,
        0,
        LayerRole::Overlay,
        super::shell_layer_flags(),
    )?;
    let quick_settings_height = min(384, screen.height.saturating_sub(236)).max(280);
    let quick_settings_y = min(
        screen.y + 348,
        screen.bottom() - quick_settings_height as i32 - 32,
    );
    let quick_settings_window = shell_client.create_layer_window(
        "Quick Settings",
        screen.right() - 364,
        quick_settings_y,
        320,
        quick_settings_height,
        0,
        LayerRole::Overlay,
        super::shell_layer_flags(),
    )?;
    let command_palette_window = shell_client.create_layer_window(
        "Command Palette",
        screen.x + (screen.width as i32 / 2) - 310,
        screen.y + 136,
        620,
        312,
        0,
        LayerRole::Overlay,
        super::shell_layer_flags(),
    )?;
    let stage_rail_window = shell_client.create_layer_window(
        "Workspace Overview",
        screen.x + 18,
        screen.y + 108,
        236,
        264,
        0,
        LayerRole::Overlay,
        super::shell_layer_flags(),
    )?;
    let dialog_window = shell_client.create_layer_window(
        "Dialog Broker",
        screen.x + (screen.width as i32 / 2) - 180,
        screen.y + (screen.height as i32 / 2) - 120,
        360,
        190,
        0,
        LayerRole::Modal,
        super::shell_layer_flags(),
    )?;
    let context_menu_window = shell_client.create_layer_window(
        "Context Menu",
        screen.right() - 310,
        screen.bottom() - 290,
        240,
        224,
        0,
        LayerRole::Overlay,
        super::shell_layer_flags(),
    )?;
    let switcher_window = shell_client.create_layer_window(
        "App Switcher",
        screen.x + (screen.width as i32 / 2) - 220,
        screen.y + (screen.height as i32 / 2) - 120,
        440,
        240,
        0,
        LayerRole::Overlay,
        super::shell_layer_flags(),
    )?;
    let lock_window = shell_client.create_layer_window(
        "Login",
        screen.x + (screen.width as i32 / 2) - 240,
        screen.y + (screen.height as i32 / 2) - 160,
        480,
        320,
        0,
        LayerRole::Modal,
        super::shell_layer_flags(),
    )?;

    let _ = shell_client.set_visibility(dialog_window.window_id, false);
    let _ = shell_client.set_visibility(context_menu_window.window_id, false);
    let _ = shell_client.set_visibility(switcher_window.window_id, false);
    let _ = shell_client.set_visibility(notifications_window.window_id, false);
    let _ = shell_client.set_visibility(quick_settings_window.window_id, false);
    let _ = shell_client.set_visibility(command_palette_window.window_id, false);

    Ok(DesktopShellWindows {
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
    })
}

pub(super) fn register_bootstrap_apps(clients: &DesktopBootstrapClients) {
    crate::serial_println!("[DESKTOP] session bootstrap step=register-apps");
    let _ = shell_client_contract::register_shell_app(TERMINAL_APP_ID, "Terminal");
    let _ = shell_client_contract::register_shell_app(FILES_APP_ID, "Files");
    let _ = shell_client_contract::register_shell_app(BROWSER_APP_ID, "Web");
    let _ = shell_client_contract::register_shell_app(SETTINGS_APP_ID, "Settings");
    let _ = shell_client_contract::register_shell_app(EDITOR_APP_ID, "Editor");
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
        let _ = shell_client_contract::set_permission(client.app_id(), permission, state);
    }
}

fn grant_default_file_access(client: &DesktopClient, prefixes: &[&str]) {
    for prefix in prefixes {
        let _ = shell_client_contract::grant_file_access(client.app_id(), prefix, false);
    }
}

pub(super) fn configure_shell_environment(clients: &DesktopBootstrapClients) {
    crate::serial_println!("[DESKTOP] session bootstrap step=permissions");
    grant_default_permissions(&clients.shell_client, true);
    grant_default_permissions(&clients.terminal_client, true);
    grant_default_permissions(&clients.files_client, true);
    grant_default_permissions(&clients.browser_client, true);
    grant_default_permissions(&clients.editor_client, true);
    grant_default_permissions(&clients.settings_client, true);
    grant_default_file_access(&clients.terminal_client, &["/workspace", "/"]);
    grant_default_file_access(&clients.files_client, &["/", "/workspace", "/system"]);
    grant_default_file_access(&clients.browser_client, &["/", "/workspace", "/downloads"]);
    grant_default_file_access(&clients.editor_client, &["/workspace"]);
    grant_default_file_access(&clients.settings_client, &["/system", "/workspace"]);
    grant_default_file_access(&clients.shell_client, &["/", "/workspace", "/system"]);
    let _ = shell_client_contract::set_power_state(SessionPowerState::Active);
    let _ = display_client_contract::set_theme_mode(Theme::default_mode());
    for workspace_id in 0..WORKSPACE_COUNT {
        let rule = super::default_workspace_rule(workspace_id);
        let _ = shell_client_contract::set_workspace_rule(workspace_id, rule);
        let _ = shell_client_contract::set_workspace_layout(workspace_id, rule.layout);
        let _ = virtual_desktops().lock().set_profile(
            workspace_id,
            crate::personalization::DesktopProfile {
                wallpaper_id: workspace_id as u32,
                icon_pack: rule.default_name_str(),
            },
        );
    }
    let _ = shell_client_contract::set_workspace_rule(
        SCRATCHPAD_WORKSPACE,
        super::default_workspace_rule(SCRATCHPAD_WORKSPACE),
    );
}
