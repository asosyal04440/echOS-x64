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
pub type SceneNodeId = u64;
pub type SceneRootId = u64;
pub type SceneRevision = u64;
pub type TextBlobId = u64;
pub type AtlasId = u64;
pub type FrameTicket = u64;

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
pub struct OutputMode {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
}

impl OutputMode {
    pub const fn new(width: u32, height: u32, refresh_hz: u32) -> Self {
        Self {
            width,
            height,
            refresh_hz,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum WorkspaceLayout {
    Dwindle,
    Master,
    Floating,
    Overview,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LayerRole {
    Background,
    Bottom,
    Window,
    TopBar,
    Dock,
    Overlay,
    Modal,
    WorkspaceScratchpad,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkspaceRule {
    pub gaps_in: u16,
    pub gaps_out: u16,
    pub border_size: u16,
    pub rounding: u16,
    pub decorate: bool,
    pub persistent: bool,
    pub default_name: [u8; 16],
    pub layout: WorkspaceLayout,
}

impl WorkspaceRule {
    pub const fn new(default_name: [u8; 16], layout: WorkspaceLayout) -> Self {
        Self {
            gaps_in: 10,
            gaps_out: 18,
            border_size: 1,
            rounding: 14,
            decorate: true,
            persistent: true,
            default_name,
            layout,
        }
    }

    pub fn default_name_str(&self) -> String {
        let len = self
            .default_name
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(self.default_name.len());
        String::from(core::str::from_utf8(&self.default_name[..len]).unwrap_or("Workspace"))
    }
}

impl Default for WorkspaceRule {
    fn default() -> Self {
        Self::new(*b"Workspace\0\0\0\0\0\0\0", WorkspaceLayout::Dwindle)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowFlags {
    pub floating: bool,
    pub pseudotile: bool,
    pub pinned: bool,
    pub scratchpad: bool,
    pub decorate: bool,
}

impl WindowFlags {
    pub const fn decorated() -> Self {
        Self {
            floating: false,
            pseudotile: false,
            pinned: false,
            scratchpad: false,
            decorate: true,
        }
    }

    pub const fn layer_shell() -> Self {
        Self {
            floating: true,
            pseudotile: false,
            pinned: true,
            scratchpad: false,
            decorate: false,
        }
    }
}

impl Default for WindowFlags {
    fn default() -> Self {
        Self::decorated()
    }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageLane {
    Shell,
    Window,
    Text,
    Cursor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidationTarget {
    TopBar,
    Dock,
    Launcher,
    Overview,
    QuickSettings,
    CommandPalette,
    NotificationCenter,
    Dialog,
    ContextMenu,
    Switcher,
    LockScreen,
    WorkspaceViewport,
    Cursor,
    Wallpaper,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidationReason {
    StateChanged,
    LayoutChanged,
    AnimationAdvanced,
    InputHoverChanged,
    FocusChanged,
    ThemeChanged,
    TextChanged,
    AssetChanged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneInvalidation {
    pub target: InvalidationTarget,
    pub reason: InvalidationReason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellState {
    ColdBoot,
    DesktopReady,
    OverlayInteractive,
    WorkspaceTransition,
    Locked,
    Suspended,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TextRunStyle {
    Ui,
    Mono,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderObjectKind {
    SolidRect {
        color: u32,
        corner_radius: u16,
    },
    Raster {
        width: u32,
        height: u32,
        pixels: Vec<u32>,
    },
    TextRun {
        blob_id: TextBlobId,
        text: String,
        color: u32,
        style: TextRunStyle,
        max_width: u32,
    },
    GlyphRun {
        blob_id: TextBlobId,
        atlas_id: AtlasId,
        width: u32,
        height: u32,
        pixels: Vec<u32>,
        color: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderObject {
    pub object_id: u64,
    pub bounds: Rect,
    pub clip: Option<Rect>,
    pub z_index: u32,
    pub opacity: u8,
    pub lane: DamageLane,
    pub kind: RenderObjectKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneUpdate {
    pub root_id: SceneRootId,
    pub revision: SceneRevision,
    pub render_objects: Vec<RenderObject>,
    pub damage_hint: Vec<Rect>,
    pub semantic_root: Option<u64>,
}

impl SceneUpdate {
    pub fn canonicalize(&mut self) {
        self.render_objects
            .sort_by_key(|object| (object.z_index, object.object_id));
        self.damage_hint.retain(|rect| !rect.is_empty());
        self.damage_hint
            .sort_by_key(|rect| (rect.y, rect.x, rect.height, rect.width));
        self.damage_hint.dedup();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowBufferMode {
    Pixels,
    Scene,
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
    pub workspace_id: WorkspaceId,
    pub layer_role: LayerRole,
    pub flags: WindowFlags,
    pub scene_node_id: SceneNodeId,
    pub scene_root: Option<SceneRootId>,
    pub semantic_root: Option<u64>,
    pub buffer_mode: WindowBufferMode,
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
    ToggleOverview,
    ToggleScratchpad,
    LaunchTerminal,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreDisposition {
    NoRestore,
    RestoreIfClean,
    RestoreIfPinned,
    ForceRestoreShellOwned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapTemplate {
    LeftHalf,
    RightHalf,
    TopHalf,
    BottomHalf,
    TopLeftQuarter,
    TopRightQuarter,
    BottomLeftQuarter,
    BottomRightQuarter,
    LeftThird,
    CenterThird,
    RightThird,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapGroup {
    pub id: u64,
    pub template: SnapTemplate,
    pub monitor_id: u32,
    pub window_ids: Vec<WindowId>,
    pub restore: RestoreDisposition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StageSetPolicy {
    pub active_stage_set: Option<u64>,
    pub restore_on_login: bool,
    pub follow_workspace: bool,
}

impl Default for StageSetPolicy {
    fn default() -> Self {
        Self {
            active_stage_set: None,
            restore_on_login: true,
            follow_workspace: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellDensityProfile {
    Comfort,
    Balanced,
    Compact,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MotionProfile {
    Calm,
    Standard,
    Expressive,
    Reduced,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VrrPolicy {
    Off,
    On,
    Auto,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HdrPolicy {
    Off,
    On,
    Auto,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowRule {
    pub app_id: Option<AppId>,
    pub title_contains: Option<String>,
    pub workspace_id: Option<WorkspaceId>,
    pub monitor_id: Option<u32>,
    pub force_floating: bool,
    pub pin: bool,
    pub scratchpad: bool,
    pub pseudotile: bool,
    pub snap_template: Option<SnapTemplate>,
    pub restore: RestoreDisposition,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayCapability {
    pub connected_outputs: u8,
    pub fractional_scaling: bool,
    pub adaptive_sync: bool,
    pub hdr_output: bool,
    pub hdr_metadata: bool,
    pub ten_bit_scanout: bool,
    pub icc_profile: bool,
    pub color_transform: bool,
    pub mirror: bool,
    pub rotation: bool,
    pub multi_monitor: bool,
    pub direct_scanout: bool,
    pub max_refresh_hz: u32,
    pub supported_scales_100x: Vec<u16>,
    pub supported_modes: Vec<OutputMode>,
}

impl Default for DisplayCapability {
    fn default() -> Self {
        Self {
            connected_outputs: 1,
            fractional_scaling: true,
            adaptive_sync: false,
            hdr_output: false,
            hdr_metadata: false,
            ten_bit_scanout: false,
            icc_profile: false,
            color_transform: false,
            mirror: false,
            rotation: false,
            multi_monitor: false,
            direct_scanout: false,
            max_refresh_hz: 60,
            supported_scales_100x: alloc::vec![100, 125, 150, 175, 200],
            supported_modes: alloc::vec![OutputMode::new(1920, 1080, 60)],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonitorPolicy {
    pub output_id: u32,
    pub scale_100x: u16,
    pub text_scale_100x: u16,
    pub refresh_hz: u32,
    pub vrr_policy: VrrPolicy,
    pub hdr_policy: HdrPolicy,
    pub transform: SurfaceTransform,
    pub mirror_target: Option<u32>,
    pub workspace_binding: Option<WorkspaceId>,
    pub color_profile: String,
}

impl MonitorPolicy {
    pub fn single_output(output_mode: OutputMode) -> Self {
        Self {
            output_id: 0,
            scale_100x: 100,
            text_scale_100x: 100,
            refresh_hz: output_mode.refresh_hz,
            vrr_policy: VrrPolicy::Auto,
            hdr_policy: HdrPolicy::Off,
            transform: SurfaceTransform::Identity,
            mirror_target: None,
            workspace_binding: Some(0),
            color_profile: String::from("srgb"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayProfile {
    pub primary_output: u32,
    pub outputs: Vec<MonitorPolicy>,
    pub capability: DisplayCapability,
}

impl DisplayProfile {
    pub fn single_output(output_mode: OutputMode) -> Self {
        Self {
            primary_output: 0,
            outputs: alloc::vec![MonitorPolicy::single_output(output_mode)],
            capability: DisplayCapability {
                max_refresh_hz: output_mode.refresh_hz,
                supported_modes: alloc::vec![output_mode],
                ..DisplayCapability::default()
            },
        }
    }
}

impl Default for DisplayProfile {
    fn default() -> Self {
        Self::single_output(OutputMode::new(1920, 1080, 60))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccessibilityProfile {
    pub screen_reader: bool,
    pub magnifier: bool,
    pub contrast_theme: bool,
    pub color_filter: bool,
    pub reduced_motion: bool,
    pub sticky_keys: bool,
    pub slow_keys: bool,
    pub cursor_scale_100x: u16,
    pub text_scale_100x: u16,
    pub captions_enabled: bool,
    pub voice_access_mode: bool,
}

impl Default for AccessibilityProfile {
    fn default() -> Self {
        Self {
            screen_reader: false,
            magnifier: false,
            contrast_theme: false,
            color_filter: false,
            reduced_motion: false,
            sticky_keys: false,
            slow_keys: false,
            cursor_scale_100x: 100,
            text_scale_100x: 100,
            captions_enabled: false,
            voice_access_mode: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessibilityEventKind {
    FocusChanged,
    WindowOpened,
    WindowClosed,
    DialogOpened,
    SelectionChanged,
    NotificationPosted,
    ValueChanged,
    LiveRegionChanged,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessibilityEvent {
    pub app_id: AppId,
    pub window_id: Option<WindowId>,
    pub node_id: Option<u64>,
    pub kind: AccessibilityEventKind,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptionEvent {
    pub app_id: AppId,
    pub source_label: String,
    pub text: String,
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
    pub workspace_layout: WorkspaceLayout,
    pub power_state: SessionPowerState,
    pub unread_notifications: u32,
    pub apps_running: u32,
    pub apps_crashed: u32,
    pub overview_active: bool,
    pub scratchpad_visible: bool,
    pub shell_ready: bool,
    pub boot_clean_desktop: bool,
    pub output_scale: u32,
    pub text_scale: u32,
    pub clipboard_history_len: u32,
    pub accessibility_profile: AccessibilityProfile,
    pub display_profile: DisplayProfile,
    pub shell_density: ShellDensityProfile,
    pub motion_profile: MotionProfile,
    pub restore_state: RestoreDisposition,
    pub stage_set_policy: StageSetPolicy,
    pub locale: String,
    pub theme_variant: String,
    pub shell_state: ShellState,
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
