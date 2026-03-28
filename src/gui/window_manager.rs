//! Window manager for the native echOS desktop.

use crate::gui::protocol::{
    AppId, LayerRole, Point, Rect, SceneNodeId, SurfaceId, WindowBufferMode, WindowFlags, WindowId,
    WindowInfo, WorkspaceId,
};
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const TITLEBAR_HEIGHT: u32 = 34;
pub const BORDER_THICKNESS: u32 = 1;
pub const RESIZE_HANDLE_SIZE: u32 = 6;
pub const MIN_CONTENT_WIDTH: u32 = 160;
pub const MIN_CONTENT_HEIGHT: u32 = 96;
pub const CHROME_BUTTON_SIZE: u32 = 14;
pub const CHROME_BUTTON_HIT_WIDTH: u32 = 44;
pub const CHROME_BUTTON_HIT_HEIGHT: u32 = 28;
pub const CHROME_BUTTON_GAP: i32 = 6;
pub const CHROME_BUTTON_TOP: i32 = 10;
pub const CHROME_BUTTON_RIGHT_PADDING: i32 = 14;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeEdge {
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChromeButton {
    Minimize,
    Maximize,
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowHitTarget {
    Content(WindowId),
    Titlebar(WindowId),
    Resize(WindowId, ResizeEdge),
    Chrome(WindowId, ChromeButton),
}

#[derive(Clone, Debug)]
pub enum WindowError {
    WindowNotFound,
    InvalidSize,
}

#[derive(Clone, Debug)]
struct WindowRecord {
    id: WindowId,
    app_id: AppId,
    surface_id: SurfaceId,
    title: String,
    frame_rect: Rect,
    content_rect: Rect,
    restore_rect: Rect,
    visible: bool,
    focused: bool,
    minimized: bool,
    maximized: bool,
    z_index: u32,
    workspace_id: WorkspaceId,
    layer_role: LayerRole,
    flags: WindowFlags,
    scene_node_id: SceneNodeId,
}

pub struct WindowManager {
    next_id: WindowId,
    next_scene_node_id: SceneNodeId,
    windows: BTreeMap<WindowId, WindowRecord>,
    focused: Option<WindowId>,
    next_z: u32,
}

impl WindowManager {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            next_scene_node_id: 1,
            windows: BTreeMap::new(),
            focused: None,
            next_z: 1,
        }
    }

    pub fn create_window(
        &mut self,
        app_id: AppId,
        surface_id: SurfaceId,
        title: &str,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<WindowId, WindowError> {
        self.create_window_with_meta(
            app_id,
            surface_id,
            title,
            x,
            y,
            width,
            height,
            0,
            LayerRole::Window,
            WindowFlags::default(),
        )
    }

    pub fn create_window_with_meta(
        &mut self,
        app_id: AppId,
        surface_id: SurfaceId,
        title: &str,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        workspace_id: WorkspaceId,
        layer_role: LayerRole,
        flags: WindowFlags,
    ) -> Result<WindowId, WindowError> {
        if width == 0 || height == 0 {
            return Err(WindowError::InvalidSize);
        }

        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let z_index = self.alloc_z();
        let scene_node_id = self.next_scene_node_id;
        self.next_scene_node_id = self.next_scene_node_id.saturating_add(1);
        let frame_rect = frame_rect_for_content(x, y, width, height, flags.decorate);
        let content_rect = frame_to_content_rect(frame_rect, flags.decorate);

        for window in self.windows.values_mut() {
            if window.layer_role == LayerRole::Window || layer_role == LayerRole::Window {
                window.focused = false;
            }
        }

        self.windows.insert(
            id,
            WindowRecord {
                id,
                app_id,
                surface_id,
                title: title.to_string(),
                frame_rect,
                content_rect,
                restore_rect: frame_rect,
                visible: true,
                focused: layer_role == LayerRole::Window,
                minimized: false,
                maximized: false,
                z_index,
                workspace_id,
                layer_role,
                flags,
                scene_node_id,
            },
        );
        if layer_role == LayerRole::Window {
            self.focused = Some(id);
        }
        Ok(id)
    }

    pub fn destroy_window(&mut self, window_id: WindowId) -> Option<WindowInfo> {
        let removed = self
            .windows
            .remove(&window_id)
            .map(|window| Self::as_info_record(&window));
        if self.focused == Some(window_id) {
            self.focused = None;
            self.focus_top_visible();
        }
        removed
    }

    pub fn destroy_windows_for_app(&mut self, app_id: AppId) -> Vec<WindowInfo> {
        let ids: Vec<WindowId> = self
            .windows
            .values()
            .filter(|w| w.app_id == app_id)
            .map(|w| w.id)
            .collect();
        ids.into_iter()
            .filter_map(|id| self.destroy_window(id))
            .collect()
    }

    pub fn set_window_frame(
        &mut self,
        window_id: WindowId,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<(Rect, Rect, Rect), WindowError> {
        if width == 0 || height == 0 {
            return Err(WindowError::InvalidSize);
        }

        let window = self
            .windows
            .get_mut(&window_id)
            .ok_or(WindowError::WindowNotFound)?;
        let old_frame = window.frame_rect;
        let old_content = window.content_rect;
        window.frame_rect = frame_rect_for_content(x, y, width, height, window.flags.decorate);
        window.content_rect = frame_to_content_rect(window.frame_rect, window.flags.decorate);
        window.visible = true;
        window.minimized = false;
        if !window.maximized {
            window.restore_rect = window.frame_rect;
        }
        Ok((old_frame, window.frame_rect, old_content))
    }

    pub fn focus_window(&mut self, window_id: WindowId) -> Result<Option<Rect>, WindowError> {
        if !self.windows.contains_key(&window_id) {
            return Err(WindowError::WindowNotFound);
        }

        let layer_role = self
            .windows
            .get(&window_id)
            .map(|window| window.layer_role)
            .ok_or(WindowError::WindowNotFound)?;
        if layer_role != LayerRole::Window {
            let new_z = self.alloc_z();
            let frame_rect = {
                let window = self
                    .windows
                    .get_mut(&window_id)
                    .ok_or(WindowError::WindowNotFound)?;
                window.visible = true;
                window.minimized = false;
                window.z_index = new_z;
                window.frame_rect
            };
            return Ok(Some(frame_rect));
        }

        let mut damage: Option<Rect> = None;
        if let Some(current) = self.focused {
            if current != window_id {
                if let Some(window) = self.windows.get_mut(&current) {
                    window.focused = false;
                    damage = Some(window.frame_rect);
                }
            }
        }

        let new_z = self.alloc_z();
        let window = self
            .windows
            .get_mut(&window_id)
            .ok_or(WindowError::WindowNotFound)?;
        window.focused = true;
        window.visible = true;
        window.minimized = false;
        window.z_index = new_z;
        damage = Some(match damage {
            Some(existing) => existing.union(&window.frame_rect),
            None => window.frame_rect,
        });
        self.focused = Some(window_id);
        Ok(damage)
    }

    pub fn set_visible(&mut self, window_id: WindowId, visible: bool) -> Result<Rect, WindowError> {
        let mut damage = self
            .windows
            .get(&window_id)
            .ok_or(WindowError::WindowNotFound)?
            .frame_rect;

        {
            let window = self
                .windows
                .get_mut(&window_id)
                .ok_or(WindowError::WindowNotFound)?;
            window.visible = visible;
            window.minimized = !visible;
            if !visible {
                window.focused = false;
            }
        }

        if !visible && self.focused == Some(window_id) {
            self.focused = None;
            if let Some(next_id) = self.top_visible_window_id() {
                if let Some(next) = self.windows.get_mut(&next_id) {
                    next.focused = true;
                    damage = damage.union(&next.frame_rect);
                    self.focused = Some(next_id);
                }
            }
        }

        Ok(damage)
    }

    pub fn set_title(&mut self, window_id: WindowId, title: &str) -> Result<Rect, WindowError> {
        let window = self
            .windows
            .get_mut(&window_id)
            .ok_or(WindowError::WindowNotFound)?;
        window.title = title.to_string();
        Ok(window.frame_rect)
    }

    pub fn toggle_maximize(
        &mut self,
        window_id: WindowId,
        work_area: Rect,
    ) -> Result<(Rect, Rect, Rect), WindowError> {
        let window = self
            .windows
            .get_mut(&window_id)
            .ok_or(WindowError::WindowNotFound)?;
        let old_frame = window.frame_rect;
        let old_content = window.content_rect;

        if window.maximized {
            window.frame_rect = window.restore_rect;
            window.content_rect = frame_to_content_rect(window.restore_rect, window.flags.decorate);
            window.maximized = false;
        } else {
            if !window.minimized {
                window.restore_rect = window.frame_rect;
            }
            let frame_rect = Rect::new(
                work_area.x,
                work_area.y,
                work_area.width.max(MIN_CONTENT_WIDTH),
                work_area.height.max(
                    MIN_CONTENT_HEIGHT
                        + if window.flags.decorate {
                            TITLEBAR_HEIGHT
                        } else {
                            0
                        },
                ),
            );
            window.frame_rect = frame_rect;
            window.content_rect = frame_to_content_rect(frame_rect, window.flags.decorate);
            window.maximized = true;
            window.visible = true;
            window.minimized = false;
        }

        Ok((old_frame, window.frame_rect, old_content))
    }

    pub fn window_surface(&self, window_id: WindowId) -> Option<SurfaceId> {
        self.windows.get(&window_id).map(|w| w.surface_id)
    }

    pub fn window_app(&self, window_id: WindowId) -> Option<AppId> {
        self.windows.get(&window_id).map(|w| w.app_id)
    }

    pub fn content_rect(&self, window_id: WindowId) -> Option<Rect> {
        self.windows.get(&window_id).map(|w| w.content_rect)
    }

    pub fn frame_rect(&self, window_id: WindowId) -> Option<Rect> {
        self.windows.get(&window_id).map(|w| w.frame_rect)
    }

    pub fn focused_window(&self) -> Option<WindowId> {
        self.focused
    }

    pub fn set_window_meta(
        &mut self,
        window_id: WindowId,
        workspace_id: WorkspaceId,
        layer_role: LayerRole,
        flags: WindowFlags,
    ) -> Result<(Rect, Rect), WindowError> {
        let old_frame;
        let new_frame;
        {
            let window = self
                .windows
                .get_mut(&window_id)
                .ok_or(WindowError::WindowNotFound)?;
            old_frame = window.frame_rect;
            window.workspace_id = workspace_id;
            window.layer_role = layer_role;
            window.flags = flags;
            window.frame_rect = frame_rect_for_content(
                window.content_rect.x,
                window.content_rect.y,
                window.content_rect.width,
                window.content_rect.height,
                flags.decorate,
            );
            window.content_rect = frame_to_content_rect(window.frame_rect, flags.decorate);
            if layer_role != LayerRole::Window {
                window.focused = false;
            }
            new_frame = window.frame_rect;
        }
        if layer_role != LayerRole::Window && self.focused == Some(window_id) {
            self.focused = None;
        }
        Ok((old_frame, new_frame))
    }

    pub fn set_window_workspace(
        &mut self,
        window_id: WindowId,
        workspace_id: WorkspaceId,
    ) -> Result<(), WindowError> {
        let window = self
            .windows
            .get_mut(&window_id)
            .ok_or(WindowError::WindowNotFound)?;
        window.workspace_id = workspace_id;
        Ok(())
    }

    pub fn list_windows(&self) -> Vec<WindowInfo> {
        self.windows.values().map(Self::as_info_record).collect()
    }

    pub fn ordered_windows(&self) -> Vec<WindowInfo> {
        let mut windows = self.list_windows();
        windows.sort_by_key(|w| (layer_rank(w.layer_role), w.z_index, w.id));
        windows
    }

    pub fn window_at(&self, point: Point) -> Option<WindowId> {
        let mut windows = self.ordered_windows();
        windows.reverse();
        windows
            .into_iter()
            .find(|w| w.visible && w.frame_rect.contains(point))
            .map(|w| w.id)
    }

    pub fn hit_test(&self, point: Point) -> Option<WindowHitTarget> {
        let mut windows = self.ordered_windows();
        windows.reverse();
        let mut background_hit = None;

        for window in windows.into_iter().filter(|window| window.visible) {
            if !window.frame_rect.contains(point) {
                continue;
            }

            if window.layer_role == LayerRole::Background {
                if window.content_rect.contains(point) {
                    background_hit = Some(WindowHitTarget::Content(window.id));
                }
                continue;
            }

            if !window.flags.decorate || window.layer_role != LayerRole::Window {
                return Some(WindowHitTarget::Content(window.id));
            }

            if let Some(button) = chrome_button_for_point(window.frame_rect, point) {
                return Some(WindowHitTarget::Chrome(window.id, button));
            }

            if let Some(edge) = resize_edge_for_point(window.frame_rect, point) {
                return Some(WindowHitTarget::Resize(window.id, edge));
            }

            let titlebar_rect = titlebar_rect(window.frame_rect);
            if titlebar_rect.contains(point) {
                return Some(WindowHitTarget::Titlebar(window.id));
            }

            if window.content_rect.contains(point) {
                return Some(WindowHitTarget::Content(window.id));
            }

            return Some(WindowHitTarget::Content(window.id));
        }

        background_hit
    }

    fn as_info_record(window: &WindowRecord) -> WindowInfo {
        WindowInfo {
            id: window.id,
            app_id: window.app_id,
            surface_id: window.surface_id,
            title: window.title.clone(),
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
            scene_root: None,
            semantic_root: None,
            buffer_mode: WindowBufferMode::Pixels,
        }
    }

    fn alloc_z(&mut self) -> u32 {
        let z = self.next_z;
        self.next_z = self.next_z.saturating_add(1);
        z
    }

    fn top_visible_window_id(&self) -> Option<WindowId> {
        self.windows
            .values()
            .filter(|window| window.visible && window.layer_role == LayerRole::Window)
            .max_by_key(|window| window.z_index)
            .map(|window| window.id)
    }

    fn focus_top_visible(&mut self) {
        for window in self.windows.values_mut() {
            window.focused = false;
        }

        if let Some(next_id) = self.top_visible_window_id() {
            if let Some(next) = self.windows.get_mut(&next_id) {
                next.focused = true;
                self.focused = Some(next_id);
            }
        }
    }
}

fn resize_edge_for_point(frame_rect: Rect, point: Point) -> Option<ResizeEdge> {
    let handle = RESIZE_HANDLE_SIZE as i32;
    let left = point.x < frame_rect.x.saturating_add(handle);
    let right = point.x >= frame_rect.right().saturating_sub(handle);
    let top = point.y < frame_rect.y.saturating_add(handle);
    let bottom = point.y >= frame_rect.bottom().saturating_sub(handle);

    match (left, right, top, bottom) {
        (true, _, true, _) => Some(ResizeEdge::TopLeft),
        (_, true, true, _) => Some(ResizeEdge::TopRight),
        (true, _, _, true) => Some(ResizeEdge::BottomLeft),
        (_, true, _, true) => Some(ResizeEdge::BottomRight),
        (true, _, _, _) => Some(ResizeEdge::Left),
        (_, true, _, _) => Some(ResizeEdge::Right),
        (_, _, true, _) => Some(ResizeEdge::Top),
        (_, _, _, true) => Some(ResizeEdge::Bottom),
        _ => None,
    }
}

pub fn titlebar_rect(frame_rect: Rect) -> Rect {
    Rect::new(
        frame_rect.x + BORDER_THICKNESS as i32,
        frame_rect.y + BORDER_THICKNESS as i32,
        frame_rect.width.saturating_sub(BORDER_THICKNESS * 2),
        TITLEBAR_HEIGHT.saturating_sub(BORDER_THICKNESS),
    )
}

pub fn chrome_button_rect(frame_rect: Rect, button: ChromeButton) -> Rect {
    let base_x = frame_rect
        .right()
        .saturating_sub(CHROME_BUTTON_RIGHT_PADDING)
        .saturating_sub(CHROME_BUTTON_SIZE as i32);
    let step = CHROME_BUTTON_SIZE as i32 + CHROME_BUTTON_GAP;
    let index = match button {
        ChromeButton::Close => 0,
        ChromeButton::Maximize => 1,
        ChromeButton::Minimize => 2,
    };

    Rect::new(
        base_x.saturating_sub(step.saturating_mul(index)),
        frame_rect.y.saturating_add(CHROME_BUTTON_TOP),
        CHROME_BUTTON_SIZE,
        CHROME_BUTTON_SIZE,
    )
}

pub fn chrome_button_hit_rect(frame_rect: Rect, button: ChromeButton) -> Rect {
    let visual = chrome_button_rect(frame_rect, button);
    let x = visual
        .x
        .saturating_sub((CHROME_BUTTON_HIT_WIDTH as i32 - visual.width as i32) / 2);
    let y = visual
        .y
        .saturating_sub((CHROME_BUTTON_HIT_HEIGHT as i32 - visual.height as i32) / 2);
    Rect::new(x, y, CHROME_BUTTON_HIT_WIDTH, CHROME_BUTTON_HIT_HEIGHT)
}

fn chrome_button_for_point(frame_rect: Rect, point: Point) -> Option<ChromeButton> {
    let close = chrome_button_hit_rect(frame_rect, ChromeButton::Close);
    if close.contains(point) {
        return Some(ChromeButton::Close);
    }

    let maximize = chrome_button_hit_rect(frame_rect, ChromeButton::Maximize);
    if maximize.contains(point) {
        return Some(ChromeButton::Maximize);
    }

    let minimize = chrome_button_hit_rect(frame_rect, ChromeButton::Minimize);
    if minimize.contains(point) {
        return Some(ChromeButton::Minimize);
    }

    None
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

fn frame_rect_for_content(x: i32, y: i32, width: u32, height: u32, decorate: bool) -> Rect {
    let chrome_height = if decorate { TITLEBAR_HEIGHT } else { 0 };
    Rect::new(x, y, width, height.saturating_add(chrome_height))
}

fn frame_to_content_rect(frame_rect: Rect, decorate: bool) -> Rect {
    if decorate {
        Rect::new(
            frame_rect.x,
            frame_rect.y.saturating_add(TITLEBAR_HEIGHT as i32),
            frame_rect.width,
            frame_rect.height.saturating_sub(TITLEBAR_HEIGHT),
        )
    } else {
        frame_rect
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_test_returns_background_content_when_no_higher_layer_matches() {
        let mut windows = WindowManager::new();
        let background = windows
            .create_window_with_meta(
                7,
                70,
                "Desktop",
                0,
                0,
                1280,
                720,
                0,
                LayerRole::Background,
                WindowFlags::layer_shell(),
            )
            .expect("background window should be created");

        assert_eq!(
            windows.hit_test(Point::new(40, 40)),
            Some(WindowHitTarget::Content(background))
        );
    }

    #[test]
    fn hit_test_prefers_top_bar_over_background_shell_surface() {
        let mut windows = WindowManager::new();
        let _background = windows
            .create_window_with_meta(
                7,
                70,
                "Desktop",
                0,
                0,
                1280,
                720,
                0,
                LayerRole::Background,
                WindowFlags::layer_shell(),
            )
            .expect("background window should be created");
        let top_bar = windows
            .create_window_with_meta(
                7,
                71,
                "Top Bar",
                18,
                18,
                1244,
                60,
                0,
                LayerRole::TopBar,
                WindowFlags::layer_shell(),
            )
            .expect("top bar should be created");

        assert_eq!(
            windows.hit_test(Point::new(32, 32)),
            Some(WindowHitTarget::Content(top_bar))
        );
    }
}
