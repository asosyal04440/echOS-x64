//! # Sürükle ve Bırak Desteği
//!
//! GUI öğeleri için sistem genelinde sürükle ve bırak işlevselliği.
//! Dosya sürükleme, metin seçimi ve widget sıralamayı destekler.
//!
//! ## Mimari
//! - `DragData`: Sürüklenen veri türleri (Dosyalar / Metin / Resim / Widget / Özel)
//! - `DropTarget`: Bırakma hedefi alanı; kabul edilen veri türleri ve vurgulama
//! - `DragOperation`: Mevcut sürükleme durumu; önizleme, rozet, efekt
//! - `DragDropManager`: Global yönetici; hedef kaydı, animasyon, bahar yükleme
//!
//! ## Bahar Yükleme (Spring Loading)
//! Klasörün üzerine belirli süre tutulduğunda otomatik açılır.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};

// ============================================================================
// SÜRÜKLEME VERİ TÜRLERİ
// ============================================================================

/// Sürüklenebilecek veri türleri
#[derive(Clone, Debug)]
pub enum DragData {
    /// Veri yok
    None,
    /// Dosya yolları
    Files(Vec<String>),
    /// Metin dizisi
    Text(String),
    /// Resim verisi (ham pikseller)
    Image { width: usize, height: usize, data: Vec<u32> },
    /// Widget referansı
    Widget { window_id: u32, widget_id: u32 },
    /// MIME türüyle özel veri
    Custom { mime_type: String, data: Vec<u8> },
}

impl DragData {
    pub fn is_empty(&self) -> bool {
        match self {
            DragData::None => true,
            DragData::Files(f) => f.is_empty(),
            DragData::Text(t) => t.is_empty(),
            DragData::Image { data, .. } => data.is_empty(),
            DragData::Custom { data, .. } => data.is_empty(),
            _ => false,
        }
    }

    pub fn get_text(&self) -> Option<&str> {
        match self {
            DragData::Text(t) => Some(t),
            _ => None,
        }
    }

    pub fn get_files(&self) -> Option<&Vec<String>> {
        match self {
            DragData::Files(f) => Some(f),
            _ => None,
        }
    }

    pub fn description(&self) -> String {
        match self {
            DragData::None => String::from("Nothing"),
            DragData::Files(f) => {
                if f.len() == 1 {
                    format!("File: {}", f[0])
                } else {
                    format!("{} files", f.len())
                }
            }
            DragData::Text(t) => {
                if t.len() > 20 {
                    format!("Text: {}...", &t[..20])
                } else {
                    format!("Text: {}", t)
                }
            }
            DragData::Image { width, height, .. } => {
                format!("Image: {}x{}", width, height)
            }
            DragData::Widget { .. } => String::from("Widget"),
            DragData::Custom { mime_type, .. } => {
                format!("Custom: {}", mime_type)
            }
        }
    }
}

// ============================================================================
// BIRAKMA HEDEFİ
// ============================================================================

/// Bir bırakma hedefi alanı
#[derive(Clone, Debug)]
pub struct DropTarget {
    /// Hedef kimliği
    pub id: u32,
    /// Hedef sınırları
    pub x: i32,
    pub y: i32,
    pub width: usize,
    pub height: usize,
    /// Kabul edilen veri türleri
    pub accepts: Vec<DragDataType>,
    /// Hedef türü
    pub target_type: DropTargetType,
    /// Vurgulanmış mı
    pub highlighted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragDataType {
    Files,
    Text,
    Image,
    Widget,
    Custom,
    All,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropTargetType {
    Window,
    Folder,
    TextArea,
    ImageWell,
    List,
    Trash,
    Custom,
}

impl DropTarget {
    pub fn new(id: u32, x: i32, y: i32, width: usize, height: usize) -> Self {
        DropTarget {
            id,
            x,
            y,
            width,
            height,
            accepts: vec![DragDataType::All],
            target_type: DropTargetType::Custom,
            highlighted: false,
        }
    }

    pub fn accepts_type(&self, data_type: DragDataType) -> bool {
        self.accepts.contains(&DragDataType::All) || self.accepts.contains(&data_type)
    }

    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.width as i32
            && py >= self.y && py < self.y + self.height as i32
    }

    pub fn highlight(&mut self, highlight: bool) {
        self.highlighted = highlight;
    }
}

// ============================================================================
// SÜRÜKLEME İŞLEMİ
// ============================================================================

/// Mevcut sürükleme işlemi durumu
#[derive(Clone, Debug)]
pub struct DragOperation {
    /// Sürükleme devam ediyor mu
    pub active: bool,
    /// Sürüklenen veri
    pub data: DragData,
    /// Kaynak pencere/widget
    pub source: Option<DragSource>,
    /// Mevcut fare konumu
    pub position: (i32, i32),
    /// Sürükleme başlangıcından ofset
    pub offset: (i32, i32),
    /// Sürükleme resmi ofseti
    pub image_offset: (i32, i32),
    /// Mevcut bırakma hedefi
    pub target: Option<u32>,
    /// Sürükleme efekti
    pub effect: DragEffect,
    /// Sürükleme başladı mı
    pub started: bool,
    /// Sürükleme önizleme resmi
    pub preview: Option<DragPreview>,
}

#[derive(Clone, Debug)]
pub struct DragSource {
    /// Kaynak pencere kimliği
    pub window_id: u32,
    /// Kaynak widget kimliği
    pub widget_id: Option<u32>,
    /// Kaynak sınırları
    pub bounds: (i32, i32, usize, usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragEffect {
    None,
    Copy,
    Move,
    Link,
}

#[derive(Clone, Debug)]
pub struct DragPreview {
    /// Önizleme resmi genişliği
    pub width: usize,
    /// Önizleme resmi yüksekliği
    pub height: usize,
    /// Önizleme piksel verisi
    pub data: Vec<u32>,
    /// Rozet göster
    pub badge: Option<DragBadge>,
    /// Opaklık
    pub opacity: f32,
}

#[derive(Clone, Debug)]
pub struct DragBadge {
    /// Rozet simgesi
    pub icon: BadgeIcon,
    /// Rozet konumu
    pub offset: (i32, i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadgeIcon {
    Copy,
    Move,
    Link,
    Trash,
    Plus,
}

impl DragOperation {
    pub fn new() -> Self {
        DragOperation {
            active: false,
            data: DragData::None,
            source: None,
            position: (0, 0),
            offset: (0, 0),
            image_offset: (0, 0),
            target: None,
            effect: DragEffect::None,
            started: false,
            preview: None,
        }
    }

    /// Sürükleme işlemini başlat
    pub fn start(&mut self, data: DragData, source: DragSource, start_pos: (i32, i32)) {
        self.active = true;
        self.data = data;
        self.source = Some(source);
        self.position = start_pos;
        self.offset = (0, 0);
        self.target = None;
        self.effect = DragEffect::Copy;
        self.started = true;

        // Önizleme oluştur
        self.create_preview();
    }

    /// Sürükleme konumunu güncelle
    pub fn update_position(&mut self, x: i32, y: i32) {
        if !self.active {
            return;
        }

        self.offset = (x - self.position.0, y - self.position.1);
        self.position = (x, y);

        // Sürükleme mesafesi eşiğine ulaşıldı mı kontrol et
        if !self.started {
            let dist = (self.offset.0 * self.offset.0 + self.offset.1 * self.offset.1) as f32;
            if dist > 16.0 { // 4 piksel eşiği
                self.started = true;
            }
        }
    }

    /// Sürükleme işlemini bitir
    pub fn end(&mut self) -> (DragData, Option<DragSource>, Option<u32>, DragEffect) {
        let result = (
            self.data.clone(),
            self.source.clone(),
            self.target,
            self.effect,
        );

        self.active = false;
        self.data = DragData::None;
        self.source = None;
        self.target = None;
        self.effect = DragEffect::None;
        self.started = false;
        self.preview = None;

        result
    }

    /// Sürükleme işlemini iptal et
    pub fn cancel(&mut self) {
        self.active = false;
        self.data = DragData::None;
        self.source = None;
        self.target = None;
        self.effect = DragEffect::None;
        self.started = false;
        self.preview = None;
    }

    /// Bırakma hedefini ayarla
    pub fn set_target(&mut self, target_id: Option<u32>, effect: DragEffect) {
        self.target = target_id;
        self.effect = effect;

        // Efekte göre rozeti güncelle
        if let Some(ref mut preview) = self.preview {
            preview.badge = match effect {
                DragEffect::Copy => Some(DragBadge { icon: BadgeIcon::Copy, offset: (8, 8) }),
                DragEffect::Move => Some(DragBadge { icon: BadgeIcon::Move, offset: (8, 8) }),
                DragEffect::Link => Some(DragBadge { icon: BadgeIcon::Link, offset: (8, 8) }),
                DragEffect::None => None,
            };
        }
    }

    fn create_preview(&mut self) {
        let preview = match &self.data {
            DragData::Files(files) => {
                if files.len() == 1 {
                    // Tek dosya - dosya simgesi göster
                    DragPreview {
                        width: 64,
                        height: 64,
                        data: Self::create_file_preview(&files[0]),
                        badge: Some(DragBadge { icon: BadgeIcon::Copy, offset: (8, 8) }),
                        opacity: 0.8,
                    }
                } else {
                    // Birden fazla dosya - sayı göster
                    DragPreview {
                        width: 80,
                        height: 80,
                        data: Self::create_multi_file_preview(files.len()),
                        badge: Some(DragBadge { icon: BadgeIcon::Copy, offset: (8, 8) }),
                        opacity: 0.8,
                    }
                }
            }
            DragData::Text(text) => {
                DragPreview {
                    width: (text.len().min(20) * 8 + 16).max(60),
                    height: 24,
                    data: Self::create_text_preview(text),
                    badge: None,
                    opacity: 0.8,
                }
            }
            DragData::Image { width, height, data } => {
                DragPreview {
                    width: (*width).min(128),
                    height: (*height).min(128),
                    data: data.clone(),
                    badge: Some(DragBadge { icon: BadgeIcon::Copy, offset: (8, 8) }),
                    opacity: 0.8,
                }
            }
            _ => {
                DragPreview {
                    width: 48,
                    height: 48,
                    data: vec![0x40808080; 48 * 48],
                    badge: None,
                    opacity: 0.6,
                }
            }
        };

        self.preview = Some(preview);
    }

    fn create_file_preview(_path: &str) -> Vec<u32> {
        // Basit dosya simgesi önizlemesi oluştur
        let mut data = vec![0x00000000; 64 * 64];

        // Dosya simgesi çerçevesi çiz
        for y in 8..56 {
            for x in 12..52 {
                let is_corner = (x < 16 && y < 16) || (x > 44 && y < 16);
                if !is_corner {
                    data[y * 64 + x] = 0xE0FFFFFF;
                }
            }
        }

        data
    }

    fn create_multi_file_preview(count: usize) -> Vec<u32> {
        let mut data = vec![0x00000000; 80 * 80];

        // Üst üste dizilmiş dosya simgeleri çiz
        for offset in 0..3 {
            let x_off = offset * 4;
            let y_off = offset * 4;

            for y in 8 + y_off..56 + y_off {
                for x in 12 + x_off..52 + x_off {
                    if y < 80 && x < 80 {
                        data[y * 80 + x] = 0xE0FFFFFF;
                    }
                }
            }
        }

        // Sayı rozeti çiz
        let count_str = format!("{}", count);
        let badge_x = 56;
        let badge_y = 56;

        for y in badge_y..badge_y + 20 {
            for x in badge_x..badge_x + 20 {
                data[y * 80 + x] = 0xFF007AFF;
            }
        }

        data
    }

    fn create_text_preview(text: &str) -> Vec<u32> {
        let width = (text.len().min(20) * 8 + 16).max(60);
        let mut data = vec![0xE0FFFFFF; width * 24];

        // Gerçek metin çizilecek - şimdilik sadece beyaz arka plan
        data
    }
}

// ============================================================================
// SÜRÜKLE BIRAK YÖNETİCİSİ
// ============================================================================

/// Global sürükle ve bırak yöneticisi
pub struct DragDropManager {
    /// Mevcut sürükleme işlemi
    pub operation: DragOperation,
    /// Kayıtlı bırakma hedefleri
    pub targets: Vec<DropTarget>,
    /// Sürükleme eşiği (piksel)
    pub drag_threshold: i32,
    /// Bahar yükleme gecikmesi (klasörler için)
    pub spring_delay: f32,
    /// Bahar yükleme zamanlayıcısı
    pub spring_timer: f32,
    /// Bahar yükleme hedefi
    pub spring_target: Option<u32>,
    /// Otomatik kaydırma etkin
    pub auto_scroll: bool,
    /// Otomatik kaydırma hızı
    pub scroll_speed: i32,
    /// Sonraki hedef kimliği
    pub next_target_id: u32,
}

impl DragDropManager {
    pub fn new() -> Self {
        DragDropManager {
            operation: DragOperation::new(),
            targets: Vec::new(),
            drag_threshold: 4,
            spring_delay: 0.5,
            spring_timer: 0.0,
            spring_target: None,
            auto_scroll: true,
            scroll_speed: 8,
            next_target_id: 1,
        }
    }

    /// Bırakma hedefini kaydet
    pub fn register_target(&mut self, x: i32, y: i32, width: usize, height: usize, accepts: Vec<DragDataType>) -> u32 {
        let id = self.next_target_id;
        self.next_target_id += 1;

        let mut target = DropTarget::new(id, x, y, width, height);
        target.accepts = accepts;

        self.targets.push(target);
        id
    }

    /// Bırakma hedefinin kaydını sil
    pub fn unregister_target(&mut self, id: u32) {
        self.targets.retain(|t| t.id != id);
    }

    /// Hedef konumunu güncelle
    pub fn update_target(&mut self, id: u32, x: i32, y: i32, width: usize, height: usize) {
        if let Some(target) = self.targets.iter_mut().find(|t| t.id == id) {
            target.x = x;
            target.y = y;
            target.width = width;
            target.height = height;
        }
    }

    /// Sürüklemeyi başlat
    pub fn start_drag(&mut self, data: DragData, source: DragSource, start_pos: (i32, i32)) {
        self.operation.start(data, source, start_pos);
    }

    /// Sürükleme konumunu güncelle
    pub fn update_drag(&mut self, x: i32, y: i32) -> Option<DropEvent> {
        if !self.operation.active {
            return None;
        }

        self.operation.update_position(x, y);

        // İmleç altındaki bırakma hedefini bul
        let data_type = self.get_data_type();
        let mut found_target_id: Option<u32> = None;
        for target in &self.targets {
            if target.contains(x, y) && target.accepts_type(data_type) {
                found_target_id = Some(target.id);
                break;
            }
        }

        // Vurgulamaları güncelle
        for target in &mut self.targets {
            let should_highlight = found_target_id == Some(target.id);
            target.highlight(should_highlight);
        }

        // Hedefi ve efekti ayarla
        if let Some(target_id) = found_target_id {
            let effect = if let Some(target) = self.targets.iter().find(|t| t.id == target_id) {
                self.determine_effect(target)
            } else {
                DragEffect::None
            };
            self.operation.set_target(Some(target_id), effect);

            // Klasörler için bahar yükleme
            if let Some(target) = self.targets.iter().find(|t| t.id == target_id) {
                if target.target_type == DropTargetType::Folder {
                    if self.spring_target != Some(target.id) {
                        self.spring_target = Some(target.id);
                        self.spring_timer = 0.0;
                    }
                } else {
                    self.spring_target = None;
                    self.spring_timer = 0.0;
                }
            } else {
                self.spring_target = None;
                self.spring_timer = 0.0;
            }

            Some(DropEvent::TargetChanged(target_id, effect))
        } else {
            self.operation.set_target(None, DragEffect::None);
            self.spring_target = None;
            self.spring_timer = 0.0;
            Some(DropEvent::TargetChanged(0, DragEffect::None))
        }
    }

    /// Sürüklemeyi bitir
    pub fn end_drag(&mut self) -> DropEvent {
        let (data, source, target_id, effect) = self.operation.end();

        // Vurgulamaları temizle
        for target in &mut self.targets {
            target.highlight(false);
        }

        self.spring_target = None;
        self.spring_timer = 0.0;

        if let Some(target_id) = target_id {
            DropEvent::Dropped { data, source, target_id, effect }
        } else {
            DropEvent::Cancelled
        }
    }

    /// Sürüklemeyi iptal et
    pub fn cancel_drag(&mut self) {
        self.operation.cancel();

        for target in &mut self.targets {
            target.highlight(false);
        }

        self.spring_target = None;
        self.spring_timer = 0.0;
    }

    /// Bahar yüklemeyi güncelle
    pub fn update(&mut self, dt: f32) -> Option<DropEvent> {
        if self.spring_target.is_some() {
            self.spring_timer += dt;

            if self.spring_timer >= self.spring_delay {
                let target_id = self.spring_target.unwrap();
                self.spring_target = None;
                self.spring_timer = 0.0;
                return Some(DropEvent::SpringLoaded(target_id));
            }
        }

        None
    }

    /// Sürükleme katmanını çiz
    pub fn draw(&self, fb: &mut Framebuffer) {
        // Vurgulanan hedefleri çiz
        for target in &self.targets {
            if target.highlighted {
                self.draw_drop_highlight(fb, target);
            }
        }

        // Sürükleme önizlemesini çiz
        if self.operation.active && self.operation.started {
            self.draw_drag_preview(fb);
        }
    }

    fn draw_drop_highlight(&self, fb: &mut Framebuffer, target: &DropTarget) {
        let color = match self.operation.effect {
            DragEffect::Copy => Theme::ACCENT_PRIMARY.to_u32(),
            DragEffect::Move => Theme::ACCENT_WARNING.to_u32(),
            DragEffect::Link => Theme::ACCENT_SUCCESS.to_u32(),
            DragEffect::None => Theme::ERROR.to_u32(),
        };

        // Kenarlık çiz
        for i in 0..3 {
            let x = (target.x + i) as usize;
            let y = (target.y + i) as usize;
            let w = target.width - (i * 2) as usize;
            let h = target.height - (i * 2) as usize;

            fb.draw_rect_outline(x, y, w, h, color);
        }

        // Listeler için ekleme göstergesi çiz
        if target.target_type == DropTargetType::List {
            let insert_y = self.operation.position.1.min(target.y + target.height as i32 - 2).max(target.y);
            let insert_x = target.x;
            let insert_w = target.width;

            // Çizgi çiz
            fb.draw_rect(insert_x as usize, insert_y as usize, insert_w, 2, color);

            // Oklar çiz
            fb.draw_rect(insert_x as usize, insert_y as usize - 4, 8, 10, color);
            fb.draw_rect((insert_x + insert_w as i32 - 8) as usize, insert_y as usize - 4, 8, 10, color);
        }
    }

    fn draw_drag_preview(&self, fb: &mut Framebuffer) {
        if let Some(ref preview) = self.operation.preview {
            let x = (self.operation.position.0 + self.operation.image_offset.0) as usize;
            let y = (self.operation.position.1 + self.operation.image_offset.1) as usize;

            // Önizleme resmini çiz
            for py in 0..preview.height {
                for px in 0..preview.width {
                    let screen_x = x + px;
                    let screen_y = y + py;

                    if screen_x < fb.width && screen_y < fb.height {
                        let color = preview.data[py * preview.width + px];
                        if color != 0 {
                            fb.plot_pixel(screen_x, screen_y, color);
                        }
                    }
                }
            }

            // Rozeti çiz
            if let Some(ref badge) = preview.badge {
                let badge_x = x + badge.offset.0 as usize;
                let badge_y = y + badge.offset.1 as usize;

                // Rozet arka planı
                fb.draw_rect(badge_x, badge_y, 20, 20, 0xFF007AFF);

                // Rozet simgesi
                let icon = match badge.icon {
                    BadgeIcon::Copy => "+",
                    BadgeIcon::Move => "↗",
                    BadgeIcon::Link => "⌘",
                    BadgeIcon::Trash => "⌫",
                    BadgeIcon::Plus => "+",
                };
                fb.draw_string(badge_x + 4, badge_y + 2, icon, 0xFFFFFFFF);
            }
        }
    }

    fn get_data_type(&self) -> DragDataType {
        match &self.operation.data {
            DragData::None => DragDataType::All,
            DragData::Files(_) => DragDataType::Files,
            DragData::Text(_) => DragDataType::Text,
            DragData::Image { .. } => DragDataType::Image,
            DragData::Widget { .. } => DragDataType::Widget,
            DragData::Custom { .. } => DragDataType::Custom,
        }
    }

    fn determine_effect(&self, target: &DropTarget) -> DragEffect {
        // Varsayılan olarak kopyala; değiştirici tuşlarla değiştirilebilir
        // Option = kopyala, Command = taşı, Control+Option = bağla
        DragEffect::Copy
    }

    /// Sürükleme devam ediyor mu
    pub fn is_dragging(&self) -> bool {
        self.operation.active && self.operation.started
    }

    /// Mevcut sürükleme verisini al
    pub fn get_drag_data(&self) -> Option<&DragData> {
        if self.operation.active {
            Some(&self.operation.data)
        } else {
            None
        }
    }
}

/// Bırakma olayları
#[derive(Clone, Debug)]
pub enum DropEvent {
    None,
    TargetChanged(u32, DragEffect),
    Dropped {
        data: DragData,
        source: Option<DragSource>,
        target_id: u32,
        effect: DragEffect,
    },
    Cancelled,
    SpringLoaded(u32),
}

// ============================================================================
// GLOBAL SÜRÜKLE BIRAK YÖNETİCİSİ
// ============================================================================

lazy_static::lazy_static! {
    static ref DRAG_DROP: Mutex<DragDropManager> = Mutex::new(DragDropManager::new());
}

/// Sürükle ve bırakı başlat
pub fn init() {
    crate::serial_println!("[GUI] Sürükle ve bırak başlatıldı");
}

/// Sürükle ve bırak yöneticisini al
pub fn get_drag_drop() -> &'static Mutex<DragDropManager> {
    &DRAG_DROP
}
