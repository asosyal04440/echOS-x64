//! Shared shell rendering helpers for the Hybrid Titan desktop.

use crate::gop::framebuffer::Framebuffer;
use crate::gui::icon_pack::{emit_desktop_icon_rects, DesktopIconKind};
use crate::gui::protocol::Rect;
use crate::gui::theme::{ButtonRole, Theme, ThemeMode};
use crate::gui::wallpaper;
use crate::personalization::virtual_desktops;

pub const HALO_BAR_HEIGHT: i32 = Theme::HALO_BAR_HEIGHT as i32;
pub const PULSE_DOCK_HEIGHT: i32 = Theme::PULSE_DOCK_HEIGHT as i32;

const DOCK_ICON_COUNT: usize = 5;
const DOCK_ICON_SIZE: i32 = 32;
const DOCK_ICON_GAP: i32 = 14;
const DESKTOP_SHORTCUT_ICON_SIZE: i32 = 56;
const DESKTOP_SHORTCUT_STEP_Y: i32 = 102;

const ICON_COLORS: [u32; DOCK_ICON_COUNT] =
    [0xFF26E6C6, 0xFF5AB3FF, 0xFFFFB84D, 0xFFFF6B6B, 0xFF7FE6A6];

#[derive(Clone, Copy)]
struct DesktopShortcutEntry {
    icon_rect: Rect,
    label_y: i32,
    kind: DesktopIconKind,
    label: &'static str,
    accent: u32,
}

pub fn desktop_work_area(screen: Rect) -> Rect {
    screen.inset(24, HALO_BAR_HEIGHT + 18, 24, PULSE_DOCK_HEIGHT + 30)
}

pub fn draw_desktop_scene(
    fb: &mut Framebuffer,
    screen: Rect,
    clip: Rect,
    mode: ThemeMode,
    show_dashboard: bool,
) {
    draw_wallpaper_backdrop(fb, screen, clip, mode);
    if show_dashboard {
        draw_desktop_dashboard(fb, screen, clip, mode);
    }
}

pub fn draw_wallpaper_backdrop(fb: &mut Framebuffer, screen: Rect, clip: Rect, mode: ThemeMode) {
    let active_workspace = virtual_desktops().lock().active();
    if wallpaper::draw_workspace_backdrop(fb, active_workspace, clip) {
        return;
    }

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
        let base = lerp_color(
            tokens.surfaces.desktop_top,
            tokens.surfaces.desktop_bottom,
            t as u8,
        );
        for x in clipped.x.max(0) as usize..clipped.right().max(0) as usize {
            let grain =
                ((((x as u32).wrapping_mul(13)) ^ ((y as u32).wrapping_mul(29))) & 0x07) as i16 - 3;
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

fn desktop_shortcut_entries(screen: Rect, mode: ThemeMode) -> [DesktopShortcutEntry; 5] {
    let tokens = Theme::tokens(mode);
    let work = desktop_work_area(screen).inset(18, 14, 18, 18);
    let left = work.x + 18;
    let top = work.y + 10;
    [
        DesktopShortcutEntry {
            icon_rect: Rect::new(
                left,
                top,
                DESKTOP_SHORTCUT_ICON_SIZE as u32,
                DESKTOP_SHORTCUT_ICON_SIZE as u32,
            ),
            label_y: top + DESKTOP_SHORTCUT_ICON_SIZE + 10,
            kind: DesktopIconKind::Terminal,
            label: "Terminal",
            accent: tokens.accent.primary,
        },
        DesktopShortcutEntry {
            icon_rect: Rect::new(
                left,
                top + DESKTOP_SHORTCUT_STEP_Y,
                DESKTOP_SHORTCUT_ICON_SIZE as u32,
                DESKTOP_SHORTCUT_ICON_SIZE as u32,
            ),
            label_y: top + DESKTOP_SHORTCUT_STEP_Y + DESKTOP_SHORTCUT_ICON_SIZE + 10,
            kind: DesktopIconKind::Files,
            label: "Files",
            accent: tokens.accent.secondary,
        },
        DesktopShortcutEntry {
            icon_rect: Rect::new(
                left,
                top + DESKTOP_SHORTCUT_STEP_Y * 2,
                DESKTOP_SHORTCUT_ICON_SIZE as u32,
                DESKTOP_SHORTCUT_ICON_SIZE as u32,
            ),
            label_y: top + DESKTOP_SHORTCUT_STEP_Y * 2 + DESKTOP_SHORTCUT_ICON_SIZE + 10,
            kind: DesktopIconKind::Browser,
            label: "Web",
            accent: tokens.accent.success,
        },
        DesktopShortcutEntry {
            icon_rect: Rect::new(
                left,
                top + DESKTOP_SHORTCUT_STEP_Y * 3,
                DESKTOP_SHORTCUT_ICON_SIZE as u32,
                DESKTOP_SHORTCUT_ICON_SIZE as u32,
            ),
            label_y: top + DESKTOP_SHORTCUT_STEP_Y * 3 + DESKTOP_SHORTCUT_ICON_SIZE + 10,
            kind: DesktopIconKind::Settings,
            label: "Settings",
            accent: tokens.accent.warning,
        },
        DesktopShortcutEntry {
            icon_rect: Rect::new(
                left,
                top + DESKTOP_SHORTCUT_STEP_Y * 4,
                DESKTOP_SHORTCUT_ICON_SIZE as u32,
                DESKTOP_SHORTCUT_ICON_SIZE as u32,
            ),
            label_y: top + DESKTOP_SHORTCUT_STEP_Y * 4 + DESKTOP_SHORTCUT_ICON_SIZE + 10,
            kind: DesktopIconKind::Recycle,
            label: "Recycle Bin",
            accent: 0xFF536476,
        },
    ]
}

fn draw_desktop_dashboard(fb: &mut Framebuffer, screen: Rect, clip: Rect, mode: ThemeMode) {
    let tokens = Theme::tokens(mode);
    for entry in desktop_shortcut_entries(screen, mode) {
        let shadow_rect = Rect::new(
            entry.icon_rect.x - 4,
            entry.icon_rect.y - 4,
            entry.icon_rect.width.saturating_add(8),
            entry.icon_rect.height.saturating_add(8),
        );
        fill_blended_rect(fb, shadow_rect, clip, tokens.surfaces.overlay, 0x28);
        fill_blended_rect(fb, entry.icon_rect, clip, entry.accent, 0xD8);
        draw_rect_outline_clipped(fb, entry.icon_rect, clip, tokens.borders.focus);
        draw_desktop_icon(
            fb,
            entry.kind,
            Rect::new(
                entry.icon_rect.x + 10,
                entry.icon_rect.y + 10,
                entry.icon_rect.width.saturating_sub(20),
                entry.icon_rect.height.saturating_sub(20),
            ),
            clip,
            tokens.text.on_accent,
        );
        draw_desktop_label(
            fb,
            centered_label_x(entry.icon_rect, entry.label),
            entry.label_y,
            entry.label,
            tokens.text.primary,
        );
    }
}

pub fn draw_halo_bar(fb: &mut Framebuffer, screen: Rect, clip: Rect, mode: ThemeMode, title: &str) {
    let tokens = Theme::tokens(mode);
    let rect = Rect::new(
        18,
        12,
        screen.width.saturating_sub(36),
        Theme::HALO_BAR_HEIGHT as u32,
    );
    fill_blended_rect(fb, rect, clip, tokens.surfaces.halo_bar, 0xD0);
    draw_rect_outline_clipped(fb, rect, clip, tokens.borders.subtle);

    let title_x = (rect.x + 14).max(0) as usize;
    let title_y = (rect.y + 10).max(0) as usize;
    if rect.intersects(&clip) {
        fb.draw_string(title_x, title_y, title, tokens.text.primary);
        let power_rect = Rect::new(rect.right().saturating_sub(64), rect.y + 6, 48, 28);
        let time_rect = Rect::new(power_rect.x.saturating_sub(90), rect.y + 6, 80, 28);
        let status_width = 38;
        let status_gap = 8;
        let pwr_rect = Rect::new(
            time_rect.x.saturating_sub(status_width + status_gap),
            rect.y + 6,
            status_width as u32,
            28,
        );
        let aud_rect = Rect::new(
            pwr_rect.x.saturating_sub(status_width + status_gap),
            rect.y + 6,
            status_width as u32,
            28,
        );
        let net_rect = Rect::new(
            aud_rect.x.saturating_sub(status_width + status_gap),
            rect.y + 6,
            status_width as u32,
            28,
        );
        let cmd_rect = Rect::new(
            rect.x + (rect.width as i32 / 2) - 116,
            rect.y + 6,
            232,
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
        draw_halo_status_chip(fb, net_rect, clip, "NET", tokens.accent.secondary, mode);
        draw_halo_status_chip(fb, aud_rect, clip, "AUD", tokens.accent.success, mode);
        draw_halo_status_chip(fb, pwr_rect, clip, "PWR", tokens.accent.warning, mode);
        draw_halo_status_chip(fb, time_rect, clip, "12:00", tokens.text.secondary, mode);
        draw_halo_status_chip(fb, power_rect, clip, "Lock", tokens.accent.primary, mode);
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
        let kind = match index {
            0 => DesktopIconKind::Files,
            1 => DesktopIconKind::Terminal,
            2 => DesktopIconKind::Editor,
            3 => DesktopIconKind::Settings,
            _ => DesktopIconKind::Alerts,
        };
        if icon_rect.intersects(&clip) {
            draw_desktop_icon(
                fb,
                kind,
                Rect::new(icon_rect.x + 4, icon_rect.y + 4, 24, 24),
                clip,
                if hovered {
                    0xFF09131E
                } else {
                    tokens.text.on_dark
                },
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

fn draw_desktop_icon(
    fb: &mut Framebuffer,
    kind: DesktopIconKind,
    rect: Rect,
    clip: Rect,
    color: u32,
) {
    emit_desktop_icon_rects(kind, rect, |segment| {
        fill_rect_clipped(fb, segment, clip, color);
    });
}

fn draw_dashboard_panel(
    fb: &mut Framebuffer,
    rect: Rect,
    clip: Rect,
    color: u32,
    alpha: u8,
    border: u32,
    radius_hint: i32,
) {
    let _ = radius_hint;
    fill_blended_rect(fb, rect, clip, color, alpha);
    draw_rect_outline_clipped(fb, rect, clip, border);
}

fn draw_dashboard_text(fb: &mut Framebuffer, x: i32, y: i32, text: &str, color: u32) {
    fb.draw_string(x.max(0) as usize, y.max(0) as usize, text, color);
}

fn draw_desktop_label(fb: &mut Framebuffer, x: i32, y: i32, text: &str, color: u32) {
    fb.draw_string(
        (x + 1).max(0) as usize,
        (y + 1).max(0) as usize,
        text,
        0xFF081019,
    );
    fb.draw_string(x.max(0) as usize, y.max(0) as usize, text, color);
}

fn centered_label_x(icon_rect: Rect, text: &str) -> i32 {
    let text_width = (text.len() as i32).saturating_mul(8);
    icon_rect.x + (icon_rect.width as i32 - text_width) / 2
}

fn draw_halo_status_chip(
    fb: &mut Framebuffer,
    rect: Rect,
    clip: Rect,
    label: &str,
    accent: u32,
    mode: ThemeMode,
) {
    let tokens = Theme::tokens(mode);
    let fill = if accent == tokens.text.secondary {
        tokens.surfaces.overlay
    } else {
        blend_color(tokens.surfaces.overlay, accent, 0x20)
    };
    let border = if accent == tokens.text.secondary {
        tokens.borders.subtle
    } else {
        blend_color(tokens.borders.strong, accent, 0x80)
    };
    fill_blended_rect(fb, rect, clip, fill, 0xD6);
    draw_rect_outline_clipped(fb, rect, clip, border);
    let text_width = (label.len() as i32).saturating_mul(8);
    let text_x = rect.x + ((rect.width as i32 - text_width) / 2);
    let text_y = rect.y + 7;
    fb.draw_string(
        (text_x + 1).max(0) as usize,
        (text_y + 1).max(0) as usize,
        label,
        0xFF081019,
    );
    fb.draw_string(
        text_x.max(0) as usize,
        text_y.max(0) as usize,
        label,
        tokens.text.primary,
    );
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

pub fn fill_blended_rect(fb: &mut Framebuffer, rect: Rect, clip: Rect, color: u32, alpha: u8) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gop::framebuffer::Framebuffer;
    use crate::personalization::{virtual_desktops, DesktopProfile};
    use alloc::string::String;

    fn rects_overlap(a: Rect, b: Rect) -> bool {
        a.x < b.right() && a.right() > b.x && a.y < b.bottom() && a.bottom() > b.y
    }

    #[test]
    fn desktop_shortcut_layout_fits_inside_work_area_without_overlap() {
        let screen = Rect::new(0, 0, 1600, 900);
        let work = desktop_work_area(screen);
        let entries = desktop_shortcut_entries(screen, ThemeMode::Dark);
        for entry in entries {
            let bounds = Rect::new(
                entry.icon_rect.x.saturating_sub(8),
                entry.icon_rect.y,
                132,
                entry.icon_rect.height.saturating_add(30),
            );
            assert!(bounds.x >= work.x);
            assert!(bounds.y >= work.y);
            assert!(bounds.right() <= work.right());
            assert!(bounds.bottom() <= work.bottom());
        }
        for left in 0..entries.len() {
            for right in left + 1..entries.len() {
                let a = Rect::new(
                    entries[left].icon_rect.x.saturating_sub(8),
                    entries[left].icon_rect.y,
                    132,
                    entries[left].icon_rect.height.saturating_add(30),
                );
                let b = Rect::new(
                    entries[right].icon_rect.x.saturating_sub(8),
                    entries[right].icon_rect.y,
                    132,
                    entries[right].icon_rect.height.saturating_add(30),
                );
                assert!(!rects_overlap(a, b));
            }
        }
    }

    #[test]
    fn desktop_scene_without_dashboard_matches_wallpaper_only_backdrop() {
        let screen = Rect::new(0, 0, 640, 360);
        let clip = screen;
        let mut wallpaper =
            Framebuffer::new_for_test(screen.width as usize, screen.height as usize);
        let mut scene = Framebuffer::new_for_test(screen.width as usize, screen.height as usize);

        draw_wallpaper_backdrop(&mut wallpaper, screen, clip, ThemeMode::Dark);
        draw_desktop_scene(&mut scene, screen, clip, ThemeMode::Dark, false);

        assert_eq!(wallpaper.front_buffer(), scene.front_buffer());
    }

    #[test]
    fn workspace_profiles_drive_distinct_backdrops() {
        let screen = Rect::new(0, 0, 640, 360);
        let clip = screen;
        {
            let mut desktops = virtual_desktops().lock();
            let _ = desktops.set_profile(
                0,
                DesktopProfile {
                    wallpaper_id: 11,
                    icon_pack: String::from("test-aurora"),
                },
            );
            let _ = desktops.set_profile(
                1,
                DesktopProfile {
                    wallpaper_id: 4,
                    icon_pack: String::from("test-ocean"),
                },
            );
            let _ = desktops.switch_to(0);
        }

        let mut aurora = Framebuffer::new_for_test(screen.width as usize, screen.height as usize);
        draw_wallpaper_backdrop(&mut aurora, screen, clip, ThemeMode::Dark);

        {
            let mut desktops = virtual_desktops().lock();
            let _ = desktops.switch_to(1);
        }
        let mut ocean = Framebuffer::new_for_test(screen.width as usize, screen.height as usize);
        draw_wallpaper_backdrop(&mut ocean, screen, clip, ThemeMode::Dark);

        assert_ne!(aurora.front_buffer(), ocean.front_buffer());

        let mut desktops = virtual_desktops().lock();
        let _ = desktops.switch_to(0);
    }
}
