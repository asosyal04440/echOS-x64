//! # echOS GOP Framebuffer
//!
//! UEFI Graphics Output Protocol frambuffer wrapper.
//! Piksel çizim, metin rendering ve temel grafik operasyonları.

use crate::font;
use uefi::proto::console::gop::GraphicsOutput;

/// Framebuffer yapısı.
/// Doğrudan ekran belleğine erişim sağlar.
#[derive(Clone, Copy)]
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

    /// Dikdörtgen çerçevesi çizer (outline).
    pub fn draw_rect_outline(&mut self, x: usize, y: usize, width: usize, height: usize, color: u32) {
        // Top edge
        for i in 0..width {
            self.plot_pixel(x + i, y, color);
        }
        // Bottom edge
        for i in 0..width {
            self.plot_pixel(x + i, y + height - 1, color);
        }
        // Left edge
        for j in 0..height {
            self.plot_pixel(x, y + j, color);
        }
        // Right edge
        for j in 0..height {
            self.plot_pixel(x + width - 1, y + j, color);
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
        let row_stride_bytes = self.pixels_per_scan_line * bytes_per_pixel;
        let total_rows = self.height;

        if lines == 0 {
            return;
        }

        let scroll_rows = lines.min(total_rows);
        let visible_rows = total_rows - scroll_rows;

        unsafe {
            for row in 0..visible_rows {
                let dst = (self.base_addr + row * row_stride_bytes) as *mut u8;
                let src = (self.base_addr + (row + scroll_rows) * row_stride_bytes) as *const u8;
                core::ptr::copy_nonoverlapping(src, dst, row_stride_bytes);
            }

            let clear_start = (self.base_addr + visible_rows * row_stride_bytes) as *mut u32;
            let clear_pixels = self.pixels_per_scan_line * scroll_rows;
            for i in 0..clear_pixels {
                *clear_start.add(i) = 0x000000;
            }
        }
    }

    /// Ham buffer'a mutable erişim sağlar.
    pub fn buffer_mut(&mut self) -> &mut [u32] {
        let len = self.pixels_per_scan_line * self.height;
        unsafe { core::slice::from_raw_parts_mut(self.base_addr as *mut u32, len) }
    }

    /// Çift tamponlamayı etkinleştirir (stub — GOP tek tampon kullanır).
    /// desktop::run() tarafından çağrılır; GOP doğrudan fiziksel belleğe yazar
    /// bu yüzden ek bir arka tampon gerekmez.
    pub fn enable_double_buffering(&mut self) {
        // GOP framebuffer tek tamponda çalışır; bu fonksiyon gelecekteki
        // shadow-buffer implementasyonu için yer tutucudur.
    }

    /// Arka tamponu ön tampona kopyalar (stub — GOP tek tampon kullanır).
    /// desktop::run() her kare sonunda bunu çağırır.
    pub fn swap_buffers(&mut self) {
        // Tek tampon modunda yazılan her piksel zaten ekrana gider;
        // gerçek bir double-buffer için burada blit yapılacak.
    }
}
