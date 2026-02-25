//! # Animation System
//!
//! High-performance animation timeline with easing functions
//! 60 FPS guaranteed with frame pacing

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use spin::Mutex;
use core::f32::consts::PI;
use libm::{sinf, cosf, powf, sqrtf};

// ============================================================================
// EASING TYPES
// ============================================================================

/// Easing function type
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EasingType {
    // Linear
    Linear,
    
    // Quadratic
    EaseInQuad,
    EaseOutQuad,
    EaseInOutQuad,
    
    // Cubic
    EaseInCubic,
    EaseOutCubic,
    EaseInOutCubic,
    
    // Quartic
    EaseInQuart,
    EaseOutQuart,
    EaseInOutQuart,
    
    // Quintic
    EaseInQuint,
    EaseOutQuint,
    EaseInOutQuint,
    
    // Sinusoidal
    EaseInSine,
    EaseOutSine,
    EaseInOutSine,
    
    // Exponential
    EaseInExpo,
    EaseOutExpo,
    EaseInOutExpo,
    
    // Circular
    EaseInCirc,
    EaseOutCirc,
    EaseInOutCirc,
    
    // Elastic
    EaseInElastic,
    EaseOutElastic,
    EaseInOutElastic,
    
    // Back
    EaseInBack,
    EaseOutBack,
    EaseInOutBack,
    
    // Bounce
    EaseInBounce,
    EaseOutBounce,
    EaseInOutBounce,
}

// ============================================================================
// PRE-COMPUTED EASING CACHE
// ============================================================================

/// Number of sample points per easing curve
const EASING_SAMPLES: usize = 256;

/// Pre-computed easing function samples
/// This avoids runtime computation for common easing functions
struct EasingCache {
    samples: [[f32; EASING_SAMPLES]; 31], // 31 easing types
}

impl EasingCache {
    const fn new() -> Self {
        // Initialize with linear as default
        let mut samples = [[0.0f32; EASING_SAMPLES]; 31];
        
        let mut i = 0;
        while i < EASING_SAMPLES {
            let t = i as f32 / (EASING_SAMPLES - 1) as f32;
            samples[EasingType::Linear as usize][i] = t;
            i += 1;
        }
        
        EasingCache { samples }
    }
    
    fn compute_all(&mut self) {
        // Compute all easing function samples
        for i in 0..EASING_SAMPLES {
            let t = i as f32 / (EASING_SAMPLES - 1) as f32;
            
            self.samples[EasingType::Linear as usize][i] = t;
            
            // Quadratic
            self.samples[EasingType::EaseInQuad as usize][i] = t * t;
            self.samples[EasingType::EaseOutQuad as usize][i] = 1.0 - (1.0 - t) * (1.0 - t);
            self.samples[EasingType::EaseInOutQuad as usize][i] = {
                if t < 0.5 { 2.0 * t * t } else { 1.0 - powf(-2.0 * t + 2.0, 2.0) / 2.0 }
            };
            
            // Cubic
            self.samples[EasingType::EaseInCubic as usize][i] = t * t * t;
            self.samples[EasingType::EaseOutCubic as usize][i] = 1.0 - powf(1.0 - t, 3.0);
            self.samples[EasingType::EaseInOutCubic as usize][i] = {
                if t < 0.5 { 4.0 * t * t * t } else { 1.0 - powf(-2.0 * t + 2.0, 3.0) / 2.0 }
            };
            
            // Quartic
            self.samples[EasingType::EaseInQuart as usize][i] = t * t * t * t;
            self.samples[EasingType::EaseOutQuart as usize][i] = 1.0 - powf(1.0 - t, 4.0);
            self.samples[EasingType::EaseInOutQuart as usize][i] = {
                if t < 0.5 { 8.0 * powf(t, 4.0) } else { 1.0 - powf(-2.0 * t + 2.0, 4.0) / 2.0 }
            };
            
            // Quintic
            self.samples[EasingType::EaseInQuint as usize][i] = powf(t, 5.0);
            self.samples[EasingType::EaseOutQuint as usize][i] = 1.0 - powf(1.0 - t, 5.0);
            self.samples[EasingType::EaseInOutQuint as usize][i] = {
                if t < 0.5 { 16.0 * powf(t, 5.0) } else { 1.0 - powf(-2.0 * t + 2.0, 5.0) / 2.0 }
            };
            
            // Sinusoidal
            self.samples[EasingType::EaseInSine as usize][i] = 1.0 - cosf(t * PI / 2.0);
            self.samples[EasingType::EaseOutSine as usize][i] = sinf(t * PI / 2.0);
            self.samples[EasingType::EaseInOutSine as usize][i] = {
                -(cosf(PI * t) - 1.0) / 2.0
            };
            
            // Exponential
            self.samples[EasingType::EaseInExpo as usize][i] = {
                if t == 0.0 { 0.0 } else { powf(2.0_f32, 10.0 * t - 10.0) }
            };
            self.samples[EasingType::EaseOutExpo as usize][i] = {
                if t == 1.0 { 1.0 } else { 1.0 - powf(2.0_f32, -10.0 * t) }
            };
            self.samples[EasingType::EaseInOutExpo as usize][i] = {
                if t == 0.0 { 0.0 }
                else if t == 1.0 { 1.0 }
                else if t < 0.5 { powf(2.0_f32, 20.0 * t - 10.0) / 2.0 }
                else { (2.0 - powf(2.0_f32, -20.0 * t + 10.0)) / 2.0 }
            };
            
            // Circular
            self.samples[EasingType::EaseInCirc as usize][i] = 1.0 - sqrtf(1.0 - t * t);
            self.samples[EasingType::EaseOutCirc as usize][i] = sqrtf(1.0 - powf(t - 1.0, 2.0));
            self.samples[EasingType::EaseInOutCirc as usize][i] = {
                if t < 0.5 {
                    (1.0 - sqrtf(1.0 - powf(2.0 * t, 2.0))) / 2.0
                } else {
                    (sqrtf(1.0 - powf(-2.0 * t + 2.0, 2.0)) + 1.0) / 2.0
                }
            };
            
            // Back
            const C1: f32 = 1.70158;
            const C2: f32 = C1 * 1.525;
            const C3: f32 = C1 + 1.0;
            
            self.samples[EasingType::EaseInBack as usize][i] = {
                C3 * t * t * t - C1 * t * t
            };
            self.samples[EasingType::EaseOutBack as usize][i] = {
                1.0 + C3 * powf(t - 1.0, 3.0) + C1 * powf(t - 1.0, 2.0)
            };
            self.samples[EasingType::EaseInOutBack as usize][i] = {
                if t < 0.5 {
                    (powf(2.0 * t, 2.0) * ((C2 + 1.0) * 2.0 * t - C2)) / 2.0
                } else {
                    (powf(2.0 * t - 2.0, 2.0) * ((C2 + 1.0) * (t * 2.0 - 2.0) + C2) + 2.0) / 2.0
                }
            };
            
            // Elastic
            const C4: f32 = (2.0 * PI) / 3.0;
            const C5: f32 = (2.0 * PI) / 4.5;
            
            self.samples[EasingType::EaseInElastic as usize][i] = {
                if t == 0.0 { 0.0 }
                else if t == 1.0 { 1.0 }
                else {
                    -powf(2.0_f32, 10.0 * t - 10.0) * sinf((t * 10.0 - 10.75) * C4)
                }
            };
            self.samples[EasingType::EaseOutElastic as usize][i] = {
                if t == 0.0 { 0.0 }
                else if t == 1.0 { 1.0 }
                else {
                    powf(2.0_f32, -10.0 * t) * sinf((t * 10.0 - 0.75) * C4) + 1.0
                }
            };
            self.samples[EasingType::EaseInOutElastic as usize][i] = {
                if t == 0.0 { 0.0 }
                else if t == 1.0 { 1.0 }
                else if t < 0.5 {
                    -(powf(2.0_f32, 20.0 * t - 10.0) * sinf((20.0 * t - 11.125) * C5)) / 2.0
                } else {
                    (powf(2.0_f32, -20.0 * t + 10.0) * sinf((20.0 * t - 11.125) * C5)) / 2.0 + 1.0
                }
            };
            
            // Bounce
            self.samples[EasingType::EaseOutBounce as usize][i] = {
                const N1: f32 = 7.5625;
                const D1: f32 = 2.75;
                
                if t < 1.0 / D1 {
                    N1 * t * t
                } else if t < 2.0 / D1 {
                    N1 * powf(t - 1.5 / D1, 2.0) + 0.75
                } else if t < 2.5 / D1 {
                    N1 * powf(t - 2.25 / D1, 2.0) + 0.9375
                } else {
                    N1 * powf(t - 2.625 / D1, 2.0) + 0.984375
                }
            };
            self.samples[EasingType::EaseInBounce as usize][i] = {
                1.0 - self.samples[EasingType::EaseOutBounce as usize][(EASING_SAMPLES - 1 - i) as usize]
            };
            self.samples[EasingType::EaseInOutBounce as usize][i] = {
                if t < 0.5 {
                    (1.0 - self.samples[EasingType::EaseOutBounce as usize][(EASING_SAMPLES - 1 - 2 * i as usize).min(EASING_SAMPLES - 1)]) / 2.0
                } else {
                    (1.0 + self.samples[EasingType::EaseOutBounce as usize][(2 * i as usize - EASING_SAMPLES).max(0)]) / 2.0
                }
            };
        }
    }
}

// Global easing cache
lazy_static::lazy_static! {
    static ref EASING_CACHE: Mutex<EasingCache> = {
        let mut cache = EasingCache::new();
        cache.compute_all();
        Mutex::new(cache)
    };
}

/// Get easing value from cache (with interpolation)
pub fn get_easing(easing: EasingType, t: f32) -> f32 {
    let cache = EASING_CACHE.lock();
    
    // Clamp t to [0, 1]
    let t = t.max(0.0).min(1.0);
    
    // Get sample index
    let exact = t * (EASING_SAMPLES - 1) as f32;
    let idx = exact as usize;
    let frac = exact - idx as f32;
    
    let idx2 = (idx + 1).min(EASING_SAMPLES - 1);
    
    // Linear interpolation between samples
    let a = cache.samples[easing as usize][idx];
    let b = cache.samples[easing as usize][idx2];
    
    a + (b - a) * frac
}

// ============================================================================
// ANIMATION TARGET
// ============================================================================

/// Animation target identifier
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AnimationTarget {
    pub target_type: AnimationTargetType,
    pub id: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AnimationTargetType {
    WindowPosition,
    WindowSize,
    WindowOpacity,
    WidgetPosition,
    WidgetSize,
    WidgetOpacity,
    WidgetProperty,
    Custom,
}

// ============================================================================
// ANIMATION
// ============================================================================

/// Single animation instance
pub struct Animation {
    /// Target being animated
    pub target: AnimationTarget,
    /// Starting value
    pub start_value: f32,
    /// Ending value
    pub end_value: f32,
    /// Duration in seconds
    pub duration: f64,
    /// Elapsed time in seconds
    pub elapsed: f64,
    /// Easing function
    pub easing: EasingType,
    /// Current value
    pub current_value: f32,
    /// Is animation complete
    pub complete: bool,
    /// Callback when complete
    pub on_complete: Option<fn(&AnimationTarget)>,
    /// Delay before starting
    pub delay: f64,
    /// Is paused
    pub paused: bool,
    /// Loop mode
    pub loop_mode: LoopMode,
    /// Loop count (0 = infinite)
    pub loop_count: u32,
    /// Current loop iteration
    pub current_loop: u32,
    /// Play direction (for ping-pong)
    pub forward: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopMode {
    None,
    Loop,
    PingPong,
}

impl Animation {
    pub fn new(
        target: AnimationTarget,
        start_value: f32,
        end_value: f32,
        duration: f64,
        easing: EasingType,
    ) -> Self {
        Animation {
            target,
            start_value,
            end_value,
            duration,
            elapsed: 0.0,
            easing,
            current_value: start_value,
            complete: false,
            on_complete: None,
            delay: 0.0,
            paused: false,
            loop_mode: LoopMode::None,
            loop_count: 0,
            current_loop: 0,
            forward: true,
        }
    }
    
    /// Create position animation
    pub fn position(window_id: u32, start: f32, end: f32, duration: f64) -> Self {
        Animation::new(
            AnimationTarget { target_type: AnimationTargetType::WindowPosition, id: window_id },
            start, end, duration, EasingType::EaseOutCubic,
        )
    }
    
    /// Create size animation
    pub fn size(window_id: u32, start: f32, end: f32, duration: f64) -> Self {
        Animation::new(
            AnimationTarget { target_type: AnimationTargetType::WindowSize, id: window_id },
            start, end, duration, EasingType::EaseOutCubic,
        )
    }
    
    /// Create opacity animation (fade)
    pub fn opacity(window_id: u32, start: f32, end: f32, duration: f64) -> Self {
        Animation::new(
            AnimationTarget { target_type: AnimationTargetType::WindowOpacity, id: window_id },
            start, end, duration, EasingType::EaseOutSine,
        )
    }
    
    /// Set delay
    pub fn with_delay(mut self, delay: f64) -> Self {
        self.delay = delay;
        self
    }
    
    /// Set loop mode
    pub fn with_loop(mut self, mode: LoopMode, count: u32) -> Self {
        self.loop_mode = mode;
        self.loop_count = count;
        self
    }
    
    /// Set callback
    pub fn with_callback(mut self, callback: fn(&AnimationTarget)) -> Self {
        self.on_complete = Some(callback);
        self
    }
    
    /// Update animation
    pub fn update(&mut self, dt: f64) -> bool {
        if self.paused || self.complete {
            return false;
        }
        
        // Handle delay
        if self.delay > 0.0 {
            self.delay -= dt;
            return false;
        }
        
        // Update elapsed time
        self.elapsed += dt;
        
        // Check if complete
        if self.elapsed >= self.duration {
            if self.loop_mode != LoopMode::None {
                // Handle looping
                self.current_loop += 1;
                
                if self.loop_count > 0 && self.current_loop >= self.loop_count {
                    self.complete = true;
                    self.current_value = self.end_value;
                } else {
                    // Reset for next loop
                    if self.loop_mode == LoopMode::PingPong {
                        self.forward = !self.forward;
                        if self.forward {
                            self.elapsed = 0.0;
                            self.start_value = self.start_value;
                        } else {
                            core::mem::swap(&mut self.start_value, &mut self.end_value);
                            self.elapsed = 0.0;
                        }
                    } else {
                        self.elapsed = 0.0;
                    }
                }
            } else {
                self.complete = true;
                self.current_value = self.end_value;
            }
            
            if self.complete {
                if let Some(callback) = self.on_complete {
                    callback(&self.target);
                }
            }
            
            return true;
        }
        
        // Calculate current value
        let t = self.elapsed / self.duration;
        let eased = get_easing(self.easing, t as f32);
        self.current_value = self.start_value + (self.end_value - self.start_value) * eased;
        
        true
    }
    
    /// Get current value
    pub fn value(&self) -> f32 {
        self.current_value
    }
    
    /// Is animation running
    pub fn is_running(&self) -> bool {
        !self.paused && !self.complete && self.delay <= 0.0
    }
    
    /// Pause animation
    pub fn pause(&mut self) {
        self.paused = true;
    }
    
    /// Resume animation
    pub fn resume(&mut self) {
        self.paused = false;
    }
    
    /// Reset animation
    pub fn reset(&mut self) {
        self.elapsed = 0.0;
        self.complete = false;
        self.current_value = self.start_value;
        self.current_loop = 0;
        self.forward = true;
    }
}

// ============================================================================
// ANIMATION TIMELINE
// ============================================================================

/// Animation timeline manager
pub struct AnimationTimeline {
    /// Active animations
    animations: Vec<Animation>,
    /// Frame time in seconds
    frame_time: f64,
    /// Total time
    total_time: f64,
    /// Animation ID counter
    next_id: u64,
}

impl AnimationTimeline {
    pub fn new(target_fps: u32) -> Self {
        AnimationTimeline {
            animations: Vec::new(),
            frame_time: 1.0 / target_fps as f64,
            total_time: 0.0,
            next_id: 0,
        }
    }
    
    /// Add animation
    pub fn add(&mut self, animation: Animation) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.animations.push(animation);
        id
    }
    
    /// Remove animation by index
    pub fn remove(&mut self, target: &AnimationTarget) {
        self.animations.retain(|a| a.target != *target);
    }
    
    /// Clear all animations
    pub fn clear(&mut self) {
        self.animations.clear();
    }
    
    /// Update all animations
    pub fn update(&mut self, dt: f64) -> bool {
        let mut needs_redraw = false;
        
        self.total_time += dt;
        
        for animation in &mut self.animations {
            if animation.update(dt) {
                needs_redraw = true;
            }
        }
        
        // Remove completed animations
        self.animations.retain(|a| !a.complete);
        
        needs_redraw
    }
    
    /// Get animation value for target
    pub fn get_value(&self, target: &AnimationTarget) -> Option<f32> {
        for animation in &self.animations {
            if animation.target == *target {
                return Some(animation.current_value);
            }
        }
        None
    }
    
    /// Check if any animations are running
    pub fn is_animating(&self) -> bool {
        self.animations.iter().any(|a| a.is_running())
    }
    
    /// Get active animation count
    pub fn count(&self) -> usize {
        self.animations.len()
    }
    
    /// Pause all animations
    pub fn pause_all(&mut self) {
        for animation in &mut self.animations {
            animation.pause();
        }
    }
    
    /// Resume all animations
    pub fn resume_all(&mut self) {
        for animation in &mut self.animations {
            animation.resume();
        }
    }
}

// ============================================================================
// FRAME PACER
// ============================================================================

/// Frame pacing for consistent frame timing
pub struct FramePacer {
    /// Target frame time in nanoseconds
    target_frame_ns: u64,
    /// Last frame timestamp
    last_frame_ns: u64,
    /// Accumulated timing error
    accumulated_error_ns: i64,
    /// Frame count
    frame_count: u64,
    /// Total frame time for average
    total_frame_time_ns: u64,
}

impl FramePacer {
    pub fn new(target_fps: u32) -> Self {
        FramePacer {
            target_frame_ns: 1_000_000_000 / target_fps as u64,
            last_frame_ns: 0,
            accumulated_error_ns: 0,
            frame_count: 0,
            total_frame_time_ns: 0,
        }
    }
    
    /// Begin frame - call at start of frame
    pub fn begin_frame(&mut self) {
        self.last_frame_ns = get_time_ns();
    }
    
    /// End frame - wait for next frame timing
    pub fn end_frame(&mut self) {
        let now = get_time_ns();
        let elapsed = now - self.last_frame_ns;
        
        // Calculate target with error correction
        let target = self.target_frame_ns as i64 - self.accumulated_error_ns;
        let remaining = target - elapsed as i64;
        
        if remaining > 0 {
            // Sleep for most of the remaining time
            if remaining > 2_000_000 { // > 2ms
                sleep_ns((remaining - 1_000_000) as u64);
            }
            
            // Spin for precise timing
            while get_time_ns() - self.last_frame_ns < target as u64 {
                core::hint::spin_loop();
            }
        }
        
        // Update stats
        let actual_elapsed = get_time_ns() - self.last_frame_ns;
        self.accumulated_error_ns = actual_elapsed as i64 - target;
        
        // Clamp error to prevent runaway
        self.accumulated_error_ns = self.accumulated_error_ns
            .max(-(self.target_frame_ns as i64) / 2)
            .min(self.target_frame_ns as i64 / 2);
        
        self.frame_count += 1;
        self.total_frame_time_ns += actual_elapsed;
    }
    
    /// Get average frame time in milliseconds
    pub fn avg_frame_time_ms(&self) -> f64 {
        if self.frame_count == 0 {
            return 0.0;
        }
        self.total_frame_time_ns as f64 / self.frame_count as f64 / 1_000_000.0
    }
    
    /// Get current FPS
    pub fn current_fps(&self) -> f64 {
        let avg = self.avg_frame_time_ms();
        if avg > 0.0 {
            1000.0 / avg
        } else {
            0.0
        }
    }
    
    /// Get frame count
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }
}

// ============================================================================
// TIME UTILITIES
// ============================================================================

/// Get current time in nanoseconds
pub fn get_time_ns() -> u64 {
    // Use TSC or HPET
    crate::cpu::tsc::read_ns()
}

/// Sleep for specified nanoseconds
pub fn sleep_ns(ns: u64) {
    // Convert to milliseconds for sleep
    let ms = ns / 1_000_000;
    if ms > 0 {
        crate::task::scheduler::sleep(ms as usize);
    }
}

// ============================================================================
// GLOBAL ANIMATION STATE
// ============================================================================

lazy_static::lazy_static! {
    static ref ANIMATION_TIMELINE: Mutex<AnimationTimeline> = Mutex::new(AnimationTimeline::new(60));
    static ref FRAME_PACER: Mutex<FramePacer> = Mutex::new(FramePacer::new(60));
}

/// Add animation to global timeline
pub fn add_animation(animation: Animation) -> u64 {
    ANIMATION_TIMELINE.lock().add(animation)
}

/// Update global timeline
pub fn update_animations(dt: f64) -> bool {
    ANIMATION_TIMELINE.lock().update(dt)
}

/// Get animation value
pub fn get_animation_value(target: &AnimationTarget) -> Option<f32> {
    ANIMATION_TIMELINE.lock().get_value(target)
}

/// Begin frame
pub fn begin_frame() {
    FRAME_PACER.lock().begin_frame();
}

/// End frame with pacing
pub fn end_frame() {
    FRAME_PACER.lock().end_frame();
}

/// Get current FPS
pub fn get_fps() -> f64 {
    FRAME_PACER.lock().current_fps()
}

/// Initialize animation system
pub fn init() {
    crate::serial_println!("[ANIM] Animation system initialized (60 FPS target)");
}
