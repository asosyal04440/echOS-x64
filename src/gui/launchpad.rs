//! # Launchpad
//!
//! macOS tarzı tam ekran uygulama kılavuzu; arama ve klasör desteğiyle.
//! Animasyonlu uygulama simgeleri içeren tam ekran katmanı olarak gösterilir.
//!
//! ## Mimari
//! - `LaunchpadApp`: Uygulama verisi; eğrilme (scale), opaklık ve sayfa konumu
//! - `LaunchpadFolder`: Birden fazla uygulama içeren klasör; animasyonlu açılma
//! - `Launchpad`: Sayfalama, arama filtreleme ve ızgara düzeni yönetimi
//!
//! ## Animasyon Algoritması
//! `ease_out_back` fonksiyonu geri sekme (overshoot) efekti uygular:
//! `f(t) = 1 + C3*(t-1)^3 + C1*(t-1)^2`
//! C1 = 1.70158 (sıçrama katsayısı), C3 = C1 + 1 ≈ 2.70158.
//! Bu formül t∈[0,1]'de önce hedefin ötesine geçer, sonra geri döner.
//!
//! ## Izgara Düzeni
//! Satır başına 7 simge, sayfa başına 5 satır (35 uygulama/sayfa).
//! Izgara merkezi ekranda ortalanır; simge boyutu 80px, aralık 20px.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;
use libm::{sinf, cosf, powf};

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};

// ============================================================================
// LAUNCHPAD SABİTLERİ
// ============================================================================

/// Simge boyutu (piksel)
pub const ICON_SIZE: usize = 80;

/// Simgeler arası boşluk (piksel)
pub const ICON_SPACING: usize = 20;

/// Satır başına simge sayısı
pub const ICONS_PER_ROW: usize = 7;

/// Sayfa başına satır sayısı
pub const ROWS_PER_PAGE: usize = 5;

/// Klasör simge boyutu (piksel)
pub const FOLDER_SIZE: usize = 100;

// ============================================================================
// LAUNCHPAD UYGULAMASI
// ============================================================================

/// Launchpad'deki bir uygulama
#[derive(Clone, Debug)]
pub struct LaunchpadApp {
    /// Uygulama kimliği
    pub id: String,
    /// Görüntü adı
    pub name: String,
    /// Simge türü
    pub icon: LaunchpadIcon,
    /// Klasörde mi (klasör adı)
    pub folder: Option<String>,
    /// Sayfa numarası
    pub page: usize,
    /// Izgara konumu (satır, sütun)
    pub position: (usize, usize),
    /// Animasyon ofseti (x, y piksel)
    pub anim_offset: (f32, f32),
    /// Animasyon için ölçek çarpanı
    pub scale: f32,
    /// Animasyon için opaklık (0.0 - 1.0)
    pub opacity: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchpadIcon {
    Finder,
    Safari,
    Mail,
    Messages,
    Maps,
    Photos,
    Music,
    Videos,
    Notes,
    Calendar,
    Reminders,
    News,
    Stocks,
    Weather,
    Clock,
    Calculator,
    Settings,
    Terminal,
    Files,
    TextEdit,
    AppStore,
    Custom(u16),
}

impl LaunchpadApp {
    pub fn new(id: &str, name: &str, icon: LaunchpadIcon) -> Self {
        LaunchpadApp {
            id: String::from(id),
            name: String::from(name),
            icon,
            folder: None,
            page: 0,
            position: (0, 0),
            anim_offset: (0.0, 0.0),
            scale: 1.0,
            opacity: 1.0,
        }
    }

    /// Uygulama simgesini çiz
    pub fn draw(&self, fb: &mut Framebuffer, x: usize, y: usize, size: usize) {
        let scale = self.scale;
        let opacity = self.opacity;

        if opacity < 0.01 || scale < 0.01 {
            return;
        }

        let scaled_size = (size as f32 * scale) as usize;
        let offset = (size - scaled_size) / 2;
        let draw_x = x + offset + self.anim_offset.0 as usize;
        let draw_y = y + offset + self.anim_offset.1 as usize;

        // Simge arka planını çiz
        let bg_color = self.get_icon_color();
        self.draw_rounded_rect(fb, draw_x, draw_y, scaled_size, scaled_size, scaled_size / 5, bg_color);

        // Simge sembolünü çiz
        self.draw_icon_symbol(fb, draw_x, draw_y, scaled_size);

        // Adı simgenin altına çiz
        let name_y = draw_y + scaled_size + 4;
        let text_color = Self::blend_color(Theme::TEXT_PRIMARY.to_u32(), 0x000000, 1.0 - opacity);
        fb.draw_string(draw_x + (scaled_size - self.name.len() * 8) / 2, name_y, &self.name, text_color);
    }

    fn get_icon_color(&self) -> u32 {
        match self.icon {
            LaunchpadIcon::Finder => 0xFF3D67FF,
            LaunchpadIcon::Safari => 0xFF007AFF,
            LaunchpadIcon::Mail => 0xFF007AFF,
            LaunchpadIcon::Messages => 0xFF34C759,
            LaunchpadIcon::Maps => 0xFFFF3B30,
            LaunchpadIcon::Photos => 0xFFFF2D55,
            LaunchpadIcon::Music => 0xFFFC3C44,
            LaunchpadIcon::Videos => 0xFF5856D6,
            LaunchpadIcon::Notes => 0xFFFFCC00,
            LaunchpadIcon::Calendar => 0xFFFF3B30,
            LaunchpadIcon::Reminders => 0xFFFFFF00,
            LaunchpadIcon::News => 0xFFFF2D55,
            LaunchpadIcon::Stocks => 0xFF000000,
            LaunchpadIcon::Weather => 0xFF5AC8FA,
            LaunchpadIcon::Clock => 0xFF000000,
            LaunchpadIcon::Calculator => 0xFF1C1C1E,
            LaunchpadIcon::Settings => 0xFF8E8E93,
            LaunchpadIcon::Terminal => 0xFF1E1E1E,
            LaunchpadIcon::Files => 0xFF007AFF,
            LaunchpadIcon::TextEdit => 0xFFFFCC00,
            LaunchpadIcon::AppStore => 0xFF007AFF,
            LaunchpadIcon::Custom(code) => {
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
        }
    }

    fn draw_icon_symbol(&self, fb: &mut Framebuffer, x: usize, y: usize, size: usize) {
        let center_x = x + size / 2;
        let center_y = y + size / 2;
        let icon_scale = size as f32 / ICON_SIZE as f32;

        match self.icon {
            LaunchpadIcon::Finder => {
                // Gülen yüz
                let r = (size as f32 * 0.35) as usize;
                self.draw_circle(fb, center_x, center_y, r, 0xFFFFFFFF);

                // Gözler
                let eye_y = center_y - size / 10;
                fb.draw_rect(center_x - size / 6, eye_y, 4, 4, 0xFF3D67FF);
                fb.draw_rect(center_x + size / 6 - 4, eye_y, 4, 4, 0xFF3D67FF);
            }

            LaunchpadIcon::Safari => {
                // Pusula
                let r = (size as f32 * 0.35) as usize;
                self.draw_circle_outline(fb, center_x, center_y, r, 0xFFFFFFFF);

                // Pusula iğnesi
                fb.draw_rect(center_x - 2, center_y - r + 4, 4, r - 4, 0xFFFF3B30);
                fb.draw_rect(center_x - 2, center_y + 4, 4, r - 4, 0xFFFFFFFF);
            }

            LaunchpadIcon::Mail => {
                // Zarf
                let w = (size as f32 * 0.6) as usize;
                let h = (size as f32 * 0.4) as usize;
                let mail_x = center_x - w / 2;
                let mail_y = center_y - h / 2;

                fb.draw_rect(mail_x, mail_y, w, h, 0xFFFFFFFF);
            }

            LaunchpadIcon::Messages => {
                // Konuşma balonu
                let r = (size as f32 * 0.3) as usize;
                self.draw_circle(fb, center_x, center_y, r, 0xFFFFFFFF);
            }

            LaunchpadIcon::Settings => {
                // Dişli çark
                let outer_r = (size as f32 * 0.35) as usize;
                let inner_r = (size as f32 * 0.15) as usize;

                for angle in 0..8 {
                    let a = angle as f32 * core::f32::consts::PI / 4.0;
                    let tooth_x = center_x as i32 + (cosf(a) * outer_r as f32) as i32;
                    let tooth_y = center_y as i32 + (sinf(a) * outer_r as f32) as i32;
                    fb.draw_rect((tooth_x - 4).max(0) as usize, (tooth_y - 4).max(0) as usize, 8, 8, 0xFFFFFFFF);
                }

                self.draw_circle(fb, center_x, center_y, inner_r, 0xFF8E8E93);
            }

            LaunchpadIcon::Terminal => {
                // Terminal istemi
                let w = (size as f32 * 0.6) as usize;
                let h = (size as f32 * 0.5) as usize;
                let term_x = center_x - w / 2;
                let term_y = center_y - h / 2;

                fb.draw_rect(term_x, term_y, w, h, 0xFF2D2D2D);
                fb.draw_string(term_x + 4, term_y + 4, ">_", 0xFF00FF00);
            }

            LaunchpadIcon::Files => {
                // Klasör
                let folder_color = 0xFFFFFFFF;
                let w = (size as f32 * 0.55) as usize;
                let h = (size as f32 * 0.45) as usize;
                let folder_x = center_x - w / 2;
                let folder_y = center_y - h / 2;

                fb.draw_rect(folder_x, folder_y, w / 2, h / 4, folder_color);
                fb.draw_rect(folder_x, folder_y + h / 4, w, h * 3 / 4, folder_color);
            }

            LaunchpadIcon::Music => {
                // Müzik notası
                let note_size = (size as f32 * 0.25) as usize;
                self.draw_ellipse(fb, center_x - note_size / 3, center_y + note_size / 2,
                                  note_size, note_size / 2, 0xFFFFFFFF);
                fb.draw_rect(center_x + note_size / 3, center_y - note_size, 3, note_size * 2, 0xFFFFFFFF);
            }

            LaunchpadIcon::Calculator => {
                // Hesap makinesi tuş ızgarası
                let btn_size = (size as f32 * 0.15) as usize;
                let start_x = center_x - btn_size * 2 - 4;
                let start_y = center_y - btn_size * 2 - 4;

                for row in 0..4 {
                    for col in 0..4 {
                        let btn_x = start_x + col * (btn_size + 2);
                        let btn_y = start_y + row * (btn_size + 2);
                        let color = if row == 3 { 0xFFFF9500 } else { 0xFF505050 };
                        fb.draw_rect(btn_x, btn_y, btn_size, btn_size, color);
                    }
                }
            }

            LaunchpadIcon::Calendar => {
                // Tarihli takvim
                let w = (size as f32 * 0.6) as usize;
                let h = (size as f32 * 0.6) as usize;
                let cal_x = center_x - w / 2;
                let cal_y = center_y - h / 2;

                fb.draw_rect(cal_x, cal_y, w, h, 0xFFFFFFFF);
                fb.draw_rect(cal_x, cal_y, w, h / 4, 0xFFFF3B30);
                fb.draw_string(center_x - 8, center_y, "25", 0xFF333333);
            }

            LaunchpadIcon::Clock => {
                // Saat kadranı
                let r = (size as f32 * 0.35) as usize;
                self.draw_circle(fb, center_x, center_y, r, 0xFFFFFFFF);
                self.draw_circle(fb, center_x, center_y, r - 2, 0xFF000000);

                // Akrep ve yelkovan
                fb.draw_rect(center_x - 1, center_y - r / 2, 2, r / 2, 0xFFFFFFFF);
                fb.draw_rect(center_x, center_y - 1, r / 2, 2, 0xFFFFFFFF);
            }

            LaunchpadIcon::Weather => {
                // Güneşli/Bulutlu hava
                let sun_r = (size as f32 * 0.2) as usize;
                self.draw_circle(fb, center_x - size / 8, center_y - size / 8, sun_r, 0xFFFFFF00);
            }

            LaunchpadIcon::Photos => {
                // Çiçek yaprakları
                let petal_r = (size as f32 * 0.15) as usize;
                let colors = [0xFFFF2D55, 0xFFFF9500, 0xFFFFCC00, 0xFF34C759, 0xFF007AFF];

                for (i, &color) in colors.iter().enumerate() {
                    let angle = i as f32 * 2.0 * core::f32::consts::PI / 5.0;
                    let px = center_x as i32 + (cosf(angle) * petal_r as f32 * 1.5) as i32;
                    let py = center_y as i32 + (sinf(angle) * petal_r as f32 * 1.5) as i32;
                    self.draw_circle(fb, px.max(0) as usize, py.max(0) as usize, petal_r, color);
                }

                self.draw_circle(fb, center_x, center_y, petal_r / 2, 0xFFFFFFFF);
            }

            LaunchpadIcon::Notes => {
                // Sarı yapışkan not
                let w = (size as f32 * 0.5) as usize;
                let h = (size as f32 * 0.6) as usize;
                let note_x = center_x - w / 2;
                let note_y = center_y - h / 2;

                fb.draw_rect(note_x, note_y, w, h, 0xFFFFCC00);

                for i in 1..4 {
                    let line_y = note_y + i as usize * h / 4;
                    fb.draw_rect(note_x + 4, line_y, w - 8, 1, 0xFFB38F00);
                }
            }

            LaunchpadIcon::Maps => {
                // Harita pini
                let pin_r = (size as f32 * 0.2) as usize;
                self.draw_circle(fb, center_x, center_y - pin_r, pin_r, 0xFFFF3B30);
            }

            LaunchpadIcon::TextEdit => {
                // Belge simgesi
                let w = (size as f32 * 0.4) as usize;
                let h = (size as f32 * 0.55) as usize;
                let doc_x = center_x - w / 2;
                let doc_y = center_y - h / 2;

                fb.draw_rect(doc_x, doc_y, w, h, 0xFFFFFFFF);
                fb.draw_rect(doc_x + w - 8, doc_y, 8, 8, 0xFFE6B800);
            }

            _ => {
                // Varsayılan: uygulamanın ilk harfini göster
                let letter = self.name.chars().next().unwrap_or('?');
                fb.draw_char(center_x - 6, center_y - 10, letter, 0xFFFFFFFF);
            }
        }
    }

    fn draw_rounded_rect(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize, radius: usize, color: u32) {
        for py in 0..h {
            for px in 0..w {
                let in_corner =
                    (px < radius && py < radius &&
                     (radius - px) as i32 * (radius - px) as i32 + (radius - py) as i32 * (radius - py) as i32 > radius as i32 * radius as i32) ||
                    (px >= w - radius && py < radius &&
                     (px - (w - radius)) as i32 * (px - (w - radius)) as i32 + (radius - py) as i32 * (radius - py) as i32 > radius as i32 * radius as i32) ||
                    (px < radius && py >= h - radius &&
                     (radius - px) as i32 * (radius - px) as i32 + (py - (h - radius)) as i32 * (py - (h - radius)) as i32 > radius as i32 * radius as i32) ||
                    (px >= w - radius && py >= h - radius &&
                     (px - (w - radius)) as i32 * (px - (w - radius)) as i32 + (py - (h - radius)) as i32 * (py - (h - radius)) as i32 > radius as i32 * radius as i32);

                if !in_corner {
                    fb.plot_pixel(x + px, y + py, color);
                }
            }
        }
    }

    fn draw_circle(&self, fb: &mut Framebuffer, x: usize, y: usize, radius: usize, color: u32) {
        for py in 0..radius * 2 {
            for px in 0..radius * 2 {
                let dx = px as i32 - radius as i32;
                let dy = py as i32 - radius as i32;
                if dx * dx + dy * dy <= (radius * radius) as i32 {
                    fb.plot_pixel(x + px - radius, y + py - radius, color);
                }
            }
        }
    }

    fn draw_circle_outline(&self, fb: &mut Framebuffer, x: usize, y: usize, radius: usize, color: u32) {
        for py in 0..radius * 2 {
            for px in 0..radius * 2 {
                let dx = px as i32 - radius as i32;
                let dy = py as i32 - radius as i32;
                let dist = dx * dx + dy * dy;
                if dist <= (radius * radius) as i32 && dist > ((radius - 3) * (radius - 3)) as i32 {
                    fb.plot_pixel(x + px - radius, y + py - radius, color);
                }
            }
        }
    }

    fn draw_ellipse(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize, color: u32) {
        for py in 0..h {
            for px in 0..w {
                let dx = (px as f32 / w as f32 - 0.5) * 2.0;
                let dy = (py as f32 / h as f32 - 0.5) * 2.0;
                if dx * dx + dy * dy <= 1.0 {
                    fb.plot_pixel(x + px, y + py, color);
                }
            }
        }
    }

    fn blend_color(color: u32, _: u32, alpha: f32) -> u32 {
        let r = (((color >> 16) & 0xFF) as f32 * alpha) as u32;
        let g = (((color >> 8) & 0xFF) as f32 * alpha) as u32;
        let b = ((color & 0xFF) as f32 * alpha) as u32;
        (r << 16) | (g << 8) | b
    }
}

// ============================================================================
// LAUNCHPAD KLASÖRÜ
// ============================================================================

/// Birden fazla uygulama içeren klasör
#[derive(Clone, Debug)]
pub struct LaunchpadFolder {
    /// Klasör adı
    pub name: String,
    /// Klasördeki uygulamalar
    pub apps: Vec<LaunchpadApp>,
    /// Açık mı
    pub open: bool,
    /// Izgara konumu
    pub position: (usize, usize),
    /// Animasyon ilerlemesi (0.0 - 1.0)
    pub anim_progress: f32,
}

impl LaunchpadFolder {
    pub fn new(name: &str) -> Self {
        LaunchpadFolder {
            name: String::from(name),
            apps: Vec::new(),
            open: false,
            position: (0, 0),
            anim_progress: 0.0,
        }
    }

    pub fn add_app(&mut self, app: LaunchpadApp) {
        self.apps.push(app);
    }
}

// ============================================================================
// LAUNCHPAD
// ============================================================================

/// Launchpad katmanı (tam ekran uygulama kılavuzu)
pub struct Launchpad {
    /// Görünür mü
    pub visible: bool,
    /// Klasörde olmayan uygulamalar
    pub apps: Vec<LaunchpadApp>,
    /// Klasörler
    pub folders: Vec<LaunchpadFolder>,
    /// Geçerli sayfa numarası
    pub current_page: usize,
    /// Toplam sayfa sayısı
    pub total_pages: usize,
    /// Arama sorgusu
    pub search_query: String,
    /// Arama sonuçları
    pub search_results: Vec<LaunchpadApp>,
    /// Arama modunda mı
    pub searching: bool,
    /// Görünme/kapanma animasyon ilerlemesi
    pub animation_progress: f32,
    /// Sayfa geçiş ofseti
    pub page_offset: f32,
    /// Ekran genişliği
    pub screen_width: usize,
    /// Ekran yüksekliği
    pub screen_height: usize,
    /// Seçili uygulama indeksi
    pub selected_app: Option<usize>,
    /// Açık klasör indeksi
    pub open_folder: Option<usize>,
    /// Üzerine gelinen uygulama indeksi
    pub hover_app: Option<usize>,
    /// Izgara başlangıç noktası (ekranda ortalanmış)
    pub grid_origin: (usize, usize),
}

impl Launchpad {
    pub fn new(screen_width: usize, screen_height: usize) -> Self {
        let mut launchpad = Launchpad {
            visible: false,
            apps: Vec::new(),
            folders: Vec::new(),
            current_page: 0,
            total_pages: 1,
            search_query: String::new(),
            search_results: Vec::new(),
            searching: false,
            animation_progress: 0.0,
            page_offset: 0.0,
            screen_width,
            screen_height,
            selected_app: None,
            open_folder: None,
            hover_app: None,
            grid_origin: (0, 0),
        };

        launchpad.add_default_apps();
        launchpad.calculate_grid();
        launchpad
    }

    fn add_default_apps(&mut self) {
        let default_apps = vec![
            LaunchpadApp::new("finder", "Finder", LaunchpadIcon::Finder),
            LaunchpadApp::new("safari", "Safari", LaunchpadIcon::Safari),
            LaunchpadApp::new("mail", "Mail", LaunchpadIcon::Mail),
            LaunchpadApp::new("messages", "Messages", LaunchpadIcon::Messages),
            LaunchpadApp::new("maps", "Maps", LaunchpadIcon::Maps),
            LaunchpadApp::new("photos", "Photos", LaunchpadIcon::Photos),
            LaunchpadApp::new("music", "Music", LaunchpadIcon::Music),
            LaunchpadApp::new("videos", "Videos", LaunchpadIcon::Videos),
            LaunchpadApp::new("notes", "Notes", LaunchpadIcon::Notes),
            LaunchpadApp::new("calendar", "Calendar", LaunchpadIcon::Calendar),
            LaunchpadApp::new("reminders", "Reminders", LaunchpadIcon::Reminders),
            LaunchpadApp::new("news", "News", LaunchpadIcon::News),
            LaunchpadApp::new("stocks", "Stocks", LaunchpadIcon::Stocks),
            LaunchpadApp::new("weather", "Weather", LaunchpadIcon::Weather),
            LaunchpadApp::new("clock", "Clock", LaunchpadIcon::Clock),
            LaunchpadApp::new("calculator", "Calculator", LaunchpadIcon::Calculator),
            LaunchpadApp::new("settings", "Settings", LaunchpadIcon::Settings),
            LaunchpadApp::new("terminal", "Terminal", LaunchpadIcon::Terminal),
            LaunchpadApp::new("files", "Files", LaunchpadIcon::Files),
            LaunchpadApp::new("textedit", "TextEdit", LaunchpadIcon::TextEdit),
            LaunchpadApp::new("appstore", "App Store", LaunchpadIcon::AppStore),
        ];

        self.apps = default_apps;
    }

    fn calculate_grid(&mut self) {
        // Izgara başlangıç noktasını hesapla (ekranda ortalanmış)
        let grid_width = ICONS_PER_ROW * (ICON_SIZE + ICON_SPACING) - ICON_SPACING;
        let grid_height = ROWS_PER_PAGE * (ICON_SIZE + ICON_SPACING + 20) - ICON_SPACING;

        self.grid_origin = (
            (self.screen_width - grid_width) / 2,
            (self.screen_height - grid_height) / 2 + 40,
        );

        // Sayfa sayısını hesapla
        let apps_per_page = ICONS_PER_ROW * ROWS_PER_PAGE;
        self.total_pages = (self.apps.len() + apps_per_page - 1) / apps_per_page;
        self.total_pages = self.total_pages.max(1);

        // Konumları ata
        for (i, app) in self.apps.iter_mut().enumerate() {
            app.page = i / apps_per_page;
            let pos_in_page = i % apps_per_page;
            app.position = (
                pos_in_page / ICONS_PER_ROW,
                pos_in_page % ICONS_PER_ROW,
            );
        }
    }

    /// Launchpad'i göster
    pub fn show(&mut self) {
        self.visible = true;
        self.animation_progress = 0.0;
        self.search_query.clear();
        self.searching = false;
        self.selected_app = None;
        self.open_folder = None;

        // Animasyonları sıfırla
        for app in &mut self.apps {
            app.scale = 0.0;
            app.opacity = 0.0;
        }
    }

    /// Launchpad'i gizle
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

    /// Animasyonu güncelle
    pub fn update(&mut self, dt: f32) {
        if self.visible {
            if self.animation_progress < 1.0 {
                self.animation_progress = (self.animation_progress + dt * 4.0).min(1.0);
            }

            // Uygulamaların belirme animasyonu
            let apps_per_page = ICONS_PER_ROW * ROWS_PER_PAGE;
            for (i, app) in self.apps.iter_mut().enumerate() {
                if app.page == self.current_page || self.searching {
                    // Her uygulama sıralı olarak küçük bir gecikmeyle başlar
                    let delay = if self.searching { 0.0 } else { (i % apps_per_page) as f32 * 0.02 };
                    let progress = (self.animation_progress - delay).max(0.0).min(1.0);

                    app.scale = Self::ease_out_back(progress);
                    app.opacity = progress;
                }
            }
        } else if self.animation_progress > 0.0 {
            self.animation_progress = (self.animation_progress - dt * 4.0).max(0.0);
        }
    }

    /// Geri sekerek hızlanan animasyon eğrisi
    /// C1 = 1.70158 (sıçrama katsayısı), formül: 1 + C3*(t-1)^3 + C1*(t-1)^2
    fn ease_out_back(t: f32) -> f32 {
        const C1: f32 = 1.70158;
        const C3: f32 = C1 + 1.0;
        1.0 + C3 * powf(t - 1.0, 3.0) + C1 * powf(t - 1.0, 2.0)
    }

    /// Launchpad'i çiz
    pub fn draw(&self, fb: &mut Framebuffer) {
        if self.animation_progress <= 0.0 {
            return;
        }

        // Arka planı karart
        let bg_alpha = 0.5 * self.animation_progress;
        for y in 0..self.screen_height {
            for x in 0..self.screen_width {
                let ptr = unsafe { (fb.base_addr as *mut u32).add(y * fb.pixels_per_scan_line + x) };
                let bg = unsafe { *ptr };
                let dimmed = Self::blend_color(bg, 0x000000, bg_alpha as f32);
                unsafe { *ptr = dimmed; }
            }
        }

        // Arama çubuğu
        if self.searching || !self.search_query.is_empty() {
            let search_width = 400;
            let search_height = 36;
            let search_x = (self.screen_width - search_width) / 2;
            let search_y = 60;

            fb.draw_rect(search_x, search_y, search_width, search_height, 0xE0FFFFFF);
            fb.draw_string(search_x + 12, search_y + 8, "🔍", 0xFF888888);

            if self.search_query.is_empty() {
                fb.draw_string(search_x + 36, search_y + 8, "Search", 0xFF888888);
            } else {
                fb.draw_string(search_x + 36, search_y + 8, &self.search_query, 0xFF333333);
            }
        }

        // Uygulamaları çiz
        let (origin_x, origin_y) = self.grid_origin;

        if self.searching && !self.search_results.is_empty() {
            // Arama sonuçlarını çiz
            for (i, app) in self.search_results.iter().enumerate() {
                let row = i / ICONS_PER_ROW;
                let col = i % ICONS_PER_ROW;

                let x = origin_x + col * (ICON_SIZE + ICON_SPACING);
                let y = origin_y + row * (ICON_SIZE + ICON_SPACING + 20);

                app.draw(fb, x, y, ICON_SIZE);
            }
        } else {
            // Geçerli sayfanın uygulamalarını çiz
            for app in &self.apps {
                if app.page == self.current_page {
                    let (row, col) = app.position;
                    let x = origin_x + col * (ICON_SIZE + ICON_SPACING);
                    let y = origin_y + row * (ICON_SIZE + ICON_SPACING + 20);

                    app.draw(fb, x, y, ICON_SIZE);
                }
            }
        }

        // Sayfa göstergelerini çiz
        if self.total_pages > 1 && !self.searching {
            let indicator_y = self.screen_height - 60;
            let total_width = self.total_pages * 12 + (self.total_pages - 1) * 8;
            let start_x = (self.screen_width - total_width) / 2;

            for i in 0..self.total_pages {
                let dot_x = start_x + i * 20;
                let dot_size = if i == self.current_page { 8 } else { 6 };
                let dot_color = if i == self.current_page { 0xFFFFFFFF } else { 0x80FFFFFF };

                for py in 0..dot_size {
                    for px in 0..dot_size {
                        let dx = px as i32 - dot_size as i32 / 2;
                        let dy = py as i32 - dot_size as i32 / 2;
                        if dx * dx + dy * dy <= (dot_size / 2) * (dot_size / 2) {
                            fb.plot_pixel((dot_x as i32 + px as i32) as usize, (indicator_y as i32 + py as i32) as usize, dot_color);
                        }
                    }
                }
            }
        }

        // Açık klasörü çiz
        if let Some(folder_idx) = self.open_folder {
            if folder_idx < self.folders.len() {
                // Klasör katmanı burada çizilir
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

    /// Tıklama olayını işle
    pub fn on_click(&mut self, mx: i32, my: i32) -> LaunchpadEvent {
        // Arama çubuğuna tıklandı mı kontrol et
        let search_width = 400;
        let search_x = (self.screen_width - search_width) / 2;

        if mx >= search_x as i32 && mx < (search_x + search_width) as i32 && my >= 60 && my < 96 {
            self.searching = true;
            return LaunchpadEvent::SearchFocused;
        }

        // Uygulamaları kontrol et
        let (origin_x, origin_y) = self.grid_origin;
        let apps_to_check = if self.searching { &self.search_results.clone() } else { &self.apps.clone() };

        for (i, app) in apps_to_check.iter().enumerate() {
            if self.searching || app.page == self.current_page {
                let (row, col) = app.position;
                let x = origin_x + col * (ICON_SIZE + ICON_SPACING);
                let y = origin_y + row * (ICON_SIZE + ICON_SPACING + 20);

                if mx >= x as i32 && mx < (x + ICON_SIZE) as i32
                    && my >= y as i32 && my < (y + ICON_SIZE + 20) as i32 {
                    self.selected_app = Some(i);
                    return LaunchpadEvent::AppSelected(app.id.clone());
                }
            }
        }

        // Dışarı tıklandı — kapat
        self.hide();
        LaunchpadEvent::Cancelled
    }

    /// Tuş girişini işle
    pub fn on_key_press(&mut self, c: char) -> LaunchpadEvent {
        if c == '\x1b' { // Escape
            if self.searching {
                self.searching = false;
                self.search_query.clear();
                return LaunchpadEvent::None;
            }
            self.hide();
            return LaunchpadEvent::Cancelled;
        }

        if self.searching {
            if c == '\x08' { // Geri silme
                if !self.search_query.is_empty() {
                    self.search_query.pop();
                    self.update_search();
                }
            } else if !c.is_control() {
                self.search_query.push(c);
                self.update_search();
            }
        }

        LaunchpadEvent::None
    }

    fn update_search(&mut self) {
        if self.search_query.is_empty() {
            self.search_results.clear();
            return;
        }

        let query = self.search_query.to_lowercase();
        self.search_results = self.apps.iter()
            .filter(|app| app.name.to_lowercase().contains(&query))
            .cloned()
            .collect();
    }

    /// Sonraki sayfaya geç
    pub fn next_page(&mut self) {
        if self.current_page < self.total_pages - 1 {
            self.current_page += 1;
            self.page_offset = -1.0;
        }
    }

    /// Önceki sayfaya geç
    pub fn prev_page(&mut self) {
        if self.current_page > 0 {
            self.current_page -= 1;
            self.page_offset = 1.0;
        }
    }

    /// Yeniden boyutlandır
    pub fn resize(&mut self, width: usize, height: usize) {
        self.screen_width = width;
        self.screen_height = height;
        self.calculate_grid();
    }
}

/// Launchpad olayları
#[derive(Clone, Debug)]
pub enum LaunchpadEvent {
    None,
    AppSelected(String),
    FolderOpened(String),
    SearchFocused,
    Cancelled,
}

// ============================================================================
// GLOBAL LAUNCHPAD
// ============================================================================

lazy_static::lazy_static! {
    static ref LAUNCHPAD: Mutex<Launchpad> = Mutex::new(Launchpad::new(1920, 1080));
}

/// Launchpad'i başlat
pub fn init(width: usize, height: usize) {
    let mut launchpad = LAUNCHPAD.lock();
    launchpad.resize(width, height);
    crate::serial_println!("[GUI] Launchpad initialized");
}

/// Launchpad'e erişim sağla
pub fn get_launchpad() -> &'static Mutex<Launchpad> {
    &LAUNCHPAD
}
