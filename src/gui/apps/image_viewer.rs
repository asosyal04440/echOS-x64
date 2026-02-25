//! # Image Viewer Application
//!
//! View images with zoom, pan, and slideshow support
//! Supports PNG and JPEG formats

use alloc::boxed::Box;
use alloc::string::String;
use alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};
use crate::gui::widgets::{Widget, Rect};

// ============================================================================
// IMAGE FORMAT
// ============================================================================

/// Supported image formats
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    Unknown,
    Png,
    Jpeg,
    Bmp,
    Gif,
    WebP,
}

impl ImageFormat {
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "png" => ImageFormat::Png,
            "jpg" | "jpeg" => ImageFormat::Jpeg,
            "bmp" => ImageFormat::Bmp,
            "gif" => ImageFormat::Gif,
            "webp" => ImageFormat::WebP,
            _ => ImageFormat::Unknown,
        }
    }
    
    pub fn extension(&self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
            ImageFormat::Bmp => "bmp",
            ImageFormat::Gif => "gif",
            ImageFormat::WebP => "webp",
            _ => "unknown",
        }
    }
}

// ============================================================================
// DECODED IMAGE
// ============================================================================

/// Decoded image data
#[derive(Clone)]
pub struct DecodedImage {
    /// Width in pixels
    pub width: usize,
    /// Height in pixels
    pub height: usize,
    /// Pixel data (RGBA)
    pub data: Vec<u32>,
    /// Original format
    pub format: ImageFormat,
    /// Has alpha channel
    pub has_alpha: bool,
}

impl DecodedImage {
    pub fn new(width: usize, height: usize) -> Self {
        DecodedImage {
            width,
            height,
            data: vec![0xFF000000; width * height], // Black with full alpha
            format: ImageFormat::Unknown,
            has_alpha: true,
        }
    }
    
    /// Create from raw pixel data
    pub fn from_pixels(width: usize, height: usize, pixels: Vec<u32>) -> Self {
        DecodedImage {
            width,
            height,
            data: pixels,
            format: ImageFormat::Unknown,
            has_alpha: true,
        }
    }
    
    /// Get pixel at position
    pub fn get_pixel(&self, x: usize, y: usize) -> u32 {
        if x < self.width && y < self.height {
            self.data[y * self.width + x]
        } else {
            0
        }
    }
    
    /// Set pixel at position
    pub fn set_pixel(&mut self, x: usize, y: usize, color: u32) {
        if x < self.width && y < self.height {
            self.data[y * self.width + x] = color;
        }
    }
    
    /// Scale image to new size (nearest neighbor)
    pub fn scale(&self, new_width: usize, new_height: usize) -> Self {
        if new_width == 0 || new_height == 0 {
            return DecodedImage::new(1, 1);
        }
        
        let mut scaled = DecodedImage::new(new_width, new_height);
        
        let x_ratio = self.width as f32 / new_width as f32;
        let y_ratio = self.height as f32 / new_height as f32;
        
        for y in 0..new_height {
            for x in 0..new_width {
                let src_x = (x as f32 * x_ratio) as usize;
                let src_y = (y as f32 * y_ratio) as usize;
                
                scaled.set_pixel(x, y, self.get_pixel(src_x, src_y));
            }
        }
        
        scaled.format = self.format;
        scaled.has_alpha = self.has_alpha;
        scaled
    }
    
    /// Scale image with bilinear interpolation
    pub fn scale_bilinear(&self, new_width: usize, new_height: usize) -> Self {
        if new_width == 0 || new_height == 0 {
            return DecodedImage::new(1, 1);
        }
        
        let mut scaled = DecodedImage::new(new_width, new_height);
        
        let x_ratio = (self.width - 1) as f32 / (new_width - 1).max(1) as f32;
        let y_ratio = (self.height - 1) as f32 / (new_height - 1).max(1) as f32;
        
        for y in 0..new_height {
            for x in 0..new_width {
                let src_x = x as f32 * x_ratio;
                let src_y = y as f32 * y_ratio;
                
                let x0 = src_x as usize;
                let y0 = src_y as usize;
                let x1 = (x0 + 1).min(self.width - 1);
                let y1 = (y0 + 1).min(self.height - 1);
                
                let x_frac = src_x - x0 as f32;
                let y_frac = src_y - y0 as f32;
                
                let p00 = self.get_pixel(x0, y0);
                let p01 = self.get_pixel(x1, y0);
                let p10 = self.get_pixel(x0, y1);
                let p11 = self.get_pixel(x1, y1);
                
                // Interpolate
                let r = Self::bilerp(
                    ((p00 >> 16) & 0xFF) as f32,
                    ((p01 >> 16) & 0xFF) as f32,
                    ((p10 >> 16) & 0xFF) as f32,
                    ((p11 >> 16) & 0xFF) as f32,
                    x_frac, y_frac
                ) as u32;
                
                let g = Self::bilerp(
                    ((p00 >> 8) & 0xFF) as f32,
                    ((p01 >> 8) & 0xFF) as f32,
                    ((p10 >> 8) & 0xFF) as f32,
                    ((p11 >> 8) & 0xFF) as f32,
                    x_frac, y_frac
                ) as u32;
                
                let b = Self::bilerp(
                    (p00 & 0xFF) as f32,
                    (p01 & 0xFF) as f32,
                    (p10 & 0xFF) as f32,
                    (p11 & 0xFF) as f32,
                    x_frac, y_frac
                ) as u32;
                
                let a = if self.has_alpha {
                    Self::bilerp(
                        ((p00 >> 24) & 0xFF) as f32,
                        ((p01 >> 24) & 0xFF) as f32,
                        ((p10 >> 24) & 0xFF) as f32,
                        ((p11 >> 24) & 0xFF) as f32,
                        x_frac, y_frac
                    ) as u32
                } else {
                    255
                };
                
                scaled.set_pixel(x, y, (a << 24) | (r << 16) | (g << 8) | b);
            }
        }
        
        scaled.format = self.format;
        scaled.has_alpha = self.has_alpha;
        scaled
    }
    
    fn bilerp(p00: f32, p01: f32, p10: f32, p11: f32, x: f32, y: f32) -> f32 {
        let top = p00 + (p01 - p00) * x;
        let bottom = p10 + (p11 - p10) * x;
        top + (bottom - top) * y
    }
    
    /// Rotate image 90 degrees clockwise
    pub fn rotate_90(&self) -> Self {
        let mut rotated = DecodedImage::new(self.height, self.width);
        
        for y in 0..self.height {
            for x in 0..self.width {
                let new_x = self.height - 1 - y;
                let new_y = x;
                rotated.set_pixel(new_x, new_y, self.get_pixel(x, y));
            }
        }
        
        rotated.format = self.format;
        rotated.has_alpha = self.has_alpha;
        rotated
    }
    
    /// Flip horizontal
    pub fn flip_h(&self) -> Self {
        let mut flipped = DecodedImage::new(self.width, self.height);
        
        for y in 0..self.height {
            for x in 0..self.width {
                flipped.set_pixel(self.width - 1 - x, y, self.get_pixel(x, y));
            }
        }
        
        flipped.format = self.format;
        flipped.has_alpha = self.has_alpha;
        flipped
    }
    
    /// Flip vertical
    pub fn flip_v(&self) -> Self {
        let mut flipped = DecodedImage::new(self.width, self.height);
        
        for y in 0..self.height {
            for x in 0..self.width {
                flipped.set_pixel(x, self.height - 1 - y, self.get_pixel(x, y));
            }
        }
        
        flipped.format = self.format;
        flipped.has_alpha = self.has_alpha;
        flipped
    }
}

// ============================================================================
// IMAGE DECODER
// ============================================================================

/// Image decoder trait
pub trait ImageDecoder {
    fn decode(&self, data: &[u8]) -> Option<DecodedImage>;
    fn can_decode(&self, format: ImageFormat) -> bool;
}

/// PNG decoder (simplified)
pub struct PngDecoder;

impl PngDecoder {
    pub fn new() -> Self {
        PngDecoder
    }
}

impl ImageDecoder for PngDecoder {
    fn decode(&self, data: &[u8]) -> Option<DecodedImage> {
        // Check PNG signature
        if data.len() < 8 {
            return None;
        }
        
        let signature: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        if data[..8] != signature {
            return None;
        }
        
        // Simplified PNG parsing - just extract IHDR for dimensions
        // Real implementation would need zlib decompression
        
        // Find IHDR chunk
        if data.len() < 33 {
            return None;
        }
        
        // IHDR is at offset 8, length is 13 bytes
        let width = ((data[16] as usize) << 24) | ((data[17] as usize) << 16) 
                  | ((data[18] as usize) << 8) | (data[19] as usize);
        let height = ((data[20] as usize) << 24) | ((data[21] as usize) << 16) 
                   | ((data[22] as usize) << 8) | (data[23] as usize);
        
        // Create placeholder image
        let mut image = DecodedImage::new(width, height);
        image.format = ImageFormat::Png;
        image.has_alpha = true;
        
        // Fill with gradient as placeholder (real decoder would decompress IDAT)
        for y in 0..height {
            for x in 0..width {
                let r = ((x * 255) / width.max(1)) as u32;
                let g = ((y * 255) / height.max(1)) as u32;
                let b = 128;
                image.set_pixel(x, y, (255 << 24) | (r << 16) | (g << 8) | b);
            }
        }
        
        Some(image)
    }
    
    fn can_decode(&self, format: ImageFormat) -> bool {
        format == ImageFormat::Png
    }
}

/// JPEG decoder (simplified)
pub struct JpegDecoder;

impl JpegDecoder {
    pub fn new() -> Self {
        JpegDecoder
    }
}

impl ImageDecoder for JpegDecoder {
    fn decode(&self, data: &[u8]) -> Option<DecodedImage> {
        // Check JPEG signature
        if data.len() < 2 {
            return None;
        }
        
        // JPEG starts with 0xFF 0xD8
        if data[0] != 0xFF || data[1] != 0xD8 {
            return None;
        }
        
        // Simplified - create placeholder
        // Real implementation would need Huffman decoding, IDCT, etc.
        
        let image = DecodedImage::new(800, 600);
        // Placeholder would be filled here
        
        Some(image)
    }
    
    fn can_decode(&self, format: ImageFormat) -> bool {
        format == ImageFormat::Jpeg
    }
}

// ============================================================================
// IMAGE VIEWER
// ============================================================================

/// Image Viewer Application
pub struct ImageViewer {
    /// Window position and size
    rect: Rect,
    /// Current image
    image: Option<DecodedImage>,
    /// Displayed image (scaled for view)
    display_image: Option<DecodedImage>,
    /// Current file path
    file_path: String,
    /// Zoom level (1.0 = 100%)
    zoom: f32,
    /// Pan offset
    pan_x: i32,
    pan_y: i32,
    /// Is panning
    is_panning: bool,
    /// Last pan position
    last_pan: (i32, i32),
    /// Fit mode
    fit_mode: FitMode,
    /// Image list for slideshow
    image_list: Vec<String>,
    /// Current image index
    current_index: usize,
    /// Slideshow active
    slideshow_active: bool,
    /// Slideshow interval (ms)
    slideshow_interval: u64,
    /// Background color
    bg_color: u32,
    /// Show toolbar
    show_toolbar: bool,
    /// Toolbar height
    toolbar_height: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FitMode {
    None,
    FitWindow,
    FitWidth,
    FitHeight,
}

impl ImageViewer {
    pub fn new() -> Self {
        ImageViewer {
            rect: Rect::new(200, 100, 800, 600),
            image: None,
            display_image: None,
            file_path: String::new(),
            zoom: 1.0,
            pan_x: 0,
            pan_y: 0,
            is_panning: false,
            last_pan: (0, 0),
            fit_mode: FitMode::FitWindow,
            image_list: Vec::new(),
            current_index: 0,
            slideshow_active: false,
            slideshow_interval: 3000,
            bg_color: 0x1A1A1A, // Dark gray
            show_toolbar: true,
            toolbar_height: 40,
        }
    }
    
    /// Load image from file
    pub fn load_image(&mut self, path: &str) -> bool {
        self.file_path = String::from(path);
        
        // Determine format
        let ext = path.rsplit('.').next().unwrap_or("");
        let format = ImageFormat::from_extension(ext);
        
        // VFS not available in no_std yet - use placeholder
        let data: Option<Vec<u8>> = None;
        
        if let Some(file_data) = data {
            // Try to decode
            let decoded = match format {
                ImageFormat::Png => PngDecoder::new().decode(&file_data),
                ImageFormat::Jpeg => JpegDecoder::new().decode(&file_data),
                _ => None,
            };
            
            if let Some(img) = decoded {
                self.image = Some(img);
                self.update_display();
                return true;
            }
        }
        
        // Create placeholder image
        let placeholder = self.create_placeholder();
        self.image = Some(placeholder);
        self.update_display();
        false
    }
    
    /// Create placeholder image
    fn create_placeholder(&self) -> DecodedImage {
        let mut img = DecodedImage::new(400, 300);
        
        // Draw checkerboard pattern
        for y in 0..300 {
            for x in 0..400 {
                let checker = ((x / 20) + (y / 20)) % 2 == 0;
                let color = if checker { 0x404040 } else { 0x303030 };
                img.set_pixel(x, y, 0xFF000000 | color);
            }
        }
        
        // Draw "No Image" text area
        for y in 130..170 {
            for x in 100..300 {
                img.set_pixel(x, y, 0xFF202020);
            }
        }
        
        img
    }
    
    /// Update display image based on zoom and fit mode
    fn update_display(&mut self) {
        if let Some(ref img) = self.image {
            let view_width = self.rect.width as usize;
            let view_height = (self.rect.height - self.toolbar_height as i32) as usize;
            
            let (new_width, new_height) = match self.fit_mode {
                FitMode::None => {
                    let w = (img.width as f32 * self.zoom) as usize;
                    let h = (img.height as f32 * self.zoom) as usize;
                    (w.max(1), h.max(1))
                }
                FitMode::FitWindow => {
                    let scale_x = view_width as f32 / img.width as f32;
                    let scale_y = view_height as f32 / img.height as f32;
                    let scale = scale_x.min(scale_y).min(4.0); // Max 400%
                    
                    let w = (img.width as f32 * scale) as usize;
                    let h = (img.height as f32 * scale) as usize;
                    (w.max(1), h.max(1))
                }
                FitMode::FitWidth => {
                    let scale = view_width as f32 / img.width as f32;
                    let w = view_width;
                    let h = (img.height as f32 * scale) as usize;
                    (w.max(1), h.max(1))
                }
                FitMode::FitHeight => {
                    let scale = view_height as f32 / img.height as f32;
                    let w = (img.width as f32 * scale) as usize;
                    let h = view_height;
                    (w.max(1), h.max(1))
                }
            };
            
            // Scale image
            if new_width != img.width || new_height != img.height {
                self.display_image = Some(img.scale_bilinear(new_width, new_height));
            } else {
                self.display_image = Some(img.clone());
            }
            
            // Center image
            self.pan_x = ((view_width as i32 - new_width as i32) / 2).max(0);
            self.pan_y = ((view_height as i32 - new_height as i32) / 2).max(0);
        }
    }
    
    /// Draw the image viewer
    pub fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let width = self.rect.width as usize;
        let height = self.rect.height as usize;
        
        // Window background
        fb.draw_rect(x, y, width, height, Theme::WINDOW_BG.to_u32());
        
        // Title bar
        fb.draw_rect(x, y, width, 32, Theme::TITLEBAR_BG.to_u32());
        
        let title = if self.file_path.is_empty() {
            String::from("Image Viewer")
        } else {
            let name = self.file_path.rsplit('/').next().unwrap_or(&self.file_path);
            format!("Image Viewer - {}", name)
        };
        fb.draw_string(x + 12, y + 8, &title, Theme::TEXT_PRIMARY.to_u32());
        
        // Close button
        fb.draw_rect(x + width - 28, y + 4, 24, 24, Theme::ERROR.to_u32());
        fb.draw_string(x + width - 20, y + 8, "×", Theme::TEXT_ON_ACCENT.to_u32());
        
        // Toolbar
        if self.show_toolbar {
            self.draw_toolbar(fb, x, y + 32, width);
        }
        
        // Image area
        let img_y = y + 32 + self.toolbar_height;
        let img_height = height - 32 - self.toolbar_height;
        
        // Background
        fb.draw_rect(x, img_y, width, img_height, self.bg_color);
        
        // Draw image
        if let Some(ref display) = self.display_image {
            self.draw_image(fb, display, x, img_y, width, img_height);
        }
        
        // Status bar
        let status_y = y + height - 24;
        fb.draw_rect(x, status_y, width, 24, Theme::TOOLBAR_BG.to_u32());
        
        if let Some(ref img) = self.image {
            let status = format!("{} × {}  |  {}%", img.width, img.height, (self.zoom * 100.0) as i32);
            fb.draw_string(x + 12, status_y + 4, &status, Theme::TEXT_SECONDARY.to_u32());
        }
        
        // Zoom indicator
        let zoom_text = format!("Zoom: {}%", (self.zoom * 100.0) as i32);
        fb.draw_string(x + width - 120, status_y + 4, &zoom_text, Theme::TEXT_SECONDARY.to_u32());
    }
    
    fn draw_toolbar(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize) {
        fb.draw_rect(x, y, width, self.toolbar_height, Theme::TOOLBAR_BG.to_u32());
        
        let mut btn_x = x + 8;
        let btn_y = y + 6;
        let btn_size = 28;
        
        // Previous
        self.draw_toolbar_button(fb, btn_x, btn_y, "◀", self.current_index > 0);
        btn_x += btn_size + 4;
        
        // Next
        self.draw_toolbar_button(fb, btn_x, btn_y, "▶", self.current_index < self.image_list.len().saturating_sub(1));
        btn_x += btn_size + 8;
        
        // Separator
        fb.draw_rect(btn_x, btn_y, 1, btn_size, Theme::BORDER.to_u32());
        btn_x += 8;
        
        // Zoom out
        self.draw_toolbar_button(fb, btn_x, btn_y, "−", self.zoom > 0.1);
        btn_x += btn_size + 2;
        
        // Zoom in
        self.draw_toolbar_button(fb, btn_x, btn_y, "+", self.zoom < 10.0);
        btn_x += btn_size + 2;
        
        // Reset zoom
        self.draw_toolbar_button(fb, btn_x, btn_y, "1:1", true);
        btn_x += btn_size + 8;
        
        // Separator
        fb.draw_rect(btn_x, btn_y, 1, btn_size, Theme::BORDER.to_u32());
        btn_x += 8;
        
        // Fit buttons
        let fits = [("Fit", FitMode::FitWindow), ("W", FitMode::FitWidth), ("H", FitMode::FitHeight), ("1:1", FitMode::None)];
        for (label, mode) in &fits {
            let is_active = *mode == self.fit_mode;
            self.draw_toolbar_button_active(fb, btn_x, btn_y, label, true, is_active);
            btn_x += btn_size + 2;
        }
        
        btn_x += 8;
        
        // Separator
        fb.draw_rect(btn_x, btn_y, 1, btn_size, Theme::BORDER.to_u32());
        btn_x += 8;
        
        // Rotate
        self.draw_toolbar_button(fb, btn_x, btn_y, "↻", self.image.is_some());
        btn_x += btn_size + 2;
        
        // Flip H
        self.draw_toolbar_button(fb, btn_x, btn_y, "⇆", self.image.is_some());
        btn_x += btn_size + 2;
        
        // Flip V
        self.draw_toolbar_button(fb, btn_x, btn_y, "⇅", self.image.is_some());
    }
    
    fn draw_toolbar_button(&self, fb: &mut Framebuffer, x: usize, y: usize, icon: &str, enabled: bool) {
        let color = if enabled { Theme::TEXT_PRIMARY.to_u32() } else { Theme::TEXT_DISABLED.to_u32() };
        fb.draw_rect(x, y, 28, 28, Theme::TRANSPARENT.to_u32());
        fb.draw_string(x + 6, y + 6, icon, color);
    }
    
    fn draw_toolbar_button_active(&self, fb: &mut Framebuffer, x: usize, y: usize, icon: &str, enabled: bool, active: bool) {
        let bg = if active { Theme::ACCENT_PRIMARY.to_u32() } else { Theme::TRANSPARENT.to_u32() };
        let color = if !enabled {
            Theme::TEXT_DISABLED.to_u32()
        } else if active {
            Theme::TEXT_ON_ACCENT.to_u32()
        } else {
            Theme::TEXT_PRIMARY.to_u32()
        };
        
        fb.draw_rect(x, y, 28, 28, bg);
        fb.draw_string(x + 6, y + 6, icon, color);
    }
    
    fn draw_image(&self, fb: &mut Framebuffer, img: &DecodedImage, x: usize, y: usize, width: usize, height: usize) {
        let img_x = x + self.pan_x as usize;
        let img_y = y + self.pan_y as usize;
        
        for py in 0..img.height {
            let fb_y = img_y + py;
            if fb_y < y || fb_y >= y + height {
                continue;
            }
            
            for px in 0..img.width {
                let fb_x = img_x + px;
                if fb_x < x || fb_x >= x + width {
                    continue;
                }
                
                let pixel = img.get_pixel(px, py);
                
                // Alpha blending with background
                let alpha = ((pixel >> 24) & 0xFF) as f32 / 255.0;
                
                if alpha >= 1.0 {
                    fb.plot_pixel(fb_x, fb_y, pixel);
                } else if alpha > 0.0 {
                    let bg = self.bg_color;
                    
                    let pr = ((pixel >> 16) & 0xFF) as f32;
                    let pg = ((pixel >> 8) & 0xFF) as f32;
                    let pb = (pixel & 0xFF) as f32;
                    
                    let br = ((bg >> 16) & 0xFF) as f32;
                    let bg_ = ((bg >> 8) & 0xFF) as f32;
                    let bb = (bg & 0xFF) as f32;
                    
                    let r = (pr * alpha + br * (1.0 - alpha)) as u32;
                    let g = (pg * alpha + bg_ * (1.0 - alpha)) as u32;
                    let b = (pb * alpha + bb * (1.0 - alpha)) as u32;
                    
                    fb.plot_pixel(fb_x, fb_y, (r << 16) | (g << 8) | b);
                }
            }
        }
    }
    
    /// Handle mouse down
    pub fn on_mouse_down(&mut self, mx: i32, my: i32) -> ImageViewerAction {
        // Close button
        let close_x = self.rect.x + self.rect.width - 28;
        if mx >= close_x && mx < close_x + 24 && my >= self.rect.y + 4 && my < self.rect.y + 28 {
            return ImageViewerAction::Close;
        }
        
        // Toolbar
        let toolbar_y = self.rect.y + 32;
        if my >= toolbar_y && my < toolbar_y + self.toolbar_height as i32 {
            let mut btn_x = self.rect.x + 8;
            
            // Previous
            if mx >= btn_x && mx < btn_x + 28 && self.current_index > 0 {
                return ImageViewerAction::Previous;
            }
            btn_x += 32;
            
            // Next
            if mx >= btn_x && mx < btn_x + 28 {
                return ImageViewerAction::Next;
            }
            btn_x += 36;
            
            // Zoom out
            if mx >= btn_x && mx < btn_x + 28 {
                self.zoom_out();
                return ImageViewerAction::ZoomChanged(self.zoom);
            }
            btn_x += 30;
            
            // Zoom in
            if mx >= btn_x && mx < btn_x + 28 {
                self.zoom_in();
                return ImageViewerAction::ZoomChanged(self.zoom);
            }
            btn_x += 30;
            
            // Reset
            if mx >= btn_x && mx < btn_x + 28 {
                self.zoom = 1.0;
                self.fit_mode = FitMode::None;
                self.update_display();
                return ImageViewerAction::ZoomChanged(self.zoom);
            }
        }
        
        // Image area - start pan
        let img_y = self.rect.y + 32 + self.toolbar_height as i32;
        if my >= img_y {
            self.is_panning = true;
            self.last_pan = (mx, my);
        }
        
        ImageViewerAction::None
    }
    
    /// Handle mouse move
    pub fn on_mouse_move(&mut self, mx: i32, my: i32) {
        if self.is_panning {
            let dx = mx - self.last_pan.0;
            let dy = my - self.last_pan.1;
            
            self.pan_x += dx;
            self.pan_y += dy;
            
            self.last_pan = (mx, my);
        }
    }
    
    /// Handle mouse up
    pub fn on_mouse_up(&mut self) {
        self.is_panning = false;
    }
    
    /// Handle scroll (zoom)
    pub fn on_scroll(&mut self, delta: i32) {
        if delta > 0 {
            self.zoom_in();
        } else {
            self.zoom_out();
        }
    }
    
    /// Zoom in
    pub fn zoom_in(&mut self) {
        self.zoom = (self.zoom * 1.25).min(10.0);
        self.fit_mode = FitMode::None;
        self.update_display();
    }
    
    /// Zoom out
    pub fn zoom_out(&mut self) {
        self.zoom = (self.zoom / 1.25).max(0.1);
        self.fit_mode = FitMode::None;
        self.update_display();
    }
    
    /// Set fit mode
    pub fn set_fit_mode(&mut self, mode: FitMode) {
        self.fit_mode = mode;
        self.update_display();
    }
    
    /// Rotate image
    pub fn rotate(&mut self) {
        if let Some(ref img) = self.image {
            self.image = Some(img.rotate_90());
            self.update_display();
        }
    }
    
    /// Flip horizontal
    pub fn flip_h(&mut self) {
        if let Some(ref img) = self.image {
            self.image = Some(img.flip_h());
            self.update_display();
        }
    }
    
    /// Flip vertical
    pub fn flip_v(&mut self) {
        if let Some(ref img) = self.image {
            self.image = Some(img.flip_v());
            self.update_display();
        }
    }
    
    /// Next image
    pub fn next_image(&mut self) {
        if self.current_index < self.image_list.len() - 1 {
            self.current_index += 1;
            let path = self.image_list[self.current_index].clone();
            self.load_image(&path);
        }
    }
    
    /// Previous image
    pub fn prev_image(&mut self) {
        if self.current_index > 0 {
            self.current_index -= 1;
            let path = self.image_list[self.current_index].clone();
            self.load_image(&path);
        }
    }
    
    /// Set image list for slideshow
    pub fn set_image_list(&mut self, images: Vec<String>) {
        self.image_list = images;
        self.current_index = 0;
    }
    
    /// Get rect
    pub fn rect(&self) -> Rect {
        self.rect
    }
    
    /// Set rect
    pub fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
        self.update_display();
    }
}

/// Actions from image viewer
#[derive(Clone, Debug)]
pub enum ImageViewerAction {
    None,
    Close,
    ZoomChanged(f32),
    Next,
    Previous,
    FileOpened(String),
}

impl Default for ImageViewer {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// GLOBAL IMAGE VIEWER
// ============================================================================

lazy_static::lazy_static! {
    static ref IMAGE_VIEWER: Mutex<ImageViewer> = Mutex::new(ImageViewer::new());
}

/// Get image viewer
pub fn get_viewer() -> &'static Mutex<ImageViewer> {
    &IMAGE_VIEWER
}

/// Initialize image viewer
pub fn init() {
    crate::serial_println!("[GUI] Image Viewer initialized");
}
