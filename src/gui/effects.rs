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
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;
use libm::{sqrtf, expf};

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};

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
                            (fb.base_addr as *mut u32).add(screen_y * fb.pixels_per_scan_line + screen_x)
                        };
                        let bg = unsafe { *ptr };
                        let blended = Self::blend_color(bg, self.color, alpha);
                        unsafe { *ptr = blended; }
                    }
                }
            }
        }
    }

    /// Pencere için gölge çiz (optimize - yalnızca köşeler ve kenarlar)
    pub fn draw_window_shadow(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize, height: usize) {
        if self.radius == 0 || self.opacity < 0.01 {
            return;
        }

        let kernel = self.gaussian_kernel(self.radius);
        let r = self.radius;

        // Üst kenar ve köşeler
        for py in 0..r {
            let screen_y = (y as i32 + self.offset_y - r as i32 + py as i32).max(0) as usize;
            if screen_y >= fb.height { continue; }

            for px in 0..width + r * 2 {
                let screen_x = (x as i32 + self.offset_x - r as i32 + px as i32).max(0) as usize;
                if screen_x >= fb.width { continue; }

                // Konuma göre azalmayı hesapla
                let dist_top = py;
                let dist_left = px;
                let dist_right = width + r * 2 - 1 - px;

                // Köşe tespiti
                let in_corner = (px < r && py < r) || (px > width + r && py < r) ||
                               (px < r && py > height + r) || (px > width + r && py > height + r);

                let falloff = if in_corner {
                    // Köşeler için dairesel azalma
                    let cx = if px < r { r - px } else { px - (width + r) };
                    let cy = r - py;
                    let dist = sqrtf((cx * cx + cy * cy) as f32) as usize;
                    if dist < r { kernel[dist] } else { 0.0 }
                } else {
                    // Kenarlar için doğrusal azalma
                    let dist = dist_top.min(dist_left).min(dist_right);
                    if dist < r { kernel[dist] } else { 0.0 }
                };

                if falloff > 0.0 {
                    let alpha = falloff * self.opacity;
                    let ptr = unsafe {
                        (fb.base_addr as *mut u32).add(screen_y * fb.pixels_per_scan_line + screen_x)
                    };
                    let bg = unsafe { *ptr };
                    let blended = Self::blend_color(bg, self.color, alpha);
                    unsafe { *ptr = blended; }
                }
            }
        }

        // Sol kenar
        for px in 0..r {
            let screen_x = (x as i32 + self.offset_x - r as i32 + px as i32).max(0) as usize;
            if screen_x >= fb.width { continue; }

            let falloff = kernel[px];
            let alpha = falloff * self.opacity;

            for screen_y in y..y + height {
                if screen_y >= fb.height { continue; }

                let ptr = unsafe {
                    (fb.base_addr as *mut u32).add(screen_y * fb.pixels_per_scan_line + screen_x)
                };
                let bg = unsafe { *ptr };
                let blended = Self::blend_color(bg, self.color, alpha);
                unsafe { *ptr = blended; }
            }
        }

        // Sağ kenar
        for px in 0..r {
            let screen_x = x + width + self.offset_x as usize + px;
            if screen_x >= fb.width { continue; }

            let falloff = kernel[r - 1 - px];
            let alpha = falloff * self.opacity;

            for screen_y in y..y + height {
                if screen_y >= fb.height { continue; }

                let ptr = unsafe {
                    (fb.base_addr as *mut u32).add(screen_y * fb.pixels_per_scan_line + screen_x)
                };
                let bg = unsafe { *ptr };
                let blended = Self::blend_color(bg, self.color, alpha);
                unsafe { *ptr = blended; }
            }
        }

        // Alt kenar
        for py in 0..r {
            let screen_y = y + height + self.offset_y as usize + py;
            if screen_y >= fb.height { continue; }

            let falloff = kernel[r - 1 - py];
            let alpha = falloff * self.opacity;

            for screen_x in x..x + width {
                if screen_x >= fb.width { continue; }

                let ptr = unsafe {
                    (fb.base_addr as *mut u32).add(screen_y * fb.pixels_per_scan_line + screen_x)
                };
                let bg = unsafe { *ptr };
                let blended = Self::blend_color(bg, self.color, alpha);
                unsafe { *ptr = blended; }
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
        let mut temp = vec![0u32; width * height];

        // Kaynağı geçici tampona kopyala
        for py in 0..height {
            for px in 0..width {
                let src_x = x + px;
                let src_y = y + py;

                if src_x < fb.width && src_y < fb.height {
                    let ptr = unsafe {
                        (fb.base_addr as *const u32).add(src_y * fb.pixels_per_scan_line + src_x)
                    };
                    temp[py * width + px] = unsafe { *ptr };
                }
            }
        }

        // Yatay bulanıklık uygula
        let mut h_blur = vec![0u32; width * height];
        for py in 0..height {
            for px in 0..width {
                let (r, g, b, count) = self.blur_row(&temp, py * width, width, px);

                let idx = py * width + px;
                h_blur[idx] = ((r / count) << 16) | ((g / count) << 8) | (b / count);
            }
        }

        // Dikey bulanıklık uygula
        for py in 0..height {
            for px in 0..width {
                let (r, g, b, count) = self.blur_col(&h_blur, width, px, py, height);

                let dst_x = x + px;
                let dst_y = y + py;

                if dst_x < fb.width && dst_y < fb.height {
                    let ptr = unsafe {
                        (fb.base_addr as *mut u32).add(dst_y * fb.pixels_per_scan_line + dst_x)
                    };
                    unsafe { *ptr = ((r / count) << 16) | ((g / count) << 8) | (b / count); }
                }
            }
        }
    }

    fn blur_row(&self, data: &[u32], row_start: usize, width: usize, x: usize) -> (u32, u32, u32, u32) {
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

    fn blur_col(&self, data: &[u32], stride: usize, x: usize, y: usize, height: usize) -> (u32, u32, u32, u32) {
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
                        (fb.base_addr as *mut u32).add(screen_y * fb.pixels_per_scan_line + screen_x)
                    };
                    let bg = unsafe { *ptr };
                    let tinted = Self::blend_color(bg, self.tint_color, self.tint_opacity);
                    unsafe { *ptr = tinted; }
                }
            }
        }

        // Kenarı çiz
        self.draw_border(fb, x, y, width, height);
    }

    fn is_in_corner(&self, px: usize, py: usize, width: usize, height: usize, radius: usize) -> bool {
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
            shadows_enabled: true,
            blur_enabled: true,
        }
    }

    /// Pencere gölgesini çiz
    pub fn draw_window_shadow(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize, height: usize, active: bool) {
        if !self.shadows_enabled {
            return;
        }

        let shadow = if active { &self.active_shadow } else { &self.default_shadow };
        shadow.draw_window_shadow(fb, x, y, width, height);
    }

    /// Buzlanmış cam paneli çiz
    pub fn draw_frosted_panel(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize, height: usize) {
        if !self.blur_enabled {
            // Yalnızca yarı saydam arka plan çiz
            for py in 0..height {
                for px in 0..width {
                    let screen_x = x + px;
                    let screen_y = y + py;
                    if screen_x < fb.width && screen_y < fb.height {
                        let ptr = unsafe {
                            (fb.base_addr as *mut u32).add(screen_y * fb.pixels_per_scan_line + screen_x)
                        };
                        let bg = unsafe { *ptr };
                        let blended = Self::blend_colors(bg, 0xF0202020);
                        unsafe { *ptr = blended; }
                    }
                }
            }
            return;
        }

        self.frosted_glass.draw(fb, x, y, width, height);
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
