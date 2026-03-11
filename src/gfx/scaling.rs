//! # DPI Scaling System
//!
//! Resolution-aware scaling for high-DPI displays.
//! All UI elements should use logical pixels that are scaled to physical pixels at render time.
//!
//! ## Scaling Factor Table
//!
//! | Resolution | Scale Factor | Example |
//! |------------|--------------|---------|
//! | 1080p      | 1.0x (100)   | 28px titlebar = 28px |
//! | 1440p      | 1.25x (125)  | 28px titlebar = 35px |
//! | 2160p/4K   | 1.5x (150)   | 28px titlebar = 42px |
//! | 5K+        | 2.0x (200)   | 28px titlebar = 56px |
//!
//! ## Usage
//!
//! ```rust
//! use crate::gfx::scaling::{scale, scale_f32, LogicalPx, PhysicalPx};
//!
//! // Convert logical pixels to physical pixels
//! let titlebar_height = scale(28);  // Returns scaled value
//!
//! // For floating point calculations
//! let icon_size = scale_f32(48.0);
//! ```

use core::sync::atomic::{AtomicU32, Ordering};

// ============================================================================
// SCALE FACTOR
// ============================================================================

/// Global scale factor stored as percentage (100 = 1.0x, 125 = 1.25x, etc.)
/// Using integer percentage avoids floating point in atomic operations.
pub static SCALE_FACTOR: AtomicU32 = AtomicU32::new(100);

/// Minimum supported scale factor (1.0x)
pub const SCALE_MIN: u32 = 100;

/// Maximum supported scale factor (3.0x)
pub const SCALE_MAX: u32 = 300;

// ============================================================================
// LOGICAL & PHYSICAL PIXEL TYPES
// ============================================================================

/// Logical pixel value - resolution independent.
/// These values are what you design with and will be scaled at render time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LogicalPx(pub i32);

impl LogicalPx {
    /// Convert to physical pixels using current scale factor.
    #[inline]
    pub fn to_physical(self) -> PhysicalPx {
        PhysicalPx(scale(self.0))
    }

    /// Create from raw value.
    #[inline]
    pub const fn new(value: i32) -> Self {
        Self(value)
    }
}

/// Physical pixel value - actual screen pixels.
/// These are the final values written to the framebuffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysicalPx(pub i32);

impl PhysicalPx {
    /// Convert back to logical pixels (for hit testing).
    #[inline]
    pub fn to_logical(self) -> LogicalPx {
        LogicalPx(unscale(self.0))
    }

    /// Create from raw value.
    #[inline]
    pub const fn new(value: i32) -> Self {
        Self(value)
    }
}

// ============================================================================
// INITIALIZATION
// ============================================================================

/// Initialize scale factor based on screen resolution.
/// Should be called once at compositor startup.
///
/// # Arguments
/// * `width` - Screen width in pixels
/// * `height` - Screen height in pixels
///
/// # Scale Factor Selection
/// Based on vertical resolution (height) as it's the primary indicator of display density.
pub fn init_from_resolution(width: u32, height: u32) {
    let factor = determine_scale_factor(width, height);
    SCALE_FACTOR.store(factor, Ordering::SeqCst);

    #[cfg(feature = "serial_debug")]
    crate::serial_println!(
        "[SCALING] Resolution {}x{} -> scale factor {}.{:02}x",
        width,
        height,
        factor / 100,
        factor % 100
    );
}

/// Determine appropriate scale factor for given resolution.
fn determine_scale_factor(width: u32, height: u32) -> u32 {
    // Primary: use height as the main indicator
    // Secondary: consider width for ultra-wide displays
    let height_factor = match height {
        0..=900 => 100,     // 720p, 900p: 1.0x
        901..=1080 => 100,  // 1080p: 1.0x
        1081..=1440 => 125, // 1440p: 1.25x
        1441..=2160 => 150, // 4K: 1.5x
        2161..=2880 => 175, // 5K: 1.75x
        _ => 200,           // 6K+: 2.0x
    };

    // Adjust for ultra-wide displays (21:9 or wider)
    // These often have lower pixel density despite high resolution
    let aspect_ratio = if height > 0 {
        (width * 100) / height
    } else {
        177
    };

    if aspect_ratio > 220 {
        // Ultra-wide (21:9 = 233): reduce scale slightly
        (height_factor * 90) / 100
    } else {
        height_factor
    }
}

/// Manually set the scale factor (for user preferences).
pub fn set_scale_factor(factor: u32) {
    let clamped = factor.clamp(SCALE_MIN, SCALE_MAX);
    SCALE_FACTOR.store(clamped, Ordering::SeqCst);
}

/// Get current scale factor as percentage (100 = 1.0x).
#[inline]
pub fn get_scale_factor() -> u32 {
    SCALE_FACTOR.load(Ordering::SeqCst)
}

/// Get current scale factor as floating point (1.0, 1.25, 1.5, etc.).
#[inline]
pub fn get_scale_factor_f32() -> f32 {
    SCALE_FACTOR.load(Ordering::SeqCst) as f32 / 100.0
}

// ============================================================================
// SCALING FUNCTIONS
// ============================================================================

/// Scale a logical pixel value to physical pixels.
/// This is the primary function for UI layout.
///
/// # Example
/// ```rust
/// let titlebar_height = scale(28);  // 28 logical -> 35 physical at 1.25x
/// ```
#[inline]
pub fn scale(logical: i32) -> i32 {
    let factor = SCALE_FACTOR.load(Ordering::SeqCst) as i32;
    (logical * factor) / 100
}

/// Scale a floating point value.
#[inline]
pub fn scale_f32(logical: f32) -> f32 {
    let factor = SCALE_FACTOR.load(Ordering::SeqCst) as f32 / 100.0;
    logical * factor
}

/// Scale an unsigned value.
#[inline]
pub fn scale_usize(logical: usize) -> usize {
    let factor = SCALE_FACTOR.load(Ordering::SeqCst) as usize;
    (logical * factor) / 100
}

/// Reverse scale: convert physical pixels back to logical (for hit testing).
#[inline]
pub fn unscale(physical: i32) -> i32 {
    let factor = SCALE_FACTOR.load(Ordering::SeqCst) as i32;
    if factor == 0 {
        return physical;
    }
    (physical * 100) / factor
}

/// Reverse scale for floating point.
#[inline]
pub fn unscale_f32(physical: f32) -> f32 {
    let factor = SCALE_FACTOR.load(Ordering::SeqCst) as f32 / 100.0;
    if factor == 0.0 {
        return physical;
    }
    physical / factor
}

// ============================================================================
// SCALED CONSTANTS
// ============================================================================

/// Get scaled titlebar height.
#[inline]
pub fn titlebar_height() -> i32 {
    scale(28)
}

/// Get scaled icon size (standard).
#[inline]
pub fn icon_size() -> i32 {
    scale(48)
}

/// Get scaled icon size (small).
#[inline]
pub fn icon_size_small() -> i32 {
    scale(24)
}

/// Get scaled icon size (large).
#[inline]
pub fn icon_size_large() -> i32 {
    scale(64)
}

/// Get scaled dock height.
#[inline]
pub fn dock_height() -> i32 {
    scale(70)
}

/// Get scaled panel height.
#[inline]
pub fn panel_height() -> i32 {
    scale(24)
}

/// Get scaled button radius.
#[inline]
pub fn button_radius() -> i32 {
    scale(7)
}

/// Get scaled border width.
#[inline]
pub fn border_width() -> i32 {
    scale(1).max(1) // At least 1 pixel
}

/// Get scaled scrollbar width.
#[inline]
pub fn scrollbar_width() -> i32 {
    scale(12)
}

/// Get scaled font size (normal).
#[inline]
pub fn font_size() -> i32 {
    scale(14)
}

/// Get scaled font size (small).
#[inline]
pub fn font_size_small() -> i32 {
    scale(12)
}

/// Get scaled font size (large).
#[inline]
pub fn font_size_large() -> i32 {
    scale(18)
}

/// Get scaled spacing (standard gap).
#[inline]
pub fn spacing() -> i32 {
    scale(8)
}

/// Get scaled padding (standard).
#[inline]
pub fn padding() -> i32 {
    scale(12)
}

// ============================================================================
// RECT SCALING HELPERS
// ============================================================================

/// Scale a rectangle from logical to physical coordinates.
#[inline]
pub fn scale_rect(x: i32, y: i32, w: i32, h: i32) -> (i32, i32, i32, i32) {
    (scale(x), scale(y), scale(w), scale(h))
}

/// Unscale a rectangle from physical to logical coordinates.
#[inline]
pub fn unscale_rect(x: i32, y: i32, w: i32, h: i32) -> (i32, i32, i32, i32) {
    (unscale(x), unscale(y), unscale(w), unscale(h))
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scale_100() {
        SCALE_FACTOR.store(100, Ordering::SeqCst);
        assert_eq!(scale(28), 28);
        assert_eq!(scale(100), 100);
    }

    #[test]
    fn test_scale_125() {
        SCALE_FACTOR.store(125, Ordering::SeqCst);
        assert_eq!(scale(28), 35);
        assert_eq!(scale(100), 125);
    }

    #[test]
    fn test_scale_150() {
        SCALE_FACTOR.store(150, Ordering::SeqCst);
        assert_eq!(scale(28), 42);
        assert_eq!(scale(100), 150);
    }

    #[test]
    fn test_unscale() {
        SCALE_FACTOR.store(125, Ordering::SeqCst);
        assert_eq!(unscale(125), 100);
    }
}
