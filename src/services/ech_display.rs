//! # EchDisplay - Week-2 Display Service
//!
//! Sorumluluk:
//! - Window lifecycle
//! - Surface buffer commit
//! - Focus/raise
//! - Damage-tracked composition
//! - Pointer hit-test, drag and resize capture
//! - Native window chrome actions

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

use crate::gop::framebuffer::Framebuffer;
use crate::gui::damage::DamageTracker;
use crate::gui::protocol::{
    AppId, DamagePacket, DisplayPresentMode, FrameIntent, InputEvent, KeyState, PlaneAssignment,
    Point, PointerButton, Rect, SharedSurfaceDescriptor, SurfaceId, VblankFeedback, WindowId,
    WindowInfo,
};
use crate::gui::shell;
use crate::gui::surface::{SurfaceError, SurfaceInfo, SurfaceManager};
use crate::gui::surface_memory::SharedSurfaceMemory;
use crate::gui::theme::{Theme, ThemeMode, WindowChromeVariant};
use crate::gui::window_manager::{
    chrome_button_rect, titlebar_rect, ChromeButton, ResizeEdge, WindowError, WindowHitTarget,
    WindowManager, BORDER_THICKNESS, CHROME_BUTTON_SIZE, MIN_CONTENT_HEIGHT, MIN_CONTENT_WIDTH,
    TITLEBAR_HEIGHT,
};
use crate::services::display_atomic::{AtomicPresenter, HotPathMetrics, SurfacePlacement};

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
    CommitWindowBuffer {
        window_id: WindowId,
        pixels: Vec<u32>,
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
    SetThemeMode {
        mode: ThemeMode,
    },
    SubmitFrameIntent {
        intent: FrameIntent,
    },
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
    PresentMetrics {
        metrics: HotPathMetrics,
    },
    Error(String),
}

pub struct EchDisplay {
    framebuffer: Arc<Mutex<Framebuffer>>,
    running: AtomicBool,
    screen_rect: Rect,
    surfaces: Mutex<SurfaceManager>,
    windows: Mutex<WindowManager>,
    damage: Mutex<DamageTracker>,
    interaction: Mutex<Option<WindowInteraction>>,
    pointer_capture: Mutex<Option<PointerCapture>>,
    cursor_position: Mutex<Point>,
    swallow_left_release: Mutex<bool>,
    atomic_presenter: Mutex<AtomicPresenter>,
    theme_mode: Mutex<ThemeMode>,
    last_presented_frame: AtomicU64,
    command_queue: Mutex<Vec<DisplayCommand>>,
    response_queue: Mutex<Vec<DisplayResponse>>,
}

impl EchDisplay {
    pub fn new(framebuffer: Arc<Mutex<Framebuffer>>) -> Self {
        let screen_rect = {
            let fb = framebuffer.lock();
            Rect::new(0, 0, fb.width as u32, fb.height as u32)
        };

        Self {
            framebuffer,
            running: AtomicBool::new(false),
            screen_rect,
            surfaces: Mutex::new(SurfaceManager::new()),
            windows: Mutex::new(WindowManager::new()),
            damage: Mutex::new(DamageTracker::new()),
            interaction: Mutex::new(None),
            pointer_capture: Mutex::new(None),
            cursor_position: Mutex::new(Point::new(640, 400)),
            swallow_left_release: Mutex::new(false),
            atomic_presenter: Mutex::new(AtomicPresenter::new()),
            theme_mode: Mutex::new(Theme::default_mode()),
            last_presented_frame: AtomicU64::new(0),
            command_queue: Mutex::new(Vec::new()),
            response_queue: Mutex::new(Vec::new()),
        }
    }

    pub fn start(&self) {
        self.running.store(true, Ordering::SeqCst);
        crate::serial_println!("[ECHDISPLAY] Week-2 display service started");
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        crate::serial_println!("[ECHDISPLAY] Week-2 display service stopped");
    }

    pub fn send_command(&self, command: DisplayCommand) {
        self.command_queue.lock().push(command);
    }

    pub fn receive_response(&self) -> Option<DisplayResponse> {
        self.response_queue.lock().pop()
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
            } => self.create_window(app_id, &title, x, y, width, height),
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
            DisplayCommand::CommitWindowBuffer { window_id, pixels } => {
                self.commit_window_buffer(window_id, &pixels)
            }
            DisplayCommand::MapWindowSurface { window_id } => self.map_window_surface(window_id),
            DisplayCommand::SubmitWindowDamage { window_id, packet } => {
                self.submit_window_damage(window_id, packet)
            }
            DisplayCommand::SetPresentMode { mode } => self.set_present_mode(mode),
            DisplayCommand::SetThemeMode { mode } => self.set_theme_mode(mode),
            DisplayCommand::SubmitFrameIntent { intent } => self.submit_frame_intent(intent),
            DisplayCommand::QueryPresentMetrics => self.query_present_metrics(),
            DisplayCommand::ListWindows => {
                let windows = self.windows.lock().ordered_windows();
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
            let commands = {
                let mut queue = self.command_queue.lock();
                core::mem::take(&mut *queue)
            };

            for command in commands {
                let response = self.process_command(command);
                self.response_queue.lock().push(response);
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
            match windows.create_window(app_id, surface_id, title, x, y, width, height) {
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
        let frame = {
            let windows = self.windows.lock();
            let Some(frame_rect) = windows.frame_rect(window_id) else {
                return DisplayResponse::Error(String::from("window not found"));
            };
            frame_rect
        };

        match self.update_window_frame(
            window_id,
            x,
            y,
            frame.width,
            frame.height.saturating_sub(TITLEBAR_HEIGHT),
        ) {
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
        let damage_regions = self.damage.lock().take(self.screen_rect);
        if damage_regions.is_empty() {
            self.surfaces.lock().clear_dirty();
            return self.service_present_queue();
        }
        let theme_mode = *self.theme_mode.lock();
        let native_scanout_available = crate::drivers::gpu_native::device_count() > 0;

        let placements = self.collect_surface_placements();
        let cursor = *self.cursor_position.lock();
        if native_scanout_available {
            {
                let mut presenter = self.atomic_presenter.lock();
                let intent =
                    presenter.build_intent(self.screen_rect, &damage_regions, &placements, cursor);
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

        let windows = self.windows.lock().ordered_windows();
        let surfaces = self.surfaces.lock();
        let snapshots: Vec<(WindowInfo, Vec<u32>)> = windows
            .into_iter()
            .filter(|window| window.visible)
            .filter_map(|window| {
                surfaces
                    .snapshot(window.surface_id)
                    .map(|surface| (window, surface.pixels))
            })
            .collect();

        {
            let mut fb = self.framebuffer.lock();
            for damage in damage_regions.iter() {
                shell::draw_desktop_scene(&mut fb, self.screen_rect, *damage, theme_mode);
                for (window, pixels) in snapshots.iter() {
                    if window.frame_rect.intersects(damage) {
                        draw_window(&mut fb, window, pixels, *damage, theme_mode);
                    }
                }
                draw_cursor(&mut fb, cursor, *damage);
            }
        }

        drop(surfaces);
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

    fn set_theme_mode(&self, mode: ThemeMode) -> DisplayResponse {
        *self.theme_mode.lock() = mode;
        self.damage.lock().mark_rect(self.screen_rect);
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

    fn service_present_queue(&self) -> DisplayResponse {
        if crate::drivers::gpu_native::device_count() == 0 {
            return DisplayResponse::Ack;
        }

        let placements = self.collect_surface_placements();
        let mut presenter = self.atomic_presenter.lock();
        let now_ns = crate::cpu::tsc::read_ns();
        match presenter.commit_latest(self.screen_rect, &placements, now_ns) {
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
        shell::desktop_work_area(self.screen_rect)
    }

    fn collect_surface_placements(&self) -> Vec<SurfacePlacement> {
        self.windows
            .lock()
            .ordered_windows()
            .into_iter()
            .filter(|window| window.visible)
            .map(|window| SurfacePlacement {
                surface_id: window.surface_id,
                rect: window.frame_rect,
                z_index: window.z_index,
                opaque: !window.minimized,
            })
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
        let clamped = Point::new(
            position
                .x
                .clamp(0, self.screen_rect.right().saturating_sub(1)),
            position
                .y
                .clamp(0, self.screen_rect.bottom().saturating_sub(1)),
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
    damage: Rect,
    mode: ThemeMode,
) {
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

    if titlebar.intersects(&damage) && !window.title.is_empty() {
        let tx = (window.frame_rect.x + 16).max(0) as usize;
        let ty = (window.frame_rect.y + 11).max(0) as usize;
        fb.draw_string(tx, ty, &window.title, tokens.text.primary);
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
