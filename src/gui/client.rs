//! Minimal native desktop client API for echOS Week-2.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

use crate::gui::protocol::{
    AccessibilityNode, AppHealth, AppId, ClipboardPayload, DamagePacket, DesktopPermission,
    DialogId, DialogKind, DialogRequest, DialogResult, DialogSelection, FileGrant, LayerRole,
    NotificationEntry, NotificationLevel, PermissionEntry, PermissionState, Rect, RenderObject,
    SceneUpdate, ScreenshotEntry, SessionPowerState, SessionSnapshot, SharedSurfaceDescriptor,
    ShellAppEntry, ShellShortcut, SurfaceId, WindowFlags, WindowId, WindowInfo, WindowInputEvent,
    WorkspaceId, WorkspaceLayout, WorkspaceRule,
};
use crate::gui::theme::ThemeMode;
use crate::services::display_atomic::HotPathMetrics;
use crate::services::FileEntry;
use crate::services::{
    ech_capture::get_capture_service, ech_clipboard::get_clipboard_service,
    ech_dialogs::get_dialogs_service, ech_display::get_display, ech_input::get_input,
    ech_notifications::get_notifications_service, ech_shell::get_shell_service, get_store,
    CaptureCommand, CaptureResponse, ClipboardCommand, ClipboardResponse, DialogCommand,
    DialogResponse, DisplayCommand, DisplayResponse, InputCommand, InputResponse,
    NotificationCommand, NotificationResponse, ShellCommand, ShellResponse, StoreCommand,
    StoreResponse,
};

#[derive(Clone, Copy, Debug)]
pub struct ClientWindow {
    pub window_id: WindowId,
    pub surface_id: SurfaceId,
    pub content_rect: Rect,
}

pub struct DesktopClient {
    app_id: AppId,
    mapped_surfaces: Mutex<BTreeMap<WindowId, SharedSurfaceDescriptor>>,
}

impl DesktopClient {
    pub fn connect(app_id: AppId) -> Result<Self, String> {
        let input = get_input();
        match input.process_command(InputCommand::RegisterApp { app_id }) {
            InputResponse::FocusChanged { .. } | InputResponse::Ack => Ok(Self {
                app_id,
                mapped_surfaces: Mutex::new(BTreeMap::new()),
            }),
            InputResponse::Error(err) => Err(err),
            _ => Ok(Self {
                app_id,
                mapped_surfaces: Mutex::new(BTreeMap::new()),
            }),
        }
    }

    pub fn app_id(&self) -> AppId {
        self.app_id
    }

    pub fn create_window(
        &self,
        title: &str,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<ClientWindow, String> {
        match self
            .display()?
            .process_command(DisplayCommand::CreateWindow {
                app_id: self.app_id,
                title: String::from(title),
                x,
                y,
                width,
                height,
            }) {
            DisplayResponse::WindowCreated {
                window_id,
                surface_id,
                content_rect,
            } => Ok(ClientWindow {
                window_id,
                surface_id,
                content_rect,
            }),
            DisplayResponse::Error(err) => Err(err),
            _ => Err(String::from("display returned unexpected response")),
        }
    }

    pub fn create_layer_window(
        &self,
        title: &str,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        workspace_id: WorkspaceId,
        layer_role: LayerRole,
        flags: WindowFlags,
    ) -> Result<ClientWindow, String> {
        match self
            .display()?
            .process_command(DisplayCommand::CreateWindowWithMeta {
                app_id: self.app_id,
                title: String::from(title),
                x,
                y,
                width,
                height,
                workspace_id,
                layer_role,
                flags,
            }) {
            DisplayResponse::WindowCreated {
                window_id,
                surface_id,
                content_rect,
            } => Ok(ClientWindow {
                window_id,
                surface_id,
                content_rect,
            }),
            DisplayResponse::Error(err) => Err(err),
            _ => Err(String::from("display returned unexpected response")),
        }
    }

    pub fn destroy_window(&self, window_id: WindowId) -> Result<(), String> {
        self.expect_ack(
            self.display()?
                .process_command(DisplayCommand::DestroyWindow { window_id }),
        )
    }

    pub fn present(&self, window_id: WindowId, pixels: &[u32]) -> Result<(), String> {
        if self.present_zero_copy(window_id, pixels).is_ok() {
            return Ok(());
        }

        self.expect_ack(
            self.display()?
                .process_command(DisplayCommand::CommitWindowBuffer {
                    window_id,
                    pixels: pixels.to_vec(),
                }),
        )
    }

    pub fn commit_scene(&self, window_id: WindowId, mut scene: SceneUpdate) -> Result<(), String> {
        scene.canonicalize();
        self.expect_ack(
            self.display()?
                .process_command(DisplayCommand::CommitScene { window_id, scene }),
        )
    }

    pub fn commit_render_objects(
        &self,
        window_id: WindowId,
        render_objects: Vec<RenderObject>,
    ) -> Result<(), String> {
        let mut scene = SceneUpdate {
            root_id: window_id,
            revision: 1,
            render_objects,
            damage_hint: Vec::new(),
            semantic_root: None,
        };
        scene.canonicalize();
        self.commit_scene(window_id, scene)
    }

    pub fn map_surface(&self, window_id: WindowId) -> Result<SharedSurfaceDescriptor, String> {
        let descriptor = match self
            .display()?
            .process_command(DisplayCommand::MapWindowSurface { window_id })
        {
            DisplayResponse::SurfaceMapped(descriptor) => descriptor,
            DisplayResponse::Error(err) => return Err(err),
            _ => return Err(String::from("display returned unexpected response")),
        };
        self.mapped_surfaces.lock().insert(window_id, descriptor);
        Ok(descriptor)
    }

    pub fn submit_damage(
        &self,
        window_id: WindowId,
        surface_id: SurfaceId,
        generation: u64,
        rect: Rect,
    ) -> Result<(), String> {
        self.expect_ack(
            self.display()?
                .process_command(DisplayCommand::SubmitWindowDamage {
                    window_id,
                    packet: DamagePacket {
                        surface_id,
                        generation,
                        rect,
                    },
                }),
        )
    }

    pub fn move_window(&self, window_id: WindowId, x: i32, y: i32) -> Result<(), String> {
        self.expect_ack(self.display()?.process_command(DisplayCommand::MoveWindow {
            window_id,
            x,
            y,
        }))
    }

    pub fn resize_window(
        &self,
        window_id: WindowId,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        self.expect_ack(
            self.display()?
                .process_command(DisplayCommand::ResizeWindow {
                    window_id,
                    width,
                    height,
                }),
        )
    }

    pub fn focus_window(&self, window_id: WindowId) -> Result<(), String> {
        self.expect_ack(
            self.display()?
                .process_command(DisplayCommand::FocusWindow { window_id }),
        )?;

        match get_input().process_command(InputCommand::RequestFocus {
            app_id: self.app_id,
        }) {
            InputResponse::FocusChanged { .. } | InputResponse::Ack => Ok(()),
            InputResponse::Error(err) => Err(err),
            _ => Ok(()),
        }
    }

    pub fn set_title(&self, window_id: WindowId, title: &str) -> Result<(), String> {
        self.expect_ack(
            self.display()?
                .process_command(DisplayCommand::SetWindowTitle {
                    window_id,
                    title: String::from(title),
                }),
        )
    }

    pub fn set_visibility(&self, window_id: WindowId, visible: bool) -> Result<(), String> {
        self.expect_ack(
            self.display()?
                .process_command(DisplayCommand::SetWindowVisibility { window_id, visible }),
        )
    }

    pub fn set_window_meta(
        &self,
        window_id: WindowId,
        workspace_id: WorkspaceId,
        layer_role: LayerRole,
        flags: WindowFlags,
    ) -> Result<(), String> {
        self.expect_ack(
            self.display()?
                .process_command(DisplayCommand::SetWindowMeta {
                    window_id,
                    workspace_id,
                    layer_role,
                    flags,
                }),
        )
    }

    pub fn move_window_to_workspace(
        &self,
        window_id: WindowId,
        workspace_id: WorkspaceId,
    ) -> Result<(), String> {
        self.expect_ack(
            self.display()?
                .process_command(DisplayCommand::MoveWindowToWorkspace {
                    window_id,
                    workspace_id,
                }),
        )
    }

    pub fn list_windows(&self) -> Result<Vec<WindowInfo>, String> {
        match self.display()?.process_command(DisplayCommand::ListWindows) {
            DisplayResponse::WindowList { windows } => Ok(windows),
            DisplayResponse::Error(err) => Err(err),
            _ => Err(String::from("display returned unexpected response")),
        }
    }

    pub fn window_info(&self, window_id: WindowId) -> Result<WindowInfo, String> {
        self.list_windows()?
            .into_iter()
            .find(|window| window.id == window_id)
            .ok_or_else(|| String::from("window not found"))
    }

    pub fn present_frame(&self) -> Result<(), String> {
        self.expect_ack(self.display()?.process_command(DisplayCommand::Present))
    }

    pub fn set_present_mode(
        &self,
        mode: crate::gui::protocol::DisplayPresentMode,
    ) -> Result<(), String> {
        self.expect_ack(
            self.display()?
                .process_command(DisplayCommand::SetPresentMode { mode }),
        )
    }

    pub fn present_metrics(&self) -> Result<HotPathMetrics, String> {
        match self
            .display()?
            .process_command(DisplayCommand::QueryPresentMetrics)
        {
            DisplayResponse::PresentMetrics { metrics } => Ok(metrics),
            DisplayResponse::Error(err) => Err(err),
            _ => Err(String::from("display returned unexpected response")),
        }
    }

    pub fn set_theme_mode(&self, mode: ThemeMode) -> Result<(), String> {
        self.expect_ack(
            self.display()?
                .process_command(DisplayCommand::SetThemeMode { mode }),
        )
    }

    pub fn submit_frame_intent(
        &self,
        intent: crate::gui::protocol::FrameIntent,
    ) -> Result<(), String> {
        self.expect_ack(
            self.display()?
                .process_command(DisplayCommand::SubmitFrameIntent { intent }),
        )
    }

    pub fn clipboard_set(&self, payload: ClipboardPayload) -> Result<(), String> {
        match get_clipboard_service().process_command(ClipboardCommand::Set {
            app_id: self.app_id,
            payload,
        }) {
            ClipboardResponse::Ack => Ok(()),
            ClipboardResponse::Error(err) => Err(err),
            _ => Err(String::from("clipboard returned unexpected response")),
        }
    }

    pub fn clipboard_get(&self) -> Result<ClipboardPayload, String> {
        match get_clipboard_service().process_command(ClipboardCommand::GetCurrent {
            app_id: self.app_id,
        }) {
            ClipboardResponse::Current(payload) => Ok(payload),
            ClipboardResponse::Error(err) => Err(err),
            _ => Err(String::from("clipboard returned unexpected response")),
        }
    }

    pub fn notify(
        &self,
        title: &str,
        message: &str,
        level: NotificationLevel,
    ) -> Result<u64, String> {
        match get_notifications_service().process_command(NotificationCommand::Push(
            crate::gui::protocol::NotificationRequest {
                app_id: self.app_id,
                title: String::from(title),
                message: String::from(message),
                level,
                action_label: Some(String::from("Open")),
            },
        )) {
            NotificationResponse::NotificationId(id) => Ok(id),
            NotificationResponse::Error(err) => Err(err),
            _ => Err(String::from("notifications returned unexpected response")),
        }
    }

    pub fn list_notifications(&self, max_items: usize) -> Result<Vec<NotificationEntry>, String> {
        match get_notifications_service().process_command(NotificationCommand::List {
            app_id: self.app_id,
            include_read: true,
            max_items,
        }) {
            NotificationResponse::Notifications(entries) => Ok(entries),
            NotificationResponse::Error(err) => Err(err),
            _ => Err(String::from("notifications returned unexpected response")),
        }
    }

    pub fn clear_notifications(&self) -> Result<(), String> {
        match get_notifications_service().process_command(NotificationCommand::Clear {
            app_id: self.app_id,
        }) {
            NotificationResponse::Ack => Ok(()),
            NotificationResponse::Error(err) => Err(err),
            _ => Err(String::from("notifications returned unexpected response")),
        }
    }

    pub fn mark_notification_read(&self, id: u64) -> Result<(), String> {
        match get_notifications_service().process_command(NotificationCommand::MarkRead {
            app_id: self.app_id,
            id,
        }) {
            NotificationResponse::Ack => Ok(()),
            NotificationResponse::Error(err) => Err(err),
            _ => Err(String::from("notifications returned unexpected response")),
        }
    }

    pub fn request_dialog(
        &self,
        kind: DialogKind,
        title: &str,
        message: &str,
        path_hint: &str,
    ) -> Result<DialogId, String> {
        match get_dialogs_service().process_command(DialogCommand::Request {
            app_id: self.app_id,
            kind,
            title: String::from(title),
            message: String::from(message),
            path_hint: String::from(path_hint),
        }) {
            DialogResponse::Requested(id) => Ok(id),
            DialogResponse::Error(err) => Err(err),
            _ => Err(String::from("dialogs returned unexpected response")),
        }
    }

    pub fn open_file_dialog(&self, title: &str, path_hint: &str) -> Result<DialogId, String> {
        self.request_dialog(DialogKind::OpenFile, title, "", path_hint)
    }

    pub fn save_file_dialog(&self, title: &str, path_hint: &str) -> Result<DialogId, String> {
        self.request_dialog(DialogKind::SaveFile, title, "", path_hint)
    }

    pub fn pick_folder_dialog(&self, title: &str, path_hint: &str) -> Result<DialogId, String> {
        self.request_dialog(DialogKind::PickFolder, title, "", path_hint)
    }

    pub fn poll_dialog_result(&self, dialog_id: DialogId) -> Result<Option<DialogResult>, String> {
        match get_dialogs_service().process_command(DialogCommand::PollResult {
            app_id: self.app_id,
            dialog_id,
        }) {
            DialogResponse::Result(result) => Ok(result),
            DialogResponse::Error(err) => Err(err),
            _ => Err(String::from("dialogs returned unexpected response")),
        }
    }

    pub fn list_pending_dialogs(&self, max_items: usize) -> Result<Vec<DialogRequest>, String> {
        match get_dialogs_service().process_command(DialogCommand::ListPending { max_items }) {
            DialogResponse::Pending(requests) => Ok(requests),
            DialogResponse::Error(err) => Err(err),
            _ => Err(String::from("dialogs returned unexpected response")),
        }
    }

    pub fn resolve_dialog(
        &self,
        dialog_id: DialogId,
        selection: DialogSelection,
    ) -> Result<(), String> {
        match get_dialogs_service().process_command(DialogCommand::Resolve {
            dialog_id,
            selection,
        }) {
            DialogResponse::Ack => Ok(()),
            DialogResponse::Error(err) => Err(err),
            _ => Err(String::from("dialogs returned unexpected response")),
        }
    }

    pub fn register_shell_app(&self, name: &str) -> Result<(), String> {
        match get_shell_service().process_command(ShellCommand::RegisterApp {
            app_id: self.app_id,
            name: String::from(name),
        }) {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn mark_app_launched(&self, status_line: &str) -> Result<(), String> {
        match get_shell_service().process_command(ShellCommand::MarkAppLaunch {
            app_id: self.app_id,
            status_line: String::from(status_line),
        }) {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn mark_app_exited(&self, clean: bool, status_line: &str) -> Result<(), String> {
        match get_shell_service().process_command(ShellCommand::MarkAppExit {
            app_id: self.app_id,
            clean,
            status_line: String::from(status_line),
        }) {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn record_app_fault(&self, detail: &str) -> Result<(), String> {
        match get_shell_service().process_command(ShellCommand::RecordAppFault {
            app_id: self.app_id,
            detail: String::from(detail),
        }) {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn clear_app_attention(&self, status_line: Option<&str>) -> Result<(), String> {
        match get_shell_service().process_command(ShellCommand::ClearAppAttention {
            app_id: self.app_id,
            status_line: status_line.map(String::from),
        }) {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn set_permission(
        &self,
        permission: DesktopPermission,
        state: PermissionState,
    ) -> Result<(), String> {
        match get_shell_service().process_command(ShellCommand::SetPermission {
            app_id: self.app_id,
            permission,
            state,
        }) {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn permission_state(
        &self,
        permission: DesktopPermission,
    ) -> Result<PermissionState, String> {
        match get_shell_service().process_command(ShellCommand::GetPermission {
            app_id: self.app_id,
            permission,
        }) {
            ShellResponse::Permission(state) => Ok(state),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn list_permissions(&self) -> Result<Vec<PermissionEntry>, String> {
        match get_shell_service().process_command(ShellCommand::ListPermissions {
            app_id: self.app_id,
        }) {
            ShellResponse::Permissions(entries) => Ok(entries),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn grant_file_access(&self, path_prefix: &str, read_only: bool) -> Result<(), String> {
        match get_shell_service().process_command(ShellCommand::GrantFileAccess {
            app_id: self.app_id,
            path_prefix: String::from(path_prefix),
            read_only,
        }) {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn list_file_grants(&self) -> Result<Vec<FileGrant>, String> {
        match get_shell_service().process_command(ShellCommand::ListFileGrants {
            app_id: self.app_id,
        }) {
            ShellResponse::FileGrants(grants) => Ok(grants),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn set_auto_restore(&self, enabled: bool) -> Result<(), String> {
        match get_shell_service().process_command(ShellCommand::SetAutoRestore {
            app_id: self.app_id,
            enabled,
        }) {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn publish_accessibility_tree(&self, nodes: Vec<AccessibilityNode>) -> Result<(), String> {
        match get_shell_service().process_command(ShellCommand::SetAccessibilityTree {
            app_id: self.app_id,
            nodes,
        }) {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn accessibility_tree(&self) -> Result<Vec<AccessibilityNode>, String> {
        match get_shell_service().process_command(ShellCommand::GetAccessibilityTree {
            app_id: self.app_id,
        }) {
            ShellResponse::AccessibilityTree(nodes) => Ok(nodes),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn update_shell_window(
        &self,
        window_id: Option<WindowId>,
        visible: bool,
        focused: bool,
        workspace_id: WorkspaceId,
    ) -> Result<(), String> {
        match get_shell_service().process_command(ShellCommand::UpdateAppWindow {
            app_id: self.app_id,
            window_id,
            visible,
            focused,
            workspace_id,
        }) {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn list_shell_apps(&self) -> Result<Vec<ShellAppEntry>, String> {
        match get_shell_service().process_command(ShellCommand::ListApps) {
            ShellResponse::Apps(apps) => Ok(apps),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn set_workspace(&self, workspace_id: WorkspaceId) -> Result<(), String> {
        match get_shell_service().process_command(ShellCommand::SetWorkspace { workspace_id }) {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn workspace(&self) -> Result<WorkspaceId, String> {
        match get_shell_service().process_command(ShellCommand::GetWorkspace) {
            ShellResponse::Workspace(workspace_id) => Ok(workspace_id),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn set_workspace_layout(
        &self,
        workspace_id: WorkspaceId,
        layout: WorkspaceLayout,
    ) -> Result<(), String> {
        match get_shell_service().process_command(ShellCommand::SetWorkspaceLayout {
            workspace_id,
            layout,
        }) {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn workspace_layout(&self, workspace_id: WorkspaceId) -> Result<WorkspaceLayout, String> {
        match get_shell_service().process_command(ShellCommand::GetWorkspaceLayout { workspace_id })
        {
            ShellResponse::WorkspaceLayout(layout) => Ok(layout),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn set_workspace_rule(
        &self,
        workspace_id: WorkspaceId,
        rule: WorkspaceRule,
    ) -> Result<(), String> {
        match get_shell_service()
            .process_command(ShellCommand::SetWorkspaceRule { workspace_id, rule })
        {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn workspace_rule(&self, workspace_id: WorkspaceId) -> Result<WorkspaceRule, String> {
        match get_shell_service().process_command(ShellCommand::GetWorkspaceRule { workspace_id }) {
            ShellResponse::WorkspaceRule(rule) => Ok(rule),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn toggle_scratchpad(&self) -> Result<bool, String> {
        match get_shell_service().process_command(ShellCommand::ToggleScratchpad) {
            ShellResponse::ToggleState(state) => Ok(state),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn toggle_overview(&self) -> Result<bool, String> {
        match get_shell_service().process_command(ShellCommand::ToggleOverview) {
            ShellResponse::ToggleState(state) => Ok(state),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn set_power_state(&self, power_state: SessionPowerState) -> Result<(), String> {
        match get_shell_service().process_command(ShellCommand::SetPowerState { power_state }) {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn session_snapshot(&self) -> Result<SessionSnapshot, String> {
        match get_shell_service().process_command(ShellCommand::GetSessionSnapshot) {
            ShellResponse::SessionSnapshot(snapshot) => Ok(snapshot),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn capture_screen(&self, label: &str) -> Result<ScreenshotEntry, String> {
        match get_capture_service().process_command(CaptureCommand::CaptureDesktop {
            app_id: self.app_id,
            label: String::from(label),
        }) {
            CaptureResponse::Captured(entry) => Ok(entry),
            CaptureResponse::Error(err) => Err(err),
            _ => Err(String::from("capture returned unexpected response")),
        }
    }

    pub fn list_captures(&self, max_items: usize) -> Result<Vec<ScreenshotEntry>, String> {
        match get_capture_service().process_command(CaptureCommand::ListCaptures {
            app_id: self.app_id,
            max_items,
        }) {
            CaptureResponse::Captures(entries) => Ok(entries),
            CaptureResponse::Error(err) => Err(err),
            _ => Err(String::from("capture returned unexpected response")),
        }
    }

    pub fn save_capture_ppm(&self, capture_id: u64, path: &str) -> Result<(), String> {
        self.ensure_permission(DesktopPermission::ScreenCapture)?;
        self.ensure_file_access(path, true)?;
        match get_capture_service().process_command(CaptureCommand::GetCapture {
            app_id: self.app_id,
            capture_id,
        }) {
            CaptureResponse::CaptureData { entry, pixels } => {
                let mut output =
                    format!("P6\n{} {}\n255\n", entry.width, entry.height).into_bytes();
                for pixel in pixels {
                    output.push(((pixel >> 16) & 0xFF) as u8);
                    output.push(((pixel >> 8) & 0xFF) as u8);
                    output.push((pixel & 0xFF) as u8);
                }
                self.write_file(path, &output)
            }
            CaptureResponse::Error(err) => Err(err),
            _ => Err(String::from("capture returned unexpected response")),
        }
    }

    pub fn register_shortcut_sink(&self) -> Result<(), String> {
        match get_input().process_command(InputCommand::RegisterShortcutSink {
            app_id: self.app_id,
        }) {
            InputResponse::Ack => Ok(()),
            InputResponse::Error(err) => Err(err),
            _ => Err(String::from("input returned unexpected response")),
        }
    }

    pub fn poll_shortcuts(&self, max_events: usize) -> Result<Vec<ShellShortcut>, String> {
        match get_input().process_command(InputCommand::PollShortcuts {
            app_id: self.app_id,
            max_events,
        }) {
            InputResponse::Shortcuts(shortcuts) => Ok(shortcuts),
            InputResponse::Error(err) => Err(err),
            _ => Err(String::from("input returned unexpected response")),
        }
    }

    pub fn poll_input(&self, max_events: usize) -> Result<Vec<WindowInputEvent>, String> {
        match get_input().process_command(InputCommand::PollEvents {
            app_id: self.app_id,
            max_events,
        }) {
            InputResponse::Events { events, .. } => Ok(events),
            InputResponse::Error(err) => Err(err),
            _ => Err(String::from("input returned unexpected response")),
        }
    }

    pub fn read_file(&self, path: &str) -> Result<Vec<u8>, String> {
        self.ensure_permission(DesktopPermission::FileSystem)?;
        self.ensure_file_access(path, false)?;
        match get_store().process_command(StoreCommand::ReadFile {
            path: String::from(path),
        }) {
            StoreResponse::FileData(data) => Ok(data),
            StoreResponse::Error(err) => Err(err),
            _ => Err(String::from("store returned unexpected response")),
        }
    }

    pub fn write_file(&self, path: &str, data: &[u8]) -> Result<(), String> {
        self.ensure_permission(DesktopPermission::FileSystem)?;
        self.ensure_file_access(path, true)?;
        match get_store().process_command(StoreCommand::WriteFile {
            path: String::from(path),
            data: data.to_vec(),
        }) {
            StoreResponse::Success => Ok(()),
            StoreResponse::Error(err) => Err(err),
            _ => Err(String::from("store returned unexpected response")),
        }
    }

    pub fn delete_file(&self, path: &str) -> Result<(), String> {
        self.ensure_permission(DesktopPermission::FileSystem)?;
        self.ensure_file_access(path, true)?;
        match get_store().process_command(StoreCommand::DeleteFile {
            path: String::from(path),
        }) {
            StoreResponse::Success => Ok(()),
            StoreResponse::Error(err) => Err(err),
            _ => Err(String::from("store returned unexpected response")),
        }
    }

    pub fn create_directory(&self, path: &str) -> Result<(), String> {
        self.ensure_permission(DesktopPermission::FileSystem)?;
        self.ensure_file_access(path, true)?;
        match get_store().process_command(StoreCommand::CreateDirectory {
            path: String::from(path),
        }) {
            StoreResponse::Success => Ok(()),
            StoreResponse::Error(err) => Err(err),
            _ => Err(String::from("store returned unexpected response")),
        }
    }

    pub fn list_directory(&self, path: &str) -> Result<Vec<FileEntry>, String> {
        self.ensure_permission(DesktopPermission::FileSystem)?;
        self.ensure_file_access(path, false)?;
        match get_store().process_command(StoreCommand::ListDirectory {
            path: String::from(path),
        }) {
            StoreResponse::DirectoryContents(entries) => Ok(entries),
            StoreResponse::Error(err) => Err(err),
            _ => Err(String::from("store returned unexpected response")),
        }
    }

    fn display(
        &self,
    ) -> Result<alloc::sync::Arc<crate::services::ech_display::EchDisplay>, String> {
        get_display()
            .lock()
            .clone()
            .ok_or_else(|| String::from("display service unavailable"))
    }

    fn expect_ack(&self, response: DisplayResponse) -> Result<(), String> {
        match response {
            DisplayResponse::Ack | DisplayResponse::Presented { .. } => Ok(()),
            DisplayResponse::Error(err) => Err(err),
            _ => Err(String::from("display returned unexpected response")),
        }
    }

    fn present_zero_copy(&self, window_id: WindowId, pixels: &[u32]) -> Result<(), String> {
        let descriptor = self.cached_surface_descriptor(window_id)?;
        let display = self.display()?;
        let (_, shared_surface) = display
            .shared_surface_for_window(window_id)
            .ok_or_else(|| String::from("shared surface unavailable"))?;

        Self::write_shared_surface(&shared_surface, pixels)?;
        let generation = shared_surface.generation();
        self.submit_damage(
            window_id,
            descriptor.surface_id,
            generation,
            Rect::new(0, 0, descriptor.width, descriptor.height),
        )
    }

    fn cached_surface_descriptor(
        &self,
        window_id: WindowId,
    ) -> Result<SharedSurfaceDescriptor, String> {
        if let Some(descriptor) = self.mapped_surfaces.lock().get(&window_id).copied() {
            return Ok(descriptor);
        }
        self.map_surface(window_id)
    }

    fn write_shared_surface(
        surface: &Arc<crate::gui::surface_memory::SharedSurfaceMemory>,
        pixels: &[u32],
    ) -> Result<(), String> {
        surface
            .write_full(pixels)
            .map_err(|_| String::from("shared surface write size mismatch"))
    }

    fn ensure_permission(&self, permission: DesktopPermission) -> Result<(), String> {
        match self.permission_state(permission)? {
            PermissionState::Granted => Ok(()),
            PermissionState::Ask => Err(String::from("permission not granted")),
            PermissionState::Denied => Err(String::from("permission denied")),
        }
    }

    fn ensure_file_access(&self, path: &str, write: bool) -> Result<(), String> {
        match get_shell_service().process_command(ShellCommand::CheckFileAccess {
            app_id: self.app_id,
            path: String::from(path),
            write,
        }) {
            ShellResponse::FileAccess(true) => Ok(()),
            ShellResponse::FileAccess(false) => Err(String::from("file access not granted")),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }
}
