//! Session shell registry, permissions, and recovery metadata service.

use crate::services::display_atomic::MailboxRing;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

const SHELL_COMMAND_QUEUE_CAPACITY: usize = 256;
const SHELL_RESPONSE_QUEUE_CAPACITY: usize = 256;

use crate::gui::protocol::{
    AccessibilityNode, AccessibilityProfile, AppHealth, AppId, DesktopPermission, DisplayProfile,
    FileGrant, MotionProfile, PermissionEntry, PermissionState, RestoreDisposition,
    SessionPowerState, SessionSnapshot, ShellAppEntry, ShellDensityProfile, StageSet,
    StageSetPolicy, WindowId, WindowRule, WorkspaceId, WorkspaceLayout, WorkspaceRule,
};

#[derive(Clone, Debug)]
pub enum ShellCommand {
    RegisterApp {
        app_id: AppId,
        name: String,
    },
    UnregisterApp {
        app_id: AppId,
    },
    UpdateAppWindow {
        app_id: AppId,
        window_id: Option<WindowId>,
        visible: bool,
        focused: bool,
        workspace_id: WorkspaceId,
    },
    MarkAppLaunch {
        app_id: AppId,
        status_line: String,
    },
    MarkAppExit {
        app_id: AppId,
        clean: bool,
        status_line: String,
    },
    RecordAppFault {
        app_id: AppId,
        detail: String,
    },
    ClearAppAttention {
        app_id: AppId,
        status_line: Option<String>,
    },
    SetAutoRestore {
        app_id: AppId,
        enabled: bool,
    },
    SetPermission {
        app_id: AppId,
        permission: DesktopPermission,
        state: PermissionState,
    },
    GetPermission {
        app_id: AppId,
        permission: DesktopPermission,
    },
    ListPermissions {
        app_id: AppId,
    },
    GrantFileAccess {
        app_id: AppId,
        path_prefix: String,
        read_only: bool,
    },
    CheckFileAccess {
        app_id: AppId,
        path: String,
        write: bool,
    },
    ListFileGrants {
        app_id: AppId,
    },
    SetAccessibilityTree {
        app_id: AppId,
        nodes: Vec<AccessibilityNode>,
    },
    GetAccessibilityTree {
        app_id: AppId,
    },
    SetAccessibilityProfile {
        profile: AccessibilityProfile,
    },
    GetAccessibilityProfile,
    NoteNotification {
        app_id: AppId,
    },
    ClearNotifications {
        app_id: Option<AppId>,
    },
    SetWorkspace {
        workspace_id: WorkspaceId,
    },
    GetWorkspace,
    SetWorkspaceLayout {
        workspace_id: WorkspaceId,
        layout: WorkspaceLayout,
    },
    GetWorkspaceLayout {
        workspace_id: WorkspaceId,
    },
    SetWorkspaceRule {
        workspace_id: WorkspaceId,
        rule: WorkspaceRule,
    },
    GetWorkspaceRule {
        workspace_id: WorkspaceId,
    },
    ToggleScratchpad,
    ToggleOverview,
    SetPowerState {
        power_state: SessionPowerState,
    },
    SetDisplayProfileState {
        profile: DisplayProfile,
    },
    GetDisplayProfileState,
    SetClipboardHistoryLen {
        len: u32,
    },
    SetShellDensity {
        profile: ShellDensityProfile,
    },
    GetShellDensity,
    SetMotionProfile {
        profile: MotionProfile,
    },
    GetMotionProfile,
    SetRestoreDisposition {
        disposition: RestoreDisposition,
    },
    GetRestoreDisposition,
    SetStageSets {
        sets: Vec<StageSet>,
    },
    GetStageSets,
    SetStageSetPolicy {
        policy: StageSetPolicy,
    },
    GetStageSetPolicy,
    SetWindowRules {
        rules: Vec<WindowRule>,
    },
    GetWindowRules,
    GetSessionSnapshot,
    ListApps,
}

#[derive(Clone, Debug)]
pub enum ShellResponse {
    Ack,
    Apps(Vec<ShellAppEntry>),
    Workspace(WorkspaceId),
    WorkspaceLayout(WorkspaceLayout),
    WorkspaceRule(WorkspaceRule),
    ToggleState(bool),
    Permission(PermissionState),
    Permissions(Vec<PermissionEntry>),
    FileAccess(bool),
    FileGrants(Vec<FileGrant>),
    AccessibilityTree(Vec<AccessibilityNode>),
    AccessibilityProfile(AccessibilityProfile),
    DisplayProfile(DisplayProfile),
    ShellDensity(ShellDensityProfile),
    MotionProfile(MotionProfile),
    RestoreDisposition(RestoreDisposition),
    StageSets(Vec<StageSet>),
    StageSetPolicy(StageSetPolicy),
    WindowRules(Vec<WindowRule>),
    SessionSnapshot(SessionSnapshot),
    Error(String),
}

pub struct EchShell {
    running: AtomicBool,
    workspace_id: Mutex<WorkspaceId>,
    power_state: Mutex<SessionPowerState>,
    workspace_layouts: Mutex<BTreeMap<WorkspaceId, WorkspaceLayout>>,
    workspace_rules: Mutex<BTreeMap<WorkspaceId, WorkspaceRule>>,
    overview_active: AtomicBool,
    scratchpad_visible: AtomicBool,
    unread_notifications: Mutex<u32>,
    clipboard_history_len: Mutex<u32>,
    apps: Mutex<BTreeMap<AppId, ShellAppEntry>>,
    permissions: Mutex<BTreeMap<AppId, BTreeMap<DesktopPermission, PermissionState>>>,
    file_grants: Mutex<BTreeMap<AppId, Vec<FileGrant>>>,
    accessibility: Mutex<BTreeMap<AppId, Vec<AccessibilityNode>>>,
    accessibility_profile: Mutex<AccessibilityProfile>,
    display_profile: Mutex<DisplayProfile>,
    shell_density: Mutex<ShellDensityProfile>,
    motion_profile: Mutex<MotionProfile>,
    restore_state: Mutex<RestoreDisposition>,
    stage_sets: Mutex<Vec<StageSet>>,
    stage_set_policy: Mutex<StageSetPolicy>,
    window_rules: Mutex<Vec<WindowRule>>,
    command_queue: MailboxRing<ShellCommand>,
    response_queue: MailboxRing<ShellResponse>,
}

impl EchShell {
    pub fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            workspace_id: Mutex::new(0),
            power_state: Mutex::new(SessionPowerState::Active),
            workspace_layouts: Mutex::new(BTreeMap::new()),
            workspace_rules: Mutex::new(BTreeMap::new()),
            overview_active: AtomicBool::new(false),
            scratchpad_visible: AtomicBool::new(false),
            unread_notifications: Mutex::new(0),
            clipboard_history_len: Mutex::new(0),
            apps: Mutex::new(BTreeMap::new()),
            permissions: Mutex::new(BTreeMap::new()),
            file_grants: Mutex::new(BTreeMap::new()),
            accessibility: Mutex::new(BTreeMap::new()),
            accessibility_profile: Mutex::new(AccessibilityProfile::default()),
            display_profile: Mutex::new(DisplayProfile::default()),
            shell_density: Mutex::new(ShellDensityProfile::Balanced),
            motion_profile: Mutex::new(MotionProfile::Standard),
            restore_state: Mutex::new(RestoreDisposition::RestoreIfClean),
            stage_sets: Mutex::new(Vec::new()),
            stage_set_policy: Mutex::new(StageSetPolicy::default()),
            window_rules: Mutex::new(Vec::new()),
            command_queue: MailboxRing::with_capacity_pow2(SHELL_COMMAND_QUEUE_CAPACITY),
            response_queue: MailboxRing::with_capacity_pow2(SHELL_RESPONSE_QUEUE_CAPACITY),
        }
    }

    pub fn start(&self) {
        self.running.store(true, Ordering::SeqCst);
        crate::serial_println!("[ECHSHELL] service started");
    }

    pub fn send_command(&self, command: ShellCommand) -> bool {
        self.command_queue.try_push(command).is_ok()
    }

    pub fn receive_response(&self) -> Option<ShellResponse> {
        self.response_queue.pop()
    }

    pub fn process_command(&self, command: ShellCommand) -> ShellResponse {
        match command {
            ShellCommand::RegisterApp { app_id, name } => {
                let workspace_id = *self.workspace_id.lock();
                self.apps.lock().insert(
                    app_id,
                    ShellAppEntry {
                        app_id,
                        name,
                        window_id: None,
                        visible: false,
                        focused: false,
                        workspace_id,
                        running: false,
                        health: AppHealth::Idle,
                        launch_count: 0,
                        crash_count: 0,
                        needs_attention: false,
                        status_line: String::from("registered"),
                        auto_restore: false,
                    },
                );
                self.ensure_permission_defaults(app_id);
                ShellResponse::Ack
            }
            ShellCommand::UnregisterApp { app_id } => {
                self.apps.lock().remove(&app_id);
                self.permissions.lock().remove(&app_id);
                self.file_grants.lock().remove(&app_id);
                self.accessibility.lock().remove(&app_id);
                ShellResponse::Ack
            }
            ShellCommand::UpdateAppWindow {
                app_id,
                window_id,
                visible,
                focused,
                workspace_id,
            } => {
                let mut apps = self.apps.lock();
                let Some(entry) = apps.get_mut(&app_id) else {
                    return ShellResponse::Error(String::from("app not registered"));
                };
                entry.window_id = window_id;
                entry.visible = visible;
                entry.focused = focused;
                entry.workspace_id = workspace_id;
                if window_id.is_some() && visible {
                    entry.running = true;
                    if entry.health != AppHealth::Crashed {
                        entry.health = AppHealth::Running;
                    }
                }
                if focused {
                    entry.needs_attention = false;
                    if entry.health == AppHealth::Attention {
                        entry.health = AppHealth::Running;
                    }
                }
                ShellResponse::Ack
            }
            ShellCommand::MarkAppLaunch {
                app_id,
                status_line,
            } => {
                let mut apps = self.apps.lock();
                let Some(entry) = apps.get_mut(&app_id) else {
                    return ShellResponse::Error(String::from("app not registered"));
                };
                entry.running = true;
                entry.health = AppHealth::Running;
                entry.needs_attention = false;
                entry.launch_count = entry.launch_count.saturating_add(1);
                entry.status_line = status_line;
                ShellResponse::Ack
            }
            ShellCommand::MarkAppExit {
                app_id,
                clean,
                status_line,
            } => {
                let mut apps = self.apps.lock();
                let Some(entry) = apps.get_mut(&app_id) else {
                    return ShellResponse::Error(String::from("app not registered"));
                };
                entry.running = false;
                entry.visible = false;
                entry.focused = false;
                entry.window_id = None;
                if clean {
                    entry.health = AppHealth::Idle;
                    entry.needs_attention = false;
                } else {
                    entry.health = AppHealth::Crashed;
                    entry.needs_attention = true;
                    entry.crash_count = entry.crash_count.saturating_add(1);
                }
                entry.status_line = status_line;
                ShellResponse::Ack
            }
            ShellCommand::RecordAppFault { app_id, detail } => {
                let mut apps = self.apps.lock();
                let Some(entry) = apps.get_mut(&app_id) else {
                    return ShellResponse::Error(String::from("app not registered"));
                };
                entry.running = false;
                entry.health = AppHealth::Crashed;
                entry.needs_attention = true;
                entry.visible = false;
                entry.focused = false;
                entry.window_id = None;
                entry.crash_count = entry.crash_count.saturating_add(1);
                entry.status_line = detail;
                ShellResponse::Ack
            }
            ShellCommand::ClearAppAttention {
                app_id,
                status_line,
            } => {
                let mut apps = self.apps.lock();
                let Some(entry) = apps.get_mut(&app_id) else {
                    return ShellResponse::Error(String::from("app not registered"));
                };
                entry.needs_attention = false;
                if entry.running {
                    entry.health = AppHealth::Running;
                } else if entry.health != AppHealth::Crashed {
                    entry.health = AppHealth::Idle;
                }
                if let Some(status_line) = status_line {
                    entry.status_line = status_line;
                }
                ShellResponse::Ack
            }
            ShellCommand::SetAutoRestore { app_id, enabled } => {
                let mut apps = self.apps.lock();
                let Some(entry) = apps.get_mut(&app_id) else {
                    return ShellResponse::Error(String::from("app not registered"));
                };
                entry.auto_restore = enabled;
                ShellResponse::Ack
            }
            ShellCommand::SetPermission {
                app_id,
                permission,
                state,
            } => {
                self.ensure_permission_defaults(app_id);
                self.permissions
                    .lock()
                    .entry(app_id)
                    .or_default()
                    .insert(permission, state);
                ShellResponse::Ack
            }
            ShellCommand::GetPermission { app_id, permission } => {
                self.ensure_permission_defaults(app_id);
                let state = self
                    .permissions
                    .lock()
                    .get(&app_id)
                    .and_then(|entries| entries.get(&permission).copied())
                    .unwrap_or(PermissionState::Ask);
                ShellResponse::Permission(state)
            }
            ShellCommand::ListPermissions { app_id } => {
                self.ensure_permission_defaults(app_id);
                let entries = self
                    .permissions
                    .lock()
                    .get(&app_id)
                    .map(|entries| {
                        entries
                            .iter()
                            .map(|(permission, state)| PermissionEntry {
                                permission: *permission,
                                state: *state,
                            })
                            .collect()
                    })
                    .unwrap_or_else(Vec::new);
                ShellResponse::Permissions(entries)
            }
            ShellCommand::GrantFileAccess {
                app_id,
                path_prefix,
                read_only,
            } => {
                let mut grants = self.file_grants.lock();
                let entries = grants.entry(app_id).or_default();
                if !entries.iter().any(|grant| grant.path_prefix == path_prefix) {
                    entries.push(FileGrant {
                        path_prefix,
                        read_only,
                    });
                }
                ShellResponse::Ack
            }
            ShellCommand::CheckFileAccess {
                app_id,
                path,
                write,
            } => {
                let granted = self
                    .file_grants
                    .lock()
                    .get(&app_id)
                    .map(|grants| {
                        grants.iter().any(|grant| {
                            path.starts_with(&grant.path_prefix) && (!write || !grant.read_only)
                        })
                    })
                    .unwrap_or(false);
                ShellResponse::FileAccess(granted)
            }
            ShellCommand::ListFileGrants { app_id } => {
                let grants = self
                    .file_grants
                    .lock()
                    .get(&app_id)
                    .cloned()
                    .unwrap_or_else(Vec::new);
                ShellResponse::FileGrants(grants)
            }
            ShellCommand::SetAccessibilityTree { app_id, nodes } => {
                self.accessibility.lock().insert(app_id, nodes.clone());
                crate::services::at_spi::get_bridge().publish_tree(app_id, &nodes);
                ShellResponse::Ack
            }
            ShellCommand::GetAccessibilityTree { app_id } => {
                let nodes = self
                    .accessibility
                    .lock()
                    .get(&app_id)
                    .cloned()
                    .unwrap_or_else(Vec::new);
                ShellResponse::AccessibilityTree(nodes)
            }
            ShellCommand::SetAccessibilityProfile { profile } => {
                *self.accessibility_profile.lock() = profile;
                ShellResponse::Ack
            }
            ShellCommand::GetAccessibilityProfile => {
                ShellResponse::AccessibilityProfile(*self.accessibility_profile.lock())
            }
            ShellCommand::NoteNotification { app_id } => {
                let mut unread = self.unread_notifications.lock();
                *unread = unread.saturating_add(1);
                if let Some(entry) = self.apps.lock().get_mut(&app_id) {
                    entry.needs_attention = true;
                    if entry.health != AppHealth::Crashed {
                        entry.health = AppHealth::Attention;
                    }
                }
                ShellResponse::Ack
            }
            ShellCommand::ClearNotifications { app_id } => {
                *self.unread_notifications.lock() = 0;
                if let Some(app_id) = app_id {
                    if let Some(entry) = self.apps.lock().get_mut(&app_id) {
                        entry.needs_attention = false;
                        if entry.health == AppHealth::Attention {
                            entry.health = if entry.running {
                                AppHealth::Running
                            } else {
                                AppHealth::Idle
                            };
                        }
                    }
                } else {
                    for entry in self.apps.lock().values_mut() {
                        entry.needs_attention = false;
                        if entry.health == AppHealth::Attention {
                            entry.health = if entry.running {
                                AppHealth::Running
                            } else {
                                AppHealth::Idle
                            };
                        }
                    }
                }
                ShellResponse::Ack
            }
            ShellCommand::SetWorkspace { workspace_id } => {
                *self.workspace_id.lock() = workspace_id;
                ShellResponse::Ack
            }
            ShellCommand::GetWorkspace => ShellResponse::Workspace(*self.workspace_id.lock()),
            ShellCommand::SetWorkspaceLayout {
                workspace_id,
                layout,
            } => {
                self.workspace_layouts.lock().insert(workspace_id, layout);
                ShellResponse::Ack
            }
            ShellCommand::GetWorkspaceLayout { workspace_id } => {
                let layout = self
                    .workspace_layouts
                    .lock()
                    .get(&workspace_id)
                    .copied()
                    .unwrap_or(WorkspaceLayout::Dwindle);
                ShellResponse::WorkspaceLayout(layout)
            }
            ShellCommand::SetWorkspaceRule { workspace_id, rule } => {
                self.workspace_rules.lock().insert(workspace_id, rule);
                ShellResponse::Ack
            }
            ShellCommand::GetWorkspaceRule { workspace_id } => {
                let rule = self
                    .workspace_rules
                    .lock()
                    .get(&workspace_id)
                    .copied()
                    .unwrap_or_else(WorkspaceRule::default);
                ShellResponse::WorkspaceRule(rule)
            }
            ShellCommand::ToggleScratchpad => {
                let next = !self.scratchpad_visible.load(Ordering::Acquire);
                self.scratchpad_visible.store(next, Ordering::Release);
                ShellResponse::ToggleState(next)
            }
            ShellCommand::ToggleOverview => {
                let next = !self.overview_active.load(Ordering::Acquire);
                self.overview_active.store(next, Ordering::Release);
                ShellResponse::ToggleState(next)
            }
            ShellCommand::SetPowerState { power_state } => {
                *self.power_state.lock() = power_state;
                ShellResponse::Ack
            }
            ShellCommand::SetDisplayProfileState { profile } => {
                *self.display_profile.lock() = profile;
                ShellResponse::Ack
            }
            ShellCommand::GetDisplayProfileState => {
                ShellResponse::DisplayProfile(self.display_profile.lock().clone())
            }
            ShellCommand::SetClipboardHistoryLen { len } => {
                *self.clipboard_history_len.lock() = len;
                ShellResponse::Ack
            }
            ShellCommand::SetShellDensity { profile } => {
                *self.shell_density.lock() = profile;
                ShellResponse::Ack
            }
            ShellCommand::GetShellDensity => {
                ShellResponse::ShellDensity(*self.shell_density.lock())
            }
            ShellCommand::SetMotionProfile { profile } => {
                *self.motion_profile.lock() = profile;
                ShellResponse::Ack
            }
            ShellCommand::GetMotionProfile => {
                ShellResponse::MotionProfile(*self.motion_profile.lock())
            }
            ShellCommand::SetRestoreDisposition { disposition } => {
                *self.restore_state.lock() = disposition;
                ShellResponse::Ack
            }
            ShellCommand::GetRestoreDisposition => {
                ShellResponse::RestoreDisposition(*self.restore_state.lock())
            }
            ShellCommand::SetStageSets { sets } => {
                *self.stage_sets.lock() = sets;
                ShellResponse::Ack
            }
            ShellCommand::GetStageSets => ShellResponse::StageSets(self.stage_sets.lock().clone()),
            ShellCommand::SetStageSetPolicy { policy } => {
                *self.stage_set_policy.lock() = policy;
                ShellResponse::Ack
            }
            ShellCommand::GetStageSetPolicy => {
                ShellResponse::StageSetPolicy(*self.stage_set_policy.lock())
            }
            ShellCommand::SetWindowRules { rules } => {
                *self.window_rules.lock() = rules;
                ShellResponse::Ack
            }
            ShellCommand::GetWindowRules => {
                ShellResponse::WindowRules(self.window_rules.lock().clone())
            }
            ShellCommand::GetSessionSnapshot => {
                let apps = self.apps.lock();
                let apps_running = apps.values().filter(|entry| entry.running).count() as u32;
                let apps_crashed = apps
                    .values()
                    .filter(|entry| entry.health == AppHealth::Crashed)
                    .count() as u32;
                let workspace_id = *self.workspace_id.lock();
                let workspace_layout = self
                    .workspace_layouts
                    .lock()
                    .get(&workspace_id)
                    .copied()
                    .unwrap_or(WorkspaceLayout::Dwindle);
                let power_state = *self.power_state.lock();
                let shell_state = if power_state == SessionPowerState::Locked {
                    crate::gui::protocol::ShellState::Locked
                } else if self.overview_active.load(Ordering::Acquire) {
                    crate::gui::protocol::ShellState::OverlayInteractive
                } else {
                    crate::gui::protocol::ShellState::DesktopReady
                };
                let accessibility_profile = *self.accessibility_profile.lock();
                let display_profile = self.display_profile.lock().clone();
                let output_scale = display_profile
                    .outputs
                    .iter()
                    .find(|output| output.output_id == display_profile.primary_output)
                    .map(|output| output.scale_100x as u32)
                    .unwrap_or(100);
                ShellResponse::SessionSnapshot(SessionSnapshot {
                    workspace_id,
                    workspace_layout,
                    power_state,
                    unread_notifications: *self.unread_notifications.lock(),
                    apps_running,
                    apps_crashed,
                    overview_active: self.overview_active.load(Ordering::Acquire),
                    scratchpad_visible: self.scratchpad_visible.load(Ordering::Acquire),
                    shell_ready: self.running.load(Ordering::Acquire),
                    boot_clean_desktop: apps_running == 0,
                    output_scale,
                    text_scale: accessibility_profile.text_scale_100x as u32,
                    clipboard_history_len: *self.clipboard_history_len.lock(),
                    accessibility_profile,
                    display_profile,
                    shell_density: *self.shell_density.lock(),
                    motion_profile: *self.motion_profile.lock(),
                    restore_state: *self.restore_state.lock(),
                    stage_set_policy: *self.stage_set_policy.lock(),
                    locale: String::from("en-US"),
                    theme_variant: String::from("hybrid-titan"),
                    shell_state,
                })
            }
            ShellCommand::ListApps => {
                let apps = self.apps.lock().values().cloned().collect();
                ShellResponse::Apps(apps)
            }
        }
    }

    fn ensure_permission_defaults(&self, app_id: AppId) {
        let mut permissions = self.permissions.lock();
        let entries = permissions.entry(app_id).or_default();
        for permission in [
            DesktopPermission::ClipboardRead,
            DesktopPermission::ClipboardWrite,
            DesktopPermission::Notifications,
            DesktopPermission::FileDialogs,
            DesktopPermission::FileSystem,
            DesktopPermission::ScreenCapture,
        ] {
            entries.entry(permission).or_insert(PermissionState::Ask);
        }
    }

    pub fn run_service(&self) {
        while self.running.load(Ordering::SeqCst) {
            while let Some(command) = self.command_queue.pop() {
                let response = self.process_command(command);
                let _ = self.response_queue.push_overwrite(response);
            }

            for _ in 0..1000 {
                core::hint::spin_loop();
            }
        }
    }
}

lazy_static::lazy_static! {
    static ref ECH_SHELL: Arc<EchShell> = Arc::new(EchShell::new());
}

pub fn init() {
    ECH_SHELL.start();
    crate::serial_println!("[ECHSHELL] initialized");
}

pub fn get_shell_service() -> Arc<EchShell> {
    Arc::clone(&ECH_SHELL)
}

pub fn service_task() -> ! {
    let svc = get_shell_service();
    svc.run_service();
    loop {
        core::hint::spin_loop();
    }
}
