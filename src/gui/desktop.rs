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
use pc_keyboard::KeyCode;
use crate::gop::framebuffer::Framebuffer;
use super::widgets::Rect;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

// ============================================================================
// UYGULAMA PENCERESİ
// ============================================================================

/// Uygulama penceresi türleri
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
    pub fn draw(&mut self, fb: &mut Framebuffer) {
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
            _ => true, // Diğer uygulamalar tıklamaları kendi içinde işler
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
// MASAÜSTÜ
// ============================================================================

/// Masaüstü Yöneticisi
pub struct Desktop {
    width: usize,
    height: usize,
    /// Uygulama pencereleri
    app_windows: Vec<AppWindow>,
    /// Basit pencereler (eski)
    windows: Vec<Window<'static>>,
    taskbar_height: usize,

    // GUI Bileşenleri
    dock: Dock,
    menu_bar: MenuBar,
    spotlight: Spotlight,
    drag_drop: DragDropManager,
    clipboard: ClipboardManager,

    // Durum
    dragging_window_idx: Option<usize>,
    /// Yeniden boyutlandırılan pencere indeksi
    resizing_window_idx: Option<usize>,
    drag_start_offset: (i32, i32),
    last_mouse_left: bool,
    active_window_idx: Option<usize>,
    spotlight_open: bool,
    menu_open: bool,
    last_time: f32,
    /// Son dock mouse konumu (hover/magnification için)
    last_dock_mouse_pos: Option<(i32, i32)>,
    /// Son menu bar mouse konumu
    last_menu_mouse_pos: Option<(i32, i32)>,
    /// Wallpaper animasyon zamanı (saniye)
    anim_t: f32,
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
            resizing_window_idx: None,
            last_dock_mouse_pos: None,
            last_menu_mouse_pos: None,
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
            anim_t: 0.0,
        }
    }

    /// Basit pencere ekler (eski)
    pub fn add_window(&mut self, window: Window<'static>) {
        self.windows.push(window);
    }

    /// Uygulama ID'sine göre uygulama başlatır
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
                // Bilinmeyen uygulamalar için basit pencere oluştur
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

    /// Pencereyi dizinine göre kapatır
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
    pub fn draw(&mut self, fb: &mut Framebuffer) {
        // Arkaplan
        self.draw_background(fb);

        // Menü çubuğu (üst)
        self.menu_bar.draw(fb);

        // Pencereler (uygulama pencereleri)
        for window in self.app_windows.iter_mut() {
            window.draw(fb);
        }

        // Basit pencereler (eski)
        for window in &self.windows {
            window.draw(fb);
        }

        // Dock (alt)
        self.dock.draw(fb);

        // Spotlight (katman)
        if self.spotlight_open {
            self.spotlight.draw(fb);
        }

        // Sürükle ve bırak katmanı
        self.drag_drop.draw(fb);
    }

    fn draw_background(&self, fb: &mut Framebuffer) {
        let w = self.width;
        let h = self.height;
        let t = self.anim_t;

        // ── 1. Dikey gradyan taban ──────────────────────────────────────
        // Üst: 0x050C17 (koyu lacivert) → Alt: 0x020608 (neredeyse siyah)
        for row in 0..h {
            let frac = row as f32 / h as f32;
            let r = (5.0f32 * (1.0 - frac) + 2.0 * frac) as u32;
            let g = (12.0f32 * (1.0 - frac) + 6.0 * frac) as u32;
            let b = (23.0f32 * (1.0 - frac) + 8.0 * frac) as u32;
            // Her 4 satırda tarama çizgisi: hafif karartma
            let dim = if row % 4 < 2 { 0u32 } else { 1u32 };
            let color = ((r.saturating_sub(dim)) << 16)
                      | ((g.saturating_sub(dim)) << 8)
                      |  (b.saturating_sub(dim));
            fb.draw_rect(0, row, w, 1, color);
        }

        // ── 2. Merkez ışıma (siyanets çekirdeği) ───────────────────────
        // Birbirinin içinde 4 eliptik bant
        let cx = w / 2;
        let cy = h / 2;
        let layers: &[(usize, usize, u32)] = &[
            (540, 310, 0x030B14),
            (370, 210, 0x051220),
            (210, 120, 0x07182C),
            ( 95,  55, 0x0A2040),
        ];
        for &(rx, ry, col) in layers {
            let x0 = cx.saturating_sub(rx);
            let y0 = cy.saturating_sub(ry);
            let rw = (2 * rx).min(w.saturating_sub(x0));
            let rh = (2 * ry).min(h.saturating_sub(y0));
            if rw > 0 && rh > 0 { fb.draw_rect(x0, y0, rw, rh, col); }
        }

        // ── 3. Çevre nokta matris / devre-ağ ızgarası ──────────────────
        let phase_x = (libm::sinf(t * 0.4) * 6.0) as i32;
        let grid = 32usize;
        for gy in (0..h).step_by(grid) {
            for gx in (0..w).step_by(grid) {
                let px = ((gx as i32 + phase_x).max(0) as usize).min(w.saturating_sub(3));
                if gy + 1 < h {
                    fb.plot_pixel(px,     gy,     0x0C2232);
                    fb.plot_pixel(px + 1, gy,     0x0C2232);
                    fb.plot_pixel(px,     gy + 1, 0x0C2232);
                }
            }
        }

        // ── 4. Animasyonlu veri akışı çizgileri (2 yatay iz) ───────────
        let traces: &[(f32, f32)] = &[
            (0.28, 0.0),
            (0.72, 2.1),
        ];
        for &(yfrac, phase) in traces {
            let wave = (libm::sinf(t * 0.6 + phase) * 8.0) as i32;
            let ty_raw = (h as f32 * yfrac) as i32 + wave;
            if ty_raw < 0 || ty_raw >= h as i32 { continue; }
            let ty = ty_raw as usize;
            let seg_on  = 12usize;
            let seg_off = 8usize;
            let scroll  = ((t * 20.0) as usize) % (seg_on + seg_off);
            for px in 0..w {
                if (px + scroll) % (seg_on + seg_off) < seg_on {
                    fb.plot_pixel(px, ty, 0x0D2535);
                }
            }
        }

        // ── 5. Köşe aksan çizgileri ─────────────────────────────────────
        let sz = 90usize;
        let ac = 0x081D30u32;
        fb.draw_rect(0,                   0,                    sz, 1, ac); // sol-üst yatay
        fb.draw_rect(0,                   0,                    1, sz, ac); // sol-üst dikey
        fb.draw_rect(w.saturating_sub(sz),h.saturating_sub(1),  sz, 1, ac); // sağ-alt yatay
        fb.draw_rect(w.saturating_sub(1), h.saturating_sub(sz), 1, sz, ac); // sağ-alt dikey
    }

    fn dock_height(&self) -> usize {
        70 // DOCK_HEIGHT sabiti
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
        }

        // 5. Resize (bottom-right handle)
        if let Some(idx) = self.resizing_window_idx {
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

    /// Dock eylemini işle
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

    /// Menü eylemini işle
    fn handle_menu_action(&mut self, action: MenuAction) {
        match action {
            MenuAction::NewFile => self.launch_app("textedit"),
            MenuAction::OpenFile => self.launch_app("finder"),
            MenuAction::Preferences => self.launch_app("settings"),
            MenuAction::About => {
                // Hakkında iletişim kutusunu göster
            }
            MenuAction::Quit => {
                // Aktif pencereyi kapat
                if let Some(idx) = self.active_window_idx {
                    self.close_window(idx);
                }
            }
            _ => {}
        }
    }

    /// Sağ öğe eylemini işle (menü çubuğu)
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

    /// Klavye girdisini işle
    pub fn on_key(&mut self, c: char) -> bool {
        // Spotlight açık mı?
        if self.spotlight_open {
            let event = self.spotlight.on_key_press(c);
            match event {
                SpotlightEvent::ResultSelected(result) => {
                    // Seçilen sonucu başlat
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
                // Cmd+Q simülasyonu - pencereyi kapat
                // Şimdilik sadece 'q' ile kapat
            } else if c == 'w' || c == 'W' {
                // Cmd+W - sekme/pencereyi kapat
            } else {
                self.app_windows[idx].on_key(c);
            }
            return true;
        }

        false
    }

    /// Özel tuşu işle
    pub fn on_special_key(&mut self, key: KeyCode) -> bool {
        // Özel tuşla Spotlight aç/kapat (Cmd+Space simülasyonu)
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

    /// Masaüstü durumunu günceller
    pub fn update(&mut self, dt: f32) -> bool {
        let mut needs_redraw = false;

        // Wallpaper animasyon sayacı
        self.anim_t += dt * 0.25;
        if self.anim_t > 628.0 { self.anim_t -= 628.0; } // 200π overflow koruması

        let old_rects: Vec<(usize, Rect)> = self.app_windows
            .iter()
            .enumerate()
            .map(|(idx, window)| {
                let (x, y, width, height) = window.get_rect();
                (idx, Rect::new(x as i32, y as i32, width as i32, height as i32))
            })
            .collect();
        
        // Dock animasyonunu güncelle
        self.dock.update(dt);
        needs_redraw = true;
        
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
                    // Klasörü aç
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

    /// idx numaralı pencerenin Rect'ini döndürür.
    pub fn window_rect_at(&self, idx: usize) -> Option<Rect> {
        self.app_windows.get(idx).map(|w| {
            let (x, y, width, height) = w.get_rect();
            Rect::new(x as i32, y as i32, width as i32, height as i32)
        })
    }

    /// Belirtilen bölgeyi kirli (damage) olarak işaretle.
    /// Şu an no-op — gerçek implementasyonda dirty rect kuyruğuna eklenir.
    pub fn add_window_damage(&mut self, _rect: Rect) {
        // TODO: dirty rect kuyruğuna ekle
    }

    /// idx penceresini en öne getirir (z-order). Yeni indeksini döndürür.
    pub fn bring_window_to_front(&mut self, idx: usize) -> Option<usize> {
        if idx >= self.app_windows.len() { return None; }
        let win = self.app_windows.remove(idx);
        self.app_windows.push(win);
        let new_idx = self.app_windows.len() - 1;
        self.active_window_idx = Some(new_idx);
        Some(new_idx)
    }

    /// Yeniden boyutlandırma hover indeksini ayarlar.
    pub fn set_resize_hover_idx(&mut self, idx: Option<usize>) -> Option<usize> {
        self.resizing_window_idx = idx;
        idx
    }

    // ============================================================
    // COMPOSITOR ENTEGRASYON API (Faz 2)
    // ============================================================

    /// SADECE arkaplanı (wallpaper + grid) çizer. Pencereler, dock, panel yok.
    /// Compositor bu metodu render döngüsünün ilk adımı olarak çağırır.
    pub fn draw_wallpaper_only(&self, fb: &mut Framebuffer) {
        self.draw_background(fb);
    }

    /// Dock ve Spotlight katmanlarını çizer (pencereler ve panel hariç).
    /// Compositor her karedeki render döngüsünün en sonunda (panel ve cursor öncesinde) çağırır.
    pub fn draw_chrome(&mut self, fb: &mut Framebuffer) {
        self.dock.draw(fb);
        if self.spotlight_open {
            self.spotlight.draw(fb);
        }
        self.drag_drop.draw(fb);
    }

    /// İndeksteki tek bir uygulama penceresini çizer (içerik + kendi dekorasyonları).
    /// Compositor Z-sırasına göre sıralayarak çağırır.
    pub fn draw_app_window(&mut self, fb: &mut Framebuffer, idx: usize) {
        if let Some(win) = self.app_windows.get_mut(idx) {
            win.draw(fb);
        }
    }

    /// Toplam uygulama penceresi sayısı.
    pub fn app_count(&self) -> usize {
        self.app_windows.len()
    }

    /// idx'deki pencerenin başlığını döndürür.
    pub fn app_title(&self, idx: usize) -> Option<&str> {
        Some(match self.app_windows.get(idx)? {
            AppWindow::Finder(_)          => "Finder",
            AppWindow::Browser(_)         => "Browser",
            AppWindow::Terminal(_)        => "Terminal",
            AppWindow::Preferences(_)     => "System Preferences",
            AppWindow::Preview(_)         => "Preview",
            AppWindow::ActivityMonitor(_) => "Activity Monitor",
            AppWindow::FontBook(_)        => "Font Book",
            AppWindow::Simple(w)          => w.title.as_str(),
        })
    }

    /// idx'deki pencerenin Rect bilgisi.
    pub fn app_rect_at(&self, idx: usize) -> Option<Rect> {
        self.window_rect_at(idx)
    }

    /// Pencereyi konumlandırır (compositor sürükleme/snap sonrası uygular).
    pub fn set_app_rect(&mut self, idx: usize, rect: Rect) {
        if let Some(win) = self.app_windows.get_mut(idx) {
            win.set_position(rect.x as usize, rect.y as usize);
            win.set_size(rect.width as usize, rect.height as usize);
        }
    }

    /// WM'in close komutu üzerine pencereyi kapat.
    pub fn close_app(&mut self, idx: usize) {
        self.close_window(idx);
    }

    /// Aktif pencere indeksi.
    pub fn active_app_idx(&self) -> Option<usize> {
        self.active_window_idx
    }

    /// Verilen indeksi aktif yap.
    pub fn set_active_app(&mut self, idx: usize) {
        if idx < self.app_windows.len() {
            self.active_window_idx = Some(idx);
        }
    }

    /// Tüm app pencerelerinin başlık+rect bilgisini döndürür (WM frame sync için).
    pub fn app_window_infos(&self) -> alloc::vec::Vec<(alloc::string::String, Rect)> {
        self.app_windows.iter().map(|w| {
            let title = match w {
                AppWindow::Finder(_)          => alloc::string::String::from("Finder"),
                AppWindow::Browser(_)         => alloc::string::String::from("Browser"),
                AppWindow::Terminal(_)        => alloc::string::String::from("Terminal"),
                AppWindow::Preferences(_)     => alloc::string::String::from("System Preferences"),
                AppWindow::Preview(_)         => alloc::string::String::from("Preview"),
                AppWindow::ActivityMonitor(_) => alloc::string::String::from("Activity Monitor"),
                AppWindow::FontBook(_)        => alloc::string::String::from("Font Book"),
                AppWindow::Simple(win)        => win.title.clone(),
            };
            let (x, y, w2, h) = w.get_rect();
            let rect = Rect::new(x as i32, y as i32, w2 as i32, h as i32);
            (title, rect)
        }).collect()
    }
} // impl Desktop son

/// Masaüstü ana döngüsünü çalıştırır (bağımsız fonksiyon)
pub fn run(fb: &mut Framebuffer) -> ! {
    let width = fb.width;
    let height = fb.height;
    
    // Enable double buffering for smooth rendering
    fb.enable_double_buffering();
    
    let mut desktop = Desktop::new(width, height);

    // Varsayılan uygulamaları başlat
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
