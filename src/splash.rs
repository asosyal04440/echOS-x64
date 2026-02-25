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

static PROGRESS: AtomicU8 = AtomicU8::new(0);

pub fn set_progress(value: u8) {
    PROGRESS.store(value.min(100), Ordering::SeqCst);
}

pub fn get_progress() -> u8 {
    PROGRESS.load(Ordering::SeqCst)
}

pub struct Splash {
    bar_pos: Point,
    bar_size: Size,
    progress: u8,
}

struct FramebufferDrawTarget<'a> {
    fb: &'a mut Framebuffer,
}

impl OriginDimensions for FramebufferDrawTarget<'_> {
    fn size(&self) -> Size {
        Size::new(self.fb.width as u32, self.fb.height as u32)
    }
}

impl DrawTarget for FramebufferDrawTarget<'_> {
    type Color = Rgb888;
    type Error = Infallible;

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
            let value = ((color.r() as u32) << 16) | ((color.g() as u32) << 8) | (color.b() as u32);
            self.fb.plot_pixel(x, y, value);
        }
        Ok(())
    }

    fn clear(&mut self, color: Rgb888) -> Result<(), Self::Error> {
        let value = ((color.r() as u32) << 16) | ((color.g() as u32) << 8) | (color.b() as u32);
        self.fb.clear(value);
        Ok(())
    }
}

impl Splash {
    pub fn new(fb: &mut Framebuffer) -> Self {
        let width = fb.width as i32;
        let height = fb.height as i32;
        let width_u32 = fb.width as u32;
        let mut target = FramebufferDrawTarget { fb };
        let white = Rgb888::new(255, 255, 255);
        let bg_top = Rgb888::new(16, 16, 16);
        let bg_bottom = Rgb888::new(28, 28, 28);
        let height_u32 = height.max(1) as u32;
        for y in 0..height_u32 {
            let t = if height_u32 > 1 {
                (y * 255) / (height_u32 - 1)
            } else {
                0
            };
            let r = bg_top.r() as u32 + ((bg_bottom.r() as u32 - bg_top.r() as u32) * t) / 255;
            let g = bg_top.g() as u32 + ((bg_bottom.g() as u32 - bg_top.g() as u32) * t) / 255;
            let b = bg_top.b() as u32 + ((bg_bottom.b() as u32 - bg_top.b() as u32) * t) / 255;
            let row_color = Rgb888::new(r as u8, g as u8, b as u8);
            Rectangle::new(Point::new(0, y as i32), Size::new(width_u32, 1))
                .into_styled(PrimitiveStyle::with_fill(row_color))
                .draw(&mut target)
                .ok();
        }

        let text = "echOS";
        let font = FONT_10X20;
        let text_width = (text.len() as u32) * font.character_size.width;
        let text_height = font.character_size.height;
        let center_x = (width - text_width as i32) / 2;
        let center_y = (height - text_height as i32) / 2;
        let text_pos = Point::new(center_x, center_y);
        let bold_pos = Point::new(center_x + 1, center_y + 1);

        let text_style = MonoTextStyle::new(&font, white);
        Text::new(text, bold_pos, text_style).draw(&mut target).ok();
        Text::new(text, text_pos, text_style).draw(&mut target).ok();

        let bar_width = core::cmp::min(
            width_u32.saturating_sub(120),
            core::cmp::max(text_width, (width_u32 * 4) / 10),
        );
        let bar_height = 6u32;
        let bar_x = (width - bar_width as i32) / 2;
        let mut bar_y = center_y + text_height as i32 + 16;
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

    pub fn update_progress(&mut self, fb: &mut Framebuffer, value: u8) {
        let value = value.min(100);
        self.progress = value;
        set_progress(value);
        self.draw_progress(fb, value);
    }

    fn draw_progress(&self, fb: &mut Framebuffer, value: u8) {
        let mut target = FramebufferDrawTarget { fb };
        let white = Rgb888::new(255, 255, 255);
        let bar_bg = Rgb888::new(64, 64, 64);

        Rectangle::new(self.bar_pos, self.bar_size)
            .into_styled(PrimitiveStyle::with_fill(bar_bg))
            .draw(&mut target)
            .ok();

        let fill_width = (self.bar_size.width as u32 * value as u32) / 100;
        if fill_width > 0 {
            Rectangle::new(self.bar_pos, Size::new(fill_width, self.bar_size.height))
                .into_styled(PrimitiveStyle::with_fill(white))
                .draw(&mut target)
                .ok();
        }
    }
}
