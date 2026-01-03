//! # echOS Compositor (Grafik Birleştirici)
//! 
//! Bu modül, tile-tabanlı grafik rendering engine'ini içerir.
//! Desktop, pencereler ve mouse cursor'u birleştirerek ekrana çizer.

use crate::gop::framebuffer::Framebuffer;
use crate::drivers::mouse;
use crate::gui::desktop::Desktop;
use crate::gui::window::Window;

/// Tile boyutu (64x64 piksel).
/// Her tile bağımsız olarak render edilir ve framebuffer'a aktarılır.
const TILE_SIZE: usize = 64;

/// Ana compositor döngüsü.
/// 
/// # Açıklama
/// Bu fonksiyon sonsuza kadar çalışır ve aşağıdaki işlemleri yapar:
/// 1. Mouse inputunu okur
/// 2. Desktop durumunu günceller
/// 3. Tile-tabanlı rendering yapar
/// 4. Framebuffer'a çizer
/// 
/// # Tile-Based Rendering Avantajları
/// - Bellek erişimi cache-friendly (64x64 = 16KB, L1 cache'e sığar)
/// - Sadece değişen tile'lar yeniden çizilebilir (dirty rectangles)
/// - GPU-benzeri paralel yapıya uygun mimari
pub fn run(fb: &mut Framebuffer) -> ! {
    let width = fb.width;
    let height = fb.height;

    crate::serial_println!("Compositor: {}x{} (Tile-Based)", width, height);

    // Desktop ve pencereler
    let mut desktop = Desktop::new(width, height);
    
    // Test pencereleri
    let mut win1 = Window::new(100, 100, 400, 300, "Debug Window");
    win1.add_line("echOS GUI initialized.");
    win1.add_line("Drag me by the title bar!");
    win1.add_line("Status: Tile-Based Rendering Active");
    
    let mut win2 = Window::new(600, 150, 300, 200, "Notes");
    win2.add_line("Multi-window test.");
    win2.is_active = false;

    desktop.add_window(win1);
    desktop.add_window(win2);

    // Durum değişkenleri
    let mut last_mx = 0;
    let mut last_my = 0;
    let mut frame_count: u64 = 0;
    
    // Tile buffer (stack'te 16KB)
    let mut tile_buffer = [0u32; TILE_SIZE * TILE_SIZE];

    // ========================================================================
    // ANA DÖNGÜ
    // ========================================================================
    loop {
        frame_count += 1;
        
        // --------------------------------------------------------------------
        // 1. INPUT POLLING
        // --------------------------------------------------------------------
        mouse::poll();
        use crate::drivers::input::{pop_event, InputEvent};
        while let Some(event) = pop_event() {
            match event {
                InputEvent::MouseByte(byte) => {
                    crate::drivers::mouse::handle_packet(byte);
                },
                _ => {}
            }
        }
        
        let (mx, my) = mouse::get_position();
        let buttons = mouse::get_buttons();
        
        let mouse_moved = mx != last_mx || my != last_my;
        last_mx = mx;
        last_my = my;

        // --------------------------------------------------------------------
        // 2. DESKTOP UPDATE
        // --------------------------------------------------------------------
        let needs_redraw = desktop.update_mouse(mx, my, buttons.left);
        
        // --------------------------------------------------------------------
        // 3. TILE-BASED RENDERING
        // --------------------------------------------------------------------
        if needs_redraw || mouse_moved || frame_count == 1 {
            use crate::gui::theme::Theme;

            // Her tile için döngü
            for ty in (0..height).step_by(TILE_SIZE) {
                for tx in (0..width).step_by(TILE_SIZE) {
                    
                    // Tile boyutları (kenar tile'lar için düzeltme)
                    let tile_w = if tx + TILE_SIZE > width { width - tx } else { TILE_SIZE };
                    let tile_h = if ty + TILE_SIZE > height { height - ty } else { TILE_SIZE };
                    
                    // --------------------------------------------------------
                    // Layer 1: Background (Grid Pattern)
                    // --------------------------------------------------------
                    for y in 0..tile_h {
                        for x in 0..tile_w {
                            let abs_x = tx + x;
                            let abs_y = ty + y;
                            let color = if abs_x % 40 == 0 || abs_y % 40 == 0 {
                                0x4a2b7e // Grid çizgisi
                            } else {
                                0x1a0b2e // Arkaplan
                            };
                            tile_buffer[y * TILE_SIZE + x] = color as u32;
                        }
                    }
                    
                    // --------------------------------------------------------
                    // Layer 2: Taskbar
                    // --------------------------------------------------------
                    let taskbar_y = height - 40;
                    if ty + tile_h > taskbar_y {
                        for y in 0..tile_h {
                            let abs_y = ty + y;
                            if abs_y >= taskbar_y {
                                for x in 0..tile_w {
                                    tile_buffer[y * TILE_SIZE + x] = Theme::TASKBAR_BG.to_u32();
                                }
                            }
                        }
                    }
                    
                    // --------------------------------------------------------
                    // Layer 3: Windows (Z-Order: alttan üste)
                    // --------------------------------------------------------
                    for window in desktop.windows() {
                        let win_right = window.x + window.width;
                        let win_bottom = window.y + window.height;
                        
                        // Tile ile pencere kesişiyor mu?
                        if window.x < tx + tile_w && win_right > tx &&
                           window.y < ty + tile_h && win_bottom > ty 
                        {
                            // Kesişim bölgesini hesapla
                            let ix_start = core::cmp::max(window.x, tx);
                            let ix_end = core::cmp::min(win_right, tx + tile_w);
                            let iy_start = core::cmp::max(window.y, ty);
                            let iy_end = core::cmp::min(win_bottom, ty + tile_h);
                            
                            for iy in iy_start..iy_end {
                                for ix in ix_start..ix_end {
                                    // Tile-local koordinatlar
                                    let tile_loc_x = ix - tx;
                                    let tile_loc_y = iy - ty;
                                    
                                    // Window-local koordinatlar
                                    let win_loc_x = ix - window.x;
                                    let win_loc_y = iy - window.y;
                                    
                                    // Piksel rengi belirleme
                                    let color = if win_loc_y < window.titlebar_height {
                                        // Titlebar
                                        if window.is_active { 
                                            Theme::TITLEBAR_ACTIVE.to_u32() 
                                        } else { 
                                            Theme::TITLEBAR_BG.to_u32() 
                                        }
                                    } else if win_loc_x == 0 || win_loc_x == window.width - 1 || 
                                              win_loc_y == window.height - 1 {
                                        // Border
                                        Theme::BORDER.to_u32()
                                    } else {
                                        // Window content area
                                        Theme::WINDOW_BG.to_u32()
                                    };
                                    
                                    tile_buffer[tile_loc_y * TILE_SIZE + tile_loc_x] = color;
                                }
                            }
                        }
                    }
                    
                    // --------------------------------------------------------
                    // Layer 4: Mouse Cursor
                    // --------------------------------------------------------
                    let cx = mx as usize;
                    let cy = my as usize;
                    
                    // Cursor tile ile kesişiyor mu? (12x19 piksel cursor)
                    if cx < tx + tile_w && cx + 12 > tx &&
                       cy < ty + tile_h && cy + 19 > ty 
                    {
                        for cy_off in 0..19 {
                            for cx_off in 0..12 {
                                if cx + cx_off >= tx && cx + cx_off < tx + tile_w &&
                                   cy + cy_off >= ty && cy + cy_off < ty + tile_h 
                                {
                                    let tlx = (cx + cx_off) - tx;
                                    let tly = (cy + cy_off) - ty;
                                    tile_buffer[tly * TILE_SIZE + tlx] = 0xFFFFFF; // Beyaz cursor
                                }
                            }
                        }
                    }

                    // --------------------------------------------------------
                    // FLUSH: Tile'ı Framebuffer'a yaz
                    // --------------------------------------------------------
                    for y in 0..tile_h {
                        for x in 0..tile_w {
                            let color = tile_buffer[y * TILE_SIZE + x];
                            fb.plot_pixel(tx + x, ty + y, color);
                        }
                    }
                }
            }
        }

        // --------------------------------------------------------------------
        // 4. IDLE (CPU'yu dinlendir)
        // --------------------------------------------------------------------
        core::hint::spin_loop();
    }
}
