//! # Masaüstü Duvar Kağıtları
//!
//! Geçişler ve dinamik arka planlarla duvar kağıdı yönetimi.
//! Görüntüler, degradeler ve dinamik içerik desteklenir.
//!
//! ## Mimari
//! - `WallpaperType`: Solid, Gradient, RadialGradient, Image, Dynamic, Slideshow, Animated varyantları
//! - `TransitionType`: Fade, CrossFade, SlideLeft/Right/Up/Down, Zoom, Cube geçiş efektleri
//! - `WallpaperManager`: Duvar kağıdı listesi, aktif indeks, animasyon zamanlayıcısı
//!
//! ## Animasyonlu Türler
//! - `Stars`: Sahte-rastgele yıldız konumları; `sinf(t)` ile titreme (twinkle) efekti
//! - `Aurora`: Çift sinüs dalgası aurora bandı; yeşil/mor renk geçişi
//! - `Waves`: Üç katmanlı sinüs dalgası; okyanus mavisi degrade
//! - `Particles`: `sinf+cosf` ile yüzen parçacıklar; parlama efekti (2x2 piksel)
//!
//! ## Renk Karıştırma
//! `lerp_color(c1, c2, t, alpha)`: Her kanal için doğrusal interpolasyon + alpha ölçekleme.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;
use libm::{sinf, cosf, sqrtf};

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};

// ============================================================================
// DUVAR KAĞIDI SABİTLERİ
// ============================================================================

/// Geçiş animasyonu süresi (saniye)
pub const TRANSITION_DURATION: f32 = 1.0;

/// Rotasyondaki maksimum duvar kağıdı sayısı
pub const MAX_WALLPAPERS: usize = 20;

// ============================================================================
// DUVAR KAĞIDI TÜRÜ
// ============================================================================

/// Duvar kağıdı türleri
#[derive(Clone, Debug)]
pub enum WallpaperType {
    /// Düz renk
    Solid(u32),
    /// Degrade (üstten alta)
    Gradient(u32, u32),
    /// Radyal degrade
    RadialGradient { center_color: u32, edge_color: u32 },
    /// Yoldan görüntü
    Image(String),
    /// Dinamik (zamana bağlı)
    Dynamic {
        day_image: String,
        night_image: String,
    },
    /// Slayt gösterisi
    Slideshow {
        images: Vec<String>,
        interval: f32, // saniye
        shuffle: bool,
    },
    /// Animasyonlu (basit efektler)
    Animated(AnimatedType),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimatedType {
    Stars,
    Particles,
    Waves,
    Aurora,
}

// ============================================================================
// DUVAR KAĞIDI
// ============================================================================

/// Duvar kağıdı yapılandırması
#[derive(Clone, Debug)]
pub struct Wallpaper {
    /// Duvar kağıdı kimliği
    pub id: u32,
    /// Görüntü adı
    pub name: String,
    /// Duvar kağıdı türü
    pub wallpaper_type: WallpaperType,
    /// Şu an aktif mi
    pub active: bool,
    /// Geçiş ilerlemesi (0.0 - 1.0)
    pub transition_progress: f32,
    /// Önceki duvar kağıdı (geçiş için)
    pub previous: Option<u32>,
}

impl Wallpaper {
    pub fn solid(id: u32, name: &str, color: u32) -> Self {
        Wallpaper {
            id,
            name: String::from(name),
            wallpaper_type: WallpaperType::Solid(color),
            active: false,
            transition_progress: 0.0,
            previous: None,
        }
    }

    pub fn gradient(id: u32, name: &str, top: u32, bottom: u32) -> Self {
        Wallpaper {
            id,
            name: String::from(name),
            wallpaper_type: WallpaperType::Gradient(top, bottom),
            active: false,
            transition_progress: 0.0,
            previous: None,
        }
    }

    pub fn image(id: u32, name: &str, path: &str) -> Self {
        Wallpaper {
            id,
            name: String::from(name),
            wallpaper_type: WallpaperType::Image(String::from(path)),
            active: false,
            transition_progress: 0.0,
            previous: None,
        }
    }

    pub fn slideshow(id: u32, name: &str, images: Vec<String>, interval: f32) -> Self {
        Wallpaper {
            id,
            name: String::from(name),
            wallpaper_type: WallpaperType::Slideshow {
                images,
                interval,
                shuffle: false,
            },
            active: false,
            transition_progress: 0.0,
            previous: None,
        }
    }

    pub fn animated(id: u32, name: &str, anim_type: AnimatedType) -> Self {
        Wallpaper {
            id,
            name: String::from(name),
            wallpaper_type: WallpaperType::Animated(anim_type),
            active: false,
            transition_progress: 0.0,
            previous: None,
        }
    }
}

// ============================================================================
// DUVAR KAĞIDI YÖNETİCİSİ
// ============================================================================

/// Duvar kağıdı yöneticisi
pub struct WallpaperManager {
    /// Mevcut duvar kağıtları
    pub wallpapers: Vec<Wallpaper>,
    /// Geçerli duvar kağıdı indeksi
    pub current_index: usize,
    /// Ekran genişliği
    pub screen_width: usize,
    /// Ekran yüksekliği
    pub screen_height: usize,
    /// Geçiş yapılıyor mu
    pub transitioning: bool,
    /// Geçiş türü
    pub transition_type: TransitionType,
    /// Slayt gösterisi zamanlayıcısı
    pub slideshow_timer: f32,
    /// Slayt gösterisindeki geçerli görüntü
    pub slideshow_index: usize,
    /// Animasyon zamanı
    pub anim_time: f32,
    /// Önbelleğe alınmış önceki kare
    pub prev_frame: Vec<u32>,
    /// Önceki kare önbelleğini kullan
    pub use_cache: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransitionType {
    None,
    Fade,
    CrossFade,
    SlideLeft,
    SlideRight,
    SlideUp,
    SlideDown,
    Zoom,
    Cube,
}

impl WallpaperManager {
    pub fn new(screen_width: usize, screen_height: usize) -> Self {
        let mut manager = WallpaperManager {
            wallpapers: Vec::new(),
            current_index: 0,
            screen_width,
            screen_height,
            transitioning: false,
            transition_type: TransitionType::CrossFade,
            slideshow_timer: 0.0,
            slideshow_index: 0,
            anim_time: 0.0,
            prev_frame: Vec::new(),
            use_cache: false,
        };

        manager.add_default_wallpapers();
        manager
    }

    fn add_default_wallpapers(&mut self) {
        // Düz renkler
        self.wallpapers.push(Wallpaper::solid(0, "Solid Black", 0x000000));
        self.wallpapers.push(Wallpaper::solid(1, "Solid Dark", 0x1E1E1E));
        self.wallpapers.push(Wallpaper::solid(2, "Solid Blue", 0x003366));

        // Degradeler
        self.wallpapers.push(Wallpaper::gradient(3, "Sunset", 0xFF6B35, 0x1E1E2E));
        self.wallpapers.push(Wallpaper::gradient(4, "Ocean", 0x006994, 0x001F3F));
        self.wallpapers.push(Wallpaper::gradient(5, "Forest", 0x228B22, 0x0B3D0B));
        self.wallpapers.push(Wallpaper::gradient(6, "Night Sky", 0x0F0F23, 0x000011));
        self.wallpapers.push(Wallpaper::gradient(7, "Dawn", 0xFFB347, 0x87CEEB));
        self.wallpapers.push(Wallpaper::gradient(8, "Dusk", 0x4B0082, 0x191970));

        // Radyal degradeler
        self.wallpapers.push(Wallpaper {
            id: 9,
            name: String::from("Spotlight"),
            wallpaper_type: WallpaperType::RadialGradient {
                center_color: 0x333333,
                edge_color: 0x000000,
            },
            active: false,
            transition_progress: 0.0,
            previous: None,
        });

        // Animasyonlu
        self.wallpapers.push(Wallpaper::animated(10, "Stars", AnimatedType::Stars));
        self.wallpapers.push(Wallpaper::animated(11, "Aurora", AnimatedType::Aurora));
        self.wallpapers.push(Wallpaper::animated(12, "Waves", AnimatedType::Waves));

        // Varsayılanı ata
        if !self.wallpapers.is_empty() {
            self.wallpapers[0].active = true;
        }
    }

    /// Duvar kağıdını indekse göre ayarla
    pub fn set_wallpaper(&mut self, index: usize) {
        if index >= self.wallpapers.len() || index == self.current_index {
            return;
        }

        // Geçişi başlat
        self.wallpapers[self.current_index].active = false;
        self.wallpapers[self.current_index].previous = None;

        self.wallpapers[index].active = true;
        self.wallpapers[index].transition_progress = 0.0;
        self.wallpapers[index].previous = Some(self.wallpapers[self.current_index].id);

        self.current_index = index;
        self.transitioning = true;
        self.use_cache = true;
    }

    /// Duvar kağıdını kimliğe göre ayarla
    pub fn set_wallpaper_by_id(&mut self, id: u32) {
        if let Some(index) = self.wallpapers.iter().position(|w| w.id == id) {
            self.set_wallpaper(index);
        }
    }

    /// Sonraki duvar kağıdı
    pub fn next_wallpaper(&mut self) {
        let next = (self.current_index + 1) % self.wallpapers.len();
        self.set_wallpaper(next);
    }

    /// Önceki duvar kağıdı
    pub fn prev_wallpaper(&mut self) {
        let prev = if self.current_index == 0 { self.wallpapers.len() - 1 } else { self.current_index - 1 };
        self.set_wallpaper(prev);
    }

    /// Özel duvar kağıdı ekle
    pub fn add_wallpaper(&mut self, wallpaper: Wallpaper) {
        if self.wallpapers.len() < MAX_WALLPAPERS {
            self.wallpapers.push(wallpaper);
        }
    }

    /// Duvar kağıdını kaldır
    pub fn remove_wallpaper(&mut self, id: u32) {
        if let Some(index) = self.wallpapers.iter().position(|w| w.id == id) {
            if self.wallpapers.len() > 1 {
                if index == self.current_index {
                    self.next_wallpaper();
                }
                self.wallpapers.remove(index);
                if index < self.current_index {
                    self.current_index -= 1;
                }
            }
        }
    }

    /// Animasyonu ve geçişleri güncelle
    pub fn update(&mut self, dt: f32) {
        self.anim_time += dt;

        // Geçişi güncelle
        if self.transitioning {
            self.wallpapers[self.current_index].transition_progress += dt / TRANSITION_DURATION;

            if self.wallpapers[self.current_index].transition_progress >= 1.0 {
                self.wallpapers[self.current_index].transition_progress = 1.0;
                self.transitioning = false;
                self.use_cache = false;
            }
        }

        // Slayt gösterisini güncelle
        if let Some(wallpaper) = self.wallpapers.get(self.current_index) {
            if let WallpaperType::Slideshow { interval, .. } = &wallpaper.wallpaper_type {
                self.slideshow_timer += dt;
                if self.slideshow_timer >= *interval {
                    self.slideshow_timer = 0.0;
                    // Sonraki görüntüye geçilecek
                }
            }
        }
    }

    /// Duvar kağıdını çiz
    pub fn draw(&self, fb: &mut Framebuffer) {
        if self.wallpapers.is_empty() {
            // Varsayılan renkle doldur
            for y in 0..fb.height {
                for x in 0..fb.width {
                    fb.plot_pixel(x, y, Theme::DESKTOP_BG.to_u32());
                }
            }
            return;
        }

        let wallpaper = &self.wallpapers[self.current_index];

        // Geçiş sırasında önceki duvar kağıdını çiz
        if self.transitioning && self.use_cache {
            // Önbelleğe alınmış önceki kareyi geçiş efektiyle çiz
            self.draw_transition(fb, wallpaper);
        } else {
            // Geçerli duvar kağıdını çiz
            self.draw_wallpaper_type(fb, &wallpaper.wallpaper_type, 1.0);
        }
    }

    fn draw_transition(&self, fb: &mut Framebuffer, wallpaper: &Wallpaper) {
        let progress = wallpaper.transition_progress;

        match self.transition_type {
            TransitionType::Fade | TransitionType::CrossFade => {
                // Öncekini çiz (soluklaşıyor)
                if let Some(prev_id) = wallpaper.previous {
                    if let Some(prev) = self.wallpapers.iter().find(|w| w.id == prev_id) {
                        self.draw_wallpaper_type(fb, &prev.wallpaper_type, 1.0 - progress);
                    }
                }
                // Geçerlisini çiz (belirginleşiyor)
                self.draw_wallpaper_type(fb, &wallpaper.wallpaper_type, progress);
            }
            TransitionType::SlideLeft => {
                // Önceki sola kayıyor
                let offset = (self.screen_width as f32 * progress) as i32;

                if let Some(prev_id) = wallpaper.previous {
                    if let Some(prev) = self.wallpapers.iter().find(|w| w.id == prev_id) {
                        // Öncekini ofsetle çiz
                        self.draw_wallpaper_offset(fb, &prev.wallpaper_type, -offset);
                    }
                }
                // Geçerlisini sağdan çiz
                self.draw_wallpaper_offset(fb, &wallpaper.wallpaper_type, self.screen_width as i32 - offset);
            }
            TransitionType::SlideRight => {
                let offset = (self.screen_width as f32 * progress) as i32;

                if let Some(prev_id) = wallpaper.previous {
                    if let Some(prev) = self.wallpapers.iter().find(|w| w.id == prev_id) {
                        self.draw_wallpaper_offset(fb, &prev.wallpaper_type, offset);
                    }
                }
                self.draw_wallpaper_offset(fb, &wallpaper.wallpaper_type, -(self.screen_width as i32) + offset);
            }
            _ => {
                // Varsayılan: çapraz geçiş
                self.draw_wallpaper_type(fb, &wallpaper.wallpaper_type, progress);
            }
        }
    }

    fn draw_wallpaper_type(&self, fb: &mut Framebuffer, wallpaper_type: &WallpaperType, alpha: f32) {
        match wallpaper_type {
            WallpaperType::Solid(color) => {
                let color = Self::alpha_color(*color, alpha);
                for y in 0..fb.height {
                    for x in 0..fb.width {
                        fb.plot_pixel(x, y, color);
                    }
                }
            }
            WallpaperType::Gradient(top, bottom) => {
                for y in 0..fb.height {
                    let t = y as f32 / fb.height as f32;
                    let color = Self::lerp_color(*top, *bottom, t, alpha);

                    for x in 0..fb.width {
                        fb.plot_pixel(x, y, color);
                    }
                }
            }
            WallpaperType::RadialGradient { center_color, edge_color } => {
                let center_x = fb.width / 2;
                let center_y = fb.height / 2;
                let max_dist = sqrtf((center_x * center_x + center_y * center_y) as f32);

                for y in 0..fb.height {
                    for x in 0..fb.width {
                        let dx = x as i32 - center_x as i32;
                        let dy = y as i32 - center_y as i32;
                        let dist = sqrtf((dx * dx + dy * dy) as f32);
                        let t = (dist / max_dist).min(1.0);

                        let color = Self::lerp_color(*center_color, *edge_color, t, alpha);
                        fb.plot_pixel(x, y, color);
                    }
                }
            }
            WallpaperType::Image(_path) => {
                // Görüntü yüklenip çizilecek - düz renge geri dön
                self.draw_wallpaper_type(fb, &WallpaperType::Solid(Theme::DESKTOP_BG.to_u32()), alpha);
            }
            WallpaperType::Dynamic { day_image, night_image: _ } => {
                // Zamanı kontrol edip uygun görüntüyü çizecek
                self.draw_wallpaper_type(fb, &WallpaperType::Image(day_image.clone()), alpha);
            }
            WallpaperType::Slideshow { images, .. } => {
                if !images.is_empty() {
                    let idx = self.slideshow_index.min(images.len() - 1);
                    self.draw_wallpaper_type(fb, &WallpaperType::Image(images[idx].clone()), alpha);
                }
            }
            WallpaperType::Animated(anim_type) => {
                self.draw_animated(fb, *anim_type, alpha);
            }
        }
    }

    fn draw_wallpaper_offset(&self, fb: &mut Framebuffer, wallpaper_type: &WallpaperType, offset: i32) {
        // Duvar kağıdını yatay ofsete göre çiz
        match wallpaper_type {
            WallpaperType::Solid(color) => {
                for y in 0..fb.height {
                    let start_x = offset.max(0) as usize;
                    let end_x = (fb.width as i32 + offset).min(fb.width as i32) as usize;

                    for x in start_x..end_x {
                        fb.plot_pixel(x, y, *color);
                    }
                }
            }
            WallpaperType::Gradient(top, bottom) => {
                for y in 0..fb.height {
                    let t = y as f32 / fb.height as f32;
                    let color = Self::lerp_color(*top, *bottom, t, 1.0);

                    let start_x = offset.max(0) as usize;
                    let end_x = (fb.width as i32 + offset).min(fb.width as i32) as usize;

                    for x in start_x..end_x {
                        fb.plot_pixel(x, y, color);
                    }
                }
            }
            _ => {
                self.draw_wallpaper_type(fb, wallpaper_type, 1.0);
            }
        }
    }

    fn draw_animated(&self, fb: &mut Framebuffer, anim_type: AnimatedType, alpha: f32) {
        // Temel degrade arka plan
        let base_top = 0x0F0F23;
        let base_bottom = 0x000011;

        for y in 0..fb.height {
            let t = y as f32 / fb.height as f32;
            let base_color = Self::lerp_color(base_top, base_bottom, t, alpha);

            for x in 0..fb.width {
                fb.plot_pixel(x, y, base_color);
            }
        }

        match anim_type {
            AnimatedType::Stars => {
                self.draw_stars(fb, alpha);
            }
            AnimatedType::Aurora => {
                self.draw_aurora(fb, alpha);
            }
            AnimatedType::Waves => {
                self.draw_waves(fb, alpha);
            }
            AnimatedType::Particles => {
                self.draw_particles(fb, alpha);
            }
        }
    }

    fn draw_stars(&self, fb: &mut Framebuffer, alpha: f32) {
        // Basit yıldız alanı animasyonu
        let time = self.anim_time;

        // Konuma göre sahte-rastgele yıldızlar oluştur
        for i in 0..200 {
            let seed = i * 7919;
            let x = seed % fb.width;
            let y = (seed * 3) % fb.height;

            // Titreme efekti
            let twinkle = (sinf(time * 2.0 + seed as f32 * 0.1) + 1.0) / 2.0;
            let brightness = (0.3 + 0.7 * twinkle) * alpha;

            let star_color = Self::alpha_color(0xFFFFFF, brightness);

            // Yıldızı çiz (küçük nokta)
            if x < fb.width && y < fb.height {
                fb.plot_pixel(x, y, star_color);
                if x + 1 < fb.width {
                    fb.plot_pixel(x + 1, y, star_color);
                }
            }
        }
    }

    fn draw_aurora(&self, fb: &mut Framebuffer, alpha: f32) {
        let time = self.anim_time;

        for y in 0..fb.height {
            for x in 0..fb.width {
                // Aurora dalga efekti
                let wave1 = (sinf(x as f32 * 0.01 + time * 0.5) * 50.0) as i32;
                let wave2 = (sinf(x as f32 * 0.02 + time * 0.3) * 30.0) as i32;

                let aurora_y = fb.height as i32 / 2 + wave1 + wave2;

                let dist = (y as i32 - aurora_y).abs();

                if dist < 80 {
                    let intensity = (1.0 - dist as f32 / 80.0) * alpha * 0.4;

                    // Aurora renkleri (yeşil/mor)
                    let hue = (x as f32 * 0.003 + time * 0.2) % 1.0;
                    let color = if hue < 0.5 {
                        Self::lerp_color(0x00FF88, 0x8800FF, hue * 2.0, intensity)
                    } else {
                        Self::lerp_color(0x8800FF, 0x00FF88, (hue - 0.5) * 2.0, intensity)
                    };

                    let ptr = unsafe { (fb.base_addr as *mut u32).add(y * fb.pixels_per_scan_line + x) };
                    let bg = unsafe { *ptr };
                    unsafe { *ptr = Self::blend_color(bg, color); }
                }
            }
        }
    }

    fn draw_waves(&self, fb: &mut Framebuffer, alpha: f32) {
        let time = self.anim_time;

        for y in 0..fb.height {
            for x in 0..fb.width {
                // Çoklu dalga katmanları
                let wave1 = (sinf(x as f32 * 0.02 + time * 1.5) * 20.0) as i32;
                let wave2 = (sinf(x as f32 * 0.03 - time * 1.2) * 15.0) as i32;
                let wave3 = (sinf(x as f32 * 0.01 + time * 0.8) * 25.0) as i32;

                let wave_y = fb.height as i32 - 100 + wave1 + wave2 + wave3;

                if y as i32 > wave_y {
                    let depth = (y as i32 - wave_y) as f32;
                    let intensity = (depth / 100.0).min(1.0) * alpha;

                    // Okyanus mavisi degrade
                    let color = Self::lerp_color(0x006994, 0x001F3F, intensity, intensity);

                    let ptr = unsafe { (fb.base_addr as *mut u32).add(y * fb.pixels_per_scan_line + x) };
                    unsafe { *ptr = color; }
                }
            }
        }
    }

    fn draw_particles(&self, fb: &mut Framebuffer, alpha: f32) {
        let time = self.anim_time;

        for i in 0..50 {
            let seed = i * 1234;
            let base_x = (seed % fb.width) as f32;
            let base_y = ((seed * 7) % fb.height) as f32;

            // Yüzen hareket
            let x = (base_x + sinf(time) * 20.0 + cosf(time) * 15.0) as usize;
            let y = (base_y + sinf(time * 0.5) * 30.0) as usize;

            let x = x % fb.width;
            let y = y % fb.height;

            let color = Self::alpha_color(0xFFFFFF, 0.3 * alpha);

            // Parçacığı parlama efektiyle çiz
            for py in 0..4 {
                for px in 0..4 {
                    let px = x + px;
                    let py = y + py;
                    if px < fb.width && py < fb.height {
                        fb.plot_pixel(px, py, color);
                    }
                }
            }
        }
    }

    fn lerp_color(c1: u32, c2: u32, t: f32, alpha: f32) -> u32 {
        let r1 = ((c1 >> 16) & 0xFF) as f32;
        let g1 = ((c1 >> 8) & 0xFF) as f32;
        let b1 = (c1 & 0xFF) as f32;

        let r2 = ((c2 >> 16) & 0xFF) as f32;
        let g2 = ((c2 >> 8) & 0xFF) as f32;
        let b2 = (c2 & 0xFF) as f32;

        let r = (r1 + (r2 - r1) * t) * alpha;
        let g = (g1 + (g2 - g1) * t) * alpha;
        let b = (b1 + (b2 - b1) * t) * alpha;

        (r as u32) << 16 | (g as u32) << 8 | (b as u32)
    }

    fn alpha_color(color: u32, alpha: f32) -> u32 {
        let r = (((color >> 16) & 0xFF) as f32 * alpha) as u32;
        let g = (((color >> 8) & 0xFF) as f32 * alpha) as u32;
        let b = ((color & 0xFF) as f32 * alpha) as u32;
        (r << 16) | (g << 8) | b
    }

    fn blend_color(bg: u32, fg: u32) -> u32 {
        let br = ((bg >> 16) & 0xFF) as f32;
        let bg_ = ((bg >> 8) & 0xFF) as f32;
        let bb = (bg & 0xFF) as f32;

        let fr = ((fg >> 16) & 0xFF) as f32;
        let fg_ = ((fg >> 8) & 0xFF) as f32;
        let fb = (fg & 0xFF) as f32;

        let r = (br + fr).min(255.0) as u32;
        let g = (bg_ + fg_).min(255.0) as u32;
        let b = (bb + fb).min(255.0) as u32;

        (r << 16) | (g << 8) | b
    }

    /// Yeniden boyutlandır
    pub fn resize(&mut self, width: usize, height: usize) {
        self.screen_width = width;
        self.screen_height = height;
    }

    /// Geçerli duvar kağıdı adını al
    pub fn current_name(&self) -> &str {
        &self.wallpapers[self.current_index].name
    }

    /// Ayarlar için duvar kağıdı listesini al
    pub fn get_wallpaper_list(&self) -> Vec<(u32, String)> {
        self.wallpapers.iter().map(|w| (w.id, w.name.clone())).collect()
    }
}

// ============================================================================
// GLOBAL DUVAR KAĞIDI YÖNETİCİSİ
// ============================================================================

lazy_static::lazy_static! {
    static ref WALLPAPER: Mutex<WallpaperManager> = Mutex::new(WallpaperManager::new(1920, 1080));
}

/// Duvar kağıdı yöneticisini başlat
pub fn init(width: usize, height: usize) {
    let mut wallpaper = WALLPAPER.lock();
    wallpaper.resize(width, height);
    crate::serial_println!("[GUI] Wallpaper manager initialized");
}

/// Duvar kağıdı yöneticisine erişim sağla
pub fn get_wallpaper() -> &'static Mutex<WallpaperManager> {
    &WALLPAPER
}
