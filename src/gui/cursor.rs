//! # echOS Mouse Cursor Modülü
//!
//! Sprite tabanlı fare imleci çizimi ve piksel yedekleme/geri yükleme işlemleri.
//! Çekirdek seviyesinde çalıştığından GPU hızlandırması veya işletim sistemi sürücüsü
//! kullanılamaz; tüm çizim doğrudan framebuffer bellek adresi üzerinden yapılır.
//!
//! ## Cursor Types
//! - `Arrow` - Default pointer (12x19)
//! - `IBeam` - Text selection cursor (7x15)
//! - `ResizeH` - Horizontal resize (15x5)
//! - `ResizeV` - Vertical resize (5x15)
//! - `ResizeDiag1` - Diagonal resize  (15x15)
//! - `ResizeDiag2` - Diagonal resize / (15x15)
//! - `Hand` - Link/grab cursor (13x13)
//! - `Crosshair` - Precise selection (13x13)
//! - `NotAllowed` - Action not allowed (14x14)

use crate::drivers::mouse;
use crate::gop::framebuffer::Framebuffer;
use core::sync::atomic::{AtomicU8, Ordering};

// ============================================================================
// CURSOR TYPES
// ============================================================================

/// Available cursor types for different UI contexts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CursorKind {
    /// Default arrow pointer
    Arrow = 0,
    /// I-beam for text selection
    IBeam = 1,
    /// Horizontal resize (left-right arrows)
    ResizeH = 2,
    /// Vertical resize (up-down arrows)
    ResizeV = 3,
    /// Diagonal resize \
    ResizeDiag1 = 4,
    /// Diagonal resize /
    ResizeDiag2 = 5,
    /// Hand/grab for links
    Hand = 6,
    /// Crosshair for precise selection
    Crosshair = 7,
    /// Not allowed (prohibited action)
    NotAllowed = 8,
    /// Wait/spinner
    Wait = 9,
    /// Move cursor (4 arrows)
    Move = 10,
}

impl Default for CursorKind {
    fn default() -> Self {
        Self::Arrow
    }
}

impl CursorKind {
    /// Get the dimensions for this cursor type.
    pub fn dimensions(&self) -> (usize, usize) {
        match self {
            CursorKind::Arrow => (12, 19),
            CursorKind::IBeam => (7, 15),
            CursorKind::ResizeH => (15, 5),
            CursorKind::ResizeV => (5, 15),
            CursorKind::ResizeDiag1 => (15, 15),
            CursorKind::ResizeDiag2 => (15, 15),
            CursorKind::Hand => (13, 13),
            CursorKind::Crosshair => (13, 13),
            CursorKind::NotAllowed => (14, 14),
            CursorKind::Wait => (16, 16),
            CursorKind::Move => (15, 15),
        }
    }
}

// ============================================================================
// CURSOR SPRITES
// ============================================================================

/// Maximum cursor dimensions for backup buffer
const MAX_CURSOR_WIDTH: usize = 20;
const MAX_CURSOR_HEIGHT: usize = 20;

/// Arrow cursor (12x19) - default pointer
const CURSOR_ARROW: [[u8; 12]; 19] = [
    [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [2, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [2, 1, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [2, 1, 1, 2, 0, 0, 0, 0, 0, 0, 0, 0],
    [2, 1, 1, 1, 2, 0, 0, 0, 0, 0, 0, 0],
    [2, 1, 1, 1, 1, 2, 0, 0, 0, 0, 0, 0],
    [2, 1, 1, 1, 1, 1, 2, 0, 0, 0, 0, 0],
    [2, 1, 1, 1, 1, 1, 1, 2, 0, 0, 0, 0],
    [2, 1, 1, 1, 1, 1, 1, 1, 2, 0, 0, 0],
    [2, 1, 1, 1, 1, 1, 1, 1, 1, 2, 0, 0],
    [2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 0],
    [2, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2],
    [2, 1, 1, 1, 2, 1, 1, 2, 0, 0, 0, 0],
    [2, 1, 1, 2, 0, 2, 1, 1, 2, 0, 0, 0],
    [2, 1, 2, 0, 0, 2, 1, 1, 2, 0, 0, 0],
    [2, 2, 0, 0, 0, 0, 2, 1, 1, 2, 0, 0],
    [2, 0, 0, 0, 0, 0, 2, 1, 1, 2, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 2, 1, 1, 2, 0],
    [0, 0, 0, 0, 0, 0, 0, 2, 2, 2, 0, 0],
];

/// I-beam cursor (7x15) - text editing
const CURSOR_IBEAM: [[u8; 7]; 15] = [
    [2, 2, 2, 2, 2, 2, 2],
    [0, 0, 2, 2, 2, 0, 0],
    [0, 0, 2, 1, 2, 0, 0],
    [0, 0, 2, 1, 2, 0, 0],
    [0, 0, 2, 1, 2, 0, 0],
    [0, 0, 2, 1, 2, 0, 0],
    [0, 0, 2, 1, 2, 0, 0],
    [0, 0, 2, 1, 2, 0, 0],
    [0, 0, 2, 1, 2, 0, 0],
    [0, 0, 2, 1, 2, 0, 0],
    [0, 0, 2, 1, 2, 0, 0],
    [0, 0, 2, 1, 2, 0, 0],
    [0, 0, 2, 1, 2, 0, 0],
    [0, 0, 2, 2, 2, 0, 0],
    [2, 2, 2, 2, 2, 2, 2],
];

/// Horizontal resize cursor (15x5)
const CURSOR_RESIZE_H: [[u8; 15]; 5] = [
    [0, 0, 0, 2, 1, 1, 3, 3, 3, 1, 1, 2, 0, 0, 0],
    [2, 2, 2, 2, 1, 1, 3, 3, 3, 1, 1, 2, 2, 2, 2],
    [0, 0, 0, 2, 1, 1, 3, 3, 3, 1, 1, 2, 0, 0, 0],
    [0, 0, 0, 2, 1, 1, 3, 3, 3, 1, 1, 2, 0, 0, 0],
    [0, 0, 0, 0, 2, 2, 2, 2, 2, 2, 2, 0, 0, 0, 0],
];

/// Vertical resize cursor (5x15)
const CURSOR_RESIZE_V: [[u8; 5]; 15] = [
    [0, 2, 0, 0, 0],
    [0, 2, 0, 0, 0],
    [2, 3, 2, 0, 0],
    [2, 3, 2, 2, 2],
    [2, 3, 3, 3, 2],
    [2, 2, 3, 2, 2],
    [0, 2, 3, 2, 0],
    [0, 2, 3, 2, 0],
    [0, 2, 3, 2, 0],
    [0, 2, 3, 2, 0],
    [2, 2, 3, 2, 2],
    [2, 3, 3, 3, 2],
    [2, 3, 2, 2, 2],
    [2, 3, 2, 0, 0],
    [0, 2, 0, 0, 0],
];

/// Hand cursor (13x13) - for links
const CURSOR_HAND: [[u8; 13]; 13] = [
    [0, 0, 0, 0, 0, 0, 0, 2, 2, 2, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 2, 3, 1, 1, 2, 0, 0],
    [0, 0, 0, 0, 0, 2, 3, 3, 1, 1, 1, 2, 0],
    [0, 0, 0, 0, 0, 2, 3, 3, 1, 1, 1, 1, 2],
    [0, 0, 0, 0, 0, 2, 3, 3, 1, 1, 1, 1, 1],
    [0, 2, 2, 2, 2, 2, 3, 3, 3, 1, 1, 1, 1],
    [2, 3, 3, 3, 3, 3, 3, 3, 3, 3, 1, 1, 1],
    [2, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 1, 1],
    [2, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 1],
    [2, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 1],
    [0, 2, 3, 3, 3, 3, 3, 3, 3, 3, 3, 2, 0],
    [0, 0, 2, 3, 3, 3, 3, 3, 3, 3, 2, 0, 0],
    [0, 0, 0, 2, 2, 2, 2, 2, 2, 2, 0, 0, 0],
];

/// Crosshair cursor (13x13)
const CURSOR_CROSSHAIR: [[u8; 13]; 13] = [
    [0, 0, 0, 0, 0, 2, 2, 2, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 2, 1, 2, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 2, 1, 2, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 2, 1, 2, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 2, 1, 2, 0, 0, 0, 0, 0],
    [2, 2, 2, 2, 2, 2, 3, 2, 2, 2, 2, 2, 2],
    [2, 1, 1, 1, 1, 3, 3, 3, 1, 1, 1, 1, 2],
    [2, 2, 2, 2, 2, 2, 3, 2, 2, 2, 2, 2, 2],
    [0, 0, 0, 0, 0, 2, 1, 2, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 2, 1, 2, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 2, 1, 2, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 2, 1, 2, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 2, 2, 2, 0, 0, 0, 0, 0],
];

/// Not allowed cursor (14x14)
const CURSOR_NOT_ALLOWED: [[u8; 14]; 14] = [
    [0, 0, 0, 2, 2, 2, 2, 2, 2, 2, 0, 0, 0, 0],
    [0, 0, 2, 3, 3, 3, 3, 3, 3, 1, 2, 0, 0, 0],
    [0, 2, 3, 3, 3, 3, 3, 3, 3, 1, 1, 2, 0, 0],
    [2, 3, 3, 3, 3, 3, 3, 3, 2, 2, 1, 1, 2, 0],
    [2, 3, 3, 3, 3, 3, 3, 2, 1, 1, 1, 1, 2, 0],
    [2, 3, 3, 3, 3, 3, 2, 1, 1, 1, 1, 1, 2, 0],
    [2, 3, 3, 3, 3, 2, 1, 1, 1, 1, 1, 1, 2, 0],
    [2, 3, 3, 3, 2, 1, 1, 1, 1, 1, 1, 1, 2, 0],
    [2, 3, 3, 2, 1, 1, 1, 1, 1, 1, 1, 1, 2, 0],
    [2, 3, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 0],
    [2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 0],
    [0, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 0, 0],
    [0, 0, 2, 1, 1, 1, 1, 1, 1, 1, 2, 0, 0, 0],
    [0, 0, 0, 2, 2, 2, 2, 2, 2, 2, 0, 0, 0, 0],
];

// ============================================================================
// GLOBAL STATE
// ============================================================================

/// Current cursor type
static CURRENT_CURSOR: AtomicU8 = AtomicU8::new(0);

/// Backup buffer for cursor pixels
static mut CURSOR_BACKUP: [[u32; MAX_CURSOR_WIDTH]; MAX_CURSOR_HEIGHT] =
    [[0; MAX_CURSOR_WIDTH]; MAX_CURSOR_HEIGHT];
/// Last cursor X position
static mut LAST_X: i32 = -1;
/// Last cursor Y position
static mut LAST_Y: i32 = -1;
/// Is cursor visible?
static mut CURSOR_VISIBLE: bool = false;
/// Last cursor kind
static mut LAST_CURSOR_KIND: u8 = 0;

// ============================================================================
// CURSOR API
// ============================================================================

/// Set the current cursor type.
pub fn set_cursor(kind: CursorKind) {
    CURRENT_CURSOR.store(kind as u8, Ordering::SeqCst);
}

/// Get the current cursor type.
pub fn get_cursor() -> CursorKind {
    match CURRENT_CURSOR.load(Ordering::SeqCst) {
        0 => CursorKind::Arrow,
        1 => CursorKind::IBeam,
        2 => CursorKind::ResizeH,
        3 => CursorKind::ResizeV,
        4 => CursorKind::ResizeDiag1,
        5 => CursorKind::ResizeDiag2,
        6 => CursorKind::Hand,
        7 => CursorKind::Crosshair,
        8 => CursorKind::NotAllowed,
        9 => CursorKind::Wait,
        10 => CursorKind::Move,
        _ => CursorKind::Arrow,
    }
}

/// Draw cursor at current mouse position.
pub fn draw(fb: &mut Framebuffer) {
    let (mx, my) = mouse::get_position();
    let x = mx as usize;
    let y = my as usize;
    let kind = get_cursor();
    let (w, h) = kind.dimensions();

    // Restore previous position
    unsafe {
        if CURSOR_VISIBLE && LAST_X >= 0 && LAST_Y >= 0 {
            restore_backup(fb, LAST_X as usize, LAST_Y as usize, LAST_CURSOR_KIND);
        }
    }

    // Save current position
    unsafe {
        save_backup(fb, x, y, w, h);
        LAST_X = mx;
        LAST_Y = my;
        LAST_CURSOR_KIND = kind as u8;
        CURSOR_VISIBLE = true;
    }

    // Draw cursor
    draw_cursor_sprite(fb, x, y, kind);
}

/// Draw the appropriate cursor sprite.
fn draw_cursor_sprite(fb: &mut Framebuffer, x: usize, y: usize, kind: CursorKind) {
    let accent = 0x00FFB2; // Cyan accent color

    match kind {
        CursorKind::Arrow => draw_sprite_12x19(fb, x, y, &CURSOR_ARROW, accent),
        CursorKind::IBeam => draw_sprite_7x15(fb, x, y, &CURSOR_IBEAM, accent),
        CursorKind::ResizeH => draw_sprite_15x5(fb, x, y, &CURSOR_RESIZE_H, accent),
        CursorKind::ResizeV => draw_sprite_5x15(fb, x, y, &CURSOR_RESIZE_V, accent),
        CursorKind::ResizeDiag1 => draw_sprite_12x19(fb, x, y, &CURSOR_ARROW, accent), // Fallback
        CursorKind::ResizeDiag2 => draw_sprite_12x19(fb, x, y, &CURSOR_ARROW, accent), // Fallback
        CursorKind::Hand => draw_sprite_13x13(fb, x, y, &CURSOR_HAND, accent),
        CursorKind::Crosshair => draw_sprite_13x13(fb, x, y, &CURSOR_CROSSHAIR, accent),
        CursorKind::NotAllowed => draw_sprite_14x14(fb, x, y, &CURSOR_NOT_ALLOWED, accent),
        CursorKind::Wait => draw_sprite_12x19(fb, x, y, &CURSOR_ARROW, accent), // Fallback
        CursorKind::Move => draw_sprite_12x19(fb, x, y, &CURSOR_ARROW, accent), // Fallback
    }
}

// Sprite drawing helpers
fn draw_sprite_12x19(
    fb: &mut Framebuffer,
    x: usize,
    y: usize,
    sprite: &[[u8; 12]; 19],
    accent: u32,
) {
    for row in 0..19 {
        for col in 0..12 {
            match sprite[row][col] {
                1 => fb.plot_pixel(x + col, y + row, 0xFFFFFF),
                2 => fb.plot_pixel(x + col, y + row, 0x000000),
                3 => fb.plot_pixel(x + col, y + row, accent),
                _ => {}
            }
        }
    }
}

fn draw_sprite_7x15(fb: &mut Framebuffer, x: usize, y: usize, sprite: &[[u8; 7]; 15], accent: u32) {
    for row in 0..15 {
        for col in 0..7 {
            match sprite[row][col] {
                1 => fb.plot_pixel(x + col, y + row, 0xFFFFFF),
                2 => fb.plot_pixel(x + col, y + row, 0x000000),
                3 => fb.plot_pixel(x + col, y + row, accent),
                _ => {}
            }
        }
    }
}

fn draw_sprite_15x5(fb: &mut Framebuffer, x: usize, y: usize, sprite: &[[u8; 15]; 5], accent: u32) {
    for row in 0..5 {
        for col in 0..15 {
            match sprite[row][col] {
                1 => fb.plot_pixel(x + col, y + row, 0xFFFFFF),
                2 => fb.plot_pixel(x + col, y + row, 0x000000),
                3 => fb.plot_pixel(x + col, y + row, accent),
                _ => {}
            }
        }
    }
}

fn draw_sprite_5x15(fb: &mut Framebuffer, x: usize, y: usize, sprite: &[[u8; 5]; 15], accent: u32) {
    for row in 0..15 {
        for col in 0..5 {
            match sprite[row][col] {
                1 => fb.plot_pixel(x + col, y + row, 0xFFFFFF),
                2 => fb.plot_pixel(x + col, y + row, 0x000000),
                3 => fb.plot_pixel(x + col, y + row, accent),
                _ => {}
            }
        }
    }
}

fn draw_sprite_13x13(
    fb: &mut Framebuffer,
    x: usize,
    y: usize,
    sprite: &[[u8; 13]; 13],
    accent: u32,
) {
    for row in 0..13 {
        for col in 0..13 {
            match sprite[row][col] {
                1 => fb.plot_pixel(x + col, y + row, 0xFFFFFF),
                2 => fb.plot_pixel(x + col, y + row, 0x000000),
                3 => fb.plot_pixel(x + col, y + row, accent),
                _ => {}
            }
        }
    }
}

fn draw_sprite_14x14(
    fb: &mut Framebuffer,
    x: usize,
    y: usize,
    sprite: &[[u8; 14]; 14],
    accent: u32,
) {
    for row in 0..14 {
        for col in 0..14 {
            match sprite[row][col] {
                1 => fb.plot_pixel(x + col, y + row, 0xFFFFFF),
                2 => fb.plot_pixel(x + col, y + row, 0x000000),
                3 => fb.plot_pixel(x + col, y + row, accent),
                _ => {}
            }
        }
    }
}

/// Save backup of pixels under cursor.
fn save_backup(fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
    unsafe {
        for row in 0..h {
            for col in 0..w {
                if x + col < fb.width && y + row < fb.height {
                    CURSOR_BACKUP[row][col] = fb.get_pixel(x + col, y + row);
                }
            }
        }
    }
}

/// Restore backup of pixels.
fn restore_backup(fb: &mut Framebuffer, x: usize, y: usize, kind: u8) {
    let cursor_kind = match kind {
        0 => CursorKind::Arrow,
        1 => CursorKind::IBeam,
        2 => CursorKind::ResizeH,
        3 => CursorKind::ResizeV,
        _ => CursorKind::Arrow,
    };
    let (w, h) = cursor_kind.dimensions();

    unsafe {
        for row in 0..h {
            for col in 0..w {
                if x + col < fb.width && y + row < fb.height {
                    fb.plot_pixel(x + col, y + row, CURSOR_BACKUP[row][col]);
                }
            }
        }
    }
}

/// Hide cursor.
pub fn hide(fb: &mut Framebuffer) {
    unsafe {
        if CURSOR_VISIBLE && LAST_X >= 0 && LAST_Y >= 0 {
            restore_backup(fb, LAST_X as usize, LAST_Y as usize, LAST_CURSOR_KIND);
            CURSOR_VISIBLE = false;
        }
    }
}
