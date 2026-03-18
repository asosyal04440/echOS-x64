//! echOS GOP framebuffer wrapper.

use alloc::vec;
use alloc::vec::Vec;
#[cfg(test)]
use alloc::boxed::Box;

use crate::font;
use uefi::proto::console::gop::GraphicsOutput;

#[derive(Clone)]
pub struct Framebuffer {
    pub base_addr: usize,
    pub width: usize,
    pub height: usize,
    pub pixels_per_scan_line: usize,
    shadow_buffer: Option<Vec<u32>>,
}

impl Framebuffer {
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
            shadow_buffer: None,
        }
    }

    #[cfg(test)]
    pub fn new_for_test(width: usize, height: usize) -> Self {
        let stride = width.max(1);
        let backing = vec![0u32; stride.saturating_mul(height.max(1))].into_boxed_slice();
        let leaked = Box::leak(backing);
        Self {
            base_addr: leaked.as_mut_ptr() as usize,
            width: width.max(1),
            height: height.max(1),
            pixels_per_scan_line: stride,
            shadow_buffer: None,
        }
    }

    #[inline]
    fn offset(&self, x: usize, y: usize) -> usize {
        y.saturating_mul(self.pixels_per_scan_line)
            .saturating_add(x)
    }

    pub fn plot_pixel(&mut self, x: usize, y: usize, color: u32) {
        if x >= self.width || y >= self.height {
            return;
        }

        let offset = self.offset(x, y);
        if let Some(shadow) = self.shadow_buffer.as_mut() {
            shadow[offset] = color;
            return;
        }

        unsafe {
            *((self.base_addr as *mut u32).add(offset)) = color;
        }
    }

    pub fn clear(&mut self, color: u32) {
        if let Some(shadow) = self.shadow_buffer.as_mut() {
            for pixel in shadow.iter_mut() {
                *pixel = color;
            }
            return;
        }

        for pixel in self.front_buffer_mut().iter_mut() {
            *pixel = color;
        }
    }

    pub fn draw_rect(&mut self, x: usize, y: usize, width: usize, height: usize, color: u32) {
        for row in 0..height {
            for col in 0..width {
                self.plot_pixel(x + col, y + row, color);
            }
        }
    }

    pub fn draw_rect_outline(
        &mut self,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
        color: u32,
    ) {
        if width == 0 || height == 0 {
            return;
        }

        for col in 0..width {
            self.plot_pixel(x + col, y, color);
            self.plot_pixel(x + col, y + height - 1, color);
        }
        for row in 0..height {
            self.plot_pixel(x, y + row, color);
            self.plot_pixel(x + width - 1, y + row, color);
        }
    }

    pub fn get_pixel(&self, x: usize, y: usize) -> u32 {
        if x >= self.width || y >= self.height {
            return 0;
        }

        let offset = self.offset(x, y);
        if let Some(shadow) = self.shadow_buffer.as_ref() {
            return shadow[offset];
        }

        unsafe { *((self.base_addr as *const u32).add(offset)) }
    }

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

    pub fn draw_string(&mut self, x: usize, y: usize, s: &str, color: u32) {
        let mut current_x = x;
        for c in s.chars() {
            self.draw_char(current_x, y, c, color);
            current_x += 8;
        }
    }

    pub fn scroll_up(&mut self, lines: usize) {
        if lines == 0 {
            return;
        }

        let scroll_rows = lines.min(self.height);
        let visible_rows = self.height.saturating_sub(scroll_rows);
        let row_len = self.pixels_per_scan_line;

        if let Some(shadow) = self.shadow_buffer.as_mut() {
            for row in 0..visible_rows {
                let dst = row * row_len;
                let src = (row + scroll_rows) * row_len;
                shadow.copy_within(src..src + row_len, dst);
            }
            for pixel in shadow.iter_mut().skip(visible_rows * row_len) {
                *pixel = 0x000000;
            }
            return;
        }

        let bytes_per_pixel = 4;
        let row_stride_bytes = row_len * bytes_per_pixel;
        unsafe {
            for row in 0..visible_rows {
                let dst = (self.base_addr + row * row_stride_bytes) as *mut u8;
                let src = (self.base_addr + (row + scroll_rows) * row_stride_bytes) as *const u8;
                core::ptr::copy_nonoverlapping(src, dst, row_stride_bytes);
            }

            let clear_start = (self.base_addr + visible_rows * row_stride_bytes) as *mut u32;
            let clear_pixels = row_len * scroll_rows;
            for idx in 0..clear_pixels {
                *clear_start.add(idx) = 0x000000;
            }
        }
    }

    pub fn buffer_mut(&mut self) -> &mut [u32] {
        if self.shadow_buffer.is_none() {
            return self.front_buffer_mut();
        }
        self.shadow_buffer.as_mut().unwrap().as_mut_slice()
    }

    pub fn front_buffer(&self) -> &[u32] {
        let len = self.pixels_per_scan_line.saturating_mul(self.height);
        unsafe { core::slice::from_raw_parts(self.base_addr as *const u32, len) }
    }

    pub fn front_buffer_mut(&mut self) -> &mut [u32] {
        let len = self.pixels_per_scan_line.saturating_mul(self.height);
        unsafe { core::slice::from_raw_parts_mut(self.base_addr as *mut u32, len) }
    }

    pub fn enable_double_buffering(&mut self) {
        if self.shadow_buffer.is_some() {
            return;
        }

        let len = self.pixels_per_scan_line.saturating_mul(self.height);
        let mut shadow = vec![0; len];
        shadow.copy_from_slice(self.front_buffer());
        self.shadow_buffer = Some(shadow);
    }

    pub fn swap_buffers(&mut self) {
        let Some(shadow) = self.shadow_buffer.as_ref() else {
            return;
        };

        unsafe {
            core::ptr::copy_nonoverlapping(
                shadow.as_ptr(),
                self.base_addr as *mut u32,
                shadow.len(),
            );
        }
    }
}
