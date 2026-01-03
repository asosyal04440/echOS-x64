//! # echOS Masaüstü Ortamı (Desktop)
//! 
//! Arkaplanı, görev çubuğunu (taskbar) ve pencereleri yönetir.
//! Pencere sürükleme, odaklama ve çizim işlemlerini koordine eder.

use super::theme::{Theme, Color};
use super::window::Window;
use crate::gop::framebuffer::Framebuffer;
use alloc::vec::Vec;
use alloc::string::String;

/// Masaüstü Yöneticisi
pub struct Desktop<'a> {
    width: usize,
    height: usize,
    windows: Vec<Window<'a>>,
    taskbar_height: usize,
    
    // Sürükleme durumu
    dragging_window_idx: Option<usize>,
    drag_start_offset: (i32, i32),
    last_mouse_left: bool,
}

impl<'a> Desktop<'a> {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            windows: Vec::new(),
            taskbar_height: 40,
            dragging_window_idx: None,
            drag_start_offset: (0, 0),
            last_mouse_left: false,
        }
    }
    
    /// Masaüstüne pencere ekler.
    pub fn add_window(&mut self, window: Window<'a>) {
        self.windows.push(window);
    }
    
    /// Tüm masaüstünü çizer.
    pub fn draw(&self, fb: &mut Framebuffer) {
        // Arkaplan
        self.draw_background(fb);
        
        // Taskbar
        self.draw_taskbar(fb);
        
        // Pencereler (Sırayla, en son eklenen en üstte)
        for window in &self.windows {
            window.draw(fb);
        }
    }
    
    fn draw_background(&self, fb: &mut Framebuffer) {
        // Koyu mor arkaplan
        let bg_color = 0x1a0b2e; 
        fb.clear(bg_color);

        // Izgara Deseni (Grid)
        let grid_color = 0x4a2b7e; // Açık mor
        let grid_size = 40;
        
        // Dikey çizgiler
        for x in (0..self.width).step_by(grid_size) {
            for y in 0..(self.height - self.taskbar_height) {
                 fb.plot_pixel(x, y, grid_color);
            }
        }
        
        // Yatay çizgiler
        for y in (0..(self.height - self.taskbar_height)).step_by(grid_size) {
            for x in 0..self.width {
                fb.plot_pixel(x, y, grid_color);
            }
        }
    }
    
    fn draw_taskbar(&self, fb: &mut Framebuffer) {
        let taskbar_y = self.height - self.taskbar_height;
        
        // Taskbar arkaplanı
        fb.draw_rect(0, taskbar_y, self.width, self.taskbar_height, Theme::TASKBAR_BG.to_u32());
        
        // Üst kenarlık çizgisi
        for x in 0..self.width {
            fb.plot_pixel(x, taskbar_y, Theme::BORDER.to_u32());
        }
        
        // echOS Başlat butonu benzeri logo alanı
        fb.draw_rect(5, taskbar_y + 5, 80, 30, Theme::ACCENT_PRIMARY.to_u32());
        
        // "echOS" yazısı
        fb.draw_string(20, taskbar_y + 12, "echOS", Theme::DESKTOP_BG.to_u32());
        
        // Saat alanı (Sağ taraf)
        fb.draw_string(self.width - 60, taskbar_y + 12, "00:00", Theme::TEXT_PRIMARY.to_u32());
    }
    
    /// Mouse hareketlerini ve tıklamalarını yönetir.
    /// Eğer yeniden çizim gerekirse `true` döndürür.
    pub fn update_mouse(&mut self, x: i32, y: i32, left_down: bool) -> bool {
        let mut redraw = false;
        
        let just_pressed = left_down && !self.last_mouse_left;
        let just_released = !left_down && self.last_mouse_left;
        
        // 1. Bırakma (Release)
        if just_released {
            if self.dragging_window_idx.is_some() {
                self.dragging_window_idx = None;
            }
        }
        
        // 2. Sürükleme (Dragging)
        if let Some(idx) = self.dragging_window_idx {
            if left_down {
                let win = &mut self.windows[idx];
                let new_x = (x - self.drag_start_offset.0).max(0) as usize;
                let new_y = (y - self.drag_start_offset.1).max(0) as usize;
                
                if win.x != new_x || win.y != new_y {
                    win.x = new_x;
                    win.y = new_y;
                    redraw = true;
                }
            } else {
                self.dragging_window_idx = None;
            }
        }
        // 3. Yeni Tıklama (Click / Drag Start / Focus)
        else if just_pressed {
            // Taskbar tıklaması mı?
            if y >= (self.height - self.taskbar_height) as i32 {
                return true; 
            }
            
            // Pencereleri sondan başa (üstten alta) kontrol et
            let mut hit_idx = None;
            for (i, window) in self.windows.iter_mut().enumerate().rev() {
                 if x >= window.x as i32 && x < (window.x + window.width) as i32 &&
                    y >= window.y as i32 && y < (window.y + window.height) as i32 {
                        hit_idx = Some(i);
                        break;
                    }
            }
            
            if let Some(idx) = hit_idx {
                // Pencereyi en öne getir
                let mut window = self.windows.remove(idx);
                let is_titlebar = window.is_titlebar_hit(x, y);
                
                // Aktiflik durumunu güncelle
                for w in &mut self.windows { w.is_active = false; }
                window.is_active = true;
                
                if is_titlebar {
                    // Başlık çubuğundan sürükleme başlat
                    self.dragging_window_idx = Some(self.windows.len()); // Son indekse eklenecek
                    self.drag_start_offset = (x - window.x as i32, y - window.y as i32);
                } else {
                    // İçerik tıklaması
                    window.on_click(x, y);
                }
                
                self.windows.push(window);
                redraw = true;
            }
        }
        
        self.last_mouse_left = left_down;
        redraw
    }
    
    pub fn on_click(&mut self, x: i32, y: i32) -> bool {
        self.update_mouse(x, y, true)
    }
    
    pub fn update(&mut self) -> bool {
        let mut needs_redraw = false;
        for window in &mut self.windows {
            if window.update() {
                needs_redraw = true;
            }
        }
        needs_redraw
    }

    pub fn get_window_by_title_mut(&mut self, title: &str) -> Option<&mut Window<'a>> {
        for window in &mut self.windows {
            if window.title == title {
                return Some(window);
            }
        }
        None
    }

    pub fn windows(&self) -> &Vec<Window<'a>> {
        &self.windows
    }
}
