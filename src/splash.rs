//! # echOS Açılış Ekranı (Splash Screen) Modülü
//!
//! Sistem açılışında çerçeve tamponuna (framebuffer) çizilen ilerleme çubuğu
//! ve logo içeren görsel açılış ekranını yönetir.
//!
//! ## Nasıl Çalışır?
//! ```ascii
//! Ekran
//! +-------------------------------------------+
//! |  (karanlık degrade arka plan)             |
//! |                                           |
//! |          [  echOS  ]                      |
//! |      [=====ilerleme=====>        ]        |
//! |                                           |
//! +-------------------------------------------+
//! ```
//!
//! - `FramebufferDrawTarget`: `embedded_graphics` kütüphanesi ile framebuffer'a
//!   piksel çizmek için oluşturulmuş sarmalayıcı.
//! - `Splash`: İlerleme çubuğunun konumunu ve boyutunu tutar, güncellemeyi yönetir.

use core::convert::Infallible;
use core::sync::atomic::{AtomicU8, Ordering};

use embedded_graphics::geometry::{Point, Size};
use embedded_graphics::mono_font::{ascii::FONT_10X20, MonoTextStyle};
use embedded_graphics::pixelcolor::Rgb888;
use embedded_graphics::prelude::{DrawTarget, OriginDimensions, Pixel, Primitive, RgbColor};
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;
use embedded_graphics::Drawable;

use crate::gop::framebuffer::Framebuffer;

/// Açılış ekranı ilerleme değeri (yüzde, 0-100).
///
/// Atomik olarak saklanır; birden fazla çekirdek/görev güvenle güncelleyebilir.
/// `SeqCst` sıralama en güçlü garanti; splash ekranı için yeterince nadirdir.
static PROGRESS: AtomicU8 = AtomicU8::new(0);

/// Küresel ilerleme değerini günceller (0-100 arası sıkıştırılır).
///
/// 100'den büyük değerler otomatik olarak 100'e düşürülür.
pub fn set_progress(value: u8) {
    PROGRESS.store(value.min(100), Ordering::SeqCst);
}

/// Küresel ilerleme değerini okur.
pub fn get_progress() -> u8 {
    PROGRESS.load(Ordering::SeqCst)
}

/// Açılış ekranı durumunu tutan yapı.
///
/// İlerleme çubuğunun ekrandaki konumunu, boyutunu ve mevcut yüzdesini saklar.
/// `new()` ile oluşturulurken arka plan ve metin de çizilir.
pub struct Splash {
    /// İlerleme çubuğunun sol-üst köşe koordinatı.
    bar_pos: Point,
    /// İlerleme çubuğunun genişlik ve yüksekliği.
    bar_size: Size,
    /// Şu an gösterilen ilerleme yüzdesi (0-100).
    progress: u8,
}

/// `embedded_graphics` çizim hedefi olarak `Framebuffer` sarmalayıcısı.
///
/// `DrawTarget` traitini uygulayarak `embedded_graphics` primitiflerinin
/// (dikdörtgen, metin vb.) doğrudan framebuffer'a piksel piksel çizilmesini sağlar.
struct FramebufferDrawTarget<'a> {
    fb: &'a mut Framebuffer,
}

impl OriginDimensions for FramebufferDrawTarget<'_> {
    /// Framebuffer'ın piksel cinsinden boyutunu döner.
    fn size(&self) -> Size {
        Size::new(self.fb.width as u32, self.fb.height as u32)
    }
}

impl DrawTarget for FramebufferDrawTarget<'_> {
    type Color = Rgb888;
    type Error = Infallible;

    /// Piksel listesini framebuffer'a çizer.
    ///
    /// Negatif koordinatlar veya sınır dışı koordinatlar sessizce atlanır.
    /// Renk değeri RGB 24-bit olarak `plot_pixel`'e aktarılır.
    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Rgb888>>,
    {
        for Pixel(point, color) in pixels {
            if point.x < 0 || point.y < 0 {
                continue;
            }
            let x = point.x as usize;
            let y = point.y as usize;
            if x >= self.fb.width || y >= self.fb.height {
                continue;
            }
            // RGB888 rengini 0x00RRGGBB biçimli 32-bit değere çevir
            let value = ((color.r() as u32) << 16) | ((color.g() as u32) << 8) | (color.b() as u32);
            self.fb.plot_pixel(x, y, value);
        }
        Ok(())
    }

    /// Tüm ekranı tek renk ile doldurur.
    fn clear(&mut self, color: Rgb888) -> Result<(), Self::Error> {
        let value = ((color.r() as u32) << 16) | ((color.g() as u32) << 8) | (color.b() as u32);
        self.fb.clear(value);
        Ok(())
    }
}

impl Splash {
    /// Açılış ekranını oluşturur ve framebuffer'a çizer.
    ///
    /// Adımlar:
    /// 1. Karanlıktan aydınlığa doğru dikey degrade arka plan çiz.
    /// 2. Ekranın ortasına "echOS" metnini çiz (kalın efekti için bir piksel kayık ek çizim).
    /// 3. Metnin altına ilerleme çubuğunu yerleştir.
    /// 4. Başlangıç ilerleme değerini çiz.
    pub fn new(fb: &mut Framebuffer) -> Self {
        let width = fb.width as i32;
        let height = fb.height as i32;
        let width_u32 = fb.width as u32;
        let mut target = FramebufferDrawTarget { fb };
        let white = Rgb888::new(255, 255, 255);
        let bg_top = Rgb888::new(16, 16, 16);       // Üst: çok koyu gri
        let bg_bottom = Rgb888::new(28, 28, 28);    // Alt: biraz daha açık koyu gri
        let height_u32 = height.max(1) as u32;
        // Her satır için doğrusal interpolasyon ile degrade renk hesapla
        for y in 0..height_u32 {
            let t = if height_u32 > 1 {
                (y * 255) / (height_u32 - 1) // t: 0..=255 arası normalleştirilmiş konum
            } else {
                0
            };
            // RGB her kanalı için doğrusal interpolasyon: renk = üst + (alt - üst) * t / 255
            let r = bg_top.r() as u32 + ((bg_bottom.r() as u32 - bg_top.r() as u32) * t) / 255;
            let g = bg_top.g() as u32 + ((bg_bottom.g() as u32 - bg_top.g() as u32) * t) / 255;
            let b = bg_top.b() as u32 + ((bg_bottom.b() as u32 - bg_top.b() as u32) * t) / 255;
            let row_color = Rgb888::new(r as u8, g as u8, b as u8);
            Rectangle::new(Point::new(0, y as i32), Size::new(width_u32, 1))
                .into_styled(PrimitiveStyle::with_fill(row_color))
                .draw(&mut target)
                .ok();
        }

        // Metin boyutlarını ve ekran merkezini hesapla
        let text = "echOS";
        let font = FONT_10X20;
        let text_width = (text.len() as u32) * font.character_size.width;
        let text_height = font.character_size.height;
        let center_x = (width - text_width as i32) / 2;
        let center_y = (height - text_height as i32) / 2;
        let text_pos = Point::new(center_x, center_y);
        // Bir piksel sağ-aşağı kaydırarak sahte kalın efekt (bold)
        let bold_pos = Point::new(center_x + 1, center_y + 1);

        let text_style = MonoTextStyle::new(&font, white);
        // Önce gölge/bold pozisyonuna, sonra asıl pozisyona çiz
        Text::new(text, bold_pos, text_style).draw(&mut target).ok();
        Text::new(text, text_pos, text_style).draw(&mut target).ok();

        // İlerleme çubuğu genişliği: metnin genişliği ile ekranın %40'ı arasında en büyük,
        // ancak kenarlarda 120 piksel boşluk bırakarak sınırla
        let bar_width = core::cmp::min(
            width_u32.saturating_sub(120),
            core::cmp::max(text_width, (width_u32 * 4) / 10),
        );
        let bar_height = 6u32;  // İnce ve minimal tasarım
        let bar_x = (width - bar_width as i32) / 2;
        let mut bar_y = center_y + text_height as i32 + 16;
        // Ekranın altına taşmaması için sınır kontrolü
        if bar_y + bar_height as i32 >= height {
            bar_y = height.saturating_sub(bar_height as i32 + 1);
        }
        let bar_pos = Point::new(bar_x, bar_y);
        let progress = get_progress();

        let splash = Self {
            bar_pos,
            bar_size: Size::new(bar_width, bar_height),
            progress,
        };
        splash.draw_progress(fb, progress);
        splash
    }

    /// İlerleme çubuğunu günceller ve yeniden çizer.
    ///
    /// Hem yerel `progress` alanını hem de global `PROGRESS` atomik değişkenini günceller.
    pub fn update_progress(&mut self, fb: &mut Framebuffer, value: u8) {
        let value = value.min(100);
        self.progress = value;
        set_progress(value);
        self.draw_progress(fb, value);
    }

    /// İlerleme çubuğunu framebuffer'a çizer.
    ///
    /// 1. Tüm çubuk alanını koyu gri arka planla doldur.
    /// 2. `value / 100` oranında beyaz dolgu dikdörtgeni üstüne çiz.
    fn draw_progress(&self, fb: &mut Framebuffer, value: u8) {
        let mut target = FramebufferDrawTarget { fb };
        let white = Rgb888::new(255, 255, 255);
        let bar_bg = Rgb888::new(64, 64, 64); // Koyu gri çubuk arka planı

        // Önce tüm çubuk bölgesini arka plan rengiyle sil
        Rectangle::new(self.bar_pos, self.bar_size)
            .into_styled(PrimitiveStyle::with_fill(bar_bg))
            .draw(&mut target)
            .ok();

        // Ardından yüzdeye karşılık gelen genişlikte beyaz dolgu çiz
        let fill_width = (self.bar_size.width as u32 * value as u32) / 100;
        if fill_width > 0 {
            Rectangle::new(self.bar_pos, Size::new(fill_width, self.bar_size.height))
                .into_styled(PrimitiveStyle::with_fill(white))
                .draw(&mut target)
                .ok();
        }
    }
}
