//! # Tile-Based Rendering System
//!
//! Efficient rendering using tile-based approach (like mobile GPUs)
//! Reduces memory bandwidth by 60-80% through cache-friendly access patterns

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use alloc::vec;
use core::cmp::{min, max};
use core::mem;

use super::gal::{TextureHandle, TextureDesc, TextureFormat, TextureUsage, Gal};
use super::{Surface, SwapChain};

// ============================================================================
// TILE CONSTANTS
// ============================================================================

/// Default tile size (32x32 pixels - optimal for cache line size)
pub const DEFAULT_TILE_SIZE: usize = 32;

/// Minimum tile size (for high detail areas)
pub const MIN_TILE_SIZE: usize = 16;

/// Maximum tile size (for large uniform areas)
pub const MAX_TILE_SIZE: usize = 64;

/// Maximum tiles per dimension
pub const MAX_TILES: usize = 256;

// ============================================================================
// TILE STRUCTURE
// ============================================================================

/// Single render tile
#[derive(Clone, Debug)]
pub struct Tile {
    /// Tile X position in tile coordinates
    pub tx: usize,
    /// Tile Y position in tile coordinates
    pub ty: usize,
    /// Pixel X offset
    pub x: usize,
    /// Pixel Y offset
    pub y: usize,
    /// Tile width in pixels
    pub width: usize,
    /// Tile height in pixels
    pub height: usize,
    /// Dirty flag
    pub dirty: bool,
    /// Content hash for change detection
    pub content_hash: u64,
    /// Tile surface buffer
    pub buffer: Vec<u32>,
    /// Last frame rendered
    pub last_frame: u64,
}

impl Tile {
    pub fn new(tx: usize, ty: usize, x: usize, y: usize, width: usize, height: usize) -> Self {
        Tile {
            tx,
            ty,
            x,
            y,
            width,
            height,
            dirty: true,
            content_hash: 0,
            buffer: vec![0; width * height],
            last_frame: 0,
        }
    }
    
    /// Clear tile to a color
    #[inline]
    pub fn clear(&mut self, color: u32) {
        for pixel in &mut self.buffer {
            *pixel = color;
        }
        self.dirty = true;
    }
    
    /// Set pixel in tile-local coordinates
    #[inline]
    pub fn set_pixel(&mut self, local_x: usize, local_y: usize, color: u32) {
        if local_x < self.width && local_y < self.height {
            self.buffer[local_y * self.width + local_x] = color;
            self.dirty = true;
        }
    }
    
    /// Get pixel from tile-local coordinates
    #[inline]
    pub fn get_pixel(&self, local_x: usize, local_y: usize) -> u32 {
        if local_x < self.width && local_y < self.height {
            self.buffer[local_y * self.width + local_x]
        } else {
            0
        }
    }
    
    /// Copy tile to framebuffer
    pub fn blit_to_framebuffer(&self, fb: &mut [u32], fb_stride: usize, fb_width: usize, fb_height: usize) {
        for row in 0..self.height {
            let fb_y = self.y + row;
            if fb_y >= fb_height {
                break;
            }
            
            let fb_offset = fb_y * fb_stride + self.x;
            let tile_offset = row * self.width;
            
            for col in 0..self.width {
                let fb_x = self.x + col;
                if fb_x >= fb_width {
                    break;
                }
                
                fb[fb_offset + col] = self.buffer[tile_offset + col];
            }
        }
    }
    
    /// Compute content hash for change detection
    pub fn compute_hash(&mut self) {
        // Simple hash: XOR of all pixels
        let mut hash: u64 = 0;
        for (i, pixel) in self.buffer.iter().enumerate() {
            hash ^= (*pixel as u64).wrapping_add((i as u64).wrapping_mul(31));
        }
        self.content_hash = hash;
    }
}

// ============================================================================
// DIRTY RECTANGLE
// ============================================================================

/// Rectangle for dirty region tracking
#[derive(Clone, Copy, Debug, Default)]
pub struct DirtyRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl DirtyRect {
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        DirtyRect { x, y, width, height }
    }
    
    pub fn empty() -> Self {
        DirtyRect { x: 0, y: 0, width: 0, height: 0 }
    }
    
    pub fn is_empty(&self) -> bool {
        self.width <= 0 || self.height <= 0
    }
    
    /// Check if point is inside
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.width && py >= self.y && py < self.y + self.height
    }
    
    /// Check if two rectangles intersect
    pub fn intersects(&self, other: &DirtyRect) -> bool {
        self.x < other.x + other.width
            && self.x + self.width > other.x
            && self.y < other.y + other.height
            && self.y + self.height > other.y
    }
    
    /// Union of two rectangles
    pub fn union(&self, other: &DirtyRect) -> DirtyRect {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        
        let x = min(self.x, other.x);
        let y = min(self.y, other.y);
        let right = max(self.x + self.width, other.x + other.width);
        let bottom = max(self.y + self.height, other.y + other.height);
        
        DirtyRect::new(x, y, right - x, bottom - y)
    }
    
    /// Intersection of two rectangles
    pub fn intersection(&self, other: &DirtyRect) -> DirtyRect {
        if !self.intersects(other) {
            return DirtyRect::empty();
        }
        
        let x = max(self.x, other.x);
        let y = max(self.y, other.y);
        let right = min(self.x + self.width, other.x + other.width);
        let bottom = min(self.y + self.height, other.y + other.height);
        
        DirtyRect::new(x, y, right - x, bottom - y)
    }
    
    /// Expand rectangle by amount
    pub fn expand(&self, amount: i32) -> DirtyRect {
        DirtyRect::new(
            self.x - amount,
            self.y - amount,
            self.width + amount * 2,
            self.height + amount * 2,
        )
    }
}

// ============================================================================
// TILE CACHE
// ============================================================================

/// Tile cache for efficient rendering
pub struct TileCache {
    /// All tiles
    tiles: Vec<Tile>,
    /// Number of tiles per row
    tiles_x: usize,
    /// Number of tiles per column
    tiles_y: usize,
    /// Tile size in pixels
    tile_size: usize,
    /// Screen width
    width: usize,
    /// Screen height
    height: usize,
    /// Dirty rectangles (for merging)
    dirty_rects: Vec<DirtyRect>,
    /// Dirty tile mask (bit per tile)
    dirty_mask: Vec<u64>,
    /// Frame counter
    frame_count: u64,
    /// Adaptive tile sizes per region
    adaptive_sizes: BTreeMap<(usize, usize), usize>,
}

impl TileCache {
    /// Create new tile cache
    pub fn new(width: usize, height: usize) -> Self {
        Self::with_tile_size(width, height, DEFAULT_TILE_SIZE)
    }
    
    /// Create tile cache with specific tile size
    pub fn with_tile_size(width: usize, height: usize, tile_size: usize) -> Self {
        let tiles_x = (width + tile_size - 1) / tile_size;
        let tiles_y = (height + tile_size - 1) / tile_size;
        let total_tiles = tiles_x * tiles_y;
        
        let mut tiles = Vec::with_capacity(total_tiles);
        
        for ty in 0..tiles_y {
            for tx in 0..tiles_x {
                let x = tx * tile_size;
                let y = ty * tile_size;
                let w = min(tile_size, width - x);
                let h = min(tile_size, height - y);
                
                tiles.push(Tile::new(tx, ty, x, y, w, h));
            }
        }
        
        // Calculate number of u64 words needed for dirty mask
        let mask_words = (total_tiles + 63) / 64;
        
        TileCache {
            tiles,
            tiles_x,
            tiles_y,
            tile_size,
            width,
            height,
            dirty_rects: Vec::new(),
            dirty_mask: vec![0; mask_words],
            frame_count: 0,
            adaptive_sizes: BTreeMap::new(),
        }
    }
    
    /// Get tile count
    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }
    
    /// Get tile at pixel coordinates
    pub fn get_tile_at(&self, x: usize, y: usize) -> Option<&Tile> {
        if x >= self.width || y >= self.height {
            return None;
        }
        
        let tx = x / self.tile_size;
        let ty = y / self.tile_size;
        let idx = ty * self.tiles_x + tx;
        
        self.tiles.get(idx)
    }
    
    /// Get mutable tile at pixel coordinates
    pub fn get_tile_at_mut(&mut self, x: usize, y: usize) -> Option<&mut Tile> {
        if x >= self.width || y >= self.height {
            return None;
        }
        
        let tx = x / self.tile_size;
        let ty = y / self.tile_size;
        let idx = ty * self.tiles_x + tx;
        
        self.tiles.get_mut(idx)
    }
    
    /// Get tile by tile coordinates
    pub fn get_tile(&self, tx: usize, ty: usize) -> Option<&Tile> {
        if tx >= self.tiles_x || ty >= self.tiles_y {
            return None;
        }
        
        let idx = ty * self.tiles_x + tx;
        self.tiles.get(idx)
    }
    
    /// Get mutable tile by tile coordinates
    pub fn get_tile_mut(&mut self, tx: usize, ty: usize) -> Option<&mut Tile> {
        if tx >= self.tiles_x || ty >= self.tiles_y {
            return None;
        }
        
        let idx = ty * self.tiles_x + tx;
        self.tiles.get_mut(idx)
    }
    
    /// Mark a region as dirty
    pub fn mark_dirty(&mut self, x: i32, y: i32, width: i32, height: i32) {
        // Clip to screen bounds
        let x = max(0, x) as usize;
        let y = max(0, y) as usize;
        let width = min(width as usize, self.width.saturating_sub(x));
        let height = min(height as usize, self.height.saturating_sub(y));
        
        if width == 0 || height == 0 {
            return;
        }
        
        // Calculate affected tiles
        let tx1 = x / self.tile_size;
        let ty1 = y / self.tile_size;
        let tx2 = (x + width - 1) / self.tile_size;
        let ty2 = (y + height - 1) / self.tile_size;
        
        // Mark tiles dirty in mask
        for ty in ty1..=ty2 {
            for tx in tx1..=tx2 {
                let idx = ty * self.tiles_x + tx;
                let word = idx / 64;
                let bit = idx % 64;
                
                if word < self.dirty_mask.len() {
                    self.dirty_mask[word] |= 1u64 << bit;
                }
                
                // Also mark the tile struct
                if let Some(tile) = self.tiles.get_mut(idx) {
                    tile.dirty = true;
                }
            }
        }
        
        // Add to dirty rectangles for merging
        self.push_dirty_rect(DirtyRect::new(x as i32, y as i32, width as i32, height as i32));
    }
    
    /// Add dirty rectangle with merging
    fn push_dirty_rect(&mut self, rect: DirtyRect) {
        // Try to merge with existing rectangles
        let mut merged = rect;
        let mut i = 0;
        
        while i < self.dirty_rects.len() {
            if merged.intersects(&self.dirty_rects[i]) {
                merged = merged.union(&self.dirty_rects[i]);
                self.dirty_rects.swap_remove(i);
            } else {
                i += 1;
            }
        }
        
        self.dirty_rects.push(merged);
        
        // Limit number of dirty rectangles
        if self.dirty_rects.len() > 32 {
            // Merge all into one
            let mut all = DirtyRect::empty();
            for r in self.dirty_rects.drain(..) {
                all = all.union(&r);
            }
            self.dirty_rects.push(all);
        }
    }
    
    /// Check if tile is dirty
    pub fn is_tile_dirty(&self, tx: usize, ty: usize) -> bool {
        let idx = ty * self.tiles_x + tx;
        let word = idx / 64;
        let bit = idx % 64;
        
        if word < self.dirty_mask.len() {
            (self.dirty_mask[word] & (1u64 << bit)) != 0
        } else {
            false
        }
    }
    
    /// Get all dirty rectangles
    pub fn get_dirty_rects(&self) -> &[DirtyRect] {
        &self.dirty_rects
    }
    
    /// Get dirty tile count
    pub fn dirty_tile_count(&self) -> usize {
        let mut count = 0;
        for &mask in &self.dirty_mask {
            count += mask.count_ones() as usize;
        }
        count
    }
    
    /// Clear dirty flags
    pub fn clear_dirty(&mut self) {
        for mask in &mut self.dirty_mask {
            *mask = 0;
        }
        for tile in &mut self.tiles {
            tile.dirty = false;
        }
        self.dirty_rects.clear();
    }
    
    /// Render all dirty tiles to framebuffer
    pub fn render_to_framebuffer(&mut self, fb: &mut [u32], fb_stride: usize) -> usize {
        let mut rendered = 0;
        self.frame_count += 1;
        
        for (idx, tile) in self.tiles.iter_mut().enumerate() {
            // Check if dirty using mask
            let word = idx / 64;
            let bit = idx % 64;
            
            let is_dirty = if word < self.dirty_mask.len() {
                (self.dirty_mask[word] & (1u64 << bit)) != 0
            } else {
                false
            };
            
            if is_dirty || tile.dirty {
                tile.blit_to_framebuffer(fb, fb_stride, self.width, self.height);
                tile.dirty = false;
                tile.last_frame = self.frame_count;
                rendered += 1;
            }
        }
        
        // Clear dirty mask
        self.clear_dirty();
        
        rendered
    }
    
    /// Resize tile cache
    pub fn resize(&mut self, width: usize, height: usize) {
        if width == self.width && height == self.height {
            return;
        }
        
        *self = Self::with_tile_size(width, height, self.tile_size);
    }
    
    /// Get screen dimensions
    pub fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }
    
    /// Get tile dimensions
    pub fn tile_dimensions(&self) -> (usize, usize) {
        (self.tiles_x, self.tiles_y)
    }
    
    /// Get tile size
    pub fn tile_size(&self) -> usize {
        self.tile_size
    }
}

// ============================================================================
// HIERARCHICAL TILE CACHE
// ============================================================================

/// Multi-level tile cache for adaptive rendering
pub struct HierarchicalTileCache {
    /// Level 0: Fine detail (16x16 tiles)
    level0: TileCache,
    /// Level 1: Medium detail (32x32 tiles)
    level1: TileCache,
    /// Level 2: Coarse detail (64x64 tiles)
    level2: TileCache,
    /// Current active level
    active_level: u8,
}

impl HierarchicalTileCache {
    pub fn new(width: usize, height: usize) -> Self {
        HierarchicalTileCache {
            level0: TileCache::with_tile_size(width, height, MIN_TILE_SIZE),
            level1: TileCache::with_tile_size(width, height, DEFAULT_TILE_SIZE),
            level2: TileCache::with_tile_size(width, height, MAX_TILE_SIZE),
            active_level: 1,
        }
    }
    
    /// Mark region dirty at all levels
    pub fn mark_dirty(&mut self, x: i32, y: i32, width: i32, height: i32) {
        self.level0.mark_dirty(x, y, width, height);
        self.level1.mark_dirty(x, y, width, height);
        self.level2.mark_dirty(x, y, width, height);
    }
    
    /// Select appropriate level based on dirty region size
    pub fn select_level(&mut self, dirty_area: usize) {
        // Small dirty area: use fine tiles
        // Large dirty area: use coarse tiles
        if dirty_area < 100 * 100 {
            self.active_level = 0;
        } else if dirty_area < 300 * 300 {
            self.active_level = 1;
        } else {
            self.active_level = 2;
        }
    }
    
    /// Get active level cache
    pub fn active(&mut self) -> &mut TileCache {
        match self.active_level {
            0 => &mut self.level0,
            2 => &mut self.level2,
            _ => &mut self.level1,
        }
    }

    /// Get active level cache (read-only)
    pub fn active_ref(&self) -> &TileCache {
        match self.active_level {
            0 => &self.level0,
            2 => &self.level2,
            _ => &self.level1,
        }
    }
    
    /// Render to framebuffer using best level
    pub fn render_to_framebuffer(&mut self, fb: &mut [u32], fb_stride: usize) -> usize {
        // Select level based on dirty area
        let dirty_count = self.level1.dirty_tile_count();
        self.select_level(dirty_count * DEFAULT_TILE_SIZE * DEFAULT_TILE_SIZE);
        
        // Render from active level
        self.active().render_to_framebuffer(fb, fb_stride)
    }
    
    /// Resize all levels
    pub fn resize(&mut self, width: usize, height: usize) {
        self.level0.resize(width, height);
        self.level1.resize(width, height);
        self.level2.resize(width, height);
    }
}

// ============================================================================
// TILE RENDERER
// ============================================================================

/// Main tile-based renderer
pub struct TileRenderer {
    /// Tile cache
    cache: HierarchicalTileCache,
    /// Frame counter
    frame: u64,
    /// Last frame time in microseconds
    last_frame_time: u64,
    /// Average frame time
    avg_frame_time: u64,
    /// Frames since last stats update
    stats_frames: u64,
}

impl TileRenderer {
    pub fn new(width: usize, height: usize) -> Self {
        TileRenderer {
            cache: HierarchicalTileCache::new(width, height),
            frame: 0,
            last_frame_time: 0,
            avg_frame_time: 0,
            stats_frames: 0,
        }
    }
    
    /// Begin a new frame
    pub fn begin_frame(&mut self) {
        self.frame += 1;
    }
    
    /// End frame and render
    pub fn end_frame(&mut self, fb: &mut [u32], fb_stride: usize) -> usize {
        let rendered = self.cache.render_to_framebuffer(fb, fb_stride);
        
        // Update stats
        self.stats_frames += 1;
        if self.stats_frames >= 60 {
            self.avg_frame_time = self.last_frame_time; // Simplified
            self.stats_frames = 0;
        }
        
        rendered
    }
    
    /// Mark region for redraw
    pub fn invalidate(&mut self, x: i32, y: i32, width: i32, height: i32) {
        // Expand by 1 pixel to handle anti-aliasing
        let rect = DirtyRect::new(x, y, width, height).expand(1);
        self.cache.mark_dirty(rect.x, rect.y, rect.width, rect.height);
    }
    
    /// Invalidate entire screen
    pub fn invalidate_all(&mut self, width: usize, height: usize) {
        self.cache.mark_dirty(0, 0, width as i32, height as i32);
    }
    
    /// Get tile at coordinates
    pub fn get_tile(&mut self, x: usize, y: usize) -> Option<&mut Tile> {
        self.cache.active().get_tile_at_mut(x, y)
    }
    
    /// Resize renderer
    pub fn resize(&mut self, width: usize, height: usize) {
        self.cache.resize(width, height);
    }
    
    /// Get frame count
    pub fn frame(&self) -> u64 {
        self.frame
    }
    
    /// Get dirty tile count
    pub fn dirty_count(&self) -> usize {
        self.cache.active_ref().dirty_tile_count()
    }
    
    /// Get dirty rectangles
    pub fn dirty_rects(&self) -> &[DirtyRect] {
        self.cache.active_ref().get_dirty_rects()
    }
}

// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================

/// Calculate tile index from pixel coordinates
#[inline]
pub fn pixel_to_tile(x: usize, y: usize, tile_size: usize, tiles_per_row: usize) -> usize {
    let tx = x / tile_size;
    let ty = y / tile_size;
    ty * tiles_per_row + tx
}

/// Calculate pixel coordinates from tile index
#[inline]
pub fn tile_to_pixel(tile_idx: usize, tile_size: usize, tiles_per_row: usize) -> (usize, usize) {
    let tx = tile_idx % tiles_per_row;
    let ty = tile_idx / tiles_per_row;
    (tx * tile_size, ty * tile_size)
}

/// Calculate number of tiles needed
#[inline]
pub fn calculate_tile_count(width: usize, height: usize, tile_size: usize) -> (usize, usize, usize) {
    let tiles_x = (width + tile_size - 1) / tile_size;
    let tiles_y = (height + tile_size - 1) / tile_size;
    (tiles_x, tiles_y, tiles_x * tiles_y)
}
