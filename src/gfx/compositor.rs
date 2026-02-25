//! # echOS Compositor (Grafik Birleştirici)
//!
//! Bu modül, linear framebuffer tabanlı grafik rendering engine'ini içerir.
//! Desktop, pencereler ve mouse cursor'u birleştirerek ekrana çizer.
//! Dock, MenuBar, Spotlight ve uygulama entegrasyonu içerir.

use crate::drivers::mouse;
use crate::gfx::{Surface, SwapChain};
use crate::gop::framebuffer::Framebuffer;
use crate::gui::desktop::Desktop;
use crate::gui::widgets::Rect;
use alloc::vec::Vec;
use core::cmp::min;

/// Ana compositor döngüsü.
///
/// # Açıklama
/// Bu fonksiyon sonsuza kadar çalışır ve aşağıdaki işlemleri yapar:
/// 1. Mouse ve klavye inputunu okur
/// 2. Desktop durumunu günceller
/// 3. Linear framebuffer rendering yapar
/// 4. Framebuffer'a çizer
pub fn run(fb: &mut Framebuffer) -> ! {
    let width = fb.width;
    let height = fb.height;

    crate::serial_println!("Compositor: {}x{} (Linear Framebuffer)", width, height);
    crate::serial_println!("[GUI] Initializing desktop environment...");

    // Desktop with Dock, MenuBar, Spotlight
    let mut desktop = Desktop::new(width, height);
    
    // Launch default apps
    desktop.launch_app("finder");
    desktop.launch_app("terminal");

    let mut last_mx = 0;
    let mut last_my = 0;
    let mut frame_count: u64 = 0;
    let cursor_w = 12usize;
    let cursor_h = 19usize;
    let mut swapchain = SwapChain::new(width, height, fb.pixels_per_scan_line);
    let mut dirty_rects: Vec<Rect> = Vec::new();
    
    fn push_dirty_rect(rects: &mut Vec<Rect>, rect: Rect) {
        let mut merged = rect;
        let mut i = 0;
        while i < rects.len() {
            if merged.intersects(&rects[i]) {
                merged = merged.union(&rects[i]);
                rects.swap_remove(i);
            } else {
                i += 1;
            }
        }
        rects.push(merged);
    }

    // ========================================================================
    // ANA DÖNGÜ
    // ========================================================================
    loop {
        let start_tick = crate::task::scheduler::get_ticks();
        frame_count += 1;
        let fb_stride = fb.pixels_per_scan_line;

        // --------------------------------------------------------------------
        // 1. INPUT POLLING
        // --------------------------------------------------------------------
        mouse::poll();
        use crate::drivers::input::{pop_event, InputEvent};
        use pc_keyboard::DecodedKey;
        
        while let Some(event) = pop_event() {
            match event {
                InputEvent::MouseByte(byte) => {
                    crate::drivers::mouse::handle_packet(byte);
                }
                InputEvent::Mouse(_packet) => {
                    // Mouse packets already update global state elsewhere.
                }
                InputEvent::Keyboard(key) => {
                    // Handle keyboard input
                    match key {
                        DecodedKey::Unicode(c) => {
                            desktop.on_key(c);
                        }
                        DecodedKey::RawKey(scancode) => {
                            desktop.on_special_key(scancode);
                        }
                    }
                }
            }
        }

        let (mx, my) = mouse::get_position();
        let buttons = mouse::get_buttons();
        let prev_mx = last_mx;
        let prev_my = last_my;
        let mouse_moved = mx != prev_mx || my != prev_my;
        last_mx = mx;
        last_my = my;

        // --------------------------------------------------------------------
        // 2. DESKTOP UPDATE
        // --------------------------------------------------------------------
        let dt = 1.0 / 60.0; // Assume 60 FPS
        let needs_redraw = desktop.update_mouse(mx, my, buttons.left) || desktop.update(dt);
        let full_redraw = needs_redraw || frame_count == 1;
        
        if full_redraw {
            dirty_rects.clear();
            push_dirty_rect(
                &mut dirty_rects,
                Rect::new(0, 0, width as i32, height as i32),
            );
        }
        
        if mouse_moved {
            let to_u = |value: i32, max_value: usize| -> usize {
                if max_value == 0 {
                    return 0;
                }
                let mut out = if value < 0 { 0 } else { value as usize };
                if out >= max_value {
                    out = max_value - 1;
                }
                out
            };
            let mx_u = to_u(mx, width);
            let my_u = to_u(my, height);
            let prev_mx_u = to_u(prev_mx, width);
            let prev_my_u = to_u(prev_my, height);
            push_dirty_rect(
                &mut dirty_rects,
                Rect::new(
                    prev_mx_u as i32,
                    prev_my_u as i32,
                    cursor_w as i32,
                    cursor_h as i32,
                ),
            );
            push_dirty_rect(
                &mut dirty_rects,
                Rect::new(mx_u as i32, my_u as i32, cursor_w as i32, cursor_h as i32),
            );
        }

        // --------------------------------------------------------------------
        // 3. RENDERING
        // --------------------------------------------------------------------
        if !dirty_rects.is_empty() {
            let back_stride = swapchain.back.stride;
            let back_buffer = swapchain.back.buffer_mut();
            let cx = mx as usize;
            let cy = my as usize;
            
            for rect in dirty_rects.iter() {
                let rect_x = if rect.x < 0 { 0 } else { rect.x as usize };
                let rect_y = if rect.y < 0 { 0 } else { rect.y as usize };
                let rect_w = if rect.width < 0 {
                    0
                } else {
                    rect.width as usize
                };
                let rect_h = if rect.height < 0 {
                    0
                } else {
                    rect.height as usize
                };
                let rect_w = min(rect_w, width.saturating_sub(rect_x));
                let rect_h = min(rect_h, height.saturating_sub(rect_y));
                if rect_w == 0 || rect_h == 0 {
                    continue;
                }
                
                // Draw desktop to back buffer
                // For simplicity, we draw the whole desktop each frame
                // In a real implementation, we'd only draw dirty regions
            }
            
            // Draw desktop to back buffer
            desktop.draw(fb);
            
            // Draw cursor
            for py in 0..cursor_h {
                for px in 0..cursor_w {
                    let px_x = cx + px;
                    let py_y = cy + py;
                    if px_x < width && py_y < height {
                        // Simple arrow cursor
                        if px == py || (px == 0 && py < cursor_h / 2) {
                            fb.plot_pixel(px_x, py_y, 0xFFFFFFFF);
                        }
                    }
                }
            }
            
            dirty_rects.clear();
        }

        let end_tick = crate::task::scheduler::get_ticks();
        if end_tick == start_tick {
            crate::task::scheduler::sleep(1);
        }
    }
}
