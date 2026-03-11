//! # EchLive Wallpaper Engine
//!
//! GPU gerektirmeyen, prosedürel duvar kağıdı efektleri.
//! HexGrid, Rain (Matrix), Wave gibi modlar içerir.

use crate::gop::framebuffer::Framebuffer;
use libm::sinf;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum WallpaperMode {
    Static,  // Düz renk veya resim (varsayılan)
    HexGrid, // Altıgen ızgara (Cyberpunk)
    Rain,    // Matrix yağmuru (basitleştirilmiş)
    Wave,    // Sinüs dalgaları
}

pub struct WallpaperEngine {
    pub mode: WallpaperMode,
    time: f32,
    width: usize,
    height: usize,
}

impl WallpaperEngine {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            mode: WallpaperMode::Static,
            time: 0.0,
            width,
            height,
        }
    }

    pub fn set_mode(&mut self, mode: WallpaperMode) {
        self.mode = mode;
        self.time = 0.0;
    }

    pub fn update(&mut self, dt: f32) {
        self.time += dt;
    }

    pub fn draw(&self, fb: &mut Framebuffer) {
        match self.mode {
            WallpaperMode::Static => {
                // Static modda bir şey yapma, desktop.rs hallediyor
            }
            WallpaperMode::HexGrid => self.draw_hex_grid(fb),
            WallpaperMode::Rain => self.draw_rain(fb),
            WallpaperMode::Wave => self.draw_wave(fb),
        }
    }

    fn draw_hex_grid(&self, fb: &mut Framebuffer) {
        // HexGrid efekti: Siyah arka plan, siyan noktalar
        let w = fb.width as i32;
        let h = fb.height as i32;
        let t = self.time;

        // Arkaplanı temizle
        let bg_color = 0xFF101012;
        fb.draw_rect(0, 0, w as usize, h as usize, bg_color);

        let size = 40.0;
        let dx = size * 1.5;
        let dy = size * 1.732; // size * sqrt(3)

        let cols = (w as f32 / dx) as i32 + 2;
        let rows = (h as f32 / dy) as i32 + 2;

        let center_x = w as f32 / 2.0;
        let center_y = h as f32 / 2.0;

        for y in 0..rows {
            for x in 0..cols {
                let cx = x as f32 * dx + if y % 2 == 1 { dx / 2.0 } else { 0.0 };
                let cy = y as f32 * dy;

                // Pulse efekti
                let dist_x = cx - center_x;
                let dist_y = cy - center_y;
                let dist = libm::sqrtf(dist_x * dist_x + dist_y * dist_y);

                let phase = dist * 0.01 - t * 2.0;
                let brightness = (sinf(phase) * 0.5 + 0.5) * 0.5; // 0.0 - 0.5

                if brightness > 0.1 {
                    let r = 0;
                    let g = (150.0 * brightness) as u8;
                    let b = (200.0 * brightness) as u8;
                    let color = 0xFF000000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);

                    // Nokta çiz
                    fb.plot_pixel(cx as usize, cy as usize, color);
                    fb.plot_pixel((cx + 1.0) as usize, cy as usize, color);
                    fb.plot_pixel(cx as usize, (cy + 1.0) as usize, color);
                    fb.plot_pixel((cx + 1.0) as usize, (cy + 1.0) as usize, color);
                }
            }
        }
    }

    fn draw_rain(&self, fb: &mut Framebuffer) {
        // Matrix yağmuru (dikey çizgiler)
        let w = fb.width;
        let h = fb.height;
        let t = self.time;

        let bg_color = 0xFF000000;
        fb.draw_rect(0, 0, w, h, bg_color);

        let col_width = 20;
        let cols = w / col_width;

        for i in 0..cols {
            // Basit psöz-rastgelelik
            let speed = 100.0 + ((i * 123) % 100) as f32;
            let offset = ((i * 456) % h) as f32;
            let y_pos = (t * speed + offset) % (h as f32 + 200.0) - 200.0;

            let x = i * col_width;

            // Kuyruk çiz
            for j in 0..10 {
                let py = y_pos - (j * 20) as f32;
                if py >= 0.0 && py < h as f32 {
                    let alpha = 255 - (j * 25);
                    let color =
                        0xFF000000 | ((0 as u32) << 16) | ((alpha as u32) << 8) | (0 as u32);
                    fb.draw_rect(x, py as usize, 2, 12, color);
                }
            }
        }
    }

    fn draw_wave(&self, fb: &mut Framebuffer) {
        // Sinüs dalgaları
        let w = fb.width;
        let h = fb.height;
        let t = self.time;

        // Ekranı temizle
        fb.draw_rect(0, 0, w, h, 0xFF101020);

        let center_y = h as f32 / 2.0;

        for i in 0..5 {
            let offset = i as f32 * 50.0;
            let color_val = 100 + i * 30;
            let color = 0xFF000000 | ((0) << 16) | ((color_val as u32) << 8) | ((255) << 0);

            for x in (0..w).step_by(6) {
                let fx = x as f32;
                let fy = center_y + sinf(fx * 0.01 + t + offset) * 100.0;

                if fy >= 0.0 && fy < h as f32 {
                    fb.draw_rect(x, fy as usize, 4, 4, color);
                }
            }
        }
    }
}
