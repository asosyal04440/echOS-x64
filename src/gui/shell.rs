//! Shared shell rendering helpers for the Hybrid Titan desktop.

use crate::gop::framebuffer::Framebuffer;
use crate::gui::protocol::Rect;
use crate::gui::theme::{ButtonRole, Theme, ThemeMode};

pub const HALO_BAR_HEIGHT: i32 = Theme::HALO_BAR_HEIGHT as i32;
pub const PULSE_DOCK_HEIGHT: i32 = Theme::PULSE_DOCK_HEIGHT as i32;

const DOCK_ICON_COUNT: usize = 5;
const DOCK_ICON_SIZE: i32 = 32;
const DOCK_ICON_GAP: i32 = 14;

const ICON_COLORS: [u32; DOCK_ICON_COUNT] = [
    0xFF26E6C6,
    0xFF5AB3FF,
    0xFFFFB84D,
    0xFFFF6B6B,
    0xFF7FE6A6,
];

pub fn desktop_work_area(screen: Rect) -> Rect {
    screen.inset(24, HALO_BAR_HEIGHT + 18, 24, PULSE_DOCK_HEIGHT + 30)
}

pub fn draw_desktop_scene(fb: &mut Framebuffer, screen: Rect, clip: Rect, mode: ThemeMode) {
    draw_wallpaper_backdrop(fb, screen, clip, mode);
    draw_halo_bar(fb, screen, clip, mode, "echOS");
    draw_pulse_dock(fb, screen, clip, mode);
}

pub fn draw_wallpaper_backdrop(fb: &mut Framebuffer, screen: Rect, clip: Rect, mode: ThemeMode) {
    let tokens = Theme::tokens(mode);
    let Some(clipped) = screen.intersection(&clip) else {
        return;
    };

    let width = screen.width.max(1) as usize;
    let height = screen.height.max(1) as usize;
    let orb_a_x = (screen.width as i32 * 22) / 100;
    let orb_a_y = (screen.height as i32 * 26) / 100;
    let orb_b_x = (screen.width as i32 * 76) / 100;
    let orb_b_y = (screen.height as i32 * 18) / 100;
    let orb_radius_a = (screen.width as i32 * 36) / 100;
    let orb_radius_b = (screen.width as i32 * 28) / 100;

    for y in clipped.y.max(0) as usize..clipped.bottom().max(0) as usize {
        let t = (y * 255) / height;
        let base = lerp_color(tokens.surfaces.desktop_top, tokens.surfaces.desktop_bottom, t as u8);
        for x in clipped.x.max(0) as usize..clipped.right().max(0) as usize {
            let grain = ((((x as u32).wrapping_mul(13)) ^ ((y as u32).wrapping_mul(29))) & 0x07) as i16 - 3;
            let mut color = Theme::shade(base, grain);
            color = blend_glow(
                color,
                x as i32,
                y as i32,
                orb_a_x,
                orb_a_y,
                orb_radius_a.max(1),
                tokens.accent.primary,
            );
            color = blend_glow(
                color,
                x as i32,
                y as i32,
                orb_b_x,
                orb_b_y,
                orb_radius_b.max(1),
                tokens.accent.secondary,
            );
            fb.plot_pixel(x, y, color);
        }
    }
}

pub fn draw_halo_bar(
    fb: &mut Framebuffer,
    screen: Rect,
    clip: Rect,
    mode: ThemeMode,
    title: &str,
) {
    let tokens = Theme::tokens(mode);
    let rect = Rect::new(18, 12, screen.width.saturating_sub(36), Theme::HALO_BAR_HEIGHT as u32);
    fill_blended_rect(fb, rect, clip, tokens.surfaces.halo_bar, 0xD0);
    draw_rect_outline_clipped(fb, rect, clip, tokens.borders.subtle);

    let title_x = (rect.x + 14).max(0) as usize;
    let title_y = (rect.y + 10).max(0) as usize;
    if rect.intersects(&clip) {
        fb.draw_string(title_x, title_y, title, tokens.text.primary);
        let cmd_rect = Rect::new(
            rect.x + (rect.width as i32 / 2) - 96,
            rect.y + 6,
            192,
            rect.height.saturating_sub(12),
        );
        fill_blended_rect(fb, cmd_rect, clip, tokens.surfaces.overlay, 0xA8);
        draw_rect_outline_clipped(fb, cmd_rect, clip, tokens.borders.subtle);
        fb.draw_string(
            (cmd_rect.x + 12).max(0) as usize,
            (cmd_rect.y + 7).max(0) as usize,
            "Search / Command",
            tokens.text.tertiary,
        );

        let status = "NET  AUD  PWR  12:00";
        let status_x = rect
            .right()
            .saturating_sub((status.len() as i32 * 8) + 18)
            .max(0) as usize;
        fb.draw_string(status_x, title_y, status, tokens.text.secondary);
    }
}

pub fn draw_pulse_dock(fb: &mut Framebuffer, screen: Rect, clip: Rect, mode: ThemeMode) {
    let tokens = Theme::tokens(mode);
    let width = (DOCK_ICON_COUNT as i32 * DOCK_ICON_SIZE)
        + ((DOCK_ICON_COUNT as i32 - 1) * DOCK_ICON_GAP)
        + 36;
    let height = 56i32;
    let rect = Rect::new(
        ((screen.width as i32 - width) / 2).max(0),
        screen.bottom().saturating_sub(PULSE_DOCK_HEIGHT + 12),
        width.max(1) as u32,
        height.max(1) as u32,
    );
    fill_blended_rect(fb, rect, clip, tokens.surfaces.dock, 0xD8);
    draw_rect_outline_clipped(fb, rect, clip, tokens.borders.subtle);

    let mut x = rect.x + 18;
    let y = rect.y + 12;
    for (index, color) in ICON_COLORS.iter().enumerate() {
        let hovered = index == 1;
        let icon_rect = Rect::new(
            x,
            if hovered { y - 3 } else { y },
            DOCK_ICON_SIZE as u32,
            DOCK_ICON_SIZE as u32,
        );
        fill_rect_clipped(
            fb,
            icon_rect,
            clip,
            if hovered {
                Theme::button_fill(ButtonRole::Primary, mode, false, true)
            } else {
                *color
            },
        );
        draw_rect_outline_clipped(fb, icon_rect, clip, tokens.borders.strong);
        let glyph = match index {
            0 => "F",
            1 => "T",
            2 => "W",
            3 => "S",
            _ => "M",
        };
        if icon_rect.intersects(&clip) {
            fb.draw_string(
                (icon_rect.x + 12).max(0) as usize,
                (icon_rect.y + 8).max(0) as usize,
                glyph,
                tokens.text.on_dark,
            );
            let dot_rect = Rect::new(icon_rect.x + 12, icon_rect.bottom() + 4, 8, 3);
            fill_rect_clipped(fb, dot_rect, clip, tokens.accent.primary);
        }
        x += DOCK_ICON_SIZE + DOCK_ICON_GAP;
    }
}

pub fn draw_emblem_wordmark(
    fb: &mut Framebuffer,
    center_x: i32,
    top_y: i32,
    mode: ThemeMode,
    draw_wordmark: bool,
) {
    let tokens = Theme::tokens(mode);
    let emblem = Rect::new(center_x - 54, top_y, 44, 44);
    fill_rect_clipped(fb, emblem, emblem, tokens.accent.primary);
    let inner = Rect::new(emblem.x + 8, emblem.y + 8, 20, 6);
    let mid = Rect::new(emblem.x + 8, emblem.y + 18, 16, 6);
    let low = Rect::new(emblem.x + 8, emblem.y + 28, 20, 6);
    let spine = Rect::new(emblem.x + 8, emblem.y + 8, 6, 26);
    fill_rect_clipped(fb, inner, inner, tokens.text.on_accent);
    fill_rect_clipped(fb, mid, mid, tokens.text.on_accent);
    fill_rect_clipped(fb, low, low, tokens.text.on_accent);
    fill_rect_clipped(fb, spine, spine, tokens.text.on_accent);

    if draw_wordmark {
        fb.draw_string(
            (center_x - 2).max(0) as usize,
            (top_y + 14).max(0) as usize,
            "echOS",
            tokens.text.primary,
        );
    }
}

fn blend_glow(base: u32, x: i32, y: i32, cx: i32, cy: i32, radius: i32, glow: u32) -> u32 {
    let dx = x.saturating_sub(cx);
    let dy = y.saturating_sub(cy);
    let dist_sq = dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy));
    let radius_sq = radius.saturating_mul(radius).max(1);
    if dist_sq >= radius_sq {
        return base;
    }

    let remaining = radius_sq.saturating_sub(dist_sq);
    let alpha = ((remaining as u64).saturating_mul(96) / radius_sq as u64) as u8;
    blend_color(base, glow, alpha)
}

fn lerp_color(a: u32, b: u32, t: u8) -> u32 {
    let t = t as u32;
    let inv = 255 - t;
    let aa = (a >> 24) & 0xFF;
    let ar = (a >> 16) & 0xFF;
    let ag = (a >> 8) & 0xFF;
    let ab = a & 0xFF;
    let ba = (b >> 24) & 0xFF;
    let br = (b >> 16) & 0xFF;
    let bg = (b >> 8) & 0xFF;
    let bb = b & 0xFF;
    let a_out = (aa * inv + ba * t) / 255;
    let r_out = (ar * inv + br * t) / 255;
    let g_out = (ag * inv + bg * t) / 255;
    let b_out = (ab * inv + bb * t) / 255;
    (a_out << 24) | (r_out << 16) | (g_out << 8) | b_out
}

pub fn blend_color(base: u32, top: u32, alpha: u8) -> u32 {
    let alpha = alpha as u32;
    let inv = 255 - alpha;
    let ba = (base >> 24) & 0xFF;
    let br = (base >> 16) & 0xFF;
    let bg = (base >> 8) & 0xFF;
    let bb = base & 0xFF;
    let ta = (top >> 24) & 0xFF;
    let tr = (top >> 16) & 0xFF;
    let tg = (top >> 8) & 0xFF;
    let tb = top & 0xFF;
    let a_out = (ba * inv + ta * alpha) / 255;
    let r_out = (br * inv + tr * alpha) / 255;
    let g_out = (bg * inv + tg * alpha) / 255;
    let b_out = (bb * inv + tb * alpha) / 255;
    (a_out << 24) | (r_out << 16) | (g_out << 8) | b_out
}

pub fn fill_rect_clipped(fb: &mut Framebuffer, rect: Rect, clip: Rect, color: u32) {
    let Some(clipped) = rect.intersection(&clip) else {
        return;
    };
    for y in clipped.y.max(0) as usize..clipped.bottom().max(0) as usize {
        for x in clipped.x.max(0) as usize..clipped.right().max(0) as usize {
            fb.plot_pixel(x, y, color);
        }
    }
}

pub fn draw_rect_outline_clipped(fb: &mut Framebuffer, rect: Rect, clip: Rect, color: u32) {
    fill_rect_clipped(fb, Rect::new(rect.x, rect.y, rect.width, 1), clip, color);
    fill_rect_clipped(
        fb,
        Rect::new(rect.x, rect.bottom().saturating_sub(1), rect.width, 1),
        clip,
        color,
    );
    fill_rect_clipped(fb, Rect::new(rect.x, rect.y, 1, rect.height), clip, color);
    fill_rect_clipped(
        fb,
        Rect::new(rect.right().saturating_sub(1), rect.y, 1, rect.height),
        clip,
        color,
    );
}

pub fn fill_blended_rect(
    fb: &mut Framebuffer,
    rect: Rect,
    clip: Rect,
    color: u32,
    alpha: u8,
) {
    let Some(clipped) = rect.intersection(&clip) else {
        return;
    };
    for y in clipped.y.max(0) as usize..clipped.bottom().max(0) as usize {
        for x in clipped.x.max(0) as usize..clipped.right().max(0) as usize {
            let base = fb.get_pixel(x, y);
            fb.plot_pixel(x, y, blend_color(base, color, alpha));
        }
    }
}
