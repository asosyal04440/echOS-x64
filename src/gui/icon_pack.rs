//! Shared desktop icon pack.
//!
//! Visual direction: Tabler-inspired stroke geometry adapted to echOS' rect-only
//! renderer so dock, launcher, and shell fallback surfaces share one icon language.

use crate::gui::protocol::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopIconKind {
    Terminal,
    Files,
    Browser,
    Settings,
    Editor,
    Alerts,
    Recycle,
}

pub fn emit_desktop_icon_rects<F>(kind: DesktopIconKind, rect: Rect, mut emit: F)
where
    F: FnMut(Rect),
{
    let icon = fit_square(rect.inset(2, 2, 2, 2));
    match kind {
        DesktopIconKind::Terminal => {
            emit(sub(icon, 12, 18, 76, 8));
            emit(sub(icon, 12, 74, 76, 8));
            emit(sub(icon, 12, 26, 8, 48));
            emit(sub(icon, 80, 26, 8, 48));
            emit(sub(icon, 28, 40, 8, 8));
            emit(sub(icon, 36, 48, 8, 8));
            emit(sub(icon, 28, 56, 8, 8));
            emit(sub(icon, 52, 58, 18, 6));
        }
        DesktopIconKind::Files => {
            emit(sub(icon, 18, 20, 30, 10));
            emit(sub(icon, 18, 28, 12, 46));
            emit(sub(icon, 26, 66, 50, 8));
            emit(sub(icon, 74, 36, 8, 38));
            emit(sub(icon, 26, 30, 48, 8));
            emit(sub(icon, 36, 46, 26, 6));
            emit(sub(icon, 36, 56, 20, 6));
        }
        DesktopIconKind::Browser => {
            emit(sub(icon, 18, 22, 64, 8));
            emit(sub(icon, 18, 70, 64, 8));
            emit(sub(icon, 18, 30, 8, 40));
            emit(sub(icon, 74, 30, 8, 40));
            emit(sub(icon, 46, 30, 8, 40));
            emit(sub(icon, 30, 46, 40, 8));
            emit(sub(icon, 30, 30, 8, 8));
            emit(sub(icon, 62, 30, 8, 8));
            emit(sub(icon, 30, 62, 8, 8));
            emit(sub(icon, 62, 62, 8, 8));
        }
        DesktopIconKind::Settings => {
            emit(sub(icon, 18, 24, 64, 6));
            emit(sub(icon, 18, 47, 64, 6));
            emit(sub(icon, 18, 70, 64, 6));
            emit(sub(icon, 28, 18, 12, 18));
            emit(sub(icon, 50, 41, 12, 18));
            emit(sub(icon, 36, 64, 12, 18));
        }
        DesktopIconKind::Editor => {
            emit(sub(icon, 18, 18, 8, 58));
            emit(sub(icon, 18, 18, 48, 8));
            emit(sub(icon, 58, 26, 8, 50));
            emit(sub(icon, 18, 68, 48, 8));
            emit(sub(icon, 50, 18, 8, 16));
            emit(sub(icon, 32, 34, 22, 6));
            emit(sub(icon, 32, 48, 24, 6));
            emit(sub(icon, 32, 62, 18, 6));
        }
        DesktopIconKind::Alerts => {
            emit(sub(icon, 38, 18, 20, 8));
            emit(sub(icon, 26, 28, 8, 26));
            emit(sub(icon, 58, 28, 8, 26));
            emit(sub(icon, 34, 28, 24, 8));
            emit(sub(icon, 28, 52, 36, 8));
            emit(sub(icon, 22, 60, 48, 8));
            emit(sub(icon, 42, 70, 12, 8));
        }
        DesktopIconKind::Recycle => {
            emit(sub(icon, 28, 22, 40, 8));
            emit(sub(icon, 24, 30, 48, 8));
            emit(sub(icon, 28, 38, 8, 38));
            emit(sub(icon, 60, 38, 8, 38));
            emit(sub(icon, 36, 68, 24, 8));
            emit(sub(icon, 40, 44, 4, 20));
            emit(sub(icon, 48, 44, 4, 20));
            emit(sub(icon, 56, 44, 4, 20));
            emit(sub(icon, 38, 16, 20, 6));
        }
    }
}

fn fit_square(rect: Rect) -> Rect {
    let side = rect.width.min(rect.height).max(1);
    Rect::new(
        rect.x + (rect.width as i32 - side as i32) / 2,
        rect.y + (rect.height as i32 - side as i32) / 2,
        side,
        side,
    )
}

fn sub(rect: Rect, x: u32, y: u32, width: u32, height: u32) -> Rect {
    Rect::new(
        rect.x + scale_axis(rect.width, x),
        rect.y + scale_axis(rect.height, y),
        scale_dim(rect.width, width),
        scale_dim(rect.height, height),
    )
}

fn scale_axis(size: u32, percent: u32) -> i32 {
    ((size.saturating_mul(percent)).saturating_add(50) / 100) as i32
}

fn scale_dim(size: u32, percent: u32) -> u32 {
    ((size.saturating_mul(percent)).saturating_add(50) / 100).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[test]
    fn every_desktop_icon_emits_real_geometry() {
        for kind in [
            DesktopIconKind::Terminal,
            DesktopIconKind::Files,
            DesktopIconKind::Browser,
            DesktopIconKind::Settings,
            DesktopIconKind::Editor,
            DesktopIconKind::Alerts,
            DesktopIconKind::Recycle,
        ] {
            let mut rects = Vec::new();
            emit_desktop_icon_rects(kind, Rect::new(0, 0, 32, 32), |rect| rects.push(rect));
            assert!(!rects.is_empty());
            assert!(rects.iter().all(|rect| rect.width > 0 && rect.height > 0));
        }
    }
}
