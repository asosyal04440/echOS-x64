//! # Mission Control
//!
//! macOS tarzı pencere genel görünümü ve çalışma alanları.
//! Tüm pencereleri, masaüstü alanlarını (spaces) ve gösterge panosunu görüntüler.
//!
//! ## Mimari
//! - `WindowThumbnail`: Pencere kimliği, başlık, uygulama adı, küçük resim konumu ve animasyon ilerleme değeri
//! - `Space`: Masaüstü alanı; seçim durumu, duvar kağıdı rengi, animasyon
//! - `MissionControl`: Tüm pencereleri ve alanları yöneten, ızgara düzeni ve animasyon işleyen yapı
//!
//! ## Animasyon Algoritması
//! Pencere küçük resimleri orijinal konumlarından hedef ızgara konumuna doğru
//! üstel yumuşatma (dx * 0.15 her karede) ile kayar.
//! `anim_progress` 0→1 arasında artarak opaklık ve ölçeği kontrol eder.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};

// ============================================================================
// MISSION CONTROL SABİTLERİ
// ============================================================================

/// Pencere küçük resim ölçeği
pub const THUMBNAIL_SCALE: f32 = 0.25;

/// Alan küçük resim genişliği (piksel)
pub const SPACE_WIDTH: usize = 200;

/// Alan küçük resim yüksekliği (piksel)
pub const SPACE_HEIGHT: usize = 120;

/// Alan boşlukları (piksel)
pub const SPACE_SPACING: usize = 16;

/// Pencere boşlukları (piksel)
pub const WINDOW_SPACING: usize = 20;

// ============================================================================
// PENCERE KÜÇÜK RESMİ
// ============================================================================

/// Mission Control'de bir pencere küçük resmi
#[derive(Clone, Debug)]
pub struct WindowThumbnail {
    /// Pencere kimliği
    pub window_id: u32,
    /// Pencere başlığı
    pub title: String,
    /// Uygulama adı
    pub app_name: String,
    /// Özgün konum ve boyut
    pub original_rect: (i32, i32, usize, usize), // x, y, g, y
    /// Küçük resim konumu (animasyonlu)
    pub thumbnail_pos: (f32, f32),
    /// Küçük resim boyutu
    pub thumbnail_size: (usize, usize),
    /// Seçili mi
    pub selected: bool,
    /// Animasyon ilerlemesi
    pub anim_progress: f32,
    /// Görünür mü
    pub visible: bool,
    /// Z-sırası
    pub z_order: usize,
    /// Uygulama simgesi
    pub app_icon: AppIcon,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppIcon {
    Finder,
    Safari,
    Mail,
    Terminal,
    Settings,
    Files,
    TextEdit,
    Music,
    Custom(u16),
}

impl WindowThumbnail {
    pub fn new(window_id: u32, title: &str, app_name: &str, x: i32, y: i32, w: usize, h: usize) -> Self {
        // Küçük resim boyutunu hesapla
        let scale = THUMBNAIL_SCALE;
        let thumb_w = (w as f32 * scale) as usize;
        let thumb_h = (h as f32 * scale) as usize;

        WindowThumbnail {
            window_id,
            title: String::from(title),
            app_name: String::from(app_name),
            original_rect: (x, y, w, h),
            thumbnail_pos: (x as f32, y as f32),
            thumbnail_size: (thumb_w.max(150), thumb_h.max(100)),
            selected: false,
            anim_progress: 0.0,
            visible: true,
            z_order: 0,
            app_icon: AppIcon::Finder,
        }
    }

    /// Animasyonu güncelle
    pub fn update(&mut self, dt: f32, target_pos: (f32, f32)) {
        // Yumuşak konum animasyonu (üstel yaklaşım)
        let dx = target_pos.0 - self.thumbnail_pos.0;
        let dy = target_pos.1 - self.thumbnail_pos.1;

        self.thumbnail_pos.0 += dx * 0.15;
        self.thumbnail_pos.1 += dy * 0.15;

        // Animasyon ilerlemesi
        if self.anim_progress < 1.0 {
            self.anim_progress = (self.anim_progress + dt * 3.0).min(1.0);
        }
    }

    /// Küçük resmi çiz
    pub fn draw(&self, fb: &mut Framebuffer) {
        if !self.visible || self.anim_progress < 0.01 {
            return;
        }

        let x = self.thumbnail_pos.0 as usize;
        let y = self.thumbnail_pos.1 as usize;
        let w = self.thumbnail_size.0;
        let h = self.thumbnail_size.1;

        let alpha = self.anim_progress;

        // Gölge çiz
        for sy in 0..8 {
            let shadow_alpha = (0.3 - sy as f32 * 0.04) * alpha;
            let shadow_y = y + h + sy;

            for sx in 0..w {
                let screen_x = x + sx;
                if screen_x < fb.width && shadow_y < fb.height {
                    let ptr = unsafe {
                        (fb.base_addr as *mut u32).add(shadow_y * fb.pixels_per_scan_line + screen_x)
                    };
                    let bg = unsafe { *ptr };
                    unsafe { *ptr = MissionControl::blend_color(bg, 0x000000, shadow_alpha); }
                }
            }
        }

        // Pencere arka planını çiz
        let bg_color = Self::blend_color(Theme::WINDOW_BG.to_u32(), alpha);
        fb.draw_rect(x, y, w, h, bg_color);

        // Başlık çubuğunu çiz
        let titlebar_color = Self::blend_color(Theme::TITLEBAR_BG.to_u32(), alpha);
        fb.draw_rect(x, y, w, 24, titlebar_color);

        // Başlığı çiz
        let title_color = Self::blend_color(Theme::TEXT_PRIMARY.to_u32(), alpha);
        let title_display = if self.title.len() > w / 8 - 4 {
            format!("{}...", &self.title[..w / 8 - 7])
        } else {
            self.title.clone()
        };
        fb.draw_string(x + 8, y + 4, &title_display, title_color);

        // Kapatma düğmesini çiz
        let close_color = Self::blend_color(Theme::ERROR.to_u32(), alpha);
        fb.draw_rect(x + w - 20, y + 4, 16, 16, close_color);

        // Uygulama simgesini çiz
        self.draw_app_icon(fb, x + 4, y + h + 4, self.app_icon);

        // Seçim vurgusunu çiz
        if self.selected {
            let highlight_color = Self::blend_color(Theme::ACCENT_PRIMARY.to_u32(), 0.5 * alpha);
            fb.draw_rect_outline(x - 2, y - 2, w + 4, h + 4, highlight_color);
        }
    }

    fn draw_app_icon(&self, fb: &mut Framebuffer, x: usize, y: usize, icon: AppIcon) {
        let icon_color = match icon {
            AppIcon::Finder => 0xFF3D67FF,
            AppIcon::Safari => 0xFF007AFF,
            AppIcon::Mail => 0xFF007AFF,
            AppIcon::Terminal => 0xFF1E1E1E,
            AppIcon::Settings => 0xFF8E8E93,
            AppIcon::Files => 0xFF007AFF,
            AppIcon::TextEdit => 0xFFFFCC00,
            AppIcon::Music => 0xFFFC3C44,
            AppIcon::Custom(code) => {
                match code % 8 {
                    0 => 0xFFFF3B30,
                    1 => 0xFFFF9500,
                    2 => 0xFFFFCC00,
                    3 => 0xFF34C759,
                    4 => 0xFF00C7BE,
                    5 => 0xFF007AFF,
                    6 => 0xFF5856D6,
                    _ => 0xFFFF2D55,
                }
            }
        };

        // Küçük simge çemberi çiz
        for py in 0..16 {
            for px in 0..16 {
                let dx = px as i32 - 8;
                let dy = py as i32 - 8;
                if dx * dx + dy * dy <= 64 {
                    fb.plot_pixel(x + px, y + py, icon_color);
                }
            }
        }
    }

    fn blend_color(color: u32, alpha: f32) -> u32 {
        let r = (((color >> 16) & 0xFF) as f32 * alpha) as u32;
        let g = (((color >> 8) & 0xFF) as f32 * alpha) as u32;
        let b = ((color & 0xFF) as f32 * alpha) as u32;
        (r << 16) | (g << 8) | b
    }

    /// İsabet testi
    pub fn hit_test(&self, mx: i32, my: i32) -> bool {
        let x = self.thumbnail_pos.0 as i32;
        let y = self.thumbnail_pos.1 as i32;
        let w = self.thumbnail_size.0 as i32;
        let h = self.thumbnail_size.1 as i32;

        mx >= x && mx < x + w && my >= y && my < y + h
    }
}

// ============================================================================
// ALAN (MASAÜSTÜ)
// ============================================================================

/// Mission Control'de bir masaüstü alanı
#[derive(Clone, Debug)]
pub struct Space {
    /// Alan kimliği
    pub id: u32,
    /// Alan adı
    pub name: String,
    /// Küçük resim konumu
    pub pos: (usize, usize),
    /// Seçili mi
    pub selected: bool,
    /// Pencereler var mı
    pub has_windows: bool,
    /// Duvar kağıdı rengi
    pub wallpaper_color: u32,
    /// Animasyon ilerlemesi
    pub anim_progress: f32,
}

impl Space {
    pub fn new(id: u32, name: &str) -> Self {
        Space {
            id,
            name: String::from(name),
            pos: (0, 0),
            selected: false,
            has_windows: false,
            wallpaper_color: Theme::DESKTOP_BG.to_u32(),
            anim_progress: 0.0,
        }
    }

    /// Animasyonu güncelle
    pub fn update(&mut self, dt: f32) {
        if self.anim_progress < 1.0 {
            self.anim_progress = (self.anim_progress + dt * 4.0).min(1.0);
        }
    }

    /// Alan küçük resmini çiz
    pub fn draw(&self, fb: &mut Framebuffer) {
        let x = self.pos.0;
        let y = self.pos.1;
        let w = SPACE_WIDTH;
        let h = SPACE_HEIGHT;

        let alpha = self.anim_progress;

        // Arka plan
        let bg_color = Self::blend_color(self.wallpaper_color, alpha);
        fb.draw_rect(x, y, w, h, bg_color);

        // Kenarlık
        let border_color = if self.selected {
            Self::blend_color(Theme::ACCENT_PRIMARY.to_u32(), alpha)
        } else {
            Self::blend_color(Theme::BORDER.to_u32(), alpha)
        };
        fb.draw_rect_outline(x, y, w, h, border_color);

        // Ad
        let text_color = Self::blend_color(Theme::TEXT_PRIMARY.to_u32(), alpha);
        fb.draw_string(x + 8, y + h + 4, &self.name, text_color);

        // Pencere göstergesi
        if self.has_windows {
            let dot_x = x + w / 2 - 3;
            let dot_y = y + h + 20;
            fb.draw_rect(dot_x, dot_y, 6, 6, border_color);
        }
    }

    fn blend_color(color: u32, alpha: f32) -> u32 {
        let r = (((color >> 16) & 0xFF) as f32 * alpha) as u32;
        let g = (((color >> 8) & 0xFF) as f32 * alpha) as u32;
        let b = ((color & 0xFF) as f32 * alpha) as u32;
        (r << 16) | (g << 8) | b
    }

    /// İsabet testi
    pub fn hit_test(&self, mx: i32, my: i32) -> bool {
        let x = self.pos.0 as i32;
        let y = self.pos.1 as i32;

        mx >= x && mx < x + SPACE_WIDTH as i32 && my >= y && my < y + SPACE_HEIGHT as i32
    }
}

// ============================================================================
// MISSION CONTROL
// ============================================================================

/// Mission Control katmanı
pub struct MissionControl {
    /// Görünür mü
    pub visible: bool,
    /// Pencere küçük resimleri
    pub windows: Vec<WindowThumbnail>,
    /// Masaüstü alanları
    pub spaces: Vec<Space>,
    /// Geçerli alan indeksi
    pub current_space: usize,
    /// Animasyon ilerlemesi
    pub animation_progress: f32,
    /// Ekran genişliği
    pub screen_width: usize,
    /// Ekran yüksekliği
    pub screen_height: usize,
    /// Seçili pencere indeksi
    pub selected_window: Option<usize>,
    /// Seçili alan indeksi
    pub selected_space: Option<usize>,
    /// Üzerine gelinen pencere indeksi
    pub hover_window: Option<usize>,
    /// Alanlar çubuğunun Y konumu
    pub spaces_bar_y: usize,
}

impl MissionControl {
    pub fn new(screen_width: usize, screen_height: usize) -> Self {
        let mut mc = MissionControl {
            visible: false,
            windows: Vec::new(),
            spaces: Vec::new(),
            current_space: 0,
            animation_progress: 0.0,
            screen_width,
            screen_height,
            selected_window: None,
            selected_space: None,
            hover_window: None,
            spaces_bar_y: screen_height - 180,
        };

        mc.add_default_spaces();
        mc
    }

    fn add_default_spaces(&mut self) {
        self.spaces.push(Space::new(0, "Desktop 1"));
        self.spaces.push(Space::new(1, "Desktop 2"));
        self.spaces.push(Space::new(2, "Desktop 3"));

        self.spaces[0].selected = true;
    }

    /// Mission Control'ü göster
    pub fn show(&mut self) {
        self.visible = true;
        self.animation_progress = 0.0;
        self.selected_window = None;

        // Animasyonları sıfırla
        for window in &mut self.windows {
            window.anim_progress = 0.0;
        }
        for space in &mut self.spaces {
            space.anim_progress = 0.0;
        }

        // Küçük resim konumlarını hesapla
        self.layout_windows();
        self.layout_spaces();
    }

    /// Mission Control'ü gizle
    pub fn hide(&mut self) {
        self.visible = false;
        self.animation_progress = 0.0;
    }

    /// Görünürlüğü değiştir
    pub fn toggle(&mut self) {
        if self.visible {
            self.hide();
        } else {
            self.show();
        }
    }

    /// Pencere ekle
    pub fn add_window(&mut self, window: WindowThumbnail) {
        self.windows.push(window);
    }

    /// Pencere kaldır
    pub fn remove_window(&mut self, window_id: u32) {
        self.windows.retain(|w| w.window_id != window_id);
    }

    /// Pencereleri ızgaraya yerleştir
    fn layout_windows(&mut self) {
        if self.windows.is_empty() {
            return;
        }

        // Izgara düzenini hesapla
        let padding = 60;
        let available_width = self.screen_width - padding * 2;
        let available_height = self.spaces_bar_y - padding * 2 - 40;

        // Kaç sütun sığacağını hesapla
        let cols = ((available_width + WINDOW_SPACING) / (300 + WINDOW_SPACING)).max(1);
        let rows = ((available_height + WINDOW_SPACING) / (200 + WINDOW_SPACING)).max(1);

        // Başlangıç konumunu hesapla (ortalanmış)
        let total_width = cols * (300 + WINDOW_SPACING) - WINDOW_SPACING;
        let total_height = rows * (200 + WINDOW_SPACING) - WINDOW_SPACING;
        let start_x = (self.screen_width - total_width) / 2;
        let start_y = (self.spaces_bar_y - total_height) / 2 + 20;

        for (i, window) in self.windows.iter_mut().enumerate() {
            let col = i % cols;
            let row = i / cols;

            let target_x = (start_x + col * (300 + WINDOW_SPACING) + (300 - window.thumbnail_size.0) / 2) as f32;
            let target_y = (start_y + row * (200 + WINDOW_SPACING)) as f32;

            window.thumbnail_pos = (window.original_rect.0 as f32, window.original_rect.1 as f32);
            // Hedef konum animasyonla yaklaşılacak
        }
    }

    /// Alan çubuğunu yerleştir
    fn layout_spaces(&mut self) {
        let total_width = self.spaces.len() * (SPACE_WIDTH + SPACE_SPACING) - SPACE_SPACING;
        let start_x = (self.screen_width - total_width) / 2;

        for (i, space) in self.spaces.iter_mut().enumerate() {
            space.pos = (start_x + i * (SPACE_WIDTH + SPACE_SPACING), self.spaces_bar_y);
        }
    }

    /// Animasyonu güncelle
    pub fn update(&mut self, dt: f32) {
        if self.visible {
            if self.animation_progress < 1.0 {
                self.animation_progress = (self.animation_progress + dt * 4.0).min(1.0);
            }

            // Pencereleri güncelle
            for window in &mut self.windows {
                window.update(dt, window.thumbnail_pos);
            }

            // Alanları güncelle
            for space in &mut self.spaces {
                space.update(dt);
            }
        } else if self.animation_progress > 0.0 {
            self.animation_progress = (self.animation_progress - dt * 4.0).max(0.0);
        }
    }

    /// Mission Control'ü çiz
    pub fn draw(&self, fb: &mut Framebuffer) {
        if self.animation_progress <= 0.0 {
            return;
        }

        // Arka planı karart
        let bg_alpha = 0.6 * self.animation_progress;
        for y in 0..self.screen_height {
            for x in 0..self.screen_width {
                let ptr = unsafe { (fb.base_addr as *mut u32).add(y * fb.pixels_per_scan_line + x) };
                let bg = unsafe { *ptr };
                let dimmed = Self::blend_color(bg, 0x000000, bg_alpha as f32);
                unsafe { *ptr = dimmed; }
            }
        }

        // Pencereleri çiz (z-sırasına göre sıralanmış)
        let mut sorted_windows: Vec<_> = self.windows.iter().collect();
        sorted_windows.sort_by_key(|w| w.z_order);

        for window in sorted_windows {
            window.draw(fb);
        }

        // Alan çubuğu arka planını çiz
        let bar_y = self.spaces_bar_y - 20;
        let bar_h = 160;
        let bar_color = Self::blend_color(0x20202020, 0x20202020, self.animation_progress);
        fb.draw_rect(0, bar_y, self.screen_width, bar_h, bar_color);

        // Alanları çiz
        for space in &self.spaces {
            space.draw(fb);
        }

        // "Masaüstü Ekle" düğmesini çiz
        let add_x = self.spaces.last().map(|s| s.pos.0 + SPACE_WIDTH + SPACE_SPACING).unwrap_or(100);
        let add_y = self.spaces_bar_y;

        fb.draw_rect(add_x, add_y, SPACE_WIDTH, SPACE_HEIGHT, Self::blend_color(Theme::SIDEBAR_BG.to_u32(), Theme::SIDEBAR_BG.to_u32(), self.animation_progress));
        fb.draw_rect_outline(add_x, add_y, SPACE_WIDTH, SPACE_HEIGHT, Self::blend_color(Theme::BORDER.to_u32(), Theme::BORDER.to_u32(), self.animation_progress));
        fb.draw_string(add_x + SPACE_WIDTH / 2 - 20, add_y + SPACE_HEIGHT / 2 - 6, "+ New", Self::blend_color(Theme::TEXT_SECONDARY.to_u32(), Theme::TEXT_SECONDARY.to_u32(), self.animation_progress));
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

    /// Tıklama olayını işle
    pub fn on_click(&mut self, mx: i32, my: i32) -> MissionControlEvent {
        // Alanları kontrol et
        for (i, space) in self.spaces.iter().enumerate() {
            if space.hit_test(mx, my) {
                self.selected_space = Some(i);

                // Alana geç
                for s in &mut self.spaces {
                    s.selected = false;
                }
                self.spaces[i].selected = true;
                self.current_space = i;

                self.hide();
                return MissionControlEvent::SpaceSelected(i);
            }
        }

        // "Masaüstü Ekle" düğmesini kontrol et
        let add_x = self.spaces.last().map(|s| s.pos.0 + SPACE_WIDTH + SPACE_SPACING).unwrap_or(100);
        if mx >= add_x as i32 && mx < (add_x + SPACE_WIDTH) as i32
            && my >= self.spaces_bar_y as i32 && my < (self.spaces_bar_y + SPACE_HEIGHT) as i32 {

            let new_id = self.spaces.len() as u32;
            let name = format!("Desktop {}", new_id + 1);
            self.spaces.push(Space::new(new_id, &name));
            self.layout_spaces();

            return MissionControlEvent::SpaceCreated(new_id);
        }

        // Pencereleri kontrol et
        for (i, window) in self.windows.iter().enumerate() {
            if window.hit_test(mx, my) {
                let window_id = window.window_id;
                self.selected_window = Some(i);
                self.hide();
                return MissionControlEvent::WindowSelected(window_id);
            }
        }

        // Dışarı tıklandı — kapat
        self.hide();
        MissionControlEvent::Cancelled
    }

    /// Fare hareketi olayını işle
    pub fn on_mouse_move(&mut self, mx: i32, my: i32) {
        self.hover_window = None;

        for (i, window) in self.windows.iter().enumerate() {
            if window.hit_test(mx, my) {
                self.hover_window = Some(i);
                break;
            }
        }
    }

    /// Sonraki alana geç
    pub fn next_space(&mut self) {
        if self.current_space < self.spaces.len() - 1 {
            self.current_space += 1;
            self.update_space_selection();
        }
    }

    /// Önceki alana geç
    pub fn prev_space(&mut self) {
        if self.current_space > 0 {
            self.current_space -= 1;
            self.update_space_selection();
        }
    }

    fn update_space_selection(&mut self) {
        for space in &mut self.spaces {
            space.selected = false;
        }
        self.spaces[self.current_space].selected = true;
    }

    /// Yeniden boyutlandır
    pub fn resize(&mut self, width: usize, height: usize) {
        self.screen_width = width;
        self.screen_height = height;
        self.spaces_bar_y = height - 180;
        self.layout_windows();
        self.layout_spaces();
    }
}

/// Mission Control olayları
#[derive(Clone, Debug)]
pub enum MissionControlEvent {
    None,
    WindowSelected(u32),
    SpaceSelected(usize),
    SpaceCreated(u32),
    SpaceDeleted(u32),
    Cancelled,
}

// ============================================================================
// GLOBAL MISSION CONTROL
// ============================================================================

lazy_static::lazy_static! {
    static ref MISSION_CONTROL: Mutex<MissionControl> = Mutex::new(MissionControl::new(1920, 1080));
}

/// Mission Control'ü başlat
pub fn init(width: usize, height: usize) {
    let mut mc = MISSION_CONTROL.lock();
    mc.resize(width, height);
    crate::serial_println!("[GUI] Mission Control initialized");
}

/// Mission Control'e erişim sağla
pub fn get_mission_control() -> &'static Mutex<MissionControl> {
    &MISSION_CONTROL
}
