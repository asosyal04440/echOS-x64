//! Week-2 desktop protocol shared between EchDisplay and EchInput.

use alloc::string::String;
use alloc::vec::Vec;

/// Logical application id that owns windows and input channels.
pub type AppId = u32;
/// Unique surface id allocated by display service.
pub type SurfaceId = u64;
/// Unique window id allocated by the window manager.
pub type WindowId = u64;
/// Dialog request id.
pub type DialogId = u64;
/// Shell workspace id.
pub type WorkspaceId = u8;
pub type ScreenshotId = u64;
pub type ClientId = u32;
pub type GpuBufferHandle = u64;
pub type DamageEpoch = u64;
pub type FenceId = u64;

pub const MOD_SHIFT: u8 = 1 << 0;
pub const MOD_CTRL: u8 = 1 << 1;
pub const MOD_ALT: u8 = 1 << 2;
pub const MOD_SUPER: u8 = 1 << 3;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RingCursorPair {
    pub head: u32,
    pub tail: u32,
    pub capacity: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SharedSurfaceDescriptor {
    pub client_id: ClientId,
    pub surface_id: SurfaceId,
    pub width: u32,
    pub height: u32,
    pub pixel_stride: u32,
    pub generation: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DamagePacket {
    pub surface_id: SurfaceId,
    pub generation: u64,
    pub rect: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayPresentMode {
    Mailbox,
    VblankFifo,
    AdaptiveSync,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceTransform {
    Identity,
    Rotate90,
    Rotate180,
    Rotate270,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DamageTile {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanoutCandidate {
    pub surface_id: SurfaceId,
    pub z: u32,
    pub opaque: bool,
    pub transform: SurfaceTransform,
    pub damage: Rect,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaneAssignment {
    pub primary: Option<SurfaceId>,
    pub overlays: Vec<SurfaceId>,
    pub cursor: Option<SurfaceId>,
}

impl PlaneAssignment {
    pub fn empty() -> Self {
        Self {
            primary: None,
            overlays: Vec::new(),
            cursor: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompositorPass {
    BaseComposite,
    KawaseBlur { radius: u8, passes: u8 },
    SdfShadow { radius: u8, spread: u8 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameIntent {
    pub frame_id: u64,
    pub enqueue_timestamp_ns: u64,
    pub damage_tiles: Vec<DamageTile>,
    pub candidates: Vec<ScanoutCandidate>,
    pub composed_passes: Vec<CompositorPass>,
    pub target_refresh_hz: u32,
    pub mode: DisplayPresentMode,
    pub cursor_position: Option<Point>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VblankFeedback {
    pub timestamp_ns: u64,
    pub presented_frame_id: u64,
    pub refresh_hz: u32,
    pub graph_crc: u32,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbiOpcode {
    CreateWindow = 1,
    DestroyWindow = 2,
    MapSurface = 3,
    SubmitDamage = 4,
    InputEvent = 5,
    ShortcutEvent = 6,
}

/// Basic integer point.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// Integer rectangle.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> i32 {
        self.x.saturating_add(self.width as i32)
    }

    pub fn bottom(&self) -> i32 {
        self.y.saturating_add(self.height as i32)
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    pub fn contains(&self, point: Point) -> bool {
        point.x >= self.x && point.x < self.right() && point.y >= self.y && point.y < self.bottom()
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.right()
            && self.right() > other.x
            && self.y < other.bottom()
            && self.bottom() > other.y
    }

    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = self.right().min(other.right());
        let y1 = self.bottom().min(other.bottom());
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        Some(Rect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32))
    }

    pub fn union(&self, other: &Rect) -> Rect {
        let x0 = self.x.min(other.x);
        let y0 = self.y.min(other.y);
        let x1 = self.right().max(other.right());
        let y1 = self.bottom().max(other.bottom());
        Rect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32)
    }

    pub fn inset(&self, left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        let x = self.x.saturating_add(left);
        let y = self.y.saturating_add(top);
        let width = (self.width as i32)
            .saturating_sub(left)
            .saturating_sub(right)
            .max(0) as u32;
        let height = (self.height as i32)
            .saturating_sub(top)
            .saturating_sub(bottom)
            .max(0) as u32;
        Rect::new(x, y, width, height)
    }

    pub fn local_point(&self, point: Point) -> Point {
        Point::new(
            point.x.saturating_sub(self.x),
            point.y.saturating_sub(self.y),
        )
    }
}

#[derive(Clone, Debug)]
pub struct WindowInfo {
    pub id: WindowId,
    pub app_id: AppId,
    pub surface_id: SurfaceId,
    pub title: String,
    pub frame_rect: Rect,
    pub content_rect: Rect,
    pub visible: bool,
    pub focused: bool,
    pub minimized: bool,
    pub maximized: bool,
    pub z_index: u32,
}

/// Generic key/button state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyState {
    Pressed,
    Released,
}

/// Pointer button id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerButton {
    Left,
    Right,
    Middle,
    Other(u8),
}

/// Unified input event format for desktop routing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputEvent {
    Key {
        unicode: Option<char>,
        scan_code: u16,
        modifiers: u8,
        state: KeyState,
    },
    PointerMove {
        position: Point,
        delta: Point,
    },
    PointerButton {
        button: PointerButton,
        state: KeyState,
        position: Point,
    },
    Scroll {
        delta: Point,
        position: Point,
    },
}

/// Window-targeted input envelope delivered to native applications.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowInputEvent {
    pub app_id: AppId,
    pub window_id: WindowId,
    pub global_position: Option<Point>,
    pub local_position: Option<Point>,
    pub captured: bool,
    pub event: InputEvent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellShortcut {
    AppSwitcherNext,
    AppSwitcherConfirm,
    AppSwitcherCancel,
    Workspace(WorkspaceId),
    ToggleCommandPalette,
    ToggleQuickSettings,
    CycleStageSet,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandPaletteAction {
    pub id: u64,
    pub title: String,
    pub category: String,
    pub shortcut: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageSet {
    pub id: u64,
    pub name: String,
    pub window_ids: Vec<WindowId>,
    pub pinned: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DesktopPermission {
    ClipboardRead,
    ClipboardWrite,
    Notifications,
    FileDialogs,
    FileSystem,
    ScreenCapture,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionState {
    Ask,
    Granted,
    Denied,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppHealth {
    Idle,
    Running,
    Attention,
    Crashed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionPowerState {
    Active,
    Locked,
    Suspended,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipboardPayload {
    Empty,
    Text(String),
    Files(alloc::vec::Vec<String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationLevel {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationRequest {
    pub app_id: AppId,
    pub title: String,
    pub message: String,
    pub level: NotificationLevel,
    pub action_label: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationEntry {
    pub id: u64,
    pub app_id: AppId,
    pub source_name: String,
    pub title: String,
    pub message: String,
    pub level: NotificationLevel,
    pub read: bool,
    pub timestamp_ticks: u64,
    pub action_label: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogKind {
    OpenFile,
    SaveFile,
    PickFolder,
    Message,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogRequest {
    pub id: DialogId,
    pub app_id: AppId,
    pub kind: DialogKind,
    pub title: String,
    pub message: String,
    pub path_hint: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DialogSelection {
    Accepted(String),
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogResult {
    pub id: DialogId,
    pub app_id: AppId,
    pub selection: DialogSelection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellAppEntry {
    pub app_id: AppId,
    pub name: String,
    pub window_id: Option<WindowId>,
    pub visible: bool,
    pub focused: bool,
    pub workspace_id: WorkspaceId,
    pub running: bool,
    pub health: AppHealth,
    pub launch_count: u32,
    pub crash_count: u32,
    pub needs_attention: bool,
    pub status_line: String,
    pub auto_restore: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionEntry {
    pub permission: DesktopPermission,
    pub state: PermissionState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub workspace_id: WorkspaceId,
    pub power_state: SessionPowerState,
    pub unread_notifications: u32,
    pub apps_running: u32,
    pub apps_crashed: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileGrant {
    pub path_prefix: String,
    pub read_only: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenshotEntry {
    pub id: ScreenshotId,
    pub app_id: AppId,
    pub label: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessibilityRole {
    Window,
    Button,
    List,
    ListItem,
    Text,
    Input,
    Notification,
    Dialog,
    Toolbar,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessibilityNode {
    pub id: u64,
    pub app_id: AppId,
    pub role: AccessibilityRole,
    pub label: String,
    pub description: String,
    pub focused: bool,
    pub bounds: Rect,
}
