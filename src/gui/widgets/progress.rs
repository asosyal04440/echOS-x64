//! # echOS İlerleme Çubuğu ve Döndürücü Widget'ları
//!
//! Yükleme ve işlem ilerlemesini görsel olarak gösteren widget'ları içerir.
//!
//! ## İçerilen Widget'lar
//! - [`ProgressBar`]       — yatay, dolum tabanlı ilerleme çubuğu
//! - [`Spinner`]           — dönen 8 segmentli belirsiz yükleme göstergesi
//! - [`CircularProgress`]  — yüzde dolumlu dairesel ilerleme halkası
//!
//! ## no_std Notu
//! Bu modül `std` kütüphanesi olmadan çalışır; bu nedenle `sin` ve `cos`
//! matematik fonksiyonları Taylor serisiyle yaklaşık olarak hesaplanır.

use super::{Rect, Widget};
use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::Theme;
use core::sync::atomic::{AtomicU32, Ordering};

/// `no_std` ortamı için sinüs (sin) yaklaşımı.
///
/// Taylor serisi açılımı kullanılır: sin(x) = x - x³/3! + x⁵/5! - ...
/// 7 terim yeterince hassas bir sonuç verir.
/// Önce x, [0, 2π] aralığına alınır (mod işlemi).
fn sin_approx(x: f64) -> f64 {
    let x = x % (2.0 * core::f64::consts::PI);
    let mut result = 0.0;
    let mut term = x;
    for i in 1..=7 {
        result += term;
        term *= -x * x / ((2.0 * i as f64) * (2.0 * i as f64 + 1.0));
    }
    result
}

/// `no_std` ortamı için kosinüs (cos) yaklaşımı.
///
/// cos(x) = sin(x + π/2) kimliği kullanılarak `sin_approx` üzerinden türetilir.
fn cos_approx(x: f64) -> f64 {
    sin_approx(x + core::f64::consts::PI / 2.0)
}

/// Yatay ilerleme çubuğu widget'ı.
///
/// Değer ve maksimum değere göre dolum oranı hesaplanır.
/// İsteğe bağlı olarak yüzde metni ve animasyonlu gradyan gösterilebilir.
pub struct ProgressBar {
    rect: Rect,
    value: u32,
    max_value: u32,
    /// Dolum alanının üzerinde "XX%" metninin gösterilip gösterilmeyeceği
    show_percentage: bool,
    /// Animasyonlu gradyan efektinin etkin olup olmadığı
    animated: bool,
    /// Gradyan animasyonunun mevcut kaydırma ofseti (0–19 döngülü)
    animation_offset: u32,
}

impl ProgressBar {
    /// Varsayılan ayarlarla yeni bir ilerleme çubuğu oluşturur.
    /// Başlangıç değeri 0, maksimum 100, yüzde görünür, animasyon kapalıdır.
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            rect: Rect::new(x, y, width, height),
            value: 0,
            max_value: 100,
            show_percentage: true,
            animated: false,
            animation_offset: 0,
        }
    }

    /// Builder kalıbıyla başlangıç değeri ve maksimum ayarlar.
    /// `value`, `max`'ı aşarsa `max`'a sabitlenir (clamp).
    pub fn with_value(mut self, value: u32, max: u32) -> Self {
        self.value = value.min(max);
        self.max_value = max;
        self
    }

    /// Çubuğun mevcut değerini günceller.
    /// Değer `max_value`'dan büyükse otomatik kırpılır.
    pub fn set_value(&mut self, value: u32) {
        self.value = value.min(self.max_value);
    }

    /// Mevcut değeri döndürür.
    pub fn value(&self) -> u32 {
        self.value
    }

    /// Yüzde metninin görünürlüğünü ayarlar.
    pub fn set_show_percentage(&mut self, show: bool) {
        self.show_percentage = show;
    }

    /// Animasyonlu gradyan efektini açar veya kapatır.
    pub fn set_animated(&mut self, animated: bool) {
        self.animated = animated;
    }

    /// Mevcut değerin yüzdesini hesaplar: `(value * 100) / max_value`
    /// Sıfıra bölünmeyi önlemek için `max_value == 0` durumunda 0 döner.
    fn percentage(&self) -> u32 {
        if self.max_value == 0 {
            return 0;
        }
        self.value * 100 / self.max_value
    }
}

impl Widget for ProgressBar {
    /// İlerleme çubuğunu çizer.
    /// Sırasıyla: arka plan → kenarlık → dolum alanı → yüzde metni.
    fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let w = self.rect.width as usize;
        let h = self.rect.height as usize;

        // Boş arka plan dikdörtgeni
        fb.draw_rect(x, y, w, h, Theme::BUTTON_BG.to_u32());

        // Dört kenarlık çizgisi — üst/alt ve sol/sağ kenarlar
        for col in x..(x + w) {
            fb.plot_pixel(col, y, Theme::BORDER.to_u32());
            fb.plot_pixel(col, y + h - 1, Theme::BORDER.to_u32());
        }
        for row in y..(y + h) {
            fb.plot_pixel(x, row, Theme::BORDER.to_u32());
            fb.plot_pixel(x + w - 1, row, Theme::BORDER.to_u32());
        }

        // Dolum alanı — 2 piksel iç boşlukta başlar, değer oranına göre genişler
        if self.max_value > 0 {
            let fill_width = ((w - 4) as u32 * self.value / self.max_value) as usize;

            if self.animated && fill_width > 0 {
                // Animasyonlu gradyan: her sütun 0–20 döngüsünde parlaklık değiştirir
                for i in 0..fill_width {
                    let color_offset = (i as u32 + self.animation_offset) % 20;
                    let intensity = if color_offset < 10 {
                        200 + color_offset * 5
                    } else {
                        250 - (color_offset - 10) * 5
                    };
                    let color = ((intensity as u32) << 8) | Theme::ACCENT_PRIMARY.to_u32();
                    for row in (y + 2)..(y + h - 2) {
                        fb.plot_pixel(x + 2 + i, row, color);
                    }
                }
            } else {
                // Düz renk dolum — aksent rengiyle dolar
                fb.draw_rect(x + 2, y + 2, fill_width, h - 4, Theme::ACCENT_PRIMARY.to_u32());
            }
        }

        // Yüzde metni — yalnızca widget yeterince yüksekse (>=16 piksel) çizilir
        if self.show_percentage && h >= 16 {
            let pct = self.percentage();
            let pct_str = alloc::format!("{}%", pct);
            let text_x = x + (w - pct_str.len() * 8) / 2;
            let text_y = y + (h - 16) / 2;

            // %50'den fazla dolmuşsa metin rengini tersine çevir (okunabilirlik)
            let text_color = if self.percentage() > 50 {
                Theme::DESKTOP_BG.to_u32()
            } else {
                Theme::TEXT_PRIMARY.to_u32()
            };
            fb.draw_string(text_x, text_y, &pct_str, text_color);
        }
    }

    /// Animasyon ofsetini ilerletir.
    /// Her `update` çağrısında `animation_offset` 0–19 arasında döner.
    fn update(&mut self) {
        if self.animated {
            self.animation_offset = (self.animation_offset + 1) % 20;
        }
    }

    /// Widget sınırlarını döndürür.
    fn bounds(&self) -> Rect {
        self.rect
    }

    /// İlerleme çubuğu tıklanabilir değildir; her zaman false döner.
    fn on_click(&mut self, _x: i32, _y: i32) -> bool {
        false
    }
}

/// Dönen yükleme göstergesi (spinner) widget'ı.
///
/// 8 segment halinde bir daire çizer; her segment farklı opaklıkta gösterilir.
/// Baştaki segment en parlak, kuyruktaki en soluktur — bu sayede dönme hissi yaratılır.
/// Her `update` çağrısında `angle` 15° artarak tam dönüşü 24 adımda tamamlar.
pub struct Spinner {
    rect: Rect,
    /// Mevcut açısal konum (derece, 0–359)
    angle: u32,
    /// Widget'ın piksel cinsinden genişliği ve yüksekliği (kare)
    size: usize,
    /// Segmentlerin rengi (varsayılan: aksent rengi)
    color: u32,
    /// Animasyon durumu; false ise döndürücü dondurulmuş görünür
    spinning: bool,
}

impl Spinner {
    /// Verilen konumda kare boyutlu bir spinner oluşturur.
    pub fn new(x: i32, y: i32, size: usize) -> Self {
        Self {
            rect: Rect::new(x, y, size as i32, size as i32),
            angle: 0,
            size,
            color: Theme::ACCENT_PRIMARY.to_u32(),
            spinning: true,
        }
    }

    /// Döndürücüyü başlatır veya durdurur.
    pub fn set_spinning(&mut self, spinning: bool) {
        self.spinning = spinning;
    }

    /// Döndürücünün şu an dönüp dönmediğini döndürür.
    pub fn is_spinning(&self) -> bool {
        self.spinning
    }
}

impl Widget for Spinner {
    /// Spinner'ı çizer.
    /// `angle + i * 45°` formülüyle 8 segment konumu hesaplanır.
    /// Her segmentin opaklığı sabit bir tabloya göre seçilir (255 → 20).
    /// Renk, ön plan rengi ile pencere arka planı arasında lineer karıştırılır (alpha blend).
    fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let center = self.size / 2;
        let radius = (self.size / 2 - 2) as f64;

        // 8 segmenti farklı opaklıklarla çiz
        for i in 0..8 {
            let segment_angle = (self.angle as f64 + i as f64 * 45.0).to_radians();

            // Baş segment en parlak (255), kuyruk en soluk (20) — kayış (trail) efekti
            let opacity = match i {
                0 => 255,
                1 => 200,
                2 => 150,
                3 => 100,
                4 => 80,
                5 => 60,
                6 => 40,
                7 => 20,
                _ => 255,
            };

            // Her segment için ±15° yay aralığında piksel çiz
            for angle_offset in -15..=15 {
                let rad = segment_angle + (angle_offset as f64) * core::f64::consts::PI / 180.0;
                let px = (center as f64 + radius * cos_approx(rad)) as usize;
                let py = (center as f64 + radius * sin_approx(rad)) as usize;

                if px < self.size && py < self.size {
                    // Doğrusal renk karıştırma (linear blend): ön renk × opaklık + arka plan × (1 - opaklık)
                    let base_color = self.color;
                    let bg_color = Theme::WINDOW_BG.to_u32();

                    let r = ((base_color >> 16) as u32 * opacity / 255) + ((bg_color >> 16) as u32 * (255 - opacity) / 255);
                    let g = (((base_color >> 8) & 0xFF) as u32 * opacity / 255) + (((bg_color >> 8) & 0xFF) as u32 * (255 - opacity) / 255);
                    let b = ((base_color & 0xFF) as u32 * opacity / 255) + ((bg_color & 0xFF) as u32 * (255 - opacity) / 255);

                    let blended = (r << 16) | (g << 8) | b;
                    fb.plot_pixel(x + px, y + py, blended);
                }
            }
        }
    }

    /// Açıyı 15° ilerletir; 360°'ye ulaşınca sıfırlanır.
    fn update(&mut self) {
        if self.spinning {
            self.angle = (self.angle + 15) % 360;
        }
    }

    /// Widget sınırlarını döndürür.
    fn bounds(&self) -> Rect {
        self.rect
    }

    /// Spinner tıklanabilir değildir; her zaman false döner.
    fn on_click(&mut self, _x: i32, _y: i32) -> bool {
        false
    }
}

/// Dairesel (halka şeklinde) ilerleme göstergesi.
///
/// Belirsiz spinner'ın aksine bu widget belirli bir değeri gösterir.
/// Değer / maksimum oranına göre halkayı belirli bir açıya kadar doldurur.
/// `thickness` alanı halkanın piksel kalınlığını belirler.
pub struct CircularProgress {
    rect: Rect,
    value: u32,
    max_value: u32,
    /// Halka çizgisinin piksel kalınlığı
    thickness: usize,
    /// Dolum rengı
    color: u32,
}

impl CircularProgress {
    /// Verilen boyutta dairesel ilerleme widget'ı oluşturur.
    /// Varsayılan kalınlık 4 piksel, değer 0/100'dür.
    pub fn new(x: i32, y: i32, size: usize) -> Self {
        Self {
            rect: Rect::new(x, y, size as i32, size as i32),
            value: 0,
            max_value: 100,
            thickness: 4,
            color: Theme::ACCENT_PRIMARY.to_u32(),
        }
    }

    /// Builder kalıbıyla başlangıç değeri ve maksimum ayarlar.
    pub fn with_value(mut self, value: u32, max: u32) -> Self {
        self.value = value.min(max);
        self.max_value = max;
        self
    }

    /// İlerleme değerini günceller; değer kırpılır.
    pub fn set_value(&mut self, value: u32) {
        self.value = value.min(self.max_value);
    }

    /// Mevcut değeri döndürür.
    pub fn value(&self) -> u32 {
        self.value
    }
}

impl Widget for CircularProgress {
    /// Dairesel ilerleme halkasını çizer.
    /// Önce tüm halka gri arka plan rengiyle çizilir,
    /// ardından değer oranına karşılık gelen açıya kadar aksent rengiyle üzerine çizilir.
    /// Ortada widget yeterince büyükse yüzde metni gösterilir.
    fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let center = self.rect.width as usize / 2;
        let radius = (center - self.thickness / 2 - 1) as f64;

        // Arka plan halkası — tüm 360° BUTTON_BG rengiyle çizilir
        for angle in 0..360 {
            let rad = (angle as f64) * core::f64::consts::PI / 180.0;
            for t in 0..self.thickness {
                let r = radius - t as f64;
                let px = (center as f64 + r * cos_approx(rad)) as usize;
                let py = (center as f64 + r * sin_approx(rad)) as usize;
                if px < self.rect.width as usize && py < self.rect.height as usize {
                    fb.plot_pixel(x + px, y + py, Theme::BUTTON_BG.to_u32());
                }
            }
        }

        // Dolum yayı — değer oranınca açı hesaplanır: fill_angle = value * 360 / max
        if self.max_value > 0 {
            let fill_angle = (self.value * 360 / self.max_value) as i32;

            for angle in 0..fill_angle {
                let rad = (angle as f64) * core::f64::consts::PI / 180.0;
                for t in 0..self.thickness {
                    let r = radius - t as f64;
                    let px = (center as f64 + r * cos_approx(rad)) as usize;
                    let py = (center as f64 + r * sin_approx(rad)) as usize;
                    if px < self.rect.width as usize && py < self.rect.height as usize {
                        fb.plot_pixel(x + px, y + py, self.color);
                    }
                }
            }
        }

        // Orta yüzde metni — widget 40 px'den büyükse çizilir
        if self.rect.width as usize >= 40 {
            let pct = if self.max_value > 0 {
                self.value * 100 / self.max_value
            } else {
                0
            };
            let pct_str = alloc::format!("{}%", pct);
            let text_x = x + center - (pct_str.len() * 8) / 2;
            let text_y = y + center - 8;
            fb.draw_string(text_x, text_y, &pct_str, Theme::TEXT_PRIMARY.to_u32());
        }
    }

    /// Widget sınırlarını döndürür.
    fn bounds(&self) -> Rect {
        self.rect
    }

    /// Dairesel ilerleme tıklanabilir değildir; her zaman false döner.
    fn on_click(&mut self, _x: i32, _y: i32) -> bool {
        false
    }
}

/// Tüm animasyonlar için paylaşılan global zamanlayıcı sayacı.
/// `AtomicU32` kullanılması, kesme (interrupt) ortamında güvenli artış sağlar.
static ANIMATION_TICK: AtomicU32 = AtomicU32::new(0);

/// Geçerli animasyon tick değerini döndürür.
/// Birden fazla widget'ın aynı zamanlayıcıya senkronize olması için kullanılır.
pub fn animation_tick() -> u32 {
    ANIMATION_TICK.load(Ordering::Relaxed)
}

/// Animasyon sayacını bir artırır.
/// Bu fonksiyon, sistem zamanlayıcı kesmesinden (timer interrupt) çağrılmalıdır.
/// `Relaxed` sıralaması yeterlidir; çünkü yalnızca sıralı artış gereklidir.
pub fn advance_animation_tick() {
    ANIMATION_TICK.fetch_add(1, Ordering::Relaxed);
}
