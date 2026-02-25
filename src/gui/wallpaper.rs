//! # Desktop Wallpapers
//!
//! Wallpaper management with transitions and dynamic backgrounds
//! Supports images, gradients, and dynamic content

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;
use libm::{sinf, cosf, sqrtf};

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};

// ============================================================================
// WALLPAPER CONSTANTS
// ============================================================================

/// Transition duration in seconds
pub const TRANSITION_DURATION: f32 = 1.0;

/// Maximum wallpapers in rotation
pub const MAX_WALLPAPERS: usize = 20;

// ============================================================================
// WALLPAPER TYPE
// ============================================================================

/// Wallpaper types
#[derive(Clone, Debug)]
pub enum WallpaperType {
    /// Solid color
    Solid(u32),
    /// Gradient (top to bottom)
    Gradient(u32, u32),
    /// Radial gradient
    RadialGradient { center_color: u32, edge_color: u32 },
    /// Image from path
    Image(String),
    /// Dynamic (time-based)
    Dynamic {
        day_image: String,
        night_image: String,
    },
    /// Slideshow
    Slideshow {
        images: Vec<String>,
        interval: f32, // seconds
        shuffle: bool,
    },
    /// Animated (simple effects)
    Animated(AnimatedType),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimatedType {
    Stars,
    Particles,
    Waves,
    Aurora,
}

// ============================================================================
// WALLPAPER
// ============================================================================

/// A wallpaper configuration
#[derive(Clone, Debug)]
pub struct Wallpaper {
    /// Wallpaper ID
    pub id: u32,
    /// Display name
    pub name: String,
    /// Wallpaper type
    pub wallpaper_type: WallpaperType,
    /// Is currently active
    pub active: bool,
    /// Transition progress (0.0 - 1.0)
    pub transition_progress: f32,
    /// Previous wallpaper (for transition)
    pub previous: Option<u32>,
}

impl Wallpaper {
    pub fn solid(id: u32, name: &str, color: u32) -> Self {
        Wallpaper {
            id,
            name: String::from(name),
            wallpaper_type: WallpaperType::Solid(color),
            active: false,
            transition_progress: 0.0,
            previous: None,
        }
    }
    
    pub fn gradient(id: u32, name: &str, top: u32, bottom: u32) -> Self {
        Wallpaper {
            id,
            name: String::from(name),
            wallpaper_type: WallpaperType::Gradient(top, bottom),
            active: false,
            transition_progress: 0.0,
            previous: None,
        }
    }
    
    pub fn image(id: u32, name: &str, path: &str) -> Self {
        Wallpaper {
            id,
            name: String::from(name),
            wallpaper_type: WallpaperType::Image(String::from(path)),
            active: false,
            transition_progress: 0.0,
            previous: None,
        }
    }
    
    pub fn slideshow(id: u32, name: &str, images: Vec<String>, interval: f32) -> Self {
        Wallpaper {
            id,
            name: String::from(name),
            wallpaper_type: WallpaperType::Slideshow {
                images,
                interval,
                shuffle: false,
            },
            active: false,
            transition_progress: 0.0,
            previous: None,
        }
    }
    
    pub fn animated(id: u32, name: &str, anim_type: AnimatedType) -> Self {
        Wallpaper {
            id,
            name: String::from(name),
            wallpaper_type: WallpaperType::Animated(anim_type),
            active: false,
            transition_progress: 0.0,
            previous: None,
        }
    }
}

// ============================================================================
// WALLPAPER MANAGER
// ============================================================================

/// Wallpaper manager
pub struct WallpaperManager {
    /// Available wallpapers
    pub wallpapers: Vec<Wallpaper>,
    /// Current wallpaper index
    pub current_index: usize,
    /// Screen width
    pub screen_width: usize,
    /// Screen height
    pub screen_height: usize,
    /// Is transitioning
    pub transitioning: bool,
    /// Transition type
    pub transition_type: TransitionType,
    /// Slideshow timer
    pub slideshow_timer: f32,
    /// Slideshow current image
    pub slideshow_index: usize,
    /// Animation time
    pub anim_time: f32,
    /// Cached previous frame
    pub prev_frame: Vec<u32>,
    /// Use cached previous
    pub use_cache: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransitionType {
    None,
    Fade,
    CrossFade,
    SlideLeft,
    SlideRight,
    SlideUp,
    SlideDown,
    Zoom,
    Cube,
}

impl WallpaperManager {
    pub fn new(screen_width: usize, screen_height: usize) -> Self {
        let mut manager = WallpaperManager {
            wallpapers: Vec::new(),
            current_index: 0,
            screen_width,
            screen_height,
            transitioning: false,
            transition_type: TransitionType::CrossFade,
            slideshow_timer: 0.0,
            slideshow_index: 0,
            anim_time: 0.0,
            prev_frame: Vec::new(),
            use_cache: false,
        };
        
        manager.add_default_wallpapers();
        manager
    }
    
    fn add_default_wallpapers(&mut self) {
        // Solid colors
        self.wallpapers.push(Wallpaper::solid(0, "Solid Black", 0x000000));
        self.wallpapers.push(Wallpaper::solid(1, "Solid Dark", 0x1E1E1E));
        self.wallpapers.push(Wallpaper::solid(2, "Solid Blue", 0x003366));
        
        // Gradients
        self.wallpapers.push(Wallpaper::gradient(3, "Sunset", 0xFF6B35, 0x1E1E2E));
        self.wallpapers.push(Wallpaper::gradient(4, "Ocean", 0x006994, 0x001F3F));
        self.wallpapers.push(Wallpaper::gradient(5, "Forest", 0x228B22, 0x0B3D0B));
        self.wallpapers.push(Wallpaper::gradient(6, "Night Sky", 0x0F0F23, 0x000011));
        self.wallpapers.push(Wallpaper::gradient(7, "Dawn", 0xFFB347, 0x87CEEB));
        self.wallpapers.push(Wallpaper::gradient(8, "Dusk", 0x4B0082, 0x191970));
        
        // Radial gradients
        self.wallpapers.push(Wallpaper {
            id: 9,
            name: String::from("Spotlight"),
            wallpaper_type: WallpaperType::RadialGradient {
                center_color: 0x333333,
                edge_color: 0x000000,
            },
            active: false,
            transition_progress: 0.0,
            previous: None,
        });
        
        // Animated
        self.wallpapers.push(Wallpaper::animated(10, "Stars", AnimatedType::Stars));
        self.wallpapers.push(Wallpaper::animated(11, "Aurora", AnimatedType::Aurora));
        self.wallpapers.push(Wallpaper::animated(12, "Waves", AnimatedType::Waves));
        
        // Set default
        if !self.wallpapers.is_empty() {
            self.wallpapers[0].active = true;
        }
    }
    
    /// Set wallpaper by index
    pub fn set_wallpaper(&mut self, index: usize) {
        if index >= self.wallpapers.len() || index == self.current_index {
            return;
        }
        
        // Start transition
        self.wallpapers[self.current_index].active = false;
        self.wallpapers[self.current_index].previous = None;
        
        self.wallpapers[index].active = true;
        self.wallpapers[index].transition_progress = 0.0;
        self.wallpapers[index].previous = Some(self.wallpapers[self.current_index].id);
        
        self.current_index = index;
        self.transitioning = true;
        self.use_cache = true;
    }
    
    /// Set wallpaper by ID
    pub fn set_wallpaper_by_id(&mut self, id: u32) {
        if let Some(index) = self.wallpapers.iter().position(|w| w.id == id) {
            self.set_wallpaper(index);
        }
    }
    
    /// Next wallpaper
    pub fn next_wallpaper(&mut self) {
        let next = (self.current_index + 1) % self.wallpapers.len();
        self.set_wallpaper(next);
    }
    
    /// Previous wallpaper
    pub fn prev_wallpaper(&mut self) {
        let prev = if self.current_index == 0 { self.wallpapers.len() - 1 } else { self.current_index - 1 };
        self.set_wallpaper(prev);
    }
    
    /// Add custom wallpaper
    pub fn add_wallpaper(&mut self, wallpaper: Wallpaper) {
        if self.wallpapers.len() < MAX_WALLPAPERS {
            self.wallpapers.push(wallpaper);
        }
    }
    
    /// Remove wallpaper
    pub fn remove_wallpaper(&mut self, id: u32) {
        if let Some(index) = self.wallpapers.iter().position(|w| w.id == id) {
            if self.wallpapers.len() > 1 {
                if index == self.current_index {
                    self.next_wallpaper();
                }
                self.wallpapers.remove(index);
                if index < self.current_index {
                    self.current_index -= 1;
                }
            }
        }
    }
    
    /// Update animation and transitions
    pub fn update(&mut self, dt: f32) {
        self.anim_time += dt;
        
        // Update transition
        if self.transitioning {
            self.wallpapers[self.current_index].transition_progress += dt / TRANSITION_DURATION;
            
            if self.wallpapers[self.current_index].transition_progress >= 1.0 {
                self.wallpapers[self.current_index].transition_progress = 1.0;
                self.transitioning = false;
                self.use_cache = false;
            }
        }
        
        // Update slideshow
        if let Some(wallpaper) = self.wallpapers.get(self.current_index) {
            if let WallpaperType::Slideshow { interval, .. } = &wallpaper.wallpaper_type {
                self.slideshow_timer += dt;
                if self.slideshow_timer >= *interval {
                    self.slideshow_timer = 0.0;
                    // Would advance to next image
                }
            }
        }
    }
    
    /// Draw wallpaper
    pub fn draw(&self, fb: &mut Framebuffer) {
        if self.wallpapers.is_empty() {
            // Fill with default color
            for y in 0..fb.height {
                for x in 0..fb.width {
                    fb.plot_pixel(x, y, Theme::DESKTOP_BG.to_u32());
                }
            }
            return;
        }
        
        let wallpaper = &self.wallpapers[self.current_index];
        
        // Draw previous wallpaper during transition
        if self.transitioning && self.use_cache {
            // Draw cached previous frame with transition effect
            self.draw_transition(fb, wallpaper);
        } else {
            // Draw current wallpaper
            self.draw_wallpaper_type(fb, &wallpaper.wallpaper_type, 1.0);
        }
    }
    
    fn draw_transition(&self, fb: &mut Framebuffer, wallpaper: &Wallpaper) {
        let progress = wallpaper.transition_progress;
        
        match self.transition_type {
            TransitionType::Fade | TransitionType::CrossFade => {
                // Draw previous (fading out)
                if let Some(prev_id) = wallpaper.previous {
                    if let Some(prev) = self.wallpapers.iter().find(|w| w.id == prev_id) {
                        self.draw_wallpaper_type(fb, &prev.wallpaper_type, 1.0 - progress);
                    }
                }
                // Draw current (fading in)
                self.draw_wallpaper_type(fb, &wallpaper.wallpaper_type, progress);
            }
            TransitionType::SlideLeft => {
                // Previous slides left
                let offset = (self.screen_width as f32 * progress) as i32;
                
                if let Some(prev_id) = wallpaper.previous {
                    if let Some(prev) = self.wallpapers.iter().find(|w| w.id == prev_id) {
                        // Draw previous at offset
                        self.draw_wallpaper_offset(fb, &prev.wallpaper_type, -offset);
                    }
                }
                // Draw current from right
                self.draw_wallpaper_offset(fb, &wallpaper.wallpaper_type, self.screen_width as i32 - offset);
            }
            TransitionType::SlideRight => {
                let offset = (self.screen_width as f32 * progress) as i32;
                
                if let Some(prev_id) = wallpaper.previous {
                    if let Some(prev) = self.wallpapers.iter().find(|w| w.id == prev_id) {
                        self.draw_wallpaper_offset(fb, &prev.wallpaper_type, offset);
                    }
                }
                self.draw_wallpaper_offset(fb, &wallpaper.wallpaper_type, -(self.screen_width as i32) + offset);
            }
            _ => {
                // Default: crossfade
                self.draw_wallpaper_type(fb, &wallpaper.wallpaper_type, progress);
            }
        }
    }
    
    fn draw_wallpaper_type(&self, fb: &mut Framebuffer, wallpaper_type: &WallpaperType, alpha: f32) {
        match wallpaper_type {
            WallpaperType::Solid(color) => {
                let color = Self::alpha_color(*color, alpha);
                for y in 0..fb.height {
                    for x in 0..fb.width {
                        fb.plot_pixel(x, y, color);
                    }
                }
            }
            WallpaperType::Gradient(top, bottom) => {
                for y in 0..fb.height {
                    let t = y as f32 / fb.height as f32;
                    let color = Self::lerp_color(*top, *bottom, t, alpha);
                    
                    for x in 0..fb.width {
                        fb.plot_pixel(x, y, color);
                    }
                }
            }
            WallpaperType::RadialGradient { center_color, edge_color } => {
                let center_x = fb.width / 2;
                let center_y = fb.height / 2;
                let max_dist = sqrtf((center_x * center_x + center_y * center_y) as f32);
                
                for y in 0..fb.height {
                    for x in 0..fb.width {
                        let dx = x as i32 - center_x as i32;
                        let dy = y as i32 - center_y as i32;
                        let dist = sqrtf((dx * dx + dy * dy) as f32);
                        let t = (dist / max_dist).min(1.0);
                        
                        let color = Self::lerp_color(*center_color, *edge_color, t, alpha);
                        fb.plot_pixel(x, y, color);
                    }
                }
            }
            WallpaperType::Image(_path) => {
                // Would load and draw image - fallback to solid
                self.draw_wallpaper_type(fb, &WallpaperType::Solid(Theme::DESKTOP_BG.to_u32()), alpha);
            }
            WallpaperType::Dynamic { day_image, night_image: _ } => {
                // Would check time and draw appropriate image
                self.draw_wallpaper_type(fb, &WallpaperType::Image(day_image.clone()), alpha);
            }
            WallpaperType::Slideshow { images, .. } => {
                if !images.is_empty() {
                    let idx = self.slideshow_index.min(images.len() - 1);
                    self.draw_wallpaper_type(fb, &WallpaperType::Image(images[idx].clone()), alpha);
                }
            }
            WallpaperType::Animated(anim_type) => {
                self.draw_animated(fb, *anim_type, alpha);
            }
        }
    }
    
    fn draw_wallpaper_offset(&self, fb: &mut Framebuffer, wallpaper_type: &WallpaperType, offset: i32) {
        // Draw wallpaper with horizontal offset
        match wallpaper_type {
            WallpaperType::Solid(color) => {
                for y in 0..fb.height {
                    let start_x = offset.max(0) as usize;
                    let end_x = (fb.width as i32 + offset).min(fb.width as i32) as usize;
                    
                    for x in start_x..end_x {
                        fb.plot_pixel(x, y, *color);
                    }
                }
            }
            WallpaperType::Gradient(top, bottom) => {
                for y in 0..fb.height {
                    let t = y as f32 / fb.height as f32;
                    let color = Self::lerp_color(*top, *bottom, t, 1.0);
                    
                    let start_x = offset.max(0) as usize;
                    let end_x = (fb.width as i32 + offset).min(fb.width as i32) as usize;
                    
                    for x in start_x..end_x {
                        fb.plot_pixel(x, y, color);
                    }
                }
            }
            _ => {
                self.draw_wallpaper_type(fb, wallpaper_type, 1.0);
            }
        }
    }
    
    fn draw_animated(&self, fb: &mut Framebuffer, anim_type: AnimatedType, alpha: f32) {
        // Base gradient
        let base_top = 0x0F0F23;
        let base_bottom = 0x000011;
        
        for y in 0..fb.height {
            let t = y as f32 / fb.height as f32;
            let base_color = Self::lerp_color(base_top, base_bottom, t, alpha);
            
            for x in 0..fb.width {
                fb.plot_pixel(x, y, base_color);
            }
        }
        
        match anim_type {
            AnimatedType::Stars => {
                self.draw_stars(fb, alpha);
            }
            AnimatedType::Aurora => {
                self.draw_aurora(fb, alpha);
            }
            AnimatedType::Waves => {
                self.draw_waves(fb, alpha);
            }
            AnimatedType::Particles => {
                self.draw_particles(fb, alpha);
            }
        }
    }
    
    fn draw_stars(&self, fb: &mut Framebuffer, alpha: f32) {
        // Simple star field animation
        let time = self.anim_time;
        
        // Generate pseudo-random stars based on position
        for i in 0..200 {
            let seed = i * 7919;
            let x = seed % fb.width;
            let y = (seed * 3) % fb.height;
            
            // Twinkle effect
            let twinkle = (sinf(time * 2.0 + seed as f32 * 0.1) + 1.0) / 2.0;
            let brightness = (0.3 + 0.7 * twinkle) * alpha;
            
            let star_color = Self::alpha_color(0xFFFFFF, brightness);
            
            // Draw star (small dot)
            if x < fb.width && y < fb.height {
                fb.plot_pixel(x, y, star_color);
                if x + 1 < fb.width {
                    fb.plot_pixel(x + 1, y, star_color);
                }
            }
        }
    }
    
    fn draw_aurora(&self, fb: &mut Framebuffer, alpha: f32) {
        let time = self.anim_time;
        
        for y in 0..fb.height {
            for x in 0..fb.width {
                // Aurora wave effect
                let wave1 = (sinf(x as f32 * 0.01 + time * 0.5) * 50.0) as i32;
                let wave2 = (sinf(x as f32 * 0.02 + time * 0.3) * 30.0) as i32;
                
                let aurora_y = fb.height as i32 / 2 + wave1 + wave2;
                
                let dist = (y as i32 - aurora_y).abs();
                
                if dist < 80 {
                    let intensity = (1.0 - dist as f32 / 80.0) * alpha * 0.4;
                    
                    // Aurora colors (green/purple)
                    let hue = (x as f32 * 0.003 + time * 0.2) % 1.0;
                    let color = if hue < 0.5 {
                        Self::lerp_color(0x00FF88, 0x8800FF, hue * 2.0, intensity)
                    } else {
                        Self::lerp_color(0x8800FF, 0x00FF88, (hue - 0.5) * 2.0, intensity)
                    };
                    
                    let ptr = unsafe { (fb.base_addr as *mut u32).add(y * fb.pixels_per_scan_line + x) };
                    let bg = unsafe { *ptr };
                    unsafe { *ptr = Self::blend_color(bg, color); }
                }
            }
        }
    }
    
    fn draw_waves(&self, fb: &mut Framebuffer, alpha: f32) {
        let time = self.anim_time;
        
        for y in 0..fb.height {
            for x in 0..fb.width {
                // Multiple wave layers
                let wave1 = (sinf(x as f32 * 0.02 + time * 1.5) * 20.0) as i32;
                let wave2 = (sinf(x as f32 * 0.03 - time * 1.2) * 15.0) as i32;
                let wave3 = (sinf(x as f32 * 0.01 + time * 0.8) * 25.0) as i32;
                
                let wave_y = fb.height as i32 - 100 + wave1 + wave2 + wave3;
                
                if y as i32 > wave_y {
                    let depth = (y as i32 - wave_y) as f32;
                    let intensity = (depth / 100.0).min(1.0) * alpha;
                    
                    // Ocean blue gradient
                    let color = Self::lerp_color(0x006994, 0x001F3F, intensity, intensity);
                    
                    let ptr = unsafe { (fb.base_addr as *mut u32).add(y * fb.pixels_per_scan_line + x) };
                    unsafe { *ptr = color; }
                }
            }
        }
    }
    
    fn draw_particles(&self, fb: &mut Framebuffer, alpha: f32) {
        let time = self.anim_time;
        
        for i in 0..50 {
            let seed = i * 1234;
            let base_x = (seed % fb.width) as f32;
            let base_y = ((seed * 7) % fb.height) as f32;
            
            // Floating motion
            let x = (base_x + sinf(time) * 20.0 + cosf(time) * 15.0) as usize;
            let y = (base_y + sinf(time * 0.5) * 30.0) as usize;
            
            let x = x % fb.width;
            let y = y % fb.height;
            
            let color = Self::alpha_color(0xFFFFFF, 0.3 * alpha);
            
            // Draw particle with glow
            for py in 0..4 {
                for px in 0..4 {
                    let px = x + px;
                    let py = y + py;
                    if px < fb.width && py < fb.height {
                        fb.plot_pixel(px, py, color);
                    }
                }
            }
        }
    }
    
    fn lerp_color(c1: u32, c2: u32, t: f32, alpha: f32) -> u32 {
        let r1 = ((c1 >> 16) & 0xFF) as f32;
        let g1 = ((c1 >> 8) & 0xFF) as f32;
        let b1 = (c1 & 0xFF) as f32;
        
        let r2 = ((c2 >> 16) & 0xFF) as f32;
        let g2 = ((c2 >> 8) & 0xFF) as f32;
        let b2 = (c2 & 0xFF) as f32;
        
        let r = (r1 + (r2 - r1) * t) * alpha;
        let g = (g1 + (g2 - g1) * t) * alpha;
        let b = (b1 + (b2 - b1) * t) * alpha;
        
        (r as u32) << 16 | (g as u32) << 8 | (b as u32)
    }
    
    fn alpha_color(color: u32, alpha: f32) -> u32 {
        let r = (((color >> 16) & 0xFF) as f32 * alpha) as u32;
        let g = (((color >> 8) & 0xFF) as f32 * alpha) as u32;
        let b = ((color & 0xFF) as f32 * alpha) as u32;
        (r << 16) | (g << 8) | b
    }
    
    fn blend_color(bg: u32, fg: u32) -> u32 {
        let br = ((bg >> 16) & 0xFF) as f32;
        let bg_ = ((bg >> 8) & 0xFF) as f32;
        let bb = (bg & 0xFF) as f32;
        
        let fr = ((fg >> 16) & 0xFF) as f32;
        let fg_ = ((fg >> 8) & 0xFF) as f32;
        let fb = (fg & 0xFF) as f32;
        
        let r = (br + fr).min(255.0) as u32;
        let g = (bg_ + fg_).min(255.0) as u32;
        let b = (bb + fb).min(255.0) as u32;
        
        (r << 16) | (g << 8) | b
    }
    
    /// Resize
    pub fn resize(&mut self, width: usize, height: usize) {
        self.screen_width = width;
        self.screen_height = height;
    }
    
    /// Get current wallpaper name
    pub fn current_name(&self) -> &str {
        &self.wallpapers[self.current_index].name
    }
    
    /// Get wallpaper list for settings
    pub fn get_wallpaper_list(&self) -> Vec<(u32, String)> {
        self.wallpapers.iter().map(|w| (w.id, w.name.clone())).collect()
    }
}

// ============================================================================
// GLOBAL WALLPAPER MANAGER
// ============================================================================

lazy_static::lazy_static! {
    static ref WALLPAPER: Mutex<WallpaperManager> = Mutex::new(WallpaperManager::new(1920, 1080));
}

/// Initialize wallpaper manager
pub fn init(width: usize, height: usize) {
    let mut wallpaper = WALLPAPER.lock();
    wallpaper.resize(width, height);
    crate::serial_println!("[GUI] Wallpaper manager initialized");
}

/// Get wallpaper manager
pub fn get_wallpaper() -> &'static Mutex<WallpaperManager> {
    &WALLPAPER
}
