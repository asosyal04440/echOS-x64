//! Hybrid Titan boot splash for echOS.

use core::sync::atomic::{AtomicU8, Ordering};

use crate::gop::framebuffer::Framebuffer;
use crate::gui::protocol::Rect;
use crate::gui::shell;
use crate::gui::theme::{Theme, ThemeMode};

static PROGRESS: AtomicU8 = AtomicU8::new(0);

pub fn set_progress(value: u8) {
    PROGRESS.store(value.min(100), Ordering::SeqCst);
}

pub fn get_progress() -> u8 {
    PROGRESS.load(Ordering::SeqCst)
}

pub struct Splash {
    beam_rect: Rect,
    status_pos: (usize, usize),
    screen_rect: Rect,
}

impl Splash {
    pub fn new(fb: &mut Framebuffer) -> Self {
        let screen_rect = Rect::new(0, 0, fb.width as u32, fb.height as u32);
        let beam_width = (fb.width as u32 / 3).max(240);
        let beam_rect = Rect::new(
            ((fb.width as i32 - beam_width as i32) / 2).max(32),
            (fb.height as i32 / 2) + 30,
            beam_width,
            6,
        );
        let status_pos = (
            ((fb.width / 2).saturating_sub(116)).max(8),
            ((beam_rect.y + 24).max(0)) as usize,
        );

        let splash = Self {
            beam_rect,
            status_pos,
            screen_rect,
        };
        splash.draw_progress(fb, get_progress());
        splash
    }

    pub fn update_progress(&mut self, fb: &mut Framebuffer, value: u8) {
        let value = value.min(100);
        set_progress(value);
        self.draw_progress(fb, value);
    }

    fn draw_progress(&self, fb: &mut Framebuffer, value: u8) {
        shell::draw_wallpaper_backdrop(fb, self.screen_rect, self.screen_rect, ThemeMode::Dark);

        let panel = Rect::new(
            self.beam_rect.x - 44,
            self.beam_rect.y - 84,
            self.beam_rect.width + 88,
            152,
        );
        shell::fill_blended_rect(
            fb,
            panel,
            self.screen_rect,
            Theme::tokens(ThemeMode::Dark).surfaces.overlay,
            0xA8,
        );
        shell::draw_rect_outline_clipped(
            fb,
            panel,
            self.screen_rect,
            Theme::tokens(ThemeMode::Dark).borders.subtle,
        );

        shell::draw_emblem_wordmark(fb, fb.width as i32 / 2, panel.y + 18, ThemeMode::Dark, true);

        let beam_track = Rect::new(
            self.beam_rect.x,
            self.beam_rect.y,
            self.beam_rect.width,
            self.beam_rect.height,
        );
        shell::fill_blended_rect(
            fb,
            beam_track,
            self.screen_rect,
            Theme::tokens(ThemeMode::Dark).surfaces.field,
            0xFF,
        );

        let fill_width = ((self.beam_rect.width as u64 * value as u64) / 100) as u32;
        let beam_fill = Rect::new(
            self.beam_rect.x,
            self.beam_rect.y,
            fill_width,
            self.beam_rect.height,
        );
        shell::fill_rect_clipped(
            fb,
            beam_fill,
            self.screen_rect,
            Theme::tokens(ThemeMode::Dark).accent.primary,
        );
        if fill_width > 0 {
            let pulse = Rect::new(
                self.beam_rect.x + fill_width as i32 - 4,
                self.beam_rect.y - 2,
                8,
                self.beam_rect.height + 4,
            );
            shell::fill_blended_rect(
                fb,
                pulse,
                self.screen_rect,
                Theme::tokens(ThemeMode::Dark).accent.secondary,
                0x90,
            );
        }

        let phase = if value < 20 {
            "Mapping framebuffer"
        } else if value < 45 {
            "Sealing memory layout"
        } else if value < 70 {
            "Bringing services online"
        } else if value < 95 {
            "Starting shell runtime"
        } else {
            "Finalizing session"
        };
        let status = alloc::format!("{}   {:>3}%", phase, value);
        fb.draw_string(
            self.status_pos.0,
            self.status_pos.1,
            &status,
            Theme::tokens(ThemeMode::Dark).text.secondary,
        );
    }
}
