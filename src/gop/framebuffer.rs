//! # echOS GOP Framebuffer
//! 
//! UEFI Graphics Output Protocol frambuffer wrapper.
//! Piksel çizim, metin rendering ve temel grafik operasyonları.

use uefi::proto::console::gop::GraphicsOutput;
use crate::font;

/// Framebuffer yapısı.
/// Doğrudan ekran belleğine erişim sağlar.
pub struct Framebuffer {
    /// Framebuffer'ın fiziksel bellek adresi
    pub base_addr: usize,
    /// Ekran genişliği (piksel)
    pub width: usize,
    /// Ekran yüksekliği (piksel)
    pub height: usize,
    /// Satır başına piksel sayısı (stride)
    pub pixels_per_scan_line: usize,
}

impl Framebuffer {
    /// UEFI GOP'tan yeni framebuffer oluşturur.
    pub fn new(gop: &mut GraphicsOutput) -> Self {
        let mode_info = gop.current_mode_info();
        let (width, height) = mode_info.resolution();
        let stride = mode_info.stride();
        let mut frame_buffer = gop.frame_buffer();
        let base_addr = frame_buffer.as_mut_ptr() as usize;

        Self {
            base_addr,
            width,
            height,
            pixels_per_scan_line: stride,
        }
    }

    /// Tek bir piksel çizer.
    pub fn plot_pixel(&mut self, x: usize, y: usize, color: u32) {
        if x >= self.width || y >= self.height {
            return;
        }

        let offset = (y * self.pixels_per_scan_line + x) * 4;
        let pixel_addr = (self.base_addr + offset) as *mut u32;
        unsafe {
            *pixel_addr = color;
        }
    }

    /// Tüm ekranı tek renkle temizler.
    pub fn clear(&mut self, color: u32) {
        for y in 0..self.height {
            for x in 0..self.width {
                self.plot_pixel(x, y, color);
            }
        }
    }

    /// Dikdörtgen çizer.
    pub fn draw_rect(&mut self, x: usize, y: usize, width: usize, height: usize, color: u32) {
        for i in 0..width {
            for j in 0..height {
                self.plot_pixel(x + i, y + j, color);
            }
        }
    }
    
    /// Bir pikselin rengini okur.
    pub fn get_pixel(&self, x: usize, y: usize) -> u32 {
        if x >= self.width || y >= self.height {
            return 0x000000;
        }
        
        let offset = (y * self.pixels_per_scan_line + x) * 4;
        let pixel_addr = (self.base_addr + offset) as *const u32;
        unsafe { *pixel_addr }
    }
    
    /// VGA font kullanarak karakter çizer.
    pub fn draw_char(&mut self, x: usize, y: usize, c: char, color: u32) {
        let font_data = font::vga_font::get_font_data(c);
        for (row, byte) in font_data.iter().enumerate() {
            for col in 0..8 {
                if (byte >> (7 - col)) & 1 == 1 {
                    self.plot_pixel(x + col, y + row, color);
                }
            }
        }
    }

    /// String çizer (8 piksel karakter genişliği).
    pub fn draw_string(&mut self, x: usize, y: usize, s: &str, color: u32) {
        let mut current_x = x;
        for c in s.chars() {
            self.draw_char(current_x, y, c, color);
            current_x += 8;
        }
    }
    
    /// Ekranı yukarı kaydırır.
    pub fn scroll_up(&mut self, lines: usize) {
        let bytes_per_pixel = 4;
        let bytes_per_line = self.pixels_per_scan_line * bytes_per_pixel * lines;
        let total_bytes = self.height * self.pixels_per_scan_line * bytes_per_pixel;
        
        unsafe {
            let src = (self.base_addr + bytes_per_line) as *const u8;
            let dst = self.base_addr as *mut u8;
            core::ptr::copy(src, dst, total_bytes - bytes_per_line);
            
            // Alt satırları temizle
            let clear_start = (self.base_addr + total_bytes - bytes_per_line) as *mut u32;
            for i in 0..(self.pixels_per_scan_line * lines) {
                *clear_start.add(i) = 0x000000;
            }
        }
    }

    /// Ham buffer'a mutable erişim sağlar.
    pub fn buffer_mut(&mut self) -> &mut [u32] {
        let len = self.pixels_per_scan_line * self.height;
        unsafe {
            core::slice::from_raw_parts_mut(self.base_addr as *mut u32, len)
        }
    }
}
