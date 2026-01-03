//! # echOS Mouse Cursor Modülü
//! 
//! Mouse cursor rendering ve backup/restore işlemleri.
//! Sprite-based cursor çizimi yapılır.

use crate::gop::framebuffer::Framebuffer;
use crate::drivers::mouse;

/// Cursor boyutları (12x19 piksel)
const CURSOR_WIDTH: usize = 12;
const CURSOR_HEIGHT: usize = 19;

/// Cursor sprite bitmap.
/// 0 = transparent, 1 = beyaz, 2 = siyah
const CURSOR_SPRITE: [[u8; CURSOR_WIDTH]; CURSOR_HEIGHT] = [
    [2,0,0,0,0,0,0,0,0,0,0,0],
    [2,2,0,0,0,0,0,0,0,0,0,0],
    [2,1,2,0,0,0,0,0,0,0,0,0],
    [2,1,1,2,0,0,0,0,0,0,0,0],
    [2,1,1,1,2,0,0,0,0,0,0,0],
    [2,1,1,1,1,2,0,0,0,0,0,0],
    [2,1,1,1,1,1,2,0,0,0,0,0],
    [2,1,1,1,1,1,1,2,0,0,0,0],
    [2,1,1,1,1,1,1,1,2,0,0,0],
    [2,1,1,1,1,1,1,1,1,2,0,0],
    [2,1,1,1,1,1,1,1,1,1,2,0],
    [2,1,1,1,1,1,1,2,2,2,2,2],
    [2,1,1,1,2,1,1,2,0,0,0,0],
    [2,1,1,2,0,2,1,1,2,0,0,0],
    [2,1,2,0,0,2,1,1,2,0,0,0],
    [2,2,0,0,0,0,2,1,1,2,0,0],
    [2,0,0,0,0,0,2,1,1,2,0,0],
    [0,0,0,0,0,0,0,2,1,1,2,0],
    [0,0,0,0,0,0,0,2,2,2,0,0],
];

/// Cursor altındaki piksellerin yedeği
static mut CURSOR_BACKUP: [[u32; CURSOR_WIDTH]; CURSOR_HEIGHT] = [[0; CURSOR_WIDTH]; CURSOR_HEIGHT];
/// Son cursor X pozisyonu
static mut LAST_X: i32 = -1;
/// Son cursor Y pozisyonu
static mut LAST_Y: i32 = -1;
/// Cursor görünür mü?
static mut CURSOR_VISIBLE: bool = false;

/// Mouse cursor'u mevcut pozisyona çizer.
/// Önceki pozisyondaki pikselleri geri yükler.
pub fn draw(fb: &mut Framebuffer) {
    let (mx, my) = mouse::get_position();
    let x = mx as usize;
    let y = my as usize;
    
    // Önceki pozisyonu geri yükle
    unsafe {
        if CURSOR_VISIBLE && LAST_X >= 0 && LAST_Y >= 0 {
            restore_backup(fb, LAST_X as usize, LAST_Y as usize);
        }
    }
    
    // Mevcut pozisyonu yedekle
    unsafe {
        save_backup(fb, x, y);
        LAST_X = mx;
        LAST_Y = my;
        CURSOR_VISIBLE = true;
    }
    
    // Cursor'u çiz
    for row in 0..CURSOR_HEIGHT {
        for col in 0..CURSOR_WIDTH {
            let px = x + col;
            let py = y + row;
            
            match CURSOR_SPRITE[row][col] {
                1 => fb.plot_pixel(px, py, 0xFFFFFF), // Beyaz
                2 => fb.plot_pixel(px, py, 0x000000), // Siyah
                _ => {} // Transparan
            }
        }
    }
}

/// Cursor altındaki pikselleri yedekler.
fn save_backup(fb: &mut Framebuffer, x: usize, y: usize) {
    unsafe {
        for row in 0..CURSOR_HEIGHT {
            for col in 0..CURSOR_WIDTH {
                CURSOR_BACKUP[row][col] = fb.get_pixel(x + col, y + row);
            }
        }
    }
}

/// Yedeklenen pikselleri geri yükler.
fn restore_backup(fb: &mut Framebuffer, x: usize, y: usize) {
    unsafe {
        for row in 0..CURSOR_HEIGHT {
            for col in 0..CURSOR_WIDTH {
                fb.plot_pixel(x + col, y + row, CURSOR_BACKUP[row][col]);
            }
        }
    }
}

/// Cursor'u gizler.
pub fn hide(fb: &mut Framebuffer) {
    unsafe {
        if CURSOR_VISIBLE && LAST_X >= 0 && LAST_Y >= 0 {
            restore_backup(fb, LAST_X as usize, LAST_Y as usize);
            CURSOR_VISIBLE = false;
        }
    }
}
