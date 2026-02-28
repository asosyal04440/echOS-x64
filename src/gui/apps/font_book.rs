//! # Font Book Application
//!
//! Font management application for viewing, installing, and organizing fonts
//! Supports preview, categorization, and font metadata
//!
//! Bu modül, macOS Font Book'a benzer bir yazı tipi yönetim uygulamasını uygular.
//! İşletim sistemleri, metin oluşturmak için harfleri, rakamları ve simgeleri
//! içeren "font" (yazı tipi) dosyalarını kullanır.
//!
//! Temel font kavramları:
//! - **Aile (Family)**: "Helvetica" gibi tüm stilleri kapsayan üst kategori.
//! - **Stil (Style)**: Regular, Bold, Italic gibi bir ailenin varyantları.
//! - **Ağırlık (Weight)**: 100 (İnce) ile 900 (Siyah) arasında kalınlık.
//! - **EM birimi**: Font metriklerinin ölçüldüğü temel birim (genellikle 1000).
//! - **Glif (Glyph)**: Bir karakterin grafik temsili.

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
// FONT BOOK CONSTANTS
// ============================================================================
// Font Book arayüzünün piksel ölçüleri.
// MacOS Font Book'a yakın bir düzen oluşturmak için kullanılır.

/// Araç çubuğunun yüksekliği (görünüm değiştirme ve arama butonları)
pub const TOOLBAR_HEIGHT: usize = 44;

/// Kategori ve koleksiyon listesini gösteren sol kenar çubuğunun genişliği
pub const SIDEBAR_WIDTH: usize = 200;

/// Seçili fontu büyük puntoda önizleyen alanın yüksekliği
pub const PREVIEW_HEIGHT: usize = 200;

/// Font listesinde her satırın yüksekliği
pub const ROW_HEIGHT: usize = 32;

// ============================================================================
// FONT INFO
// ============================================================================
// Bir font dosyasının tüm üst veri (metadata) ve tanımlayıcı bilgilerini
// tutan yapı. `FontFormat` ve `FontType` enum'ları font kategorisini belirler.
// `Vec<FontCategory>` alanı ile bir font birden fazla kategoriye ait olabilir.

/// Bir yazı tipi dosyasına ait tüm bilgiler
#[derive(Clone, Debug)]
pub struct FontInfo {
    /// Sistemde fontu tanımlayan benzersiz sayısal kimlik
    pub id: u32,
    /// Font ailesi adı (örn. "Helvetica", "Times New Roman")
    pub family: String,
    /// Stil adı (örn. "Regular", "Bold Italic")
    pub style: String,
    /// Tam ad: aile + stil birleşimi (örn. "Helvetica Bold")
    pub full_name: String,
    /// PostScript adı (eğik çizgi olmadan, örn. "Helvetica-Bold")
    pub postscript_name: String,
    /// Dosya formatı (TrueType, OpenType vb.)
    pub format: FontFormat,
    /// Görsel sınıflandırma (Serif, Sans-Serif, Monospace vb.)
    pub font_type: FontType,
    /// Font dosyasının disk üzerindeki yolu
    pub path: String,
    /// Font dosyasının boyutu (bayt cinsinden)
    pub file_size: u64,
    /// Sistem fontlarına yüklenmiş mi?
    pub installed: bool,
    /// Uygulama tarafından etkin mi? (devre dışı bırakılmış fontlar kullanılamaz)
    pub enabled: bool,
    /// Kullanıcı tarafından favorilere eklenmiş mi?
    pub favorite: bool,
    /// Fontu listeleyen kategoriler
    pub categories: Vec<FontCategory>,
    /// Ek teknik üst veriler
    pub metadata: FontMetadata,
    /// Önizlemede gösterilecek örnek metin
    pub sample: String,
}

// Desteklenen yazı tipi formatları.
// TrueType (.ttf) ve OpenType (.otf) en yaygın modern formatlardır.
// WOFF/WOFF2 web için sıkıştırılmış versiyonlardır.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontFormat {
    TrueType, // Apple/Microsoft geliştirdi; .ttf uzantılı
    OpenType, // TrueType üzerine inşa edilmiş gelişmiş format; .otf
    Type1,    // Adobe'nin eski PostScript font formatı
    WOFF,     // Web Open Font Format (sıkıştırılmış)
    WOFF2,    // Daha iyi sıkıştırma ile güncellenmiş WOFF
    Bitmap,   // Sabit boyutlu piksel tabanlı font
    Unknown,  // Tanımlanamayan format
}

// Yazı tipi görsel sınıflandırması.
// Tipografide fontlar karakterlerinin şekline göre kategorilere ayrılır.
// Serif vs. Sans-Serif: tırnak/süsleme olan vs. olmayan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontType {
    Serif,       // Harf uçlarında ince çizgiler (tırnaklar) olan fontlar (örn. Times)
    SansSerif,   // Tırnaksız, temiz çizgili fontlar (örn. Helvetica)
    Display,     // Büyük başlıklar için tasarlanmış dekoratif fontlar
    Handwriting, // El yazısı stilinde olanlar
    Monospace,   // Her karakterin aynı genişlikte olduğu fontlar (kod için ideal)
    Symbol,      // Harf yerine özel simge ve şekilleri içeren fontlar
    Unknown,     // Sınıflandırılamayan
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontCategory {
    All,
    Favorites,
    Serif,
    SansSerif,
    Display,
    Handwriting,
    Monospace,
    Symbol,
    Installed,
    User,
    System,
}

impl FontInfo {
    pub fn new(id: u32, family: &str, style: &str) -> Self {
        FontInfo {
            id,
            family: String::from(family),
            style: String::from(style),
            full_name: format!("{} {}", family, style),
            postscript_name: format!("{}-{}", family.replace(' ', ""), style),
            format: FontFormat::TrueType,
            font_type: FontType::SansSerif,
            path: String::new(),
            file_size: 0,
            installed: true,
            enabled: true,
            favorite: false,
            categories: vec![FontCategory::SansSerif],
            metadata: FontMetadata::default(),
            sample: String::from("The quick brown fox jumps over the lazy dog"),
        }
    }
    
    pub fn display_name(&self) -> String {
        if self.style == "Regular" {
            self.family.clone()
        } else {
            format!("{} {}", self.family, self.style)
        }
    }
    
    pub fn format_size(&self) -> String {
        if self.file_size < 1024 {
            format!("{} B", self.file_size)
        } else if self.file_size < 1024 * 1024 {
            format!("{:.1} KB", self.file_size as f64 / 1024.0)
        } else {
            format!("{:.1} MB", self.file_size as f64 / (1024.0 * 1024.0))
        }
    }
    
    pub fn icon(&self) -> &'static str {
        match self.font_type {
            FontType::Serif => "𝐓",
            FontType::SansSerif => "𝐓",
            FontType::Display => "𝐀",
            FontType::Handwriting => "𝒯",
            FontType::Monospace => "𝚃",
            FontType::Symbol => "★",
            FontType::Unknown => "T",
        }
    }
}

// ============================================================================
// FONT METADATA
// ============================================================================
// Font dosyasının içinde gömülü teknik ve telif hakkı bilgilerini tutan yapı.
// `ascender` ve `descender`: Temel hattın (baseline) üstünde ve altında kalan
// maksimum glif yüksekliği (EM birimi cinsinden).
// `units_per_em`: Koordinat sisteminin temel ölçeği; genellikle 1000 veya 2048.

/// Bir fonttaki teknik metrikler ve telif hakkı bilgileri
#[derive(Clone, Debug)]
pub struct FontMetadata {
    /// Fontu tasarlayan kişi veya kuruluş
    pub designer: String,
    /// Fontu yayımlayan kuruluş (döküm evi)
    pub foundry: String,
    /// Font sürümü (örn. "2.001")
    pub version: String,
    /// Telif hakkı metni
    pub copyright: String,
    /// Lisans türü (örn. "OFL 1.1")
    pub license: String,
    /// Lisans URL'i
    pub license_url: String,
    /// Fonun kısa açıklaması
    pub description: String,
    /// Önizleme için örnek metin
    pub sample_text: String,
    /// Kalınlık değeri: 100 (Thin) — 900 (Black)
    pub weight: u16,
    /// Genişlik değeri: 1 (Ultra Condensed) — 9 (Ultra Expanded)
    pub width: u8,
    /// İtalik mi?
    pub italic: bool,
    /// Sabit genişlikli (monospace) mi?
    pub fixed_pitch: bool,
    /// EM koordinat birimi başına birim sayısı
    pub units_per_em: u16,
    /// Temel hattın üstündeki maksimum yükseklik (EM birimiyle)
    pub ascender: i16,
    /// Temel hattın altındaki maksimum derinlik (negatif EM birimiyle)
    pub descender: i16,
    /// Satır boşluğu (line gap) değeri
    pub line_gap: i16,
    /// Büyük harf yüksekliği (cap height)
    pub cap_height: i16,
    /// Küçük "x" harfinin yüksekliği
    pub x_height: i16,
    /// Desteklenen yazı sistemleri (örn. "Latin", "Cyrillic")
    pub scripts: Vec<String>,
    /// Desteklenen diller
    pub languages: Vec<String>,
    /// Fontun içerdiği toplam glif sayısı
    pub glyph_count: u32,
}

impl FontMetadata {
    pub fn default() -> Self {
        FontMetadata {
            designer: String::new(),
            foundry: String::new(),
            version: String::from("1.0"),
            copyright: String::new(),
            license: String::new(),
            license_url: String::new(),
            description: String::new(),
            sample_text: String::from("The quick brown fox jumps over the lazy dog"),
            weight: 400,
            width: 5,
            italic: false,
            fixed_pitch: false,
            units_per_em: 1000,
            ascender: 800,
            descender: -200,
            line_gap: 0,
            cap_height: 700,
            x_height: 500,
            scripts: vec![String::from("Latin")],
            languages: vec![String::from("English")],
            glyph_count: 256,
        }
    }
    
    pub fn weight_name(&self) -> &'static str {
        match self.weight {
            100 => "Thin",
            200 => "Extra Light",
            300 => "Light",
            400 => "Regular",
            500 => "Medium",
            600 => "Semi Bold",
            700 => "Bold",
            800 => "Extra Bold",
            900 => "Black",
            _ => "Unknown",
        }
    }
    
    pub fn width_name(&self) -> &'static str {
        match self.width {
            1 => "Ultra Condensed",
            2 => "Extra Condensed",
            3 => "Condensed",
            4 => "Semi Condensed",
            5 => "Normal",
            6 => "Semi Expanded",
            7 => "Expanded",
            8 => "Extra Expanded",
            9 => "Ultra Expanded",
            _ => "Unknown",
        }
    }
}

// ============================================================================
// FONT COLLECTION
// ============================================================================
// Kullanıcının özel olarak oluşturduğu, el ile veya akıllı kurallarla
// doldurulabilen font gruplamalarıdır.
// `smart: true` olan koleksiyonlar, belirlenen kurallara uyan tüm fontları
// otomatik olarak içerir (iTunes'daki Akıllı Listeler gibi).

/// Bir font koleksiyonu (manuel veya akıllı kural tabanlı)
#[derive(Clone, Debug)]
pub struct FontCollection {
    /// Koleksiyonun benzersiz kimliği
    pub id: u32,
    /// Koleksiyon adı (örn. "Sık Kullanılanlar", "Sunum Fontları")
    pub name: String,
    /// Bu koleksiyona dahil font ID'leri
    pub fonts: Vec<u32>,
    /// Akıllı koleksiyon mu? (kural tabanlı otomatik filtreleme)
    pub smart: bool,
    /// Akıllı koleksiyonun filtreleme kuralları
    pub rules: Vec<CollectionRule>,
}

#[derive(Clone, Debug)]
pub struct CollectionRule {
    pub field: CollectionField,
    pub condition: CollectionCondition,
    pub value: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectionField {
    Family,
    Style,
    Type,
    Designer,
    Foundry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectionCondition {
    Contains,
    Equals,
    StartsWith,
    EndsWith,
}

impl FontCollection {
    pub fn new(id: u32, name: &str) -> Self {
        FontCollection {
            id,
            name: String::from(name),
            fonts: Vec::new(),
            smart: false,
            rules: Vec::new(),
        }
    }
    
    pub fn smart(id: u32, name: &str, rules: Vec<CollectionRule>) -> Self {
        FontCollection {
            id,
            name: String::from(name),
            fonts: Vec::new(),
            smart: true,
            rules,
        }
    }
    
    pub fn add_font(&mut self, font_id: u32) {
        if !self.fonts.contains(&font_id) {
            self.fonts.push(font_id);
        }
    }
    
    pub fn remove_font(&mut self, font_id: u32) {
        self.fonts.retain(|&id| id != font_id);
    }
}

// ============================================================================
// FONT BOOK WINDOW
// ============================================================================
// Font Book ana penceresi: fontları listeler, önizler ve yönetir.
// `get_filtered_fonts()` metodu, kategori filtresi ve arama metnini
// birleştirerek hangi fontların gösterileceğini hesaplar (indeks listesi döner).

/// Font Book uygulama penceresi
pub struct FontBookWindow {
    /// Pencerenin ekrandaki konumu ve boyutu
    pub rect: Rect,
    /// Tüm kayıtlı fontlar
    pub fonts: Vec<FontInfo>,
    /// Kullanıcı tanımlı koleksiyonlar
    pub collections: Vec<FontCollection>,
    /// Şu an aktif olan kategori filtresi
    pub current_category: FontCategory,
    /// Seçili fontu temsil eden indeks (listedeki sırası)
    pub selected_font: Option<usize>,
    /// Seçili koleksiyonun indeksi
    pub selected_collection: Option<usize>,
    /// Arama kutusu içeriği
    pub search_query: String,
    /// Kullanıcı tarafından girilmiş önizleme metni
    pub preview_text: String,
    /// Önizleme punto boyutu
    pub preview_size: usize,
    /// Sağ panelde meta veri detayları gösterilsin mi?
    pub show_metadata: bool,
    /// Font listesinde kaydırma konumu
    pub scroll_offset: usize,
    /// Fare imlecinin üzerinde olduğu font satırının indeksi
    pub hovered_font: Option<usize>,
    /// Fare imlecinin üzerinde olduğu koleksiyon satırının indeksi
    pub hovered_collection: Option<usize>,
    /// Aktif görünüm modu (Liste, Izgara, Önizleme)
    pub view_mode: FontViewMode,
    /// Bir sonraki font ID'si (artımlı)
    pub next_font_id: u32,
    /// Bir sonraki koleksiyon ID'si
    pub next_collection_id: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontViewMode {
    List,
    Grid,
    Preview,
}

impl FontBookWindow {
    pub fn new(rect: Rect) -> Self {
        let mut fontbook = FontBookWindow {
            rect,
            fonts: Vec::new(),
            collections: Vec::new(),
            current_category: FontCategory::All,
            selected_font: None,
            selected_collection: None,
            search_query: String::new(),
            preview_text: String::from("The quick brown fox jumps over the lazy dog"),
            preview_size: 24,
            show_metadata: true,
            scroll_offset: 0,
            hovered_font: None,
            hovered_collection: None,
            view_mode: FontViewMode::List,
            next_font_id: 1,
            next_collection_id: 1,
        };
        
        fontbook.init_fonts();
        fontbook.init_collections();
        
        fontbook
    }
    
    fn init_fonts(&mut self) {
        // System fonts
        let system_fonts = [
            ("System UI", "Regular", FontType::SansSerif),
            ("System UI", "Bold", FontType::SansSerif),
            ("System UI", "Italic", FontType::SansSerif),
            ("Monaco", "Regular", FontType::Monospace),
            ("Monaco", "Bold", FontType::Monospace),
            ("Times", "Regular", FontType::Serif),
            ("Times", "Bold", FontType::Serif),
            ("Times", "Italic", FontType::Serif),
            ("Helvetica", "Regular", FontType::SansSerif),
            ("Helvetica", "Bold", FontType::SansSerif),
            ("Courier", "Regular", FontType::Monospace),
            ("Courier", "Bold", FontType::Monospace),
            ("Palatino", "Regular", FontType::Serif),
            ("Symbol", "Regular", FontType::Symbol),
        ];
        
        for (family, style, font_type) in system_fonts {
            let mut font = FontInfo::new(self.next_font_id, family, style);
            font.font_type = font_type;
            font.installed = true;
            font.file_size = 50000 + (self.next_font_id as u64 * 10000);
            
            // Set categories
            font.categories = match font_type {
                FontType::Serif => vec![FontCategory::Serif, FontCategory::System],
                FontType::SansSerif => vec![FontCategory::SansSerif, FontCategory::System],
                FontType::Monospace => vec![FontCategory::Monospace, FontCategory::System],
                FontType::Symbol => vec![FontCategory::Symbol, FontCategory::System],
                _ => vec![FontCategory::System],
            };
            
            // Set metadata
            font.metadata.weight = match style {
                "Bold" => 700,
                "Italic" => 400,
                _ => 400,
            };
            font.metadata.italic = style == "Italic";
            font.metadata.fixed_pitch = font_type == FontType::Monospace;
            
            self.fonts.push(font);
            self.next_font_id += 1;
        }
        
        // User fonts
        let user_fonts = [
            ("Custom Sans", "Regular", FontType::SansSerif),
            ("Custom Serif", "Regular", FontType::Serif),
            ("Custom Mono", "Regular", FontType::Monospace),
            ("Handwritten", "Regular", FontType::Handwriting),
            ("Display", "Regular", FontType::Display),
        ];
        
        for (family, style, font_type) in user_fonts {
            let mut font = FontInfo::new(self.next_font_id, family, style);
            font.font_type = font_type;
            font.installed = true;
            font.file_size = 30000 + (self.next_font_id as u64 * 5000);
            
            font.categories = match font_type {
                FontType::Serif => vec![FontCategory::Serif, FontCategory::User],
                FontType::SansSerif => vec![FontCategory::SansSerif, FontCategory::User],
                FontType::Monospace => vec![FontCategory::Monospace, FontCategory::User],
                FontType::Handwriting => vec![FontCategory::Handwriting, FontCategory::User],
                FontType::Display => vec![FontCategory::Display, FontCategory::User],
                _ => vec![FontCategory::User],
            };
            
            self.fonts.push(font);
            self.next_font_id += 1;
        }
    }
    
    fn init_collections(&mut self) {
        // Default collections
        self.collections.push(FontCollection::new(self.next_collection_id, "All Fonts"));
        self.next_collection_id += 1;
        
        self.collections.push(FontCollection::new(self.next_collection_id, "Favorites"));
        self.next_collection_id += 1;
        
        self.collections.push(FontCollection::new(self.next_collection_id, "Recently Used"));
        self.next_collection_id += 1;
        
        // Smart collections
        self.collections.push(FontCollection::new(self.next_collection_id, "English"));
        self.next_collection_id += 1;
        
        self.collections.push(FontCollection::new(self.next_collection_id, "Fixed Width"));
        self.next_collection_id += 1;
    }
    
    pub fn select_font(&mut self, index: usize) {
        if index < self.fonts.len() {
            self.selected_font = Some(index);
            self.selected_collection = None;
        }
    }
    
    pub fn select_collection(&mut self, index: usize) {
        if index < self.collections.len() {
            self.selected_collection = Some(index);
            self.selected_font = None;
        }
    }
    
    pub fn set_category(&mut self, category: FontCategory) {
        self.current_category = category;
        self.scroll_offset = 0;
    }
    
    pub fn toggle_favorite(&mut self, font_id: u32) {
        if let Some(font) = self.fonts.iter_mut().find(|f| f.id == font_id) {
            font.favorite = !font.favorite;
        }
    }
    
    pub fn toggle_enabled(&mut self, font_id: u32) {
        if let Some(font) = self.fonts.iter_mut().find(|f| f.id == font_id) {
            font.enabled = !font.enabled;
        }
    }
    
    pub fn get_filtered_fonts(&self) -> Vec<usize> {
        let mut indices = Vec::new();
        
        for (i, font) in self.fonts.iter().enumerate() {
            // Category filter
            let category_match = self.current_category == FontCategory::All
                || font.categories.contains(&self.current_category)
                || (self.current_category == FontCategory::Favorites && font.favorite)
                || (self.current_category == FontCategory::Installed && font.installed);
            
            if !category_match {
                continue;
            }
            
            // Search filter
            if !self.search_query.is_empty() {
                let query = self.search_query.to_lowercase();
                if !font.family.to_lowercase().contains(&query)
                    && !font.style.to_lowercase().contains(&query)
                    && !font.full_name.to_lowercase().contains(&query) {
                    continue;
                }
            }
            
            indices.push(i);
        }
        
        indices
    }
    
    /// Draw Font Book
    pub fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let w = self.rect.width as usize;
        let h = self.rect.height as usize;
        
        // Background
        fb.draw_rect(x, y, w, h, Theme::WINDOW_BG.to_u32());
        fb.draw_rect_outline(x, y, w, h, Theme::BORDER.to_u32());
        
        // Toolbar
        fb.draw_rect(x, y, w, TOOLBAR_HEIGHT, Theme::TOOLBAR_BG.to_u32());
        self.draw_toolbar(fb, x, y, w);
        
        // Sidebar
        let content_y = y + TOOLBAR_HEIGHT;
        fb.draw_rect(x, content_y, SIDEBAR_WIDTH, h - TOOLBAR_HEIGHT, Theme::SIDEBAR_BG.to_u32());
        self.draw_sidebar(fb, x, content_y, h - TOOLBAR_HEIGHT);
        
        // Main content
        let content_x = x + SIDEBAR_WIDTH;
        let content_w = w - SIDEBAR_WIDTH;
        
        // Preview area
        if let Some(&idx) = self.selected_font.as_ref() {
            self.draw_preview(fb, content_x, content_y, content_w, PREVIEW_HEIGHT, idx);
        }
        
        // Font list
        let list_y = content_y + if self.selected_font.is_some() { PREVIEW_HEIGHT + 8 } else { 0 };
        let list_h = h - TOOLBAR_HEIGHT - if self.selected_font.is_some() { PREVIEW_HEIGHT + 8 } else { 0 };
        
        self.draw_font_list(fb, content_x, list_y, content_w, list_h);
    }
    
    fn draw_toolbar(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize) {
        // View mode buttons
        let mut btn_x = x + 8;
        
        let views = [("≡", FontViewMode::List), ("⊞", FontViewMode::Grid), ("👁", FontViewMode::Preview)];
        for (icon, mode) in views {
            let is_active = self.view_mode == mode;
            let bg = if is_active { Theme::ACCENT_PRIMARY.to_u32() } else { Theme::SIDEBAR_BG.to_u32() };
            
            fb.draw_rect(btn_x, y + 8, 28, 28, bg);
            fb.draw_string(btn_x + 6, y + 12, icon, Theme::TEXT_PRIMARY.to_u32());
            btn_x += 32;
        }
        
        // Search field
        let search_x = x + w - 180;
        fb.draw_rect(search_x, y + 8, 160, 28, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(search_x + 8, y + 12, "🔍", Theme::TEXT_SECONDARY.to_u32());
        
        if self.search_query.is_empty() {
            fb.draw_string(search_x + 28, y + 12, "Search fonts", Theme::TEXT_SECONDARY.to_u32());
        } else {
            fb.draw_string(search_x + 28, y + 12, &self.search_query, Theme::TEXT_PRIMARY.to_u32());
        }
    }
    
    fn draw_sidebar(&self, fb: &mut Framebuffer, x: usize, y: usize, h: usize) {
        // Categories header
        fb.draw_string(x + 8, y + 8, "LIBRARY", Theme::TEXT_SECONDARY.to_u32());
        
        let categories = [
            (FontCategory::All, "All Fonts", "📁"),
            (FontCategory::Favorites, "Favorites", "⭐"),
            (FontCategory::Installed, "Installed", "✓"),
            (FontCategory::User, "User", "👤"),
            (FontCategory::System, "System", "⚙"),
        ];
        
        let mut item_y = y + 28;
        
        for (cat, name, icon) in categories {
            let is_selected = self.current_category == cat;
            let bg = if is_selected { Theme::ACCENT_PRIMARY.to_u32() } else { Theme::TRANSPARENT.to_u32() };
            let text_color = if is_selected { Theme::TEXT_ON_ACCENT.to_u32() } else { Theme::TEXT_PRIMARY.to_u32() };
            
            fb.draw_rect(x, item_y, SIDEBAR_WIDTH, 24, bg);
            fb.draw_string(x + 8, item_y + 4, icon, text_color);
            fb.draw_string(x + 28, item_y + 4, name, text_color);
            
            // Count
            let count = self.fonts.iter().filter(|f| {
                cat == FontCategory::All
                    || f.categories.contains(&cat)
                    || (cat == FontCategory::Favorites && f.favorite)
                    || (cat == FontCategory::Installed && f.installed)
            }).count();
            fb.draw_string(x + SIDEBAR_WIDTH - 28, item_y + 4, &format!("{}", count), text_color);
            
            item_y += 26;
        }
        
        // Font Types header
        item_y += 12;
        fb.draw_string(x + 8, item_y, "FONT TYPES", Theme::TEXT_SECONDARY.to_u32());
        item_y += 20;
        
        let types = [
            (FontCategory::SansSerif, "Sans Serif"),
            (FontCategory::Serif, "Serif"),
            (FontCategory::Monospace, "Monospace"),
            (FontCategory::Display, "Display"),
            (FontCategory::Handwriting, "Handwriting"),
            (FontCategory::Symbol, "Symbol"),
        ];
        
        for (cat, name) in types {
            let is_selected = self.current_category == cat;
            let bg = if is_selected { Theme::ACCENT_PRIMARY.to_u32() } else { Theme::TRANSPARENT.to_u32() };
            let text_color = if is_selected { Theme::TEXT_ON_ACCENT.to_u32() } else { Theme::TEXT_PRIMARY.to_u32() };
            
            fb.draw_rect(x, item_y, SIDEBAR_WIDTH, 24, bg);
            fb.draw_string(x + 8, item_y + 4, name, text_color);
            
            item_y += 26;
        }
        
        // Collections header
        item_y += 12;
        fb.draw_string(x + 8, item_y, "COLLECTIONS", Theme::TEXT_SECONDARY.to_u32());
        item_y += 20;
        
        for (i, collection) in self.collections.iter().enumerate() {
            let is_selected = self.selected_collection == Some(i);
            let is_hovered = self.hovered_collection == Some(i);
            
            let bg = if is_selected { Theme::ACCENT_PRIMARY.to_u32() }
                     else if is_hovered { Theme::LIST_ITEM_HOVER.to_u32() }
                     else { Theme::TRANSPARENT.to_u32() };
            let text_color = if is_selected { Theme::TEXT_ON_ACCENT.to_u32() } else { Theme::TEXT_PRIMARY.to_u32() };
            
            fb.draw_rect(x, item_y, SIDEBAR_WIDTH, 24, bg);
            fb.draw_string(x + 8, item_y + 4, "📁", text_color);
            fb.draw_string(x + 28, item_y + 4, &collection.name, text_color);
            
            item_y += 26;
            
            if item_y > y + h - 30 {
                break;
            }
        }
    }
    
    fn draw_preview(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize, font_idx: usize) {
        let font = &self.fonts[font_idx];
        
        // Preview background
        fb.draw_rect(x, y, w, h, Theme::WINDOW_BG.to_u32());
        fb.draw_rect_outline(x, y, w, h, Theme::BORDER.to_u32());
        
        // Font name
        fb.draw_string(x + 8, y + 8, &font.display_name(), Theme::TEXT_PRIMARY.to_u32());
        
        // Font type badge
        let type_name = match font.font_type {
            FontType::Serif => "Serif",
            FontType::SansSerif => "Sans",
            FontType::Monospace => "Mono",
            FontType::Display => "Display",
            FontType::Handwriting => "Script",
            FontType::Symbol => "Symbol",
            FontType::Unknown => "Unknown",
        };
        fb.draw_rect(x + w - 80, y + 4, 60, 20, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(x + w - 72, y + 6, type_name, Theme::TEXT_SECONDARY.to_u32());
        
        // Preview text
        let preview_y = y + 40;
        let preview_text = if !self.preview_text.is_empty() { &self.preview_text } else { &font.sample };
        
        // Draw preview at different sizes
        let sizes = [32, 24, 18, 14];
        let mut size_y = preview_y;
        
        for &size in &sizes {
            fb.draw_string(x + 8, size_y, preview_text, Theme::TEXT_PRIMARY.to_u32());
            size_y += size + 8;
        }
        
        // Metadata (if enabled)
        if self.show_metadata {
            let meta_y = y + h - 60;
            
            fb.draw_string(x + 8, meta_y, &format!("Weight: {}", font.metadata.weight_name()), Theme::TEXT_SECONDARY.to_u32());
            fb.draw_string(x + 120, meta_y, &format!("Width: {}", font.metadata.width_name()), Theme::TEXT_SECONDARY.to_u32());
            fb.draw_string(x + 240, meta_y, &format!("Glyphs: {}", font.metadata.glyph_count), Theme::TEXT_SECONDARY.to_u32());
            fb.draw_string(x + 360, meta_y, &font.format_size(), Theme::TEXT_SECONDARY.to_u32());
        }
    }
    
    fn draw_font_list(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        // Column headers
        let header_h = 28;
        fb.draw_rect(x, y, w, header_h, Theme::TOOLBAR_BG.to_u32());
        
        let columns = [
            ("", 40),
            ("Family", 200),
            ("Style", 100),
            ("Type", 80),
            ("Size", 80),
        ];
        
        let mut col_x = x + 8;
        for (name, width) in columns {
            if !name.is_empty() {
                fb.draw_string(col_x, y + 6, name, Theme::TEXT_SECONDARY.to_u32());
            }
            col_x += width;
        }
        
        // Font rows
        let row_y = y + header_h;
        let visible_rows = (h - header_h) / ROW_HEIGHT;
        let filtered = self.get_filtered_fonts();
        
        for (i, &font_idx) in filtered.iter().skip(self.scroll_offset).take(visible_rows).enumerate() {
            let font = &self.fonts[font_idx];
            let row_y = row_y + i * ROW_HEIGHT;
            
            let is_selected = self.selected_font == Some(font_idx);
            let is_hovered = self.hovered_font == Some(self.scroll_offset + i);
            
            let bg = if is_selected { Theme::ACCENT_PRIMARY.to_u32() }
                     else if is_hovered { Theme::LIST_ITEM_HOVER.to_u32() }
                     else { Theme::WINDOW_BG.to_u32() };
            
            fb.draw_rect(x, row_y, w, ROW_HEIGHT, bg);
            
            let text_color = if is_selected { Theme::TEXT_ON_ACCENT.to_u32() } else { Theme::TEXT_PRIMARY.to_u32() };
            let secondary_color = if is_selected { Theme::TEXT_ON_ACCENT.to_u32() } else { Theme::TEXT_SECONDARY.to_u32() };
            
            // Icon
            fb.draw_string(x + 8, row_y + 6, font.icon(), text_color);
            
            // Favorite indicator
            if font.favorite {
                fb.draw_string(x + 28, row_y + 6, "⭐", 0xFFFFD700);
            } else {
                fb.draw_string(x + 28, row_y + 6, "☆", secondary_color);
            }
            
            // Family
            fb.draw_string(x + 48, row_y + 6, &font.family, text_color);
            
            // Style
            fb.draw_string(x + 248, row_y + 6, &font.style, secondary_color);
            
            // Type
            let type_name = match font.font_type {
                FontType::Serif => "Serif",
                FontType::SansSerif => "Sans",
                FontType::Monospace => "Mono",
                FontType::Display => "Display",
                FontType::Handwriting => "Script",
                FontType::Symbol => "Symbol",
                FontType::Unknown => "?",
            };
            fb.draw_string(x + 348, row_y + 6, type_name, secondary_color);
            
            // Size
            fb.draw_string(x + 428, row_y + 6, &font.format_size(), secondary_color);
            
            // Enabled indicator
            if !font.enabled {
                fb.draw_string(x + w - 40, row_y + 6, "⊘", Theme::ERROR.to_u32());
            }
        }
        
        // Empty state
        if filtered.is_empty() {
            fb.draw_string(x + w / 2 - 50, y + h / 2, "No fonts found", Theme::TEXT_SECONDARY.to_u32());
        }
    }
    
    /// Handle click
    pub fn on_click(&mut self, mx: i32, my: i32) -> FontBookAction {
        let x = self.rect.x;
        let y = self.rect.y;
        let w = self.rect.width;
        let h = self.rect.height;
        
        // View mode buttons
        if my >= (y + 8) as i32 && my < (y + 36) as i32 {
            let mut btn_x = x + 8;
            let views = [FontViewMode::List, FontViewMode::Grid, FontViewMode::Preview];
            
            for mode in views {
                if mx >= btn_x as i32 && mx < (btn_x + 28) as i32 {
                    self.view_mode = mode;
                    return FontBookAction::None;
                }
                btn_x += 32;
            }
        }
        
        // Sidebar
        let content_y = y + TOOLBAR_HEIGHT as i32;
        if mx >= x && mx < x + SIDEBAR_WIDTH as i32 && my >= content_y {
            let mut item_y = content_y + 28;
            
            // Categories
            let categories = [
                FontCategory::All, FontCategory::Favorites, FontCategory::Installed,
                FontCategory::User, FontCategory::System,
            ];
            
            for cat in categories {
                if my >= item_y && my < item_y + 24 {
                    self.set_category(cat);
                    return FontBookAction::None;
                }
                item_y += 26;
            }
            
            // Font types
            item_y += 32;
            let types = [
                FontCategory::SansSerif, FontCategory::Serif, FontCategory::Monospace,
                FontCategory::Display, FontCategory::Handwriting, FontCategory::Symbol,
            ];
            
            for cat in types {
                if my >= item_y && my < item_y + 24 {
                    self.set_category(cat);
                    return FontBookAction::None;
                }
                item_y += 26;
            }
            
            // Collections
            item_y += 32;
            for i in 0..self.collections.len() {
                if my >= item_y as i32 && my < (item_y + 24) as i32 {
                    self.select_collection(i);
                    return FontBookAction::None;
                }
                item_y += 26;
                
                if item_y > (y + h - 30) as i32 {
                    break;
                }
            }
        }
        
        // Font list
        let content_x = x + SIDEBAR_WIDTH as i32;
        let content_w = (w as usize) - SIDEBAR_WIDTH;
        let list_y = content_y + if self.selected_font.is_some() { PREVIEW_HEIGHT as i32 + 8 } else { 0 } + 28;
        let list_h = (h as usize) - TOOLBAR_HEIGHT - if self.selected_font.is_some() { PREVIEW_HEIGHT + 8 } else { 0 } - 28;
        
        if mx >= content_x && my >= list_y {
            let filtered = self.get_filtered_fonts();
            let row_idx = ((my - list_y) / ROW_HEIGHT as i32) as usize;
            let actual_idx = self.scroll_offset + row_idx;
            
            if actual_idx < filtered.len() {
                let font_idx = filtered[actual_idx];
                
                // Check favorite button
                if mx >= content_x + 28 && mx < content_x + 48 {
                    let font = &self.fonts[font_idx];
                    return FontBookAction::ToggleFavorite(font.id);
                }
                
                self.select_font(font_idx);
                return FontBookAction::FontSelected(self.fonts[font_idx].id);
            }
        }
        
        FontBookAction::None
    }
    
    /// Handle key press
    pub fn on_key_press(&mut self, c: char) -> FontBookAction {
        match c {
            '\x08' => { // Backspace
                self.search_query.pop();
            }
            '\x1b' => { // Escape
                self.search_query.clear();
                self.selected_font = None;
            }
            _ if !c.is_control() => {
                self.search_query.push(c);
            }
            _ => {}
        }
        
        FontBookAction::None
    }
    
    /// Resize
    pub fn resize(&mut self, width: usize, height: usize) {
        self.rect.width = width as i32;
        self.rect.height = height as i32;
    }
}

/// Font Book actions
#[derive(Clone, Debug)]
pub enum FontBookAction {
    None,
    FontSelected(u32),
    ToggleFavorite(u32),
    ToggleEnabled(u32),
    InstallFont(String),
    UninstallFont(u32),
    CreateCollection(String),
    DeleteCollection(u32),
}

// ============================================================================
// GLOBAL FONT BOOK
// ============================================================================
// Font Book'un global örneği. `lazy_static!` ile ilk erişimde başlatılır.
// `init()` fonksiyonu çağrıldığında serial porta başlatma mesajı yazılır;
// bu, kernel'ın hangi bileşenleri başlattığını takip etmenizi sağlar.

lazy_static::lazy_static! {
    static ref FONTBOOK: Mutex<FontBookWindow> = Mutex::new(FontBookWindow::new(Rect {
        x: 100,
        y: 100,
        width: 800,
        height: 600,
    }));
}

/// Initialize Font Book
pub fn init() {
    crate::serial_println!("[GUI] Font Book initialized");
}

/// Get Font Book
pub fn get_fontbook() -> &'static Mutex<FontBookWindow> {
    &FONTBOOK
}
