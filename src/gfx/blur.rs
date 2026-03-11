//! # Blur & Shadow Effects
//!
//! Yumuşak gölgeler ve blur efektleri.

use crate::gop::framebuffer::Framebuffer;
use crate::gui::widgets::Rect;

pub fn draw_shadow(fb: &mut Framebuffer, rect: &Rect, spread: i32, opacity: u8, color: u32) {
    let fw = fb.width as i32;
    let fh = fb.height as i32;

    // Katmanlı gölge çizimi (Box Shadow simülasyonu)
    for layer in 0..spread {
        let frac = (spread - layer) as f32 / spread as f32;
        // Karesel düşüş (quadratic falloff) daha doğal görünür
        let a = (opacity as f32 * frac * frac) as u8;

        let lx = rect.x - spread + layer;
        let ly = rect.y - spread + layer;
        let lw = rect.width + (spread - layer) * 2;
        let lh = rect.height + (spread - layer) * 2;

        if lw <= 0 || lh <= 0 {
            continue;
        }

        let shadow_color = color & 0x00FFFFFF;
        draw_rect_outline(fb, lx, ly, lw, lh, shadow_color, a, fw, fh);
    }
}

fn draw_rect_outline(
    fb: &mut Framebuffer,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    color: u32,
    alpha: u8,
    fw: i32,
    fh: i32,
) {
    draw_hline(fb, x, y, w, color, alpha, fw, fh);
    draw_hline(fb, x, y + h - 1, w, color, alpha, fw, fh);
    draw_vline(fb, x, y, h, color, alpha, fw, fh);
    draw_vline(fb, x + w - 1, y, h, color, alpha, fw, fh);
}

fn draw_hline(
    fb: &mut Framebuffer,
    x: i32,
    y: i32,
    w: i32,
    color: u32,
    alpha: u8,
    fw: i32,
    fh: i32,
) {
    if y < 0 || y >= fh {
        return;
    }
    let y = y as usize;
    let x_start = x.max(0);
    let x_end = (x + w).min(fw);
    for xi in x_start..x_end {
        let bg = fb.get_pixel(xi as usize, y);
        fb.plot_pixel(xi as usize, y, blend(color, bg, alpha));
    }
}

fn draw_vline(
    fb: &mut Framebuffer,
    x: i32,
    y: i32,
    h: i32,
    color: u32,
    alpha: u8,
    fw: i32,
    fh: i32,
) {
    if x < 0 || x >= fw {
        return;
    }
    let x = x as usize;
    let y_start = y.max(0);
    let y_end = (y + h).min(fh);
    for yi in y_start..y_end {
        let bg = fb.get_pixel(x, yi as usize);
        fb.plot_pixel(x, yi as usize, blend(color, bg, alpha));
    }
}

#[inline(always)]
fn blend(src: u32, dst: u32, alpha: u8) -> u32 {
    let a = alpha as u32;
    let inv = 255 - a;
    let or = ((src >> 16 & 0xFF) * a + (dst >> 16 & 0xFF) * inv) / 255;
    let og = ((src >> 8 & 0xFF) * a + (dst >> 8 & 0xFF) * inv) / 255;
    let ob = ((src & 0xFF) * a + (dst & 0xFF) * inv) / 255;
    0xFF000000 | (or << 16) | (og << 8) | ob
}
