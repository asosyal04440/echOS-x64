//! # echOS Compositor (Grafik Birleştirici)
//!
//! Bu modül, linear framebuffer tabanlı grafik rendering engine'ini içerir.
//! Desktop, pencereler ve mouse cursor'u birleştirerek ekrana çizer.
//! Dock, MenuBar, Spotlight ve uygulama entegrasyonu içerir.

use crate::drivers::mouse;
use crate::gfx::SwapChain;
use crate::gop::framebuffer::Framebuffer;
use crate::gui::desktop::Desktop;
use crate::gui::widgets::Rect;
use alloc::vec::Vec;
use core::cmp::Reverse;

/// Ana compositor döngüsü.
///
/// # Açıklama
/// Bu fonksiyon sonsuza kadar çalışır ve aşağıdaki işlemleri yapar:
/// 1. Mouse ve klavye inputunu okur
/// 2. Desktop durumunu günceller
/// 3. Linear framebuffer rendering yapar
/// 4. Framebuffer'a çizer
pub fn run(fb: &mut Framebuffer) -> ! {
    const STATS_INTERVAL_FRAMES: u64 = 300;
    const DIRTY_FULL_REDRAW_THRESHOLD_PERCENT: u64 = 60;
    const DIRTY_FULL_REDRAW_MIN_THRESHOLD_PERCENT: u64 = 36;
    const DIRTY_PARTIAL_BUDGET_PERCENT: u64 = 30;
    const DIRTY_PARTIAL_BUDGET_MIN_PERCENT: u64 = 20;
    const DIRTY_PARTIAL_BUDGET_MAX_PERCENT: u64 = 45;
    const SCENE_DAMAGE_BURST_PROMOTE_THRESHOLD: u64 = 6;
    const DIRTY_RECT_COUNT_FORCE_FULL: usize = 24;
    const MAX_PARTIAL_RECTS: usize = 12;
    const MIN_PARTIAL_RECTS: usize = 6;
    const MAX_PARTIAL_RECTS_DYNAMIC: usize = 16;
    const TINY_RECT_AREA_CUTOFF: u64 = 16;
    const FRAME_BUDGET_TICKS: u64 = 2;
    const LATENCY_FORCE_FULL_STREAK: u64 = 4;

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
    let mut stats_full_redraw_frames: u64 = 0;
    let mut stats_partial_restore_frames: u64 = 0;
    let mut stats_scene_damage_frames: u64 = 0;
    let mut stats_forced_full_redraw_frames: u64 = 0;
    let mut stats_rendered_frames: u64 = 0;
    let mut stats_dirty_rects_total: u64 = 0;
    let mut stats_dirty_pixels: u64 = 0;
    let mut stats_cursor_pixels: u64 = 0;
    let mut stats_min_dirty_pixels: u64 = u64::MAX;
    let mut stats_max_dirty_pixels: u64 = 0;
    let mut stats_total_frame_ticks: u64 = 0;
    let mut stats_scheduler_yields: u64 = 0;
    let mut stats_clamped_rects: u64 = 0;
    let mut stats_overflow_merged_rects: u64 = 0;
    let mut stats_adaptive_threshold_sum: u64 = 0;
    let mut stats_budget_forced_full_redraw_frames: u64 = 0;
    let mut stats_tiny_rects_culled: u64 = 0;
    let mut stats_dynamic_budget_sum: u64 = 0;
    let mut stats_dynamic_rect_limit_sum: u64 = 0;
    let mut stats_priority_score_sum: u64 = 0;
    let mut stats_priority_rect_count: u64 = 0;
    let mut stats_scene_burst_mode_frames: u64 = 0;
    let mut stats_scene_mode_frames: u64 = 0;
    let mut stats_cursor_mode_frames: u64 = 0;
    let mut stats_balanced_mode_frames: u64 = 0;
    let mut stats_idle_frames: u64 = 0;
    let mut stats_latency_pressure_frames: u64 = 0;
    let mut stats_latency_forced_full_redraw_frames: u64 = 0;
    let mut interval_frame_ticks: Vec<u64> = Vec::new();
    let mut scene_damage_burst_streak: u64 = 0;
    let mut latency_pressure_streak: u64 = 0;

    fn rects_are_nearby(a: &Rect, b: &Rect, padding: i32) -> bool {
        let ax1 = a.x;
        let ay1 = a.y;
        let ax2 = a.x.saturating_add(a.width);
        let ay2 = a.y.saturating_add(a.height);

        let bx1 = b.x;
        let by1 = b.y;
        let bx2 = b.x.saturating_add(b.width);
        let by2 = b.y.saturating_add(b.height);

        let expanded_ax1 = ax1.saturating_sub(padding);
        let expanded_ay1 = ay1.saturating_sub(padding);
        let expanded_ax2 = ax2.saturating_add(padding);
        let expanded_ay2 = ay2.saturating_add(padding);

        expanded_ax1 < bx2 && expanded_ax2 > bx1 && expanded_ay1 < by2 && expanded_ay2 > by1
    }
    
    fn push_dirty_rect(rects: &mut Vec<Rect>, rect: Rect) {
        if rect.width <= 0 || rect.height <= 0 {
            return;
        }

        let mut merged = rect;
        let mut i = 0;
        while i < rects.len() {
            if merged.intersects(&rects[i]) || rects_are_nearby(&merged, &rects[i], 2) {
                merged = merged.union(&rects[i]);
                rects.swap_remove(i);
            } else {
                i += 1;
            }
        }
        rects.push(merged);
    }

    fn clamp_rect_to_screen(
        rect: &Rect,
        width: usize,
        height: usize,
    ) -> Option<(usize, usize, usize, usize, bool)> {
        let rect_x = if rect.x < 0 { 0 } else { rect.x as usize };
        let rect_y = if rect.y < 0 { 0 } else { rect.y as usize };
        let rect_w = if rect.width < 0 { 0 } else { rect.width as usize };
        let rect_h = if rect.height < 0 { 0 } else { rect.height as usize };

        let was_clamped = rect.x < 0
            || rect.y < 0
            || rect_w != rect.width.max(0) as usize
            || rect_h != rect.height.max(0) as usize;

        let rect_w = rect_w.min(width.saturating_sub(rect_x));
        let rect_h = rect_h.min(height.saturating_sub(rect_y));
        if rect_w == 0 || rect_h == 0 {
            return None;
        }

        Some((rect_x, rect_y, rect_w, rect_h, was_clamped))
    }

    fn rect_area(rect: &Rect) -> u64 {
        let width = if rect.width < 0 { 0 } else { rect.width as u64 };
        let height = if rect.height < 0 { 0 } else { rect.height as u64 };
        width.saturating_mul(height)
    }

    fn dirty_rect_priority_score(
        rect: &Rect,
        active_window_rect: Option<&Rect>,
        cursor_rect: &Rect,
        age_rank: usize,
        total_rects: usize,
    ) -> u64 {
        let area_score = rect_area(rect) / 128;
        let active_bonus = if active_window_rect
            .map(|active| rect.intersects(active))
            .unwrap_or(false)
        {
            120
        } else {
            0
        };
        let cursor_bonus = if rect.intersects(cursor_rect) { 40 } else { 0 };
        let age_score = total_rects.saturating_sub(age_rank) as u64;

        area_score
            .saturating_add(active_bonus)
            .saturating_add(cursor_bonus)
            .saturating_add(age_score)
    }

    fn dynamic_partial_tuning(
        has_scene_damage: bool,
        mouse_moved: bool,
        scene_damage_burst_streak: u64,
        latency_pressure_streak: u64,
    ) -> (u64, usize, u8) {
        let mut budget = DIRTY_PARTIAL_BUDGET_PERCENT;
        let mut rect_limit = MAX_PARTIAL_RECTS;
        let mut mode = 0u8;

        if has_scene_damage {
            budget = budget.saturating_sub(8);
            rect_limit = rect_limit.saturating_sub(2);
            mode = 1;
        } else if mouse_moved {
            budget = budget.saturating_add(8);
            rect_limit = rect_limit.saturating_add(2);
            mode = 2;
        }

        if scene_damage_burst_streak >= SCENE_DAMAGE_BURST_PROMOTE_THRESHOLD {
            budget = budget.saturating_sub(4);
            rect_limit = rect_limit.saturating_sub(1);
            mode = 3;
        }

        if latency_pressure_streak > 0 {
            let pressure = latency_pressure_streak.min(6);
            budget = budget.saturating_sub(pressure);
            rect_limit = rect_limit.saturating_sub((pressure / 2) as usize + 1);
            mode = 4;
        }

        let clamped_budget = budget.clamp(
            DIRTY_PARTIAL_BUDGET_MIN_PERCENT,
            DIRTY_PARTIAL_BUDGET_MAX_PERCENT,
        );
        let clamped_rect_limit = rect_limit.clamp(MIN_PARTIAL_RECTS, MAX_PARTIAL_RECTS_DYNAMIC);
        (clamped_budget, clamped_rect_limit, mode)
    }

    // ========================================================================
    // ANA DÖNGÜ
    // ========================================================================
    loop {
        let start_tick = crate::task::scheduler::get_ticks();
        frame_count += 1;
        let mut input_needs_redraw = false;

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
                            input_needs_redraw |= desktop.on_key(c);
                        }
                        DecodedKey::RawKey(scancode) => {
                            input_needs_redraw |= desktop.on_special_key(scancode);
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
        let needs_redraw = input_needs_redraw
            || desktop.update_mouse(mx, my, buttons.left)
            || desktop.update(dt);
        let mut has_scene_damage = false;
        if let Some(damages) = desktop.take_window_damage() {
            for damage in damages {
                push_dirty_rect(&mut dirty_rects, damage);
            }
            has_scene_damage = true;
        }

        if has_scene_damage {
            scene_damage_burst_streak = scene_damage_burst_streak.saturating_add(1);
        } else {
            scene_damage_burst_streak = 0;
        }

        let full_redraw = frame_count == 1 || (needs_redraw && !has_scene_damage);
        
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

        let mut effective_full_redraw = full_redraw;
        let mut pending_dirty_pixels: u64 = 0;
        let mut dynamic_partial_budget_percent = DIRTY_PARTIAL_BUDGET_PERCENT;
        let mut dynamic_partial_rect_limit = MAX_PARTIAL_RECTS;
        if !effective_full_redraw && !dirty_rects.is_empty() {
            let (budget, rect_limit, mode) = dynamic_partial_tuning(
                has_scene_damage,
                mouse_moved,
                scene_damage_burst_streak,
                latency_pressure_streak,
            );
            dynamic_partial_budget_percent = budget;
            dynamic_partial_rect_limit = rect_limit;
            match mode {
                3 => stats_scene_burst_mode_frames = stats_scene_burst_mode_frames.saturating_add(1),
                4 => stats_latency_pressure_frames = stats_latency_pressure_frames.saturating_add(1),
                1 => stats_scene_mode_frames = stats_scene_mode_frames.saturating_add(1),
                2 => stats_cursor_mode_frames = stats_cursor_mode_frames.saturating_add(1),
                _ => stats_balanced_mode_frames = stats_balanced_mode_frames.saturating_add(1),
            }

            if latency_pressure_streak >= LATENCY_FORCE_FULL_STREAK {
                effective_full_redraw = true;
                dirty_rects.clear();
                push_dirty_rect(
                    &mut dirty_rects,
                    Rect::new(0, 0, width as i32, height as i32),
                );
                stats_forced_full_redraw_frames += 1;
                stats_latency_forced_full_redraw_frames += 1;
            }

            stats_dynamic_budget_sum =
                stats_dynamic_budget_sum.saturating_add(dynamic_partial_budget_percent);
            stats_dynamic_rect_limit_sum =
                stats_dynamic_rect_limit_sum.saturating_add(dynamic_partial_rect_limit as u64);

            let before_retain = dirty_rects.len();
            dirty_rects.retain(|rect| rect_area(rect) > TINY_RECT_AREA_CUTOFF);
            stats_tiny_rects_culled = stats_tiny_rects_culled
                .saturating_add(before_retain.saturating_sub(dirty_rects.len()) as u64);

            if dirty_rects.is_empty() {
                continue;
            }

            if dirty_rects.len() >= DIRTY_RECT_COUNT_FORCE_FULL {
                effective_full_redraw = true;
                dirty_rects.clear();
                push_dirty_rect(
                    &mut dirty_rects,
                    Rect::new(0, 0, width as i32, height as i32),
                );
                stats_forced_full_redraw_frames += 1;
            }

            for rect in dirty_rects.iter() {
                if let Some((_, _, rect_w, rect_h, _)) = clamp_rect_to_screen(rect, width, height) {
                    pending_dirty_pixels += (rect_w.saturating_mul(rect_h)) as u64;
                }
            }

            let screen_pixels = (width.saturating_mul(height)) as u64;
            let adaptive_penalty = (dirty_rects.len() as u64).min(20);
            let adaptive_threshold_percent = DIRTY_FULL_REDRAW_THRESHOLD_PERCENT
                .saturating_sub(adaptive_penalty)
                .max(DIRTY_FULL_REDRAW_MIN_THRESHOLD_PERCENT);
            stats_adaptive_threshold_sum = stats_adaptive_threshold_sum.saturating_add(adaptive_threshold_percent);
            if screen_pixels > 0
                && pending_dirty_pixels * 100 >= screen_pixels * adaptive_threshold_percent
            {
                effective_full_redraw = true;
                dirty_rects.clear();
                push_dirty_rect(
                    &mut dirty_rects,
                    Rect::new(0, 0, width as i32, height as i32),
                );
                stats_forced_full_redraw_frames += 1;
            }

            if !effective_full_redraw
                && screen_pixels > 0
                && dirty_rects.len() >= (dynamic_partial_rect_limit / 2)
                && pending_dirty_pixels * 100 >= screen_pixels * dynamic_partial_budget_percent
            {
                effective_full_redraw = true;
                dirty_rects.clear();
                push_dirty_rect(
                    &mut dirty_rects,
                    Rect::new(0, 0, width as i32, height as i32),
                );
                stats_forced_full_redraw_frames += 1;
                stats_budget_forced_full_redraw_frames += 1;
            }
        }

        if !effective_full_redraw && dirty_rects.len() > 1 {
            let cursor_rect = Rect::new(mx, my, cursor_w as i32, cursor_h as i32);
            let active_window_rect = desktop.active_window_rect();
            let total_rects = dirty_rects.len();
            let mut weighted_rects: Vec<(Rect, u64)> = dirty_rects
                .iter()
                .enumerate()
                .map(|(idx, rect)| {
                    let score = dirty_rect_priority_score(
                        rect,
                        active_window_rect.as_ref(),
                        &cursor_rect,
                        idx,
                        total_rects,
                    );
                    (*rect, score)
                })
                .collect();
            weighted_rects.sort_by_key(|(_, score)| Reverse(*score));
            stats_priority_score_sum = stats_priority_score_sum
                .saturating_add(weighted_rects.iter().map(|(_, score)| *score).sum::<u64>());
            stats_priority_rect_count =
                stats_priority_rect_count.saturating_add(weighted_rects.len() as u64);
            dirty_rects.clear();
            dirty_rects.extend(weighted_rects.into_iter().map(|(rect, _)| rect));
        }

        if !effective_full_redraw && dirty_rects.len() > dynamic_partial_rect_limit {
            let mut overflow_rect: Option<Rect> = None;
            let overflow_count = dirty_rects
                .len()
                .saturating_sub(dynamic_partial_rect_limit.saturating_sub(1)) as u64;
            for rect in dirty_rects.drain(dynamic_partial_rect_limit.saturating_sub(1)..) {
                overflow_rect = Some(match overflow_rect {
                    Some(prev) => prev.union(&rect),
                    None => rect,
                });
            }
            stats_overflow_merged_rects = stats_overflow_merged_rects.saturating_add(overflow_count);

            if let Some(overflow) = overflow_rect {
                push_dirty_rect(&mut dirty_rects, overflow);
            }
        }

        // --------------------------------------------------------------------
        // 3. RENDERING
        // --------------------------------------------------------------------
        if !dirty_rects.is_empty() {
            stats_rendered_frames += 1;
            if effective_full_redraw {
                stats_full_redraw_frames += 1;
            } else {
                stats_partial_restore_frames += 1;
            }
            if has_scene_damage {
                stats_scene_damage_frames += 1;
            }

            let cx = if mx < 0 { 0 } else { mx as usize };
            let cy = if my < 0 { 0 } else { my as usize };
            let mut frame_dirty_pixels: u64 = 0;
            let mut frame_cursor_pixels: u64 = 0;

            let frame_dirty_rects = dirty_rects.len() as u64;
            stats_dirty_rects_total += frame_dirty_rects;

            if effective_full_redraw {
                desktop.draw(fb);
                let fb_buffer = fb.buffer_mut();
                let cached_front = swapchain.front.buffer_mut();
                cached_front.copy_from_slice(fb_buffer);
                frame_dirty_pixels = (width.saturating_mul(height)) as u64;
            } else if has_scene_damage {
                desktop.draw(fb);

                let stride = fb.pixels_per_scan_line;
                let fb_buffer = fb.buffer_mut();
                let cached_front = swapchain.front.buffer_mut();

                for rect in dirty_rects.iter() {
                    let Some((rect_x, rect_y, rect_w, rect_h, was_clamped)) =
                        clamp_rect_to_screen(rect, width, height)
                    else {
                        continue;
                    };
                    if was_clamped {
                        stats_clamped_rects = stats_clamped_rects.saturating_add(1);
                    }
                    frame_dirty_pixels += (rect_w.saturating_mul(rect_h)) as u64;

                    for row in 0..rect_h {
                        let y = rect_y + row;
                        let start = y * stride + rect_x;
                        let end = start + rect_w;
                        cached_front[start..end].copy_from_slice(&fb_buffer[start..end]);
                    }
                }
            } else {
                let stride = fb.pixels_per_scan_line;
                let fb_buffer = fb.buffer_mut();
                let cached_front = swapchain.front.buffer.as_slice();

                for rect in dirty_rects.iter() {
                    let Some((rect_x, rect_y, rect_w, rect_h, was_clamped)) =
                        clamp_rect_to_screen(rect, width, height)
                    else {
                        continue;
                    };
                    if was_clamped {
                        stats_clamped_rects = stats_clamped_rects.saturating_add(1);
                    }
                    frame_dirty_pixels += (rect_w.saturating_mul(rect_h)) as u64;

                    for row in 0..rect_h {
                        let y = rect_y + row;
                        let start = y * stride + rect_x;
                        let end = start + rect_w;
                        fb_buffer[start..end].copy_from_slice(&cached_front[start..end]);
                    }
                }
            }

            let stride = fb.pixels_per_scan_line;
            let fb_buffer = fb.buffer_mut();
            for py in 0..cursor_h {
                for px in 0..cursor_w {
                    if !(px == py || (px == 0 && py < cursor_h / 2)) {
                        continue;
                    }
                    let px_x = cx + px;
                    let py_y = cy + py;
                    if px_x < width && py_y < height {
                        let idx = py_y * stride + px_x;
                        fb_buffer[idx] = 0xFFFFFFFF;
                        frame_cursor_pixels += 1;
                    }
                }
            }

            stats_dirty_pixels += frame_dirty_pixels;
            stats_cursor_pixels += frame_cursor_pixels;
            if frame_dirty_pixels < stats_min_dirty_pixels {
                stats_min_dirty_pixels = frame_dirty_pixels;
            }
            if frame_dirty_pixels > stats_max_dirty_pixels {
                stats_max_dirty_pixels = frame_dirty_pixels;
            }

            dirty_rects.clear();
        } else {
            stats_idle_frames = stats_idle_frames.saturating_add(1);
        }

        if frame_count % STATS_INTERVAL_FRAMES == 0 {
            let total_redraw_frames = stats_full_redraw_frames + stats_partial_restore_frames;
            let avg_dirty_px = if stats_rendered_frames > 0 {
                stats_dirty_pixels / stats_rendered_frames
            } else {
                0
            };
            let avg_cursor_px = if stats_rendered_frames > 0 {
                stats_cursor_pixels / stats_rendered_frames
            } else {
                0
            };
            let partial_ratio_percent = if total_redraw_frames > 0 {
                (stats_partial_restore_frames * 100) / total_redraw_frames
            } else {
                0
            };
            let scene_damage_ratio_percent = if stats_rendered_frames > 0 {
                (stats_scene_damage_frames * 100) / stats_rendered_frames
            } else {
                0
            };
            let avg_dirty_rects = if stats_rendered_frames > 0 {
                stats_dirty_rects_total / stats_rendered_frames
            } else {
                0
            };
            let avg_adaptive_threshold = if stats_rendered_frames > 0 {
                stats_adaptive_threshold_sum / stats_rendered_frames
            } else {
                DIRTY_FULL_REDRAW_THRESHOLD_PERCENT
            };
            let avg_dynamic_budget = if stats_rendered_frames > 0 {
                stats_dynamic_budget_sum / stats_rendered_frames
            } else {
                DIRTY_PARTIAL_BUDGET_PERCENT
            };
            let avg_dynamic_rect_limit = if stats_rendered_frames > 0 {
                stats_dynamic_rect_limit_sum / stats_rendered_frames
            } else {
                MAX_PARTIAL_RECTS as u64
            };
            let avg_priority_score = if stats_priority_rect_count > 0 {
                stats_priority_score_sum / stats_priority_rect_count
            } else {
                0
            };
            let avg_frame_ticks = if STATS_INTERVAL_FRAMES > 0 {
                stats_total_frame_ticks / STATS_INTERVAL_FRAMES
            } else {
                0
            };
            let (p95_ticks, p99_ticks) = if interval_frame_ticks.is_empty() {
                (0, 0)
            } else {
                interval_frame_ticks.sort_unstable();
                let len = interval_frame_ticks.len();
                let p95_idx = ((len.saturating_sub(1)) * 95) / 100;
                let p99_idx = ((len.saturating_sub(1)) * 99) / 100;
                (interval_frame_ticks[p95_idx], interval_frame_ticks[p99_idx])
            };
            let min_dirty_px = if stats_rendered_frames > 0 {
                stats_min_dirty_pixels
            } else {
                0
            };

            crate::serial_println!(
                "[GUI][CompositorStats] frames={} rendered={} idle={} full={} partial={} forced_full={} budget_forced={} latency_forced={} scene_damage={} scene_damage_ratio={}%% partial_ratio={}%% avg_rects={} avg_dirty_px={} min_dirty_px={} max_dirty_px={} avg_cursor_px={} avg_ticks={} p95_ticks={} p99_ticks={} yields={} clamped_rects={} overflow_merged={} tiny_culled={} avg_adapt_thresh={}%% avg_dyn_budget={}%% avg_dyn_rect_limit={} avg_priority_score={} mode_balanced={} mode_cursor={} mode_scene={} mode_scene_burst={} mode_latency_pressure={} latency_streak={}",
                STATS_INTERVAL_FRAMES,
                stats_rendered_frames,
                stats_idle_frames,
                stats_full_redraw_frames,
                stats_partial_restore_frames,
                stats_forced_full_redraw_frames,
                stats_budget_forced_full_redraw_frames,
                stats_latency_forced_full_redraw_frames,
                stats_scene_damage_frames,
                scene_damage_ratio_percent,
                partial_ratio_percent,
                avg_dirty_rects,
                avg_dirty_px,
                min_dirty_px,
                stats_max_dirty_pixels,
                avg_cursor_px,
                avg_frame_ticks,
                p95_ticks,
                p99_ticks,
                stats_scheduler_yields,
                stats_clamped_rects,
                stats_overflow_merged_rects,
                stats_tiny_rects_culled,
                avg_adaptive_threshold,
                avg_dynamic_budget,
                avg_dynamic_rect_limit,
                avg_priority_score,
                stats_balanced_mode_frames,
                stats_cursor_mode_frames,
                stats_scene_mode_frames,
                stats_scene_burst_mode_frames,
                stats_latency_pressure_frames,
                latency_pressure_streak
            );

            stats_full_redraw_frames = 0;
            stats_partial_restore_frames = 0;
            stats_scene_damage_frames = 0;
            stats_forced_full_redraw_frames = 0;
            stats_rendered_frames = 0;
            stats_dirty_rects_total = 0;
            stats_dirty_pixels = 0;
            stats_cursor_pixels = 0;
            stats_min_dirty_pixels = u64::MAX;
            stats_max_dirty_pixels = 0;
            stats_total_frame_ticks = 0;
            stats_scheduler_yields = 0;
            stats_clamped_rects = 0;
            stats_overflow_merged_rects = 0;
            stats_adaptive_threshold_sum = 0;
            stats_budget_forced_full_redraw_frames = 0;
            stats_tiny_rects_culled = 0;
            stats_dynamic_budget_sum = 0;
            stats_dynamic_rect_limit_sum = 0;
            stats_priority_score_sum = 0;
            stats_priority_rect_count = 0;
            stats_scene_burst_mode_frames = 0;
            stats_scene_mode_frames = 0;
            stats_cursor_mode_frames = 0;
            stats_balanced_mode_frames = 0;
            stats_idle_frames = 0;
            stats_latency_pressure_frames = 0;
            stats_latency_forced_full_redraw_frames = 0;
            interval_frame_ticks.clear();
        }

        let end_tick = crate::task::scheduler::get_ticks();
        let frame_ticks = end_tick.wrapping_sub(start_tick) as u64;
        if frame_ticks > FRAME_BUDGET_TICKS {
            latency_pressure_streak = latency_pressure_streak.saturating_add(1);
        } else {
            latency_pressure_streak = latency_pressure_streak.saturating_sub(1);
        }
        interval_frame_ticks.push(frame_ticks);
        stats_total_frame_ticks = stats_total_frame_ticks.saturating_add(frame_ticks);
        if end_tick == start_tick {
            stats_scheduler_yields = stats_scheduler_yields.saturating_add(1);
            crate::task::scheduler::sleep(1);
        }
    }
}
