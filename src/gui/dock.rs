//! # echOS Uyarlanabilir Dock (Adaptive Dock)
//!
//! Üzerine gelindiğinde büyütme efekti uygulayan animasyonlu dock.
//! Uygulama göstergeleri, bildirim rozetleri ve sürükle-bırak yeniden
//! sıralamayı destekler.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use core::cmp::{max, min};
use spin::Mutex;
use libm::{sinf, cosf};

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};
use crate::gui::animation::{Animation, EasingType, AnimationTarget, AnimationTargetType};

// ============================================================================
// DOCK SABİTLERİ
// ============================================================================

/// Varsayılan dock yüksekliği (piksel)
pub const DOCK_HEIGHT: usize = 70;

/// Varsayılan ikon boyutu (piksel)
pub const ICON_SIZE: usize = 48;

/// Büyütülmüş maksimum ikon boyutu (piksel)
pub const MAX_ICON_SIZE: usize = 80;

/// İkonlar arası boşluk (piksel)
pub const ICON_SPACING: usize = 8;

/// Büyütme etki yarıçapı (efektin yayıldığı mesafe)
pub const MAG_RADIUS: usize = 100;

/// Hover durumunda kullanılan orta boyut önbelleği
pub const HOVER_ICON_SIZE: usize = 64;

/// Dock'un mikro yeniden çizim optimizasyonu için kirli bölge dikdörtgeni.
///
/// Yalnızca değişen alanlar yeniden render edilir; böylece her karede
/// tüm dock yüzeyi çizilmez.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DockDirtyRect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

impl DockDirtyRect {
    /// İki kirli bölgenin çakışıp çakışmadığını döndürür.
    fn intersects(&self, other: &DockDirtyRect) -> bool {
        let ax2 = self.x + self.width;
        let ay2 = self.y + self.height;
        let bx2 = other.x + other.width;
        let by2 = other.y + other.height;
        self.x < bx2 && ax2 > other.x && self.y < by2 && ay2 > other.y
    }

    /// İki kirli bölgeyi kapsayan en küçük dikdörtgeni döndürür.
    fn union(&self, other: &DockDirtyRect) -> DockDirtyRect {
        let x = min(self.x, other.x);
        let y = min(self.y, other.y);
        let x2 = max(self.x + self.width, other.x + other.width);
        let y2 = max(self.y + self.height, other.y + other.height);
        DockDirtyRect { x, y, width: x2 - x, height: y2 - y }
    }
}

/// Birden fazla kirli dikdörtgeni birleştirerek yöneten küme.
///
/// Çakışan bölgeler otomatik olarak birleştirilir.
/// Maksimum kapasiteye ulaşıldığında en eski bölge çıkarılır.
#[derive(Clone, Debug)]
pub struct DirtyRectSet {
    rects: Vec<DockDirtyRect>,
    max_rects: usize,
}

impl DirtyRectSet {
    /// Belirtilen maksimum kapasite ile yeni küme oluşturur.
    fn new(max_rects: usize) -> Self {
        Self { rects: Vec::new(), max_rects }
    }

    /// Ekran sınırlarına göre kırpılmış bir bölge ekler.
    /// Mevcut bölgelerle çakışıyorsa birleştirir.
    fn add(&mut self, mut rect: DockDirtyRect, screen_w: usize, screen_h: usize) {
        if rect.x >= screen_w || rect.y >= screen_h {
            return;
        }
        rect.width = rect.width.min(screen_w - rect.x);
        rect.height = rect.height.min(screen_h - rect.y);
        if rect.width == 0 || rect.height == 0 {
            return;
        }

        for i in 0..self.rects.len() {
            if self.rects[i].intersects(&rect) {
                self.rects[i] = self.rects[i].union(&rect);
                return;
            }
        }

        if self.rects.len() >= self.max_rects {
            self.rects.remove(0);
        }
        self.rects.push(rect);
    }

    /// Tüm kirli bölgeleri temizler.
    fn clear(&mut self) {
        self.rects.clear();
    }

    /// Kirli bölge kuyruğunun boş olup olmadığını döndürür.
    fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }
}

/// Ön hesaplanmış sprite ölçekleme kademelerini tutan önbellek.
///
/// Üç boyut kademesi: küçük (normal), hover (orta) ve maksimum (tam büyütme).
#[derive(Clone, Copy, Debug)]
pub struct DockSpriteCache {
    /// Normal durum piksel boyutu
    pub small_px: usize,
    /// Hover durumu piksel boyutu
    pub hovered_px: usize,
    /// Maksimum büyütme piksel boyutu
    pub max_px: usize,
}

impl DockSpriteCache {
    /// Varsayılan boyut kademelerini döndürür.
    fn default_sizes() -> Self {
        Self {
            small_px: ICON_SIZE,
            hovered_px: HOVER_ICON_SIZE,
            max_px: MAX_ICON_SIZE,
        }
    }
}

/// Dock büyütme animasyonunu Q8 sabit noktalı aritmetikle yöneten yapı.
///
/// Q8 formatında `strength_q8 = 255` tam büyütme, `0` ise normal boyutu ifade eder.
/// Her karede `tick()` çağrısıyla hedef değere yaklaşır (exponential decay).
#[derive(Clone, Copy, Debug)]
pub struct HoverAnimation {
    /// Şu an üzerine gelinen ikon indeksi
    pub active_index: Option<usize>,
    /// Mevcut büyütme gücü (Q8: 0–255)
    pub strength_q8: u16,
    /// Hedef büyütme gücü (Q8: 0–255)
    target_q8: u16,
}

impl HoverAnimation {
    /// Sıfır güçle yeni hover animasyonu oluşturur.
    fn new() -> Self {
        Self {
            active_index: None,
            strength_q8: 0,
            target_q8: 0,
        }
    }

    /// Hover'ı verilen ikon indeksine yönlendirir.
    /// `None` verilirse büyütme hedefi sıfıra çekilir.
    fn set_hover(&mut self, idx: Option<usize>) {
        self.active_index = idx;
        self.target_q8 = if idx.is_some() { 255 } else { 0 };
    }

    /// Her karede çağrılır; mevcut gücü hedefe yaklaştırır.
    ///
    /// Yumuşak geçiş için `diff >> 2` (çeyrek adım) kullanılır.
    fn tick(&mut self) {
        let current = self.strength_q8 as i32;
        let target = self.target_q8 as i32;
        let diff = target - current;
        if diff == 0 {
            return;
        }
        let mut step = diff >> 2; // Çeyrek adım — exponential decay
        if step == 0 {
            step = if diff > 0 { 1 } else { -1 };
        }
        self.strength_q8 = (current + step).clamp(0, 255) as u16;
    }
}

// ============================================================================
// DOCK ÖĞESİ
// ============================================================================

/// Dock'taki tek bir uygulama veya kısayol öğesi.
///
/// Her öğe; görünen ad, ikon türü, çalışma/aktiflik durumu,
/// bildirim rozeti, ilerleme çubuğu ve animasyon durumunu barındırır.
pub struct DockItem {
    /// Benzersiz kimlik
    pub id: u32,
    /// Görünen ad
    pub name: String,
    /// İkon türü
    pub icon: DockIcon,
    /// Uygulama çalışıyor mu
    pub running: bool,
    /// Uygulama aktif (en önde) mi
    pub active: bool,
    /// Bildirim rozeti sayısı
    pub badge_count: u32,
    /// İlerleme değeri (0.0 – 1.0)
    pub progress: f32,
    /// Tıklanınca çalıştırılacak eylem
    pub action: DockAction,
    /// Animasyon için mevcut görüntüleme boyutu (piksel, f32)
    pub current_size: f32,
    /// Animasyon için hedef boyut (piksel, f32)
    pub target_size: f32,
    /// Zıplama animasyonu Y ofseti (piksel)
    pub bounce_offset: f32,
    /// Zıplama hızı (piksel/kare)
    pub bounce_velocity: f32,
    /// Zıplama animasyonu aktif mi
    pub bouncing: bool,
}

/// Dock'ta kullanılan yerleşik ikon türleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockIcon {
    Finder,
    Launchpad,
    Settings,
    Safari,
    Mail,
    Messages,
    Maps,
    Photos,
    Music,
    Notes,
    Calendar,
    Terminal,
    Files,
    TextEdit,
    Calculator,
    Trash,
    Downloads,
    Custom(u16),
}

/// Dock öğesi tıklandığında tetiklenecek eylem
#[derive(Clone, Debug)]
pub enum DockAction {
    LaunchApp(String),
    OpenFolder(String),
    OpenSettings,
    EmptyTrash,
    ShowLaunchpad,
    None,
}

impl DockItem {
    /// Varsayılan değerlerle yeni bir dock öğesi oluşturur.
    pub fn new(id: u32, name: &str, icon: DockIcon) -> Self {
        DockItem {
            id,
            name: String::from(name),
            icon,
            running: false,
            active: false,
            badge_count: 0,
            progress: 0.0,
            action: DockAction::None,
            current_size: ICON_SIZE as f32,
            target_size: ICON_SIZE as f32,
            bounce_offset: 0.0,
            bounce_velocity: 0.0,
            bouncing: false,
        }
    }

    /// Uygulama başlatma eylemiyle dock öğesi oluşturur.
    pub fn app(id: u32, name: &str, icon: DockIcon, app_id: &str) -> Self {
        let mut item = Self::new(id, name, icon);
        item.action = DockAction::LaunchApp(String::from(app_id));
        item
    }

    /// Klasör açma eylemiyle dock öğesi oluşturur.
    pub fn folder(id: u32, name: &str, path: &str) -> Self {
        let mut item = Self::new(id, name, DockIcon::Files);
        item.action = DockAction::OpenFolder(String::from(path));
        item
    }

    /// Zıplama animasyonunu başlatır.
    ///
    /// Uygulama başlatıldığında veya dock öğesi tıklandığında çağrılır.
    pub fn start_bounce(&mut self) {
        self.bouncing = true;
        self.bounce_velocity = -15.0; // Başlangıç yukarı hızı
    }

    /// Zıplama animasyonunu bir kare ilerletir.
    ///
    /// ## Fizik Modeli
    ///
    /// ```text
    ///  bounce_offset:  0 (zemin)
    ///     ↑ +15 px/kare başlangıç
    ///     ↓ +0.8 px/kare² yerçekimi ivmesi
    ///  Zeminde: hız = -(hız × 0.5)  ← sönümlü sekme
    ///  |hız| < 2.0 olunca animasyon durur
    /// ```
    pub fn update_bounce(&mut self, dt: f32) {
        if !self.bouncing {
            return;
        }

        // Yerçekimi uygula
        self.bounce_velocity += 0.8; // Yerçekimi ivmesi
        self.bounce_offset += self.bounce_velocity;

        // Zemine çarpma kontrolü
        if self.bounce_offset >= 0.0 {
            self.bounce_offset = 0.0;
            self.bounce_velocity = -self.bounce_velocity * 0.5; // Sönümlü sekme

            // Hız çok düşükse animasyonu durdur
            if self.bounce_velocity.abs() < 2.0 {
                self.bouncing = false;
                self.bounce_offset = 0.0;
                self.bounce_velocity = 0.0;
            }
        }
    }

    /// Boyutu hedef değere doğru yumuşak geçişle günceller.
    ///
    /// `diff × 0.3` katsayısıyla her karede hedefe yaklaşır.
    pub fn update_size(&mut self, dt: f32) {
        let diff = self.target_size - self.current_size;
        if diff.abs() > 0.1 {
            self.current_size += diff * 0.3; // Yumuşak boyut interpolasyonu
        } else {
            self.current_size = self.target_size;
        }
    }

    /// Dock öğesini framebuffer'a çizer.
    ///
    /// Sıralamayla: gölge → arka plan → ikon → çalışma göstergesi → rozet → ilerleme çubuğu.
    pub fn draw(&self, fb: &mut Framebuffer, x: usize, y: usize, size: usize, dock_y: usize) {
        let icon_size = size;
        let icon_x = x;
        let icon_y = y as i32 - (self.bounce_offset as i32);

        // Gölge çiz
        let shadow_size = icon_size + 4;
        let shadow_y = dock_y + DOCK_HEIGHT - 10;
        fb.draw_rect(icon_x - 2, shadow_y, shadow_size, 4, 0x40000000);

        // Yuvarlak köşeli ikon arka planını çiz
        let bg_color = if self.active {
            0x40FFFFFF // Aktif: daha parlak
        } else if self.running {
            0x20FFFFFF // Çalışıyor: orta parlaklık
        } else {
            0x10FFFFFF // Çalışmıyor: çok soluk
        };

        self.draw_rounded_icon_bg(fb, icon_x, icon_y as usize, icon_size, bg_color);

        // İkonu çiz
        self.draw_icon(fb, icon_x, icon_y as usize, icon_size);

        // Çalışma göstergesi (alttaki nokta)
        if self.running {
            let dot_y = dock_y + DOCK_HEIGHT - 6;
            let dot_size = if self.active { 5 } else { 4 };
            let dot_color = if self.active {
                Theme::ACCENT_PRIMARY.to_u32()
            } else {
                0x80FFFFFF
            };

            let dot_x = icon_x + icon_size / 2 - dot_size / 2;
            self.draw_dot(fb, dot_x, dot_y, dot_size, dot_color);
        }

        // Bildirim rozeti çiz
        if self.badge_count > 0 {
            let badge_x = icon_x + icon_size - 16;
            let badge_y = icon_y as usize - 4;

            // Rozet arka planı (kırmızı daire)
            for py in 0..16 {
                for px in 0..16 {
                    let dx = px as i32 - 8;
                    let dy = py as i32 - 8;
                    if dx * dx + dy * dy <= 64 {
                        fb.plot_pixel(badge_x + px, badge_y + py, Theme::ERROR.to_u32());
                    }
                }
            }

            // Rozet metni
            if self.badge_count < 10 {
                let digit = char::from(b'0' + self.badge_count as u8);
                fb.draw_char(badge_x + 5, badge_y + 2, digit, Theme::TEXT_ON_ACCENT.to_u32());
            } else {
                fb.draw_string(badge_x + 2, badge_y + 2, "9+", Theme::TEXT_ON_ACCENT.to_u32());
            }
        }

        // İlerleme çubuğu çiz
        if self.progress > 0.0 {
            let bar_width = (icon_size as f32 * self.progress) as usize;
            let bar_y = dock_y + DOCK_HEIGHT - 3;
            fb.draw_rect(icon_x, bar_y, bar_width, 2, Theme::ACCENT_PRIMARY.to_u32());
        }
    }

    /// Yuvarlak köşeli ikon arka planını çizer.
    ///
    /// `radius = size / 5` hesaplamasıyla köşe yuvarlaması belirlenir.
    fn draw_rounded_icon_bg(&self, fb: &mut Framebuffer, x: usize, y: usize, size: usize, color: u32) {
        let radius = size / 5;

        for py in 0..size {
            for px in 0..size {
                let in_corner =
                    (px < radius && py < radius &&
                     (radius - px) as i32 * (radius - px) as i32 + (radius - py) as i32 * (radius - py) as i32 > radius as i32 * radius as i32) ||
                    (px >= size - radius && py < radius &&
                     (px - (size - radius)) as i32 * (px - (size - radius)) as i32 + (radius - py) as i32 * (radius - py) as i32 > radius as i32 * radius as i32) ||
                    (px < radius && py >= size - radius &&
                     (radius - px) as i32 * (radius - px) as i32 + (py - (size - radius)) as i32 * (py - (size - radius)) as i32 > radius as i32 * radius as i32) ||
                    (px >= size - radius && py >= size - radius &&
                     (px - (size - radius)) as i32 * (px - (size - radius)) as i32 + (py - (size - radius)) as i32 * (py - (size - radius)) as i32 > radius as i32 * radius as i32);

                if !in_corner {
                    fb.plot_pixel(x + px, y + py, color);
                }
            }
        }
    }

    /// Dolu daire çizer (çalışma göstergesi için).
    fn draw_dot(&self, fb: &mut Framebuffer, x: usize, y: usize, size: usize, color: u32) {
        let radius = size / 2;
        for py in 0..size {
            for px in 0..size {
                let dx = px as i32 - radius as i32;
                let dy = py as i32 - radius as i32;
                if dx * dx + dy * dy <= (radius * radius) as i32 {
                    fb.plot_pixel(x + px, y + py, color);
                }
            }
        }
    }

    /// İkon grafiğini türe göre çizer.
    fn draw_icon(&self, fb: &mut Framebuffer, x: usize, y: usize, size: usize) {
        let center_x = x + size / 2;
        let center_y = y + size / 2;
        let icon_scale = size as f32 / ICON_SIZE as f32;

        match self.icon {
            DockIcon::Finder => {
                // Gülen yüz (Finder maskotu)
                let face_color = 0xFF3D67FF; // Mavi
                self.draw_circle(fb, center_x, center_y, (size as f32 * 0.4) as usize, face_color);

                // Gözler
                let eye_y = center_y - size / 8;
                fb.draw_rect(center_x - size / 5, eye_y, 4, 4, 0xFFFFFFFF);
                fb.draw_rect(center_x + size / 5 - 4, eye_y, 4, 4, 0xFFFFFFFF);

                // Gülümseme
                let smile_y = center_y + size / 10;
                fb.draw_rect(center_x - size / 6, smile_y, size / 3, 2, 0xFFFFFFFF);
            }

            DockIcon::Launchpad => {
                // 3×3 nokta ızgarası (launchpad görünümü)
                let dot_size = (6.0 * icon_scale) as usize;
                let spacing = (14.0 * icon_scale) as usize;
                let start_x = center_x - spacing;
                let start_y = center_y - spacing;

                for row in 0..3 {
                    for col in 0..3 {
                        let dot_x = start_x + col * spacing - dot_size / 2;
                        let dot_y = start_y + row * spacing - dot_size / 2;
                        let color = if (row + col) % 2 == 0 {
                            Theme::ACCENT_PRIMARY.to_u32()
                        } else {
                            0xFF666666
                        };
                        fb.draw_rect(dot_x, dot_y, dot_size, dot_size, color);
                    }
                }
            }

            DockIcon::Settings => {
                // Dişli çark ikonu
                let outer_r = (size as f32 * 0.35) as usize;
                let inner_r = (size as f32 * 0.15) as usize;

                // Dış dişler (8 diş, 45° aralıklarla)
                for angle in 0..8 {
                    let a = angle as f32 * core::f32::consts::PI / 4.0;
                    let tooth_x = center_x as i32 + (cosf(a) * outer_r as f32) as i32;
                    let tooth_y = center_y as i32 + (sinf(a) * outer_r as f32) as i32;
                    let tooth_size = (8.0 * icon_scale) as usize;
                    fb.draw_rect(
                        ((tooth_x - tooth_size as i32 / 2).max(0)) as usize,
                        ((tooth_y - tooth_size as i32 / 2).max(0)) as usize,
                        tooth_size, tooth_size,
                        0xFF888888
                    );
                }

                // Merkez daire
                self.draw_circle(fb, center_x, center_y, inner_r, 0xFF666666);
            }

            DockIcon::Safari => {
                // Pusula ikonu
                let r = (size as f32 * 0.35) as usize;
                self.draw_circle_outline(fb, center_x, center_y, r, 0xFF007AFF);

                // Pusula iğnesi
                fb.draw_rect(center_x - 2, center_y - r + 4, 4, r - 4, 0xFFFF3B30);
                fb.draw_rect(center_x - 2, center_y + 4, 4, r - 4, 0xFFFFFFFF);
            }

            DockIcon::Mail => {
                // Zarf ikonu
                let w = (size as f32 * 0.6) as usize;
                let h = (size as f32 * 0.4) as usize;
                let mail_x = center_x - w / 2;
                let mail_y = center_y - h / 2;

                fb.draw_rect(mail_x, mail_y, w, h, 0xFF007AFF);

                // Zarf kapağı (V şekli)
                for i in 0..w/2 {
                    let flap_y = mail_y + i * h / w;
                    fb.plot_pixel(mail_x + i, flap_y, 0xFF0055D5);
                    fb.plot_pixel(mail_x + w - 1 - i, flap_y, 0xFF0055D5);
                }
            }

            DockIcon::Messages => {
                // Mesaj balonu
                let r = (size as f32 * 0.3) as usize;
                self.draw_circle(fb, center_x - 4, center_y - 4, r, 0xFF34C759);

                // Balon kuyruğu
                fb.draw_rect(center_x + r / 2, center_y + r / 2, 8, 8, 0xFF34C759);
            }

            DockIcon::Music => {
                // Müzik notası
                let note_color = 0xFFFC3C44;
                let note_size = (size as f32 * 0.3) as usize;

                // Nota başı (elips)
                self.draw_ellipse(fb, center_x - note_size / 3, center_y + note_size / 2,
                                  note_size, note_size / 2, note_color);

                // Nota sapı
                fb.draw_rect(center_x + note_size / 3, center_y - note_size, 3, note_size * 2, note_color);

                // Nota bayrağı
                fb.draw_rect(center_x + note_size / 3, center_y - note_size, note_size / 2, 4, note_color);
            }

            DockIcon::Terminal => {
                // Terminal penceresi
                let w = (size as f32 * 0.7) as usize;
                let h = (size as f32 * 0.6) as usize;
                let term_x = center_x - w / 2;
                let term_y = center_y - h / 2;

                fb.draw_rect(term_x, term_y, w, h, 0xFF1E1E1E);
                fb.draw_rect(term_x, term_y, w, h / 4, 0xFF333333);

                // Komut istemi
                fb.draw_string(term_x + 4, term_y + h / 4 + 2, ">_", 0xFF00FF00);
            }

            DockIcon::Files => {
                // Klasör ikonu
                let folder_color = 0xFF007AFF;
                let w = (size as f32 * 0.7) as usize;
                let h = (size as f32 * 0.55) as usize;
                let folder_x = center_x - w / 2;
                let folder_y = center_y - h / 2;

                // Üst sekme
                fb.draw_rect(folder_x, folder_y, w / 2, h / 4, folder_color);
                // Gövde
                fb.draw_rect(folder_x, folder_y + h / 4, w, h * 3 / 4, folder_color);
            }

            DockIcon::Trash => {
                // Çöp kutusu
                let trash_color = 0xFF8E8E93;
                let w = (size as f32 * 0.5) as usize;
                let h = (size as f32 * 0.6) as usize;
                let trash_x = center_x - w / 2;
                let trash_y = center_y - h / 2;

                // Kapak
                fb.draw_rect(trash_x - 2, trash_y, w + 4, h / 5, trash_color);
                // Gövde
                fb.draw_rect(trash_x, trash_y + h / 5, w, h * 4 / 5, trash_color);

                // Dikey çizgiler
                for i in 1..4 {
                    let line_x = trash_x + i as usize * w / 4;
                    fb.draw_rect(line_x, trash_y + h / 5 + 2, 2, h * 3 / 5, 0xFF666666);
                }
            }

            DockIcon::TextEdit => {
                // Metin belgesi ikonu
                let doc_color = 0xFFFFCC00;
                let w = (size as f32 * 0.5) as usize;
                let h = (size as f32 * 0.65) as usize;
                let doc_x = center_x - w / 2;
                let doc_y = center_y - h / 2;

                fb.draw_rect(doc_x, doc_y, w, h, doc_color);

                // Kıvrık köşe
                fb.draw_rect(doc_x + w - 6, doc_y, 6, 6, 0xFFE6B800);

                // Metin satırları
                for i in 0..3 {
                    let line_y = doc_y + 10 + i * 6;
                    let line_w = w - 4 - i as usize * 3;
                    fb.draw_rect(doc_x + 2, line_y, line_w, 2, 0xFF333333);
                }
            }

            DockIcon::Calculator => {
                // Hesap makinesi
                let calc_color = 0xFF1C1C1E;
                let w = (size as f32 * 0.6) as usize;
                let h = (size as f32 * 0.75) as usize;
                let calc_x = center_x - w / 2;
                let calc_y = center_y - h / 2;

                fb.draw_rect(calc_x, calc_y, w, h, calc_color);

                // Ekran
                fb.draw_rect(calc_x + 2, calc_y + 2, w - 4, h / 4, 0xFF505050);

                // Düğmeler (4×4 ızgara)
                let btn_size = (w - 8) / 4;
                for row in 0..4 {
                    for col in 0..4 {
                        let btn_x = calc_x + 2 + col * (btn_size + 1);
                        let btn_y = calc_y + h / 4 + 4 + row * (btn_size + 1);
                        let btn_color = if row == 3 { 0xFFFF9500 } else { 0xFF333333 };
                        fb.draw_rect(btn_x, btn_y, btn_size, btn_size, btn_color);
                    }
                }
            }

            DockIcon::Calendar => {
                // Takvim ikonu (güncel gün gösterimi)
                let cal_color = 0xFFFFFFFF;
                let w = (size as f32 * 0.65) as usize;
                let h = (size as f32 * 0.65) as usize;
                let cal_x = center_x - w / 2;
                let cal_y = center_y - h / 2;

                fb.draw_rect(cal_x, cal_y, w, h, cal_color);

                // Kırmızı üst şerit
                fb.draw_rect(cal_x, cal_y, w, h / 4, 0xFFFF3B30);

                // Gün sayısı (gerçek tarih sistemi geliştirmede bağlanacak)
                fb.draw_string(center_x - 8, center_y, "25", 0xFF333333);
            }

            DockIcon::Downloads => {
                // Aşağı indirme oku
                let r = (size as f32 * 0.35) as usize;
                self.draw_circle(fb, center_x, center_y, r, 0xFF007AFF);

                // Ok
                let arrow_h = r;
                let arrow_w = r / 2;
                fb.draw_rect(center_x - 3, center_y - arrow_h / 2, 6, arrow_h, 0xFFFFFFFF);
                fb.draw_rect(center_x - arrow_w, center_y, arrow_w, 3, 0xFFFFFFFF);
                fb.draw_rect(center_x + 3, center_y, arrow_w, 3, 0xFFFFFFFF);
            }

            DockIcon::Maps => {
                // Harita iğnesi
                let pin_color = 0xFFFF3B30;
                let pin_r = (size as f32 * 0.25) as usize;

                // İğne başı
                self.draw_circle(fb, center_x, center_y - pin_r, pin_r, pin_color);

                // İğne ucu (aşağı daralan üçgen)
                for i in 0..pin_r {
                    let w = pin_r * 2 - i * 2;
                    fb.draw_rect(center_x - w / 2, center_y + i, w, 1, pin_color);
                }
            }

            DockIcon::Photos => {
                // Çiçek/yaprak ikonu (5 renkli yaprak)
                let petal_r = (size as f32 * 0.2) as usize;
                let center_r = (size as f32 * 0.12) as usize;

                let colors = [0xFFFF2D55, 0xFFFF9500, 0xFFFFCC00, 0xFF34C759, 0xFF007AFF];

                for (i, &color) in colors.iter().enumerate() {
                    let angle = i as f32 * 2.0 * core::f32::consts::PI / 5.0;
                    let px = center_x as i32 + (cosf(angle) * petal_r as f32 * 1.2) as i32;
                    let py = center_y as i32 + (sinf(angle) * petal_r as f32 * 1.2) as i32;
                    self.draw_circle(fb, px.max(0) as usize, py.max(0) as usize, petal_r, color);
                }

                // Merkez beyaz daire
                self.draw_circle(fb, center_x, center_y, center_r, 0xFFFFFFFF);
            }

            DockIcon::Notes => {
                // Sarı not defteri
                let note_color = 0xFFFFCC00;
                let w = (size as f32 * 0.6) as usize;
                let h = (size as f32 * 0.7) as usize;
                let note_x = center_x - w / 2;
                let note_y = center_y - h / 2;

                fb.draw_rect(note_x, note_y, w, h, note_color);

                // Yatay çizgiler
                for i in 1..4 {
                    let line_y = note_y + i as usize * h / 4;
                    fb.draw_rect(note_x + 4, line_y, w - 8, 1, 0xFFB38F00);
                }
            }

            DockIcon::Custom(code) => {
                // Özel ikon — renkli daire + harf
                let color = match code % 8 {
                    0 => 0xFFFF3B30,
                    1 => 0xFFFF9500,
                    2 => 0xFFFFCC00,
                    3 => 0xFF34C759,
                    4 => 0xFF00C7BE,
                    5 => 0xFF007AFF,
                    6 => 0xFF5856D6,
                    _ => 0xFFFF2D55,
                };

                let r = (size as f32 * 0.35) as usize;
                self.draw_circle(fb, center_x, center_y, r, color);

                // Harf göstergesi
                let letter = char::from(b'A' + (code % 26) as u8);
                fb.draw_char(center_x - 4, center_y - 6, letter, 0xFFFFFFFF);
            }
        }
    }

    /// Dolu daire çizer; merkez `(x, y)`, yarıçap `radius`.
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

    /// İçi boş daire (çerçeve) çizer; çizgi kalınlığı 2 piksel.
    fn draw_circle_outline(&self, fb: &mut Framebuffer, x: usize, y: usize, radius: usize, color: u32) {
        for py in 0..radius * 2 {
            for px in 0..radius * 2 {
                let dx = px as i32 - radius as i32;
                let dy = py as i32 - radius as i32;
                let dist = dx * dx + dy * dy;
                if dist <= (radius * radius) as i32 && dist > ((radius - 2) * (radius - 2)) as i32 {
                    fb.plot_pixel(x + px - radius, y + py - radius, color);
                }
            }
        }
    }

    /// Dolu elips çizer (nota başı için).
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
}

// ============================================================================
// DOCK
// ============================================================================

/// echOS Uyarlanabilir Dock yöneticisi.
///
/// Dock öğelerini, büyütme animasyonunu, zıplama animasyonunu,
/// otomatik gizlemeyi ve mikro yeniden çizim optimizasyonunu yönetir.
pub struct Dock {
    /// Dock öğeleri listesi
    pub items: Vec<DockItem>,
    /// Sonraki öğe kimliği
    next_id: u32,
    /// Dock konumu (alt/sol/sağ)
    position: DockPosition,
    /// Dock görünür mü
    visible: bool,
    /// Otomatik gizleme etkin mi
    auto_hide: bool,
    /// Dock şu an gizli mi (otomatik gizleme için)
    hidden: bool,
    /// Gizleme animasyonu ilerlemesi (0.0 – 1.0)
    hide_progress: f32,
    /// Son fare X konumu
    mouse_x: i32,
    /// Son fare Y konumu
    mouse_y: i32,
    /// Üzerine gelinen öğe indeksi
    hovered_index: Option<usize>,
    /// Tıklanan öğe indeksi
    clicked_index: Option<usize>,
    /// Sürüklenen öğe indeksi
    dragging_index: Option<usize>,
    /// Sürükleme başlangıç X konumu
    drag_start_x: i32,
    /// Ekran genişliği (piksel)
    screen_width: usize,
    /// Ekran yüksekliği (piksel)
    screen_height: usize,
    /// Animasyonlu dock Y konumu (otomatik gizleme için)
    dock_y: f32,
    /// Büyütme efekti etkin mi
    magnification: bool,
    /// Büyütme yoğunluğu (0.0 – 1.0)
    mag_intensity: f32,
    /// Sprite boyut kademeleri önbelleği
    sprite_cache: DockSpriteCache,
    /// Q8 sabit noktalı hover animasyon durumu
    hover_anim: HoverAnimation,
    /// Mikro yeniden çizim için kirli bölge kuyruğu
    dirty_rects: DirtyRectSet,
}

/// Compositor entegrasyonları için açık mimari adı.
pub type DockState = Dock;

/// Dock'un ekrandaki konumu
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockPosition {
    Bottom,
    Left,
    Right,
}

impl Dock {
    /// Ekran boyutlarıyla yeni bir dock oluşturur ve varsayılan öğeleri ekler.
    pub fn new(screen_width: usize, screen_height: usize) -> Self {
        let mut dock = Dock {
            items: Vec::new(),
            next_id: 1,
            position: DockPosition::Bottom,
            visible: true,
            auto_hide: false,
            hidden: false,
            hide_progress: 0.0,
            mouse_x: 0,
            mouse_y: 0,
            hovered_index: None,
            clicked_index: None,
            dragging_index: None,
            drag_start_x: 0,
            screen_width,
            screen_height,
            dock_y: screen_height as f32,
            magnification: true,
            mag_intensity: 1.0,
            sprite_cache: DockSpriteCache::default_sizes(),
            hover_anim: HoverAnimation::new(),
            dirty_rects: DirtyRectSet::new(12),
        };

        dock.add_default_items();
        dock
    }

    /// Varsayılan dock öğelerini ekler.
    fn add_default_items(&mut self) {
        // Dosya yöneticisi — her zaman ilk sırada
        let mut files = DockItem::app(self.next_id, "Files", DockIcon::Files, "files");
        files.running = true;
        self.items.push(files);
        self.next_id += 1;

        // Uygulama başlatıcı (spotlight/launchpad)
        let mut apps = DockItem::new(self.next_id, "Apps", DockIcon::Custom(200));
        apps.action = DockAction::ShowLaunchpad;
        self.items.push(apps);
        self.next_id += 1;

        // Tarayıcı
        self.items.push(DockItem::app(self.next_id, "Browser", DockIcon::Custom(201), "browser"));
        self.next_id += 1;

        // Terminal
        let mut terminal = DockItem::app(self.next_id, "Terminal", DockIcon::Terminal, "terminal");
        terminal.running = true;
        self.items.push(terminal);
        self.next_id += 1;

        // Ayarlar
        self.items.push(DockItem::app(self.next_id, "Settings", DockIcon::Settings, "settings"));
        self.next_id += 1;

        // Sistem monitörü
        self.items.push(DockItem::app(self.next_id, "Monitor", DockIcon::Custom(202), "activity"));
        self.next_id += 1;

        // Ayırıcı (çöp kutusu bölümü)
        self.items.push(DockItem::new(self.next_id, "", DockIcon::Custom(100)));
        self.next_id += 1;

        // Çöp kutusu
        self.items.push(DockItem::new(self.next_id, "Trash", DockIcon::Trash));
        self.next_id += 1;
    }

    /// Dock'a yeni öğe ekler; çöp kutusu bölümünden önce konumlandırır.
    pub fn add_item(&mut self, item: DockItem) {
        // Çöp kutusu bölümünden önce ekle
        let insert_pos = self.items.len().saturating_sub(2);
        self.items.insert(insert_pos, item);
    }

    /// Kimlikle eşleşen öğeyi dock'tan kaldırır.
    pub fn remove_item(&mut self, id: u32) {
        self.items.retain(|i| i.id != id);
    }

    /// Dock durumunu bir kare günceller.
    ///
    /// Sırasıyla: gizleme animasyonu → hover animasyonu → büyütme → zıplama.
    pub fn update(&mut self, dt: f32) {
        // Gizleme/gösterme animasyonunu güncelle
        if self.auto_hide {
            let target_y = if self.hidden {
                self.screen_height as f32
            } else {
                (self.screen_height - DOCK_HEIGHT) as f32
            };

            self.dock_y += (target_y - self.dock_y) * 0.2;
        } else {
            self.dock_y = (self.screen_height - DOCK_HEIGHT) as f32;
        }

        // Hover animasyonunu ve büyütmeyi güncelle
        self.hover_anim.tick();
        if self.magnification {
            self.update_magnification();
        }

        // Zıplama ve boyut animasyonlarını güncelle
        for item in &mut self.items {
            item.update_bounce(dt);
            item.update_size(dt);
        }
    }

    /// macOS tarzı büyütme animasyonunu hesaplar ve her öğeye uygular.
    ///
    /// ## macOS Tarzı Büyütme (Magnification) Animasyonu
    ///
    /// Fare dock üzerinde hareket ettiğinde, üzerine gelinen ikon ve
    /// komşuları mesafeye göre kademeli büyütülür (Gaussian-benzeri profil).
    ///
    /// ```text
    ///  Fare konumu (↓):    ikon0  ikon1  ikon2 [HOVER] ikon4  ikon5
    ///  Büyüklük (piksel):   48     56      68     80      68     56
    ///                       ────────────────────────────────────────
    ///  Ağırlık (Q8 / 255):   0     84     168    255    168     84
    ///  Uzaklık (slot):        3      2       1      0      1      2
    /// ```
    ///
    /// ## Q8 Sabit Noktalı Aritmetik
    ///
    /// `local_q8`: ikon başına mesafe ağırlığı
    /// ```text
    ///  dist_slots = 0  →  local_q8 = 255  (tam büyütme)
    ///  dist_slots = 1  →  local_q8 = 168  (~66%)
    ///  dist_slots = 2  →  local_q8 =  84  (~33%)
    ///  dist_slots ≥ 3  →  local_q8 =   0  (değişim yok)
    /// ```
    ///
    /// `global_q8`: `HoverAnimation::strength_q8` — fare dock üzerindeyken
    /// her karede 0→255'e yaklaşır; ayrılınca 255→0'a döner.
    ///
    /// Birleşik etki: `influence = (local_q8 × global_q8) >> 8`
    ///
    /// Hedef boyut: `target = min_size + (size_delta × influence) >> 8`
    ///
    /// Yumuşak yaklaşım (exponential decay):
    /// ```text
    ///  diff = target - current
    ///  step = diff >> 2   (çeyrek adım / kare)
    /// ```
    fn update_magnification(&mut self) {
        if self.items.is_empty() {
            return;
        }

        let min_size = self.sprite_cache.small_px as i32;
        let max_size = self.sprite_cache.max_px as i32;
        let size_delta = (max_size - min_size).max(0);
        let active = self.hover_anim.active_index;
        let global_q8 = self.hover_anim.strength_q8 as i32;

        for (i, item) in self.items.iter_mut().enumerate() {
            // İkon ile hover merkezi arasındaki slot mesafesi
            let dist_slots = match active {
                Some(a) if i >= a => (i - a) as i32,
                Some(a) => (a - i) as i32,
                None => 99,
            };

            // Mesafeye göre Q8 yerel ağırlık
            let local_q8 = match dist_slots {
                0 => 255,
                1 => 168,
                2 => 84,
                _ => 0,
            };

            // Birleşik etki: local × global / 256
            let influence_q8 = (local_q8 * global_q8) >> 8;
            let target_size = min_size + ((size_delta * influence_q8) >> 8);

            // Çeyrek adımlı yumuşak yaklaşım
            let cur = item.current_size as i32;
            let diff = target_size - cur;
            let mut step = diff >> 2;
            if step == 0 && diff != 0 {
                step = if diff > 0 { 1 } else { -1 };
            }
            let next = (cur + step).clamp(min_size, max_size) as f32;
            item.current_size = next;
            item.target_size = target_size as f32;
        }
    }

    /// Dock'u framebuffer'a çizer.
    ///
    /// Kirli bölge kuyruğu boşsa tam çizim (`draw_full`) yapılır;
    /// aksi hâlde yalnızca değişen bölgeler yeniden render edilir.
    pub fn draw(&mut self, fb: &mut Framebuffer) {
        if !self.visible {
            return;
        }

        if self.dirty_rects.is_empty() {
            self.draw_full(fb);
            return;
        }

        let dirty = self.dirty_rects.rects.clone();
        for rect in dirty {
            self.render_dock_dirty_rect(fb, rect);
        }
        self.dirty_rects.clear();
    }

    /// Tam dock çizimini gerçekleştirir (tüm öğeler dahil).
    fn draw_full(&self, fb: &mut Framebuffer) {
        if !self.visible {
            return;
        }

        let dock_y = self.dock_y as usize;

        // Dock arka planını çiz (cam efekti)
        let total_width = self.items.len() * (ICON_SIZE + ICON_SPACING) + ICON_SPACING * 2;
        let dock_x = self.screen_width.saturating_sub(total_width) / 2;

        // Bulanıklaştırma efektli arka plan (basitleştirilmiş)
        let bg_height = DOCK_HEIGHT + 4;
        let bg_y = dock_y;

        // Yarı saydam arka plan satırlarını çiz
        for y in 0..bg_height {
            let alpha = if y < 4 { 0x20 } else if y > bg_height - 8 { 0x10 } else { 0x40 };
            let row_color = (alpha << 24) | 0x00FFFFFF;

            for x in 0..total_width {
                let px = dock_x + x;
                let py = bg_y + y;

                if px < self.screen_width && py < self.screen_height {
                    // Arka planla karıştır
                    let ptr = unsafe { (fb.base_addr as *mut u32).add(py * fb.pixels_per_scan_line + px) };
                    let bg = unsafe { *ptr };
                    let blended = Self::blend_colors(bg, row_color);
                    unsafe { *ptr = blended; }
                }
            }
        }

        // Üst kenarlık çiz
        fb.draw_rect(dock_x, dock_y, total_width, 1, 0x60FFFFFF);

        // Büyütme uygulandıktan sonra öğeleri çiz
        let mut item_x = dock_x + ICON_SPACING;

        for (i, item) in self.items.iter().enumerate() {
            let size = item.current_size as usize;
            let x_offset = (ICON_SIZE as i32 - size as i32) / 2;
            let y_offset = (ICON_SIZE as i32 - size as i32) / 2;

            let draw_x = (item_x as i32 + x_offset).max(0) as usize;
            let draw_y = (dock_y as i32 + DOCK_HEIGHT as i32 - size as i32 - 8 + y_offset).max(0) as usize;

            item.draw(fb, draw_x, draw_y, size, dock_y);

            // Büyüme oranını hesaba katarak X konumunu ilerlet
            item_x += ((item.current_size + ICON_SPACING as f32) / 2.0 + ICON_SIZE as f32 / 2.0) as usize;
        }
    }

    /// Yalnızca belirtilen kirli bölgeyi yeniden çizer (mikro optimizasyon).
    pub fn render_dock_dirty_rect(&self, fb: &mut Framebuffer, dirty: DockDirtyRect) {
        if !self.visible {
            return;
        }

        let dock_y = self.dock_y as usize;
        let total_width = self.items.len() * (ICON_SIZE + ICON_SPACING) + ICON_SPACING * 2;
        let dock_x = self.screen_width.saturating_sub(total_width) / 2;
        let dock_rect = DockDirtyRect {
            x: dock_x,
            y: dock_y,
            width: total_width,
            height: DOCK_HEIGHT + 4,
        };

        if !dock_rect.intersects(&dirty) {
            return;
        }

        // Kesişim bölgesini hesapla
        let x0 = max(dock_rect.x, dirty.x);
        let y0 = max(dock_rect.y, dirty.y);
        let x1 = min(dock_rect.x + dock_rect.width, dirty.x + dirty.width);
        let y1 = min(dock_rect.y + dock_rect.height, dirty.y + dirty.height);

        // Yalnızca kesişen pikselleri yeniden arka planla karıştır
        for py in y0..y1 {
            let local_y = py - dock_y;
            let alpha = if local_y < 4 { 0x20 } else if local_y > DOCK_HEIGHT - 4 { 0x10 } else { 0x40 };
            let row_color = (alpha << 24) | 0x00FFFFFF;
            for px in x0..x1 {
                if px < self.screen_width && py < self.screen_height {
                    let ptr = unsafe { (fb.base_addr as *mut u32).add(py * fb.pixels_per_scan_line + px) };
                    let bg = unsafe { *ptr };
                    let blended = Self::blend_colors(bg, row_color);
                    unsafe { *ptr = blended; }
                }
            }
        }

        // Kirli bölgeyle kesişen öğeleri yeniden çiz
        let mut item_x = dock_x + ICON_SPACING;
        for item in &self.items {
            let size = item.current_size as usize;
            let x_offset = (ICON_SIZE as i32 - size as i32) / 2;
            let y_offset = (ICON_SIZE as i32 - size as i32) / 2;
            let draw_x = (item_x as i32 + x_offset).max(0) as usize;
            let draw_y = (dock_y as i32 + DOCK_HEIGHT as i32 - size as i32 - 8 + y_offset).max(0) as usize;
            let item_rect = DockDirtyRect {
                x: draw_x.saturating_sub(6),
                y: draw_y.saturating_sub(6),
                width: size + 12,
                height: size + 20,
            };
            if item_rect.intersects(&dirty) {
                item.draw(fb, draw_x, draw_y, size, dock_y);
            }
            item_x += ((item.current_size + ICON_SPACING as f32) / 2.0 + ICON_SIZE as f32 / 2.0) as usize;
        }
    }

    /// Belirli bir ikon indeksini kirli olarak işaretler.
    fn mark_index_dirty(&mut self, index: usize) {
        if index >= self.items.len() {
            return;
        }

        let total_width = self.items.len() * (ICON_SIZE + ICON_SPACING) + ICON_SPACING * 2;
        let dock_x = self.screen_width.saturating_sub(total_width) / 2;
        let slot_x = dock_x + ICON_SPACING + index * (ICON_SIZE + ICON_SPACING);
        let rect = DockDirtyRect {
            x: slot_x.saturating_sub((MAX_ICON_SIZE - ICON_SIZE) / 2 + 8),
            y: (self.dock_y as usize).saturating_sub(8),
            width: MAX_ICON_SIZE + 16,
            height: DOCK_HEIGHT + 16,
        };
        self.dirty_rects.add(rect, self.screen_width, self.screen_height);
    }

    /// Hover merkezinin ±1 komşusuyla birlikte kirli bölgeye ekler.
    ///
    /// Büyütme efekti ±2 slot yayıldığından yeterli kapsamı sağlar.
    fn mark_hover_band_dirty(&mut self, center: Option<usize>) {
        if let Some(c) = center {
            if c > 0 { self.mark_index_dirty(c - 1); }
            self.mark_index_dirty(c);
            if c + 1 < self.items.len() { self.mark_index_dirty(c + 1); }
        }
    }

    /// Alfa karıştırma (alpha blending) işlevi.
    ///
    /// `fg` renginin alfa kanalı kullanılarak `bg` ile karıştırır.
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

    /// Fare hareketi olayını işler.
    ///
    /// Otomatik gizleme kontrolü, hover tespiti ve büyütme bölgesi
    /// hesaplamalarını gerçekleştirir.
    pub fn on_mouse_move(&mut self, mx: i32, my: i32) -> DockEvent {
        self.mouse_x = mx;
        self.mouse_y = my;

        // Otomatik gizleme için fare yakınlık kontrolü
        if self.auto_hide {
            let dock_top = self.screen_height as i32 - DOCK_HEIGHT as i32 - 20;
            if my >= dock_top {
                self.hidden = false;
            } else if my < dock_top - 50 {
                self.hidden = true;
            }
        }

        // Üzerine gelinen öğeyi bul
        let total_width = self.items.len() * (ICON_SIZE + ICON_SPACING);
        let dock_start = self.screen_width.saturating_sub(total_width) / 2;

        let mut new_hovered = None;
        for (i, item) in self.items.iter().enumerate() {
            let item_center_x = dock_start + i * (ICON_SIZE + ICON_SPACING) + ICON_SIZE / 2;
            let item_size = item.current_size as i32;
            let half_size = item_size / 2;

            if mx >= item_center_x as i32 - half_size && mx <= item_center_x as i32 + half_size {
                new_hovered = Some(i);
                break;
            }
        }

        if new_hovered != self.hovered_index {
            let prev_hover = self.hovered_index;
            self.hovered_index = new_hovered;
            self.hover_anim.set_hover(new_hovered);
            // Önceki ve yeni hover bölgesini kirli olarak işaretle
            self.mark_hover_band_dirty(prev_hover);
            self.mark_hover_band_dirty(new_hovered);

            if let Some(idx) = new_hovered {
                return DockEvent::ItemHovered(idx, self.items[idx].name.clone());
            }
        }

        DockEvent::None
    }

    /// Fare tuşuna basma olayını işler.
    ///
    /// Üzerine gelinen öğe varsa zıplama animasyonunu başlatır.
    pub fn on_mouse_down(&mut self, mx: i32, my: i32) -> DockEvent {
        if let Some(idx) = self.hovered_index {
            self.clicked_index = Some(idx);

            // Zıplama animasyonunu başlat
            self.items[idx].start_bounce();
            self.mark_index_dirty(idx);

            return DockEvent::ItemClicked(idx, self.items[idx].id);
        }

        DockEvent::None
    }

    /// Fare tuşu bırakma olayını işler.
    ///
    /// Tıklama ve bırakma aynı öğedeyse `ItemActivated` olayı döndürür.
    pub fn on_mouse_up(&mut self) -> DockEvent {
        if let Some(clicked_idx) = self.clicked_index {
            if self.hovered_index == Some(clicked_idx) {
                let item_id = self.items[clicked_idx].id;
                let action = self.items[clicked_idx].action.clone();

                self.clicked_index = None;
                self.mark_index_dirty(clicked_idx);

                return DockEvent::ItemActivated(clicked_idx, item_id, action);
            }
        }

        self.clicked_index = None;
        DockEvent::None
    }

    /// Verilen kimlikli öğenin çalışma durumunu ayarlar.
    ///
    /// `running = true` ise zıplama animasyonu başlatılır.
    pub fn set_item_running(&mut self, id: u32, running: bool) {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.running = running;
            if running {
                item.start_bounce();
            }
        }
    }

    /// Verilen kimlikli öğeyi aktif (en önde) olarak işaretler;
    /// diğer tüm öğelerin aktifliği kaldırılır.
    pub fn set_item_active(&mut self, id: u32, active: bool) {
        // Önce tüm öğeleri pasif yap
        for item in &mut self.items {
            item.active = false;
        }

        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.active = active;
        }
    }

    /// Verilen kimlikli öğenin bildirim rozetini günceller.
    pub fn set_item_badge(&mut self, id: u32, count: u32) {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.badge_count = count;
        }
    }

    /// Verilen kimlikli öğenin ilerleme çubuğunu günceller (0.0 – 1.0).
    pub fn set_item_progress(&mut self, id: u32, progress: f32) {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.progress = progress;
        }
    }

    /// Ekran boyutu değişince dock'u yeniden boyutlandırır.
    pub fn resize(&mut self, width: usize, height: usize) {
        self.screen_width = width;
        self.screen_height = height;
    }

    /// Dock'un görünür yüksekliğini döndürür.
    ///
    /// Gizli veya görünmez ise `0` döndürür.
    pub fn height(&self) -> usize {
        if self.visible && !self.hidden {
            DOCK_HEIGHT
        } else {
            0
        }
    }

    /// Büyütme efektini etkinleştirir veya devre dışı bırakır.
    ///
    /// Devre dışı bırakıldığında tüm öğeler normal boyuta döner.
    pub fn set_magnification(&mut self, enabled: bool) {
        self.magnification = enabled;
        if !enabled {
            self.hover_anim.set_hover(None);
        }
    }

    /// Büyütme yoğunluğunu ayarlar (0.0 – 1.0).
    pub fn set_mag_intensity(&mut self, intensity: f32) {
        self.mag_intensity = intensity.max(0.0).min(1.0);
    }

    /// Ön hesaplanmış sprite ölçekleme kademelerini yapılandırır.
    ///
    /// `small_px` ≥ 24, `hovered_px` ≥ `small_px`, `max_px` ≥ `hovered_px` koşulları uygulanır.
    pub fn configure_sprite_cache(&mut self, small_px: usize, hovered_px: usize, max_px: usize) {
        let s = small_px.max(24);
        let h = hovered_px.max(s);
        let m = max_px.max(h);
        self.sprite_cache = DockSpriteCache {
            small_px: s,
            hovered_px: h,
            max_px: m,
        };
    }
}

/// Dock etkileşimlerinden üretilen olaylar
#[derive(Clone, Debug)]
pub enum DockEvent {
    /// Olay yok
    None,
    /// İkon üzerine gelindi: (indeks, isim)
    ItemHovered(usize, String),
    /// İkon tıklandı: (indeks, kimlik)
    ItemClicked(usize, u32),
    /// İkon etkinleştirildi (bırakıldı): (indeks, kimlik, eylem)
    ItemActivated(usize, u32, DockAction),
}

// ============================================================================
// GLOBAL DOCK
// ============================================================================

lazy_static::lazy_static! {
    /// Global dock örneği; `spin::Mutex` ile korunur.
    static ref DOCK: Mutex<Dock> = Mutex::new(Dock::new(1920, 1080));
}

/// Dock'u verilen ekran boyutlarıyla başlatır.
pub fn init(width: usize, height: usize) {
    let mut dock = DOCK.lock();
    dock.resize(width, height);
    crate::serial_println!("[GUI] Dock başlatıldı ({}x{})", width, height);
}

/// Global dock yöneticisine referans döndürür.
pub fn get_dock() -> &'static Mutex<Dock> {
    &DOCK
}
