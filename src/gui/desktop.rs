//! # echOS Masaüstü Ortamı (Desktop)
//!
//! Arkaplanı, görev çubuğunu (taskbar), dock'u ve pencereleri yönetir.
//! Pencere sürükleme, odaklama ve çizim işlemlerini koordine eder.
//! Menu Bar, Spotlight ve uygulama entegrasyonu içerir.

use super::theme::Theme;
use super::window::Window;
use super::dock::{Dock, DockAction, DockEvent, DockIcon};
use super::menu_bar::{MenuBar, MenuBarEvent, MenuAction};
use super::spotlight::{Spotlight, SpotlightEvent, SpotlightKey};
use super::apps::finder::FinderWindow;
use super::apps::browser::BrowserWindow;
use super::apps::terminal::TerminalWindow;
use super::apps::system_preferences::SystemPreferences;
use super::apps::preview::PreviewWindow;
use super::apps::activity_monitor::ActivityMonitor;
use super::apps::font_book::FontBookWindow;
use super::drag_drop::{DragDropManager, DragData, DragSource, DropEvent};
use super::clipboard::{ClipboardManager, ClipboardData};
use super::widgets::Rect;
use pc_keyboard::KeyCode;
use crate::gop::framebuffer::Framebuffer;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

// ============================================================================
// APPLICATION WINDOW
// ============================================================================

/// Application window types
pub enum AppWindow {
    Finder(FinderWindow),
    Browser(BrowserWindow),
    Terminal(TerminalWindow),
    Preferences(SystemPreferences),
    Preview(PreviewWindow),
    ActivityMonitor(ActivityMonitor),
    FontBook(FontBookWindow),
    Simple(Window<'static>),
}

impl AppWindow {
    pub fn draw(&self, fb: &mut Framebuffer) {
        match self {
            AppWindow::Finder(app) => app.draw(fb),
            AppWindow::Browser(app) => app.draw(fb),
            AppWindow::Terminal(app) => app.draw(fb),
            AppWindow::Preferences(app) => app.draw(fb),
            AppWindow::Preview(app) => app.draw(fb),
            AppWindow::ActivityMonitor(app) => app.draw(fb),
            AppWindow::FontBook(app) => app.draw(fb),
            AppWindow::Simple(win) => win.draw(fb),
        }
    }
    
    pub fn get_rect(&self) -> (usize, usize, usize, usize) {
        match self {
            AppWindow::Finder(app) => (app.rect.x as usize, app.rect.y as usize, app.rect.width as usize, app.rect.height as usize),
            AppWindow::Browser(app) => (app.rect.x as usize, app.rect.y as usize, app.rect.width as usize, app.rect.height as usize),
            AppWindow::Terminal(app) => (app.rect.x as usize, app.rect.y as usize, app.rect.width as usize, app.rect.height as usize),
            AppWindow::Preferences(app) => (app.rect.x as usize, app.rect.y as usize, app.rect.width as usize, app.rect.height as usize),
            AppWindow::Preview(app) => (app.rect.x as usize, app.rect.y as usize, app.rect.width as usize, app.rect.height as usize),
            AppWindow::ActivityMonitor(app) => (app.rect.x as usize, app.rect.y as usize, app.rect.width as usize, app.rect.height as usize),
            AppWindow::FontBook(app) => (app.rect.x as usize, app.rect.y as usize, app.rect.width as usize, app.rect.height as usize),
            AppWindow::Simple(win) => (win.x, win.y, win.width, win.height),
        }
    }
    
    pub fn get_title(&self) -> &str {
        match self {
            AppWindow::Finder(app) => {
                if let Some(tab) = app.tabs.get(app.active_tab) {
                    &tab.title
                } else {
                    "Finder"
                }
            }
            AppWindow::Browser(app) => {
                if let Some(tab) = app.tabs.get(app.active_tab) {
                    &tab.title
                } else {
                    "Browser"
                }
            }
            AppWindow::Terminal(app) => {
                if let Some(tab) = app.tabs.get(app.active_tab) {
                    &tab.title
                } else {
                    "Terminal"
                }
            }
            AppWindow::Preferences(app) => &app.title,
            AppWindow::Preview(_) => "Preview",
            AppWindow::ActivityMonitor(_) => "Activity Monitor",
            AppWindow::FontBook(_) => "Font Book",
            AppWindow::Simple(win) => &win.title,
        }
    }
    
    pub fn set_position(&mut self, x: usize, y: usize) {
        match self {
            AppWindow::Finder(app) => { app.rect.x = x as i32; app.rect.y = y as i32; }
            AppWindow::Browser(app) => { app.rect.x = x as i32; app.rect.y = y as i32; }
            AppWindow::Terminal(app) => { app.rect.x = x as i32; app.rect.y = y as i32; }
            AppWindow::Preferences(app) => { app.rect.x = x as i32; app.rect.y = y as i32; }
            AppWindow::Preview(app) => { app.rect.x = x as i32; app.rect.y = y as i32; }
            AppWindow::ActivityMonitor(app) => { app.rect.x = x as i32; app.rect.y = y as i32; }
            AppWindow::FontBook(app) => { app.rect.x = x as i32; app.rect.y = y as i32; }
            AppWindow::Simple(win) => { win.x = x; win.y = y; }
        }
    }

    pub fn set_size(&mut self, width: usize, height: usize) {
        match self {
            AppWindow::Finder(app) => { app.rect.width = width as i32; app.rect.height = height as i32; }
            AppWindow::Browser(app) => { app.rect.width = width as i32; app.rect.height = height as i32; }
            AppWindow::Terminal(app) => { app.rect.width = width as i32; app.rect.height = height as i32; }
            AppWindow::Preferences(app) => { app.rect.width = width as i32; app.rect.height = height as i32; }
            AppWindow::Preview(app) => { app.rect.width = width as i32; app.rect.height = height as i32; }
            AppWindow::ActivityMonitor(app) => { app.rect.width = width as i32; app.rect.height = height as i32; }
            AppWindow::FontBook(app) => { app.rect.width = width as i32; app.rect.height = height as i32; }
            AppWindow::Simple(win) => { win.width = width; win.height = height; }
        }
    }
    
    pub fn on_click(&mut self, mx: i32, my: i32) -> bool {
        match self {
            AppWindow::Simple(win) => {
                win.on_click(mx, my);
                true
            }
            _ => true, // Other apps handle clicks internally
        }
    }
    
    pub fn on_key(&mut self, c: char) -> bool {
        match self {
            AppWindow::Terminal(app) => {
                app.on_key_press(c);
                true
            }
            AppWindow::Simple(win) => {
                win.on_key(c, 0, 0);
                true
            }
            _ => true,
        }
    }
    
    pub fn update(&mut self, dt: f32) {
        match self {
            AppWindow::ActivityMonitor(app) => app.update(dt),
            AppWindow::Preview(app) => app.update(dt),
            _ => {}
        }
    }
}

// ============================================================================
// DESKTOP
// ============================================================================

/// Masaüstü Yöneticisi
pub struct Desktop {
    width: usize,
    height: usize,
    /// Application windows
    app_windows: Vec<AppWindow>,
    /// Simple windows (legacy)
    windows: Vec<Window<'static>>,
    taskbar_height: usize,
    
    // GUI Components
    dock: Dock,
    menu_bar: MenuBar,
    spotlight: Spotlight,
    drag_drop: DragDropManager,
    clipboard: ClipboardManager,
    
    // State
    dragging_window_idx: Option<usize>,
    drag_start_offset: (i32, i32),
    resizing_window_idx: Option<usize>,
    resize_start_mouse: (i32, i32),
    resize_start_size: (usize, usize),
    resize_handle_hover_idx: Option<usize>,
    last_mouse_left: bool,
    active_window_idx: Option<usize>,
    spotlight_open: bool,
    menu_open: bool,
    last_time: f32,
    window_damage_rects: Vec<Rect>,
    last_menu_mouse_pos: Option<(i32, i32)>,
    last_dock_mouse_pos: Option<(i32, i32)>,
}

impl Desktop {
    pub fn new(width: usize, height: usize) -> Self {
        Desktop {
            width,
            height,
            app_windows: Vec::new(),
            windows: Vec::new(),
            taskbar_height: 40,
            dock: Dock::new(width, height),
            menu_bar: MenuBar::new(width),
            spotlight: Spotlight::new(width, height),
            drag_drop: DragDropManager::new(),
            clipboard: ClipboardManager::new(),
            dragging_window_idx: None,
            drag_start_offset: (0, 0),
            resizing_window_idx: None,
            resize_start_mouse: (0, 0),
            resize_start_size: (0, 0),
            resize_handle_hover_idx: None,
            last_mouse_left: false,
            active_window_idx: None,
            spotlight_open: false,
            menu_open: false,
            last_time: 0.0,
            window_damage_rects: Vec::new(),
            last_menu_mouse_pos: None,
            last_dock_mouse_pos: None,
        }
    }

    fn add_window_damage(&mut self, rect: Rect) {
        let mut merged = rect;
        let mut index = 0;
        while index < self.window_damage_rects.len() {
            if merged.intersects(&self.window_damage_rects[index]) {
                merged = merged.union(&self.window_damage_rects[index]);
                self.window_damage_rects.swap_remove(index);
            } else {
                index += 1;
            }
        }
        self.window_damage_rects.push(merged);
    }

    fn add_fullscreen_damage(&mut self) {
        self.add_window_damage(Rect::new(0, 0, self.width as i32, self.height as i32));
    }

    fn window_rect_at(&self, idx: usize) -> Option<Rect> {
        self.app_windows.get(idx).map(|window| {
            let (x, y, width, height) = window.get_rect();
            Rect::new(x as i32, y as i32, width as i32, height as i32)
        })
    }

    fn remap_index_after_move(index: Option<usize>, from: usize, to: usize) -> Option<usize> {
        let idx = index?;
        if idx == from {
            return Some(to);
        }

        if from < to {
            if idx > from && idx <= to {
                return Some(idx - 1);
            }
        } else if from > to && idx >= to && idx < from {
            return Some(idx + 1);
        }

        Some(idx)
    }

    fn remap_index_after_remove(index: Option<usize>, removed: usize) -> Option<usize> {
        let idx = index?;
        if idx == removed {
            return None;
        }
        if idx > removed {
            return Some(idx - 1);
        }
        Some(idx)
    }

    fn bring_window_to_front(&mut self, idx: usize) -> Option<usize> {
        if idx >= self.app_windows.len() {
            return None;
        }

        let top_idx = self.app_windows.len().saturating_sub(1);
        if idx == top_idx {
            self.active_window_idx = Some(idx);
            return Some(idx);
        }

        let window = self.app_windows.remove(idx);
        self.app_windows.push(window);
        let new_idx = self.app_windows.len().saturating_sub(1);
        self.dragging_window_idx = Self::remap_index_after_move(self.dragging_window_idx, idx, new_idx);
        self.resizing_window_idx = Self::remap_index_after_move(self.resizing_window_idx, idx, new_idx);
        self.resize_handle_hover_idx = Self::remap_index_after_move(self.resize_handle_hover_idx, idx, new_idx);
        self.active_window_idx = Some(new_idx);
        Some(new_idx)
    }

    fn is_in_resize_handle(&self, x: i32, y: i32, wx: usize, wy: usize, ww: usize, wh: usize) -> bool {
        const RESIZE_HANDLE_SIZE: usize = 14;

        if ww < RESIZE_HANDLE_SIZE || wh < RESIZE_HANDLE_SIZE {
            return false;
        }

        let handle_x = wx + ww - RESIZE_HANDLE_SIZE;
        let handle_y = wy + wh - RESIZE_HANDLE_SIZE;
        x >= handle_x as i32
            && y >= handle_y as i32
            && x < (wx + ww) as i32
            && y < (wy + wh) as i32
    }

    fn clamp_window_size_for_screen(&self, wx: usize, wy: usize, width: usize, height: usize) -> (usize, usize) {
        const MIN_WINDOW_WIDTH: usize = 320;
        const MIN_WINDOW_HEIGHT: usize = 220;

        let max_w = self.width.saturating_sub(wx).max(MIN_WINDOW_WIDTH);
        let max_h = self.height.saturating_sub(wy).max(MIN_WINDOW_HEIGHT);

        let clamped_w = width.clamp(MIN_WINDOW_WIDTH, max_w);
        let clamped_h = height.clamp(MIN_WINDOW_HEIGHT, max_h);
        (clamped_w, clamped_h)
    }

    fn set_resize_hover_idx(&mut self, new_hover: Option<usize>) -> bool {
        if self.resize_handle_hover_idx == new_hover {
            return false;
        }

        if let Some(prev_idx) = self.resize_handle_hover_idx {
            if let Some(prev_rect) = self.window_rect_at(prev_idx) {
                self.add_window_damage(prev_rect);
            }
        }

        if let Some(new_idx) = new_hover {
            if let Some(new_rect) = self.window_rect_at(new_idx) {
                self.add_window_damage(new_rect);
            }
        }

        self.resize_handle_hover_idx = new_hover;
        true
    }

    pub fn take_window_damage(&mut self) -> Option<Vec<Rect>> {
        if self.window_damage_rects.is_empty() {
            None
        } else {
            Some(core::mem::take(&mut self.window_damage_rects))
        }
    }
    
    /// Add simple window (legacy)
    pub fn add_window(&mut self, window: Window<'static>) {
        self.windows.push(window);
    }
    
    /// Launch application by ID
    pub fn launch_app(&mut self, app_id: &str) {
        let x = 100 + self.app_windows.len() * 30;
        let y = 80 + self.app_windows.len() * 30;
        
        match app_id {
            "finder" => {
                let finder = FinderWindow::new(super::Rect { x: x as i32, y: y as i32, width: 800, height: 500 });
                self.app_windows.push(AppWindow::Finder(finder));
            }
            "safari" | "browser" => {
                let browser = BrowserWindow::new(super::Rect { x: x as i32, y: y as i32, width: 900, height: 600 });
                self.app_windows.push(AppWindow::Browser(browser));
            }
            "terminal" => {
                let term = TerminalWindow::new(super::Rect { x: x as i32, y: y as i32, width: 700, height: 450 });
                self.app_windows.push(AppWindow::Terminal(term));
            }
            "settings" | "preferences" => {
                let prefs = SystemPreferences::new(super::Rect { x: x as i32, y: y as i32, width: 750, height: 550 });
                self.app_windows.push(AppWindow::Preferences(prefs));
            }
            "preview" => {
                let preview = PreviewWindow::new(super::Rect { x: x as i32, y: y as i32, width: 800, height: 600 });
                self.app_windows.push(AppWindow::Preview(preview));
            }
            "activity" => {
                let monitor = ActivityMonitor::new(super::Rect { x: x as i32, y: y as i32, width: 800, height: 600 });
                self.app_windows.push(AppWindow::ActivityMonitor(monitor));
            }
            "fontbook" => {
                let fontbook = FontBookWindow::new(super::Rect { x: x as i32, y: y as i32, width: 800, height: 600 });
                self.app_windows.push(AppWindow::FontBook(fontbook));
            }
            _ => {
                // Create simple window for unknown apps
                let mut win = Window::new(x, y, 400, 300, app_id);
                win.add_line(&format!("Application: {}", app_id));
                self.app_windows.push(AppWindow::Simple(win));
            }
        }
        
        // Set as active
        let new_idx = self.app_windows.len() - 1;
        self.active_window_idx = Some(new_idx);
        if let Some(new_rect) = self.window_rect_at(new_idx) {
            self.add_window_damage(new_rect);
        }
    }
    
    /// Close window by index
    pub fn close_window(&mut self, idx: usize) {
        if idx < self.app_windows.len() {
            if let Some(closed_rect) = self.window_rect_at(idx) {
                self.add_window_damage(closed_rect);
            }

            self.app_windows.remove(idx);
            let was_active = self.active_window_idx == Some(idx);
            self.dragging_window_idx = Self::remap_index_after_remove(self.dragging_window_idx, idx);
            self.resizing_window_idx = Self::remap_index_after_remove(self.resizing_window_idx, idx);
            self.resize_handle_hover_idx = Self::remap_index_after_remove(self.resize_handle_hover_idx, idx);

            if was_active {
                self.active_window_idx = self.app_windows.len().checked_sub(1);
            } else {
                self.active_window_idx = Self::remap_index_after_remove(self.active_window_idx, idx);
            }

            if let Some(active_idx) = self.active_window_idx {
                if let Some(active_rect) = self.window_rect_at(active_idx) {
                    self.add_window_damage(active_rect);
                }
            }
        }
    }

    /// Tüm masaüstünü çizer.
    pub fn draw(&self, fb: &mut Framebuffer) {
        // Arkaplan
        self.draw_background(fb);
        
        // Menu Bar (top)
        self.menu_bar.draw(fb);
        
        // Pencereler (app windows)
        for (i, window) in self.app_windows.iter().enumerate() {
            window.draw(fb);
        }
        
        // Simple windows (legacy)
        for window in &self.windows {
            window.draw(fb);
        }
        
        // Dock (bottom)
        self.dock.draw(fb);
        
        // Spotlight (overlay)
        if self.spotlight_open {
            self.spotlight.draw(fb);
        }
        
        // Drag & drop overlay
        self.drag_drop.draw(fb);
    }

    fn draw_background(&self, fb: &mut Framebuffer) {
        // Koyu mor arkaplan
        let bg_color = Theme::DESKTOP_BG.to_u32();
        fb.clear(bg_color);

        // Izgara Deseni (Grid)
        let grid_color = 0x4a2b7e; // Açık mor
        let grid_size = 40;
        let menu_bar_height = 25;

        // Dikey çizgiler
        for x in (0..self.width).step_by(grid_size) {
            for y in menu_bar_height..(self.height - self.dock_height()) {
                fb.plot_pixel(x, y, grid_color);
            }
        }

        // Yatay çizgiler
        for y in (menu_bar_height..(self.height - self.dock_height())).step_by(grid_size) {
            for x in 0..self.width {
                fb.plot_pixel(x, y, grid_color);
            }
        }
    }
    
    fn dock_height(&self) -> usize {
        70 // DOCK_HEIGHT constant
    }

    /// Mouse hareketlerini ve tıklamalarını yönetir.
    pub fn update_mouse(&mut self, x: i32, y: i32, left_down: bool) -> bool {
        let mut redraw = false;
        
        // 1. Spotlight açık mı kontrol et
        if self.spotlight_open {
            return true; // Spotlight handle edecek
        }
        
        // 2. Menu Bar kontrolü
        if y >= 0 && y < 25 {
            self.last_dock_mouse_pos = None;
            if left_down && !self.last_mouse_left {
                let event = self.menu_bar.on_mouse_down(x, y);
                match event {
                    MenuBarEvent::MenuItemSelected(action) => {
                        self.handle_menu_action(action);
                        redraw = true;
                    }
                    MenuBarEvent::RightItemClicked(action) => {
                        self.handle_right_item_action(action);
                        redraw = true;
                    }
                    _ => {}
                }
            } else {
                let moved = self.last_menu_mouse_pos != Some((x, y));
                if moved {
                    self.last_menu_mouse_pos = Some((x, y));
                    if !matches!(self.menu_bar.on_mouse_move(x, y), MenuBarEvent::None) {
                        redraw = true;
                    }
                    redraw = true;
                }
            }
            self.last_mouse_left = left_down;
            return redraw;
        }
        
        // 3. Dock kontrolü
        let dock_y = self.height - self.dock_height();
        if y >= dock_y as i32 {
            self.last_menu_mouse_pos = None;
            // Hover/magnification state update
            let moved = self.last_dock_mouse_pos != Some((x, y));
            if moved {
                self.last_dock_mouse_pos = Some((x, y));
                if !matches!(self.dock.on_mouse_move(x, y), DockEvent::None) {
                    redraw = true;
                }
                redraw = true;
            }

            if left_down && !self.last_mouse_left {
                // Press starts bounce/click state
                if !matches!(self.dock.on_mouse_down(x, y), DockEvent::None) {
                    redraw = true;
                }
            } else if !left_down && self.last_mouse_left {
                // Release activates action if still hovering the same item
                if let DockEvent::ItemActivated(_, _, action) = self.dock.on_mouse_up() {
                    self.handle_dock_action(action);
                    redraw = true;
                }
            }
            self.last_mouse_left = left_down;
            return redraw;
        }

        self.last_menu_mouse_pos = None;
        self.last_dock_mouse_pos = None;
        
        let just_pressed = left_down && !self.last_mouse_left;
        let just_released = !left_down && self.last_mouse_left;

        // 4. Bırakma (Release)
        if just_released {
            self.dragging_window_idx = None;
            self.resizing_window_idx = None;
        }

        // 5. Resize (bottom-right handle)
        if let Some(idx) = self.resizing_window_idx {
            if left_down {
                let (win_x, win_y, win_w, win_h) = self.app_windows[idx].get_rect();
                let dx = x - self.resize_start_mouse.0;
                let dy = y - self.resize_start_mouse.1;

                let target_w = ((self.resize_start_size.0 as i32) + dx).max(1) as usize;
                let target_h = ((self.resize_start_size.1 as i32) + dy).max(1) as usize;
                let (new_w, new_h) = self.clamp_window_size_for_screen(win_x, win_y, target_w, target_h);

                if win_w != new_w || win_h != new_h {
                    let old_rect = Rect::new(win_x as i32, win_y as i32, win_w as i32, win_h as i32);
                    self.app_windows[idx].set_size(new_w, new_h);
                    let new_rect = Rect::new(win_x as i32, win_y as i32, new_w as i32, new_h as i32);
                    self.add_window_damage(old_rect.union(&new_rect));
                    redraw = true;
                }
            } else {
                self.resizing_window_idx = None;
            }
        }
        // 6. Sürükleme (Dragging)
        else if let Some(idx) = self.dragging_window_idx {
            if left_down {
                let (win_x, win_y, win_w, win_h) = self.app_windows[idx].get_rect();
                let max_x = self.width.saturating_sub(win_w);
                let min_y = 25usize;
                let max_y = self.height.saturating_sub(win_h).max(min_y);
                let new_x = (x - self.drag_start_offset.0).clamp(0, max_x as i32) as usize;
                let new_y = (y - self.drag_start_offset.1).clamp(min_y as i32, max_y as i32) as usize;

                if win_x != new_x || win_y != new_y {
                    let old_rect = Rect::new(win_x as i32, win_y as i32, win_w as i32, win_h as i32);
                    self.app_windows[idx].set_position(new_x, new_y);
                    let new_rect = Rect::new(new_x as i32, new_y as i32, win_w as i32, win_h as i32);
                    self.add_window_damage(old_rect.union(&new_rect));
                    redraw = true;
                }
            } else {
                self.dragging_window_idx = None;
            }
        }
        // 7. Yeni Tıklama (Click / Drag Start / Resize / Focus)
        else if just_pressed {
            // Pencereleri sondan başa (üstten alta) kontrol et
            let mut hit_idx = None;
            for (i, window) in self.app_windows.iter().enumerate().rev() {
                let (wx, wy, ww, wh) = window.get_rect();
                if x >= wx as i32 && x < (wx + ww) as i32
                    && y >= wy as i32 && y < (wy + wh) as i32 {
                    hit_idx = Some(i);
                    break;
                }
            }

            if let Some(idx) = hit_idx {
                // Pencereyi en öne getir
                let clicked_rect = self.window_rect_at(idx);
                let titlebar_height = 28; // Default titlebar height
                let prev_active = self.active_window_idx;
                let active_idx = self.bring_window_to_front(idx).unwrap_or(idx);
                let (wx, wy, ww, wh) = self.app_windows[active_idx].get_rect();
                let _ = self.set_resize_hover_idx(None);

                if prev_active != self.active_window_idx {
                    if let Some(prev_idx) = prev_active {
                        if let Some(prev_rect) = self.window_rect_at(prev_idx) {
                            self.add_window_damage(prev_rect);
                        }
                    }
                    if let Some(curr_rect) = clicked_rect {
                        self.add_window_damage(curr_rect);
                    }
                }
                
                // Titlebar'dan sürükleme başlat
                if y >= wy as i32 && y < (wy + titlebar_height) as i32 {
                    self.dragging_window_idx = Some(active_idx);
                    self.drag_start_offset = (x - wx as i32, y - wy as i32);
                } else if self.is_in_resize_handle(x, y, wx, wy, ww, wh) {
                    self.resizing_window_idx = Some(active_idx);
                    self.resize_start_mouse = (x, y);
                    self.resize_start_size = (ww, wh);
                    let _ = self.set_resize_hover_idx(Some(active_idx));
                } else {
                    // İçerik tıklaması
                    self.app_windows[active_idx].on_click(x, y);
                }
                
                redraw = true;
            }
        }

        if self.dragging_window_idx.is_none() && self.resizing_window_idx.is_none() {
            let mut hovered_resize: Option<usize> = None;
            for (i, window) in self.app_windows.iter().enumerate().rev() {
                let (wx, wy, ww, wh) = window.get_rect();
                let in_window = x >= wx as i32
                    && x < (wx + ww) as i32
                    && y >= wy as i32
                    && y < (wy + wh) as i32;
                if in_window {
                    if self.is_in_resize_handle(x, y, wx, wy, ww, wh) {
                        hovered_resize = Some(i);
                    }
                    break;
                }
            }

            if self.set_resize_hover_idx(hovered_resize) {
                redraw = true;
            }
        }

        self.last_mouse_left = left_down;
        redraw
    }
    
    /// Handle dock action
    fn handle_dock_action(&mut self, action: DockAction) {
        match action {
            DockAction::LaunchApp(app_id) => {
                self.launch_app(&app_id);
            }
            DockAction::ShowLaunchpad => {
                self.spotlight.show();
                self.spotlight_open = true;
                self.add_fullscreen_damage();
            }
            DockAction::OpenFolder(path) => {
                self.launch_app("finder");
            }
            _ => {}
        }
    }
    
    /// Handle menu action
    fn handle_menu_action(&mut self, action: MenuAction) {
        match action {
            MenuAction::NewFile => self.launch_app("textedit"),
            MenuAction::OpenFile => self.launch_app("finder"),
            MenuAction::Preferences => self.launch_app("settings"),
            MenuAction::About => {
                // Show about dialog
            }
            MenuAction::Quit => {
                // Close active window
                if let Some(idx) = self.active_window_idx {
                    self.close_window(idx);
                }
            }
            _ => {}
        }
    }
    
    /// Handle right item action (menu bar)
    fn handle_right_item_action(&mut self, action: super::menu_bar::RightItemAction) {
        match action {
            super::menu_bar::RightItemAction::OpenSpotlight => {
                self.spotlight.show();
                self.spotlight_open = true;
                self.add_fullscreen_damage();
            }
            super::menu_bar::RightItemAction::OpenControlCenter => {
                self.launch_app("settings");
            }
            _ => {}
        }
    }
    
    /// Handle keyboard input
    pub fn on_key(&mut self, c: char) -> bool {
        // Spotlight açık mı?
        if self.spotlight_open {
            let event = self.spotlight.on_key_press(c);
            match event {
                SpotlightEvent::ResultSelected(result) => {
                    // Launch selected result
                    match result.action {
                        super::spotlight::SearchAction::LaunchApp(app_id) => {
                            self.launch_app(&app_id);
                        }
                        super::spotlight::SearchAction::OpenFile(path) => {
                            self.launch_app("preview");
                        }
                        super::spotlight::SearchAction::OpenFolder(path) => {
                            self.launch_app("finder");
                        }
                        super::spotlight::SearchAction::OpenSetting(name) => {
                            self.launch_app("settings");
                        }
                        _ => {}
                    }
                    self.spotlight_open = false;
                    self.add_fullscreen_damage();
                    return true;
                }
                SpotlightEvent::Cancelled => {
                    self.spotlight_open = false;
                    self.add_fullscreen_damage();
                    return true;
                }
                _ => return true,
            }
        }
        
        // Aktif pencereye gönder
        if let Some(idx) = self.active_window_idx {
            if c == 'q' || c == 'Q' {
                // Cmd+Q simulation - close window
                // For now just close on 'q'
            } else if c == 'w' || c == 'W' {
                // Cmd+W - close tab/window
            } else {
                self.app_windows[idx].on_key(c);
            }
            if let Some(active_rect) = self.window_rect_at(idx) {
                self.add_window_damage(active_rect);
            }
            return true;
        }
        
        false
    }
    
    /// Handle special key
    pub fn on_special_key(&mut self, key: KeyCode) -> bool {
        // Spotlight toggle on special key (simulating Cmd+Space)
        if key == KeyCode::Spacebar {
            if !self.spotlight_open {
                self.spotlight.show();
                self.spotlight_open = true;
                self.add_fullscreen_damage();
                return true;
            }
        }
        
        if self.spotlight_open {
            let spotlight_key = match key {
                KeyCode::ArrowUp => SpotlightKey::Up,
                KeyCode::ArrowDown => SpotlightKey::Down,
                KeyCode::Return => SpotlightKey::Enter,
                KeyCode::Escape => SpotlightKey::Escape,
                KeyCode::Tab => SpotlightKey::Tab,
                _ => return false,
            };
            
            let event = self.spotlight.on_special_key(spotlight_key);
            match event {
                SpotlightEvent::ResultSelected(result) => {
                    if let super::spotlight::SearchAction::LaunchApp(app_id) = result.action {
                        self.launch_app(&app_id);
                    }
                    self.spotlight_open = false;
                    self.add_fullscreen_damage();
                    return true;
                }
                SpotlightEvent::Cancelled => {
                    self.spotlight_open = false;
                    self.add_fullscreen_damage();
                    return true;
                }
                _ => return true,
            }
        }
        
        false
    }
    
    /// Update desktop state
    pub fn update(&mut self, dt: f32) -> bool {
        let mut needs_redraw = false;

        let old_rects: Vec<(usize, Rect)> = self.app_windows
            .iter()
            .enumerate()
            .map(|(idx, window)| {
                let (x, y, width, height) = window.get_rect();
                (idx, Rect::new(x as i32, y as i32, width as i32, height as i32))
            })
            .collect();
        
        // Update dock animation
        needs_redraw |= self.dock.update(dt);
        
        // Update spotlight animation
        self.spotlight.update(dt);
        if self.spotlight.needs_redraw() {
            needs_redraw = true;
        }
        
        // Update app windows
        for window in &mut self.app_windows {
            window.update(dt);
        }

        for (idx, old_rect) in old_rects {
            if let Some(new_rect) = self.window_rect_at(idx) {
                let rect_changed = old_rect.x != new_rect.x
                    || old_rect.y != new_rect.y
                    || old_rect.width != new_rect.width
                    || old_rect.height != new_rect.height;
                if rect_changed {
                    self.add_window_damage(old_rect.union(&new_rect));
                    needs_redraw = true;
                }
            }
        }
        
        // Update drag & drop
        if let Some(event) = self.drag_drop.update(dt) {
            match event {
                DropEvent::SpringLoaded(target_id) => {
                    // Open folder
                }
                _ => {}
            }
            needs_redraw = true;
        }
        
        self.last_time += dt;
        needs_redraw
    }

    pub fn on_click(&mut self, x: i32, y: i32) -> bool {
        self.update_mouse(x, y, true)
    }

    pub fn windows(&self) -> &Vec<Window<'static>> {
        &self.windows
    }
    
    pub fn app_windows(&self) -> &Vec<AppWindow> {
        &self.app_windows
    }
    
    pub fn active_window(&self) -> Option<&AppWindow> {
        self.active_window_idx.and_then(|idx| self.app_windows.get(idx))
    }

    pub fn active_window_rect(&self) -> Option<Rect> {
        self.active_window_idx.and_then(|idx| self.window_rect_at(idx))
    }
}

/// Run desktop main loop (standalone function)
pub fn run(fb: &mut Framebuffer) -> ! {
    let width = fb.width;
    let height = fb.height;
    
    // Enable double buffering for smooth rendering
    fb.enable_double_buffering();
    
    let mut desktop = Desktop::new(width, height);
    
    // Launch default apps
    desktop.launch_app("finder");
    
    crate::serial_println!("[GUI] Desktop initialized ({}x{}), entering main loop", width, height);
    
    // Initialize PS/2 keyboard and mouse
    crate::drivers::ps2::init();
    crate::drivers::mouse::init();
    crate::keyboard::mark_tty_ready();
    
    let mut frame_count = 0u32;
    let mut last_frame_time = 0u64;
    const TARGET_FPS: u64 = 60;
    const FRAME_TIME_US: u64 = 1_000_000 / TARGET_FPS; // ~16.67ms per frame
    
    loop {
        // Get current time (simple tick counter for now)
        let current_time = unsafe { 
            // Read TSC as rough time source
            let mut tsc: u64;
            core::arch::asm!("rdtsc", out("rax") tsc, options(nomem, nostack));
            tsc
        };
        
        // Frame rate limiting - skip if too soon
        // Note: TSC frequency varies, this is approximate
        let elapsed = current_time.wrapping_sub(last_frame_time);
        if frame_count > 0 && elapsed < 500_000 { // Rough ~60fps on typical CPU
            // Process input but skip render
            // Process keyboard input
            while let Some(key) = crate::keyboard::read_key() {
                match key {
                    pc_keyboard::DecodedKey::Unicode(c) => {
                        desktop.on_key(c);
                    }
                    pc_keyboard::DecodedKey::RawKey(kc) => {
                        desktop.on_special_key(kc);
                    }
                }
            }
            
            // Process mouse input
            while let Some(event) = crate::drivers::input::pop_event() {
                if let crate::drivers::input::InputEvent::MouseByte(byte) = event {
                    crate::drivers::mouse::handle_packet(byte);
                }
            }
            continue;
        }
        last_frame_time = current_time;
        
        // Process keyboard input
        while let Some(key) = crate::keyboard::read_key() {
            match key {
                pc_keyboard::DecodedKey::Unicode(c) => {
                    desktop.on_key(c);
                }
                pc_keyboard::DecodedKey::RawKey(kc) => {
                    desktop.on_special_key(kc);
                }
            }
        }
        
        // Process mouse input from interrupt queue
        while let Some(event) = crate::drivers::input::pop_event() {
            match event {
                crate::drivers::input::InputEvent::MouseByte(byte) => {
                    // Process raw mouse byte
                    crate::drivers::mouse::handle_packet(byte);
                }
                crate::drivers::input::InputEvent::Mouse(packet) => {
                    // Already processed packet
                }
                _ => {}
            }
        }
        
        // Get mouse position and buttons
        let (mx, my) = crate::drivers::mouse::get_position();
        let buttons = crate::drivers::mouse::get_buttons();
        
        // Handle mouse click
        static mut LAST_CLICK: bool = false;
        unsafe {
            if buttons.left && !LAST_CLICK {
                desktop.on_click(mx, my);
            }
            LAST_CLICK = buttons.left;
        }
        
        // Draw desktop
        desktop.draw(fb);
        
        // Draw mouse cursor on top
        crate::gui::cursor::draw(fb);
        
        // Swap back buffer to front buffer (double buffering)
        fb.swap_buffers();
        
        frame_count += 1;
        if frame_count % 60 == 0 {
            // Log every 60 frames (~1 second at 60fps)
            crate::serial_println!("[GUI] Frame {} mouse=({},{})", frame_count, mx, my);
        }
    }
}
