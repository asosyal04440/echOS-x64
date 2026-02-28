//! # echOS Mouse Cursor Modülü
//!
//! Sprite tabanlı fare imleci çizimi ve piksel yedekleme/geri yükleme işlemleri.
//! Çekirdek seviyesinde çalıştığından GPU hızlandırması veya işletim sistemi sürücüsü
//! kullanılamaz; tüm çizim doğrudan framebuffer bellek adresi üzerinden yapılır.
//!
//! ## Çalışma Prensibi
//! 1. İmleç çizilmeden önce altındaki pikseller `CURSOR_BACKUP` tamponuna yedeklenir.
//! 2. `CURSOR_SPRITE` bitmap'i (12×19 piksel) framebuffer'a yazılır:
//!    - `0` = şeffaf (piksel yazılmaz, arka plan korunur)
//!    - `1` = beyaz (`0xFFFFFF`)
//!    - `2` = siyah dış çizgi (`0x000000`)
//! 3. Sonraki karede imleç yeni konuma taşınmadan önce eski konum yedekten geri yüklenir.
//!
//! ## Neden Yedekleme Gerekir?
//! Framebuffer doğrudan ekrana yazılan bir bellek bölgesidir; çift tamponlama (double
//! buffering) olmadığında imlecin üzerine yazıldığı pikseller kalıcı olarak bozulur.
//! Yedekleme sayesinde imleç her konumda iz bırakmadan hareket eder.
//!
//! ## Güvenlik: `unsafe` Kullanımı
//! `CURSOR_BACKUP`, `LAST_X`, `LAST_Y`, `CURSOR_VISIBLE` değişkenleri `static mut`
//! olarak tanımlanmıştır. Çekirdek tek çekirdekli (single-core) ve kesme-güvenli
//! bağlamda çalıştığından veri yarışı riski yoktur; yine de erişimler `unsafe` blok
//! içinde yapılmaktadır.

use crate::drivers::mouse;
use crate::gop::framebuffer::Framebuffer;

/// Cursor boyutları (12x19 piksel)
const CURSOR_WIDTH: usize = 12;
const CURSOR_HEIGHT: usize = 19;

/// İmleç sprite bitmap'i; her hücre bir pikseli temsil eder.
/// - `0` = şeffaf (bu pikseli çizme, arka planı koru)
/// - `1` = beyaz dolgu (#FFFFFF)
/// - `2` = siyah dış çizgi (#000000)
/// Klasik ok imleci şekli: sol üstten başlayan ince bir ok.
const CURSOR_SPRITE: [[u8; CURSOR_WIDTH]; CURSOR_HEIGHT] = [
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

/// Cursor altındaki piksellerin yedeği
static mut CURSOR_BACKUP: [[u32; CURSOR_WIDTH]; CURSOR_HEIGHT] = [[0; CURSOR_WIDTH]; CURSOR_HEIGHT];
/// Son cursor X pozisyonu
static mut LAST_X: i32 = -1;
/// Son cursor Y pozisyonu
static mut LAST_Y: i32 = -1;
/// Cursor görünür mü?
static mut CURSOR_VISIBLE: bool = false;

/// Mouse cursor'u mevcut fare pozisyonuna çizer.
/// Her çağrıda: (1) eski konumu yedekten geri yükle, (2) yeni konumu yedekle, (3) sprite çiz.
/// Bu üç adım her kare çağrılmalıdır; aksi hâlde eski imlecin izi ekranda kalır.
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
                _ => {}                               // Transparan
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
