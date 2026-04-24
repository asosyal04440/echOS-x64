//! Minimal native desktop client API for echOS Week-2.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

use super::super::runtime_layer::capture_client_contract::{CaptureCommand, CaptureResponse};
use super::super::runtime_layer::clipboard_client_contract::{ClipboardCommand, ClipboardResponse};
use super::super::runtime_layer::dialog_client_contract::{DialogCommand, DialogResponse};
use super::super::runtime_layer::display_client_contract::{DisplayCommand, DisplayResponse};
use super::super::runtime_layer::input_client_contract::{InputCommand, InputResponse};
use super::super::runtime_layer::notification_client_contract::{
    NotificationCommand, NotificationResponse,
};
use super::super::runtime_layer::shell_client_contract::{ShellCommand, ShellResponse};
use super::super::runtime_layer::store_client_contract::{StoreCommand, StoreResponse};
use super::super::runtime_layer::{
    capture_client_contract, clipboard_client_contract, dialog_client_contract,
    display_client_contract, input_client_contract, notification_client_contract,
    shell_client_contract, store_client_contract, window_session_contract,
};
use super::super::services::display_atomic::HotPathMetrics;
use super::super::services::FileEntry;
use super::protocol::{
    AccessibilityEvent, AccessibilityFocusState, AccessibilityNode, AccessibilityProfile,
    AppHealth, AppId, CaptionEvent, ClipboardPayload, DamagePacket, DesktopPermission, DialogId,
    DialogKind, DialogRequest, DialogResult, DialogSelection, DisplayCapability,
    DisplayPresentMode, DisplayProfile, FileGrant, FrameIntent, LayerRole, MotionProfile,
    NotificationEntry, NotificationLevel, NotificationRequest, OutputMode, PermissionEntry,
    PermissionState, Rect, RenderObject, RestoreDisposition, SceneUpdate, ScreenshotEntry,
    SessionPowerState, SessionSnapshot, SharedSurfaceDescriptor, ShellAppEntry,
    ShellDensityProfile, ShellShortcut, SpeechState, StageSet, StageSetPolicy, SurfaceId,
    WindowFlags, WindowId, WindowInfo, WindowInputEvent, WindowRule, WorkspaceId, WorkspaceLayout,
    WorkspaceRule,
};
use super::surface_memory::{resolve_data_plane_surface, SharedSurfaceMemory};
use super::theme::ThemeMode;

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
        match input_client_contract::request_input_sync(
            app_id,
            input_client_contract::InputCommand::RegisterApp { app_id },
        ) {
            Some(input_client_contract::InputResponse::FocusChanged { .. })
            | Some(input_client_contract::InputResponse::Ack) => Ok(Self {
                app_id,
                mapped_surfaces: Mutex::new(BTreeMap::new()),
            }),
            Some(input_client_contract::InputResponse::Error(err)) => Err(err),
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
        match display_client_contract::request_display_sync(
            self.app_id,
            display_client_contract::DisplayCommand::CreateWindow {
                app_id: self.app_id,
                title: String::from(title),
                x,
                y,
                width,
                height,
            },
        ) {
            Some(display_client_contract::DisplayResponse::WindowCreated {
                window_id,
                surface_id,
                content_rect,
            }) => {
                let _ = window_session_contract::attach_window_session(
                    self.app_id,
                    0,
                    false,
                    window_id,
                    surface_id,
                );
                Ok(ClientWindow {
                    window_id,
                    surface_id,
                    content_rect,
                })
            }
            Some(display_client_contract::DisplayResponse::Error(err)) => Err(err),
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
        match display_client_contract::request_display_sync(
            self.app_id,
            display_client_contract::DisplayCommand::CreateWindowWithMeta {
                app_id: self.app_id,
                title: String::from(title),
                x,
                y,
                width,
                height,
                workspace_id,
                layer_role,
                flags,
            },
        ) {
            Some(display_client_contract::DisplayResponse::WindowCreated {
                window_id,
                surface_id,
                content_rect,
            }) => {
                let _ = window_session_contract::attach_window_session(
                    self.app_id,
                    workspace_id,
                    false,
                    window_id,
                    surface_id,
                );
                Ok(ClientWindow {
                    window_id,
                    surface_id,
                    content_rect,
                })
            }
            Some(display_client_contract::DisplayResponse::Error(err)) => Err(err),
            _ => Err(String::from("display returned unexpected response")),
        }
    }

    pub fn destroy_window(&self, window_id: WindowId) -> Result<(), String> {
        let result =
            self.expect_ack(self.display_response(DisplayCommand::DestroyWindow { window_id })?);
        if result.is_ok() {
            window_session_contract::forget_window_session(window_id);
        }
        result
    }

    pub fn present(&self, window_id: WindowId, pixels: &[u32]) -> Result<(), String> {
        if self.present_zero_copy(window_id, pixels).is_ok() {
            return Ok(());
        }

        self.expect_ack(self.display_response(DisplayCommand::CommitWindowBuffer {
            window_id,
            pixels: pixels.to_vec(),
        })?)
    }

    pub fn commit_scene(&self, window_id: WindowId, mut scene: SceneUpdate) -> Result<(), String> {
        scene.canonicalize();
        self.expect_ack(self.display_response(DisplayCommand::CommitScene { window_id, scene })?)
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
        let descriptor =
            match self.display_response(DisplayCommand::MapWindowSurface { window_id })? {
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
        self.expect_ack(self.display_response(DisplayCommand::SubmitWindowDamage {
            window_id,
            packet: DamagePacket {
                surface_id,
                generation,
                rect,
            },
        })?)
    }

    pub fn move_window(&self, window_id: WindowId, x: i32, y: i32) -> Result<(), String> {
        self.expect_ack(self.display_response(DisplayCommand::MoveWindow { window_id, x, y })?)
    }

    pub fn resize_window(
        &self,
        window_id: WindowId,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        self.expect_ack(self.display_response(DisplayCommand::ResizeWindow {
            window_id,
            width,
            height,
        })?)
    }

    pub fn focus_window(&self, window_id: WindowId) -> Result<(), String> {
        self.expect_ack(self.display_response(DisplayCommand::FocusWindow { window_id })?)?;

        match self.input_response(InputCommand::RequestFocus {
            app_id: self.app_id,
        })? {
            InputResponse::FocusChanged { .. } | InputResponse::Ack => Ok(()),
            InputResponse::Error(err) => Err(err),
            _ => Ok(()),
        }
    }

    pub fn set_title(&self, window_id: WindowId, title: &str) -> Result<(), String> {
        self.expect_ack(self.display_response(DisplayCommand::SetWindowTitle {
            window_id,
            title: String::from(title),
        })?)
    }

    pub fn set_visibility(&self, window_id: WindowId, visible: bool) -> Result<(), String> {
        self.expect_ack(
            self.display_response(DisplayCommand::SetWindowVisibility { window_id, visible })?,
        )
    }

    pub fn set_window_meta(
        &self,
        window_id: WindowId,
        workspace_id: WorkspaceId,
        layer_role: LayerRole,
        flags: WindowFlags,
    ) -> Result<(), String> {
        self.expect_ack(self.display_response(DisplayCommand::SetWindowMeta {
            window_id,
            workspace_id,
            layer_role,
            flags,
        })?)
    }

    pub fn move_window_to_workspace(
        &self,
        window_id: WindowId,
        workspace_id: WorkspaceId,
    ) -> Result<(), String> {
        self.expect_ack(
            self.display_response(DisplayCommand::MoveWindowToWorkspace {
                window_id,
                workspace_id,
            })?,
        )
    }

    pub fn list_windows(&self) -> Result<Vec<WindowInfo>, String> {
        match self.display_response(DisplayCommand::ListWindows)? {
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
        self.expect_ack(self.display_response(DisplayCommand::Present)?)
    }

    pub fn set_present_mode(&self, mode: DisplayPresentMode) -> Result<(), String> {
        self.expect_ack(self.display_response(DisplayCommand::SetPresentMode { mode })?)
    }

    pub fn query_output_mode(&self) -> Result<(OutputMode, OutputMode, OutputMode), String> {
        match self.display_response(DisplayCommand::QueryOutputMode)? {
            DisplayResponse::OutputModeState {
                current,
                requested,
                effective,
            } => Ok((current, requested, effective)),
            DisplayResponse::Error(err) => Err(err),
            _ => Err(String::from("display returned unexpected response")),
        }
    }

    pub fn list_output_modes(&self) -> Result<Vec<OutputMode>, String> {
        match self.display_response(DisplayCommand::ListOutputModes)? {
            DisplayResponse::OutputModeCatalog { modes } => Ok(modes),
            DisplayResponse::Error(err) => Err(err),
            _ => Err(String::from("display returned unexpected response")),
        }
    }

    pub fn display_capability(&self) -> Result<DisplayCapability, String> {
        match self.display_response(DisplayCommand::QueryDisplayCapability)? {
            DisplayResponse::DisplayCapability(capability) => Ok(capability),
            DisplayResponse::Error(err) => Err(err),
            _ => Err(String::from("display returned unexpected response")),
        }
    }

    pub fn display_profile(&self) -> Result<DisplayProfile, String> {
        match self.display_response(DisplayCommand::QueryDisplayProfile)? {
            DisplayResponse::DisplayProfile(profile) => Ok(profile),
            DisplayResponse::Error(err) => Err(err),
            _ => Err(String::from("display returned unexpected response")),
        }
    }

    pub fn set_display_profile(&self, profile: DisplayProfile) -> Result<DisplayProfile, String> {
        match self.display_response(DisplayCommand::SetDisplayProfile { profile })? {
            DisplayResponse::DisplayProfile(profile) => Ok(profile),
            DisplayResponse::Error(err) => Err(err),
            _ => Err(String::from("display returned unexpected response")),
        }
    }

    pub fn set_output_mode(
        &self,
        mode: OutputMode,
    ) -> Result<(OutputMode, OutputMode, OutputMode), String> {
        match self.display_response(DisplayCommand::SetOutputMode { mode })? {
            DisplayResponse::OutputModeState {
                current,
                requested,
                effective,
            } => Ok((current, requested, effective)),
            DisplayResponse::Error(err) => Err(err),
            _ => Err(String::from("display returned unexpected response")),
        }
    }

    pub fn present_metrics(&self) -> Result<HotPathMetrics, String> {
        match self.display_response(DisplayCommand::QueryPresentMetrics)? {
            DisplayResponse::PresentMetrics { metrics } => Ok(metrics),
            DisplayResponse::Error(err) => Err(err),
            _ => Err(String::from("display returned unexpected response")),
        }
    }

    pub fn set_theme_mode(&self, mode: ThemeMode) -> Result<(), String> {
        self.expect_ack(self.display_response(DisplayCommand::SetThemeMode { mode })?)
    }

    pub fn submit_frame_intent(&self, intent: FrameIntent) -> Result<(), String> {
        self.expect_ack(self.display_response(DisplayCommand::SubmitFrameIntent { intent })?)
    }

    pub fn clipboard_set(&self, payload: ClipboardPayload) -> Result<(), String> {
        match self.clipboard_response(ClipboardCommand::Set {
            app_id: self.app_id,
            payload,
        })? {
            ClipboardResponse::Ack => Ok(()),
            ClipboardResponse::Error(err) => Err(err),
            _ => Err(String::from("clipboard returned unexpected response")),
        }
    }

    pub fn clipboard_get(&self) -> Result<ClipboardPayload, String> {
        match self.clipboard_response(ClipboardCommand::GetCurrent {
            app_id: self.app_id,
        })? {
            ClipboardResponse::Current(payload) => Ok(payload),
            ClipboardResponse::Error(err) => Err(err),
            _ => Err(String::from("clipboard returned unexpected response")),
        }
    }

    pub fn clipboard_history(&self, max_items: usize) -> Result<Vec<ClipboardPayload>, String> {
        match self.clipboard_response(ClipboardCommand::GetHistory {
            app_id: self.app_id,
            max_items,
        })? {
            ClipboardResponse::History(entries) => Ok(entries),
            ClipboardResponse::Error(err) => Err(err),
            _ => Err(String::from("clipboard returned unexpected response")),
        }
    }

    pub fn clipboard_clear(&self) -> Result<(), String> {
        match self.clipboard_response(ClipboardCommand::Clear {
            app_id: self.app_id,
        })? {
            ClipboardResponse::Ack => Ok(()),
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
        match self.notification_response(NotificationCommand::Push(NotificationRequest {
            app_id: self.app_id,
            title: String::from(title),
            message: String::from(message),
            level,
            action_label: Some(String::from("Open")),
        }))? {
            NotificationResponse::NotificationId(id) => Ok(id),
            NotificationResponse::Error(err) => Err(err),
            _ => Err(String::from("notifications returned unexpected response")),
        }
    }

    pub fn list_notifications(&self, max_items: usize) -> Result<Vec<NotificationEntry>, String> {
        match self.notification_response(NotificationCommand::List {
            app_id: self.app_id,
            include_read: true,
            max_items,
        })? {
            NotificationResponse::Notifications(entries) => Ok(entries),
            NotificationResponse::Error(err) => Err(err),
            _ => Err(String::from("notifications returned unexpected response")),
        }
    }

    pub fn clear_notifications(&self) -> Result<(), String> {
        match self.notification_response(NotificationCommand::Clear {
            app_id: self.app_id,
        })? {
            NotificationResponse::Ack => Ok(()),
            NotificationResponse::Error(err) => Err(err),
            _ => Err(String::from("notifications returned unexpected response")),
        }
    }

    pub fn mark_notification_read(&self, id: u64) -> Result<(), String> {
        match self.notification_response(NotificationCommand::MarkRead {
            app_id: self.app_id,
            id,
        })? {
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
        match self.dialog_response(DialogCommand::Request {
            app_id: self.app_id,
            kind,
            title: String::from(title),
            message: String::from(message),
            path_hint: String::from(path_hint),
        })? {
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
        match self.dialog_response(DialogCommand::PollResult {
            app_id: self.app_id,
            dialog_id,
        })? {
            DialogResponse::Result(result) => Ok(result),
            DialogResponse::Error(err) => Err(err),
            _ => Err(String::from("dialogs returned unexpected response")),
        }
    }

    pub fn list_pending_dialogs(&self, max_items: usize) -> Result<Vec<DialogRequest>, String> {
        match self.dialog_response(DialogCommand::ListPending { max_items })? {
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
        match self.dialog_response(DialogCommand::Resolve {
            dialog_id,
            selection,
        })? {
            DialogResponse::Ack => Ok(()),
            DialogResponse::Error(err) => Err(err),
            _ => Err(String::from("dialogs returned unexpected response")),
        }
    }

    pub fn register_shell_app(&self, name: &str) -> Result<(), String> {
        match self.shell_response(ShellCommand::RegisterApp {
            app_id: self.app_id,
            name: String::from(name),
        })? {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn mark_app_launched(&self, status_line: &str) -> Result<(), String> {
        match self.shell_response(ShellCommand::MarkAppLaunch {
            app_id: self.app_id,
            status_line: String::from(status_line),
        })? {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn mark_app_exited(&self, clean: bool, status_line: &str) -> Result<(), String> {
        match self.shell_response(ShellCommand::MarkAppExit {
            app_id: self.app_id,
            clean,
            status_line: String::from(status_line),
        })? {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn record_app_fault(&self, detail: &str) -> Result<(), String> {
        match self.shell_response(ShellCommand::RecordAppFault {
            app_id: self.app_id,
            detail: String::from(detail),
        })? {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn clear_app_attention(&self, status_line: Option<&str>) -> Result<(), String> {
        match self.shell_response(ShellCommand::ClearAppAttention {
            app_id: self.app_id,
            status_line: status_line.map(String::from),
        })? {
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
        match self.shell_response(ShellCommand::SetPermission {
            app_id: self.app_id,
            permission,
            state,
        })? {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn permission_state(
        &self,
        permission: DesktopPermission,
    ) -> Result<PermissionState, String> {
        match self.shell_response(ShellCommand::GetPermission {
            app_id: self.app_id,
            permission,
        })? {
            ShellResponse::Permission(state) => Ok(state),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn list_permissions(&self) -> Result<Vec<PermissionEntry>, String> {
        match self.shell_response(ShellCommand::ListPermissions {
            app_id: self.app_id,
        })? {
            ShellResponse::Permissions(entries) => Ok(entries),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn grant_file_access(&self, path_prefix: &str, read_only: bool) -> Result<(), String> {
        match self.shell_response(ShellCommand::GrantFileAccess {
            app_id: self.app_id,
            path_prefix: String::from(path_prefix),
            read_only,
        })? {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn list_file_grants(&self) -> Result<Vec<FileGrant>, String> {
        match self.shell_response(ShellCommand::ListFileGrants {
            app_id: self.app_id,
        })? {
            ShellResponse::FileGrants(grants) => Ok(grants),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn set_auto_restore(&self, enabled: bool) -> Result<(), String> {
        match self.shell_response(ShellCommand::SetAutoRestore {
            app_id: self.app_id,
            enabled,
        })? {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn publish_accessibility_tree(&self, nodes: Vec<AccessibilityNode>) -> Result<(), String> {
        match self.shell_response(ShellCommand::SetAccessibilityTree {
            app_id: self.app_id,
            nodes,
        })? {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn accessibility_tree(&self) -> Result<Vec<AccessibilityNode>, String> {
        match self.shell_response(ShellCommand::GetAccessibilityTree {
            app_id: self.app_id,
        })? {
            ShellResponse::AccessibilityTree(nodes) => Ok(nodes),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn set_accessibility_profile(&self, profile: AccessibilityProfile) -> Result<(), String> {
        match self.shell_response(ShellCommand::SetAccessibilityProfile { profile })? {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn accessibility_profile(&self) -> Result<AccessibilityProfile, String> {
        match self.shell_response(ShellCommand::GetAccessibilityProfile)? {
            ShellResponse::AccessibilityProfile(profile) => Ok(profile),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn record_accessibility_event(&self, event: AccessibilityEvent) -> Result<(), String> {
        match self.shell_response(ShellCommand::RecordAccessibilityEvent { event })? {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn accessibility_events(
        &self,
        max_items: usize,
    ) -> Result<Vec<AccessibilityEvent>, String> {
        match self.shell_response(ShellCommand::ListAccessibilityEvents { max_items })? {
            ShellResponse::AccessibilityEvents(events) => Ok(events),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn clear_accessibility_events(&self) -> Result<(), String> {
        match self.shell_response(ShellCommand::ClearAccessibilityEvents)? {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn push_caption_event(&self, event: CaptionEvent) -> Result<(), String> {
        match self.shell_response(ShellCommand::PushCaptionEvent { event })? {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn caption_events(&self, max_items: usize) -> Result<Vec<CaptionEvent>, String> {
        match self.shell_response(ShellCommand::ListCaptionEvents { max_items })? {
            ShellResponse::CaptionEvents(events) => Ok(events),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn clear_caption_events(&self) -> Result<(), String> {
        match self.shell_response(ShellCommand::ClearCaptionEvents)? {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn speech_state(&self, max_items: usize) -> Result<SpeechState, String> {
        match self.shell_response(ShellCommand::GetSpeechState { max_items })? {
            ShellResponse::SpeechState(state) => Ok(state),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn advance_speech_lane(&self) -> Result<SpeechState, String> {
        match self.shell_response(ShellCommand::AdvanceSpeechLane)? {
            ShellResponse::SpeechState(state) => Ok(state),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn tick_speech_lane(&self, now_ns: u64) -> Result<SpeechState, String> {
        match self.shell_response(ShellCommand::TickSpeechLane { now_ns })? {
            ShellResponse::SpeechState(state) => Ok(state),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn clear_speech_lane(&self) -> Result<(), String> {
        match self.shell_response(ShellCommand::ClearSpeechLane)? {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn accessibility_focus(&self) -> Result<Option<AccessibilityFocusState>, String> {
        match self.shell_response(ShellCommand::GetAccessibilityFocus)? {
            ShellResponse::AccessibilityFocus(focus) => Ok(focus),
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
        match self.shell_response(ShellCommand::UpdateAppWindow {
            app_id: self.app_id,
            window_id,
            visible,
            focused,
            workspace_id,
        })? {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn list_shell_apps(&self) -> Result<Vec<ShellAppEntry>, String> {
        match self.shell_response(ShellCommand::ListApps)? {
            ShellResponse::Apps(apps) => Ok(apps),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn set_workspace(&self, workspace_id: WorkspaceId) -> Result<(), String> {
        match self.shell_response(ShellCommand::SetWorkspace { workspace_id })? {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn workspace(&self) -> Result<WorkspaceId, String> {
        match self.shell_response(ShellCommand::GetWorkspace)? {
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
        match self.shell_response(ShellCommand::SetWorkspaceLayout {
            workspace_id,
            layout,
        })? {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn workspace_layout(&self, workspace_id: WorkspaceId) -> Result<WorkspaceLayout, String> {
        match self.shell_response(ShellCommand::GetWorkspaceLayout { workspace_id })? {
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
        match self.shell_response(ShellCommand::SetWorkspaceRule { workspace_id, rule })? {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn workspace_rule(&self, workspace_id: WorkspaceId) -> Result<WorkspaceRule, String> {
        match self.shell_response(ShellCommand::GetWorkspaceRule { workspace_id })? {
            ShellResponse::WorkspaceRule(rule) => Ok(rule),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn toggle_scratchpad(&self) -> Result<bool, String> {
        match self.shell_response(ShellCommand::ToggleScratchpad)? {
            ShellResponse::ToggleState(state) => Ok(state),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn toggle_overview(&self) -> Result<bool, String> {
        match self.shell_response(ShellCommand::ToggleOverview)? {
            ShellResponse::ToggleState(state) => Ok(state),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn set_power_state(&self, power_state: SessionPowerState) -> Result<(), String> {
        match self.shell_response(ShellCommand::SetPowerState { power_state })? {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn session_snapshot(&self) -> Result<SessionSnapshot, String> {
        match self.shell_response(ShellCommand::GetSessionSnapshot)? {
            ShellResponse::SessionSnapshot(snapshot) => Ok(snapshot),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn set_locale(&self, locale: &str) -> Result<(), String> {
        match self.shell_response(ShellCommand::SetLocale {
            locale: String::from(locale),
        })? {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn locale(&self) -> Result<String, String> {
        match self.shell_response(ShellCommand::GetLocale)? {
            ShellResponse::Locale(locale) => Ok(locale),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn list_speech_voices(&self) -> Result<Vec<crate::audio::tts::VoiceCatalogEntry>, String> {
        match self.shell_response(ShellCommand::ListSpeechVoices)? {
            ShellResponse::SpeechVoices(voices) => Ok(voices),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn set_speech_voice(&self, voice_id: Option<&str>) -> Result<(), String> {
        match self.shell_response(ShellCommand::SetSpeechVoice {
            voice_id: voice_id.map(String::from),
        })? {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn speech_output_status(
        &self,
    ) -> Result<crate::services::ech_shell::SpeechOutputStatus, String> {
        match self.shell_response(ShellCommand::GetSpeechOutputStatus)? {
            ShellResponse::SpeechOutputStatus(status) => Ok(status),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn set_shell_density(&self, profile: ShellDensityProfile) -> Result<(), String> {
        match self.shell_response(ShellCommand::SetShellDensity { profile })? {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn shell_density(&self) -> Result<ShellDensityProfile, String> {
        match self.shell_response(ShellCommand::GetShellDensity)? {
            ShellResponse::ShellDensity(profile) => Ok(profile),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn set_motion_profile(&self, profile: MotionProfile) -> Result<(), String> {
        match self.shell_response(ShellCommand::SetMotionProfile { profile })? {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn motion_profile(&self) -> Result<MotionProfile, String> {
        match self.shell_response(ShellCommand::GetMotionProfile)? {
            ShellResponse::MotionProfile(profile) => Ok(profile),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn set_restore_disposition(&self, disposition: RestoreDisposition) -> Result<(), String> {
        match self.shell_response(ShellCommand::SetRestoreDisposition { disposition })? {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn restore_disposition(&self) -> Result<RestoreDisposition, String> {
        match self.shell_response(ShellCommand::GetRestoreDisposition)? {
            ShellResponse::RestoreDisposition(disposition) => Ok(disposition),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn set_stage_sets(&self, sets: Vec<StageSet>) -> Result<(), String> {
        match self.shell_response(ShellCommand::SetStageSets { sets })? {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn stage_sets(&self) -> Result<Vec<StageSet>, String> {
        match self.shell_response(ShellCommand::GetStageSets)? {
            ShellResponse::StageSets(sets) => Ok(sets),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn set_stage_set_policy(&self, policy: StageSetPolicy) -> Result<(), String> {
        match self.shell_response(ShellCommand::SetStageSetPolicy { policy })? {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn stage_set_policy(&self) -> Result<StageSetPolicy, String> {
        match self.shell_response(ShellCommand::GetStageSetPolicy)? {
            ShellResponse::StageSetPolicy(policy) => Ok(policy),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn set_window_rules(&self, rules: Vec<WindowRule>) -> Result<(), String> {
        match self.shell_response(ShellCommand::SetWindowRules { rules })? {
            ShellResponse::Ack => Ok(()),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn window_rules(&self) -> Result<Vec<WindowRule>, String> {
        match self.shell_response(ShellCommand::GetWindowRules)? {
            ShellResponse::WindowRules(rules) => Ok(rules),
            ShellResponse::Error(err) => Err(err),
            _ => Err(String::from("shell returned unexpected response")),
        }
    }

    pub fn capture_screen(&self, label: &str) -> Result<ScreenshotEntry, String> {
        match self.capture_response(CaptureCommand::CaptureDesktop {
            app_id: self.app_id,
            label: String::from(label),
        })? {
            CaptureResponse::Captured(entry) => Ok(entry),
            CaptureResponse::Error(err) => Err(err),
            _ => Err(String::from("capture returned unexpected response")),
        }
    }

    pub fn list_captures(&self, max_items: usize) -> Result<Vec<ScreenshotEntry>, String> {
        match self.capture_response(CaptureCommand::ListCaptures {
            app_id: self.app_id,
            max_items,
        })? {
            CaptureResponse::Captures(entries) => Ok(entries),
            CaptureResponse::Error(err) => Err(err),
            _ => Err(String::from("capture returned unexpected response")),
        }
    }

    pub fn save_capture_ppm(&self, capture_id: u64, path: &str) -> Result<(), String> {
        self.ensure_permission(DesktopPermission::ScreenCapture)?;
        self.ensure_file_access(path, true)?;
        match self.capture_response(CaptureCommand::GetCapture {
            app_id: self.app_id,
            capture_id,
        })? {
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
        match self.input_response(InputCommand::RegisterShortcutSink {
            app_id: self.app_id,
        })? {
            InputResponse::Ack => Ok(()),
            InputResponse::Error(err) => Err(err),
            _ => Err(String::from("input returned unexpected response")),
        }
    }

    pub fn poll_shortcuts(&self, max_events: usize) -> Result<Vec<ShellShortcut>, String> {
        match self.input_response(InputCommand::PollShortcuts {
            app_id: self.app_id,
            max_events,
        })? {
            InputResponse::Shortcuts(shortcuts) => Ok(shortcuts),
            InputResponse::Error(err) => Err(err),
            _ => Err(String::from("input returned unexpected response")),
        }
    }

    pub fn poll_input(&self, max_events: usize) -> Result<Vec<WindowInputEvent>, String> {
        match self.input_response(InputCommand::PollEvents {
            app_id: self.app_id,
            max_events,
        })? {
            InputResponse::Events { events, .. } => Ok(events),
            InputResponse::Error(err) => Err(err),
            _ => Err(String::from("input returned unexpected response")),
        }
    }

    pub fn read_file(&self, path: &str) -> Result<Vec<u8>, String> {
        self.ensure_permission(DesktopPermission::FileSystem)?;
        self.ensure_file_access(path, false)?;
        expect_store_file_data(self.store_response(StoreCommand::ReadFile {
            path: String::from(path),
        })?)
    }

    pub fn write_file(&self, path: &str, data: &[u8]) -> Result<(), String> {
        self.ensure_permission(DesktopPermission::FileSystem)?;
        self.ensure_file_access(path, true)?;
        expect_store_success(self.store_response(StoreCommand::WriteFile {
            path: String::from(path),
            data: data.to_vec(),
        })?)
    }

    pub fn rename_path(&self, from: &str, to: &str) -> Result<(), String> {
        self.ensure_permission(DesktopPermission::FileSystem)?;
        self.ensure_file_access(from, true)?;
        self.ensure_file_access(to, true)?;
        expect_store_success(self.store_response(StoreCommand::RenamePath {
            from: String::from(from),
            to: String::from(to),
        })?)
    }

    pub fn delete_file(&self, path: &str) -> Result<(), String> {
        self.ensure_permission(DesktopPermission::FileSystem)?;
        self.ensure_file_access(path, true)?;
        expect_store_success(self.store_response(StoreCommand::DeleteFile {
            path: String::from(path),
        })?)
    }

    pub fn delete_directory(&self, path: &str) -> Result<(), String> {
        self.ensure_permission(DesktopPermission::FileSystem)?;
        self.ensure_file_access(path, true)?;
        expect_store_success(self.store_response(StoreCommand::DeleteDirectory {
            path: String::from(path),
        })?)
    }

    pub fn create_directory(&self, path: &str) -> Result<(), String> {
        self.ensure_permission(DesktopPermission::FileSystem)?;
        self.ensure_file_access(path, true)?;
        expect_store_success(self.store_response(StoreCommand::CreateDirectory {
            path: String::from(path),
        })?)
    }

    pub fn list_directory(&self, path: &str) -> Result<Vec<FileEntry>, String> {
        self.ensure_permission(DesktopPermission::FileSystem)?;
        self.ensure_file_access(path, false)?;
        expect_store_directory(self.store_response(StoreCommand::ListDirectory {
            path: String::from(path),
        })?)
    }

    fn expect_ack(&self, response: DisplayResponse) -> Result<(), String> {
        match response {
            display_client_contract::DisplayResponse::Ack
            | display_client_contract::DisplayResponse::Presented { .. } => Ok(()),
            display_client_contract::DisplayResponse::Error(err) => Err(err),
            _ => Err(String::from("display returned unexpected response")),
        }
    }

    fn display_response(
        &self,
        command: display_client_contract::DisplayCommand,
    ) -> Result<display_client_contract::DisplayResponse, String> {
        display_client_contract::request_display_sync(self.app_id, command)
            .ok_or_else(|| String::from("display service unavailable"))
    }

    fn input_response(
        &self,
        command: input_client_contract::InputCommand,
    ) -> Result<input_client_contract::InputResponse, String> {
        input_client_contract::request_input_sync(self.app_id, command)
            .ok_or_else(|| String::from("input service unavailable"))
    }

    fn shell_response(
        &self,
        command: shell_client_contract::ShellCommand,
    ) -> Result<shell_client_contract::ShellResponse, String> {
        shell_client_contract::request_shell_sync(self.app_id, command)
            .ok_or_else(|| String::from("shell service unavailable"))
    }

    fn notification_response(
        &self,
        command: notification_client_contract::NotificationCommand,
    ) -> Result<notification_client_contract::NotificationResponse, String> {
        notification_client_contract::request_notification_sync(self.app_id, command)
            .ok_or_else(|| String::from("notification service unavailable"))
    }

    fn clipboard_response(
        &self,
        command: clipboard_client_contract::ClipboardCommand,
    ) -> Result<clipboard_client_contract::ClipboardResponse, String> {
        clipboard_client_contract::request_clipboard_sync(self.app_id, command)
            .ok_or_else(|| String::from("clipboard service unavailable"))
    }

    fn dialog_response(
        &self,
        command: dialog_client_contract::DialogCommand,
    ) -> Result<dialog_client_contract::DialogResponse, String> {
        dialog_client_contract::request_dialog_sync(self.app_id, command)
            .ok_or_else(|| String::from("dialog service unavailable"))
    }

    fn capture_response(
        &self,
        command: capture_client_contract::CaptureCommand,
    ) -> Result<capture_client_contract::CaptureResponse, String> {
        capture_client_contract::request_capture_sync(self.app_id, command)
            .ok_or_else(|| String::from("capture service unavailable"))
    }

    fn store_response(
        &self,
        command: store_client_contract::StoreCommand,
    ) -> Result<store_client_contract::StoreResponse, String> {
        store_client_contract::request_store_sync(self.app_id, command)
            .ok_or_else(|| String::from("store service unavailable"))
    }

    fn present_zero_copy(&self, window_id: WindowId, pixels: &[u32]) -> Result<(), String> {
        let descriptor = self.cached_surface_descriptor(window_id)?;
        let shared_surface = resolve_data_plane_surface(descriptor)
            .map_err(|_| String::from("shared surface unavailable"))?;

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
        surface: &Arc<SharedSurfaceMemory>,
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
        expect_shell_file_access(self.shell_response(ShellCommand::CheckFileAccess {
            app_id: self.app_id,
            path: String::from(path),
            write,
        })?)
    }
}

fn expect_store_file_data(response: StoreResponse) -> Result<Vec<u8>, String> {
    match response {
        StoreResponse::FileData(data) => Ok(data),
        StoreResponse::Error(err) => Err(err),
        _ => Err(String::from("store returned unexpected response")),
    }
}

fn expect_store_success(response: StoreResponse) -> Result<(), String> {
    match response {
        StoreResponse::Success => Ok(()),
        StoreResponse::Error(err) => Err(err),
        _ => Err(String::from("store returned unexpected response")),
    }
}

fn expect_store_directory(response: StoreResponse) -> Result<Vec<FileEntry>, String> {
    match response {
        StoreResponse::DirectoryContents(entries) => Ok(entries),
        StoreResponse::Error(err) => Err(err),
        _ => Err(String::from("store returned unexpected response")),
    }
}

fn expect_shell_file_access(response: ShellResponse) -> Result<(), String> {
    match response {
        ShellResponse::FileAccess(true) => Ok(()),
        ShellResponse::FileAccess(false) => Err(String::from("file access not granted")),
        ShellResponse::Error(err) => Err(err),
        _ => Err(String::from("shell returned unexpected response")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gui_store_helpers_preserve_exact_store_errors() {
        assert_eq!(
            expect_store_file_data(StoreResponse::Error(String::from(
                "xfs: unified reads are not wired to a real backend",
            )))
            .unwrap_err(),
            "xfs: unified reads are not wired to a real backend"
        );
        assert_eq!(
            expect_store_success(StoreResponse::Error(String::from(
                "xfs: unified VFS open is not wired to a real backend",
            )))
            .unwrap_err(),
            "xfs: unified VFS open is not wired to a real backend"
        );
        assert_eq!(
            expect_store_directory(StoreResponse::Error(String::from(
                "xfs: unified directory listing is not wired to a real backend",
            )))
            .unwrap_err(),
            "xfs: unified directory listing is not wired to a real backend"
        );
    }

    #[test]
    fn gui_shell_file_access_helper_preserves_exact_shell_errors() {
        assert_eq!(
            expect_shell_file_access(ShellResponse::Error(String::from("app not registered")))
                .unwrap_err(),
            "app not registered"
        );
        assert_eq!(
            expect_shell_file_access(ShellResponse::FileAccess(false)).unwrap_err(),
            "file access not granted"
        );
    }
}
