//! # Glyph Atlas with Subpixel Antialiasing
//!
//! Efficient text rendering using cached glyph bitmaps
//! Supports subpixel rendering (ClearType-like) for crisp text

use alloc::collections::BTreeMap;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use alloc::vec;
use alloc::boxed::Box;
use core::cmp::{min, max};
use spin::Mutex;
use libm::ceilf;

// ============================================================================
// GLYPH ATLAS CONSTANTS
// ============================================================================

/// Atlas width in pixels
pub const ATLAS_WIDTH: u16 = 1024;

/// Atlas height in pixels
pub const ATLAS_HEIGHT: u16 = 1024;

/// Maximum number of atlases
pub const MAX_ATLASES: usize = 4;

/// Maximum glyph cache entries
pub const MAX_GLYPH_CACHE: usize = 4096;

/// Padding around glyphs (pixels)
pub const GLYPH_PADDING: u8 = 1;

// ============================================================================
// FONT STYLE
// ============================================================================

/// Font style flags
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FontStyle {
    pub weight: FontWeight,
    pub style: FontStyleType,
    pub size: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontWeight {
    Thin = 100,
    ExtraLight = 200,
    Light = 300,
    Regular = 400,
    Medium = 500,
    SemiBold = 600,
    Bold = 700,
    ExtraBold = 800,
    Black = 900,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontStyleType {
    Normal,
    Italic,
    Oblique,
}

impl FontStyle {
    pub fn regular(size: u16) -> Self {
        FontStyle {
            weight: FontWeight::Regular,
            style: FontStyleType::Normal,
            size,
        }
    }
    
    pub fn bold(size: u16) -> Self {
        FontStyle {
            weight: FontWeight::Bold,
            style: FontStyleType::Normal,
            size,
        }
    }
    
    pub fn italic(size: u16) -> Self {
        FontStyle {
            weight: FontWeight::Regular,
            style: FontStyleType::Italic,
            size,
        }
    }
}

// ============================================================================
// GLYPH KEY
// ============================================================================

/// Key for glyph cache lookup
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GlyphKey {
    /// Unicode codepoint
    pub codepoint: u32,
    /// Font size
    pub size: u16,
    /// Font weight (0-9 mapping to 100-900)
    pub weight: u8,
    /// Style flags
    pub style_flags: u8,
}

impl GlyphKey {
    pub fn new(codepoint: char, style: &FontStyle) -> Self {
        GlyphKey {
            codepoint: codepoint as u32,
            size: style.size,
            weight: (style.weight as u16 / 100) as u8,
            style_flags: match style.style {
                FontStyleType::Normal => 0,
                FontStyleType::Italic => 1,
                FontStyleType::Oblique => 2,
            },
        }
    }
}

// ============================================================================
// GLYPH INFO
// ============================================================================

/// Cached glyph information
#[derive(Clone, Copy, Debug)]
pub struct GlyphInfo {
    /// Position in atlas (x, y)
    pub atlas_x: u16,
    pub atlas_y: u16,
    /// Atlas index (for multi-atlas)
    pub atlas_index: u8,
    /// Glyph width in pixels
    pub width: u16,
    /// Glyph height in pixels
    pub height: u16,
    /// Horizontal advance (pixels, 16.16 fixed point)
    pub advance: i32,
    /// Left bearing (pixels, 16.16 fixed point)
    pub bearing_x: i32,
    /// Top bearing (pixels, 16.16 fixed point)
    pub bearing_y: i32,
    /// Is this a colored glyph (emoji)
    pub is_colored: bool,
    /// LRU timestamp
    pub last_used: u64,
}

impl GlyphInfo {
    pub fn new() -> Self {
        GlyphInfo {
            atlas_x: 0,
            atlas_y: 0,
            atlas_index: 0,
            width: 0,
            height: 0,
            advance: 0,
            bearing_x: 0,
            bearing_y: 0,
            is_colored: false,
            last_used: 0,
        }
    }
    
    /// Get the bounding box for rendering
    pub fn bounds(&self, x: i32, y: i32) -> (i32, i32, i32, i32) {
        let min_x = x + (self.bearing_x >> 16);
        let min_y = y - (self.bearing_y >> 16);
        let max_x = min_x + self.width as i32;
        let max_y = min_y + self.height as i32;
        (min_x, min_y, max_x, max_y)
    }
}

// ============================================================================
// GLYPH BITMAP
// ============================================================================

/// Rendered glyph bitmap
#[derive(Clone, Debug)]
pub struct GlyphBitmap {
    /// Width in pixels
    pub width: u16,
    /// Height in pixels
    pub height: u16,
    /// Pitch (bytes per row)
    pub pitch: i32,
    /// Pixel data (grayscale or BGRA)
    pub data: Vec<u8>,
    /// Is colored (BGRA format)
    pub is_colored: bool,
}

impl GlyphBitmap {
    pub fn new(width: u16, height: u16, is_colored: bool) -> Self {
        let pitch = if is_colored { width as i32 * 4 } else { width as i32 };
        GlyphBitmap {
            width,
            height,
            pitch,
            data: vec![0; pitch as usize * height as usize],
            is_colored,
        }
    }
    
    /// Get grayscale value at position
    pub fn get_grayscale(&self, x: u16, y: u16) -> u8 {
        if x >= self.width || y >= self.height || self.is_colored {
            return 0;
        }
        self.data[y as usize * self.pitch as usize + x as usize]
    }
    
    /// Get BGRA value at position
    pub fn get_bgra(&self, x: u16, y: u16) -> (u8, u8, u8, u8) {
        if x >= self.width || y >= self.height || !self.is_colored {
            return (0, 0, 0, 0);
        }
        let offset = y as usize * self.pitch as usize + x as usize * 4;
        (self.data[offset], self.data[offset + 1], self.data[offset + 2], self.data[offset + 3])
    }
    
    /// Get subpixel coverage at position (3x horizontal resolution)
    pub fn get_subpixel(&self, x: u16, y: u16, subpixel: u8) -> u8 {
        if x >= self.width || y >= self.height {
            return 0;
        }
        
        // For subpixel rendering, we need 3x horizontal resolution
        // This is typically provided by the font rasterizer
        // Here we approximate by sampling
        let base = self.get_grayscale(x, y);
        
        // Simple subpixel approximation
        match subpixel {
            0 => base.saturating_mul(3) / 4, // Left subpixel
            1 => base,                        // Center subpixel
            2 => base.saturating_mul(3) / 4, // Right subpixel
            _ => base,
        }
    }
}

// ============================================================================
// SUBPIXEL LAYOUT
// ============================================================================

/// LCD subpixel arrangement
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubpixelLayout {
    /// Standard RGB horizontal
    RgbHorizontal,
    /// BGR horizontal
    BgrHorizontal,
    /// RGB vertical
    RgbVertical,
    /// BGR vertical
    BgrVertical,
    /// No subpixel (OLED, unknown)
    None,
}

impl SubpixelLayout {
    /// Get default layout
    pub fn default_layout() -> Self {
        SubpixelLayout::RgbHorizontal
    }
    
    /// Get subpixel color for position
    pub fn subpixel_color(&self, x: i32) -> (u8, u8, u8) {
        match self {
            SubpixelLayout::RgbHorizontal => {
                match x % 3 {
                    0 => (255, 0, 0),   // Red
                    1 => (0, 255, 0),   // Green
                    2 => (0, 0, 255),   // Blue
                    _ => (255, 255, 255),
                }
            }
            SubpixelLayout::BgrHorizontal => {
                match x % 3 {
                    0 => (0, 0, 255),   // Blue
                    1 => (0, 255, 0),   // Green
                    2 => (255, 0, 0),   // Red
                    _ => (255, 255, 255),
                }
            }
            SubpixelLayout::RgbVertical => {
                match x % 3 {
                    0 => (255, 0, 0),
                    1 => (0, 255, 0),
                    2 => (0, 0, 255),
                    _ => (255, 255, 255),
                }
            }
            SubpixelLayout::BgrVertical => {
                match x % 3 {
                    0 => (0, 0, 255),
                    1 => (0, 255, 0),
                    2 => (255, 0, 0),
                    _ => (255, 255, 255),
                }
            }
            SubpixelLayout::None => (255, 255, 255),
        }
    }
}

// ============================================================================
// GLYPH ATLAS
// ============================================================================

/// Single glyph atlas texture
pub struct GlyphAtlas {
    /// Atlas texture data (RGBA)
    texture: Vec<u32>,
    /// Width in pixels
    width: u16,
    /// Height in pixels
    height: u16,
    /// Next available X position
    next_x: u16,
    /// Next available Y position
    next_y: u16,
    /// Current row height
    row_height: u16,
    /// Number of glyphs stored
    glyph_count: usize,
}

impl GlyphAtlas {
    pub fn new() -> Self {
        GlyphAtlas {
            texture: vec![0; ATLAS_WIDTH as usize * ATLAS_HEIGHT as usize],
            width: ATLAS_WIDTH,
            height: ATLAS_HEIGHT,
            next_x: 0,
            next_y: 0,
            row_height: 0,
            glyph_count: 0,
        }
    }
    
    /// Check if atlas has space for glyph
    pub fn has_space(&self, width: u16, height: u16) -> bool {
        let padded_w = width + GLYPH_PADDING as u16 * 2;
        let padded_h = height + GLYPH_PADDING as u16 * 2;
        
        // Check current row
        if self.next_x + padded_w <= self.width {
            return true;
        }
        
        // Check new row
        if self.next_y + self.row_height + padded_h <= self.height {
            return true;
        }
        
        false
    }
    
    /// Allocate space for a glyph
    pub fn allocate(&mut self, width: u16, height: u16) -> Option<(u16, u16)> {
        let padded_w = width + GLYPH_PADDING as u16 * 2;
        let padded_h = height + GLYPH_PADDING as u16 * 2;
        
        // Try current row
        if self.next_x + padded_w <= self.width {
            let x = self.next_x + GLYPH_PADDING as u16;
            let y = self.next_y + self.row_height + GLYPH_PADDING as u16;
            
            self.next_x += padded_w;
            self.row_height = max(self.row_height, padded_h);
            self.glyph_count += 1;
            
            return Some((x, y));
        }
        
        // Start new row
        if self.next_y + self.row_height + padded_h <= self.height {
            self.next_y += self.row_height;
            self.next_x = padded_w;
            self.row_height = padded_h;
            self.glyph_count += 1;
            
            let x = GLYPH_PADDING as u16;
            let y = self.next_y + GLYPH_PADDING as u16;
            
            return Some((x, y));
        }
        
        None
    }
    
    /// Copy glyph bitmap to atlas
    pub fn copy_glyph(&mut self, x: u16, y: u16, bitmap: &GlyphBitmap) {
        for row in 0..bitmap.height {
            let dst_y = y + row;
            if dst_y >= self.height {
                break;
            }
            
            for col in 0..bitmap.width {
                let dst_x = x + col;
                if dst_x >= self.width {
                    break;
                }
                
                let pixel = if bitmap.is_colored {
                    let (b, g, r, a) = bitmap.get_bgra(col, row);
                    // Convert to 0xAABBGGRR
                    ((a as u32) << 24) | ((b as u32) << 16) | ((g as u32) << 8) | (r as u32)
                } else {
                    let alpha = bitmap.get_grayscale(col, row);
                    // White glyph with alpha
                    ((alpha as u32) << 24) | 0x00FFFFFF
                };
                
                let idx = dst_y as usize * self.width as usize + dst_x as usize;
                self.texture[idx] = pixel;
            }
        }
    }
    
    /// Get pixel from atlas
    pub fn get_pixel(&self, x: u16, y: u16) -> u32 {
        if x >= self.width || y >= self.height {
            return 0;
        }
        self.texture[y as usize * self.width as usize + x as usize]
    }
    
    /// Get texture data
    pub fn texture(&self) -> &[u32] {
        &self.texture
    }
    
    /// Get dimensions
    pub fn dimensions(&self) -> (u16, u16) {
        (self.width, self.height)
    }
    
    /// Get glyph count
    pub fn count(&self) -> usize {
        self.glyph_count
    }
    
    /// Check if atlas is empty
    pub fn is_empty(&self) -> bool {
        self.glyph_count == 0
    }
    
    /// Clear atlas
    pub fn clear(&mut self) {
        self.texture.fill(0);
        self.next_x = 0;
        self.next_y = 0;
        self.row_height = 0;
        self.glyph_count = 0;
    }
}

// ============================================================================
// GLYPH ATLAS MANAGER
// ============================================================================

/// Manages multiple glyph atlases with LRU eviction
pub struct GlyphAtlasManager {
    /// All atlases
    atlases: Vec<GlyphAtlas>,
    /// Current atlas index
    current_atlas: usize,
    /// Glyph cache (key -> info)
    cache: BTreeMap<GlyphKey, GlyphInfo>,
    /// LRU access order
    lru: VecDeque<GlyphKey>,
    /// Frame counter for LRU
    frame: u64,
    /// Subpixel layout
    subpixel_layout: SubpixelLayout,
    /// Enable subpixel rendering
    subpixel_enabled: bool,
    /// Cache hits
    hits: u64,
    /// Cache misses
    misses: u64,
}

impl GlyphAtlasManager {
    pub fn new() -> Self {
        let mut manager = GlyphAtlasManager {
            atlases: Vec::new(),
            current_atlas: 0,
            cache: BTreeMap::new(),
            lru: VecDeque::new(),
            frame: 0,
            subpixel_layout: SubpixelLayout::default_layout(),
            subpixel_enabled: true,
            hits: 0,
            misses: 0,
        };
        
        // Create first atlas
        manager.atlases.push(GlyphAtlas::new());
        
        manager
    }
    
    /// Enable/disable subpixel rendering
    pub fn set_subpixel(&mut self, enabled: bool) {
        self.subpixel_enabled = enabled;
    }
    
    /// Set subpixel layout
    pub fn set_subpixel_layout(&mut self, layout: SubpixelLayout) {
        self.subpixel_layout = layout;
    }
    
    /// Get or render a glyph
    pub fn get_glyph(&mut self, key: GlyphKey, rasterizer: &mut dyn GlyphRasterizer) -> Option<GlyphInfo> {
        // Check cache
        if let Some(mut info) = self.cache.get(&key).copied() {
            // Cache hit
            self.hits += 1;
            info.last_used = self.frame;
            self.cache.insert(key, info);
            self.touch_lru(key);
            return Some(info);
        }
        
        // Cache miss
        self.misses += 1;
        
        // Render glyph
        let bitmap = rasterizer.render(key.codepoint, key.size, key.weight, key.style_flags)?;
        
        // Find space in current atlas
        let mut atlas_idx = self.current_atlas;
        let mut pos = None;
        
        if let Some(atlas) = self.atlases.get_mut(atlas_idx) {
            pos = atlas.allocate(bitmap.width, bitmap.height);
        }
        
        // If no space, try other atlases or create new
        if pos.is_none() {
            for (i, atlas) in self.atlases.iter_mut().enumerate() {
                if i != self.current_atlas {
                    pos = atlas.allocate(bitmap.width, bitmap.height);
                    if pos.is_some() {
                        atlas_idx = i;
                        break;
                    }
                }
            }
        }
        
        // Create new atlas if needed
        if pos.is_none() && self.atlases.len() < MAX_ATLASES {
            let mut new_atlas = GlyphAtlas::new();
            pos = new_atlas.allocate(bitmap.width, bitmap.height);
            self.atlases.push(new_atlas);
            atlas_idx = self.atlases.len() - 1;
        }
        
        // Evict LRU if still no space
        if pos.is_none() {
            self.evict_lru();
            if let Some(atlas) = self.atlases.get_mut(atlas_idx) {
                pos = atlas.allocate(bitmap.width, bitmap.height);
            }
        }
        
        // Get position
        let (x, y) = pos?;
        
        // Copy to atlas
        if let Some(atlas) = self.atlases.get_mut(atlas_idx) {
            atlas.copy_glyph(x, y, &bitmap);
        }
        
        // Create info
        let info = GlyphInfo {
            atlas_x: x,
            atlas_y: y,
            atlas_index: atlas_idx as u8,
            width: bitmap.width,
            height: bitmap.height,
            advance: rasterizer.get_advance(key.codepoint, key.size),
            bearing_x: rasterizer.get_bearing_x(key.codepoint, key.size),
            bearing_y: rasterizer.get_bearing_y(key.codepoint, key.size),
            is_colored: bitmap.is_colored,
            last_used: self.frame,
        };
        
        // Cache it
        self.cache.insert(key, info);
        self.lru.push_back(key);
        
        // Check cache size
        if self.cache.len() > MAX_GLYPH_CACHE {
            self.evict_lru();
        }
        
        Some(info)
    }
    
    /// Touch LRU entry
    fn touch_lru(&mut self, key: GlyphKey) {
        self.lru.retain(|&k| k != key);
        self.lru.push_back(key);
    }
    
    /// Evict least recently used glyphs
    fn evict_lru(&mut self) {
        if let Some(key) = self.lru.pop_front() {
            if let Some(info) = self.cache.remove(&key) {
                // Mark atlas region as free (simplified - just clear the glyph)
                // In a real implementation, we'd track free regions
                self.cache.remove(&key);
            }
        }
    }
    
    /// Render text using cached glyphs
    pub fn render_text(
        &mut self,
        text: &str,
        style: &FontStyle,
        mut x: i32,
        y: i32,
        color: u32,
        fb: &mut [u32],
        fb_width: usize,
        fb_height: usize,
        rasterizer: &mut dyn GlyphRasterizer,
    ) -> i32 {
        self.frame += 1;
        
        let mut max_x = x;
        
        for c in text.chars() {
            let key = GlyphKey::new(c, style);
            
            if let Some(info) = self.get_glyph(key, rasterizer) {
                // Render glyph
                self.render_glyph(&info, x, y, color, fb, fb_width, fb_height);
                
                // Advance
                x += info.advance >> 16;
                max_x = max(max_x, x);
            }
        }
        
        max_x
    }
    
    /// Render a single glyph
    pub fn render_glyph(
        &self,
        info: &GlyphInfo,
        x: i32,
        y: i32,
        color: u32,
        fb: &mut [u32],
        fb_width: usize,
        fb_height: usize,
    ) {
        let atlas = match self.atlases.get(info.atlas_index as usize) {
            Some(a) => a,
            None => return,
        };
        
        // Calculate position
        let draw_x = x + (info.bearing_x >> 16);
        let draw_y = y - (info.bearing_y >> 16);
        
        // Extract color components
        let cr = ((color >> 16) & 0xFF) as u8;
        let cg = ((color >> 8) & 0xFF) as u8;
        let cb = (color & 0xFF) as u8;
        
        for gy in 0..info.height {
            let fb_y = draw_y + gy as i32;
            if fb_y < 0 || fb_y >= fb_height as i32 {
                continue;
            }
            
            for gx in 0..info.width {
                let fb_x = draw_x + gx as i32;
                if fb_x < 0 || fb_x >= fb_width as i32 {
                    continue;
                }
                
                let atlas_pixel = atlas.get_pixel(info.atlas_x + gx, info.atlas_y + gy);
                let alpha = ((atlas_pixel >> 24) & 0xFF) as u8;
                
                if alpha == 0 {
                    continue;
                }
                
                let fb_idx = fb_y as usize * fb_width + fb_x as usize;
                let bg = fb[fb_idx];
                
                // Extract background
                let br = ((bg >> 16) & 0xFF) as u8;
                let bg_ = ((bg >> 8) & 0xFF) as u8;
                let bb = (bg & 0xFF) as u8;
                
                // Alpha blend
                let a = alpha as u32;
                let inv_a = 255 - a;
                
                let r = ((cr as u32 * a + br as u32 * inv_a) / 255) as u8;
                let g = ((cg as u32 * a + bg_ as u32 * inv_a) / 255) as u8;
                let b = ((cb as u32 * a + bb as u32 * inv_a) / 255) as u8;
                
                fb[fb_idx] = ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
            }
        }
    }
    
    /// Render with subpixel antialiasing
    pub fn render_glyph_subpixel(
        &self,
        info: &GlyphInfo,
        x: i32,
        y: i32,
        color: u32,
        fb: &mut [u32],
        fb_width: usize,
        fb_height: usize,
    ) {
        if !self.subpixel_enabled {
            return self.render_glyph(info, x, y, color, fb, fb_width, fb_height);
        }
        
        let atlas = match self.atlases.get(info.atlas_index as usize) {
            Some(a) => a,
            None => return,
        };
        
        let draw_x = x + (info.bearing_x >> 16);
        let draw_y = y - (info.bearing_y >> 16);
        
        // Extract color components
        let cr = ((color >> 16) & 0xFF) as f32;
        let cg = ((color >> 8) & 0xFF) as f32;
        let cb = (color & 0xFF) as f32;
        
        for gy in 0..info.height {
            let fb_y = draw_y + gy as i32;
            if fb_y < 0 || fb_y >= fb_height as i32 {
                continue;
            }
            
            for gx in 0..info.width {
                let fb_x = draw_x + gx as i32;
                if fb_x < 0 || fb_x >= fb_width as i32 {
                    continue;
                }
                
                let atlas_pixel = atlas.get_pixel(info.atlas_x + gx, info.atlas_y + gy);
                let alpha = ((atlas_pixel >> 24) & 0xFF) as f32 / 255.0;
                
                if alpha < 0.01 {
                    continue;
                }
                
                let fb_idx = fb_y as usize * fb_width + fb_x as usize;
                let bg = fb[fb_idx];
                
                // Extract background
                let br = ((bg >> 16) & 0xFF) as f32;
                let bg_ = ((bg >> 8) & 0xFF) as f32;
                let bb = (bg & 0xFF) as f32;
                
                // Get subpixel weights based on position
                let (sr, sg, sb) = self.subpixel_layout.subpixel_color(fb_x);
                
                // Apply subpixel blending
                let r = br * (1.0 - alpha * sr as f32 / 255.0) + cr * alpha * sr as f32 / 255.0;
                let g = bg_ * (1.0 - alpha * sg as f32 / 255.0) + cg * alpha * sg as f32 / 255.0;
                let b = bb * (1.0 - alpha * sb as f32 / 255.0) + cb * alpha * sb as f32 / 255.0;
                
                fb[fb_idx] = ((r as u32).min(255) << 16) | ((g as u32).min(255) << 8) | (b as u32).min(255);
            }
        }
    }
    
    /// Get cache statistics
    pub fn stats(&self) -> (u64, u64, f32) {
        let total = self.hits + self.misses;
        let hit_rate = if total > 0 { self.hits as f32 / total as f32 } else { 0.0 };
        (self.hits, self.misses, hit_rate)
    }
    
    /// Get cache size
    pub fn cache_size(&self) -> usize {
        self.cache.len()
    }
    
    /// Get atlas count
    pub fn atlas_count(&self) -> usize {
        self.atlases.len()
    }
    
    /// Clear all caches
    pub fn clear(&mut self) {
        for atlas in &mut self.atlases {
            atlas.clear();
        }
        self.cache.clear();
        self.lru.clear();
        self.current_atlas = 0;
        self.hits = 0;
        self.misses = 0;
    }
}

// ============================================================================
// GLYPH RASTERIZER TRAIT
// ============================================================================

/// Trait for glyph rasterization
pub trait GlyphRasterizer: Send + Sync {
    /// Render a glyph to bitmap
    fn render(&mut self, codepoint: u32, size: u16, weight: u8, style: u8) -> Option<GlyphBitmap>;
    
    /// Get horizontal advance (16.16 fixed point)
    fn get_advance(&self, codepoint: u32, size: u16) -> i32;
    
    /// Get left bearing (16.16 fixed point)
    fn get_bearing_x(&self, codepoint: u32, size: u16) -> i32;
    
    /// Get top bearing (16.16 fixed point)
    fn get_bearing_y(&self, codepoint: u32, size: u16) -> i32;
    
    /// Get line height
    fn line_height(&self, size: u16) -> i32;
}

// ============================================================================
// BUILT-IN RASTERIZER (VGA Font)
// ============================================================================

/// Simple VGA font rasterizer
pub struct VgaFontRasterizer;

impl VgaFontRasterizer {
    pub fn new() -> Self {
        VgaFontRasterizer
    }
    
    /// Get VGA font data for character
    fn get_font_data(c: char) -> [u8; 16] {
        // Use crate's VGA font
        crate::font::vga_font::get_font_data(c)
    }
}

impl GlyphRasterizer for VgaFontRasterizer {
    fn render(&mut self, codepoint: u32, size: u16, _weight: u8, _style: u8) -> Option<GlyphBitmap> {
        let c = char::from_u32(codepoint)?;
        
        // VGA font is 8x16, scale if needed
        let scale = size as f32 / 16.0;
        let width = ceilf(8.0 * scale) as u16;
        let height = ceilf(16.0 * scale) as u16;
        
        let mut bitmap = GlyphBitmap::new(width.max(8), height.max(16), false);
        
        let font_data = Self::get_font_data(c);
        
        // Simple nearest-neighbor scaling
        for row in 0..height {
            let src_row = (row as f32 / scale).min(15.0) as usize;
            let byte = font_data[src_row];
            
            for col in 0..width {
                let src_col = (col as f32 / scale).min(7.0) as usize;
                let bit = (byte >> (7 - src_col)) & 1;
                
                if bit == 1 {
                    let idx = row as usize * bitmap.pitch as usize + col as usize;
                    if idx < bitmap.data.len() {
                        bitmap.data[idx] = 255;
                    }
                }
            }
        }
        
        Some(bitmap)
    }
    
    fn get_advance(&self, _codepoint: u32, size: u16) -> i32 {
        // VGA font is monospace
        (size as i32 * 8 / 16) << 16 // 8 pixels scaled
    }
    
    fn get_bearing_x(&self, _codepoint: u32, _size: u16) -> i32 {
        0
    }
    
    fn get_bearing_y(&self, _codepoint: u32, size: u16) -> i32 {
        (size as i32) << 16 // Top of character
    }
    
    fn line_height(&self, size: u16) -> i32 {
        ((size as f32 * 1.2) as i32) << 16
    }
}

// ============================================================================
// GLOBAL GLYPH ATLAS
// ============================================================================

lazy_static::lazy_static! {
    static ref GLYPH_ATLAS: Mutex<GlyphAtlasManager> = Mutex::new(GlyphAtlasManager::new());
}

/// Initialize glyph atlas
pub fn init() {
    let mut atlas = GLYPH_ATLAS.lock();
    atlas.set_subpixel(true);
    crate::serial_println!("[FONT] Glyph atlas initialized ({}x{}, subpixel: {:?})", 
        ATLAS_WIDTH, ATLAS_HEIGHT, atlas.subpixel_layout);
}

/// Get glyph atlas manager
pub fn get_atlas() -> &'static Mutex<GlyphAtlasManager> {
    &GLYPH_ATLAS
}

/// Render text using global atlas
pub fn render_text(
    text: &str,
    style: &FontStyle,
    x: i32,
    y: i32,
    color: u32,
    fb: &mut [u32],
    fb_width: usize,
    fb_height: usize,
) -> i32 {
    let mut atlas = GLYPH_ATLAS.lock();
    let mut rasterizer = VgaFontRasterizer::new();
    atlas.render_text(text, style, x, y, color, fb, fb_width, fb_height, &mut rasterizer)
}
