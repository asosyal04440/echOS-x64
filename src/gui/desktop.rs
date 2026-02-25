//! # echOS Masaüstü Ortamı (Desktop)
//!
//! Arkaplanı, görev çubuğunu (taskbar), dock'u ve pencereleri yönetir.
//! Pencere sürükleme, odaklama ve çizim işlemlerini koordine eder.
//! Menu Bar, Spotlight ve uygulama entegrasyonu içerir.

use super::theme::Theme;
use super::window::Window;
use super::dock::{Dock, DockAction, DockIcon};
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
    last_mouse_left: bool,
    active_window_idx: Option<usize>,
    spotlight_open: bool,
    menu_open: bool,
    last_time: f32,
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
            last_mouse_left: false,
            active_window_idx: None,
            spotlight_open: false,
            menu_open: false,
            last_time: 0.0,
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
        self.active_window_idx = Some(self.app_windows.len() - 1);
    }
    
    /// Close window by index
    pub fn close_window(&mut self, idx: usize) {
        if idx < self.app_windows.len() {
            self.app_windows.remove(idx);
            if self.active_window_idx == Some(idx) {
                self.active_window_idx = self.app_windows.len().checked_sub(1);
            } else if let Some(active) = self.active_window_idx {
                if active > idx {
                    self.active_window_idx = Some(active - 1);
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
                self.menu_bar.on_mouse_move(x, y);
            }
            self.last_mouse_left = left_down;
            return redraw;
        }
        
        // 3. Dock kontrolü
        let dock_y = self.height - self.dock_height();
        if y >= dock_y as i32 {
            if left_down && !self.last_mouse_left {
                // Dock click
                if let Some(action) = self.handle_dock_click(x, y) {
                    self.handle_dock_action(action);
                    redraw = true;
                }
            }
            self.last_mouse_left = left_down;
            return redraw;
        }
        
        let just_pressed = left_down && !self.last_mouse_left;
        let just_released = !left_down && self.last_mouse_left;

        // 4. Bırakma (Release)
        if just_released {
            self.dragging_window_idx = None;
        }

        // 5. Sürükleme (Dragging)
        if let Some(idx) = self.dragging_window_idx {
            if left_down {
                let (win_x, win_y, _, _) = self.app_windows[idx].get_rect();
                let new_x = (x - self.drag_start_offset.0).max(0) as usize;
                let new_y = (y - self.drag_start_offset.1).max(25) as usize; // Below menu bar

                if win_x != new_x || win_y != new_y {
                    self.app_windows[idx].set_position(new_x, new_y);
                    redraw = true;
                }
            } else {
                self.dragging_window_idx = None;
            }
        }
        // 6. Yeni Tıklama (Click / Drag Start / Focus)
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
                let (wx, wy, _, _) = self.app_windows[idx].get_rect();
                let titlebar_height = 28; // Default titlebar height
                
                // Aktiflik durumunu güncelle
                self.active_window_idx = Some(idx);
                
                // Titlebar'dan sürükleme başlat
                if y >= wy as i32 && y < (wy + titlebar_height) as i32 {
                    self.dragging_window_idx = Some(idx);
                    self.drag_start_offset = (x - wx as i32, y - wy as i32);
                } else {
                    // İçerik tıklaması
                    self.app_windows[idx].on_click(x, y);
                }
                
                redraw = true;
            }
        }

        self.last_mouse_left = left_down;
        redraw
    }
    
    /// Handle dock click
    fn handle_dock_click(&mut self, mx: i32, _my: i32) -> Option<DockAction> {
        let total_width = self.dock.items.len() * (48 + 8) + 8 * 2;
        let dock_x = (self.width - total_width) / 2;
        
        let mut item_x = dock_x + 8;
        for item in &self.dock.items {
            if mx >= item_x as i32 && mx < (item_x + 48) as i32 {
                return Some(item.action.clone());
            }
            item_x += 48 + 8;
        }
        None
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
                    return true;
                }
                SpotlightEvent::Cancelled => {
                    self.spotlight_open = false;
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
                    return true;
                }
                SpotlightEvent::Cancelled => {
                    self.spotlight_open = false;
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
        
        // Update dock animation
        self.dock.update(dt);
        
        // Update spotlight animation
        self.spotlight.update(dt);
        
        // Update app windows
        for window in &mut self.app_windows {
            window.update(dt);
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
}

/// Run desktop main loop (standalone function)
pub fn run(fb: &mut Framebuffer) -> ! {
    let width = fb.width;
    let height = fb.height;
    let mut desktop = Desktop::new(width, height);
    
    // Launch default apps
    desktop.launch_app("finder");
    
    crate::serial_println!("[GUI] Desktop initialized ({}x{}), entering main loop", width, height);
    
    let mut frame_count = 0u32;
    loop {
        // Draw desktop
        desktop.draw(fb);
        
        frame_count += 1;
        if frame_count % 1000 == 0 {
            // Log every 1000 frames to confirm loop is running
            crate::serial_println!("[GUI] Frame {}", frame_count);
        }
        
        // Small delay
        for _ in 0..100000 {
            unsafe { core::arch::asm!("nop", options(nomem, nostack)); }
        }
    }
}
