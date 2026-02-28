//! # Gelişmiş Görev Çubuğu (Taskbar)
//!
//! Başlat menüsü, sabitlenmiş uygulamalar, sistem tepsisi ve saati içeren modern görev çubuğu.
//! Pencere önizlemeleri, hızlı liste (jump list) ve bildirim rozeti destekler.
//!
//! ## Görev Çubuğu Düzeni
//!
//! ```text
//!  ┌─────────────────────────────────────────────────────────────────┐
//!  │ [⊞] [🔍] [⧉] │ [💾] [🌐] [⌨] [⚙] │ [Uyg1] [Uyg2] │ [🔋][🔊] 00:00 │
//!  └─────────────────────────────────────────────────────────────────┘
//!    ◄── sol_buttons ──►          ◄── center_buttons ──►  ◄─ system_tray ─►
//!
//!  Sol:    Başlat / Ara / Görev Görünümü + Sabitlenmiş Uygulamalar
//!  Orta:   Çalışan uygulamalar (pencere düğmeleri)
//!  Sağ:    Sistem tepsisi + Saat + Masaüstünü Göster
//! ```
//!
//! ## Düğme Durumları
//!
//! ```text
//!  ┌──────┐  ┌──────┐  ┌──────┐  ┌──────┐
//!  │      │  │ HOVER│  │PRESS │  │ACTIVE│
//!  │  bg  │  │  bg  │  │  bg  │  │accent│
//!  └──────┘  └──────┘  └──────┘  └──┬───┘
//!  Saydam    BUTTON    BUTTON        │
//!            _HOVER    _HOVER     alt çizgi
//!                                 (aktif göstergesi)
//! ```

use alloc::boxed::Box;
use alloc::string::String;
use alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use alloc::collections::VecDeque;
use spin::Mutex;
use libm::{sinf, cosf};

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::Theme;
use crate::gui::widgets::{Widget, Rect};

// ============================================================================
// GÖREV ÇUBUĞU SABİTLERİ
// ============================================================================

/// Varsayılan görev çubuğu yüksekliği (piksel)
pub const TASKBAR_HEIGHT: usize = 48;

/// Görev çubuğu simge boyutu (piksel)
pub const ICON_SIZE: usize = 32;

/// Düğme genişliği (piksel)
pub const BUTTON_WIDTH: usize = 44;

/// Öğeler arası boşluk (piksel)
pub const SPACING: usize = 4;

// ============================================================================
// GÖREV ÇUBUĞU DÜĞMESİ
// ============================================================================

/// Görev çubuğundaki tek bir düğme.
/// Sol sabit düğmeler ve orta çalışan uygulama düğmeleri bu yapıyla temsil edilir.
#[derive(Clone)]
pub struct TaskbarButton {
    /// Düğme kimliği
    id: u32,
    /// Düğme türü
    button_type: ButtonType,
    /// X konumu (görev çubuğuna göre)
    x: usize,
    /// Y konumu (görev çubuğuna göre)
    y: usize,
    /// Genişlik
    width: usize,
    /// Yükseklik
    height: usize,
    /// Fare üzerinde mi (hover)
    hovered: bool,
    /// Basılı mı (mousedown)
    pressed: bool,
    /// Aktif mi (odaklanmış pencere)
    active: bool,
    /// Simge türü (çizim için)
    icon_type: IconType,
    /// Araç ipucu metni
    tooltip: String,
    /// Bildirim rozeti sayısı (0 = gizli)
    badge_count: u32,
    /// İlerleme çubuğu değeri (0.0 - 1.0, 0 = gizli)
    progress: f32,
}

/// Görev çubuğu düğme türleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonType {
    /// Başlat menüsü düğmesi
    Start,
    /// Spotlight/Arama düğmesi
    Search,
    /// Görev görünümü düğmesi (çalışan uygulamalar)
    TaskView,
    /// Sabitlenmiş uygulama (çalışmıyor)
    PinnedApp,
    /// Çalışan uygulama (aktif pencere var)
    RunningApp,
    /// Sistem tepsisi alanı
    SystemTray,
    /// Saat düğmesi
    Clock,
    /// Masaüstünü göster düğmesi (en sağ şerit)
    ShowDesktop,
}

/// Simge çizim türleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconType {
    Start,
    Search,
    TaskView,
    FileExplorer,
    Settings,
    Terminal,
    Browser,
    Music,
    Video,
    Photos,
    Mail,
    Calendar,
    Calculator,
    Notes,
    Custom(u16),
}

impl TaskbarButton {
    pub fn new(id: u32, button_type: ButtonType, x: usize) -> Self {
        TaskbarButton {
            id,
            button_type,
            x,
            y: SPACING,
            width: BUTTON_WIDTH,
            height: TASKBAR_HEIGHT - SPACING * 2,
            hovered: false,
            pressed: false,
            active: false,
            icon_type: IconType::Custom(0),
            tooltip: String::new(),
            badge_count: 0,
            progress: 0.0,
        }
    }

    /// Başlat düğmesi oluştur
    pub fn start(id: u32, x: usize) -> Self {
        TaskbarButton {
            id,
            button_type: ButtonType::Start,
            x,
            y: SPACING,
            width: BUTTON_WIDTH,
            height: TASKBAR_HEIGHT - SPACING * 2,
            hovered: false,
            pressed: false,
            active: false,
            icon_type: IconType::Start,
            tooltip: String::from("Start"),
            badge_count: 0,
            progress: 0.0,
        }
    }

    /// Arama düğmesi oluştur
    pub fn search(id: u32, x: usize) -> Self {
        TaskbarButton {
            id,
            button_type: ButtonType::Search,
            x,
            y: SPACING,
            width: BUTTON_WIDTH,
            height: TASKBAR_HEIGHT - SPACING * 2,
            hovered: false,
            pressed: false,
            active: false,
            icon_type: IconType::Search,
            tooltip: String::from("Search"),
            badge_count: 0,
            progress: 0.0,
        }
    }

    /// Sabitlenmiş uygulama düğmesi oluştur
    pub fn pinned_app(id: u32, x: usize, icon: IconType, tooltip: &str) -> Self {
        TaskbarButton {
            id,
            button_type: ButtonType::PinnedApp,
            x,
            y: SPACING,
            width: BUTTON_WIDTH,
            height: TASKBAR_HEIGHT - SPACING * 2,
            hovered: false,
            pressed: false,
            active: false,
            icon_type: icon,
            tooltip: String::from(tooltip),
            badge_count: 0,
            progress: 0.0,
        }
    }

    /// Çalışan uygulama düğmesi oluştur
    pub fn running_app(id: u32, x: usize, icon: IconType, tooltip: &str, active: bool) -> Self {
        TaskbarButton {
            id,
            button_type: ButtonType::RunningApp,
            x,
            y: SPACING,
            width: BUTTON_WIDTH,
            height: TASKBAR_HEIGHT - SPACING * 2,
            hovered: false,
            pressed: false,
            active,
            icon_type: icon,
            tooltip: String::from(tooltip),
            badge_count: 0,
            progress: 0.0,
        }
    }

    /// Hover durumunu ayarla
    pub fn set_hovered(&mut self, hovered: bool) {
        self.hovered = hovered;
    }

    /// Basılı durumu ayarla
    pub fn set_pressed(&mut self, pressed: bool) {
        self.pressed = pressed;
    }

    /// Aktif durumu ayarla
    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    /// Bildirim rozeti sayısını ayarla (0 = gizle)
    pub fn set_badge(&mut self, count: u32) {
        self.badge_count = count;
    }

    /// İlerleme çubuğu değerini ayarla (0.0 - 1.0)
    pub fn set_progress(&mut self, progress: f32) {
        self.progress = progress.max(0.0).min(1.0);
    }

    /// Verilen koordinatın düğme içinde olup olmadığını kontrol et
    pub fn hit_test(&self, x: i32, y: i32) -> bool {
        x >= self.x as i32 && x < (self.x + self.width) as i32
            && y >= self.y as i32 && y < (self.y + self.height) as i32
    }

    /// Düğme sınırlarını döndür
    pub fn bounds(&self) -> Rect {
        Rect::new(self.x as i32, self.y as i32, self.width as i32, self.height as i32)
    }

    /// Düğmeyi çiz.
    ///
    /// Çizim katmanları (altan üste):
    /// 1. Yuvarlak köşeli arka plan (durum rengine göre)
    /// 2. Simge (merkeze hizalanmış)
    /// 3. Aktif göstergesi (alt alt çizgi)
    /// 4. İlerleme çubuğu (alt şerit)
    /// 5. Bildirim rozeti (üst sağ)
    pub fn draw(&self, fb: &mut Framebuffer, fb_width: usize, fb_height: usize) {
        let x = self.x;
        let y = fb_height - TASKBAR_HEIGHT + self.y;

        // Arka plan rengi: basılı/hover → BUTTON_HOVER, aktif → ACCENT, normal → saydam
        let bg_color = if self.pressed {
            Theme::BUTTON_HOVER.to_u32()
        } else if self.hovered {
            Theme::BUTTON_HOVER.to_u32()
        } else if self.active {
            Theme::ACCENT_PRIMARY.to_u32()
        } else {
            Theme::TRANSPARENT.to_u32()
        };

        // Yuvarlak köşeli dikdörtgen ile arka planı çiz
        self.draw_rounded_rect(fb, x, y, self.width, self.height, 4, bg_color);

        // Simgeyi merkeze hizalayarak çiz
        let icon_x = x + (self.width - ICON_SIZE) / 2;
        let icon_y = y + (self.height - ICON_SIZE) / 2;
        self.draw_icon(fb, icon_x, icon_y, ICON_SIZE);

        // Aktif pencere göstergesi: alt kısımda accent renkli şerit
        if self.active {
            let indicator_y = y + self.height - 3;
            let indicator_width = self.width / 2;
            let indicator_x = x + (self.width - indicator_width) / 2;

            fb.draw_rect(
                indicator_x, indicator_y,
                indicator_width, 3,
                Theme::ACCENT_PRIMARY.to_u32()
            );
        }

        // İlerleme çubuğu (düğme altında ince şerit)
        if self.progress > 0.0 {
            let progress_width = (self.width as f32 * self.progress) as usize;
            let progress_y = y + self.height - 2;

            fb.draw_rect(
                x, progress_y,
                progress_width, 2,
                Theme::ACCENT_PRIMARY.to_u32()
            );
        }

        // Bildirim rozeti (kırmızı daire, üst sağ köşe)
        if self.badge_count > 0 {
            let badge_x = x + self.width - 16;
            let badge_y = y + 4;
            let badge_radius = 8;

            // Daire dolgusu (Pisagor mesafe formülüyle)
            for py in 0..badge_radius * 2 {
                for px in 0..badge_radius * 2 {
                    let dx = px as i32 - badge_radius as i32;
                    let dy = py as i32 - badge_radius as i32;
                    if dx * dx + dy * dy < (badge_radius * badge_radius) as i32 {
                        fb.plot_pixel(badge_x + px, badge_y + py, Theme::ERROR.to_u32());
                    }
                }
            }

            // Rozet sayısı metni
            if self.badge_count < 10 {
                let digit = char::from(b'0' + self.badge_count as u8);
                fb.draw_char(badge_x + 3, badge_y + 2, digit, Theme::TEXT_ON_ACCENT.to_u32());
            } else {
                fb.draw_string(badge_x, badge_y + 2, "9+", Theme::TEXT_ON_ACCENT.to_u32());
            }
        }
    }

    /// Yuvarlak köşeli dikdörtgen çiz.
    /// Merkez dikdörtgen + 4 köşe daire (radius yarıçaplı) birleştirilir.
    fn draw_rounded_rect(
        &self,
        fb: &mut Framebuffer,
        x: usize, y: usize,
        width: usize, height: usize,
        radius: usize,
        color: u32,
    ) {
        // Merkez dikdörtgenler (yatay ve dikey)
        fb.draw_rect(x + radius, y, width - radius * 2, height, color);
        fb.draw_rect(x, y + radius, width, height - radius * 2, color);

        // Dört köşe (daire çeyreği)
        for py in 0..radius {
            for px in 0..radius {
                let dx = px as i32 - radius as i32;
                let dy = py as i32 - radius as i32;

                if dx * dx + dy * dy <= (radius * radius) as i32 {
                    // Sol üst köşe
                    fb.plot_pixel(x + px, y + py, color);
                    // Sağ üst köşe
                    fb.plot_pixel(x + width - radius + px, y + py, color);
                    // Sol alt köşe
                    fb.plot_pixel(x + px, y + height - radius + py, color);
                    // Sağ alt köşe
                    fb.plot_pixel(x + width - radius + px, y + height - radius + py, color);
                }
            }
        }
    }

    /// Simgeyi çiz. Her `IconType` için piksel bazlı vektörel simge çizimi yapılır.
    fn draw_icon(&self, fb: &mut Framebuffer, x: usize, y: usize, size: usize) {
        let color = Theme::TEXT_PRIMARY.to_u32();
        let accent = Theme::ACCENT_PRIMARY.to_u32();

        match self.icon_type {
            IconType::Start => {
                // 4 kareli Windows/echOS başlat simgesi
                let square_size = size / 3;
                let gap = 2;

                // Sol üst (accent)
                fb.draw_rect(x + gap, y + gap, square_size - gap, square_size - gap, accent);
                // Sağ üst (normal)
                fb.draw_rect(x + square_size + gap, y + gap, square_size - gap, square_size - gap, color);
                // Sol alt (normal)
                fb.draw_rect(x + gap, y + square_size + gap, square_size - gap, square_size - gap, color);
                // Sağ alt (accent)
                fb.draw_rect(x + square_size + gap, y + square_size + gap, square_size - gap, square_size - gap, accent);
            }

            IconType::Search => {
                // Büyüteç simgesi: daire + sap
                let center = size / 3;
                let radius = size / 4;

                // Dairenin dış çerçevesi (2px kalınlık)
                for py in 0..size {
                    for px in 0..size {
                        let dx = px as i32 - center as i32;
                        let dy = py as i32 - center as i32;
                        if dx * dx + dy * dy <= (radius * radius) as i32
                            && dx * dx + dy * dy > ((radius - 2) * (radius - 2)) as i32 {
                            fb.plot_pixel(x + px, y + py, color);
                        }
                    }
                }

                // Sap (çapraz çizgi)
                let handle_x = x + center + radius / 2;
                let handle_y = y + center + radius / 2;
                for i in 0..(size / 3) {
                    fb.plot_pixel(handle_x + i, handle_y + i, color);
                    fb.plot_pixel(handle_x + i + 1, handle_y + i, color);
                }
            }

            IconType::TaskView => {
                // Görev görünümü: üst üste iki dikdörtgen
                let rect_w = size / 2;
                let rect_h = size / 2;

                fb.draw_rect(x + 2, y + 2, rect_w - 2, rect_h - 2, color);
                fb.draw_rect(x + size / 3, y + size / 3, rect_w - 2, rect_h - 2, accent);
            }

            IconType::FileExplorer => {
                // Klasör simgesi: sekme + ana gövde
                let tab_h = size / 4;
                fb.draw_rect(x + 2, y + 2, size / 2, tab_h, 0xFFC107);
                fb.draw_rect(x + 2, y + tab_h, size - 4, size - tab_h - 2, 0xFFC107);
            }

            IconType::Settings => {
                // Dişli simgesi: 8 diş + merkez daire
                let center = size / 2;
                let outer_r = size / 3;
                let inner_r = size / 6;

                // 8 adet eşit açıyla dağılmış dişler
                for angle in 0..8 {
                    let a = angle as f32 * core::f32::consts::PI / 4.0;
                    let tooth_x = x as i32 + center as i32 + (cosf(a) * outer_r as f32) as i32;
                    let tooth_y = y as i32 + center as i32 + (sinf(a) * outer_r as f32) as i32;

                    fb.draw_rect(
                        (tooth_x - 2).max(0) as usize,
                        (tooth_y - 2).max(0) as usize,
                        4, 4, color
                    );
                }

                // Merkez dolu daire
                for py in 0..size {
                    for px in 0..size {
                        let dx = px as i32 - center as i32;
                        let dy = py as i32 - center as i32;
                        if dx * dx + dy * dy <= (inner_r * inner_r) as i32 {
                            fb.plot_pixel(x + px, y + py, color);
                        }
                    }
                }
            }

            IconType::Terminal => {
                // Terminal simgesi: koyu arka plan + prompt
                fb.draw_rect(x + 2, y + 2, size - 4, size - 4, 0x1E1E1E);
                // ">_" terminal imleci
                fb.draw_string(x + 4, y + 6, ">_", color);
            }

            IconType::Browser => {
                // Tarayıcı simgesi: dünya küresi (daire + yatay/dikey çizgiler)
                let center = size / 2;
                let radius = size / 3;

                // Dış çerçeve
                for py in 0..size {
                    for px in 0..size {
                        let dx = px as i32 - center as i32;
                        let dy = py as i32 - center as i32;
                        if dx * dx + dy * dy <= (radius * radius) as i32
                            && dx * dx + dy * dy > ((radius - 2) * (radius - 2)) as i32 {
                            fb.plot_pixel(x + px, y + py, accent);
                        }
                    }
                }

                // Ekvatör ve meridyen çizgileri
                for i in 0..(radius * 2) {
                    fb.plot_pixel(x + center - radius + i, y + center, accent);
                    fb.plot_pixel(x + center, y + center - radius + i, accent);
                }
            }

            IconType::Music => {
                // Müzik notu simgesi: elips baş + dikey sap + bayrak
                let note_x = x + size / 3;
                let note_y = y + size / 4;
                let note_size = size / 4;

                // Not başı (dolu daire)
                for py in 0..note_size {
                    for px in 0..note_size {
                        let dx = px as i32 - note_size as i32 / 2;
                        let dy = py as i32 - note_size as i32 / 2;
                        if dx * dx + dy * dy <= (note_size * note_size / 4) as i32 {
                            fb.plot_pixel(note_x + px, note_y + py, accent);
                        }
                    }
                }

                // Dikey sap
                fb.draw_rect(note_x + note_size - 2, note_y, 2, size / 2, accent);

                // Bayrak (yatay çizgi)
                fb.draw_rect(note_x + note_size - 2, note_y, size / 4, 3, accent);
            }

            _ => {
                // Varsayılan: accent renkli dolu kare
                fb.draw_rect(x + 4, y + 4, size - 8, size - 8, accent);
            }
        }
    }

    /// Düğme türünü döndür
    pub fn button_type(&self) -> ButtonType {
        self.button_type
    }

    /// Düğme kimliğini döndür
    pub fn id(&self) -> u32 {
        self.id
    }
}

// ============================================================================
// SİSTEM TEPSİSİ ÖĞESİ (Görev çubuğu içi)
// ============================================================================

/// Görev çubuğunun sağında görünen küçük sistem tepsisi simgesi (16×16)
pub struct SystemTrayItem {
    /// Simge kimliği
    id: u32,
    /// Simge türü
    icon_type: IconType,
    /// Araç ipucu
    tooltip: String,
    /// X konumu (hesaplanan)
    x: usize,
    /// Hover durumu
    hovered: bool,
}

impl SystemTrayItem {
    pub fn new(id: u32, icon_type: IconType, tooltip: &str) -> Self {
        SystemTrayItem {
            id,
            icon_type,
            tooltip: String::from(tooltip),
            x: 0,
            hovered: false,
        }
    }

    /// Küçük tepsi simgesini çiz (16×16 piksel)
    pub fn draw(&self, fb: &mut Framebuffer, x: usize, y: usize) {
        let color = Theme::TEXT_PRIMARY.to_u32();

        match self.icon_type {
            IconType::Custom(100) => {
                // Wi-Fi sinyal çubukları (4 adet, soldan sağa yükselen)
                let bars = [4, 8, 12, 16];
                for (i, &bar_h) in bars.iter().enumerate() {
                    let bar_x = x + i * 4;
                    let bar_y = y + 16 - bar_h;
                    fb.draw_rect(bar_x, bar_y, 3, bar_h, color);
                }
            }
            IconType::Custom(101) => {
                // Ses hoparlörü simgesi (iki dikdörtgen blok)
                fb.draw_rect(x + 2, y + 6, 6, 8, color);
                fb.draw_rect(x + 8, y + 4, 6, 12, color);
            }
            IconType::Custom(102) => {
                // Pil simgesi (gövde + doluluk + başlık)
                fb.draw_rect(x + 2, y + 4, 12, 12, color);
                fb.draw_rect(x + 14, y + 6, 2, 8, color);
                // Doluluk göstergesi (yeşil)
                fb.draw_rect(x + 4, y + 6, 8, 8, Theme::ACCENT_SUCCESS.to_u32());
            }
            IconType::Custom(103) => {
                // Ağ simgesi (yatay çubuk + dikey bağlantı)
                fb.draw_rect(x + 2, y + 10, 12, 6, color);
                fb.draw_rect(x + 6, y + 4, 4, 6, color);
            }
            _ => {
                // Varsayılan: dolu kare
                fb.draw_rect(x + 2, y + 2, 12, 12, color);
            }
        }
    }

    /// Verilen koordinatın bu simge üzerinde olup olmadığını kontrol et
    pub fn hit_test(&self, mx: i32, my: i32, x: usize, y: usize) -> bool {
        mx >= x as i32 && mx < (x + 16) as i32 && my >= y as i32 && my < (y + 16) as i32
    }
}

// ============================================================================
// GELİŞMİŞ GÖREV ÇUBUĞU
// ============================================================================

/// Tüm özellikleri barındıran gelişmiş görev çubuğu.
///
/// ## Otomatik Gizleme (auto_hide)
/// Görev çubuğu, fare ekranın alt kenarına yaklaşınca ortaya çıkar.
/// Fare uzaklaştığında gizlenir. `visible` bayrağı ve `draw()`
/// birlikte kullanılarak animasyonsuz gösterme/gizleme yapılır.
pub struct EnhancedTaskbar {
    /// Görev çubuğu konumu
    position: TaskbarPosition,
    /// Yükseklik (piksel)
    height: usize,
    /// Genişlik (ekran genişliği)
    width: usize,
    /// Sol düğmeler: Başlat, Ara, Görev Görünümü, Sabitlenmiş uygulamalar
    left_buttons: Vec<TaskbarButton>,
    /// Orta düğmeler: Çalışan uygulama pencereleri
    center_buttons: Vec<TaskbarButton>,
    /// Sistem tepsisi simgeleri
    system_tray: Vec<SystemTrayItem>,
    /// Saat metni (HH:MM)
    clock_string: String,
    /// Tarih metni (Ay GG)
    date_string: String,
    /// Sonraki düğme kimliği (otomatik artırılan sayaç)
    next_id: u32,
    /// Üzerine gelinen düğme kimliği
    hovered_button: Option<u32>,
    /// Basılı tutulan düğme kimliği
    pressed_button: Option<u32>,
    /// Otomatik gizleme etkin mi
    auto_hide: bool,
    /// Görünür mü (otomatik gizleme için)
    visible: bool,
    /// Son fare konumu
    last_mouse: (i32, i32),
}

/// Görev çubuğu konumu
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskbarPosition {
    /// Ekranın alt kısmı (varsayılan)
    Bottom,
    /// Ekranın üst kısmı
    Top,
    /// Ekranın sol kısmı
    Left,
    /// Ekranın sağ kısmı
    Right,
}

impl EnhancedTaskbar {
    pub fn new(width: usize) -> Self {
        let mut taskbar = EnhancedTaskbar {
            position: TaskbarPosition::Bottom,
            height: TASKBAR_HEIGHT,
            width,
            left_buttons: Vec::new(),
            center_buttons: Vec::new(),
            system_tray: Vec::new(),
            clock_string: String::from("00:00"),
            date_string: String::from("Jan 1"),
            next_id: 1,
            hovered_button: None,
            pressed_button: None,
            auto_hide: false,
            visible: true,
            last_mouse: (0, 0),
        };

        // Varsayılan düğmeleri ekle
        taskbar.add_default_buttons();

        // Varsayılan sistem tepsisini ekle
        taskbar.add_default_tray();

        taskbar
    }

    /// Sol bölüme varsayılan düğmeleri ekle (Başlat, Ara, Görev, Sabitlenmiş uygulamalar)
    fn add_default_buttons(&mut self) {
        let mut x = SPACING;

        // Başlat düğmesi
        self.left_buttons.push(TaskbarButton::start(self.next_id, x));
        self.next_id += 1;
        x += BUTTON_WIDTH + SPACING;

        // Arama düğmesi
        self.left_buttons.push(TaskbarButton::search(self.next_id, x));
        self.next_id += 1;
        x += BUTTON_WIDTH + SPACING;

        // Görev görünümü düğmesi
        self.left_buttons.push(TaskbarButton::new(self.next_id, ButtonType::TaskView, x));
        self.next_id += 1;
        x += BUTTON_WIDTH + SPACING;

        // Sabitlenmiş uygulamalar
        let pinned_apps = [
            (IconType::FileExplorer, "File Explorer"),
            (IconType::Browser, "Browser"),
            (IconType::Terminal, "Terminal"),
            (IconType::Settings, "Settings"),
        ];

        for (icon, tooltip) in pinned_apps {
            self.left_buttons.push(TaskbarButton::pinned_app(self.next_id, x, icon, tooltip));
            self.next_id += 1;
            x += BUTTON_WIDTH + SPACING;
        }
    }

    /// Varsayılan sistem tepsisi öğelerini ekle (Ağ, Ses, Pil)
    fn add_default_tray(&mut self) {
        self.system_tray.push(SystemTrayItem::new(100, IconType::Custom(100), "Network"));
        self.system_tray.push(SystemTrayItem::new(101, IconType::Custom(101), "Volume"));
        self.system_tray.push(SystemTrayItem::new(102, IconType::Custom(102), "Battery"));
    }

    /// Çalışan uygulamayı orta bölüme ekle. Düğme kimliğini döndürür.
    pub fn add_running_app(&mut self, icon: IconType, tooltip: &str, active: bool) -> u32 {
        let x = self.center_buttons.len() * (BUTTON_WIDTH + SPACING);
        let button = TaskbarButton::running_app(self.next_id, x, icon, tooltip, active);
        self.center_buttons.push(button);
        self.next_id += 1;
        self.next_id - 1
    }

    /// Çalışan uygulamayı kimliğe göre kaldır. Sonraki düğmelerin konumlarını yeniden hesaplar.
    pub fn remove_running_app(&mut self, id: u32) {
        self.center_buttons.retain(|b| b.id != id);
        // Kaldırılan düğme sonrasındaki konumları güncelle
        for (i, button) in self.center_buttons.iter_mut().enumerate() {
            button.x = i * (BUTTON_WIDTH + SPACING);
        }
    }

    /// Saati ve tarihi güncelle (görev çubuğu sağ kısmında gösterilir)
    pub fn update_clock(&mut self, hours: u8, minutes: u8, month: u8, day: u8) {
        self.clock_string = format!("{:02}:{:02}", hours, minutes);

        let month_names = ["Jan", "Feb", "Mar", "Apr", "May", "Jun",
                          "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
        if month > 0 && month <= 12 {
            self.date_string = format!("{} {}", month_names[(month - 1) as usize], day);
        }
    }

    /// Fare hareketi işle. Otomatik gizleme ve hover durumlarını günceller.
    pub fn on_mouse_move(&mut self, x: i32, y: i32, fb_height: usize) -> TaskbarEvent {
        self.last_mouse = (x, y);

        // Otomatik gizleme kontrolü
        if self.auto_hide {
            let taskbar_y = fb_height as i32 - self.height as i32;
            if y >= taskbar_y {
                self.visible = true;
            } else if y < taskbar_y - 20 {
                self.visible = false;
            }
        }

        // Düğme hover durumlarını güncelle
        let mut found_hover = false;

        for button in self.left_buttons.iter_mut() {
            let was_hovered = button.hovered;
            button.hovered = button.hit_test(x, y);

            if button.hovered && !was_hovered {
                self.hovered_button = Some(button.id);
                return TaskbarEvent::ButtonHovered(button.id, button.tooltip.clone());
            }
            if button.hovered {
                found_hover = true;
            }
        }

        for button in self.center_buttons.iter_mut() {
            let was_hovered = button.hovered;
            button.hovered = button.hit_test(x, y);

            if button.hovered && !was_hovered {
                self.hovered_button = Some(button.id);
                return TaskbarEvent::ButtonHovered(button.id, button.tooltip.clone());
            }
            if button.hovered {
                found_hover = true;
            }
        }

        if !found_hover {
            self.hovered_button = None;
        }

        TaskbarEvent::None
    }

    /// Fare sol tuşu basış işle. İlgili düğme türüne göre olay döndürür.
    pub fn on_mouse_down(&mut self, x: i32, y: i32) -> TaskbarEvent {
        // Sol düğmeleri kontrol et
        for button in self.left_buttons.iter_mut() {
            if button.hit_test(x, y) {
                button.pressed = true;
                self.pressed_button = Some(button.id);

                return match button.button_type {
                    ButtonType::Start => TaskbarEvent::StartMenuRequested,
                    ButtonType::Search => TaskbarEvent::SearchRequested,
                    ButtonType::TaskView => TaskbarEvent::TaskViewRequested,
                    ButtonType::PinnedApp => TaskbarEvent::AppLaunched(button.icon_type),
                    _ => TaskbarEvent::ButtonPressed(button.id),
                };
            }
        }

        // Orta düğmeleri (çalışan uygulamalar) kontrol et
        for button in self.center_buttons.iter_mut() {
            if button.hit_test(x, y) {
                button.pressed = true;
                self.pressed_button = Some(button.id);
                return TaskbarEvent::WindowActivated(button.id);
            }
        }

        // Sistem tepsisini kontrol et
        let tray_x = self.width - 100;
        let tray_y = y as usize;
        for item in &self.system_tray {
            if item.hit_test(x, y, tray_x, tray_y) {
                return TaskbarEvent::TrayItemClicked(item.id);
            }
        }

        TaskbarEvent::None
    }

    /// Fare sol tuşu bırakış işle. Tüm düğmelerin basılı durumunu sıfırla.
    pub fn on_mouse_up(&mut self) {
        for button in self.left_buttons.iter_mut() {
            button.pressed = false;
        }
        for button in self.center_buttons.iter_mut() {
            button.pressed = false;
        }
        self.pressed_button = None;
    }

    /// Görev çubuğunu çiz.
    /// Arka plan → sol düğmeler → orta düğmeler (ortalanmış) → sistem tepsisi → saat sırası.
    pub fn draw(&self, fb: &mut Framebuffer) {
        if !self.visible {
            return;
        }

        let y = fb.height - self.height;

        // Arka plan
        fb.draw_rect(0, y, self.width, self.height, Theme::TASKBAR_BG.to_u32());

        // Üst kenarlık çizgisi
        fb.draw_rect(0, y, self.width, 1, Theme::BORDER.to_u32());

        // Sol düğmeleri çiz
        for button in &self.left_buttons {
            button.draw(fb, self.width, fb.height);
        }

        // Orta düğmeleri ekranda yatayda ortala
        let center_start = self.width / 2 - (self.center_buttons.len() * (BUTTON_WIDTH + SPACING)) / 2;
        for (i, button) in self.center_buttons.iter().enumerate() {
            let mut btn = button.clone();
            btn.x = center_start + i * (BUTTON_WIDTH + SPACING);
            btn.draw(fb, self.width, fb.height);
        }

        // Sistem tepsisi simgelerini sağdan sola çiz
        let mut tray_x = self.width - 16;
        for item in self.system_tray.iter().rev() {
            item.draw(fb, tray_x, y + (self.height - 16) / 2);
            tray_x -= 20;
        }

        // Saat ve tarih metni
        let clock_x = self.width - 100;
        let clock_y = y + 8;

        fb.draw_string(clock_x, clock_y, &self.clock_string, Theme::TEXT_PRIMARY.to_u32());
        fb.draw_string(clock_x, clock_y + 14, &self.date_string, Theme::TEXT_SECONDARY.to_u32());

        // Masaüstünü göster düğmesi (en sağda ince şerit)
        let desktop_btn_x = self.width - 4;
        fb.draw_rect(desktop_btn_x, y + 4, 2, self.height - 8, Theme::BORDER.to_u32());
    }

    /// Görev çubuğu genişliğini güncelle (ekran yeniden boyutlandırma)
    pub fn resize(&mut self, width: usize) {
        self.width = width;
    }

    /// Görünür yüksekliği döndür. Otomatik gizleme aktifse gizliyken 0 döner.
    pub fn height(&self) -> usize {
        if self.visible {
            self.height
        } else {
            0
        }
    }

    /// Otomatik gizlemeyi etkinleştir/devre dışı bırak
    pub fn set_auto_hide(&mut self, enabled: bool) {
        self.auto_hide = enabled;
    }

    /// Kimliğe göre çalışan uygulama düğmesini döndür (badge/progress güncelleme için)
    pub fn get_app_button(&mut self, id: u32) -> Option<&mut TaskbarButton> {
        self.center_buttons.iter_mut().find(|b| b.id == id)
    }
}

/// Görev çubuğundan yayılan olaylar
#[derive(Clone, Debug)]
pub enum TaskbarEvent {
    /// Olay yok
    None,
    /// Genel düğme tıklaması
    ButtonPressed(u32),
    /// Düğme üzerine gelinidi (araç ipucu gösterimi için)
    ButtonHovered(u32, String),
    /// Başlat menüsü açılması istendi
    StartMenuRequested,
    /// Arama (Spotlight) açılması istendi
    SearchRequested,
    /// Görev görünümü açılması istendi
    TaskViewRequested,
    /// Sabitlenmiş uygulama başlatılması istendi
    AppLaunched(IconType),
    /// Çalışan uygulama penceresi etkinleştirilmeli
    WindowActivated(u32),
    /// Sistem tepsisi simgesine tıklandı
    TrayItemClicked(u32),
}

// ============================================================================
// GLOBAL GÖREV ÇUBUĞU (Spin Mutex Singleton)
// ============================================================================

lazy_static::lazy_static! {
    /// `spin::Mutex` ile korunan global görev çubuğu örneği.
    /// Çekirdek modunda tek bir görev çubuğuna global erişim sağlar.
    static ref TASKBAR: Mutex<EnhancedTaskbar> = Mutex::new(EnhancedTaskbar::new(1920));
}

/// Görev çubuğunu başlat (ekran genişliğini ayarla)
pub fn init(width: usize) {
    let mut taskbar = TASKBAR.lock();
    taskbar.resize(width);
    crate::serial_println!("[GUI] Taskbar initialized ({}px wide)", width);
}

/// Global görev çubuğuna erişim sağla
pub fn get() -> &'static Mutex<EnhancedTaskbar> {
    &TASKBAR
}
