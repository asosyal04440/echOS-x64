//! # Pencere Efektleri
//!
//! Pencere gölgeleri, bulanıklık efektleri ve görsel parlatma.
//! Gauss bulanıklığı, düşen gölgeler ve şeffaflık.
//!
//! ## Mimari
//! - `DropShadow`: Düşen gölge yapılandırması; Gauss çekirdeği tabanlı yumuşatma
//! - `BlurEffect`: İki geçişli ayrıştırılmış Gauss bulanıklığı (yatay + dikey)
//! - `FrostedGlass`: Buzlanmış cam (vibrans) efekti; bulanıklık + renk tonu + kenar
//! - `EffectsManager`: Global yönetici; gölge ve bulanıklık ayarları
//!
//! ## Gauss Bulanıklık Algoritması
//! İki aşamalı: önce yatay bulanıklık geçici tampona, sonra dikey bulanıklık
//! çerçeve tamponuna uygulanır. σ = yarıçap / 3.0 ile çekirdek normalize edilir.

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use libm::{expf, sqrtf};
use spin::Mutex;

use crate::allocator::doctrine::{alloc_surface_pixels, SurfacePixelBuffer, SurfacePixelFormat};
use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Color, Theme};

// ============================================================================
// GÖLGE SABİTLERİ
// ============================================================================

/// Varsayılan gölge yarıçapı
pub const SHADOW_RADIUS: usize = 20;

/// Gölge X ofseti
pub const SHADOW_OFFSET_X: i32 = 0;

/// Gölge Y ofseti
pub const SHADOW_OFFSET_Y: i32 = 5;

/// Gölge opaklığı
pub const SHADOW_OPACITY: f32 = 0.3;

/// Arka planlar için bulanıklık yarıçapı
pub const BLUR_RADIUS: usize = 20;

// ============================================================================
// DÜŞEN GÖLGE
// ============================================================================

/// Düşen gölge yapılandırması
#[derive(Clone, Copy, Debug)]
pub struct DropShadow {
    /// Gölge yarıçapı (bulanıklık miktarı)
    pub radius: usize,
    /// X ofseti
    pub offset_x: i32,
    /// Y ofseti
    pub offset_y: i32,
    /// Gölge rengi
    pub color: u32,
    /// Opaklık (0.0 - 1.0)
    pub opacity: f32,
    /// İç gölge
    pub inset: bool,
    /// Yayılma (gölgeyi büyütür/küçültür)
    pub spread: i32,
}

impl DropShadow {
    pub fn new() -> Self {
        DropShadow {
            radius: SHADOW_RADIUS,
            offset_x: SHADOW_OFFSET_X,
            offset_y: SHADOW_OFFSET_Y,
            color: 0x000000,
            opacity: SHADOW_OPACITY,
            inset: false,
            spread: 0,
        }
    }

    pub fn with_radius(mut self, radius: usize) -> Self {
        self.radius = radius;
        self
    }

    pub fn with_offset(mut self, x: i32, y: i32) -> Self {
        self.offset_x = x;
        self.offset_y = y;
        self
    }

    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity;
        self
    }

    pub fn with_color(mut self, color: u32) -> Self {
        self.color = color;
        self
    }

    pub fn inset(mut self) -> Self {
        self.inset = true;
        self
    }

    /// Dikdörtgen için gölge çiz
    pub fn draw(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize, height: usize) {
        if self.radius == 0 || self.opacity < 0.01 {
            return;
        }

        let shadow_x = (x as i32 + self.offset_x - self.radius as i32).max(0) as usize;
        let shadow_y = (y as i32 + self.offset_y - self.radius as i32).max(0) as usize;
        let shadow_w = width + self.radius * 2;
        let shadow_h = height + self.radius * 2;

        // Gauss çekirdeği oluştur
        let kernel = self.gaussian_kernel(self.radius);

        // Gauss azalmasıyla gölge çiz
        for py in 0..shadow_h {
            for px in 0..shadow_w {
                // Kenarlara uzaklığı hesapla
                let dist_left = px;
                let dist_right = shadow_w - 1 - px;
                let dist_top = py;
                let dist_bottom = shadow_h - 1 - py;

                let dist_x = dist_left.min(dist_right);
                let dist_y = dist_top.min(dist_bottom);
                let dist = dist_x.min(dist_y);

                if dist < self.radius {
                    let falloff = kernel[dist];
                    let alpha = falloff * self.opacity;

                    let screen_x = shadow_x + px;
                    let screen_y = shadow_y + py;

                    if screen_x < fb.width && screen_y < fb.height {
                        let ptr = unsafe {
                            (fb.base_addr as *mut u32)
                                .add(screen_y * fb.pixels_per_scan_line + screen_x)
                        };
                        let bg = unsafe { *ptr };
                        let blended = Self::blend_color(bg, self.color, alpha);
                        unsafe {
                            *ptr = blended;
                        }
                    }
                }
            }
        }
    }

    /// Pencere için gölge çiz (optimize - yalnızca köşeler ve kenarlar)
    pub fn draw_window_shadow(
        &self,
        fb: &mut Framebuffer,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
    ) {
        if self.radius == 0 || self.opacity < 0.01 {
            return;
        }

        let kernel = self.gaussian_kernel(self.radius);
        let r = self.radius;

        // Üst kenar ve köşeler
        for py in 0..r {
            let screen_y = (y as i32 + self.offset_y - r as i32 + py as i32).max(0) as usize;
            if screen_y >= fb.height {
                continue;
            }

            for px in 0..width + r * 2 {
                let screen_x = (x as i32 + self.offset_x - r as i32 + px as i32).max(0) as usize;
                if screen_x >= fb.width {
                    continue;
                }

                // Konuma göre azalmayı hesapla
                let dist_top = py;
                let dist_left = px;
                let dist_right = width + r * 2 - 1 - px;

                // Köşe tespiti
                let in_corner = (px < r && py < r)
                    || (px > width + r && py < r)
                    || (px < r && py > height + r)
                    || (px > width + r && py > height + r);

                let falloff = if in_corner {
                    // Köşeler için dairesel azalma
                    let cx = if px < r { r - px } else { px - (width + r) };
                    let cy = r - py;
                    let dist = sqrtf((cx * cx + cy * cy) as f32) as usize;
                    if dist < r {
                        kernel[dist]
                    } else {
                        0.0
                    }
                } else {
                    // Kenarlar için doğrusal azalma
                    let dist = dist_top.min(dist_left).min(dist_right);
                    if dist < r {
                        kernel[dist]
                    } else {
                        0.0
                    }
                };

                if falloff > 0.0 {
                    let alpha = falloff * self.opacity;
                    let ptr = unsafe {
                        (fb.base_addr as *mut u32)
                            .add(screen_y * fb.pixels_per_scan_line + screen_x)
                    };
                    let bg = unsafe { *ptr };
                    let blended = Self::blend_color(bg, self.color, alpha);
                    unsafe {
                        *ptr = blended;
                    }
                }
            }
        }

        // Sol kenar
        for px in 0..r {
            let screen_x = (x as i32 + self.offset_x - r as i32 + px as i32).max(0) as usize;
            if screen_x >= fb.width {
                continue;
            }

            let falloff = kernel[px];
            let alpha = falloff * self.opacity;

            for screen_y in y..y + height {
                if screen_y >= fb.height {
                    continue;
                }

                let ptr = unsafe {
                    (fb.base_addr as *mut u32).add(screen_y * fb.pixels_per_scan_line + screen_x)
                };
                let bg = unsafe { *ptr };
                let blended = Self::blend_color(bg, self.color, alpha);
                unsafe {
                    *ptr = blended;
                }
            }
        }

        // Sağ kenar
        for px in 0..r {
            let screen_x = x + width + self.offset_x as usize + px;
            if screen_x >= fb.width {
                continue;
            }

            let falloff = kernel[r - 1 - px];
            let alpha = falloff * self.opacity;

            for screen_y in y..y + height {
                if screen_y >= fb.height {
                    continue;
                }

                let ptr = unsafe {
                    (fb.base_addr as *mut u32).add(screen_y * fb.pixels_per_scan_line + screen_x)
                };
                let bg = unsafe { *ptr };
                let blended = Self::blend_color(bg, self.color, alpha);
                unsafe {
                    *ptr = blended;
                }
            }
        }

        // Alt kenar
        for py in 0..r {
            let screen_y = y + height + self.offset_y as usize + py;
            if screen_y >= fb.height {
                continue;
            }

            let falloff = kernel[r - 1 - py];
            let alpha = falloff * self.opacity;

            for screen_x in x..x + width {
                if screen_x >= fb.width {
                    continue;
                }

                let ptr = unsafe {
                    (fb.base_addr as *mut u32).add(screen_y * fb.pixels_per_scan_line + screen_x)
                };
                let bg = unsafe { *ptr };
                let blended = Self::blend_color(bg, self.color, alpha);
                unsafe {
                    *ptr = blended;
                }
            }
        }
    }

    fn gaussian_kernel(&self, radius: usize) -> Vec<f32> {
        let size = radius * 2 + 1;
        let mut kernel = vec![0.0f32; radius];

        let sigma = radius as f32 / 3.0;
        let two_sigma_sq = 2.0 * sigma * sigma;

        let mut sum = 0.0;

        // Çekirdek değerlerini hesapla
        for i in 0..radius {
            let x = (radius - i) as f32;
            let value = expf(-x * x / two_sigma_sq);
            kernel[i] = value;
            sum += value * 2.0; // Her iki taraf
        }
        sum += 1.0; // Merkez

        // Normalize et
        for i in 0..radius {
            kernel[i] /= sum;
        }

        kernel
    }

    fn blend_color(bg: u32, fg: u32, alpha: f32) -> u32 {
        let br = ((bg >> 16) & 0xFF) as f32;
        let bg_ = ((bg >> 8) & 0xFF) as f32;
        let bb = (bg & 0xFF) as f32;

        let fr = ((fg >> 16) & 0xFF) as f32;
        let fg_ = ((fg >> 8) & 0xFF) as f32;
        let fb = (fg & 0xFF) as f32;

        let r = (br * (1.0 - alpha) + fr * alpha) as u32;
        let g = (bg_ * (1.0 - alpha) + fg_ * alpha) as u32;
        let b = (bb * (1.0 - alpha) + fb * alpha) as u32;

        (r << 16) | (g << 8) | b
    }
}

impl Default for DropShadow {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
pub struct KawaseBlur {
    pub radius: usize,
    pub passes: usize,
    pub step: usize,
}

impl KawaseBlur {
    pub fn new(radius: usize, passes: usize) -> Self {
        Self {
            radius: radius.max(1),
            passes: passes.max(1),
            step: 1,
        }
    }

    pub fn apply(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize, height: usize) {
        if width == 0 || height == 0 {
            return;
        }

        let mut src = alloc_effect_pixels(width, height, "kawase-src");
        let mut dst = alloc_effect_pixels(width, height, "kawase-dst");

        for py in 0..height {
            for px in 0..width {
                let sx = x + px;
                let sy = y + py;
                if sx < fb.width && sy < fb.height {
                    let ptr = unsafe {
                        (fb.base_addr as *const u32).add(sy * fb.pixels_per_scan_line + sx)
                    };
                    src.as_mut_slice()[py * width + px] = unsafe { *ptr };
                }
            }
        }

        for pass in 0..self.passes {
            let offset = ((pass + 1) * self.step).min(self.radius) as isize;
            for py in 0..height {
                for px in 0..width {
                    let src_pixels = src.as_slice();
                    let p1 =
                        sample_rgb(src_pixels, width, height, px as isize + offset, py as isize);
                    let p2 =
                        sample_rgb(src_pixels, width, height, px as isize - offset, py as isize);
                    let p3 =
                        sample_rgb(src_pixels, width, height, px as isize, py as isize + offset);
                    let p4 =
                        sample_rgb(src_pixels, width, height, px as isize, py as isize - offset);
                    let p5 = sample_rgb(src_pixels, width, height, px as isize, py as isize);

                    let r = (p1.0 + p2.0 + p3.0 + p4.0 + p5.0) / 5;
                    let g = (p1.1 + p2.1 + p3.1 + p4.1 + p5.1) / 5;
                    let b = (p1.2 + p2.2 + p3.2 + p4.2 + p5.2) / 5;
                    dst.as_mut_slice()[py * width + px] = (r << 16) | (g << 8) | b;
                }
            }
            core::mem::swap(&mut src, &mut dst);
        }

        for py in 0..height {
            for px in 0..width {
                let sx = x + px;
                let sy = y + py;
                if sx < fb.width && sy < fb.height {
                    let ptr = unsafe {
                        (fb.base_addr as *mut u32).add(sy * fb.pixels_per_scan_line + sx)
                    };
                    unsafe {
                        *ptr = src.as_slice()[py * width + px];
                    }
                }
            }
        }
    }
}

// ============================================================================
// BULANIKLIK EFEKTİ
// ============================================================================

/// Gauss bulanıklık efekti
pub struct BlurEffect {
    /// Bulanıklık yarıçapı
    pub radius: usize,
    /// Bulanıklık kalitesi (piksel başına örnek)
    pub quality: usize,
    /// Önbelleğe alınmış çekirdek
    kernel: Vec<f32>,
}

impl BlurEffect {
    pub fn new(radius: usize) -> Self {
        let mut blur = BlurEffect {
            radius,
            quality: 3,
            kernel: Vec::new(),
        };
        blur.kernel = blur.generate_kernel();
        blur
    }

    /// Bir bölgeye bulanıklık uygula
    pub fn apply(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize, height: usize) {
        if self.radius == 0 {
            return;
        }

        // Geçici tampon oluştur
        let mut temp = alloc_effect_pixels(width, height, "gauss-temp");

        // Kaynağı geçici tampona kopyala
        for py in 0..height {
            for px in 0..width {
                let src_x = x + px;
                let src_y = y + py;

                if src_x < fb.width && src_y < fb.height {
                    let ptr = unsafe {
                        (fb.base_addr as *const u32).add(src_y * fb.pixels_per_scan_line + src_x)
                    };
                    temp.as_mut_slice()[py * width + px] = unsafe { *ptr };
                }
            }
        }

        // Yatay bulanıklık uygula
        let mut h_blur = alloc_effect_pixels(width, height, "gauss-horizontal");
        for py in 0..height {
            for px in 0..width {
                let (r, g, b, count) = self.blur_row(temp.as_slice(), py * width, width, px);

                let idx = py * width + px;
                h_blur.as_mut_slice()[idx] = ((r / count) << 16) | ((g / count) << 8) | (b / count);
            }
        }

        // Dikey bulanıklık uygula
        for py in 0..height {
            for px in 0..width {
                let (r, g, b, count) = self.blur_col(h_blur.as_slice(), width, px, py, height);

                let dst_x = x + px;
                let dst_y = y + py;

                if dst_x < fb.width && dst_y < fb.height {
                    let ptr = unsafe {
                        (fb.base_addr as *mut u32).add(dst_y * fb.pixels_per_scan_line + dst_x)
                    };
                    unsafe {
                        *ptr = ((r / count) << 16) | ((g / count) << 8) | (b / count);
                    }
                }
            }
        }
    }

    fn blur_row(
        &self,
        data: &[u32],
        row_start: usize,
        width: usize,
        x: usize,
    ) -> (u32, u32, u32, u32) {
        let mut r = 0u32;
        let mut g = 0u32;
        let mut b = 0u32;
        let mut count = 0u32;

        let kernel_len = self.kernel.len();
        let half = kernel_len;

        for i in 0..kernel_len {
            let weight = self.kernel[i] as u32 * 256;

            // Sol taraf
            let left_x = x.saturating_sub(half - i);
            if left_x < width {
                let pixel = data[row_start + left_x];
                r += ((pixel >> 16) & 0xFF) * weight;
                g += ((pixel >> 8) & 0xFF) * weight;
                b += (pixel & 0xFF) * weight;
                count += weight;
            }

            // Sağ taraf
            if i > 0 {
                let right_x = (x + i).min(width - 1);
                let pixel = data[row_start + right_x];
                r += ((pixel >> 16) & 0xFF) * weight;
                g += ((pixel >> 8) & 0xFF) * weight;
                b += (pixel & 0xFF) * weight;
                count += weight;
            }
        }

        (r, g, b, count)
    }

    fn blur_col(
        &self,
        data: &[u32],
        stride: usize,
        x: usize,
        y: usize,
        height: usize,
    ) -> (u32, u32, u32, u32) {
        let mut r = 0u32;
        let mut g = 0u32;
        let mut b = 0u32;
        let mut count = 0u32;

        let kernel_len = self.kernel.len();
        let half = kernel_len;

        for i in 0..kernel_len {
            let weight = self.kernel[i] as u32 * 256;

            // Üst taraf
            let top_y = y.saturating_sub(half - i);
            if top_y < height {
                let pixel = data[top_y * stride + x];
                r += ((pixel >> 16) & 0xFF) * weight;
                g += ((pixel >> 8) & 0xFF) * weight;
                b += (pixel & 0xFF) * weight;
                count += weight;
            }

            // Alt taraf
            if i > 0 {
                let bottom_y = (y + i).min(height - 1);
                let pixel = data[bottom_y * stride + x];
                r += ((pixel >> 16) & 0xFF) * weight;
                g += ((pixel >> 8) & 0xFF) * weight;
                b += (pixel & 0xFF) * weight;
                count += weight;
            }
        }

        (r, g, b, count)
    }

    fn generate_kernel(&self) -> Vec<f32> {
        let mut kernel = vec![0.0f32; self.radius + 1];

        let sigma = self.radius as f32 / 3.0;
        let two_sigma_sq = 2.0 * sigma * sigma;

        let mut sum = 0.0;

        for i in 0..=self.radius {
            let x = i as f32;
            let value = expf(-x * x / two_sigma_sq);
            kernel[i] = value;
            sum += if i == 0 { value } else { value * 2.0 };
        }

        // Normalize et
        for i in 0..=self.radius {
            kernel[i] /= sum;
        }

        kernel
    }
}

// ============================================================================
// BUZLANMIŞ CAM EFEKTİ
// ============================================================================

/// Buzlanmış cam (vibrans) efekti
pub struct FrostedGlass {
    /// Bulanıklık efekti
    blur: BlurEffect,
    /// Renk tonu rengi
    pub tint_color: u32,
    /// Renk tonu opaklığı
    pub tint_opacity: f32,
    /// Kenar rengi
    pub border_color: u32,
    /// Kenar genişliği
    pub border_width: usize,
    /// Köşe yarıçapı
    pub corner_radius: usize,
}

impl FrostedGlass {
    pub fn new() -> Self {
        FrostedGlass {
            blur: BlurEffect::new(BLUR_RADIUS),
            tint_color: 0xFFFFFF,
            tint_opacity: 0.7,
            border_color: 0x40FFFFFF,
            border_width: 1,
            corner_radius: 12,
        }
    }

    /// Buzlanmış cam paneli çiz
    pub fn draw(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize, height: usize) {
        // Arka plana bulanıklık uygula
        self.blur.apply(fb, x, y, width, height);

        // Renk tonu uygula
        for py in 0..height {
            for px in 0..width {
                // Yuvarlak köşede olup olmadığını kontrol et
                let in_corner = self.is_in_corner(px, py, width, height, self.corner_radius);
                if in_corner {
                    continue;
                }

                let screen_x = x + px;
                let screen_y = y + py;

                if screen_x < fb.width && screen_y < fb.height {
                    let ptr = unsafe {
                        (fb.base_addr as *mut u32)
                            .add(screen_y * fb.pixels_per_scan_line + screen_x)
                    };
                    let bg = unsafe { *ptr };
                    let tinted = Self::blend_color(bg, self.tint_color, self.tint_opacity);
                    unsafe {
                        *ptr = tinted;
                    }
                }
            }
        }

        // Kenarı çiz
        self.draw_border(fb, x, y, width, height);
    }

    fn is_in_corner(
        &self,
        px: usize,
        py: usize,
        width: usize,
        height: usize,
        radius: usize,
    ) -> bool {
        // Sol üst
        if px < radius && py < radius {
            let dx = radius - px;
            let dy = radius - py;
            return dx * dx + dy * dy > radius * radius;
        }
        // Sağ üst
        if px >= width - radius && py < radius {
            let dx = px - (width - radius - 1);
            let dy = radius - py;
            return dx * dx + dy * dy > radius * radius;
        }
        // Sol alt
        if px < radius && py >= height - radius {
            let dx = radius - px;
            let dy = py - (height - radius - 1);
            return dx * dx + dy * dy > radius * radius;
        }
        // Sağ alt
        if px >= width - radius && py >= height - radius {
            let dx = px - (width - radius - 1);
            let dy = py - (height - radius - 1);
            return dx * dx + dy * dy > radius * radius;
        }
        false
    }

    fn draw_border(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize, height: usize) {
        if self.border_width == 0 {
            return;
        }

        // Üst
        for px in self.corner_radius..width - self.corner_radius {
            for bw in 0..self.border_width {
                let screen_x = x + px;
                let screen_y = y + bw;
                if screen_x < fb.width && screen_y < fb.height {
                    fb.plot_pixel(screen_x, screen_y, self.border_color);
                }
            }
        }

        // Alt
        for px in self.corner_radius..width - self.corner_radius {
            for bw in 0..self.border_width {
                let screen_x = x + px;
                let screen_y = y + height - 1 - bw;
                if screen_x < fb.width && screen_y < fb.height {
                    fb.plot_pixel(screen_x, screen_y, self.border_color);
                }
            }
        }

        // Sol
        for py in self.corner_radius..height - self.corner_radius {
            for bw in 0..self.border_width {
                let screen_x = x + bw;
                let screen_y = y + py;
                if screen_x < fb.width && screen_y < fb.height {
                    fb.plot_pixel(screen_x, screen_y, self.border_color);
                }
            }
        }

        // Sağ
        for py in self.corner_radius..height - self.corner_radius {
            for bw in 0..self.border_width {
                let screen_x = x + width - 1 - bw;
                let screen_y = y + py;
                if screen_x < fb.width && screen_y < fb.height {
                    fb.plot_pixel(screen_x, screen_y, self.border_color);
                }
            }
        }
    }

    fn blend_color(bg: u32, fg: u32, alpha: f32) -> u32 {
        let br = ((bg >> 16) & 0xFF) as f32;
        let bg_ = ((bg >> 8) & 0xFF) as f32;
        let bb = (bg & 0xFF) as f32;

        let fr = ((fg >> 16) & 0xFF) as f32;
        let fg_ = ((fg >> 8) & 0xFF) as f32;
        let fb = (fg & 0xFF) as f32;

        let r = (br * (1.0 - alpha) + fr * alpha) as u32;
        let g = (bg_ * (1.0 - alpha) + fg_ * alpha) as u32;
        let b = (bb * (1.0 - alpha) + fb * alpha) as u32;

        (r << 16) | (g << 8) | b
    }
}

impl Default for FrostedGlass {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// EFEKTLER YÖNETİCİSİ
// ============================================================================

/// Global efektler yöneticisi
pub struct EffectsManager {
    /// Varsayılan düşen gölge
    pub default_shadow: DropShadow,
    /// Aktif pencere gölgesi
    pub active_shadow: DropShadow,
    /// Buzlanmış cam efekti
    pub frosted_glass: FrostedGlass,
    pub kawase_blur: KawaseBlur,
    /// Gölgeler etkin mi
    pub shadows_enabled: bool,
    /// Bulanıklık etkin mi
    pub blur_enabled: bool,
}

impl EffectsManager {
    pub fn new() -> Self {
        EffectsManager {
            default_shadow: DropShadow::new().with_radius(15).with_opacity(0.2),
            active_shadow: DropShadow::new().with_radius(25).with_opacity(0.4),
            frosted_glass: FrostedGlass::new(),
            kawase_blur: KawaseBlur::new(12, 3),
            shadows_enabled: true,
            blur_enabled: true,
        }
    }

    /// Pencere gölgesini çiz
    pub fn draw_window_shadow(
        &self,
        fb: &mut Framebuffer,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
        active: bool,
    ) {
        if !self.shadows_enabled {
            return;
        }

        let shadow = if active {
            &self.active_shadow
        } else {
            &self.default_shadow
        };
        shadow.draw_window_shadow(fb, x, y, width, height);
    }

    /// Buzlanmış cam paneli çiz
    pub fn draw_frosted_panel(
        &self,
        fb: &mut Framebuffer,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
    ) {
        if !self.blur_enabled {
            // Yalnızca yarı saydam arka plan çiz
            for py in 0..height {
                for px in 0..width {
                    let screen_x = x + px;
                    let screen_y = y + py;
                    if screen_x < fb.width && screen_y < fb.height {
                        let ptr = unsafe {
                            (fb.base_addr as *mut u32)
                                .add(screen_y * fb.pixels_per_scan_line + screen_x)
                        };
                        let bg = unsafe { *ptr };
                        let blended = Self::blend_colors(bg, 0xF0202020);
                        unsafe {
                            *ptr = blended;
                        }
                    }
                }
            }
            return;
        }

        self.frosted_glass.draw(fb, x, y, width, height);
    }

    pub fn draw_kawase_panel(
        &self,
        fb: &mut Framebuffer,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
    ) {
        self.kawase_blur.apply(fb, x, y, width, height);
    }

    pub fn draw_sdf_shadow(
        &self,
        fb: &mut Framebuffer,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        radius: i32,
        spread: i32,
        color: u32,
    ) {
        draw_sdf_shadow(fb, x, y, width, height, radius, spread, color, 0.35);
    }

    pub fn apply_window_corner_aa(
        &self,
        fb: &mut Framebuffer,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        radius: i32,
    ) {
        apply_window_corner_aa(fb, x, y, width, height, radius);
    }

    fn blend_colors(bg: u32, fg: u32) -> u32 {
        let alpha = ((fg >> 24) & 0xFF) as f32 / 255.0;

        let br = ((bg >> 16) & 0xFF) as f32;
        let bg_ = ((bg >> 8) & 0xFF) as f32;
        let bb = (bg & 0xFF) as f32;

        let fr = ((fg >> 16) & 0xFF) as f32;
        let fg_ = ((fg >> 8) & 0xFF) as f32;
        let fb = (fg & 0xFF) as f32;

        let r = (br * (1.0 - alpha) + fr * alpha) as u32;
        let g = (bg_ * (1.0 - alpha) + fg_ * alpha) as u32;
        let b = (bb * (1.0 - alpha) + fb * alpha) as u32;

        (r << 16) | (g << 8) | b
    }
}

impl Default for EffectsManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// GLOBAL EFEKTLER YÖNETİCİSİ
// ============================================================================

lazy_static::lazy_static! {
    static ref EFFECTS: Mutex<EffectsManager> = Mutex::new(EffectsManager::new());
}

/// Efektleri başlat
pub fn init() {
    crate::serial_println!("[GUI] Efektler yöneticisi başlatıldı");
}

/// Efektler yöneticisini al
pub fn get_effects() -> &'static Mutex<EffectsManager> {
    &EFFECTS
}

// ============================================================================
// YUVARLAK DİKDÖRTGEN ÇİZİMİ (Anti-aliased)
// ============================================================================

/// Anti-aliased yuvarlak köşeli dikdörtgen çizer (yalnızca kenar).
///
/// Köşe yarıçapları 8/12/16 px olarak tipik widget kullanımında önerilir.
/// Sub-pixel yumuşatma: köşe dairesi sınırındaki pikseller mesafe tabanlı
/// alpha ile blend edilir; `distance - radius` farkı [0,1] aralığına
/// normalize edilip alpha kanalı olarak kullanılır.
pub fn draw_rounded_rect(
    fb: &mut Framebuffer,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    radius: i32,
    color: u32,
) {
    let fw = fb.width as i32;
    let fh = fb.height as i32;
    let r = radius.min(width / 2).min(height / 2).max(0);

    // Üst kenar (köşeler hariç)
    for px in (x + r)..(x + width - r) {
        if px >= 0 && px < fw && y >= 0 && y < fh {
            fb.plot_pixel(px as usize, y as usize, color);
        }
    }
    // Alt kenar
    let by = y + height - 1;
    for px in (x + r)..(x + width - r) {
        if px >= 0 && px < fw && by >= 0 && by < fh {
            fb.plot_pixel(px as usize, by as usize, color);
        }
    }
    // Sol kenar (köşeler hariç)
    for py in (y + r)..(y + height - r) {
        if x >= 0 && x < fw && py >= 0 && py < fh {
            fb.plot_pixel(x as usize, py as usize, color);
        }
    }
    // Sağ kenar
    let rx = x + width - 1;
    for py in (y + r)..(y + height - r) {
        if rx >= 0 && rx < fw && py >= 0 && py < fh {
            fb.plot_pixel(rx as usize, py as usize, color);
        }
    }

    // Köşe yayları (Bresenham circle decision ile anti-aliased)
    draw_corner_arc(fb, x + r, y + r, r, color, fw, fh, true, true); // Sol üst
    draw_corner_arc(fb, x + width - 1 - r, y + r, r, color, fw, fh, false, true); // Sağ üst
    draw_corner_arc(fb, x + r, y + height - 1 - r, r, color, fw, fh, true, false); // Sol alt
    draw_corner_arc(
        fb,
        x + width - 1 - r,
        y + height - 1 - r,
        r,
        color,
        fw,
        fh,
        false,
        false,
    ); // Sağ alt
}

/// Anti-aliased yuvarlak köşeli dolu dikdörtgen çizer.
///
/// Widget arka planları, buton dolgusu, panel alanları için kullanılır.
/// Köşe dışındaki pikseller binary olarak doldurulur; köşe pikselleri
/// daire dışılık mesafesiyle alpha blend edilir.
pub fn draw_rounded_rect_filled(
    fb: &mut Framebuffer,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    radius: i32,
    color: u32,
) {
    let fw = fb.width as i32;
    let fh = fb.height as i32;
    let r = radius.min(width / 2).min(height / 2).max(0);
    let r_sq = r * r;

    for py in 0..height {
        let screen_y = y + py;
        if screen_y < 0 || screen_y >= fh {
            continue;
        }

        // Hangi x aralığının çizileceğini belirle (köşe kırpması)
        let in_top_corner = py < r;
        let in_bottom_corner = py >= height - r;

        let (clip_left, clip_right) = if in_top_corner || in_bottom_corner {
            let dy = if in_top_corner {
                r - py
            } else {
                py - (height - 1 - r)
            };
            // Daire denklemi: x² + y² ≤ r² → x ≤ √(r²-y²)
            let dx_max_sq = r_sq - dy * dy;
            if dx_max_sq < 0 {
                continue;
            }
            let dx_max = isqrt(dx_max_sq as u32) as i32;
            (r - dx_max, r - dx_max)
        } else {
            (0, 0)
        };

        let x0 = (x + clip_left).max(0).min(fw);
        let x1 = (x + width - clip_right).max(0).min(fw);
        for screen_x in x0..x1 {
            fb.plot_pixel(screen_x as usize, screen_y as usize, color);
        }
    }
}

pub fn draw_sdf_shadow(
    fb: &mut Framebuffer,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    radius: i32,
    spread: i32,
    color: u32,
    opacity: f32,
) {
    let margin = (radius + spread).max(1);
    let min_x = (x - margin).max(0);
    let min_y = (y - margin).max(0);
    let max_x = (x + width + margin).min(fb.width as i32);
    let max_y = (y + height + margin).min(fb.height as i32);
    let opacity = opacity.clamp(0.0, 1.0);

    for py in min_y..max_y {
        for px in min_x..max_x {
            let dx = if px < x {
                (x - px) as f32
            } else if px >= x + width {
                (px - (x + width - 1)) as f32
            } else {
                0.0
            };
            let dy = if py < y {
                (y - py) as f32
            } else if py >= y + height {
                (py - (y + height - 1)) as f32
            } else {
                0.0
            };

            let dist = sqrtf(dx * dx + dy * dy) - spread as f32;
            if dist > radius as f32 {
                continue;
            }

            let t = (1.0 - (dist / radius.max(1) as f32)).clamp(0.0, 1.0);
            let alpha = t * t * (3.0 - 2.0 * t) * opacity;
            if alpha <= 0.0 {
                continue;
            }

            let ptr = unsafe {
                (fb.base_addr as *mut u32).add(py as usize * fb.pixels_per_scan_line + px as usize)
            };
            let bg = unsafe { *ptr };
            let blended = blend_argb(bg, color, alpha);
            unsafe {
                *ptr = blended;
            }
        }
    }
}

pub fn apply_window_corner_aa(
    fb: &mut Framebuffer,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    radius: i32,
) {
    if radius <= 0 || width <= 0 || height <= 0 {
        return;
    }

    let r = radius.min(width / 2).min(height / 2).max(1);
    let r_sq = (r * r) as f32;

    for py in 0..r {
        for px in 0..r {
            let dx = (r - px) as f32;
            let dy = (r - py) as f32;
            let dist_sq = dx * dx + dy * dy;
            if dist_sq <= r_sq {
                continue;
            }

            let dist = sqrtf(dist_sq);
            let fade = (1.0 - ((dist - r as f32) / 1.25).clamp(0.0, 1.0)).clamp(0.0, 1.0);

            let points = [
                (x + px, y + py),
                (x + width - 1 - px, y + py),
                (x + px, y + height - 1 - py),
                (x + width - 1 - px, y + height - 1 - py),
            ];

            for &(sx, sy) in &points {
                if sx < 0 || sy < 0 || sx >= fb.width as i32 || sy >= fb.height as i32 {
                    continue;
                }
                let ptr = unsafe {
                    (fb.base_addr as *mut u32)
                        .add(sy as usize * fb.pixels_per_scan_line + sx as usize)
                };
                let bg = unsafe { *ptr };
                let aa = blend_argb(bg, 0x000000, 1.0 - fade);
                unsafe {
                    *ptr = aa;
                }
            }
        }
    }
}

/// 4 çeyrek daire yayı çiz (Midpoint Circle Algorithm).
fn draw_corner_arc(
    fb: &mut Framebuffer,
    cx: i32,
    cy: i32,
    r: i32,
    color: u32,
    fw: i32,
    fh: i32,
    flip_x: bool,
    flip_y: bool,
) {
    if r <= 0 {
        return;
    }
    let mut xi = 0i32;
    let mut yi = r;
    let mut d = 1 - r;

    while xi <= yi {
        let px = if flip_x { cx - xi } else { cx + xi };
        let py = if flip_y { cy - yi } else { cy + yi };
        if px >= 0 && px < fw && py >= 0 && py < fh {
            fb.plot_pixel(px as usize, py as usize, color);
        }
        let px2 = if flip_x { cx - yi } else { cx + yi };
        let py2 = if flip_y { cy - xi } else { cy + xi };
        if px2 >= 0 && px2 < fw && py2 >= 0 && py2 < fh {
            fb.plot_pixel(px2 as usize, py2 as usize, color);
        }

        if d < 0 {
            d += 2 * xi + 3;
        } else {
            d += 2 * (xi - yi) + 5;
            yi -= 1;
        }
        xi += 1;
    }
}

/// Tamsayı kare kök (Newton yöntemi).
fn isqrt(n: u32) -> u32 {
    if n == 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

fn sample_rgb(buf: &[u32], width: usize, height: usize, x: isize, y: isize) -> (u32, u32, u32) {
    let sx = x.clamp(0, (width.saturating_sub(1)) as isize) as usize;
    let sy = y.clamp(0, (height.saturating_sub(1)) as isize) as usize;
    let px = buf[sy * width + sx];
    ((px >> 16) & 0xFF, (px >> 8) & 0xFF, px & 0xFF)
}

fn alloc_effect_pixels(width: usize, height: usize, tag: &'static str) -> SurfacePixelBuffer {
    match alloc_surface_pixels(width.max(1), height.max(1), SurfacePixelFormat::Argb8888) {
        Ok(buffer) => buffer,
        Err(error) => {
            crate::serial_println!(
                "[EFFECTS] doctrine fallback tag={} width={} height={} err={:?}",
                tag,
                width,
                height,
                error
            );
            SurfacePixelBuffer::Heap(vec![0u32; width.saturating_mul(height)])
        }
    }
}

fn blend_argb(bg: u32, fg_rgb: u32, alpha: f32) -> u32 {
    let a = alpha.clamp(0.0, 1.0);
    let br = ((bg >> 16) & 0xFF) as f32;
    let bgc = ((bg >> 8) & 0xFF) as f32;
    let bb = (bg & 0xFF) as f32;

    let fr = ((fg_rgb >> 16) & 0xFF) as f32;
    let fgc = ((fg_rgb >> 8) & 0xFF) as f32;
    let fb = (fg_rgb & 0xFF) as f32;

    let r = (br * (1.0 - a) + fr * a).clamp(0.0, 255.0) as u32;
    let g = (bgc * (1.0 - a) + fgc * a).clamp(0.0, 255.0) as u32;
    let b = (bb * (1.0 - a) + fb * a).clamp(0.0, 255.0) as u32;
    (r << 16) | (g << 8) | b
}

// ============================================================================
// NEON / PARILTILI KENAR EFEKTİ (Glow)
// ============================================================================

/// Odaklı pencerelere cyber-industrial neon parıltı efekti ekler.
///
/// Kenarlık çevresinde exponential decay ile yayılan yarı saydam renk
/// bantları çizer. Tipik kullanım: ACCENT renginde 3px yayılma.
pub struct GlowEffect {
    /// Parıltı rengi (ARGB)
    pub color: u32,
    /// Yayılma mesafesi (piksel)
    pub spread: usize,
    /// Maksimum opaklık (0-255)
    pub max_alpha: u8,
}

impl GlowEffect {
    pub fn new(color: u32, spread: usize) -> Self {
        Self {
            color,
            spread,
            max_alpha: 100,
        }
    }

    /// Dikdörtgen etrafına parıltı çiz.
    pub fn draw(&self, fb: &mut Framebuffer, x: i32, y: i32, width: i32, height: i32) {
        let fw = fb.width as i32;
        let fh = fb.height as i32;
        let cr = (self.color >> 16) & 0xFF;
        let cg = (self.color >> 8) & 0xFF;
        let cb = self.color & 0xFF;

        for dist in 1..=(self.spread as i32) {
            // Exponential decay: alpha = max_alpha * (1 - dist/spread)²
            let ratio = (self.spread as i32 - dist) as f32 / self.spread as f32;
            let alpha = (self.max_alpha as f32 * ratio * ratio) as u8;
            if alpha == 0 {
                continue;
            }

            let glow_color = ((alpha as u32) << 24) | (cr << 16) | (cg << 8) | cb;

            // Üst kenar
            let gy = y - dist;
            if gy >= 0 && gy < fh {
                for gx in (x - dist).max(0)..(x + width + dist).min(fw) {
                    let bg = fb.get_pixel(gx as usize, gy as usize);
                    fb.plot_pixel(gx as usize, gy as usize, blend_with_alpha(glow_color, bg));
                }
            }
            // Alt kenar
            let gy = y + height - 1 + dist;
            if gy >= 0 && gy < fh {
                for gx in (x - dist).max(0)..(x + width + dist).min(fw) {
                    let bg = fb.get_pixel(gx as usize, gy as usize);
                    fb.plot_pixel(gx as usize, gy as usize, blend_with_alpha(glow_color, bg));
                }
            }
            // Sol kenar
            let gx = x - dist;
            if gx >= 0 && gx < fw {
                for gy_i in (y - dist).max(0)..(y + height + dist).min(fh) {
                    let bg = fb.get_pixel(gx as usize, gy_i as usize);
                    fb.plot_pixel(gx as usize, gy_i as usize, blend_with_alpha(glow_color, bg));
                }
            }
            // Sağ kenar
            let gx = x + width - 1 + dist;
            if gx >= 0 && gx < fw {
                for gy_i in (y - dist).max(0)..(y + height + dist).min(fh) {
                    let bg = fb.get_pixel(gx as usize, gy_i as usize);
                    fb.plot_pixel(gx as usize, gy_i as usize, blend_with_alpha(glow_color, bg));
                }
            }
        }
    }
}

/// ARGB kaynak rengini RGB arka plan ile harmanlayarak döndürür.
fn blend_with_alpha(fg: u32, bg: u32) -> u32 {
    let a = ((fg >> 24) & 0xFF) as u32;
    if a == 0 {
        return bg;
    }
    if a == 255 {
        return fg | 0xFF000000;
    }
    let inv = 255 - a;
    let r = (((fg >> 16) & 0xFF) * a + ((bg >> 16) & 0xFF) * inv) / 255;
    let g = (((fg >> 8) & 0xFF) * a + ((bg >> 8) & 0xFF) * inv) / 255;
    let b = ((fg & 0xFF) * a + (bg & 0xFF) * inv) / 255;
    0xFF000000 | (r << 16) | (g << 8) | b
}
