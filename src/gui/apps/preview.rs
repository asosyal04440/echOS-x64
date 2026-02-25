//! # Preview Application
//!
//! Document and image preview app supporting multiple formats
//! Quick Look-style preview with zoom, rotation, and annotations

use alloc::boxed::Box;
use alloc::string::String;
use alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};
use crate::gui::widgets::Widget;
use crate::gui::Rect;

// ============================================================================
// PREVIEW CONSTANTS
// ============================================================================

/// Toolbar height
pub const TOOLBAR_HEIGHT: usize = 44;

/// Sidebar width (for thumbnails)
pub const SIDEBAR_WIDTH: usize = 160;

/// Status bar height
pub const STATUS_BAR_HEIGHT: usize = 24;

/// Default zoom
pub const DEFAULT_ZOOM: f32 = 1.0;

/// Min zoom
pub const MIN_ZOOM: f32 = 0.1;

/// Max zoom
pub const MAX_ZOOM: f32 = 10.0;

// ============================================================================
// DOCUMENT TYPE
// ============================================================================

/// Supported document types
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentType {
    Image,
    PDF,
    Text,
    Code,
    Markdown,
    HTML,
    RTF,
    Office,
    Unknown,
}

impl DocumentType {
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg" | "tiff" | "ico" => DocumentType::Image,
            "pdf" => DocumentType::PDF,
            "txt" => DocumentType::Text,
            "rs" | "c" | "cpp" | "h" | "hpp" | "py" | "js" | "ts" | "go" | "java" | "kt" | "swift" | "rb" | "php" | "css" | "scss" | "json" | "xml" | "yaml" | "yml" | "toml" => DocumentType::Code,
            "md" | "markdown" => DocumentType::Markdown,
            "html" | "htm" => DocumentType::HTML,
            "rtf" => DocumentType::RTF,
            "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" => DocumentType::Office,
            _ => DocumentType::Unknown,
        }
    }
    
    pub fn icon(&self) -> &'static str {
        match self {
            DocumentType::Image => "🖼",
            DocumentType::PDF => "📄",
            DocumentType::Text => "📝",
            DocumentType::Code => "💻",
            DocumentType::Markdown => "📑",
            DocumentType::HTML => "🌐",
            DocumentType::RTF => "📄",
            DocumentType::Office => "📊",
            DocumentType::Unknown => "📄",
        }
    }
}

// ============================================================================
// PREVIEW PAGE
// ============================================================================

/// A page in the document
#[derive(Clone, Debug)]
pub struct PreviewPage {
    /// Page number
    pub number: usize,
    /// Page width (original)
    pub width: usize,
    /// Page height (original)
    pub height: usize,
    /// Thumbnail
    pub thumbnail: Option<Vec<u32>>,
    /// Page content (for text documents)
    pub content: Option<String>,
    /// Annotations
    pub annotations: Vec<Annotation>,
}

impl PreviewPage {
    pub fn new(number: usize, width: usize, height: usize) -> Self {
        PreviewPage {
            number,
            width,
            height,
            thumbnail: None,
            content: None,
            annotations: Vec::new(),
        }
    }
    
    pub fn text_page(number: usize, content: String) -> Self {
        PreviewPage {
            number,
            width: 612,  // Letter width
            height: 792, // Letter height
            thumbnail: None,
            content: Some(content),
            annotations: Vec::new(),
        }
    }
}

// ============================================================================
// ANNOTATION
// ============================================================================

/// Document annotation
#[derive(Clone, Debug)]
pub struct Annotation {
    /// Annotation ID
    pub id: u32,
    /// Annotation type
    pub annotation_type: AnnotationType,
    /// Position (x, y, width, height)
    pub rect: (f32, f32, f32, f32),
    /// Page number
    pub page: usize,
    /// Content (text, etc.)
    pub content: String,
    /// Color
    pub color: u32,
    /// Author
    pub author: String,
    /// Created timestamp
    pub created: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnotationType {
    Highlight,
    Underline,
    Strikeout,
    Text,
    Freehand,
    Shape,
    Stamp,
}

// ============================================================================
// PREVIEW DOCUMENT
// ============================================================================

/// A document being previewed
#[derive(Clone, Debug)]
pub struct PreviewDocument {
    /// File path
    pub path: String,
    /// File name
    pub name: String,
    /// Document type
    pub doc_type: DocumentType,
    /// Total pages
    pub page_count: usize,
    /// Current page
    pub current_page: usize,
    /// Pages
    pub pages: Vec<PreviewPage>,
    /// Zoom level
    pub zoom: f32,
    /// Rotation (0, 90, 180, 270)
    pub rotation: u16,
    /// Scroll position
    pub scroll_x: usize,
    pub scroll_y: usize,
    /// Loading progress
    pub loading: bool,
    pub loading_progress: f32,
    /// Is modified
    pub modified: bool,
    /// Metadata
    pub metadata: DocumentMetadata,
}

#[derive(Clone, Debug)]
pub struct DocumentMetadata {
    pub title: String,
    pub author: String,
    pub subject: String,
    pub creator: String,
    pub created: String,
    pub modified: String,
    pub file_size: u64,
}

impl PreviewDocument {
    pub fn new(path: &str) -> Self {
        let name = path.rsplit('/').next().unwrap_or(path);
        let ext = name.rsplit('.').next().unwrap_or("");
        let doc_type = DocumentType::from_extension(ext);
        
        PreviewDocument {
            path: String::from(path),
            name: String::from(name),
            doc_type,
            page_count: 1,
            current_page: 0,
            pages: Vec::new(),
            zoom: DEFAULT_ZOOM,
            rotation: 0,
            scroll_x: 0,
            scroll_y: 0,
            loading: true,
            loading_progress: 0.0,
            modified: false,
            metadata: DocumentMetadata::default(),
        }
    }
    
    pub fn image(path: &str, width: usize, height: usize) -> Self {
        let mut doc = Self::new(path);
        doc.page_count = 1;
        doc.pages.push(PreviewPage::new(1, width, height));
        doc.loading = false;
        doc
    }
    
    pub fn text(path: &str, content: &str) -> Self {
        let mut doc = Self::new(path);
        doc.page_count = 1;
        doc.pages.push(PreviewPage::text_page(1, String::from(content)));
        doc.loading = false;
        doc
    }
    
    pub fn zoom_in(&mut self) {
        self.zoom = (self.zoom * 1.25).min(MAX_ZOOM);
    }
    
    pub fn zoom_out(&mut self) {
        self.zoom = (self.zoom / 1.25).max(MIN_ZOOM);
    }
    
    pub fn reset_zoom(&mut self) {
        self.zoom = DEFAULT_ZOOM;
    }
    
    pub fn fit_to_window(&mut self, window_width: usize, window_height: usize) {
        if let Some(page) = self.pages.get(self.current_page) {
            let (pw, ph) = self.get_rotated_size(page.width, page.height);
            let scale_x = window_width as f32 / pw as f32;
            let scale_y = window_height as f32 / ph as f32;
            self.zoom = scale_x.min(scale_y);
        }
    }
    
    pub fn rotate_left(&mut self) {
        self.rotation = (self.rotation + 270) % 360;
    }
    
    pub fn rotate_right(&mut self) {
        self.rotation = (self.rotation + 90) % 360;
    }
    
    fn get_rotated_size(&self, width: usize, height: usize) -> (usize, usize) {
        match self.rotation {
            90 | 270 => (height, width),
            _ => (width, height),
        }
    }
    
    pub fn next_page(&mut self) {
        if self.current_page < self.page_count - 1 {
            self.current_page += 1;
            self.scroll_y = 0;
        }
    }
    
    pub fn prev_page(&mut self) {
        if self.current_page > 0 {
            self.current_page -= 1;
            self.scroll_y = 0;
        }
    }
    
    pub fn go_to_page(&mut self, page: usize) {
        if page < self.page_count {
            self.current_page = page;
            self.scroll_y = 0;
        }
    }
}

impl DocumentMetadata {
    pub fn default() -> Self {
        DocumentMetadata {
            title: String::new(),
            author: String::new(),
            subject: String::new(),
            creator: String::new(),
            created: String::new(),
            modified: String::new(),
            file_size: 0,
        }
    }
}

// ============================================================================
// PREVIEW WINDOW
// ============================================================================

/// Preview window
pub struct PreviewWindow {
    /// Window rect
    pub rect: Rect,
    /// Current document
    pub document: Option<PreviewDocument>,
    /// Show sidebar
    pub show_sidebar: bool,
    /// Show toolbar
    pub show_toolbar: bool,
    /// Show status bar
    pub show_status_bar: bool,
    /// Annotation mode
    pub annotation_mode: Option<AnnotationType>,
    /// Current annotation color
    pub annotation_color: u32,
    /// Is in fullscreen
    pub fullscreen: bool,
    /// Slideshow mode
    pub slideshow: bool,
    /// Slideshow interval
    pub slideshow_interval: f32,
    /// Slideshow timer
    pub slideshow_timer: f32,
    /// Hovered toolbar button
    pub hovered_button: Option<usize>,
    /// Dragging
    pub dragging: bool,
    /// Drag start
    pub drag_start: (i32, i32),
}

impl PreviewWindow {
    pub fn new(rect: Rect) -> Self {
        PreviewWindow {
            rect,
            document: None,
            show_sidebar: true,
            show_toolbar: true,
            show_status_bar: true,
            annotation_mode: None,
            annotation_color: 0xFFFFFF00, // Yellow
            fullscreen: false,
            slideshow: false,
            slideshow_interval: 3.0,
            slideshow_timer: 0.0,
            hovered_button: None,
            dragging: false,
            drag_start: (0, 0),
        }
    }
    
    pub fn open_file(&mut self, path: &str) {
        // Create sample document based on type
        let ext = path.rsplit('.').next().unwrap_or("");
        
        self.document = Some(match DocumentType::from_extension(ext) {
            DocumentType::Image => {
                PreviewDocument::image(path, 1920, 1080)
            }
            DocumentType::Text | DocumentType::Code | DocumentType::Markdown => {
                PreviewDocument::text(path, "Sample document content for preview.\n\nThis is a text file that would be displayed in the preview window.\n\nYou can scroll, zoom, and navigate through the content.")
            }
            _ => {
                PreviewDocument::new(path)
            }
        });
    }
    
    pub fn close(&mut self) {
        self.document = None;
        self.fullscreen = false;
        self.slideshow = false;
    }
    
    pub fn update(&mut self, dt: f32) {
        // Update slideshow
        if self.slideshow {
            self.slideshow_timer += dt;
            if self.slideshow_timer >= self.slideshow_interval {
                self.slideshow_timer = 0.0;
                if let Some(ref mut doc) = self.document {
                    if doc.current_page < doc.page_count - 1 {
                        doc.next_page();
                    } else {
                        self.slideshow = false;
                    }
                }
            }
        }
        
        // Update loading
        if let Some(ref mut doc) = self.document {
            if doc.loading {
                doc.loading_progress += dt * 0.3;
                if doc.loading_progress >= 1.0 {
                    doc.loading = false;
                    doc.loading_progress = 1.0;
                }
            }
        }
    }
    
    /// Draw preview window
    pub fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let w = self.rect.width as usize;
        let h = self.rect.height as usize;
        
        // Background
        fb.draw_rect(x, y, w, h, Theme::WINDOW_BG.to_u32());
        
        if self.document.is_none() {
            // Empty state
            fb.draw_string(x + w / 2 - 40, y + h / 2 - 20, "No document open", Theme::TEXT_SECONDARY.to_u32());
            fb.draw_string(x + w / 2 - 60, y + h / 2, "Open a file to preview it", Theme::TEXT_SECONDARY.to_u32());
            return;
        }
        
        let doc = self.document.as_ref().unwrap();
        
        // Toolbar
        if self.show_toolbar {
            fb.draw_rect(x, y, w, TOOLBAR_HEIGHT, Theme::TOOLBAR_BG.to_u32());
            self.draw_toolbar(fb, x, y, w, doc);
        }
        
        // Sidebar
        let content_x = if self.show_sidebar {
            fb.draw_rect(x, y + TOOLBAR_HEIGHT, SIDEBAR_WIDTH, h - TOOLBAR_HEIGHT - STATUS_BAR_HEIGHT, Theme::SIDEBAR_BG.to_u32());
            self.draw_sidebar(fb, x, y + TOOLBAR_HEIGHT, doc);
            x + SIDEBAR_WIDTH
        } else {
            x
        };
        
        let content_w = if self.show_sidebar { w - SIDEBAR_WIDTH } else { w };
        let content_h = h - TOOLBAR_HEIGHT - STATUS_BAR_HEIGHT;
        let content_y = y + TOOLBAR_HEIGHT;
        
        // Content area
        self.draw_content(fb, content_x, content_y, content_w, content_h, doc);
        
        // Status bar
        if self.show_status_bar {
            let status_y = y + h - STATUS_BAR_HEIGHT;
            fb.draw_rect(x, status_y, w, STATUS_BAR_HEIGHT, Theme::TOOLBAR_BG.to_u32());
            self.draw_status_bar(fb, x, status_y, w, doc);
        }
    }
    
    fn draw_toolbar(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, doc: &PreviewDocument) {
        let mut btn_x = x + 8;
        
        // Close button
        fb.draw_rect(btn_x, y + 8, 28, 28, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(btn_x + 6, y + 12, "×", Theme::TEXT_PRIMARY.to_u32());
        btn_x += 36;
        
        // Navigation
        let nav_buttons = [("◀", "prev"), ("▶", "next")];
        for (icon, _) in nav_buttons {
            fb.draw_rect(btn_x, y + 8, 28, 28, Theme::SIDEBAR_BG.to_u32());
            fb.draw_string(btn_x + 6, y + 12, icon, Theme::TEXT_PRIMARY.to_u32());
            btn_x += 32;
        }
        
        // Zoom controls
        btn_x += 8;
        fb.draw_rect(btn_x, y + 8, 28, 28, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(btn_x + 6, y + 12, "−", Theme::TEXT_PRIMARY.to_u32());
        btn_x += 32;
        
        let zoom_text = format!("{:.0}%", doc.zoom * 100.0);
        fb.draw_string(btn_x, y + 12, &zoom_text, Theme::TEXT_PRIMARY.to_u32());
        btn_x += zoom_text.len() * 8 + 8;
        
        fb.draw_rect(btn_x, y + 8, 28, 28, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(btn_x + 6, y + 12, "+", Theme::TEXT_PRIMARY.to_u32());
        btn_x += 36;
        
        // Rotation
        fb.draw_rect(btn_x, y + 8, 28, 28, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(btn_x + 4, y + 12, "↺", Theme::TEXT_PRIMARY.to_u32());
        btn_x += 32;
        
        fb.draw_rect(btn_x, y + 8, 28, 28, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(btn_x + 4, y + 12, "↻", Theme::TEXT_PRIMARY.to_u32());
        btn_x += 40;
        
        // Document title
        let title = if doc.name.len() > 20 { format!("{}...", &doc.name[..17]) } else { doc.name.clone() };
        fb.draw_string(x + w / 2 - title.len() * 4, y + 12, &title, Theme::TEXT_PRIMARY.to_u32());
        
        // Right side buttons
        btn_x = x + w - 140;
        
        // Share
        fb.draw_rect(btn_x, y + 8, 28, 28, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(btn_x + 4, y + 12, "⬆", Theme::TEXT_PRIMARY.to_u32());
        btn_x += 32;
        
        // Annotations
        fb.draw_rect(btn_x, y + 8, 28, 28, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(btn_x + 4, y + 12, "✎", Theme::TEXT_PRIMARY.to_u32());
        btn_x += 32;
        
        // Fullscreen
        fb.draw_rect(btn_x, y + 8, 28, 28, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(btn_x + 4, y + 12, "⛶", Theme::TEXT_PRIMARY.to_u32());
    }
    
    fn draw_sidebar(&self, fb: &mut Framebuffer, x: usize, y: usize, doc: &PreviewDocument) {
        // Page thumbnails
        let thumb_height = 100;
        let thumb_width = SIDEBAR_WIDTH - 16;
        
        for (i, page) in doc.pages.iter().enumerate() {
            let thumb_y = y + i * (thumb_height + 8) + 8;
            
            if thumb_y + thumb_height > y + (self.rect.height as usize) - TOOLBAR_HEIGHT - STATUS_BAR_HEIGHT {
                break;
            }
            
            let is_selected = i == doc.current_page;
            let bg = if is_selected { Theme::ACCENT_PRIMARY.to_u32() } else { Theme::WINDOW_BG.to_u32() };
            
            // Thumbnail background
            fb.draw_rect(x + 8, thumb_y, thumb_width, thumb_height, bg);
            
            // Thumbnail content (placeholder)
            let thumb_content_y = thumb_y + 4;
            let thumb_content_h = thumb_height - 20;
            
            // Draw placeholder lines for text
            for line in 0..6 {
                let line_y = thumb_content_y + line * 12;
                let line_width = thumb_width - 16 - (line % 3) * 20;
                fb.draw_rect(x + 16, line_y, line_width, 8, Theme::TEXT_SECONDARY.to_u32());
            }
            
            // Page number
            let page_text = format!("Page {}", i + 1);
            fb.draw_string(x + 12, thumb_y + thumb_height - 14, &page_text, Theme::TEXT_SECONDARY.to_u32());
        }
        
        if doc.pages.is_empty() {
            fb.draw_string(x + 20, y + 20, "No pages", Theme::TEXT_SECONDARY.to_u32());
        }
    }
    
    fn draw_content(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize, doc: &PreviewDocument) {
        // Background
        let bg_color = if doc.doc_type == DocumentType::Image { 0xFF333333 } else { Theme::WINDOW_BG.to_u32() };
        fb.draw_rect(x, y, w, h, bg_color);
        
        if doc.loading {
            // Loading indicator
            let center_x = x + w / 2;
            let center_y = y + h / 2;
            
            fb.draw_string(center_x - 40, center_y - 8, "Loading...", Theme::TEXT_SECONDARY.to_u32());
            
            // Progress bar
            let bar_width = 200;
            let bar_x = center_x - bar_width / 2;
            let bar_y = center_y + 20;
            
            fb.draw_rect(bar_x, bar_y, bar_width, 4, Theme::BORDER.to_u32());
            fb.draw_rect(bar_x, bar_y, (bar_width as f32 * doc.loading_progress) as usize, 4, Theme::ACCENT_PRIMARY.to_u32());
            
            return;
        }
        
        // Draw based on document type
        match doc.doc_type {
            DocumentType::Image => {
                self.draw_image_content(fb, x, y, w, h, doc);
            }
            DocumentType::Text | DocumentType::Code | DocumentType::Markdown => {
                self.draw_text_content(fb, x, y, w, h, doc);
            }
            _ => {
                // Generic document display
                self.draw_generic_content(fb, x, y, w, h, doc);
            }
        }
        
        // Draw annotations
        for annotation in doc.pages.get(doc.current_page).map(|p| p.annotations.as_slice()).unwrap_or(&[]) {
            self.draw_annotation(fb, x, y, annotation, doc.zoom);
        }
    }
    
    fn draw_image_content(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize, doc: &PreviewDocument) {
        if let Some(page) = doc.pages.get(doc.current_page) {
            let (img_w, img_h) = doc.get_rotated_size(page.width, page.height);
            
            // Calculate scaled size
            let scaled_w = (img_w as f32 * doc.zoom) as usize;
            let scaled_h = (img_h as f32 * doc.zoom) as usize;
            
            // Center in viewport
            let draw_x = x + (w - scaled_w) / 2;
            let draw_y = y + (h - scaled_h) / 2;
            
            // Draw placeholder image
            // In reality, would draw actual image data
            for py in 0..scaled_h.min(h) {
                for px in 0..scaled_w.min(w) {
                    let screen_x = draw_x + px;
                    let screen_y = draw_y + py;
                    
                    if screen_x < x + w && screen_y < y + h {
                        // Checkerboard pattern for transparency indication
                        let checker = ((px / 8) + (py / 8)) % 2 == 0;
                        let color = if checker { 0xFF404040 } else { 0xFF505050 };
                        fb.plot_pixel(screen_x, screen_y, color);
                    }
                }
            }
            
            // Draw image border
            fb.draw_rect_outline(draw_x, draw_y, scaled_w.min(w), scaled_h.min(h), Theme::BORDER.to_u32());
        }
    }
    
    fn draw_text_content(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize, doc: &PreviewDocument) {
        if let Some(page) = doc.pages.get(doc.current_page) {
            if let Some(content) = &page.content {
                // Draw text with word wrap
                let margin = 20;
                let text_x = x + margin;
                let mut text_y = y + margin;
                let line_height = 18;
                let char_width = 8;
                let max_chars = (w - margin * 2) / char_width;
                
                for line in content.lines() {
                    if text_y + line_height > y + h - margin {
                        break;
                    }
                    
                    // Word wrap
                    let mut current_line = String::new();
                    for word in line.split_whitespace() {
                        if current_line.len() + word.len() + 1 > max_chars {
                            fb.draw_string(text_x, text_y, &current_line, Theme::TEXT_PRIMARY.to_u32());
                            text_y += line_height;
                            current_line = String::from(word);
                            current_line.push(' ');
                        } else {
                            current_line.push_str(word);
                            current_line.push(' ');
                        }
                        
                        if text_y + line_height > y + h - margin {
                            break;
                        }
                    }
                    
                    if !current_line.is_empty() && text_y + line_height <= y + h - margin {
                        fb.draw_string(text_x, text_y, &current_line, Theme::TEXT_PRIMARY.to_u32());
                        text_y += line_height;
                    }
                }
            }
        }
    }
    
    fn draw_generic_content(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize, doc: &PreviewDocument) {
        // Show document info
        let center_y = y + h / 2;
        
        fb.draw_string(x + w / 2 - 20, center_y - 40, doc.doc_type.icon(), Theme::TEXT_PRIMARY.to_u32());
        fb.draw_string(x + w / 2 - doc.name.len() * 4, center_y, &doc.name, Theme::TEXT_PRIMARY.to_u32());
        fb.draw_string(x + w / 2 - 40, center_y + 20, "Preview not available", Theme::TEXT_SECONDARY.to_u32());
    }
    
    fn draw_annotation(&self, fb: &mut Framebuffer, x: usize, y: usize, annotation: &Annotation, zoom: f32) {
        let ax = x + (annotation.rect.0 * zoom) as usize;
        let ay = y + (annotation.rect.1 * zoom) as usize;
        let aw = (annotation.rect.2 * zoom) as usize;
        let ah = (annotation.rect.3 * zoom) as usize;
        
        match annotation.annotation_type {
            AnnotationType::Highlight => {
                // Semi-transparent highlight
                for py in 0..ah {
                    for px in 0..aw {
                        let ptr = unsafe { 
                            (fb.base_addr as *mut u32).add((ay + py) * fb.pixels_per_scan_line + ax + px) 
                        };
                        let bg = unsafe { *ptr };
                        unsafe { *ptr = Self::blend_color(bg, annotation.color, 0.3); }
                    }
                }
            }
            AnnotationType::Underline => {
                fb.draw_rect(ax, ay + ah - 2, aw, 2, annotation.color);
            }
            AnnotationType::Strikeout => {
                fb.draw_rect(ax, ay + ah / 2 - 1, aw, 2, annotation.color);
            }
            AnnotationType::Text => {
                // Draw text annotation marker
                fb.draw_rect(ax, ay, 20, 20, annotation.color);
                fb.draw_string(ax + 4, ay + 2, "💬", Theme::TEXT_PRIMARY.to_u32());
            }
            _ => {}
        }
    }
    
    fn blend_color(bg: u32, fg: u32, alpha: f32) -> u32 {
        let br = ((bg >> 16) & 0xFF) as f32;
        let bg_ = ((bg >> 8) & 0xFF) as f32;
        let bb = (bg & 0xFF) as f32;
        
        let fr = ((fg >> 16) & 0xFF) as f32;
        let fg_ = ((fg >> 8) & 0xFF) as f32;
        let fb = (fg & 0xFF) as f32;
        
        let r = (br * (1.0 - alpha) + fr * alpha) as u32;
        let g = (bg_ * (1.0 - alpha) + fg_ * alpha) as u32;
        let b = (bb * (1.0 - alpha) + fb * alpha) as u32;
        
        (r << 16) | (g << 8) | b
    }
    
    fn draw_status_bar(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, doc: &PreviewDocument) {
        // Page info
        let page_info = format!("Page {} of {}", doc.current_page + 1, doc.page_count);
        fb.draw_string(x + 8, y + 4, &page_info, Theme::TEXT_SECONDARY.to_u32());
        
        // File size
        if doc.metadata.file_size > 0 {
            let size_text = Self::format_size(doc.metadata.file_size);
            fb.draw_string(x + w / 2 - size_text.len() * 4, y + 4, &size_text, Theme::TEXT_SECONDARY.to_u32());
        }
        
        // Zoom
        let zoom_text = format!("{:.0}%", doc.zoom * 100.0);
        fb.draw_string(x + w - zoom_text.len() * 8 - 8, y + 4, &zoom_text, Theme::TEXT_SECONDARY.to_u32());
    }
    
    fn format_size(size: u64) -> String {
        if size < 1024 {
            format!("{} B", size)
        } else if size < 1024 * 1024 {
            format!("{:.1} KB", size as f64 / 1024.0)
        } else if size < 1024 * 1024 * 1024 {
            format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
        } else {
            format!("{:.1} GB", size as f64 / (1024.0 * 1024.0 * 1024.0))
        }
    }
    
    /// Handle click
    pub fn on_click(&mut self, mx: i32, my: i32) -> PreviewAction {
        let x = self.rect.x;
        let y = self.rect.y;
        let w = self.rect.width;
        
        // Toolbar
        if my >= (y + 8) as i32 && my < (y + 36) as i32 {
            let mut btn_x = x + 8;
            
            // Close
            if mx >= btn_x as i32 && mx < (btn_x + 28) as i32 {
                self.close();
                return PreviewAction::Close;
            }
            btn_x += 36;
            
            // Prev
            if mx >= btn_x as i32 && mx < (btn_x + 28) as i32 {
                if let Some(ref mut doc) = self.document {
                    doc.prev_page();
                }
                return PreviewAction::None;
            }
            btn_x += 32;
            
            // Next
            if mx >= btn_x as i32 && mx < (btn_x + 28) as i32 {
                if let Some(ref mut doc) = self.document {
                    doc.next_page();
                }
                return PreviewAction::None;
            }
            btn_x += 40;
            
            // Zoom out
            if mx >= btn_x as i32 && mx < (btn_x + 28) as i32 {
                if let Some(ref mut doc) = self.document {
                    doc.zoom_out();
                }
                return PreviewAction::None;
            }
            btn_x += 32;
            
            // Skip zoom text
            btn_x += 40;
            
            // Zoom in
            if mx >= btn_x as i32 && mx < (btn_x + 28) as i32 {
                if let Some(ref mut doc) = self.document {
                    doc.zoom_in();
                }
                return PreviewAction::None;
            }
            btn_x += 36;
            
            // Rotate left
            if mx >= btn_x as i32 && mx < (btn_x + 28) as i32 {
                if let Some(ref mut doc) = self.document {
                    doc.rotate_left();
                }
                return PreviewAction::None;
            }
            btn_x += 32;
            
            // Rotate right
            if mx >= btn_x as i32 && mx < (btn_x + 28) as i32 {
                if let Some(ref mut doc) = self.document {
                    doc.rotate_right();
                }
                return PreviewAction::None;
            }
            
            // Right side buttons
            btn_x = x + w - 140;
            
            // Share
            if mx >= btn_x as i32 && mx < (btn_x + 28) as i32 {
                return PreviewAction::Share;
            }
            btn_x += 32;
            
            // Annotations
            if mx >= btn_x as i32 && mx < (btn_x + 28) as i32 {
                self.annotation_mode = if self.annotation_mode.is_some() { None } else { Some(AnnotationType::Highlight) };
                return PreviewAction::None;
            }
            btn_x += 32;
            
            // Fullscreen
            if mx >= btn_x as i32 && mx < (btn_x + 28) as i32 {
                self.fullscreen = !self.fullscreen;
                return PreviewAction::ToggleFullscreen;
            }
        }
        
        // Sidebar
        if self.show_sidebar && mx >= x && mx < x + SIDEBAR_WIDTH as i32 {
            let thumb_height = 100;
            let content_y = y + TOOLBAR_HEIGHT as i32;
            
            if let Some(ref doc) = self.document {
                for i in 0..doc.pages.len() {
                    let thumb_y = content_y + (i * (thumb_height + 8) + 8) as i32;
                    
                    if my >= thumb_y && my < thumb_y + thumb_height as i32 {
                        if let Some(ref mut doc) = self.document {
                            doc.go_to_page(i);
                        }
                        return PreviewAction::None;
                    }
                }
            }
        }
        
        PreviewAction::None
    }
    
    /// Handle key press
    pub fn on_key_press(&mut self, c: char) -> PreviewAction {
        match c {
            '+' | '=' => {
                if let Some(ref mut doc) = self.document {
                    doc.zoom_in();
                }
            }
            '-' => {
                if let Some(ref mut doc) = self.document {
                    doc.zoom_out();
                }
            }
            '0' => {
                if let Some(ref mut doc) = self.document {
                    doc.reset_zoom();
                }
            }
            '[' => {
                if let Some(ref mut doc) = self.document {
                    doc.rotate_left();
                }
            }
            ']' => {
                if let Some(ref mut doc) = self.document {
                    doc.rotate_right();
                }
            }
            '\x1b' => { // Escape
                if self.fullscreen {
                    self.fullscreen = false;
                    return PreviewAction::ToggleFullscreen;
                } else {
                    self.close();
                    return PreviewAction::Close;
                }
            }
            ' ' => { // Space - next page or start slideshow
                if let Some(ref mut doc) = self.document {
                    if doc.current_page < doc.page_count - 1 {
                        doc.next_page();
                    }
                }
            }
            _ => {}
        }
        
        PreviewAction::None
    }
    
    /// Resize
    pub fn resize(&mut self, width: usize, height: usize) {
        self.rect.width = width as i32;
        self.rect.height = height as i32;
    }
}

/// Preview actions
#[derive(Clone, Debug)]
pub enum PreviewAction {
    None,
    OpenFile(String),
    Close,
    Share,
    ToggleFullscreen,
    Annotate(AnnotationType),
}

// ============================================================================
// GLOBAL PREVIEW
// ============================================================================

lazy_static::lazy_static! {
    static ref PREVIEW: Mutex<PreviewWindow> = Mutex::new(PreviewWindow::new(Rect {
        x: 100,
        y: 100,
        width: 900,
        height: 700,
    }));
}

/// Initialize Preview
pub fn init() {
    crate::serial_println!("[GUI] Preview initialized");
}

/// Get Preview
pub fn get_preview() -> &'static Mutex<PreviewWindow> {
    &PREVIEW
}
