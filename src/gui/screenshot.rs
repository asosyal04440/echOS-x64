//! # Ekran Görüntüsü Aracı
//!
//! Seçim, açıklama ekleme ve kaydetme özellikleriyle ekran görüntüsü alma.
//! Tam ekran, pencere veya alan seçerek yakalama desteği.
//!
//! ## Çalışma Akışı
//!
//! ```text
//!  Kullanıcı Tetikler
//!        │
//!        ▼
//!  start_capture(mod)
//!        │
//!    ┌───┴────────────┬───────────────┐
//!    ▼                ▼               ▼
//! FullScreen      Selection        Timed
//!  (anında)       (fare ile       (geri sayım
//!   yakala)       alan seç)        sonra)
//!        │                ▼
//!        │       on_mouse_down / on_mouse_move
//!        │         SelectionRect oluştur
//!        │                ▼
//!        └────► capture_selection(fb)
//!                   framebuffer'dan
//!                   pikselleri kopyala
//!                         ▼
//!               show_toolbar = true
//!                         ▼
//!               [Kaydet / Kopyala / İptal]
//! ```

use alloc::boxed::Box;
use alloc::string::String;
use alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;
use libm::{sinf, cosf, atan2f};

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};

// ============================================================================
// EKRAN GÖRÜNTÜSÜ SABİTLERİ
// ============================================================================

/// Seçim kenarlık kalınlığı (piksel cinsinden)
pub const SELECTION_BORDER: usize = 2;

/// Seçim tutacağı boyutu (köşe ve kenar manipülasyon noktaları)
pub const HANDLE_SIZE: usize = 8;

/// Araç çubuğu yüksekliği
pub const TOOLBAR_HEIGHT: usize = 40;

// ============================================================================
// EKRAN GÖRÜNTÜSÜ MODU
// ============================================================================

/// Ekran görüntüsü alma modunu temsil eder.
///
/// ```text
/// ┌─────────────────────────────────────────┐
/// │           Capture Modları               │
/// ├──────────────┬──────────────────────────┤
/// │  FullScreen  │ Tüm ekranı anında yakala │
/// │  Window      │ Altındaki pencereyi seç  │
/// │  Selection   │ Fare ile alan çiz        │
/// │  Timed       │ 5 sn geri sayım + yakala │
/// └──────────────┴──────────────────────────┘
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenshotMode {
    /// Tüm ekranı yakala
    FullScreen,
    /// Belirli bir pencereyi yakala
    Window,
    /// Fare ile seçilen alanı yakala
    Selection,
    /// Gecikmeli yakalama (geri sayımlı)
    Timed,
}

// ============================================================================
// EKRAN GÖRÜNTÜSÜ SEÇİMİ
// ============================================================================

/// Ekranda seçilen dikdörtgen alanı temsil eder.
/// `from_points` ile başlangıç ve bitiş noktalarından,
/// negatif yönde çizim de desteklenecek şekilde normalleştirilerek oluşturulur.
#[derive(Clone, Copy, Debug)]
pub struct SelectionRect {
    pub x: i32,
    pub y: i32,
    pub width: usize,
    pub height: usize,
}

impl SelectionRect {
    pub fn new() -> Self {
        SelectionRect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        }
    }

    pub fn from_points(start: (i32, i32), end: (i32, i32)) -> Self {
        let x = start.0.min(end.0);
        let y = start.1.min(end.1);
        let width = (end.0 - start.0).abs() as usize;
        let height = (end.1 - start.1).abs() as usize;

        SelectionRect { x, y, width, height }
    }

    pub fn is_valid(&self) -> bool {
        self.width > 0 && self.height > 0
    }

    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.width as i32
            && py >= self.y && py < self.y + self.height as i32
    }
}

// ============================================================================
// EKRAN GÖRÜNTÜSÜ ARACI
// ============================================================================

/// Ekran görüntüsü yakalama, açıklama ekleme ve kaydetme aracı.
///
/// ## Açıklama Araçları (AnnotationMode)
/// ```text
///  Pen        → Serbest çizim (Bresenham çizgi algoritması)
///  Rectangle  → Dikdörtgen (dolu veya çerçeve)
///  Circle     → Daire (Pisagor mesafesiyle piksel testi)
///  Arrow      → Ok (çizgi + ok başı: atan2f açı hesabı)
///  Text       → Metin etiketi
///  Crop       → Kırpma modu
/// ```
pub struct ScreenshotTool {
    /// Araç aktif mi
    pub active: bool,
    /// Yakalama modu
    pub mode: ScreenshotMode,
    /// Seçim dikdörtgeni
    pub selection: SelectionRect,
    /// Seçimin başlangıç noktası (fare basıldığında kaydedilir)
    pub selection_start: Option<(i32, i32)>,
    /// Şu an seçim yapılıyor mu
    pub selecting: bool,
    /// Yakalanan görüntü arabelleği (ham piksel verileri)
    pub captured_buffer: Vec<u32>,
    /// Yakalanan görüntünün genişliği
    pub captured_width: usize,
    /// Yakalanan görüntünün yüksekliği
    pub captured_height: usize,
    /// Ekran genişliği
    pub screen_width: usize,
    /// Ekran yüksekliği
    pub screen_height: usize,
    /// Geri sayım zamanlayıcısı (Timed mod için, saniye)
    pub countdown: f32,
    /// Araç çubuğunu göster
    pub show_toolbar: bool,
    /// Araç çubuğunun konumu (x, y)
    pub toolbar_pos: (usize, usize),
    /// Fare altındaki pencere sınırı (Window modu için)
    pub hovered_window: Option<SelectionRect>,
    /// Açıklama modu
    pub annotation_mode: AnnotationMode,
    /// Mevcut açıklamalar listesi
    pub annotations: Vec<Annotation>,
    /// Açıklama rengi (0xAARRGGBB formatı)
    pub annotation_color: u32,
    /// Açıklama kalemi kalınlığı (piksel)
    pub annotation_size: usize,
    /// Kayıt yolu
    pub save_path: String,
    /// Dosya sayacı (screenshot_0001.png gibi isimlendirme için)
    pub file_counter: u32,
}

/// Ekran görüntüsü açıklama modu
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnotationMode {
    None,
    Pen,
    Rectangle,
    Circle,
    Arrow,
    Text,
    Crop,
}

/// Ekran görüntüsüne eklenebilecek açıklama türleri
#[derive(Clone, Debug)]
pub enum Annotation {
    Pen {
        points: Vec<(i32, i32)>,
        color: u32,
        size: usize,
    },
    Rectangle {
        x: i32,
        y: i32,
        width: usize,
        height: usize,
        color: u32,
        size: usize,
        filled: bool,
    },
    Circle {
        x: i32,
        y: i32,
        radius: usize,
        color: u32,
        size: usize,
        filled: bool,
    },
    Arrow {
        start: (i32, i32),
        end: (i32, i32),
        color: u32,
        size: usize,
    },
    Text {
        x: i32,
        y: i32,
        text: String,
        color: u32,
        size: usize,
    },
}

impl ScreenshotTool {
    pub fn new(screen_width: usize, screen_height: usize) -> Self {
        ScreenshotTool {
            active: false,
            mode: ScreenshotMode::Selection,
            selection: SelectionRect::new(),
            selection_start: None,
            selecting: false,
            captured_buffer: Vec::new(),
            captured_width: 0,
            captured_height: 0,
            screen_width,
            screen_height,
            countdown: 0.0,
            show_toolbar: false,
            toolbar_pos: (0, 0),
            hovered_window: None,
            annotation_mode: AnnotationMode::None,
            annotations: Vec::new(),
            annotation_color: 0xFFFF0000,
            annotation_size: 3,
            save_path: String::from("/home/pictures/screenshots"),
            file_counter: 0,
        }
    }

    /// Ekran görüntüsü yakalamayı başlat.
    /// FullScreen modunda anında yakalar; Timed modunda geri sayım başlatır.
    pub fn start_capture(&mut self, mode: ScreenshotMode) {
        self.active = true;
        self.mode = mode;
        self.selection = SelectionRect::new();
        self.selection_start = None;
        self.selecting = false;
        self.annotations.clear();
        self.annotation_mode = AnnotationMode::None;
        self.hovered_window = None;

        match mode {
            ScreenshotMode::FullScreen => {
                // Tam ekranı hemen yakala
                self.capture_full_screen();
            }
            ScreenshotMode::Timed => {
                // 5 saniyelik geri sayım başlat
                self.countdown = 5.0;
            }
            _ => {}
        }
    }

    /// Yakalamayı iptal et ve durumu sıfırla
    pub fn cancel(&mut self) {
        self.active = false;
        self.selecting = false;
        self.selection_start = None;
        self.captured_buffer.clear();
        self.annotations.clear();
    }

    /// Her kare güncelleme çağrısı. Geri sayımı azaltır ve süresi dolunca yakalar.
    pub fn update(&mut self, dt: f32) -> ScreenshotEvent {
        // Geri sayımı güncelle (Timed mod)
        if self.mode == ScreenshotMode::Timed && self.countdown > 0.0 {
            self.countdown -= dt;

            if self.countdown <= 0.0 {
                self.capture_full_screen();
                return ScreenshotEvent::Captured;
            }

            return ScreenshotEvent::Countdown(self.countdown as u32);
        }

        ScreenshotEvent::None
    }

    /// Tam ekranı yakala: seçim dikdörtgenini tüm ekran boyutuna ayarla
    pub fn capture_full_screen(&mut self) {
        self.captured_width = self.screen_width;
        self.captured_height = self.screen_height;
        self.captured_buffer.resize(self.screen_width * self.screen_height, 0);

        // Framebuffer'dan ekran piksellerini arabellege kopyala
        // Seçimi tam ekran boyutuna ayarla
        self.selection = SelectionRect {
            x: 0,
            y: 0,
            width: self.screen_width,
            height: self.screen_height,
        };

        self.show_toolbar = true;
        self.toolbar_pos = (self.screen_width / 2 - 150, self.screen_height - 60);
    }

    /// Seçilen alanı yakala. Framebuffer'daki piksel verilerini doğrudan okur.
    ///
    /// ## Piksel Kopyalama
    /// ```text
    /// Framebuffer (src):         captured_buffer (dst):
    /// [..src_x..]                [0..captured_width]
    ///  ┌──────────┐               ┌──────────┐
    ///  │          │   kopyala     │   dst    │
    ///  │  seçim   │ ──────────►  │  buffer  │
    ///  └──────────┘               └──────────┘
    /// src offset = src_y * pixels_per_scan_line + src_x
    /// ```
    pub fn capture_selection(&mut self, fb: &Framebuffer) {
        if !self.selection.is_valid() {
            return;
        }

        self.captured_width = self.selection.width;
        self.captured_height = self.selection.height;
        self.captured_buffer.resize(self.captured_width * self.captured_height, 0);

        // Seçilen alanı arabellege kopyala (unsafe: ham bellek okuma)
        for y in 0..self.captured_height {
            for x in 0..self.captured_width {
                let src_x = self.selection.x as usize + x;
                let src_y = self.selection.y as usize + y;

                if src_x < fb.width && src_y < fb.height {
                    let ptr = unsafe {
                        (fb.base_addr as *const u32).add(src_y * fb.pixels_per_scan_line + src_x)
                    };
                    self.captured_buffer[y * self.captured_width + x] = unsafe { *ptr };
                }
            }
        }

        self.show_toolbar = true;
        self.active = false;
    }

    /// Fare sol tuş basılınca çağrılır. Selection modunda seçim başlatır.
    pub fn on_mouse_down(&mut self, mx: i32, my: i32) -> ScreenshotEvent {
        if !self.active {
            return ScreenshotEvent::None;
        }

        match self.mode {
            ScreenshotMode::Selection => {
                self.selection_start = Some((mx, my));
                self.selecting = true;
            }
            ScreenshotMode::Window => {
                // Üzerinde durulan pencereyi seç
                if let Some(window) = self.hovered_window {
                    self.selection = window;
                    self.active = false;
                    return ScreenshotEvent::WindowSelected;
                }
            }
            _ => {}
        }

        ScreenshotEvent::None
    }

    /// Fare hareket ettiğinde çağrılır. Selection modunda canlı dikdörtgen güncellenir.
    pub fn on_mouse_move(&mut self, mx: i32, my: i32) {
        if !self.active {
            return;
        }

        match self.mode {
            ScreenshotMode::Selection => {
                if self.selecting {
                    if let Some(start) = self.selection_start {
                        self.selection = SelectionRect::from_points(start, (mx, my));
                    }
                }
            }
            ScreenshotMode::Window => {
                // Fare altındaki pencere sınırlarını güncelle
                // Gerçek uygulamada pencere yöneticisinden sorgulanır
                self.hovered_window = Some(SelectionRect {
                    x: (mx - 200).max(0),
                    y: (my - 150).max(0),
                    width: 400,
                    height: 300,
                });
            }
            _ => {}
        }
    }

    /// Fare sol tuşu bırakılınca çağrılır. Seçim tamamlandıysa yakalar.
    pub fn on_mouse_up(&mut self, fb: &Framebuffer) -> ScreenshotEvent {
        if !self.active {
            return ScreenshotEvent::None;
        }

        if self.mode == ScreenshotMode::Selection && self.selecting {
            self.selecting = false;

            if self.selection.is_valid() {
                self.capture_selection(fb);
                return ScreenshotEvent::Captured;
            }
        }

        ScreenshotEvent::None
    }

    /// Ekran görüntüsü katmanını çiz.
    ///
    /// ## Çizim Katmanları (Z-sırası)
    /// ```text
    /// 1. Karartma katmanı  (seçim dışı alanlar %30 kararır)
    /// 2. Seçim kenarlığı   (kesik çizgili + köşe tutacakları)
    /// 3. Geri sayım metni  (Timed mod)
    /// 4. Pencere vurgusu   (Window mod)
    /// 5. Araç çubuğu       (Kaydet / Kopyala / Açıklama araçları)
    /// 6. Açıklamalar       (Pen, Arrow, Shape vb.)
    /// ```
    pub fn draw(&self, fb: &mut Framebuffer) {
        if !self.active && !self.show_toolbar {
            return;
        }

        // Seçim dışını karart (alfa karıştırma ile)
        for y in 0..self.screen_height {
            for x in 0..self.screen_width {
                let ptr = unsafe { (fb.base_addr as *mut u32).add(y * fb.pixels_per_scan_line + x) };
                let bg = unsafe { *ptr };

                // Seçim içindeyse karartma
                let in_selection = self.selection.contains(x as i32, y as i32);

                if !in_selection {
                    let dimmed = Self::blend_color(bg, 0x000000, 0.3);
                    unsafe { *ptr = dimmed; }
                }
            }
        }

        // Seçim kenarlığını çiz
        if self.selection.is_valid() {
            self.draw_selection_border(fb);
        }

        // Geri sayım metnini çiz (Timed mod)
        if self.mode == ScreenshotMode::Timed && self.countdown > 0.0 {
            let text = format!("Capturing in {}...", self.countdown as u32 + 1);
            let text_x = self.screen_width / 2 - text.len() * 4;
            let text_y = self.screen_height / 2;

            // Arka plan kutusu
            fb.draw_rect(text_x - 20, text_y - 10, text.len() * 8 + 40, 40, 0xE0000000);
            fb.draw_string(text_x, text_y, &text, 0xFFFFFFFF);
        }

        // Pencere vurgusunu çiz (Window modu)
        if let Some(window) = self.hovered_window {
            self.draw_window_highlight(fb, window);
        }

        // Araç çubuğunu çiz
        if self.show_toolbar {
            self.draw_toolbar(fb);
        }

        // Açıklamaları çiz
        for annotation in &self.annotations {
            self.draw_annotation(fb, annotation);
        }
    }

    fn draw_selection_border(&self, fb: &mut Framebuffer) {
        let sel = &self.selection;
        let color = 0xFFFFFFFF;

        // Kesik çizgi efekti: her 8 pikselde 4'ü çiz, 4'ünü atla
        for i in 0..sel.width {
            if i % 8 < 4 {
                // Üst kenar
                fb.plot_pixel(sel.x as usize + i, sel.y as usize, color);
                // Alt kenar
                fb.plot_pixel(sel.x as usize + i, sel.y as usize + sel.height - 1, color);
            }
        }

        for i in 0..sel.height {
            if i % 8 < 4 {
                // Sol kenar
                fb.plot_pixel(sel.x as usize, sel.y as usize + i, color);
                // Sağ kenar
                fb.plot_pixel(sel.x as usize + sel.width - 1, sel.y as usize + i, color);
            }
        }

        // Yeniden boyutlandırma tutacaklarını çiz (4 köşe)
        let handle_color = 0xFFFFFFFF;
        let handle_size = HANDLE_SIZE;

        // Dört köşe koordinatları
        let corners = [
            (sel.x, sel.y),
            (sel.x + sel.width as i32 - handle_size as i32, sel.y),
            (sel.x, sel.y + sel.height as i32 - handle_size as i32),
            (sel.x + sel.width as i32 - handle_size as i32, sel.y + sel.height as i32 - handle_size as i32),
        ];

        for (hx, hy) in corners {
            fb.draw_rect(hx as usize, hy as usize, handle_size, handle_size, handle_color);
        }

        // Boyut metnini çiz (örn: "1280x720")
        let dim_text = format!("{}x{}", sel.width, sel.height);
        let text_x = sel.x as usize + sel.width / 2 - dim_text.len() * 4;
        let text_y = sel.y as usize + sel.height + 8;

        if text_y < self.screen_height - 20 {
            fb.draw_rect(text_x - 4, text_y - 2, dim_text.len() * 8 + 8, 16, 0xE0000000);
            fb.draw_string(text_x, text_y, &dim_text, 0xFFFFFFFF);
        }
    }

    fn draw_window_highlight(&self, fb: &mut Framebuffer, window: SelectionRect) {
        // Pencere sınırına vurgu kenarlığı çiz
        let color = Theme::ACCENT_PRIMARY.to_u32();

        for i in 0..window.width {
            fb.plot_pixel(window.x as usize + i, window.y as usize, color);
            fb.plot_pixel(window.x as usize + i, window.y as usize + window.height - 1, color);
        }

        for i in 0..window.height {
            fb.plot_pixel(window.x as usize, window.y as usize + i, color);
            fb.plot_pixel(window.x as usize + window.width - 1, window.y as usize + i, color);
        }
    }

    fn draw_toolbar(&self, fb: &mut Framebuffer) {
        let (x, y) = self.toolbar_pos;
        let width = 300;
        let height = TOOLBAR_HEIGHT;

        // Araç çubuğu arka planı
        fb.draw_rect(x, y, width, height, 0xE0202020);
        fb.draw_rect_outline(x, y, width, height, 0x40888888);

        // Düğmeler soldan sağa yerleştirilir
        let mut btn_x = x + 8;

        // Kaydet düğmesi
        fb.draw_rect(btn_x, y + 8, 60, 24, Theme::ACCENT_PRIMARY.to_u32());
        fb.draw_string(btn_x + 8, y + 12, "Save", 0xFFFFFFFF);
        btn_x += 68;

        // Kopyala düğmesi
        fb.draw_rect(btn_x, y + 8, 60, 24, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(btn_x + 8, y + 12, "Copy", Theme::TEXT_PRIMARY.to_u32());
        btn_x += 68;

        // Açıklama araçları
        let tools = [("✏", AnnotationMode::Pen), ("▢", AnnotationMode::Rectangle),
                     ("○", AnnotationMode::Circle), ("→", AnnotationMode::Arrow)];

        for (icon, mode) in tools {
            let bg = if self.annotation_mode == mode { Theme::ACCENT_PRIMARY.to_u32() } else { Theme::SIDEBAR_BG.to_u32() };
            fb.draw_rect(btn_x, y + 8, 24, 24, bg);
            fb.draw_string(btn_x + 4, y + 12, icon, Theme::TEXT_PRIMARY.to_u32());
            btn_x += 28;
        }

        // İptal düğmesi
        fb.draw_rect(x + width - 60, y + 8, 52, 24, Theme::ERROR.to_u32());
        fb.draw_string(x + width - 52, y + 12, "Cancel", 0xFFFFFFFF);
    }

    fn draw_annotation(&self, fb: &mut Framebuffer, annotation: &Annotation) {
        match annotation {
            Annotation::Pen { points, color, size } => {
                for i in 1..points.len() {
                    let p1 = points[i - 1];
                    let p2 = points[i];
                    self.draw_line(fb, p1, p2, *color, *size);
                }
            }
            Annotation::Rectangle { x, y, width, height, color, size, filled } => {
                if *filled {
                    fb.draw_rect(*x as usize, *y as usize, *width, *height, *color);
                } else {
                    for i in 0..*size {
                        fb.draw_rect_outline(
                            (*x as usize).saturating_sub(i),
                            (*y as usize).saturating_sub(i),
                            *width + i * 2,
                            *height + i * 2,
                            *color
                        );
                    }
                }
            }
            Annotation::Circle { x, y, radius, color, size, filled } => {
                if *filled {
                    for py in 0..*radius * 2 {
                        for px in 0..*radius * 2 {
                            let dx = px as i32 - *radius as i32;
                            let dy = py as i32 - *radius as i32;
                            if dx * dx + dy * dy <= *radius as i32 * *radius as i32 {
                                fb.plot_pixel(*x as usize + px, *y as usize + py, *color);
                            }
                        }
                    }
                } else {
                    for py in 0..*radius * 2 {
                        for px in 0..*radius * 2 {
                            let dx = px as i32 - *radius as i32;
                            let dy = py as i32 - *radius as i32;
                            let dist = dx * dx + dy * dy;
                            if dist <= *radius as i32 * *radius as i32
                                && dist > (*radius as i32 - *size as i32) * (*radius as i32 - *size as i32) {
                                fb.plot_pixel(*x as usize + px, *y as usize + py, *color);
                            }
                        }
                    }
                }
            }
            Annotation::Arrow { start, end, color, size } => {
                self.draw_line(fb, *start, *end, *color, *size);
                // Ok başını çiz: atan2f ile açı hesapla, iki yan kolunu çiz
                let angle = atan2f((end.1 - start.1) as f32, (end.0 - start.0) as f32);
                let head_len = *size as f32 * 3.0;

                for i in 0..3 {
                    let a = angle + core::f32::consts::PI + (i as f32 - 1.0) * 0.3;
                    let hx = end.0 as f32 + cosf(a) * head_len;
                    let hy = end.1 as f32 + sinf(a) * head_len;
                    self.draw_line(fb, *end, (hx as i32, hy as i32), *color, *size);
                }
            }
            Annotation::Text { x, y, text, color, size: _ } => {
                fb.draw_string(*x as usize, *y as usize, text, *color);
            }
        }
    }

    /// Bresenham çizgi algoritması ile iki nokta arasına `size` kalınlığında çizgi çizer.
    ///
    /// ## Bresenham Algoritması
    /// ```text
    /// Hata terimi (err) ile her adımda x veya y ekseni boyunca ilerler.
    /// Çizgi kalınlığı için her piksel, size×size'lık kare ile genişletilir.
    /// ```
    fn draw_line(&self, fb: &mut Framebuffer, start: (i32, i32), end: (i32, i32), color: u32, size: usize) {
        let dx = (end.0 - start.0).abs();
        let dy = (end.1 - start.1).abs();
        let sx = if start.0 < end.0 { 1 } else { -1 };
        let sy = if start.1 < end.1 { 1 } else { -1 };
        let mut err = dx - dy;

        let mut x = start.0;
        let mut y = start.1;

        loop {
            // Kalınlık için her pikseli size×size kare olarak çiz
            for py in 0..size {
                for px in 0..size {
                    let px = x + px as i32 - size as i32 / 2;
                    let py = y + py as i32 - size as i32 / 2;
                    if px >= 0 && py >= 0 && (px as usize) < fb.width && (py as usize) < fb.height {
                        fb.plot_pixel(px as usize, py as usize, color);
                    }
                }
            }

            if x == end.0 && y == end.1 {
                break;
            }

            let e2 = 2 * err;
            if e2 > -dy {
                err -= dy;
                x += sx;
            }
            if e2 < dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// İki rengi alfa değerine göre doğrusal karıştırır.
    /// `alpha = 0.0` → tamamen arka plan, `alpha = 1.0` → tamamen ön plan
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

    /// Araç çubuğuna tıklamayı işle. Tıklanan düğmeye göre olay döndürür.
    pub fn on_toolbar_click(&mut self, mx: i32, my: i32) -> ScreenshotEvent {
        let (x, y) = self.toolbar_pos;

        // Araç çubuğu sınırları dışındaysa yoksay
        if mx < x as i32 || mx > (x + 300) as i32 || my < y as i32 || my > (y + TOOLBAR_HEIGHT) as i32 {
            return ScreenshotEvent::None;
        }

        let mut btn_x = x + 8;

        // Kaydet düğmesi
        if mx >= btn_x as i32 && mx < (btn_x + 60) as i32 {
            return ScreenshotEvent::SaveRequested;
        }
        btn_x += 68;

        // Kopyala düğmesi
        if mx >= btn_x as i32 && mx < (btn_x + 60) as i32 {
            return ScreenshotEvent::CopyRequested;
        }
        btn_x += 68;

        // Açıklama araçları (toggle: aynı araca tekrar tıklamak kapatır)
        let tools = [AnnotationMode::Pen, AnnotationMode::Rectangle, AnnotationMode::Circle, AnnotationMode::Arrow];
        for mode in tools {
            if mx >= btn_x as i32 && mx < (btn_x + 24) as i32 {
                self.annotation_mode = if self.annotation_mode == mode { AnnotationMode::None } else { mode };
                return ScreenshotEvent::None;
            }
            btn_x += 28;
        }

        // İptal düğmesi
        if mx >= (x + 240) as i32 && mx < (x + 292) as i32 {
            self.cancel();
            return ScreenshotEvent::Cancelled;
        }

        ScreenshotEvent::None
    }

    /// Görüntüyü dosyaya kaydet. Dosya sayacını artırır ve yolu döndürür.
    pub fn save(&mut self) -> String {
        self.file_counter += 1;
        let filename = format!("screenshot_{:04}.png", self.file_counter);
        let path = format!("{}/{}", self.save_path, filename);

        // Gerçek implementasyonda dosya sistemine yazılır
        // Şimdilik yalnızca yolu döndürür

        self.show_toolbar = false;
        self.captured_buffer.clear();

        path
    }

    /// Görüntüyü panoya kopyala
    pub fn copy_to_clipboard(&mut self) {
        // Gerçek implementasyonda pano arabelleğine yazılır
        self.show_toolbar = false;
    }

    /// Ekran boyutu değiştiğinde çağrılır (pencere yeniden boyutlandırma)
    pub fn resize(&mut self, width: usize, height: usize) {
        self.screen_width = width;
        self.screen_height = height;
    }
}

/// Ekran görüntüsü aracından yayılan olaylar
#[derive(Clone, Debug)]
pub enum ScreenshotEvent {
    /// Olay yok
    None,
    /// Görüntü başarıyla yakalandı
    Captured,
    /// Geri sayım devam ediyor (kalan saniye)
    Countdown(u32),
    /// Pencere seçildi (Window modu)
    WindowSelected,
    /// Kaydetme talebi yapıldı
    SaveRequested,
    /// Panoya kopyalama talebi
    CopyRequested,
    /// Dosyaya kaydedildi (yol ile birlikte)
    Saved(String),
    /// İptal edildi
    Cancelled,
}

// ============================================================================
// GLOBAL EKRAN GÖRÜNTÜSÜ ARACI (Lazy Static Singleton)
// ============================================================================

lazy_static::lazy_static! {
    /// Spin Mutex ile korunan global ekran görüntüsü aracı.
    /// `no_std` ortamında std::sync::Mutex yerine `spin::Mutex` kullanılır.
    static ref SCREENSHOT: Mutex<ScreenshotTool> = Mutex::new(ScreenshotTool::new(1920, 1080));
}

/// Ekran görüntüsü aracını başlat (ekran boyutunu ayarla)
pub fn init(width: usize, height: usize) {
    let mut screenshot = SCREENSHOT.lock();
    screenshot.resize(width, height);
    crate::serial_println!("[GUI] Screenshot tool initialized");
}

/// Global ekran görüntüsü aracına erişim sağla
pub fn get_screenshot() -> &'static Mutex<ScreenshotTool> {
    &SCREENSHOT
}
