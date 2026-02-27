//! # echOS Cyber Compositor
//!
//! Katmanlı, alpha-blended rendering engine.
//!
//! Render pipeline (her kare):
//!   1. Wallpaper katmanı
//!   2. Gölge katmanları (pencere başına)
//!   3. Pencere içeriği (alpha-blend, cam efekti)
//!   4. Pencere dekorasyonları (başlık çubuğu, butonlar)
//!   5. CyberPanel (üst bar)
//!   6. Snap overlay (sürükleme sırasında)
//!   7. Mouse cursor (en üst)
//!
//! Input pipeline (her kare, render öncesi):
//!   1. Global kısayol filtresi → ShortcutId
//!   2. WM katmanı (drag, resize, snap)
//!   3. Desktop katmanı (panel, dock, spotlight vs.)
//!   4. App katmanı (aktif pencere widget'larına ilet)

use crate::drivers::mouse;
use crate::gop::framebuffer::Framebuffer;
use crate::gui::cyber_panel::{CyberPanel, PANEL_HEIGHT};
use crate::gui::desktop::Desktop;
use crate::gui::echos_wm::{
    CyberTheme, ModifierState, ShortcutId, SnapOverlay, SnapTarget, WinState,
    WindowFrame, WindowId, hyper_advance_q8, hyper_ease_out_q8, lerp_rect_q8, t_to_q8, q8_to_t,
};
use crate::gui::global_command_bar::{CommandAction, GlobalCommandBar};
use crate::gui::widgets::Rect;
use alloc::vec::Vec;

// ============================================================
// PENCERE DEKORASYONU SABİTLERİ
// ============================================================

/// Başlık çubuğu yüksekliği (piksel).
pub const TITLEBAR_H: i32 = 28;
/// Kontrol butonu yarıçapı.
const BTN_RADIUS: i32 = 7;
/// Kontrol butonları arası boşluk.
const BTN_GAP: i32 = 10;
/// İlk buton sol ofseti.
const BTN_LEFT_PAD: i32 = 12;
/// Animasyon hızı (t/saniye).
const ANIM_SPEED: f32 = 8.0;
/// Gölge yayılma mesafesi.
const SHADOW_SPREAD: i32 = 10;
/// Performans odaklı hızlı render yolu.
const PERF_FAST_RENDER: bool = true;

// ============================================================
// WM DURUMU
// ============================================================

struct DragState {
    window_id: WindowId,
    offset_x: i32,
    offset_y: i32,
}

struct ResizeState {
    window_id: WindowId,
    edge: ResizeEdge,
    start_rect: Rect,
    start_mx: i32,
    start_my: i32,
}

#[derive(Clone, Copy, PartialEq)]
enum ResizeEdge {
    None,
    Top, Bottom, Left, Right,
    TopLeft, TopRight, BottomLeft, BottomRight,
}

struct CompState {
    frames: Vec<WindowFrame>,
    drag: Option<DragState>,
    resize: Option<ResizeState>,
    snap_overlay: SnapOverlay,
    focused: Option<WindowId>,
    mod_state: ModifierState,
    screen_w: i32,
    screen_h: i32,
    next_id: u32,
    dock_rect: Rect,
    /// Desktop app_window index → WindowId eşlemesi (Faz 2 köprüsü).
    /// `(WindowId, desktop_app_idx)` çiftleri.
    desktop_map: Vec<(WindowId, usize)>,
    /// WM kapatma kuyruğu — compositor main loop'ta Desktop'a yönlendirilir.
    close_queue: Vec<WindowId>,
    minimize_queue: Vec<WindowId>,
}

impl CompState {
    fn new(screen_w: i32, screen_h: i32) -> Self {
        let snap_overlay = SnapOverlay::new(screen_w, screen_h, PANEL_HEIGHT);
        let dock_rect = Rect::new(screen_w / 2 - 35, screen_h - 80, 70, 70);
        Self {
            frames: Vec::new(),
            drag: None,
            resize: None,
            snap_overlay,
            focused: None,
            mod_state: ModifierState::default(),
            screen_w,
            screen_h,
            next_id: 1,
            dock_rect,
            desktop_map: Vec::new(),
            close_queue: Vec::new(),
            minimize_queue: Vec::new(),
        }
    }

    fn alloc_id(&mut self) -> WindowId {
        let id = WindowId(self.next_id);
        self.next_id += 1;
        id
    }

    /// Desktop app_window için WM çerçevesi oluştur ve kayıt et.
    fn register_desktop_window(&mut self, title: &str, rect: Rect, app_idx: usize) -> WindowId {
        use crate::gui::echos_wm::DecoFlags;
        use alloc::string::ToString;
        let id = self.alloc_id();
        let frame = WindowFrame {
            id,
            title: title.to_string(),
            rect,
            normal_rect: rect,
            state: WinState::Opening(0.0),
            z_order: app_idx as u32,
            focused: false,
            opacity: 1.0,
            blur_radius: 12,
            shadow_spread: SHADOW_SPREAD as u8,
            decorations: DecoFlags::default(),
            pid: None,
        };
        self.frames.push(frame);
        self.desktop_map.push((id, app_idx));
        id
    }

    /// WindowId'ye karşılık gelen Desktop app_idx.
    fn desktop_idx_of(&self, id: WindowId) -> Option<usize> {
        self.desktop_map.iter().find(|(wid, _)| *wid == id).map(|(_, idx)| *idx)
    }

    /// WindowId'ye karşılık gelen desktop_map tablosundaki mapping'i güncelle.
    fn update_desktop_map_rect(&self, _id: WindowId, _rect: Rect) {
        // Rect güncellemesi doğrudan frames içinde; Desktop'a set_app_rect ile bildirilir.
    }

    fn get_frame(&self, id: WindowId) -> Option<&WindowFrame> {
        self.frames.iter().find(|f| f.id == id)
    }

    fn get_frame_mut(&mut self, id: WindowId) -> Option<&mut WindowFrame> {
        self.frames.iter_mut().find(|f| f.id == id)
    }

    fn bring_to_front(&mut self, id: WindowId) {
        let max_z = self.frames.iter().map(|f| f.z_order).max().unwrap_or(0);
        if let Some(f) = self.get_frame_mut(id) {
            f.z_order = max_z + 1;
        }
    }

    fn titlebar_hit(&self, mx: i32, my: i32) -> Option<WindowId> {
        let mut sorted: Vec<&WindowFrame> = self.frames.iter()
            .filter(|f| f.state.is_visible())
            .collect();
        sorted.sort_by(|a, b| b.z_order.cmp(&a.z_order));
        for f in sorted {
            let tb = Rect::new(f.rect.x, f.rect.y, f.rect.width, TITLEBAR_H);
            if tb.contains(mx, my) {
                if !btn_close_rect(f).contains(mx, my)
                    && !btn_min_rect(f).contains(mx, my)
                    && !btn_max_rect(f).contains(mx, my)
                {
                    return Some(f.id);
                }
            }
        }
        None
    }

    fn window_hit(&self, mx: i32, my: i32) -> Option<WindowId> {
        let mut sorted: Vec<&WindowFrame> = self.frames.iter()
            .filter(|f| f.state.is_visible())
            .collect();
        sorted.sort_by(|a, b| b.z_order.cmp(&a.z_order));
        for f in sorted {
            if f.rect.contains(mx, my) {
                return Some(f.id);
            }
        }
        None
    }

    fn resize_edge_at(&self, id: WindowId, mx: i32, my: i32) -> ResizeEdge {
        let f = match self.get_frame(id) { Some(x) => x, None => return ResizeEdge::None };
        if !f.decorations.resizable { return ResizeEdge::None; }
        let margin = 6i32;
        let r = &f.rect;
        let on_left   = mx >= r.x && mx <= r.x + margin;
        let on_right  = mx >= r.x + r.width - margin && mx <= r.x + r.width;
        let on_top    = my >= r.y && my <= r.y + margin;
        let on_bottom = my >= r.y + r.height - margin && my <= r.y + r.height;
        match (on_top, on_bottom, on_left, on_right) {
            (true,  _,    true,  _    ) => ResizeEdge::TopLeft,
            (true,  _,    _,     true ) => ResizeEdge::TopRight,
            (_,     true, true,  _    ) => ResizeEdge::BottomLeft,
            (_,     true, _,     true ) => ResizeEdge::BottomRight,
            (true,  _,    _,     _    ) => ResizeEdge::Top,
            (_,     true, _,     _    ) => ResizeEdge::Bottom,
            (_,     _,    true,  _    ) => ResizeEdge::Left,
            (_,     _,    _,     true ) => ResizeEdge::Right,
            _ => ResizeEdge::None,
        }
    }

    fn on_mouse_down(&mut self, mx: i32, my: i32) {
        if my < PANEL_HEIGHT { return; }
        if let Some(id) = self.window_hit(mx, my) {
            self.focused = Some(id);
            self.bring_to_front(id);
            let edge = self.resize_edge_at(id, mx, my);
            if edge != ResizeEdge::None {
                let r = self.get_frame(id).map(|f| f.rect).unwrap_or_default();
                self.resize = Some(ResizeState { window_id: id, edge, start_rect: r, start_mx: mx, start_my: my });
            } else if let Some(f) = self.get_frame(id) {
                // Borderless drag grip: pencerenin üst 10px bandı
                if my <= f.rect.y + 10 {
                    let r = f.rect;
                    self.drag = Some(DragState { window_id: id, offset_x: mx - r.x, offset_y: my - r.y });
                }
            }
        }
    }

    fn on_mouse_drag(&mut self, mx: i32, my: i32) {
        if self.drag.is_some() {
            let (new_x, new_y, id) = {
                let d = self.drag.as_ref().unwrap();
                (mx - d.offset_x, (my - d.offset_y).max(PANEL_HEIGHT), d.window_id)
            };
            if let Some(f) = self.get_frame_mut(id) {
                if matches!(f.state, WinState::Snapped(_)) { f.state = WinState::Normal; }
                f.rect.x = new_x;
                f.rect.y = new_y;
            }
            self.snap_overlay.update(mx, my, 1.0 / 60.0);
        }
        if self.resize.is_some() {
            let (dx, dy, sr, id, edge) = {
                let rs = self.resize.as_ref().unwrap();
                (mx - rs.start_mx, my - rs.start_my, rs.start_rect, rs.window_id, rs.edge)
            };
            let min_w = 200i32;
            let min_h = 100i32;
            if let Some(f) = self.get_frame_mut(id) {
                match edge {
                    ResizeEdge::Right  => { f.rect.width  = (sr.width  + dx).max(min_w); }
                    ResizeEdge::Bottom => { f.rect.height = (sr.height + dy).max(min_h); }
                    ResizeEdge::Left   => {
                        let nw = (sr.width - dx).max(min_w);
                        f.rect.x = sr.x + sr.width - nw; f.rect.width = nw;
                    }
                    ResizeEdge::Top    => {
                        let nh = (sr.height - dy).max(min_h);
                        f.rect.y = (sr.y + sr.height - nh).max(PANEL_HEIGHT); f.rect.height = nh;
                    }
                    ResizeEdge::TopLeft => {
                        let nw = (sr.width - dx).max(min_w);
                        let nh = (sr.height - dy).max(min_h);
                        f.rect.x = sr.x + sr.width - nw;
                        f.rect.y = (sr.y + sr.height - nh).max(PANEL_HEIGHT);
                        f.rect.width = nw; f.rect.height = nh;
                    }
                    ResizeEdge::TopRight => {
                        let nw = (sr.width + dx).max(min_w);
                        let nh = (sr.height - dy).max(min_h);
                        f.rect.y = (sr.y + sr.height - nh).max(PANEL_HEIGHT);
                        f.rect.width = nw; f.rect.height = nh;
                    }
                    ResizeEdge::BottomLeft => {
                        let nw = (sr.width - dx).max(min_w);
                        f.rect.x = sr.x + sr.width - nw;
                        f.rect.width = nw; f.rect.height = (sr.height + dy).max(min_h);
                    }
                    ResizeEdge::BottomRight => {
                        f.rect.width  = (sr.width  + dx).max(min_w);
                        f.rect.height = (sr.height + dy).max(min_h);
                    }
                    ResizeEdge::None => {}
                }
            }
        }
    }

    fn on_mouse_up(&mut self, mx: i32, my: i32) {
        if self.drag.is_some() {
            if let Some(snap_target) = self.snap_overlay.hovered_target() {
                let id = self.drag.as_ref().unwrap().window_id;
                let target_rect = snap_target.compute_rect(self.screen_w, self.screen_h, PANEL_HEIGHT);
                if let Some(f) = self.get_frame_mut(id) {
                    f.normal_rect = f.rect;
                    f.state = WinState::Snapping { target: snap_target, t: 0.0 };
                    f.rect = target_rect;
                }
            }
            for z in &mut self.snap_overlay.zones {
                z.hover = false;
                z.hover_alpha = 0.0;
            }
        }
        self.drag = None;
        self.resize = None;
        let _ = (mx, my);
    }

    fn apply_snap(&mut self, target: SnapTarget) {
        if let Some(id) = self.focused {
            let target_rect = target.compute_rect(self.screen_w, self.screen_h, PANEL_HEIGHT);
            if let Some(f) = self.get_frame_mut(id) {
                if f.state != WinState::Normal && !matches!(f.state, WinState::Snapped(_)) { return; }
                f.normal_rect = f.rect;
                f.state = WinState::Snapping { target, t: 0.0 };
                f.rect = target_rect;
            }
        }
    }

    fn close_focused(&mut self) {
        if let Some(id) = self.focused {
            if let Some(f) = self.get_frame_mut(id) {
                f.state = WinState::Closing(0.0);
            }
            self.close_queue.push(id);
        }
    }

    fn minimize_focused(&mut self) {
        if let Some(id) = self.focused {
            if let Some(f) = self.get_frame_mut(id) {
                f.state = WinState::Minimizing(0.0);
            }
            self.minimize_queue.push(id);
        }
    }

    fn maximize_toggle_focused(&mut self) {
        if let Some(id) = self.focused {
            let sw = self.screen_w; let sh = self.screen_h;
            if let Some(f) = self.get_frame_mut(id) {
                match f.state {
                    WinState::Maximized => {
                        f.rect = f.normal_rect;
                        f.state = WinState::Restoring(0.0);
                    }
                    WinState::Normal | WinState::Snapped(_) => {
                        f.normal_rect = f.rect;
                        f.state = WinState::Maximizing(0.0);
                        f.rect = Rect::new(0, PANEL_HEIGHT, sw, sh - PANEL_HEIGHT);
                    }
                    _ => {}
                }
            }
        }
    }

    fn cycle_windows(&mut self) {
        if self.frames.is_empty() { return; }
        let visible: Vec<WindowId> = {
            let mut v: Vec<&WindowFrame> = self.frames.iter()
                .filter(|f| f.state.is_visible())
                .collect();
            v.sort_by(|a, b| b.z_order.cmp(&a.z_order));
            v.iter().map(|f| f.id).collect()
        };
        if visible.len() < 2 { return; }
        let next_id = if let Some(cur) = self.focused {
            let pos = visible.iter().position(|&x| x == cur).unwrap_or(0);
            visible[(pos + 1) % visible.len()]
        } else { visible[0] };
        self.focused = Some(next_id);
        self.bring_to_front(next_id);
    }

    fn update_animations(&mut self, dt: f32) {
        let _ = dt;
        let to_remove: Vec<WindowId> = self.frames.iter()
            .filter(|f| matches!(f.state, WinState::Closing(t) if t >= 1.0))
            .map(|f| f.id)
            .collect();
        for id in to_remove { self.frames.retain(|f| f.id != id); }

        for f in &mut self.frames {
            match f.state {
                WinState::Opening(ref mut t) => {
                    let q = hyper_advance_q8(t_to_q8(*t));
                    *t = q8_to_t(q);
                    if *t >= 1.0 { f.state = WinState::Normal; }
                }
                WinState::Closing(ref mut t) => {
                    let q = hyper_advance_q8(t_to_q8(*t));
                    *t = q8_to_t(q);
                }
                WinState::Minimizing(ref mut t) => {
                    let q = hyper_advance_q8(t_to_q8(*t));
                    *t = q8_to_t(q);
                    if *t >= 1.0 { f.state = WinState::Minimized; }
                }
                WinState::Restoring(ref mut t) => {
                    let q = hyper_advance_q8(t_to_q8(*t));
                    *t = q8_to_t(q);
                    if *t >= 1.0 { f.state = WinState::Normal; }
                }
                WinState::Snapping { target, ref mut t } => {
                    let q = hyper_advance_q8(t_to_q8(*t));
                    *t = q8_to_t(q);
                    if *t >= 1.0 { f.state = WinState::Snapped(target); }
                }
                WinState::Maximizing(ref mut t) => {
                    let q = hyper_advance_q8(t_to_q8(*t));
                    *t = q8_to_t(q);
                    if *t >= 1.0 { f.state = WinState::Maximized; }
                }
                _ => {}
            }
        }
    }
}

// ============================================================
// PENCERE BUTON RECT'LERİ
// ============================================================

fn btn_close_rect(f: &WindowFrame) -> Rect {
    Rect::new(f.rect.x + BTN_LEFT_PAD, f.rect.y + (TITLEBAR_H - BTN_RADIUS * 2) / 2,
              BTN_RADIUS * 2, BTN_RADIUS * 2)
}
fn btn_min_rect(f: &WindowFrame) -> Rect {
    Rect::new(f.rect.x + BTN_LEFT_PAD + (BTN_RADIUS * 2 + BTN_GAP),
              f.rect.y + (TITLEBAR_H - BTN_RADIUS * 2) / 2,
              BTN_RADIUS * 2, BTN_RADIUS * 2)
}
fn btn_max_rect(f: &WindowFrame) -> Rect {
    Rect::new(f.rect.x + BTN_LEFT_PAD + 2 * (BTN_RADIUS * 2 + BTN_GAP),
              f.rect.y + (TITLEBAR_H - BTN_RADIUS * 2) / 2,
              BTN_RADIUS * 2, BTN_RADIUS * 2)
}

// ============================================================
// RENDERING YARDIMCIları
// ============================================================

fn draw_shadow(fb: &mut Framebuffer, rect: &Rect, spread: i32, opacity: u8) {
    let fw = fb.width as i32; let fh = fb.height as i32;
    for layer in 0..spread {
        let frac = (spread - layer) as f32 / spread as f32;
        let a = (opacity as f32 * frac * frac) as u8;
        let lx = rect.x - spread + layer;
        let ly = rect.y - spread + layer;
        let lw = rect.width  + (spread - layer) * 2;
        let lh = rect.height + (spread - layer) * 2;
        if lw <= 0 || lh <= 0 { continue; }
        // Top/bottom lines
        draw_hline_alpha(fb, lx, ly,          lw, 0, a, fw, fh);
        draw_hline_alpha(fb, lx, ly + lh - 1, lw, 0, a, fw, fh);
        // Left/right lines
        draw_vline_alpha(fb, lx,          ly, lh, 0, a, fw, fh);
        draw_vline_alpha(fb, lx + lw - 1, ly, lh, 0, a, fw, fh);
    }
}

fn draw_hline_alpha(fb: &mut Framebuffer, x: i32, y: i32, w: i32, color: u32, alpha: u8, fw: i32, fh: i32) {
    if y < 0 || y >= fh { return; }
    for xi in x.max(0)..(x + w).min(fw) {
        let bg = fb.get_pixel(xi as usize, y as usize);
        fb.plot_pixel(xi as usize, y as usize, alpha_blend(color, bg, alpha));
    }
}

fn draw_vline_alpha(fb: &mut Framebuffer, x: i32, y: i32, h: i32, color: u32, alpha: u8, fw: i32, fh: i32) {
    if x < 0 || x >= fw { return; }
    for yi in y.max(0)..(y + h).min(fh) {
        let bg = fb.get_pixel(x as usize, yi as usize);
        fb.plot_pixel(x as usize, yi as usize, alpha_blend(color, bg, alpha));
    }
}

#[inline(always)]
fn alpha_blend(src: u32, dst: u32, alpha: u8) -> u32 {
    let a = alpha as u32;
    let inv = 255 - a;
    let or = ((src >> 16 & 0xFF) * a + (dst >> 16 & 0xFF) * inv) / 255;
    let og = ((src >>  8 & 0xFF) * a + (dst >>  8 & 0xFF) * inv) / 255;
    let ob = ((src       & 0xFF) * a + (dst       & 0xFF) * inv) / 255;
    0xFF000000 | (or << 16) | (og << 8) | ob
}

/// 3×3 box-blur + renk tint → frosted glass.
fn draw_frosted_glass(fb: &mut Framebuffer, rect: &Rect, tint: u32, tint_alpha: u8) {
    if PERF_FAST_RENDER {
        let x0 = rect.x.max(0) as usize;
        let y0 = rect.y.max(0) as usize;
        let x1 = (rect.x + rect.width).min(fb.width as i32).max(0) as usize;
        let y1 = (rect.y + rect.height).min(fb.height as i32).max(0) as usize;
        let alpha = tint_alpha.min(120);
        for y in y0..y1 {
            for x in x0..x1 {
                let bg = fb.get_pixel(x, y);
                fb.plot_pixel(x, y, alpha_blend(tint, bg, alpha));
            }
        }
        return;
    }

    let x0 = rect.x.max(0) as usize;
    let y0 = rect.y.max(0) as usize;
    let x1 = (rect.x + rect.width) .min(fb.width  as i32).max(0) as usize;
    let y1 = (rect.y + rect.height).min(fb.height as i32).max(0) as usize;
    let fw = fb.width as i32;
    let fh = fb.height as i32;
    for y in y0..y1 {
        for x in x0..x1 {
            let mut r = 0u32; let mut g = 0u32; let mut b = 0u32; let mut n = 0u32;
            for dy in 0..3i32 {
                for dx in 0..3i32 {
                    let bx = x as i32 + dx - 1;
                    let by = y as i32 + dy - 1;
                    if bx >= 0 && bx < fw && by >= 0 && by < fh {
                        let px = fb.get_pixel(bx as usize, by as usize);
                        r += (px >> 16) & 0xFF; g += (px >> 8) & 0xFF; b += px & 0xFF; n += 1;
                    }
                }
            }
            if n == 0 { continue; }
            let blurred = 0xFF000000 | ((r/n) << 16) | ((g/n) << 8) | (b/n);
            fb.plot_pixel(x, y, alpha_blend(tint, blurred, tint_alpha));
        }
    }
}

fn draw_titlebar(fb: &mut Framebuffer, frame: &WindowFrame, mx: i32, my: i32) {
    let r = &frame.rect;
    draw_frosted_glass(fb, &Rect::new(r.x, r.y, r.width, TITLEBAR_H), 0x0E1117, 140);
    // Bottom border line
    let border_c = if frame.focused { CyberTheme::BORDER_ACTIVE } else { CyberTheme::BORDER };
    let fw = fb.width as i32;
    let border_y = r.y + TITLEBAR_H - 1;
    if border_y >= 0 && border_y < fb.height as i32 {
        draw_hline_alpha(fb, r.x, border_y, r.width, border_c, 255, fw, fb.height as i32);
    }
    // Kontrol butonları
    if frame.decorations.has_close {
        let br = btn_close_rect(frame);
        let c = if br.contains(mx, my) { CyberTheme::BTN_HOVER_CLOSE } else { CyberTheme::BTN_CLOSE };
        fill_circle(fb, br.x + BTN_RADIUS, br.y + BTN_RADIUS, BTN_RADIUS, c);
    }
    if frame.decorations.has_minimize {
        let br = btn_min_rect(frame);
        let c = if br.contains(mx, my) { CyberTheme::BTN_HOVER_MIN } else { CyberTheme::BTN_MIN };
        fill_circle(fb, br.x + BTN_RADIUS, br.y + BTN_RADIUS, BTN_RADIUS, c);
    }
    if frame.decorations.has_maximize {
        let br = btn_max_rect(frame);
        let c = if br.contains(mx, my) { CyberTheme::BTN_HOVER_MAX } else { CyberTheme::BTN_MAX };
        fill_circle(fb, br.x + BTN_RADIUS, br.y + BTN_RADIUS, BTN_RADIUS, c);
    }
    // Başlık metni
    let tx = r.x + BTN_LEFT_PAD + 3 * (BTN_RADIUS * 2 + BTN_GAP) + 8;
    let ty = r.y + (TITLEBAR_H - 8) / 2;
    let tc = if frame.focused { CyberTheme::TEXT_PRIMARY } else { CyberTheme::TEXT_SECONDARY };
    draw_text_fb(fb, tx as usize, ty as usize, &frame.title, tc);
}

fn fill_circle(fb: &mut Framebuffer, cx: i32, cy: i32, r: i32, color: u32) {
    let fw = fb.width as i32; let fh = fb.height as i32;
    for dy in -r..=r {
        for dx in -r..=r {
            if dx*dx + dy*dy <= r*r {
                let x = cx + dx; let y = cy + dy;
                if x >= 0 && y >= 0 && x < fw && y < fh {
                    fb.plot_pixel(x as usize, y as usize, color);
                }
            }
        }
    }
}

fn draw_window_border(fb: &mut Framebuffer, frame: &WindowFrame) {
    let r = &frame.rect;
    let color = if frame.focused {
        if frame.pid.is_some() { CyberTheme::SUCCESS } else { CyberTheme::WARNING }
    } else {
        CyberTheme::BORDER
    };
    let fw = fb.width as i32; let fh = fb.height as i32;
    draw_hline_alpha(fb, r.x, r.y,              r.width,  color, 255, fw, fh);
    draw_hline_alpha(fb, r.x, r.y + r.height-1, r.width,  color, 255, fw, fh);
    draw_vline_alpha(fb, r.x,           r.y, r.height, color, 255, fw, fh);
    draw_vline_alpha(fb, r.x+r.width-1, r.y, r.height, color, 255, fw, fh);
}

fn draw_snap_overlay(fb: &mut Framebuffer, overlay: &SnapOverlay) {
    let fw = fb.width as i32; let fh = fb.height as i32;
    for zone in &overlay.zones {
        if zone.hover_alpha < 0.01 { continue; }
        let alpha = (zone.hover_alpha * 60.0) as u8;
        let r = &zone.rect;
        let x0 = r.x.max(0) as usize;
        let y0 = r.y.max(0) as usize;
        let x1 = (r.x + r.width) .min(fw).max(0) as usize;
        let y1 = (r.y + r.height).min(fh).max(0) as usize;
        for y in y0..y1 {
            for x in x0..x1 {
                let bg = fb.get_pixel(x, y);
                fb.plot_pixel(x, y, alpha_blend(CyberTheme::ACCENT, bg, alpha));
            }
        }
        let ba = (zone.hover_alpha * 160.0) as u8;
        draw_hline_alpha(fb, r.x, r.y,              r.width, CyberTheme::ACCENT, ba, fw, fh);
        draw_hline_alpha(fb, r.x, r.y + r.height-1, r.width, CyberTheme::ACCENT, ba, fw, fh);
        draw_vline_alpha(fb, r.x,           r.y, r.height, CyberTheme::ACCENT, ba, fw, fh);
        draw_vline_alpha(fb, r.x + r.width-1, r.y, r.height, CyberTheme::ACCENT, ba, fw, fh);
    }
}

fn draw_text_fb(fb: &mut Framebuffer, x: usize, y: usize, text: &str, color: u32) {
    let mut cx = x;
    for c in text.chars() {
        if c == ' ' { cx += 7; continue; }
        if cx + 8 > fb.width { break; }
        let glyph = crate::font::vga_font::get_font_data(c);
        for (row, &byte) in glyph.iter().take(8).enumerate() {
            if y + row >= fb.height { break; }
            for bit in 0..8usize {
                if byte & (0x80 >> bit) != 0 {
                    let px = cx + bit;
                    if px < fb.width { fb.plot_pixel(px, y + row, color); }
                }
            }
        }
        cx += 8;
    }
}

// ============================================================
// CURSOR ÇİZİMİ
// ============================================================

fn draw_cursor(fb: &mut Framebuffer, mx: i32, my: i32) {
    // Ok cursor: 12×19
    static CURSOR_MASK: &[u16] = &[
        0b1000000000000000,
        0b1100000000000000,
        0b1110000000000000,
        0b1111000000000000,
        0b1111100000000000,
        0b1111110000000000,
        0b1111111000000000,
        0b1111111100000000,
        0b1111111110000000,
        0b1111111111000000,
        0b1111111000000000,
        0b1101110000000000,
        0b1001111000000000,
        0b0000111000000000,
        0b0000111100000000,
        0b0000011100000000,
        0b0000011100000000,
        0b0000001110000000,
        0b0000000000000000,
    ];
    let cx = mx; let cy = my;
    let fw = fb.width as i32; let fh = fb.height as i32;
    for (row, &bits) in CURSOR_MASK.iter().enumerate() {
        let py = cy + row as i32;
        if py < 0 || py >= fh { continue; }
        for col in 0..16usize {
            if bits & (0x8000 >> col) != 0 {
                let px = cx + col as i32;
                if px >= 0 && px < fw {
                    // Siyah outline
                    fb.plot_pixel(px as usize, py as usize, 0xFF000000);
                }
            }
        }
    }
    // Beyaz iç
    for (row, &bits) in CURSOR_MASK.iter().enumerate() {
        let py = cy + row as i32;
        if py < 0 || py >= fh { continue; }
        for col in 1..15usize {
            if bits & (0x8000 >> col) != 0
                && CURSOR_MASK[row] & (0x8000 >> (col.saturating_sub(1))) != 0
                && CURSOR_MASK[row] & (0x8000 >> (col + 1)) != 0
                && row > 0 && CURSOR_MASK[row - 1] & (0x8000 >> col) != 0
            {
                let px = cx + col as i32;
                if px >= 0 && px < fw {
                    fb.plot_pixel(px as usize, py as usize, 0xFFFFFFFF);
                }
            }
        }
    }
}

#[inline]
fn wait_ticks(delta: u64) {
    if delta == 0 {
        return;
    }
    let start = crate::interrupts::get_ticks();
    let target = start.wrapping_add(delta);
    while crate::interrupts::get_ticks() < target {
        if x86_64::instructions::interrupts::are_enabled() {
            x86_64::instructions::hlt();
        } else {
            core::hint::spin_loop();
        }
    }
}

// ============================================================
// ANA COMPOSITOR DÖNGÜSÜ
// ============================================================

/// Compositor ana döngüsü — asla geri dönmez.
pub fn run(fb: &mut Framebuffer) -> ! {
    let width  = fb.width  as i32;
    let height = fb.height as i32;
    let backbuf_len = fb.pixels_per_scan_line * fb.height;
    let mut backbuf = alloc::vec![0u32; backbuf_len];

    crate::serial_println!("[COMPOSITOR] Cyber-Industrial WM başlatılıyor {}x{}", width, height);
    crate::drivers::ps2::init();
    let _ = crate::drivers::mouse::init();
    crate::drivers::mouse::reinit_streaming();
    crate::drivers::mouse::set_bounds(width, height);

    let mut wm = CompState::new(width, height);
    let mut desktop = Desktop::new(fb.width, fb.height);
    desktop.launch_app("finder");
    desktop.launch_app("terminal");

    // Faz 2 — Startup: Desktop pencerelerini WM çerçevelerine kaydet
    {
        let infos = desktop.app_window_infos();
        for (idx, (title, rect)) in infos.iter().enumerate() {
            let id = wm.register_desktop_window(title, *rect, idx);
            if idx + 1 == infos.len() {
                wm.focused = Some(id);
            }
        }
    }

    let mut panel = CyberPanel::new(width);
    panel.set_workspace_count(4);
    let mut command_bar = GlobalCommandBar::new(width, PANEL_HEIGHT);

    let mut last_mx = width / 2;
    let mut last_my = height / 2;
    let mut left_was_down = false;
    let mut drag_started = false;
    let mut frame_count: u64 = 0;
    let mut idle_frames: u32 = 0;
    let mut last_command_focus: Option<WindowId> = None;
    // Dock / Spotlight / MenuBar üzerinden açılan yeni pencereleri izler
    let mut known_app_count = desktop.app_count();

    crate::serial_println!("[COMPOSITOR] Ana döngü başlatıldı");

    loop {
        let start_tick = crate::task::scheduler::get_ticks();
        frame_count += 1;
        let dt = 1.0f32 / 60.0;

        // ------------------------------------------------------------------
        // 1. INPUT
        // ------------------------------------------------------------------
        use crate::drivers::input::{pop_event, InputEvent};
        use pc_keyboard::DecodedKey;
        let mut mouse_bytes_from_irq = 0usize;
        let mut events_processed = 0usize;
        const MAX_EVENTS_PER_FRAME: usize = 512;

        while let Some(event) = pop_event() {
            events_processed += 1;
            match event {
                InputEvent::MouseByte(byte) => {
                    crate::drivers::mouse::handle_packet(byte);
                    mouse_bytes_from_irq += 1;
                }
                InputEvent::Mouse(_) => {}
                InputEvent::Keyboard(key) => match key {
                    DecodedKey::RawKey(raw) => {
                        let sc = raw as u8;
                        wm.mod_state.update(sc, true);
                        if let Some(shortcut) = wm.mod_state.match_shortcut(sc) {
                            handle_shortcut(&mut wm, &mut panel, shortcut);
                        } else {
                            desktop.on_special_key(raw);
                        }
                    }
                    DecodedKey::Unicode(c) => { desktop.on_key(c); }
                },
            }
            if events_processed >= MAX_EVENTS_PER_FRAME {
                break;
            }
        }

        // IRQ12 gelmezse güvenli fallback: sadece AUX(byte) okuyarak mouse akışını sürdür.
        if mouse_bytes_from_irq == 0 {
            let _ = crate::drivers::mouse::poll_burst(12);
        }

        let (mx, my) = mouse::get_position();
        let buttons  = mouse::get_buttons();
        let left_down = buttons.left;
        let moved     = mx != last_mx || my != last_my;
        command_bar.on_mouse_move(mx, my);

        let has_wm_anim = wm.frames.iter().any(|f| f.state.is_animating());
        let has_overlay_anim = wm.snap_overlay.zones.iter().any(|z| z.hover_alpha > 0.01);
        let interactive_frame = moved
            || (left_down != left_was_down)
            || mouse_bytes_from_irq > 0
            || wm.drag.is_some()
            || wm.resize.is_some()
            || has_wm_anim
            || has_overlay_anim;

        if left_down && !left_was_down {
            if command_bar.on_mouse_down(mx, my) {
                drag_started = false;
            } else {
                wm.on_mouse_down(mx, my);
                drag_started = false;
                handle_btn_click(&mut wm, &mut desktop, mx, my);
                // Also route press to dock/chrome when cursor is not over a WM frame
                let in_dock_area = my >= (wm.screen_h - 90).max(0);
                let wm_frame_under = wm.frames.iter().any(|f| {
                    f.state.is_visible()
                        && mx >= f.rect.x && mx < f.rect.x + f.rect.width
                        && my >= f.rect.y && my < f.rect.y + f.rect.height
                });
                if in_dock_area || !wm_frame_under {
                    desktop.update_mouse(mx, my, true);
                }
            }
        }
        if left_down && moved { wm.on_mouse_drag(mx, my); drag_started = true; }
        if !left_down && left_was_down {
            wm.on_mouse_up(mx, my);
            if !drag_started { desktop.update_mouse(mx, my, true); }
            drag_started = false;
        }
        if !left_down { desktop.update_mouse(mx, my, false); }

        // ── Yeni pencere tespiti (Dock / Spotlight / MenuBar başlatmaları) ──
        let current_count = desktop.app_count();
        if current_count > known_app_count {
            for new_idx in known_app_count..current_count {
                let title = desktop.app_title(new_idx)
                    .map(|s| alloc::string::String::from(s))
                    .unwrap_or_else(|| alloc::string::String::from("App"));
                let rect = desktop.app_rect_at(new_idx)
                    .unwrap_or(Rect::new(
                        80 + (new_idx as i32 % 6) * 36,
                        56 + (new_idx as i32 % 4) * 28,
                        740, 500,
                    ));
                let new_id = wm.register_desktop_window(&title, rect, new_idx);
                // Açılış animasyonu ile başlat
                if let Some(f) = wm.get_frame_mut(new_id) {
                    f.state = WinState::Opening(0.0);
                }
                wm.focused = Some(new_id);
                crate::serial_println!("[WM] Yeni pencere kaydedildi: {:?} idx={}", new_id, new_idx);
            }
            known_app_count = current_count;
        }

        last_mx = mx; last_my = my; left_was_down = left_down;

        if !interactive_frame {
            idle_frames = idle_frames.saturating_add(1);
            // Tamamen donuk görünmemesi için düşük frekanslı idle redraw (~100ms)
            if idle_frames < 10 {
                wait_ticks(1);
                continue;
            }
        } else {
            idle_frames = 0;
        }

        // ------------------------------------------------------------------
        // 2. UPDATE
        // ------------------------------------------------------------------
        wm.update_animations(dt);

        // Faz 2 — Kapatılan pencereleri Desktop'a yansıt
        let close_ids: alloc::vec::Vec<WindowId> = wm.close_queue.drain(..).collect();
        for id in close_ids {
            if let Some(app_idx) = wm.desktop_idx_of(id) {
                desktop.close_app(app_idx);
                // Kaydırılan indeksleri güncelle
                wm.desktop_map.retain(|(wid, _)| *wid != id);
                for (_, aidx) in &mut wm.desktop_map {
                    if *aidx > app_idx { *aidx -= 1; }
                }
            }
            wm.frames.retain(|f| f.id != id);
        }
        // minimize_queue: sadece WM animasyonu, Desktop'ta gizleme (gerekirse)
        let _ = wm.minimize_queue.drain(..).collect::<alloc::vec::Vec<_>>();

        if left_down && drag_started { wm.snap_overlay.update(mx, my, dt); }
        panel.update(dt);
        if let Some(id) = wm.focused {
            if let Some(f) = wm.get_frame(id) {
                panel.set_active_title(&f.title);
                if last_command_focus != Some(id) {
                    command_bar.post_focus_changed(id.0, f.pid.is_some());
                    command_bar.set_active_title(&f.title);
                    last_command_focus = Some(id);
                }
            }
        } else {
            panel.set_active_title("echOS");
            if last_command_focus.is_some() {
                command_bar.post_focus_changed(0, false);
                command_bar.set_active_title("Desktop");
                last_command_focus = None;
            }
        }
        command_bar.update();
        while let Some(action) = command_bar.poll_action() {
            match action {
                CommandAction::Close => wm.close_focused(),
                CommandAction::Minimize => wm.minimize_focused(),
                CommandAction::MaximizeToggle => wm.maximize_toggle_focused(),
                CommandAction::None => {}
            }
        }

        // ── WM odak → Desktop aktif pencere senkronizasyonu ──────────────
        if let Some(fid) = wm.focused {
            if let Some(app_idx) = wm.desktop_idx_of(fid) {
                desktop.set_active_app(app_idx);
            }
        }

        desktop.update(dt);

        // ── Desktop'tan çıkan pencere sayısı azalmasını yakala ───────────
        // (spotlight / menü yoluyla launch sonrası tekrar kontrol)
        let post_update_count = desktop.app_count();
        if post_update_count > known_app_count {
            for new_idx in known_app_count..post_update_count {
                let title = desktop.app_title(new_idx)
                    .map(|s| alloc::string::String::from(s))
                    .unwrap_or_else(|| alloc::string::String::from("App"));
                let rect = desktop.app_rect_at(new_idx)
                    .unwrap_or(Rect::new(
                        80 + (new_idx as i32 % 6) * 36,
                        56 + (new_idx as i32 % 4) * 28,
                        740, 500,
                    ));
                let new_id = wm.register_desktop_window(&title, rect, new_idx);
                if let Some(f) = wm.get_frame_mut(new_id) {
                    f.state = WinState::Opening(0.0);
                }
                wm.focused = Some(new_id);
            }
            known_app_count = post_update_count;
        }

        // ------------------------------------------------------------------
        // 3. RENDER
        // ------------------------------------------------------------------

        let mut frame_fb = Framebuffer {
            base_addr: backbuf.as_mut_ptr() as usize,
            width: fb.width,
            height: fb.height,
            pixels_per_scan_line: fb.pixels_per_scan_line,
        };

        // 3a. Desktop (sadece arkaplan / ızgara — dock ve spotlight aşağıda)
        desktop.draw_wallpaper_only(&mut frame_fb);

        // 3b. Pencereler (z-order: küçük → büyük = arka → ön)
        let sorted_ids: Vec<(u32, WindowId)> = {
            let mut v: Vec<(u32, WindowId)> = wm.frames.iter()
                .filter(|f| f.state.is_visible())
                .map(|f| (f.z_order, f.id))
                .collect();
            v.sort_by_key(|(z, _)| *z);
            v
        };

        for (_, id) in &sorted_ids {
            let id = *id;
            let (frame_rect, frame_normal, is_focused, state, opacity, decos, title_str, pid_opt) = {
                let f = match wm.get_frame(id) { Some(x) => x, None => continue };
                (f.rect, f.normal_rect, f.focused, f.state, f.opacity, f.decorations, f.title.clone(), f.pid)
            };
            let dock_rect = wm.dock_rect;
            let sw = wm.screen_w; let sh = wm.screen_h;

            // Görsel rect (animasyon interpolasyonu)
            let vis_rect = match state {
                WinState::Opening(t) => {
                    let et_q8 = hyper_ease_out_q8(t_to_q8(t));
                    let cx = frame_rect.x + frame_rect.width  / 2;
                    let cy = frame_rect.y + frame_rect.height / 2;
                    let w = ((frame_rect.width * et_q8 as i32) / 255).max(1);
                    let h = ((frame_rect.height * et_q8 as i32) / 255).max(1);
                    Rect::new(cx - w/2, cy - h/2, w, h)
                }
                WinState::Closing(t) | WinState::Minimizing(t) => {
                    let et_q8 = hyper_ease_out_q8(t_to_q8(t));
                    lerp_rect_q8(&frame_rect, &dock_rect, et_q8)
                }
                WinState::Restoring(t) => {
                    let et_q8 = hyper_ease_out_q8(t_to_q8(t));
                    lerp_rect_q8(&dock_rect, &frame_normal, et_q8)
                }
                WinState::Snapping { target: snap_t, t } => {
                    let target_r = snap_t.compute_rect(sw, sh, PANEL_HEIGHT);
                    let et_q8 = hyper_ease_out_q8(t_to_q8(t));
                    lerp_rect_q8(&frame_normal, &target_r, et_q8)
                }
                WinState::Maximizing(t) => {
                    let max_r = Rect::new(0, PANEL_HEIGHT, sw, sh - PANEL_HEIGHT);
                    let et_q8 = hyper_ease_out_q8(t_to_q8(t));
                    lerp_rect_q8(&frame_normal, &max_r, et_q8)
                }
                _ => frame_rect,
            };

            let vis_alpha: u8 = match state {
                WinState::Opening(t)    => hyper_ease_out_q8(t_to_q8(t)) as u8,
                WinState::Closing(t)    => (((1.0 - t) * 255.0) as u8),
                WinState::Minimizing(t) => (((1.0 - t) * 255.0) as u8),
                WinState::Restoring(t)  => hyper_ease_out_q8(t_to_q8(t)) as u8,
                _ => (opacity * 255.0) as u8,
            };

            if vis_rect.width <= 0 || vis_rect.height <= 0 || vis_alpha == 0 { continue; }

            // Gölge
            if !PERF_FAST_RENDER {
                let shadow_op = if is_focused { 80u8 } else { 35u8 };
                let shadow_spread = if is_focused { SHADOW_SPREAD } else { SHADOW_SPREAD / 2 };
                draw_shadow(&mut frame_fb, &vis_rect, shadow_spread, shadow_op);
            }

            // İçerik alanı: cam efekti
            let content_rect = vis_rect;
            if content_rect.height > 0 {
                draw_frosted_glass(&mut frame_fb, &content_rect, 0x10141A, 210);
                // Uygulama içeriğini Cyber çerçevesinin içine çiz.
                // set_app_rect ile Desktop, pencereyi doğru konumda render eder;
                // aynı rect WM frame_rect'e de geri yazılır (drag/snap sonrası senkron).
                if let Some(app_idx) = wm.desktop_idx_of(id) {
                    // Set app rect to vis_rect so the app draws from window top.
                    // The WM Cyber titlebar is drawn AFTER the app and covers the
                    // app's own built-in chrome (toolbar/tab-bar) naturally.
                    desktop.set_app_rect(app_idx, vis_rect);
                    desktop.draw_app_window(&mut frame_fb, app_idx);
                }
            }

            // Geçici WindowFrame (render için)
            let render_frame = WindowFrame {
                id,
                title: title_str,
                rect: vis_rect,
                normal_rect: vis_rect,
                state: WinState::Normal,
                z_order: 0,
                focused: is_focused,
                opacity: 1.0,
                blur_radius: 12,
                shadow_spread: SHADOW_SPREAD as u8,
                decorations: decos,
                pid: pid_opt,
            };
            draw_window_border(&mut frame_fb, &render_frame);
        }

        // 3b-son. Dock + Spotlight (tüm pencelelerin üzerinde, snap altında)
        desktop.draw_chrome(&mut frame_fb);

        // 3c. Snap overlay
        if left_down && drag_started {
            draw_snap_overlay(&mut frame_fb, &wm.snap_overlay);
        }

        // 3d. CyberPanel
        panel.draw(&mut frame_fb);

        // 3d+. Global Command Bar (borderless kontroller)
        command_bar.draw(&mut frame_fb);

        // 3e. Cursor
        draw_cursor(&mut frame_fb, mx, my);

        // Front-buffer present (single blit)
        fb.buffer_mut().copy_from_slice(&backbuf);

        // Frame pacing — timer tick 10ms, interaktif karelerde daha düşük gecikme
        let end_tick = crate::task::scheduler::get_ticks();
        let elapsed = end_tick.wrapping_sub(start_tick);
        let target_ticks = if interactive_frame { 1 } else { 2 };
        if elapsed < target_ticks { wait_ticks((target_ticks - elapsed) as u64); }
    }
}

// ============================================================
// KLAVYE KISA YOL İŞLEYİCİ
// ============================================================

fn handle_shortcut(wm: &mut CompState, panel: &mut CyberPanel, shortcut: ShortcutId) {
    match shortcut {
        ShortcutId::SnapLeft     => wm.apply_snap(SnapTarget::Left),
        ShortcutId::SnapRight    => wm.apply_snap(SnapTarget::Right),
        ShortcutId::SnapMaximize => wm.apply_snap(SnapTarget::Maximize),
        ShortcutId::SnapRestore  => {
            if let Some(id) = wm.focused {
                if let Some(f) = wm.get_frame_mut(id) {
                    if matches!(f.state, WinState::Snapped(_) | WinState::Maximized) {
                        f.rect = f.normal_rect;
                        f.state = WinState::Restoring(0.0);
                    }
                }
            }
        }
        ShortcutId::SnapTopLeft  => wm.apply_snap(SnapTarget::TopLeft),
        ShortcutId::SnapTopRight => wm.apply_snap(SnapTarget::TopRight),
        ShortcutId::CloseWindow    => wm.close_focused(),
        ShortcutId::MinimizeWindow => wm.minimize_focused(),
        ShortcutId::MaximizeToggle => wm.maximize_toggle_focused(),
        ShortcutId::CycleWindows   => wm.cycle_windows(),
        ShortcutId::ShowDesktop    => {
            for f in &mut wm.frames {
                if f.state == WinState::Normal { f.state = WinState::Minimizing(0.0); }
            }
        }
        ShortcutId::Workspace1 => { wm.focused = None; panel.set_workspace(0); }
        ShortcutId::Workspace2 => { wm.focused = None; panel.set_workspace(1); }
        ShortcutId::Workspace3 => { wm.focused = None; panel.set_workspace(2); }
        ShortcutId::Workspace4 => { wm.focused = None; panel.set_workspace(3); }
        _ => {}
    }
}

fn handle_btn_click(wm: &mut CompState, _desktop: &mut Desktop, mx: i32, my: i32) {
    let hit_id = {
        let mut sorted: Vec<&WindowFrame> = wm.frames.iter()
            .filter(|f| f.state.is_visible())
            .collect();
        sorted.sort_by(|a, b| b.z_order.cmp(&a.z_order));
        let mut r = None;
        for f in sorted {
            if btn_close_rect(f).contains(mx, my)
                || btn_min_rect(f).contains(mx, my)
                || btn_max_rect(f).contains(mx, my)
            { r = Some(f.id); break; }
        }
        r
    };
    if let Some(id) = hit_id {
        let (is_close, is_min, is_max) = {
            let f = match wm.get_frame(id) { Some(x) => x, None => return };
            (btn_close_rect(f).contains(mx, my),
             btn_min_rect(f).contains(mx, my),
             btn_max_rect(f).contains(mx, my))
        };
        if is_close {
            if let Some(f) = wm.get_frame_mut(id) { f.state = WinState::Closing(0.0); }
        } else if is_min {
            if let Some(f) = wm.get_frame_mut(id) { f.state = WinState::Minimizing(0.0); }
        } else if is_max {
            wm.focused = Some(id);
            wm.maximize_toggle_focused();
        }
    }
}
