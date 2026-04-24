//! # EchDisplay - Week-2 Display Service
//!
//! Sorumluluk:
//! - Window lifecycle
//! - Surface buffer commit
//! - Focus/raise
//! - Damage-tracked composition
//! - Pointer hit-test, drag and resize capture
//! - Native window chrome actions

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

use crate::gop::framebuffer::Framebuffer;
use crate::gui::damage::DamageTracker;
use crate::gui::protocol::{
    AppId, DamageEpoch, DamagePacket, DisplayCapability, DisplayPresentMode, DisplayProfile,
    FrameIntent, HdrPolicy, InputEvent, KeyState, LayerRole, MonitorPolicy, OutputMode,
    PlaneAssignment, Point, PointerButton, Rect, SceneNodeId, SceneRootId, SceneUpdate,
    SharedSurfaceDescriptor, SurfaceId, SurfaceTransform, VblankFeedback, VrrPolicy,
    WindowBufferMode, WindowFlags, WindowId, WindowInfo, WorkspaceId,
};
use crate::gui::renderer::{CpuRenderer, GpuRenderer, Renderer};
use crate::gui::shell;
use crate::gui::surface::{SurfaceError, SurfaceInfo, SurfaceManager};
use crate::gui::surface_memory::SharedSurfaceMemory;
use crate::gui::text::{TextStyle, TextSystem};
use crate::gui::theme::{Theme, ThemeMode, WindowChromeVariant};
use crate::gui::window_manager::{
    chrome_button_rect, titlebar_rect, ChromeButton, ResizeEdge, WindowError, WindowHitTarget,
    WindowManager, BORDER_THICKNESS, CHROME_BUTTON_SIZE, MIN_CONTENT_HEIGHT, MIN_CONTENT_WIDTH,
    TITLEBAR_HEIGHT,
};
use crate::services::display_atomic::{
    AtomicPresenter, HotPathMetrics, MailboxRing, SurfacePlacement,
};
use crate::services::ech_shell::{get_shell_service, ShellCommand, ShellResponse};

const DISPLAY_COMMAND_QUEUE_CAPACITY: usize = 256;
const DISPLAY_RESPONSE_QUEUE_CAPACITY: usize = 256;
const COMPOSITION_DIAGNOSTIC_HISTORY: usize = 12;
const DEFAULT_DESKTOP_WIDTH: u32 = 1920;
const DEFAULT_DESKTOP_HEIGHT: u32 = 1080;

#[derive(Clone, Debug)]
enum SurfaceContent {
    PixelsOwned(Vec<u32>),
    PixelsShared(Arc<SharedSurfaceMemory>),
    Scene(SceneUpdate),
}

#[derive(Clone, Debug)]
struct ComposedWindowSnapshot {
    id: WindowId,
    app_id: AppId,
    surface_id: SurfaceId,
    title: String,
    frame_rect: Rect,
    content_rect: Rect,
    visible: bool,
    focused: bool,
    minimized: bool,
    maximized: bool,
    z_index: u32,
    workspace_id: WorkspaceId,
    layer_role: LayerRole,
    flags: WindowFlags,
    scene_node_id: SceneNodeId,
    scene_root: Option<SceneRootId>,
    semantic_root: Option<u64>,
    buffer_mode: WindowBufferMode,
    shared_mapped: bool,
    gpu_buffer_handle: u64,
    damage_epoch: DamageEpoch,
    fence_id: u64,
    content: SurfaceContent,
}

impl ComposedWindowSnapshot {
    fn window_info(&self) -> WindowInfo {
        WindowInfo {
            id: self.id,
            app_id: self.app_id,
            surface_id: self.surface_id,
            title: self.title.clone(),
            frame_rect: self.frame_rect,
            content_rect: self.content_rect,
            visible: self.visible,
            focused: self.focused,
            minimized: self.minimized,
            maximized: self.maximized,
            z_index: self.z_index,
            workspace_id: self.workspace_id,
            layer_role: self.layer_role,
            flags: self.flags,
            scene_node_id: self.scene_node_id,
            scene_root: self.scene_root,
            semantic_root: self.semantic_root,
            buffer_mode: self.buffer_mode,
        }
    }

    fn placement(&self) -> SurfacePlacement {
        SurfacePlacement {
            surface_id: self.surface_id,
            rect: self.frame_rect,
            z_index: self.z_index,
            opaque: !self.minimized,
        }
    }
}

fn translate_rect(rect: Rect, offset_x: i32, offset_y: i32) -> Rect {
    Rect::new(
        rect.x.saturating_sub(offset_x),
        rect.y.saturating_sub(offset_y),
        rect.width,
        rect.height,
    )
}

fn translated_window_info(
    snapshot: &ComposedWindowSnapshot,
    offset_x: i32,
    offset_y: i32,
) -> WindowInfo {
    WindowInfo {
        id: snapshot.id,
        app_id: snapshot.app_id,
        surface_id: snapshot.surface_id,
        title: snapshot.title.clone(),
        frame_rect: translate_rect(snapshot.frame_rect, offset_x, offset_y),
        content_rect: translate_rect(snapshot.content_rect, offset_x, offset_y),
        visible: snapshot.visible,
        focused: snapshot.focused,
        minimized: snapshot.minimized,
        maximized: snapshot.maximized,
        z_index: snapshot.z_index,
        workspace_id: snapshot.workspace_id,
        layer_role: snapshot.layer_role,
        flags: snapshot.flags,
        scene_node_id: snapshot.scene_node_id,
        scene_root: snapshot.scene_root,
        semantic_root: snapshot.semantic_root,
        buffer_mode: snapshot.buffer_mode,
    }
}

fn layer_rank(role: LayerRole) -> u8 {
    match role {
        LayerRole::Background => 0,
        LayerRole::Bottom => 1,
        LayerRole::Window => 2,
        LayerRole::TopBar | LayerRole::Dock => 3,
        LayerRole::Overlay => 4,
        LayerRole::Modal => 5,
        LayerRole::WorkspaceScratchpad => 6,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CompositionDiagnosticCode {
    UnresolvedSurface,
    DuplicateSurfaceResolution,
    BufferModeMismatch,
    StaleSharedSurface,
    InconsistentSceneMetadata,
}

impl CompositionDiagnosticCode {
    const fn index(self) -> usize {
        match self {
            Self::UnresolvedSurface => 0,
            Self::DuplicateSurfaceResolution => 1,
            Self::BufferModeMismatch => 2,
            Self::StaleSharedSurface => 3,
            Self::InconsistentSceneMetadata => 4,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::UnresolvedSurface => "unresolved-surface",
            Self::DuplicateSurfaceResolution => "duplicate-surface",
            Self::BufferModeMismatch => "buffer-mode-mismatch",
            Self::StaleSharedSurface => "stale-shared-surface",
            Self::InconsistentSceneMetadata => "inconsistent-scene-metadata",
        }
    }
}

#[derive(Clone, Debug)]
struct CompositionDiagnosticEvent {
    code: CompositionDiagnosticCode,
    window_id: WindowId,
    surface_id: SurfaceId,
}

#[derive(Clone, Debug)]
struct CompositionDiagnostics {
    counts: [u64; 5],
    recent: Vec<CompositionDiagnosticEvent>,
}

impl CompositionDiagnostics {
    fn new() -> Self {
        Self {
            counts: [0; 5],
            recent: Vec::new(),
        }
    }

    fn record(
        &mut self,
        code: CompositionDiagnosticCode,
        window_id: WindowId,
        surface_id: SurfaceId,
    ) {
        self.counts[code.index()] = self.counts[code.index()].saturating_add(1);
        if self.recent.len() >= COMPOSITION_DIAGNOSTIC_HISTORY {
            self.recent.remove(0);
        }
        self.recent.push(CompositionDiagnosticEvent {
            code,
            window_id,
            surface_id,
        });
    }

    fn count(&self, code: CompositionDiagnosticCode) -> u64 {
        self.counts[code.index()]
    }

    fn overlay_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for code in [
            CompositionDiagnosticCode::UnresolvedSurface,
            CompositionDiagnosticCode::DuplicateSurfaceResolution,
            CompositionDiagnosticCode::BufferModeMismatch,
            CompositionDiagnosticCode::StaleSharedSurface,
            CompositionDiagnosticCode::InconsistentSceneMetadata,
        ] {
            let count = self.count(code);
            if count > 0 {
                lines.push(format!("{}: {}", code.label(), count));
            }
        }
        if let Some(last) = self.recent.last() {
            lines.push(format!(
                "last={} w{} s{}",
                last.code.label(),
                last.window_id,
                last.surface_id
            ));
        }
        lines
    }
}

struct PresentPlan {
    theme_mode: ThemeMode,
    native_scanout_available: bool,
    cursor: Point,
    placements: Vec<SurfacePlacement>,
    snapshots: Vec<ComposedWindowSnapshot>,
    diagnostics_overlay: Vec<String>,
    show_desktop_dashboard: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DisplayRuntimeActivity {
    Idle,
    Interactive,
    Animation,
    Fullscreen,
}

impl DisplayRuntimeActivity {
    const fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Interactive => "interactive",
            Self::Animation => "animation",
            Self::Fullscreen => "fullscreen",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct EffectiveMonitorRuntimeState {
    output_id: u32,
    scale_100x: u16,
    text_scale_100x: u16,
    requested_vrr: VrrPolicy,
    effective_vrr: VrrPolicy,
    requested_hdr: HdrPolicy,
    effective_hdr: HdrPolicy,
    refresh_hz: u32,
    mirrored: bool,
    transform: SurfaceTransform,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputRouting {
    None,
    DeliverTo {
        app_id: AppId,
        window_id: WindowId,
        global_position: Option<Point>,
        local_position: Option<Point>,
        captured: bool,
    },
    FocusOnly(AppId),
}

#[derive(Clone, Copy, Debug)]
enum InteractionKind {
    Drag {
        grab_offset: Point,
        frame_rect: Rect,
    },
    Resize {
        edge: ResizeEdge,
        start_pointer: Point,
        start_frame: Rect,
    },
}

#[derive(Clone, Copy, Debug)]
struct WindowInteraction {
    window_id: WindowId,
    kind: InteractionKind,
}

#[derive(Clone, Copy, Debug)]
struct PointerCapture {
    window_id: WindowId,
    origin: Point,
    threshold_crossed: bool,
}

#[derive(Clone, Debug)]
pub enum DisplayCommand {
    CreateWindow {
        app_id: AppId,
        title: String,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
    CreateWindowWithMeta {
        app_id: AppId,
        title: String,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        workspace_id: WorkspaceId,
        layer_role: LayerRole,
        flags: WindowFlags,
    },
    DestroyWindow {
        window_id: WindowId,
    },
    MoveWindow {
        window_id: WindowId,
        x: i32,
        y: i32,
    },
    ResizeWindow {
        window_id: WindowId,
        width: u32,
        height: u32,
    },
    FocusWindow {
        window_id: WindowId,
    },
    SetWindowVisibility {
        window_id: WindowId,
        visible: bool,
    },
    SetWindowTitle {
        window_id: WindowId,
        title: String,
    },
    SetWindowMeta {
        window_id: WindowId,
        workspace_id: WorkspaceId,
        layer_role: LayerRole,
        flags: WindowFlags,
    },
    MoveWindowToWorkspace {
        window_id: WindowId,
        workspace_id: WorkspaceId,
    },
    CommitWindowBuffer {
        window_id: WindowId,
        pixels: Vec<u32>,
    },
    CommitScene {
        window_id: WindowId,
        scene: SceneUpdate,
    },
    MapWindowSurface {
        window_id: WindowId,
    },
    SubmitWindowDamage {
        window_id: WindowId,
        packet: DamagePacket,
    },
    SetPresentMode {
        mode: DisplayPresentMode,
    },
    SetOutputMode {
        mode: OutputMode,
    },
    SetDisplayProfile {
        profile: DisplayProfile,
    },
    SetThemeMode {
        mode: ThemeMode,
    },
    SubmitFrameIntent {
        intent: FrameIntent,
    },
    RouteInputEvent {
        event: InputEvent,
    },
    QueryOutputMode,
    ListOutputModes,
    QueryDisplayCapability,
    QueryDisplayProfile,
    QueryPresentMetrics,
    ListWindows,
    ListSurfaces,
    SnapshotDesktop,
    Present,
}

#[derive(Clone, Debug)]
pub enum DisplayResponse {
    Ack,
    WindowCreated {
        window_id: WindowId,
        surface_id: SurfaceId,
        content_rect: Rect,
    },
    WindowList {
        windows: Vec<WindowInfo>,
    },
    SurfaceMapped(SharedSurfaceDescriptor),
    SurfaceList {
        surfaces: Vec<SurfaceInfo>,
    },
    DesktopSnapshot {
        width: u32,
        height: u32,
        pixels: Vec<u32>,
    },
    Presented {
        feedback: VblankFeedback,
        assignment: PlaneAssignment,
    },
    InputRoute(InputRouting),
    OutputModeState {
        current: OutputMode,
        requested: OutputMode,
        effective: OutputMode,
    },
    OutputModeCatalog {
        modes: Vec<OutputMode>,
    },
    DisplayCapability(DisplayCapability),
    DisplayProfile(DisplayProfile),
    PresentMetrics {
        metrics: HotPathMetrics,
    },
    Error(String),
}

pub struct EchDisplay {
    framebuffer: Arc<Mutex<Framebuffer>>,
    running: AtomicBool,
    screen_rect: Mutex<Rect>,
    physical_output_mode: OutputMode,
    requested_output_mode: Mutex<OutputMode>,
    effective_output_mode: Mutex<OutputMode>,
    display_profile: Mutex<DisplayProfile>,
    surfaces: Mutex<SurfaceManager>,
    windows: Mutex<WindowManager>,
    damage: Mutex<DamageTracker>,
    interaction: Mutex<Option<WindowInteraction>>,
    pointer_capture: Mutex<Option<PointerCapture>>,
    cursor_position: Mutex<Point>,
    swallow_left_release: Mutex<bool>,
    atomic_presenter: Mutex<AtomicPresenter>,
    theme_mode: Mutex<ThemeMode>,
    runtime_activity: Mutex<DisplayRuntimeActivity>,
    effective_outputs: Mutex<Vec<EffectiveMonitorRuntimeState>>,
    effective_vrr_policy: Mutex<VrrPolicy>,
    effective_hdr_policy: Mutex<HdrPolicy>,
    diagnostics: Mutex<CompositionDiagnostics>,
    joined_damage_epochs: Mutex<BTreeMap<SurfaceId, DamageEpoch>>,
    last_presented_frame: AtomicU64,
    command_queue: MailboxRing<DisplayCommand>,
    response_queue: MailboxRing<DisplayResponse>,
}

fn preferred_initial_output_mode(physical: OutputMode) -> OutputMode {
    if physical.width >= DEFAULT_DESKTOP_WIDTH && physical.height >= DEFAULT_DESKTOP_HEIGHT {
        OutputMode::new(
            DEFAULT_DESKTOP_WIDTH,
            DEFAULT_DESKTOP_HEIGHT,
            physical.refresh_hz,
        )
    } else {
        physical
    }
}

impl EchDisplay {
    pub fn new(framebuffer: Arc<Mutex<Framebuffer>>) -> Self {
        let (screen_rect, physical_output_mode, effective_output_mode) = {
            let mut fb = framebuffer.lock();
            let physical = OutputMode::new(fb.width as u32, fb.height as u32, 60);
            let effective = preferred_initial_output_mode(physical);
            fb.width = effective.width as usize;
            fb.height = effective.height as usize;
            (
                Rect::new(0, 0, effective.width, effective.height),
                physical,
                effective,
            )
        };
        crate::drivers::mouse::set_bounds(screen_rect.width as i32, screen_rect.height as i32);
        let cursor_origin = Point::new(
            (screen_rect.width as i32 / 2).max(0),
            (screen_rect.height as i32 / 2).max(0),
        );

        Self {
            framebuffer,
            running: AtomicBool::new(false),
            screen_rect: Mutex::new(screen_rect),
            physical_output_mode,
            requested_output_mode: Mutex::new(effective_output_mode),
            effective_output_mode: Mutex::new(effective_output_mode),
            display_profile: Mutex::new(DisplayProfile::single_output(effective_output_mode)),
            surfaces: Mutex::new(SurfaceManager::new()),
            windows: Mutex::new(WindowManager::new()),
            damage: Mutex::new(DamageTracker::new()),
            interaction: Mutex::new(None),
            pointer_capture: Mutex::new(None),
            cursor_position: Mutex::new(cursor_origin),
            swallow_left_release: Mutex::new(false),
            atomic_presenter: Mutex::new(AtomicPresenter::new()),
            theme_mode: Mutex::new(Theme::default_mode()),
            runtime_activity: Mutex::new(DisplayRuntimeActivity::Idle),
            effective_outputs: Mutex::new(Vec::new()),
            effective_vrr_policy: Mutex::new(VrrPolicy::Off),
            effective_hdr_policy: Mutex::new(HdrPolicy::Off),
            diagnostics: Mutex::new(CompositionDiagnostics::new()),
            joined_damage_epochs: Mutex::new(BTreeMap::new()),
            last_presented_frame: AtomicU64::new(0),
            command_queue: MailboxRing::with_capacity_pow2(DISPLAY_COMMAND_QUEUE_CAPACITY),
            response_queue: MailboxRing::with_capacity_pow2(DISPLAY_RESPONSE_QUEUE_CAPACITY),
        }
    }

    pub fn start(&self) {
        self.framebuffer.lock().enable_double_buffering();
        self.running.store(true, Ordering::SeqCst);
        crate::serial_println!("[ECHDISPLAY] Week-2 display service started");
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        crate::serial_println!("[ECHDISPLAY] Week-2 display service stopped");
    }

    pub fn capture_composed_region(
        &self,
        region: Rect,
        exclude_windows: &[WindowId],
        include_cursor: bool,
    ) -> Vec<u32> {
        let Some(region) = region.intersection(&self.screen_rect()) else {
            return Vec::new();
        };

        let exclude: BTreeSet<WindowId> = exclude_windows.iter().copied().collect();
        let mut fb = Framebuffer::new_offscreen(region.width as usize, region.height as usize);
        let mut cpu_renderer = CpuRenderer::new();
        let mut title_text_system = TextSystem::new();
        let damage = Rect::new(0, 0, region.width, region.height);
        let mode = *self.theme_mode.lock();

        for snapshot in self.ordering(true).iter() {
            if exclude.contains(&snapshot.id) || snapshot.frame_rect.intersection(&region).is_none()
            {
                continue;
            }

            let window = translated_window_info(snapshot, region.x, region.y);
            match &snapshot.content {
                SurfaceContent::Scene(scene) => draw_window_scene(
                    &mut fb,
                    &mut cpu_renderer,
                    &window,
                    scene,
                    &mut title_text_system,
                    damage,
                    mode,
                ),
                SurfaceContent::PixelsOwned(pixels) => draw_window(
                    &mut fb,
                    &window,
                    pixels,
                    &mut title_text_system,
                    damage,
                    mode,
                ),
                SurfaceContent::PixelsShared(shared) => shared.with_pixels(|pixels| {
                    draw_window(
                        &mut fb,
                        &window,
                        pixels,
                        &mut title_text_system,
                        damage,
                        mode,
                    );
                }),
            }
        }

        if include_cursor {
            let cursor = *self.cursor_position.lock();
            draw_cursor(
                &mut fb,
                Point::new(
                    cursor.x.saturating_sub(region.x),
                    cursor.y.saturating_sub(region.y),
                ),
                damage,
            );
        }

        fb.front_buffer().to_vec()
    }

    fn screen_rect(&self) -> Rect {
        *self.screen_rect.lock()
    }

    fn ensure_runtime_pointer_geometry(&self) {
        let screen_rect = self.screen_rect();
        let expected = (screen_rect.width as i32, screen_rect.height as i32);
        let current = crate::drivers::mouse::get_bounds();
        if current == expected {
            return;
        }

        sync_output_geometry(screen_rect.width, screen_rect.height);
        let mut cursor = self.cursor_position.lock();
        cursor.x = cursor.x.clamp(0, screen_rect.right().saturating_sub(1));
        cursor.y = cursor.y.clamp(0, screen_rect.bottom().saturating_sub(1));
        self.damage.lock().mark_rect(screen_rect);
    }

    fn record_composition_diagnostic(
        &self,
        code: CompositionDiagnosticCode,
        window_id: WindowId,
        surface_id: SurfaceId,
    ) {
        self.diagnostics.lock().record(code, window_id, surface_id);
        crate::serial_println!(
            "[ECHDISPLAY][DIAG] code={} window={} surface={}",
            code.label(),
            window_id,
            surface_id
        );
    }

    fn is_debug_diagnostics_enabled(&self) -> bool {
        cfg!(debug_assertions)
    }

    fn snapshot_join(&self, presentable_only: bool) -> Vec<ComposedWindowSnapshot> {
        let windows = self.windows.lock().ordered_windows();
        let surfaces = self.surfaces.lock();
        let mut epochs = self.joined_damage_epochs.lock();
        let mut seen_surfaces = BTreeSet::new();
        let mut snapshots = Vec::new();

        for window in windows.into_iter() {
            if presentable_only && (!window.visible || window.minimized) {
                continue;
            }

            let Some(surface) = surfaces.snapshot(window.surface_id) else {
                self.record_composition_diagnostic(
                    CompositionDiagnosticCode::UnresolvedSurface,
                    window.id,
                    window.surface_id,
                );
                continue;
            };

            if !seen_surfaces.insert(surface.id) {
                self.record_composition_diagnostic(
                    CompositionDiagnosticCode::DuplicateSurfaceResolution,
                    window.id,
                    surface.id,
                );
                continue;
            }

            let buffer_mode = if surface.scene_update.is_some() {
                WindowBufferMode::Scene
            } else {
                WindowBufferMode::Pixels
            };
            let scene_root = surface.scene_update.as_ref().map(|scene| scene.root_id);
            let semantic_root = surface
                .scene_update
                .as_ref()
                .and_then(|scene| scene.semantic_root);

            if matches!(buffer_mode, WindowBufferMode::Pixels)
                && (scene_root.is_some() || semantic_root.is_some())
            {
                self.record_composition_diagnostic(
                    CompositionDiagnosticCode::BufferModeMismatch,
                    window.id,
                    surface.id,
                );
            }

            if matches!(buffer_mode, WindowBufferMode::Scene) && scene_root.is_none() {
                self.record_composition_diagnostic(
                    CompositionDiagnosticCode::InconsistentSceneMetadata,
                    window.id,
                    surface.id,
                );
                continue;
            }

            if matches!(buffer_mode, WindowBufferMode::Pixels) {
                let expected_len = surface.rect.width as usize * surface.rect.height as usize;
                let dimensions_match = surface
                    .shared
                    .as_ref()
                    .map(|shared| shared.dimensions() == (surface.rect.width, surface.rect.height))
                    .unwrap_or(true);
                let storage_valid = if surface.shared.is_some() {
                    dimensions_match
                } else {
                    surface.pixels.len() == expected_len
                };
                if !storage_valid {
                    self.record_composition_diagnostic(
                        CompositionDiagnosticCode::StaleSharedSurface,
                        window.id,
                        surface.id,
                    );
                    continue;
                }
            }

            if let Some(previous_epoch) = epochs.get(&surface.id).copied() {
                if surface.damage_epoch < previous_epoch {
                    self.record_composition_diagnostic(
                        CompositionDiagnosticCode::StaleSharedSurface,
                        window.id,
                        surface.id,
                    );
                    continue;
                }
            }
            epochs.insert(surface.id, surface.damage_epoch);

            let shared_mapped = surface.shared.is_some();
            let content = match surface.scene_update {
                Some(scene) => SurfaceContent::Scene(scene),
                None => match surface.shared {
                    Some(shared) => SurfaceContent::PixelsShared(shared),
                    None => SurfaceContent::PixelsOwned(surface.pixels),
                },
            };

            snapshots.push(ComposedWindowSnapshot {
                id: window.id,
                app_id: window.app_id,
                surface_id: window.surface_id,
                title: window.title,
                frame_rect: window.frame_rect,
                content_rect: window.content_rect,
                visible: window.visible,
                focused: window.focused,
                minimized: window.minimized,
                maximized: window.maximized,
                z_index: window.z_index,
                workspace_id: window.workspace_id,
                layer_role: window.layer_role,
                flags: window.flags,
                scene_node_id: window.scene_node_id,
                scene_root,
                semantic_root,
                buffer_mode,
                shared_mapped,
                gpu_buffer_handle: surface.gpu_buffer_handle,
                damage_epoch: surface.damage_epoch,
                fence_id: surface.fence_id,
                content,
            });
        }

        snapshots
    }

    fn ordering(&self, presentable_only: bool) -> Vec<ComposedWindowSnapshot> {
        let mut snapshots = self.snapshot_join(presentable_only);
        snapshots.sort_by_key(|window| (layer_rank(window.layer_role), window.z_index, window.id));
        snapshots
    }

    fn damage_translate(&self, content_rect: Rect, local_damage: &[Rect]) -> Vec<Rect> {
        local_damage
            .iter()
            .copied()
            .filter_map(|rect| {
                let translated = Rect::new(
                    content_rect.x.saturating_add(rect.x),
                    content_rect.y.saturating_add(rect.y),
                    rect.width.min(content_rect.width),
                    rect.height.min(content_rect.height),
                );
                (!translated.is_empty()).then_some(translated)
            })
            .collect()
    }

    fn present_plan(&self, _screen_rect: Rect) -> PresentPlan {
        let snapshots = self.ordering(true);
        let profile = self.display_profile.lock().clone();
        self.refresh_runtime_display_state(&profile, &snapshots);
        let placements = snapshots
            .iter()
            .map(ComposedWindowSnapshot::placement)
            .collect();
        let diagnostics_overlay = if self.is_debug_diagnostics_enabled() {
            let mut lines = self.diagnostics.lock().overlay_lines();
            let primary = profile
                .outputs
                .iter()
                .find(|output| output.output_id == profile.primary_output)
                .or_else(|| profile.outputs.first());
            if let Some(primary) = primary {
                lines.push(format!(
                    "display={} scale={} text={} vrr={} hdr={} activity={}",
                    primary.output_id,
                    primary.scale_100x,
                    primary.text_scale_100x,
                    match *self.effective_vrr_policy.lock() {
                        VrrPolicy::Off => "off",
                        VrrPolicy::On => "on",
                        VrrPolicy::Auto => "auto",
                    },
                    match *self.effective_hdr_policy.lock() {
                        HdrPolicy::Off => "off",
                        HdrPolicy::On => "on",
                        HdrPolicy::Auto => "auto",
                    },
                    self.runtime_activity.lock().label(),
                ));
            }
            for output in self.effective_outputs.lock().iter() {
                lines.push(format!(
                    "out{} {}%/{}% {}Hz {:?}->{:?} {:?}->{:?}{} {:?}",
                    output.output_id,
                    output.scale_100x,
                    output.text_scale_100x,
                    output.refresh_hz,
                    output.requested_vrr,
                    output.effective_vrr,
                    output.requested_hdr,
                    output.effective_hdr,
                    if output.mirrored { " mirror" } else { "" },
                    output.transform,
                ));
            }
            lines
        } else {
            Vec::new()
        };
        PresentPlan {
            theme_mode: *self.theme_mode.lock(),
            native_scanout_available: crate::drivers::gpu_native::device_count() > 0,
            cursor: *self.cursor_position.lock(),
            placements,
            diagnostics_overlay,
            snapshots,
            show_desktop_dashboard: false,
        }
    }

    fn supported_output_modes(&self) -> Vec<OutputMode> {
        const CANDIDATES: &[(u32, u32)] = &[
            (3840, 2160),
            (2560, 1440),
            (1920, 1080),
            (1920, 1020),
            (1680, 1050),
            (1600, 900),
            (1440, 900),
            (1366, 768),
            (1280, 800),
            (1280, 720),
            (1024, 768),
            (800, 600),
        ];

        let physical = self.physical_output_mode;
        let mut modes = Vec::new();
        for (width, height) in CANDIDATES.iter().copied() {
            if width <= physical.width && height <= physical.height {
                let mode = OutputMode::new(width, height, physical.refresh_hz);
                if !modes.contains(&mode) {
                    modes.push(mode);
                }
            }
        }
        if !modes.contains(&physical) {
            modes.push(physical);
        }
        modes.sort_by(|lhs, rhs| {
            rhs.width
                .cmp(&lhs.width)
                .then(rhs.height.cmp(&lhs.height))
                .then(rhs.refresh_hz.cmp(&lhs.refresh_hz))
        });
        modes
    }

    fn detect_display_capability(&self) -> DisplayCapability {
        let supported_modes = self.supported_output_modes();
        let mut capability = DisplayCapability {
            max_refresh_hz: supported_modes
                .iter()
                .map(|mode| mode.refresh_hz)
                .max()
                .unwrap_or(self.physical_output_mode.refresh_hz),
            supported_modes: supported_modes.clone(),
            direct_scanout: crate::drivers::gpu_native::device_count() > 0,
            ..DisplayCapability::default()
        };

        if let Some(device) = crate::drivers::drm::DRM_MANAGER.first_device() {
            let connected_outputs = device.connected_connector_count().max(1);
            capability.connected_outputs = connected_outputs.min(u8::MAX as usize) as u8;
            capability.multi_monitor = connected_outputs > 1;
            capability.mirror = connected_outputs > 1;
            capability.rotation = device.connected_connector_count() > 0;
            capability.adaptive_sync = true;
            capability.max_refresh_hz = capability.max_refresh_hz.max(device.max_mode_refresh_hz());
            capability.direct_scanout = capability.direct_scanout
                || !device
                    .plane_ids_by_type(crate::drivers::drm::DrmPlaneType::Primary)
                    .is_empty();
            capability.hdr_output = false;
            capability.hdr_metadata = false;
            capability.ten_bit_scanout = false;
            capability.icc_profile = false;
            capability.color_transform = false;
        }

        capability
    }

    fn current_shell_state(&self) -> Option<crate::gui::protocol::ShellState> {
        match get_shell_service().process_command(ShellCommand::GetSessionSnapshot) {
            ShellResponse::SessionSnapshot(snapshot) => Some(snapshot.shell_state),
            _ => None,
        }
    }

    fn output_rects_for_profile(
        &self,
        screen_rect: Rect,
        profile: &DisplayProfile,
    ) -> Vec<(u32, Rect)> {
        if profile.outputs.is_empty() {
            return vec![(0, screen_rect)];
        }
        let count = profile.outputs.len() as u32;
        let base_width = (screen_rect.width / count).max(1);
        let mut x = screen_rect.x;
        let mut rects = Vec::with_capacity(profile.outputs.len());
        for (index, output) in profile.outputs.iter().enumerate() {
            let remaining = screen_rect.right().saturating_sub(x);
            let width = if index + 1 == profile.outputs.len() {
                remaining.max(1) as u32
            } else {
                base_width.min(remaining.max(1) as u32)
            };
            rects.push((
                output.output_id,
                Rect::new(x, screen_rect.y, width, screen_rect.height),
            ));
            x = x.saturating_add(width as i32);
        }
        rects
    }

    fn output_rect_for_policy(
        &self,
        screen_rect: Rect,
        profile: &DisplayProfile,
        output_id: u32,
    ) -> Rect {
        self.output_rects_for_profile(screen_rect, profile)
            .into_iter()
            .find(|(candidate, _)| *candidate == output_id)
            .map(|(_, rect)| rect)
            .unwrap_or(screen_rect)
    }

    fn output_for_workspace(profile: &DisplayProfile, workspace_id: WorkspaceId) -> u32 {
        profile
            .outputs
            .iter()
            .find(|output| output.workspace_binding == Some(workspace_id))
            .map(|output| output.output_id)
            .unwrap_or(profile.primary_output)
    }

    fn frame_needs_output_rescue(frame: Rect, target_output: Rect) -> bool {
        if target_output.width == 0 || target_output.height == 0 {
            return false;
        }
        let intersection = frame.intersection(&target_output);
        let visible_area = intersection
            .map(|rect| rect.width.saturating_mul(rect.height))
            .unwrap_or(0);
        let frame_area = frame.width.saturating_mul(frame.height).max(1);
        visible_area.saturating_mul(100) < frame_area.saturating_mul(60)
    }

    fn reanchor_frame_to_output(frame: Rect, target_output: Rect) -> Rect {
        let width = frame.width.min(target_output.width.max(1));
        let height = frame.height.min(target_output.height.max(1));
        let max_x = target_output
            .right()
            .saturating_sub(width as i32)
            .max(target_output.x);
        let max_y = target_output
            .bottom()
            .saturating_sub(height as i32)
            .max(target_output.y);
        Rect::new(
            frame.x.clamp(target_output.x, max_x),
            frame.y.clamp(target_output.y, max_y),
            width,
            height,
        )
    }

    fn effective_display_activity(
        &self,
        profile: &DisplayProfile,
        snapshots: &[ComposedWindowSnapshot],
    ) -> DisplayRuntimeActivity {
        if snapshots.iter().any(|snapshot| {
            if !snapshot.visible || snapshot.minimized || snapshot.layer_role != LayerRole::Window {
                return false;
            }
            let output_id = Self::output_for_workspace(profile, snapshot.workspace_id);
            let output_rect = self.output_rect_for_policy(self.screen_rect(), profile, output_id);
            let covered = snapshot
                .frame_rect
                .intersection(&output_rect)
                .map(|rect| rect.width.saturating_mul(rect.height))
                .unwrap_or(0);
            let output_area = output_rect.width.saturating_mul(output_rect.height).max(1);
            covered.saturating_mul(100) >= output_area.saturating_mul(90)
        }) {
            return DisplayRuntimeActivity::Fullscreen;
        }
        if self.interaction.lock().is_some() || self.pointer_capture.lock().is_some() {
            return DisplayRuntimeActivity::Interactive;
        }
        match self.current_shell_state() {
            Some(crate::gui::protocol::ShellState::OverlayInteractive)
            | Some(crate::gui::protocol::ShellState::WorkspaceTransition) => {
                DisplayRuntimeActivity::Animation
            }
            _ => DisplayRuntimeActivity::Idle,
        }
    }

    fn effective_vrr_for_activity(
        &self,
        capability: &DisplayCapability,
        output: &MonitorPolicy,
        activity: DisplayRuntimeActivity,
    ) -> VrrPolicy {
        if !capability.adaptive_sync {
            return VrrPolicy::Off;
        }
        match output.vrr_policy {
            VrrPolicy::Off => VrrPolicy::Off,
            VrrPolicy::On => VrrPolicy::On,
            VrrPolicy::Auto => match activity {
                DisplayRuntimeActivity::Idle => VrrPolicy::Off,
                DisplayRuntimeActivity::Interactive
                | DisplayRuntimeActivity::Animation
                | DisplayRuntimeActivity::Fullscreen => VrrPolicy::On,
            },
        }
    }

    fn effective_hdr_for_activity(
        &self,
        capability: &DisplayCapability,
        output: &MonitorPolicy,
        activity: DisplayRuntimeActivity,
    ) -> HdrPolicy {
        if !capability.hdr_output || !capability.hdr_metadata || !capability.ten_bit_scanout {
            return HdrPolicy::Off;
        }
        match output.hdr_policy {
            HdrPolicy::Off => HdrPolicy::Off,
            HdrPolicy::On => HdrPolicy::On,
            HdrPolicy::Auto => match activity {
                DisplayRuntimeActivity::Fullscreen => HdrPolicy::On,
                DisplayRuntimeActivity::Idle
                | DisplayRuntimeActivity::Interactive
                | DisplayRuntimeActivity::Animation => HdrPolicy::Off,
            },
        }
    }

    fn refresh_runtime_display_state(
        &self,
        profile: &DisplayProfile,
        snapshots: &[ComposedWindowSnapshot],
    ) {
        let activity = self.effective_display_activity(profile, snapshots);
        let effective_outputs: Vec<EffectiveMonitorRuntimeState> = profile
            .outputs
            .iter()
            .map(|output| EffectiveMonitorRuntimeState {
                output_id: output.output_id,
                scale_100x: output.scale_100x,
                text_scale_100x: output.text_scale_100x,
                requested_vrr: output.vrr_policy,
                effective_vrr: self.effective_vrr_for_activity(
                    &profile.capability,
                    output,
                    activity,
                ),
                requested_hdr: output.hdr_policy,
                effective_hdr: self.effective_hdr_for_activity(
                    &profile.capability,
                    output,
                    activity,
                ),
                refresh_hz: output.refresh_hz,
                mirrored: output.mirror_target.is_some(),
                transform: output.transform,
            })
            .collect();
        *self.runtime_activity.lock() = activity;
        *self.effective_outputs.lock() = effective_outputs.clone();
        if let Some(primary) = effective_outputs
            .iter()
            .find(|output| output.output_id == profile.primary_output)
            .or_else(|| effective_outputs.first())
        {
            crate::gfx::scaling::set_scale_factor(primary.scale_100x as u32);
            *self.effective_vrr_policy.lock() = primary.effective_vrr;
            *self.effective_hdr_policy.lock() = primary.effective_hdr;
            self.atomic_presenter
                .lock()
                .set_mode(match primary.effective_vrr {
                    VrrPolicy::Off => DisplayPresentMode::VblankFifo,
                    VrrPolicy::On | VrrPolicy::Auto => DisplayPresentMode::AdaptiveSync,
                });
        }
    }

    fn apply_runtime_display_profile(&self, profile: &DisplayProfile) {
        self.refresh_runtime_display_state(profile, &[]);
    }

    fn harmonize_mirror_workspace_bindings(profile: &mut DisplayProfile) {
        let workspace_bindings: BTreeMap<u32, WorkspaceId> = profile
            .outputs
            .iter()
            .filter_map(|output| {
                output
                    .workspace_binding
                    .map(|binding| (output.output_id, binding))
            })
            .collect();
        for output in profile.outputs.iter_mut() {
            if let Some(target) = output.mirror_target {
                output.workspace_binding = workspace_bindings
                    .get(&target)
                    .copied()
                    .or(output.workspace_binding);
            }
        }
    }

    fn sanitize_display_profile(&self, mut profile: DisplayProfile) -> DisplayProfile {
        let capability = self.detect_display_capability();
        let current_mode = *self.effective_output_mode.lock();

        if profile.outputs.is_empty() {
            profile.outputs = vec![MonitorPolicy::single_output(current_mode)];
        } else if !capability.multi_monitor {
            let mut primary = profile.outputs.remove(0);
            primary.output_id = 0;
            primary.mirror_target = None;
            primary.workspace_binding = Some(primary.workspace_binding.unwrap_or(0));
            profile.primary_output = 0;
            profile.outputs = vec![primary];
        } else {
            let mut seen = BTreeSet::new();
            profile
                .outputs
                .retain(|output| seen.insert(output.output_id));
            let expected_outputs = capability.connected_outputs.max(1) as u32;
            for output_id in 0..expected_outputs {
                if !profile
                    .outputs
                    .iter()
                    .any(|output| output.output_id == output_id)
                {
                    let mut output = MonitorPolicy::single_output(current_mode);
                    output.output_id = output_id;
                    output.workspace_binding = Some(output_id as WorkspaceId);
                    profile.outputs.push(output);
                }
            }
            profile.outputs.sort_by_key(|output| output.output_id);
        }

        let valid_output_ids: BTreeSet<u32> = profile
            .outputs
            .iter()
            .map(|output| output.output_id)
            .collect();
        for output in profile.outputs.iter_mut() {
            if !capability.fractional_scaling {
                output.scale_100x = 100;
            }
            if !capability.adaptive_sync {
                output.vrr_policy = VrrPolicy::Off;
            }
            if !capability.hdr_output || !capability.hdr_metadata {
                output.hdr_policy = HdrPolicy::Off;
            }
            if !capability.rotation {
                output.transform = SurfaceTransform::Identity;
            }
            if !capability.multi_monitor || output.mirror_target == Some(output.output_id) {
                output.mirror_target = None;
            }
            if output
                .mirror_target
                .map(|target| !valid_output_ids.contains(&target))
                .unwrap_or(false)
            {
                output.mirror_target = None;
            }

            output.refresh_hz = output
                .refresh_hz
                .clamp(1, capability.max_refresh_hz.max(current_mode.refresh_hz));
            output.scale_100x = capability
                .supported_scales_100x
                .iter()
                .copied()
                .min_by_key(|scale| scale.abs_diff(output.scale_100x))
                .unwrap_or(100);
            output.text_scale_100x = output.text_scale_100x.clamp(75, 300);
            if output.workspace_binding.is_none() {
                output.workspace_binding = Some(output.output_id as WorkspaceId);
            }
        }
        Self::harmonize_mirror_workspace_bindings(&mut profile);

        profile.capability = capability;
        if !profile
            .outputs
            .iter()
            .any(|output| output.output_id == profile.primary_output)
        {
            profile.primary_output = profile.outputs[0].output_id;
        }
        profile
    }

    fn apply_monitor_policy_to_windows(&self, previous: &DisplayProfile, current: &DisplayProfile) {
        let screen_rect = self.screen_rect();
        let primary_workspace = current
            .outputs
            .iter()
            .find(|output| output.output_id == current.primary_output)
            .and_then(|output| output.workspace_binding)
            .unwrap_or(0);
        let current_workspaces: BTreeSet<WorkspaceId> = current
            .outputs
            .iter()
            .filter_map(|output| output.workspace_binding)
            .collect();
        let previous_workspaces: BTreeSet<WorkspaceId> = previous
            .outputs
            .iter()
            .filter_map(|output| output.workspace_binding)
            .collect();
        let windows = self.windows.lock().ordered_windows();
        for window in windows {
            let mut next_workspace = window.workspace_id;
            if !current_workspaces.contains(&window.workspace_id) {
                next_workspace = primary_workspace;
            }
            let old_output = Self::output_for_workspace(previous, window.workspace_id);
            let new_output = Self::output_for_workspace(current, next_workspace);
            let target_output_rect = self.output_rect_for_policy(screen_rect, current, new_output);
            let binding_changed = next_workspace != window.workspace_id;
            let output_changed = old_output != new_output
                || (!previous_workspaces.contains(&window.workspace_id)
                    && current_workspaces.contains(&next_workspace));
            if binding_changed {
                if let Ok((old_frame, new_frame)) = self.windows.lock().set_window_meta(
                    window.id,
                    next_workspace,
                    window.layer_role,
                    window.flags,
                ) {
                    self.damage.lock().mark_rects(&[old_frame, new_frame]);
                }
            }
            if output_changed
                || Self::frame_needs_output_rescue(window.frame_rect, target_output_rect)
            {
                let frame = Self::reanchor_frame_to_output(window.frame_rect, target_output_rect);
                let _ = self.update_window_frame(
                    window.id,
                    frame.x,
                    frame.y,
                    window.content_rect.width,
                    window.content_rect.height,
                );
            }
        }
    }

    fn sync_shell_display_profile(&self, profile: DisplayProfile) {
        let _ =
            get_shell_service().process_command(ShellCommand::SetDisplayProfileState { profile });
    }

    fn query_display_capability(&self) -> DisplayResponse {
        DisplayResponse::DisplayCapability(self.detect_display_capability())
    }

    fn query_display_profile(&self) -> DisplayResponse {
        let profile = self.sanitize_display_profile(self.display_profile.lock().clone());
        *self.display_profile.lock() = profile.clone();
        self.apply_runtime_display_profile(&profile);
        self.sync_shell_display_profile(profile.clone());
        DisplayResponse::DisplayProfile(profile)
    }

    fn set_display_profile(&self, profile: DisplayProfile) -> DisplayResponse {
        let previous = self.display_profile.lock().clone();
        let profile = self.sanitize_display_profile(profile);
        *self.display_profile.lock() = profile.clone();
        self.apply_monitor_policy_to_windows(&previous, &profile);
        self.apply_runtime_display_profile(&profile);
        self.sync_shell_display_profile(profile.clone());
        DisplayResponse::DisplayProfile(profile)
    }

    pub fn send_command(&self, command: DisplayCommand) -> bool {
        self.command_queue.try_push(command).is_ok()
    }

    pub fn receive_response(&self) -> Option<DisplayResponse> {
        self.response_queue.pop()
    }

    pub fn shared_surface_for_window(
        &self,
        window_id: WindowId,
    ) -> Option<(SurfaceId, Arc<SharedSurfaceMemory>)> {
        let surface_id = {
            let windows = self.windows.lock();
            windows.window_surface(window_id)?
        };
        let surface = {
            let surfaces = self.surfaces.lock();
            surfaces.shared_surface(surface_id)?
        };
        Some((surface_id, surface))
    }

    pub fn process_command(&self, command: DisplayCommand) -> DisplayResponse {
        match command {
            DisplayCommand::CreateWindow {
                app_id,
                title,
                x,
                y,
                width,
                height,
            } => self.create_window(
                app_id,
                &title,
                x,
                y,
                width,
                height,
                0,
                LayerRole::Window,
                WindowFlags::default(),
            ),
            DisplayCommand::CreateWindowWithMeta {
                app_id,
                title,
                x,
                y,
                width,
                height,
                workspace_id,
                layer_role,
                flags,
            } => self.create_window(
                app_id,
                &title,
                x,
                y,
                width,
                height,
                workspace_id,
                layer_role,
                flags,
            ),
            DisplayCommand::DestroyWindow { window_id } => self.destroy_window(window_id),
            DisplayCommand::MoveWindow { window_id, x, y } => self.move_window(window_id, x, y),
            DisplayCommand::ResizeWindow {
                window_id,
                width,
                height,
            } => self.resize_window(window_id, width, height),
            DisplayCommand::FocusWindow { window_id } => self.focus_window(window_id),
            DisplayCommand::SetWindowVisibility { window_id, visible } => {
                self.set_window_visibility(window_id, visible)
            }
            DisplayCommand::SetWindowTitle { window_id, title } => {
                self.set_window_title(window_id, &title)
            }
            DisplayCommand::SetWindowMeta {
                window_id,
                workspace_id,
                layer_role,
                flags,
            } => self.set_window_meta(window_id, workspace_id, layer_role, flags),
            DisplayCommand::MoveWindowToWorkspace {
                window_id,
                workspace_id,
            } => self.move_window_to_workspace(window_id, workspace_id),
            DisplayCommand::CommitWindowBuffer { window_id, pixels } => {
                self.commit_window_buffer(window_id, &pixels)
            }
            DisplayCommand::CommitScene { window_id, scene } => {
                self.commit_window_scene(window_id, scene)
            }
            DisplayCommand::MapWindowSurface { window_id } => self.map_window_surface(window_id),
            DisplayCommand::SubmitWindowDamage { window_id, packet } => {
                self.submit_window_damage(window_id, packet)
            }
            DisplayCommand::SetPresentMode { mode } => self.set_present_mode(mode),
            DisplayCommand::SetOutputMode { mode } => self.set_output_mode(mode),
            DisplayCommand::SetDisplayProfile { profile } => self.set_display_profile(profile),
            DisplayCommand::SetThemeMode { mode } => self.set_theme_mode(mode),
            DisplayCommand::SubmitFrameIntent { intent } => self.submit_frame_intent(intent),
            DisplayCommand::RouteInputEvent { event } => {
                DisplayResponse::InputRoute(self.dispatch_input_event(&event))
            }
            DisplayCommand::QueryOutputMode => self.query_output_mode(),
            DisplayCommand::ListOutputModes => self.list_output_modes(),
            DisplayCommand::QueryDisplayCapability => self.query_display_capability(),
            DisplayCommand::QueryDisplayProfile => self.query_display_profile(),
            DisplayCommand::QueryPresentMetrics => self.query_present_metrics(),
            DisplayCommand::ListWindows => {
                let windows = self.list_windows_with_buffers();
                DisplayResponse::WindowList { windows }
            }
            DisplayCommand::ListSurfaces => {
                let surfaces = self.surfaces.lock().list_surfaces();
                DisplayResponse::SurfaceList { surfaces }
            }
            DisplayCommand::SnapshotDesktop => self.snapshot_desktop(),
            DisplayCommand::Present => self.present(),
        }
    }

    pub fn dispatch_input_event(&self, event: &InputEvent) -> InputRouting {
        self.ensure_runtime_pointer_geometry();
        match event {
            InputEvent::Key { .. } => self
                .focused_window_route(None, false)
                .unwrap_or(InputRouting::None),
            InputEvent::PointerMove { position, .. } => {
                self.update_cursor(*position);
                if self.update_pointer_interaction(*position) {
                    InputRouting::None
                } else if self.update_pointer_capture(*position) {
                    self.captured_window_route(Some(*position), true)
                        .unwrap_or(InputRouting::None)
                } else {
                    self.route_pointer_motion(*position)
                }
            }
            InputEvent::PointerButton {
                button: PointerButton::Left,
                state: KeyState::Pressed,
                position,
            } => {
                self.update_cursor(*position);
                self.begin_pointer_interaction(*position)
            }
            InputEvent::PointerButton {
                button: PointerButton::Left,
                state: KeyState::Released,
                position,
            } => {
                self.update_cursor(*position);
                let swallow_release = {
                    let mut swallow = self.swallow_left_release.lock();
                    let current = *swallow;
                    *swallow = false;
                    current
                };
                let capture_route = self.end_pointer_capture(*position);
                if self.end_pointer_interaction() || swallow_release {
                    InputRouting::None
                } else if let Some(route) = capture_route {
                    route
                } else {
                    self.route_pointer_target(*position, false)
                        .unwrap_or(InputRouting::None)
                }
            }
            InputEvent::PointerButton {
                state: KeyState::Pressed,
                position,
                ..
            } => {
                self.update_cursor(*position);
                self.focus_hovered_window(*position)
            }
            InputEvent::PointerButton { position, .. } => {
                self.update_cursor(*position);
                self.route_pointer_target(*position, false)
                    .unwrap_or(InputRouting::None)
            }
            InputEvent::Scroll { position, .. } => {
                self.update_cursor(*position);
                self.route_pointer_target(*position, false)
                    .or_else(|| self.focused_window_route(None, false))
                    .unwrap_or(InputRouting::None)
            }
        }
    }

    pub fn focused_app(&self) -> Option<AppId> {
        let windows = self.windows.lock();
        windows
            .focused_window()
            .and_then(|window_id| windows.window_app(window_id))
    }

    pub fn focused_window(&self) -> Option<WindowId> {
        self.windows.lock().focused_window()
    }

    pub fn run_service(&self) {
        while self.running.load(Ordering::SeqCst) {
            self.ensure_runtime_pointer_geometry();
            while let Some(command) = self.command_queue.pop() {
                let response = self.process_command(command);
                let _ = self.response_queue.push_overwrite(response);
            }

            let has_surface_damage = self.surfaces.lock().has_dirty_surface();
            let has_frame_damage = self.damage.lock().has_damage();
            if has_surface_damage || has_frame_damage {
                let _ = self.present();
            } else {
                let _ = self.service_present_queue();
            }

            for _ in 0..1000 {
                core::hint::spin_loop();
            }
        }
    }

    fn create_window(
        &self,
        app_id: AppId,
        title: &str,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        workspace_id: WorkspaceId,
        layer_role: LayerRole,
        flags: WindowFlags,
    ) -> DisplayResponse {
        let surface_id = {
            let mut surfaces = self.surfaces.lock();
            match surfaces.create_surface(app_id, width, height) {
                Ok(surface_id) => surface_id,
                Err(err) => return DisplayResponse::Error(surface_error_message(err)),
            }
        };

        let (window_id, content_rect, frame_rect) = {
            let mut windows = self.windows.lock();
            match windows.create_window_with_meta(
                app_id,
                surface_id,
                title,
                x,
                y,
                width,
                height,
                workspace_id,
                layer_role,
                flags,
            ) {
                Ok(window_id) => {
                    let content_rect = windows.content_rect(window_id).unwrap_or_default();
                    let frame_rect = windows.frame_rect(window_id).unwrap_or_default();
                    (window_id, content_rect, frame_rect)
                }
                Err(err) => {
                    let _ = self.surfaces.lock().destroy_surface(surface_id);
                    return DisplayResponse::Error(window_error_message(err));
                }
            }
        };

        let _ = self.surfaces.lock().set_geometry(
            surface_id,
            content_rect.x,
            content_rect.y,
            content_rect.width,
            content_rect.height,
        );
        self.damage.lock().mark_rect(frame_rect);

        DisplayResponse::WindowCreated {
            window_id,
            surface_id,
            content_rect,
        }
    }

    fn destroy_window(&self, window_id: WindowId) -> DisplayResponse {
        let removed = self.windows.lock().destroy_window(window_id);
        let Some(window) = removed else {
            return DisplayResponse::Error(String::from("window not found"));
        };

        let _ = self.surfaces.lock().destroy_surface(window.surface_id);
        self.damage.lock().mark_rect(window.frame_rect);
        if self.interaction.lock().as_ref().map(|it| it.window_id) == Some(window_id) {
            *self.interaction.lock() = None;
        }
        DisplayResponse::Ack
    }

    fn move_window(&self, window_id: WindowId, x: i32, y: i32) -> DisplayResponse {
        let content = {
            let windows = self.windows.lock();
            let Some(content_rect) = windows.content_rect(window_id) else {
                return DisplayResponse::Error(String::from("window not found"));
            };
            content_rect
        };

        match self.update_window_frame(window_id, x, y, content.width, content.height) {
            Ok(()) => DisplayResponse::Ack,
            Err(err) => DisplayResponse::Error(err),
        }
    }

    fn resize_window(&self, window_id: WindowId, width: u32, height: u32) -> DisplayResponse {
        let frame = {
            let windows = self.windows.lock();
            let Some(frame_rect) = windows.frame_rect(window_id) else {
                return DisplayResponse::Error(String::from("window not found"));
            };
            frame_rect
        };

        match self.update_window_frame(window_id, frame.x, frame.y, width, height) {
            Ok(()) => DisplayResponse::Ack,
            Err(err) => DisplayResponse::Error(err),
        }
    }

    fn focus_window(&self, window_id: WindowId) -> DisplayResponse {
        match self.focus_window_internal(window_id) {
            Ok(_) => DisplayResponse::Ack,
            Err(err) => DisplayResponse::Error(err),
        }
    }

    fn set_window_visibility(&self, window_id: WindowId, visible: bool) -> DisplayResponse {
        let (frame_rect, surface_id) = {
            let mut windows = self.windows.lock();
            match windows.set_visible(window_id, visible) {
                Ok(rect) => (rect, windows.window_surface(window_id).unwrap_or(0)),
                Err(err) => return DisplayResponse::Error(window_error_message(err)),
            }
        };

        let _ = self.surfaces.lock().set_visible(surface_id, visible);
        self.damage.lock().mark_rect(frame_rect);
        DisplayResponse::Ack
    }

    fn set_window_title(&self, window_id: WindowId, title: &str) -> DisplayResponse {
        match self.windows.lock().set_title(window_id, title) {
            Ok(frame_rect) => {
                self.damage.lock().mark_rect(frame_rect);
                DisplayResponse::Ack
            }
            Err(err) => DisplayResponse::Error(window_error_message(err)),
        }
    }

    fn set_window_meta(
        &self,
        window_id: WindowId,
        workspace_id: WorkspaceId,
        layer_role: LayerRole,
        flags: WindowFlags,
    ) -> DisplayResponse {
        match self
            .windows
            .lock()
            .set_window_meta(window_id, workspace_id, layer_role, flags)
        {
            Ok((old_frame, new_frame)) => {
                self.damage.lock().mark_rects(&[old_frame, new_frame]);
                DisplayResponse::Ack
            }
            Err(err) => DisplayResponse::Error(window_error_message(err)),
        }
    }

    fn move_window_to_workspace(
        &self,
        window_id: WindowId,
        workspace_id: WorkspaceId,
    ) -> DisplayResponse {
        match self
            .windows
            .lock()
            .set_window_workspace(window_id, workspace_id)
        {
            Ok(()) => DisplayResponse::Ack,
            Err(err) => DisplayResponse::Error(window_error_message(err)),
        }
    }

    fn commit_window_buffer(&self, window_id: WindowId, pixels: &[u32]) -> DisplayResponse {
        let (surface_id, content_rect) = {
            let windows = self.windows.lock();
            let Some(surface_id) = windows.window_surface(window_id) else {
                return DisplayResponse::Error(String::from("window not found"));
            };
            let content_rect = windows.content_rect(window_id).unwrap_or_default();
            (surface_id, content_rect)
        };

        match self.surfaces.lock().commit_buffer(surface_id, pixels) {
            Ok(()) => {
                self.damage.lock().mark_rect(content_rect);
                DisplayResponse::Ack
            }
            Err(err) => DisplayResponse::Error(surface_error_message(err)),
        }
    }

    fn commit_window_scene(&self, window_id: WindowId, scene: SceneUpdate) -> DisplayResponse {
        let (surface_id, content_rect) = {
            let windows = self.windows.lock();
            let Some(surface_id) = windows.window_surface(window_id) else {
                return DisplayResponse::Error(String::from("window not found"));
            };
            let content_rect = windows.content_rect(window_id).unwrap_or_default();
            (surface_id, content_rect)
        };

        match self.surfaces.lock().commit_scene(surface_id, scene) {
            Ok(scene_damage) => {
                if scene_damage.is_empty() {
                    self.damage.lock().mark_rect(content_rect);
                } else {
                    let global_damage = self.damage_translate(content_rect, &scene_damage);
                    if global_damage.is_empty() {
                        self.damage.lock().mark_rect(content_rect);
                    } else {
                        self.damage.lock().mark_rects(&global_damage);
                    }
                }
                DisplayResponse::Ack
            }
            Err(err) => DisplayResponse::Error(surface_error_message(err)),
        }
    }

    fn map_window_surface(&self, window_id: WindowId) -> DisplayResponse {
        let surface_id = {
            let windows = self.windows.lock();
            let Some(surface_id) = windows.window_surface(window_id) else {
                return DisplayResponse::Error(String::from("window not found"));
            };
            surface_id
        };

        match self.surfaces.lock().map_shared_surface(surface_id) {
            Ok(descriptor) => DisplayResponse::SurfaceMapped(descriptor),
            Err(err) => DisplayResponse::Error(surface_error_message(err)),
        }
    }

    fn list_windows_with_buffers(&self) -> Vec<WindowInfo> {
        self.ordering(false)
            .into_iter()
            .map(|snapshot| snapshot.window_info())
            .collect()
    }

    fn submit_window_damage(&self, window_id: WindowId, packet: DamagePacket) -> DisplayResponse {
        let (surface_id, content_rect) = {
            let windows = self.windows.lock();
            let Some(surface_id) = windows.window_surface(window_id) else {
                return DisplayResponse::Error(String::from("window not found"));
            };
            let content_rect = windows.content_rect(window_id).unwrap_or_default();
            (surface_id, content_rect)
        };
        if packet.surface_id != surface_id {
            return DisplayResponse::Error(String::from("surface mismatch"));
        }

        match self
            .surfaces
            .lock()
            .submit_shared_damage(surface_id, packet.rect, packet.generation)
        {
            Ok(()) => {
                let global_damage = Rect::new(
                    content_rect.x.saturating_add(packet.rect.x),
                    content_rect.y.saturating_add(packet.rect.y),
                    packet.rect.width.min(content_rect.width),
                    packet.rect.height.min(content_rect.height),
                );
                self.damage.lock().mark_rect(global_damage);
                DisplayResponse::Ack
            }
            Err(err) => DisplayResponse::Error(surface_error_message(err)),
        }
    }

    fn present(&self) -> DisplayResponse {
        self.ensure_runtime_pointer_geometry();
        let screen_rect = self.screen_rect();
        let damage_regions = self.damage.lock().take(screen_rect);
        if damage_regions.is_empty() {
            self.surfaces.lock().clear_dirty();
            return self.service_present_queue();
        }
        let plan = self.present_plan(screen_rect);

        if plan.native_scanout_available {
            {
                let mut presenter = self.atomic_presenter.lock();
                let intent = presenter.build_intent(
                    screen_rect,
                    &damage_regions,
                    &plan.placements,
                    plan.cursor,
                );
                presenter.enqueue(intent);
            }

            if let DisplayResponse::Presented {
                feedback,
                assignment,
            } = self.service_present_queue()
            {
                self.surfaces.lock().clear_dirty();
                return DisplayResponse::Presented {
                    feedback,
                    assignment,
                };
            }

            if self.atomic_presenter.lock().has_pending_intent() {
                self.surfaces.lock().clear_dirty();
                return DisplayResponse::Ack;
            }
        }

        {
            let mut fb = self.framebuffer.lock();
            let mut cpu_renderer = CpuRenderer::new();
            let mut gpu_renderer = GpuRenderer::new(screen_rect.width, screen_rect.height);
            let mut title_text_system = TextSystem::new();
            for damage in damage_regions.iter() {
                shell::draw_desktop_scene(
                    &mut fb,
                    screen_rect,
                    *damage,
                    plan.theme_mode,
                    plan.show_desktop_dashboard,
                );
                for snapshot in plan.snapshots.iter() {
                    if snapshot.frame_rect.intersects(damage) {
                        let window = snapshot.window_info();
                        if let SurfaceContent::Scene(scene) = &snapshot.content {
                            match window.layer_role {
                                LayerRole::TopBar
                                | LayerRole::Dock
                                | LayerRole::Overlay
                                | LayerRole::Modal
                                | LayerRole::WorkspaceScratchpad => draw_window_scene(
                                    &mut fb,
                                    &mut gpu_renderer,
                                    &window,
                                    scene,
                                    &mut title_text_system,
                                    *damage,
                                    plan.theme_mode,
                                ),
                                _ => draw_window_scene(
                                    &mut fb,
                                    &mut cpu_renderer,
                                    &window,
                                    scene,
                                    &mut title_text_system,
                                    *damage,
                                    plan.theme_mode,
                                ),
                            }
                        } else if let SurfaceContent::PixelsOwned(pixels) = &snapshot.content {
                            draw_window(
                                &mut fb,
                                &window,
                                pixels,
                                &mut title_text_system,
                                *damage,
                                plan.theme_mode,
                            );
                        } else if let SurfaceContent::PixelsShared(shared) = &snapshot.content {
                            shared.with_pixels(|pixels| {
                                draw_window(
                                    &mut fb,
                                    &window,
                                    pixels,
                                    &mut title_text_system,
                                    *damage,
                                    plan.theme_mode,
                                );
                            });
                        }
                    }
                }
                draw_diagnostics_overlay(
                    &mut fb,
                    &mut title_text_system,
                    *damage,
                    &plan.diagnostics_overlay,
                );
                draw_cursor(&mut fb, plan.cursor, *damage);
            }
            fb.swap_buffers();
        }

        self.surfaces.lock().clear_dirty();
        DisplayResponse::Ack
    }

    pub fn remove_app_windows(&self, app_id: AppId) {
        let removed = self.windows.lock().destroy_windows_for_app(app_id);
        let mut surfaces = self.surfaces.lock();
        let mut damage = self.damage.lock();
        for window in removed {
            let _ = surfaces.destroy_surface(window.surface_id);
            damage.mark_rect(window.frame_rect);
        }
    }

    fn focus_hovered_window(&self, position: Point) -> InputRouting {
        let Some(target) = self.windows.lock().hit_test(position) else {
            return InputRouting::None;
        };

        let window_id = match target {
            WindowHitTarget::Content(window_id)
            | WindowHitTarget::Titlebar(window_id)
            | WindowHitTarget::Resize(window_id, _)
            | WindowHitTarget::Chrome(window_id, _) => window_id,
        };

        match self.focus_window_internal(window_id) {
            Ok(app_id) => match target {
                WindowHitTarget::Content(window_id) => {
                    self.window_route(window_id, Some(position), false)
                }
                _ => InputRouting::FocusOnly(app_id),
            },
            Err(_) => InputRouting::None,
        }
    }

    fn begin_pointer_interaction(&self, position: Point) -> InputRouting {
        let target = { self.windows.lock().hit_test(position) };
        let Some(target) = target else {
            *self.interaction.lock() = None;
            return InputRouting::None;
        };

        let window_id = match target {
            WindowHitTarget::Content(window_id)
            | WindowHitTarget::Titlebar(window_id)
            | WindowHitTarget::Resize(window_id, _)
            | WindowHitTarget::Chrome(window_id, _) => window_id,
        };

        let app_id = match self.focus_window_internal(window_id) {
            Ok(app_id) => app_id,
            Err(_) => return InputRouting::None,
        };

        match target {
            WindowHitTarget::Content(window_id) => {
                *self.pointer_capture.lock() = Some(PointerCapture {
                    window_id,
                    origin: position,
                    threshold_crossed: false,
                });
                self.window_route(window_id, Some(position), false)
            }
            WindowHitTarget::Titlebar(window_id) => {
                let Some(frame_rect) = self.windows.lock().frame_rect(window_id) else {
                    return InputRouting::FocusOnly(app_id);
                };
                *self.interaction.lock() = Some(WindowInteraction {
                    window_id,
                    kind: InteractionKind::Drag {
                        grab_offset: Point::new(
                            position.x.saturating_sub(frame_rect.x),
                            position.y.saturating_sub(frame_rect.y),
                        ),
                        frame_rect,
                    },
                });
                InputRouting::FocusOnly(app_id)
            }
            WindowHitTarget::Resize(window_id, edge) => {
                let Some(start_frame) = self.windows.lock().frame_rect(window_id) else {
                    return InputRouting::FocusOnly(app_id);
                };
                *self.interaction.lock() = Some(WindowInteraction {
                    window_id,
                    kind: InteractionKind::Resize {
                        edge,
                        start_pointer: position,
                        start_frame,
                    },
                });
                InputRouting::FocusOnly(app_id)
            }
            WindowHitTarget::Chrome(window_id, button) => {
                *self.swallow_left_release.lock() = true;
                self.handle_chrome_action(window_id, button, app_id)
            }
        }
    }

    fn set_present_mode(&self, mode: DisplayPresentMode) -> DisplayResponse {
        self.atomic_presenter.lock().set_mode(mode);
        DisplayResponse::Ack
    }

    fn set_output_mode(&self, mode: OutputMode) -> DisplayResponse {
        let supported = self.supported_output_modes();
        if !supported.contains(&mode) {
            return DisplayResponse::Error(String::from(
                "output mode unsupported on current framebuffer",
            ));
        }

        let old_rect = self.screen_rect();
        {
            let mut fb = self.framebuffer.lock();
            let physical = self.physical_output_mode;
            fb.width = physical.width as usize;
            fb.height = physical.height as usize;
            fb.clear(0x000000);
            fb.width = mode.width as usize;
            fb.height = mode.height as usize;
        }

        let new_rect = Rect::new(0, 0, mode.width, mode.height);
        *self.requested_output_mode.lock() = mode;
        *self.effective_output_mode.lock() = mode;
        *self.screen_rect.lock() = new_rect;
        sync_output_geometry(mode.width, mode.height);
        {
            let current = self.display_profile.lock().clone();
            let primary_output = current.primary_output;
            let capability = current.capability.clone();
            let updated_outputs = current
                .outputs
                .iter()
                .cloned()
                .map(|mut output| {
                    if output.output_id == primary_output {
                        output.refresh_hz = mode.refresh_hz;
                    }
                    output
                })
                .collect();
            let updated = self.sanitize_display_profile(DisplayProfile {
                primary_output,
                outputs: updated_outputs,
                capability,
            });
            *self.display_profile.lock() = updated.clone();
            self.apply_monitor_policy_to_windows(&current, &updated);
            self.apply_runtime_display_profile(&updated);
            self.sync_shell_display_profile(updated);
        }
        {
            let mut cursor = self.cursor_position.lock();
            cursor.x = cursor.x.clamp(0, new_rect.right().saturating_sub(1));
            cursor.y = cursor.y.clamp(0, new_rect.bottom().saturating_sub(1));
        }
        self.damage.lock().mark_rects(&[old_rect, new_rect]);

        DisplayResponse::OutputModeState {
            current: self.physical_output_mode,
            requested: mode,
            effective: mode,
        }
    }

    fn set_theme_mode(&self, mode: ThemeMode) -> DisplayResponse {
        *self.theme_mode.lock() = mode;
        self.damage.lock().mark_rect(self.screen_rect());
        DisplayResponse::Ack
    }

    fn submit_frame_intent(&self, intent: FrameIntent) -> DisplayResponse {
        self.atomic_presenter.lock().enqueue(intent);
        self.service_present_queue()
    }

    fn query_present_metrics(&self) -> DisplayResponse {
        let metrics = self.atomic_presenter.lock().metrics_snapshot();
        DisplayResponse::PresentMetrics { metrics }
    }

    fn query_output_mode(&self) -> DisplayResponse {
        DisplayResponse::OutputModeState {
            current: self.physical_output_mode,
            requested: *self.requested_output_mode.lock(),
            effective: *self.effective_output_mode.lock(),
        }
    }

    fn list_output_modes(&self) -> DisplayResponse {
        DisplayResponse::OutputModeCatalog {
            modes: self.supported_output_modes(),
        }
    }

    fn service_present_queue(&self) -> DisplayResponse {
        if crate::drivers::gpu_native::device_count() == 0 {
            return DisplayResponse::Ack;
        }

        let placements = self.collect_surface_placements();
        let mut presenter = self.atomic_presenter.lock();
        let now_ns = crate::cpu::tsc::read_ns();
        match presenter.commit_latest(self.screen_rect(), &placements, now_ns) {
            Ok((intent, assignment, feedback)) => {
                self.last_presented_frame
                    .store(feedback.presented_frame_id, Ordering::Release);
                let mut surfaces = self.surfaces.lock();
                if let Some(surface_id) = assignment.primary {
                    let _ = surfaces.mark_present_fence(surface_id, intent.frame_id);
                }
                for surface_id in assignment.overlays.iter() {
                    let _ = surfaces.mark_present_fence(*surface_id, intent.frame_id);
                }
                surfaces.clear_dirty();
                DisplayResponse::Presented {
                    feedback,
                    assignment,
                }
            }
            Err(_) => DisplayResponse::Ack,
        }
    }

    fn update_pointer_interaction(&self, position: Point) -> bool {
        let interaction = { *self.interaction.lock() };
        let Some(interaction) = interaction else {
            return false;
        };

        match interaction.kind {
            InteractionKind::Drag {
                grab_offset,
                frame_rect,
            } => {
                let x = position.x.saturating_sub(grab_offset.x);
                let y = position.y.saturating_sub(grab_offset.y);
                let _ = self.update_window_frame(
                    interaction.window_id,
                    x,
                    y,
                    frame_rect.width,
                    frame_rect.height.saturating_sub(TITLEBAR_HEIGHT),
                );
                true
            }
            InteractionKind::Resize {
                edge,
                start_pointer,
                start_frame,
            } => {
                let resized = resize_frame(start_frame, edge, start_pointer, position);
                let _ = self.update_window_frame(
                    interaction.window_id,
                    resized.x,
                    resized.y,
                    resized.width,
                    resized.height.saturating_sub(TITLEBAR_HEIGHT),
                );
                true
            }
        }
    }

    fn end_pointer_interaction(&self) -> bool {
        self.interaction.lock().take().is_some()
    }

    fn update_pointer_capture(&self, position: Point) -> bool {
        let mut capture = self.pointer_capture.lock();
        let Some(active) = capture.as_mut() else {
            return false;
        };

        let distance = (position.x - active.origin.x).abs() + (position.y - active.origin.y).abs();
        if distance >= 6 {
            active.threshold_crossed = true;
        }
        true
    }

    fn end_pointer_capture(&self, position: Point) -> Option<InputRouting> {
        let capture = self.pointer_capture.lock().take()?;
        Some(self.window_route(capture.window_id, Some(position), true))
    }

    fn route_pointer_motion(&self, position: Point) -> InputRouting {
        self.route_pointer_target(position, false)
            .or_else(|| self.focused_window_route(Some(position), false))
            .unwrap_or(InputRouting::None)
    }

    fn route_pointer_target(&self, position: Point, captured: bool) -> Option<InputRouting> {
        let target = { self.windows.lock().hit_test(position) }?;
        match target {
            WindowHitTarget::Content(window_id) => {
                Some(self.window_route(window_id, Some(position), captured))
            }
            WindowHitTarget::Titlebar(window_id)
            | WindowHitTarget::Resize(window_id, _)
            | WindowHitTarget::Chrome(window_id, _) => self
                .windows
                .lock()
                .window_app(window_id)
                .map(InputRouting::FocusOnly),
        }
    }

    fn window_route(
        &self,
        window_id: WindowId,
        global_position: Option<Point>,
        captured: bool,
    ) -> InputRouting {
        let windows = self.windows.lock();
        let Some(app_id) = windows.window_app(window_id) else {
            return InputRouting::None;
        };

        let local_position = global_position.and_then(|point| {
            windows
                .content_rect(window_id)
                .filter(|content_rect| content_rect.contains(point))
                .map(|content_rect| content_rect.local_point(point))
        });

        InputRouting::DeliverTo {
            app_id,
            window_id,
            global_position,
            local_position,
            captured,
        }
    }

    fn focused_window_route(
        &self,
        global_position: Option<Point>,
        captured: bool,
    ) -> Option<InputRouting> {
        self.focused_window()
            .map(|window_id| self.window_route(window_id, global_position, captured))
    }

    fn captured_window_route(
        &self,
        global_position: Option<Point>,
        captured: bool,
    ) -> Option<InputRouting> {
        let window_id = self
            .pointer_capture
            .lock()
            .as_ref()
            .map(|capture| capture.window_id)?;
        Some(self.window_route(window_id, global_position, captured))
    }

    fn focus_window_internal(&self, window_id: WindowId) -> Result<AppId, String> {
        let (damage_rect, app_id) = {
            let mut windows = self.windows.lock();
            let damage_rect = windows
                .focus_window(window_id)
                .map_err(window_error_message)?;
            let app_id = windows
                .window_app(window_id)
                .ok_or_else(|| String::from("window not found"))?;
            (damage_rect, app_id)
        };

        if let Some(rect) = damage_rect {
            self.damage.lock().mark_rect(rect);
        }
        Ok(app_id)
    }

    fn handle_chrome_action(
        &self,
        window_id: WindowId,
        button: ChromeButton,
        app_id: AppId,
    ) -> InputRouting {
        match button {
            ChromeButton::Close => {
                let _ = self.destroy_window(window_id);
                self.focused_app()
                    .map(InputRouting::FocusOnly)
                    .unwrap_or(InputRouting::None)
            }
            ChromeButton::Minimize => {
                let _ = self.set_window_visibility(window_id, false);
                self.focused_app()
                    .map(InputRouting::FocusOnly)
                    .unwrap_or(InputRouting::None)
            }
            ChromeButton::Maximize => {
                let _ = self.toggle_maximize_window(window_id);
                InputRouting::FocusOnly(app_id)
            }
        }
    }

    fn toggle_maximize_window(&self, window_id: WindowId) -> Result<(), String> {
        let (old_frame, new_frame, surface_id, content_rect) = {
            let mut windows = self.windows.lock();
            let (old_frame, new_frame, _) = windows
                .toggle_maximize(window_id, self.work_area())
                .map_err(window_error_message)?;
            let surface_id = windows
                .window_surface(window_id)
                .ok_or_else(|| String::from("window not found"))?;
            let content_rect = windows.content_rect(window_id).unwrap_or_default();
            (old_frame, new_frame, surface_id, content_rect)
        };

        {
            let mut surfaces = self.surfaces.lock();
            surfaces
                .set_visible(surface_id, true)
                .map_err(surface_error_message)?;
            surfaces
                .set_geometry(
                    surface_id,
                    content_rect.x,
                    content_rect.y,
                    content_rect.width,
                    content_rect.height,
                )
                .map_err(surface_error_message)?;
        }
        self.damage.lock().mark_rects(&[old_frame, new_frame]);
        Ok(())
    }

    fn update_window_frame(
        &self,
        window_id: WindowId,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        let (old_frame, new_frame, surface_id, content_rect) = {
            let mut windows = self.windows.lock();
            let (old_frame, new_frame, _) = windows
                .set_window_frame(window_id, x, y, width, height)
                .map_err(window_error_message)?;
            let surface_id = windows
                .window_surface(window_id)
                .ok_or_else(|| String::from("window not found"))?;
            let content_rect = windows.content_rect(window_id).unwrap_or_default();
            (old_frame, new_frame, surface_id, content_rect)
        };

        {
            let mut surfaces = self.surfaces.lock();
            surfaces
                .set_visible(surface_id, true)
                .map_err(surface_error_message)?;
            surfaces
                .set_geometry(
                    surface_id,
                    content_rect.x,
                    content_rect.y,
                    content_rect.width,
                    content_rect.height,
                )
                .map_err(surface_error_message)?;
        }

        self.damage.lock().mark_rects(&[old_frame, new_frame]);
        Ok(())
    }

    fn work_area(&self) -> Rect {
        shell::desktop_work_area(self.screen_rect())
    }

    fn collect_surface_placements(&self) -> Vec<SurfacePlacement> {
        self.ordering(true)
            .into_iter()
            .map(|snapshot| snapshot.placement())
            .collect()
    }

    fn snapshot_desktop(&self) -> DisplayResponse {
        let fb = self.framebuffer.lock();
        let mut pixels = Vec::with_capacity(fb.width.saturating_mul(fb.height));
        for y in 0..fb.height {
            for x in 0..fb.width {
                pixels.push(fb.get_pixel(x, y));
            }
        }
        DisplayResponse::DesktopSnapshot {
            width: fb.width as u32,
            height: fb.height as u32,
            pixels,
        }
    }

    fn update_cursor(&self, position: Point) {
        let screen_rect = self.screen_rect();
        let clamped = Point::new(
            position.x.clamp(0, screen_rect.right().saturating_sub(1)),
            position.y.clamp(0, screen_rect.bottom().saturating_sub(1)),
        );
        let mut cursor = self.cursor_position.lock();
        let old_rect = cursor_rect(*cursor);
        *cursor = clamped;
        let new_rect = cursor_rect(clamped);
        drop(cursor);

        let mut damage = self.damage.lock();
        damage.mark_rect(old_rect);
        damage.mark_rect(new_rect);
    }
}

fn surface_error_message(err: SurfaceError) -> String {
    match err {
        SurfaceError::InvalidSize => String::from("invalid surface size"),
        SurfaceError::SurfaceNotFound => String::from("surface not found"),
        SurfaceError::BufferSizeMismatch => String::from("surface buffer size mismatch"),
        SurfaceError::OutOfMemory => String::from("surface allocation failed"),
        SurfaceError::SharedSurfaceUnavailable => String::from("shared surface unavailable"),
    }
}

fn window_error_message(err: WindowError) -> String {
    match err {
        WindowError::WindowNotFound => String::from("window not found"),
        WindowError::InvalidSize => String::from("invalid window size"),
    }
}

fn resize_frame(start: Rect, edge: ResizeEdge, start_pointer: Point, current: Point) -> Rect {
    let dx = current.x.saturating_sub(start_pointer.x);
    let dy = current.y.saturating_sub(start_pointer.y);

    let mut left = start.x;
    let mut top = start.y;
    let mut right = start.right();
    let mut bottom = start.bottom();

    match edge {
        ResizeEdge::Left | ResizeEdge::TopLeft | ResizeEdge::BottomLeft => {
            left = left.saturating_add(dx)
        }
        ResizeEdge::Right | ResizeEdge::TopRight | ResizeEdge::BottomRight => {
            right = right.saturating_add(dx)
        }
        _ => {}
    }

    match edge {
        ResizeEdge::Top | ResizeEdge::TopLeft | ResizeEdge::TopRight => {
            top = top.saturating_add(dy)
        }
        ResizeEdge::Bottom | ResizeEdge::BottomLeft | ResizeEdge::BottomRight => {
            bottom = bottom.saturating_add(dy)
        }
        _ => {}
    }

    let min_width = MIN_CONTENT_WIDTH as i32;
    if right.saturating_sub(left) < min_width {
        match edge {
            ResizeEdge::Left | ResizeEdge::TopLeft | ResizeEdge::BottomLeft => {
                left = right.saturating_sub(min_width);
            }
            _ => {
                right = left.saturating_add(min_width);
            }
        }
    }

    let min_height = (MIN_CONTENT_HEIGHT + TITLEBAR_HEIGHT) as i32;
    if bottom.saturating_sub(top) < min_height {
        match edge {
            ResizeEdge::Top | ResizeEdge::TopLeft | ResizeEdge::TopRight => {
                top = bottom.saturating_sub(min_height);
            }
            _ => {
                bottom = top.saturating_add(min_height);
            }
        }
    }

    Rect::new(
        left,
        top,
        right.saturating_sub(left).max(1) as u32,
        bottom.saturating_sub(top).max(1) as u32,
    )
}

fn draw_window(
    fb: &mut Framebuffer,
    window: &WindowInfo,
    pixels: &[u32],
    text_system: &mut TextSystem,
    damage: Rect,
    mode: ThemeMode,
) {
    if window.layer_role == LayerRole::Background {
        draw_window_content(fb, window.content_rect, pixels, damage);
        return;
    }

    if !window.flags.decorate || window.layer_role != LayerRole::Window {
        if matches!(
            window.layer_role,
            LayerRole::Overlay | LayerRole::Modal | LayerRole::WorkspaceScratchpad
        ) {
            draw_window_shadow(
                fb,
                window.frame_rect,
                damage,
                Theme::shadow(crate::gui::theme::Elevation::Floating, mode),
            );
            fill_rect_clipped(
                fb,
                window.frame_rect,
                damage,
                Theme::surface(
                    crate::gui::theme::SurfaceRole::Overlay,
                    mode,
                    WindowChromeVariant::Inactive,
                ),
            );
        }
        draw_window_content(fb, window.content_rect, pixels, damage);
        return;
    }

    let tokens = Theme::tokens(mode);
    let chrome = if window.focused {
        WindowChromeVariant::Active
    } else {
        WindowChromeVariant::Inactive
    };
    let border_color = if window.focused {
        tokens.borders.focus
    } else {
        tokens.borders.subtle
    };

    draw_window_shadow(
        fb,
        window.frame_rect,
        damage,
        if window.focused {
            Theme::shadow(crate::gui::theme::Elevation::Focused, mode)
        } else {
            Theme::shadow(crate::gui::theme::Elevation::Floating, mode)
        },
    );
    fill_rect_clipped(
        fb,
        window.frame_rect,
        damage,
        Theme::surface(crate::gui::theme::SurfaceRole::Window, mode, chrome),
    );
    fill_rect_clipped(
        fb,
        window.content_rect,
        damage,
        if window.focused {
            Theme::surface(crate::gui::theme::SurfaceRole::Window, mode, chrome)
        } else {
            Theme::shade(
                Theme::surface(crate::gui::theme::SurfaceRole::Window, mode, chrome),
                -8,
            )
        },
    );

    let titlebar = titlebar_rect(window.frame_rect);
    fill_rect_clipped(
        fb,
        titlebar,
        damage,
        Theme::surface(crate::gui::theme::SurfaceRole::WindowTitlebar, mode, chrome),
    );
    fill_rect_clipped(
        fb,
        Rect::new(titlebar.x, titlebar.y, titlebar.width, 1),
        damage,
        tokens.borders.strong,
    );

    draw_window_content(fb, window.content_rect, pixels, damage);
    draw_frame_outline(fb, window.frame_rect, damage, border_color);
    draw_chrome_buttons(fb, window, damage, mode);

    draw_window_title(
        fb,
        text_system,
        window,
        titlebar,
        damage,
        tokens.text.primary,
    );
}

fn draw_window_scene(
    fb: &mut Framebuffer,
    renderer: &mut impl Renderer,
    window: &WindowInfo,
    scene: &SceneUpdate,
    text_system: &mut TextSystem,
    damage: Rect,
    mode: ThemeMode,
) {
    if window.layer_role == LayerRole::Background {
        renderer.render_scene(
            fb,
            damage,
            window.content_rect.x,
            window.content_rect.y,
            scene,
            text_system,
        );
        return;
    }

    if !window.flags.decorate || window.layer_role != LayerRole::Window {
        match window.layer_role {
            LayerRole::TopBar => {
                fill_rect_clipped(
                    fb,
                    window.frame_rect,
                    damage,
                    Theme::shell_surface(crate::gui::theme::ShellSurfaceRole::HaloBar, mode, true),
                );
            }
            LayerRole::Dock => {
                fill_rect_clipped(
                    fb,
                    window.frame_rect,
                    damage,
                    Theme::shell_surface(crate::gui::theme::ShellSurfaceRole::Dock, mode, true),
                );
            }
            LayerRole::Overlay | LayerRole::Modal | LayerRole::WorkspaceScratchpad => {
                draw_window_shadow(
                    fb,
                    window.frame_rect,
                    damage,
                    Theme::shadow(crate::gui::theme::Elevation::Floating, mode),
                );
                fill_rect_clipped(
                    fb,
                    window.frame_rect,
                    damage,
                    Theme::surface(
                        crate::gui::theme::SurfaceRole::Overlay,
                        mode,
                        WindowChromeVariant::Inactive,
                    ),
                );
            }
            _ => {}
        }
        renderer.render_scene(
            fb,
            damage,
            window.content_rect.x,
            window.content_rect.y,
            scene,
            text_system,
        );
        return;
    }

    let tokens = Theme::tokens(mode);
    let chrome = if window.focused {
        WindowChromeVariant::Active
    } else {
        WindowChromeVariant::Inactive
    };
    let border_color = if window.focused {
        tokens.borders.focus
    } else {
        tokens.borders.subtle
    };

    draw_window_shadow(
        fb,
        window.frame_rect,
        damage,
        if window.focused {
            Theme::shadow(crate::gui::theme::Elevation::Focused, mode)
        } else {
            Theme::shadow(crate::gui::theme::Elevation::Floating, mode)
        },
    );
    fill_rect_clipped(
        fb,
        window.frame_rect,
        damage,
        Theme::surface(crate::gui::theme::SurfaceRole::Window, mode, chrome),
    );
    fill_rect_clipped(
        fb,
        window.content_rect,
        damage,
        if window.focused {
            Theme::surface(crate::gui::theme::SurfaceRole::Window, mode, chrome)
        } else {
            Theme::shade(
                Theme::surface(crate::gui::theme::SurfaceRole::Window, mode, chrome),
                -8,
            )
        },
    );

    let titlebar = titlebar_rect(window.frame_rect);
    fill_rect_clipped(
        fb,
        titlebar,
        damage,
        Theme::surface(crate::gui::theme::SurfaceRole::WindowTitlebar, mode, chrome),
    );
    fill_rect_clipped(
        fb,
        Rect::new(titlebar.x, titlebar.y, titlebar.width, 1),
        damage,
        tokens.borders.strong,
    );

    renderer.render_scene(
        fb,
        damage,
        window.content_rect.x,
        window.content_rect.y,
        scene,
        text_system,
    );
    draw_frame_outline(fb, window.frame_rect, damage, border_color);
    draw_chrome_buttons(fb, window, damage, mode);

    draw_window_title(
        fb,
        text_system,
        window,
        titlebar,
        damage,
        tokens.text.primary,
    );
}

fn draw_window_title(
    fb: &mut Framebuffer,
    text_system: &mut TextSystem,
    window: &WindowInfo,
    titlebar: Rect,
    damage: Rect,
    color: u32,
) {
    if !titlebar.intersects(&damage) || window.title.is_empty() {
        return;
    }

    let blob = text_system.layout_text_with_style(
        &window.title,
        titlebar.width.saturating_sub(92).max(1),
        TextStyle::ui(),
        color,
    );
    let title_rect = Rect::new(
        window.frame_rect.x + 16,
        window.frame_rect.y + 8,
        blob.width_px.max(1),
        blob.height_px.max(1),
    );
    let Some(render_rect) = title_rect.intersection(&damage) else {
        return;
    };

    let offset_x = (render_rect.x - title_rect.x) as usize;
    let offset_y = (render_rect.y - title_rect.y) as usize;
    let width = blob.width_px as usize;
    for row in 0..render_rect.height as usize {
        let src_row = (offset_y + row) * width;
        let dst_y = render_rect.y.max(0) as usize + row;
        if dst_y >= fb.height {
            break;
        }
        for col in 0..render_rect.width as usize {
            let src_idx = src_row + offset_x + col;
            let dst_x = render_rect.x.max(0) as usize + col;
            if dst_x >= fb.width || src_idx >= blob.pixels.len() {
                break;
            }
            let source = blob.pixels[src_idx];
            if ((source >> 24) & 0xFF) == 0 {
                continue;
            }
            fb.plot_pixel(dst_x, dst_y, source);
        }
    }
}

fn draw_diagnostics_overlay(
    fb: &mut Framebuffer,
    text_system: &mut TextSystem,
    damage: Rect,
    lines: &[String],
) {
    if lines.is_empty() {
        return;
    }

    let max_chars = lines.iter().map(|line| line.len()).max().unwrap_or(0) as u32;
    let line_height = 18u32;
    let panel_width = (max_chars.saturating_mul(8)).saturating_add(24).max(180);
    let panel_height = line_height
        .saturating_mul(lines.len() as u32)
        .saturating_add(16);
    let panel = Rect::new(12, 12, panel_width, panel_height);
    let Some(panel_damage) = panel.intersection(&damage) else {
        return;
    };

    fill_rect_clipped(fb, panel, panel_damage, 0xCC111827);
    draw_rect_outline_clipped(fb, panel, panel_damage, 0xFF60A5FA);

    for (index, line) in lines.iter().enumerate() {
        let blob = text_system.layout_text_with_style(
            line,
            panel.width.saturating_sub(16).max(1),
            TextStyle::mono(),
            0xFFF8FAFC,
        );
        let line_rect = Rect::new(
            panel.x + 8,
            panel.y + 8 + (index as i32 * line_height as i32),
            blob.width_px.max(1),
            blob.height_px.max(1),
        );
        let Some(render_rect) = line_rect.intersection(&panel_damage) else {
            continue;
        };
        let offset_x = (render_rect.x - line_rect.x) as usize;
        let offset_y = (render_rect.y - line_rect.y) as usize;
        let width = blob.width_px as usize;
        for row in 0..render_rect.height as usize {
            let src_row = (offset_y + row) * width;
            let dst_y = render_rect.y.max(0) as usize + row;
            if dst_y >= fb.height {
                break;
            }
            for col in 0..render_rect.width as usize {
                let src_idx = src_row + offset_x + col;
                let dst_x = render_rect.x.max(0) as usize + col;
                if dst_x >= fb.width || src_idx >= blob.pixels.len() {
                    break;
                }
                let source = blob.pixels[src_idx];
                if ((source >> 24) & 0xFF) == 0 {
                    continue;
                }
                fb.plot_pixel(dst_x, dst_y, source);
            }
        }
    }
}

fn draw_window_content(fb: &mut Framebuffer, content_rect: Rect, pixels: &[u32], damage: Rect) {
    let Some(clip) = content_rect.intersection(&damage) else {
        return;
    };

    let width = content_rect.width as usize;
    if width == 0 {
        return;
    }

    let offset_x = (clip.x - content_rect.x) as usize;
    let offset_y = (clip.y - content_rect.y) as usize;

    for row in 0..clip.height as usize {
        let src_row = (offset_y + row) * width;
        let dst_y = clip.y as usize + row;
        for col in 0..clip.width as usize {
            let pixel = pixels[src_row + offset_x + col];
            let dst_x = clip.x as usize + col;
            fb.plot_pixel(dst_x, dst_y, pixel);
        }
    }
}

fn draw_frame_outline(fb: &mut Framebuffer, frame_rect: Rect, damage: Rect, color: u32) {
    let top = Rect::new(
        frame_rect.x,
        frame_rect.y,
        frame_rect.width,
        BORDER_THICKNESS,
    );
    let bottom = Rect::new(
        frame_rect.x,
        frame_rect.bottom().saturating_sub(BORDER_THICKNESS as i32),
        frame_rect.width,
        BORDER_THICKNESS,
    );
    let left = Rect::new(
        frame_rect.x,
        frame_rect.y,
        BORDER_THICKNESS,
        frame_rect.height,
    );
    let right = Rect::new(
        frame_rect.right().saturating_sub(BORDER_THICKNESS as i32),
        frame_rect.y,
        BORDER_THICKNESS,
        frame_rect.height,
    );

    fill_rect_clipped(fb, top, damage, color);
    fill_rect_clipped(fb, bottom, damage, color);
    fill_rect_clipped(fb, left, damage, color);
    fill_rect_clipped(fb, right, damage, color);
}

fn draw_chrome_buttons(fb: &mut Framebuffer, window: &WindowInfo, damage: Rect, mode: ThemeMode) {
    draw_chrome_button(
        fb,
        chrome_button_rect(window.frame_rect, ChromeButton::Minimize),
        Theme::ACCENT_WARNING.to_u32(),
        damage,
        ChromeButton::Minimize,
        window.maximized,
        mode,
    );
    draw_chrome_button(
        fb,
        chrome_button_rect(window.frame_rect, ChromeButton::Maximize),
        Theme::ACCENT_SUCCESS.to_u32(),
        damage,
        ChromeButton::Maximize,
        window.maximized,
        mode,
    );
    draw_chrome_button(
        fb,
        chrome_button_rect(window.frame_rect, ChromeButton::Close),
        Theme::ACCENT_ERROR.to_u32(),
        damage,
        ChromeButton::Close,
        window.maximized,
        mode,
    );
}

fn draw_chrome_button(
    fb: &mut Framebuffer,
    rect: Rect,
    color: u32,
    damage: Rect,
    kind: ChromeButton,
    maximized: bool,
    mode: ThemeMode,
) {
    fill_rect_clipped(fb, rect, damage, color);
    draw_rect_outline_clipped(fb, rect, damage, Theme::shade(color, -36));

    let inner = rect.inset(2, 2, 2, 2);
    match kind {
        ChromeButton::Minimize => {
            let bar = Rect::new(inner.x, inner.bottom().saturating_sub(2), inner.width, 2);
            fill_rect_clipped(
                fb,
                bar,
                damage,
                Theme::surface(
                    crate::gui::theme::SurfaceRole::Desktop,
                    mode,
                    WindowChromeVariant::Inactive,
                ),
            );
        }
        ChromeButton::Maximize => {
            draw_rect_outline_clipped(
                fb,
                inner,
                damage,
                Theme::surface(
                    crate::gui::theme::SurfaceRole::Desktop,
                    mode,
                    WindowChromeVariant::Inactive,
                ),
            );
            if maximized {
                let nested = inner.inset(2, 2, 0, 0);
                draw_rect_outline_clipped(
                    fb,
                    nested,
                    damage,
                    Theme::surface(
                        crate::gui::theme::SurfaceRole::Desktop,
                        mode,
                        WindowChromeVariant::Inactive,
                    ),
                );
            }
        }
        ChromeButton::Close => {
            let start_x = rect.x.saturating_add(2).max(0) as usize;
            let start_y = rect.y.saturating_add(2).max(0) as usize;
            let span = rect.width.saturating_sub(4) as usize;
            for step in 0..span {
                let x0 = start_x + step;
                let y0 = start_y + step;
                let x1 = start_x + span.saturating_sub(1).saturating_sub(step);
                let y1 = start_y + step;
                fb.plot_pixel(
                    x0,
                    y0,
                    Theme::surface(
                        crate::gui::theme::SurfaceRole::Desktop,
                        mode,
                        WindowChromeVariant::Inactive,
                    ),
                );
                fb.plot_pixel(
                    x1,
                    y1,
                    Theme::surface(
                        crate::gui::theme::SurfaceRole::Desktop,
                        mode,
                        WindowChromeVariant::Inactive,
                    ),
                );
            }
        }
    }
}

fn fill_rect_clipped(fb: &mut Framebuffer, rect: Rect, clip: Rect, color: u32) {
    let Some(clipped) = rect.intersection(&clip) else {
        return;
    };

    for y in 0..clipped.height as usize {
        for x in 0..clipped.width as usize {
            fb.plot_pixel(clipped.x as usize + x, clipped.y as usize + y, color);
        }
    }
}

fn draw_rect_outline_clipped(fb: &mut Framebuffer, rect: Rect, clip: Rect, color: u32) {
    if rect.is_empty() {
        return;
    }

    fill_rect_clipped(fb, Rect::new(rect.x, rect.y, rect.width, 1), clip, color);
    fill_rect_clipped(
        fb,
        Rect::new(rect.x, rect.bottom().saturating_sub(1), rect.width, 1),
        clip,
        color,
    );
    fill_rect_clipped(fb, Rect::new(rect.x, rect.y, 1, rect.height), clip, color);
    fill_rect_clipped(
        fb,
        Rect::new(rect.right().saturating_sub(1), rect.y, 1, rect.height),
        clip,
        color,
    );
}

fn draw_window_shadow(fb: &mut Framebuffer, frame_rect: Rect, damage: Rect, shadow: u32) {
    let outer = Rect::new(
        frame_rect.x.saturating_sub(10),
        frame_rect.y.saturating_sub(10),
        frame_rect.width.saturating_add(20),
        frame_rect.height.saturating_add(20),
    );
    let Some(clipped) = outer.intersection(&damage) else {
        return;
    };

    for y in clipped.y.max(0) as usize..clipped.bottom().max(0) as usize {
        for x in clipped.x.max(0) as usize..clipped.right().max(0) as usize {
            let xi = x as i32;
            let yi = y as i32;
            let inside = xi >= frame_rect.x
                && xi < frame_rect.right()
                && yi >= frame_rect.y
                && yi < frame_rect.bottom();
            if inside {
                continue;
            }

            let dx = if xi < frame_rect.x {
                frame_rect.x - xi
            } else if xi >= frame_rect.right() {
                xi - frame_rect.right() + 1
            } else {
                0
            };
            let dy = if yi < frame_rect.y {
                frame_rect.y - yi
            } else if yi >= frame_rect.bottom() {
                yi - frame_rect.bottom() + 1
            } else {
                0
            };
            let edge = dx.max(dy).min(10) as u8;
            let alpha = (11u8.saturating_sub(edge)).saturating_mul(10);
            if alpha == 0 {
                continue;
            }
            let base = fb.get_pixel(x, y);
            fb.plot_pixel(x, y, shell::blend_color(base, shadow, alpha));
        }
    }
}

const CURSOR_PATTERN: [&str; 16] = [
    "X...............",
    "XX..............",
    "X.X.............",
    "X..X............",
    "X...X...........",
    "X....X..........",
    "X.....X.........",
    "X......X........",
    "X.......X.......",
    "X........X......",
    "X.....XXXXX.....",
    "X..X..X.........",
    "X.X X..X........",
    "XX  X..X........",
    "X    X..X.......",
    ".....XXXX.......",
];

fn cursor_rect(position: Point) -> Rect {
    Rect::new(position.x, position.y, 16, 16)
}

fn draw_cursor(fb: &mut Framebuffer, position: Point, damage: Rect) {
    let Some(clip) = cursor_rect(position).intersection(&damage) else {
        return;
    };

    for (row, pattern) in CURSOR_PATTERN.iter().enumerate() {
        let y = position.y + row as i32;
        if y < clip.y || y >= clip.bottom() {
            continue;
        }

        for (col, pixel) in pattern.as_bytes().iter().enumerate() {
            let x = position.x + col as i32;
            if x < clip.x || x >= clip.right() {
                continue;
            }

            let color = match *pixel {
                b'X' => Some(0xFFF4F7FB),
                b' ' => Some(0xFF111827),
                _ => None,
            };
            if let Some(color) = color {
                fb.plot_pixel(x as usize, y as usize, color);
            }
        }
    }

    let hotspot = Rect::new(position.x, position.y, 2, 2);
    fill_rect_clipped(fb, hotspot, clip, 0xFF111827);
}

lazy_static::lazy_static! {
    static ref ECH_DISPLAY: Mutex<Option<Arc<EchDisplay>>> = Mutex::new(None);
}

fn sync_output_geometry(width: u32, height: u32) {
    crate::drivers::mouse::set_bounds(width as i32, height as i32);
    crate::gfx::scaling::init_from_resolution(width, height);
    crate::gui::login::init(width as usize, height as usize);
}

pub fn init() {
    crate::drivers::gpu_native::init();
    crate::drivers::drm::init();

    let fb = match crate::boot::get_global_framebuffer() {
        Some(fb) => fb,
        None => {
            crate::serial_println!("[ECHDISPLAY] framebuffer unavailable");
            return;
        }
    };

    sync_output_geometry(fb.width as u32, fb.height as u32);
    let fb = Arc::new(Mutex::new(fb));
    let display = Arc::new(EchDisplay::new(fb));
    display.start();
    let _ = display.process_command(DisplayCommand::Present);

    *ECH_DISPLAY.lock() = Some(Arc::clone(&display));
    crate::serial_println!("[ECHDISPLAY] Week-2 initialized");
}

pub fn get_display() -> &'static Mutex<Option<Arc<EchDisplay>>> {
    &ECH_DISPLAY
}

pub fn service_task() -> ! {
    loop {
        let display = { ECH_DISPLAY.lock().clone() };
        if let Some(display) = display {
            display.run_service();
        }
        for _ in 0..1000 {
            core::hint::spin_loop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        sync_output_geometry, CompositionDiagnosticCode, DisplayCommand, DisplayResponse,
        EchDisplay, InputRouting,
    };
    use crate::drivers::mouse::{
        get_bounds, get_position, publish_state, set_bounds, MouseButtons,
    };
    use crate::gfx::scaling::get_scale_factor;
    use crate::gfx::shell_scene::raster_surface_scene;
    use crate::gop::framebuffer::Framebuffer;
    use crate::gui::protocol::{
        DamageLane, DisplayProfile, HdrPolicy, InputEvent, KeyState, LayerRole, OutputMode, Rect,
        VrrPolicy,
    };
    use alloc::string::String;
    use alloc::sync::Arc;
    use spin::Mutex;

    #[test]
    fn sync_output_geometry_updates_mouse_bounds_and_scale() {
        publish_state(4096, 4096, MouseButtons::default());

        sync_output_geometry(1920, 1080);

        assert_eq!(get_position(), (1919, 1079));
        assert_eq!(get_scale_factor(), 100);
    }

    #[test]
    fn output_mode_catalog_is_bounded_by_live_framebuffer() {
        let display = EchDisplay::new(Arc::new(Mutex::new(Framebuffer::new_for_test(1920, 1080))));
        let modes = display.supported_output_modes();

        assert!(modes.contains(&OutputMode::new(1920, 1080, 60)));
        assert!(modes.contains(&OutputMode::new(1280, 720, 60)));
        assert!(!modes.contains(&OutputMode::new(2560, 1440, 60)));
    }

    #[test]
    fn set_output_mode_updates_effective_rect_and_mouse_bounds() {
        publish_state(4096, 4096, MouseButtons::default());

        let display = EchDisplay::new(Arc::new(Mutex::new(Framebuffer::new_for_test(1920, 1080))));
        let response = display.set_output_mode(OutputMode::new(1280, 720, 60));

        let DisplayResponse::OutputModeState {
            current,
            requested,
            effective,
        } = response
        else {
            unreachable!("set_output_mode must return output state")
        };
        assert_eq!(current, OutputMode::new(1920, 1080, 60));
        assert_eq!(requested, OutputMode::new(1280, 720, 60));
        assert_eq!(effective, OutputMode::new(1280, 720, 60));

        assert_eq!(
            display.screen_rect(),
            crate::gui::protocol::Rect::new(0, 0, 1280, 720)
        );
        assert_eq!(get_position(), (1279, 719));
    }

    #[test]
    fn ensure_runtime_pointer_geometry_resyncs_bounds_when_runtime_drift_detected() {
        publish_state(4096, 4096, MouseButtons::default());

        let display = EchDisplay::new(Arc::new(Mutex::new(Framebuffer::new_for_test(1920, 1080))));

        set_bounds(1280, 800);
        display.ensure_runtime_pointer_geometry();

        assert_eq!(get_bounds(), (1920, 1080));
        assert_eq!(get_position(), (1279, 799));
        assert_eq!(
            display.screen_rect(),
            crate::gui::protocol::Rect::new(0, 0, 1920, 1080)
        );
    }

    #[test]
    fn route_input_event_command_returns_routing_response() {
        let display = EchDisplay::new(Arc::new(Mutex::new(Framebuffer::new_for_test(1280, 720))));
        let response = display.process_command(DisplayCommand::RouteInputEvent {
            event: InputEvent::Key {
                unicode: Some('a'),
                scan_code: 30,
                modifiers: 0,
                state: KeyState::Pressed,
            },
        });

        assert!(matches!(
            response,
            DisplayResponse::InputRoute(InputRouting::None)
        ));
    }

    #[test]
    fn list_windows_reflects_scene_surface_metadata_from_joined_snapshot() {
        let display = EchDisplay::new(Arc::new(Mutex::new(Framebuffer::new_for_test(1280, 720))));
        let response = display.process_command(DisplayCommand::CreateWindow {
            app_id: 77,
            title: String::from("Scene Window"),
            x: 40,
            y: 48,
            width: 320,
            height: 220,
        });
        let DisplayResponse::WindowCreated {
            window_id,
            content_rect,
            ..
        } = response
        else {
            unreachable!("window should be created")
        };
        let scene = raster_surface_scene(
            window_id,
            content_rect.width as usize,
            content_rect.height as usize,
            alloc::vec![0xFF223344; (content_rect.width * content_rect.height) as usize],
            DamageLane::Shell,
        );
        let _ = display.process_command(DisplayCommand::CommitScene { window_id, scene });

        let DisplayResponse::WindowList { windows } =
            display.process_command(DisplayCommand::ListWindows)
        else {
            unreachable!("window listing should succeed")
        };
        let window = windows
            .into_iter()
            .find(|window| window.id == window_id)
            .expect("joined window info");
        assert_eq!(
            window.buffer_mode,
            crate::gui::protocol::WindowBufferMode::Scene
        );
        assert_eq!(window.scene_root, Some(window_id));
        assert!(window.semantic_root.is_none());
    }

    #[test]
    fn unresolved_visible_windows_emit_diagnostic_and_are_excluded_from_present() {
        let display = EchDisplay::new(Arc::new(Mutex::new(Framebuffer::new_for_test(1280, 720))));
        let response = display.process_command(DisplayCommand::CreateWindow {
            app_id: 88,
            title: String::from("Broken Window"),
            x: 64,
            y: 72,
            width: 300,
            height: 180,
        });
        let DisplayResponse::WindowCreated { window_id, .. } = response else {
            unreachable!("window should be created")
        };
        let surface_id = display
            .windows
            .lock()
            .ordered_windows()
            .into_iter()
            .find(|window| window.id == window_id)
            .expect("window metadata")
            .surface_id;
        let _ = display.surfaces.lock().destroy_surface(surface_id);
        display
            .damage
            .lock()
            .mark_rect(crate::gui::protocol::Rect::new(0, 0, 1280, 720));

        let response = display.process_command(DisplayCommand::Present);
        assert!(matches!(
            response,
            DisplayResponse::Ack | DisplayResponse::Presented { .. }
        ));
        assert_eq!(
            display
                .diagnostics
                .lock()
                .count(CompositionDiagnosticCode::UnresolvedSurface),
            1
        );
    }

    #[test]
    fn query_display_capability_reports_supported_modes() {
        crate::drivers::drm::init();
        let display = EchDisplay::new(Arc::new(Mutex::new(Framebuffer::new_for_test(1920, 1080))));

        let DisplayResponse::DisplayCapability(capability) =
            display.process_command(DisplayCommand::QueryDisplayCapability)
        else {
            unreachable!("display capability should be returned")
        };

        assert!(capability.connected_outputs >= 1);
        assert!(capability.fractional_scaling);
        assert!(capability
            .supported_modes
            .contains(&OutputMode::new(1280, 720, 60)));
    }

    #[test]
    fn set_display_profile_disables_unsupported_hdr_policy() {
        let display = EchDisplay::new(Arc::new(Mutex::new(Framebuffer::new_for_test(1920, 1080))));
        let mut profile = DisplayProfile::single_output(OutputMode::new(1920, 1080, 60));
        profile.outputs[0].scale_100x = 163;
        profile.outputs[0].hdr_policy = HdrPolicy::On;

        let DisplayResponse::DisplayProfile(updated) =
            display.process_command(DisplayCommand::SetDisplayProfile { profile })
        else {
            unreachable!("display profile should be returned")
        };

        assert_eq!(updated.outputs[0].hdr_policy, HdrPolicy::Off);
        assert!(matches!(updated.outputs[0].scale_100x, 150 | 175));
        assert_eq!(get_scale_factor(), updated.outputs[0].scale_100x as u32);
    }

    #[test]
    fn set_display_profile_rebuilds_primary_output_when_missing() {
        let display = EchDisplay::new(Arc::new(Mutex::new(Framebuffer::new_for_test(1920, 1080))));
        let mut profile = DisplayProfile::single_output(OutputMode::new(1920, 1080, 60));
        profile.primary_output = 9;
        profile.outputs[0].output_id = 3;

        let DisplayResponse::DisplayProfile(updated) =
            display.process_command(DisplayCommand::SetDisplayProfile { profile })
        else {
            unreachable!("display profile should be returned")
        };

        assert_eq!(updated.primary_output, updated.outputs[0].output_id);
    }

    #[test]
    fn mirrored_outputs_adopt_target_workspace_binding() {
        let mut profile = DisplayProfile::single_output(OutputMode::new(1920, 1080, 60));
        profile.outputs.push(crate::gui::protocol::MonitorPolicy {
            output_id: 1,
            mirror_target: Some(0),
            workspace_binding: Some(7),
            ..crate::gui::protocol::MonitorPolicy::single_output(OutputMode::new(1920, 1080, 60))
        });
        EchDisplay::harmonize_mirror_workspace_bindings(&mut profile);

        assert_eq!(profile.outputs.len(), 2);
        assert_eq!(
            profile.outputs[1].workspace_binding,
            profile.outputs[0].workspace_binding
        );
    }

    #[test]
    fn display_profile_rescues_windows_from_removed_workspace_binding() {
        let display = EchDisplay::new(Arc::new(Mutex::new(Framebuffer::new_for_test(1920, 1080))));
        let initial = DisplayProfile {
            primary_output: 0,
            outputs: alloc::vec![
                crate::gui::protocol::MonitorPolicy::single_output(OutputMode::new(1920, 1080, 60)),
                crate::gui::protocol::MonitorPolicy {
                    output_id: 1,
                    workspace_binding: Some(2),
                    ..crate::gui::protocol::MonitorPolicy::single_output(OutputMode::new(
                        1920, 1080, 60
                    ))
                },
            ],
            capability: crate::gui::protocol::DisplayCapability {
                connected_outputs: 2,
                multi_monitor: true,
                mirror: true,
                ..crate::gui::protocol::DisplayCapability::default()
            },
        };
        *display.display_profile.lock() = initial.clone();
        let _ = display.process_command(DisplayCommand::CreateWindowWithMeta {
            app_id: 91,
            title: String::from("Workspace 2"),
            x: 1100,
            y: 120,
            width: 480,
            height: 320,
            workspace_id: 2,
            layer_role: LayerRole::Window,
            flags: crate::gui::protocol::WindowFlags::default(),
        });

        let single = DisplayProfile::single_output(OutputMode::new(1920, 1080, 60));
        let window_id = display
            .list_windows_with_buffers()
            .into_iter()
            .find(|window| window.title == "Workspace 2")
            .expect("window metadata")
            .id;
        display.apply_monitor_policy_to_windows(&initial, &single);
        *display.display_profile.lock() = single;

        let rescued = display
            .list_windows_with_buffers()
            .into_iter()
            .find(|window| window.id == window_id)
            .expect("rescued window");

        assert_eq!(rescued.workspace_id, 0);
        assert!(Rect::new(0, 0, 1920, 1080)
            .intersection(&rescued.frame_rect)
            .is_some());
    }

    #[test]
    fn runtime_display_state_enters_fullscreen_auto_mode_when_window_covers_output() {
        let display = EchDisplay::new(Arc::new(Mutex::new(Framebuffer::new_for_test(1920, 1080))));
        let profile = DisplayProfile {
            primary_output: 0,
            outputs: alloc::vec![crate::gui::protocol::MonitorPolicy {
                output_id: 0,
                scale_100x: 100,
                text_scale_100x: 100,
                refresh_hz: 144,
                vrr_policy: VrrPolicy::Auto,
                hdr_policy: HdrPolicy::Auto,
                transform: crate::gui::protocol::SurfaceTransform::Identity,
                mirror_target: None,
                workspace_binding: Some(0),
                color_profile: String::from("srgb"),
            }],
            capability: crate::gui::protocol::DisplayCapability {
                adaptive_sync: true,
                hdr_output: true,
                hdr_metadata: true,
                ten_bit_scanout: true,
                max_refresh_hz: 144,
                ..crate::gui::protocol::DisplayCapability::default()
            },
        };
        let DisplayResponse::WindowCreated {
            window_id,
            content_rect,
            ..
        } = display.process_command(DisplayCommand::CreateWindowWithMeta {
            app_id: 92,
            title: String::from("Fullscreen-ish"),
            x: 0,
            y: 0,
            width: 1919,
            height: 1046,
            workspace_id: 0,
            layer_role: LayerRole::Window,
            flags: crate::gui::protocol::WindowFlags::default(),
        })
        else {
            unreachable!("window should be created")
        };
        let scene = raster_surface_scene(
            window_id,
            content_rect.width as usize,
            content_rect.height as usize,
            alloc::vec![0xFF0F172A; (content_rect.width * content_rect.height) as usize],
            DamageLane::Window,
        );
        let _ = display.process_command(DisplayCommand::CommitScene { window_id, scene });
        let snapshots = display.ordering(true);
        display.refresh_runtime_display_state(&profile, &snapshots);

        assert_eq!(
            *display.runtime_activity.lock(),
            super::DisplayRuntimeActivity::Fullscreen
        );
        assert_eq!(*display.effective_vrr_policy.lock(), VrrPolicy::On);
        assert_eq!(*display.effective_hdr_policy.lock(), HdrPolicy::On);
    }
}
