//! # Safari-like Web Browser
//!
//! Tabbed web browser with address bar, bookmarks, and history
//! Note: Actual web rendering would require a rendering engine
//!
//! Bu modül, sekmeli bir web tarayıcısının temel yapısını uygular.
//! Gerçek bir web tarayıcısı için HTML ayrıştırıcısı (parser),
//! CSS motoru ve JavaScript yorumlayıcısı gerekir. Bu uygulama,
//! kullanıcı arayüzü katmanını ve veri modellerini göstermek amacıyla
//! içerik üretimi için yer tutucu (placeholder) yaklaşımını kullanır.
//!
//! Temel kavramlar:
//! - **Sekme (Tab)**: Her sekme bağımsız bir URL geçmişi ve içeriğe sahiptir.
//! - **Yer İmi (Bookmark)**: Sık ziyaret edilen sayfalara hızlı erişim.
//! - **İndirme (Download)**: Arkaplan dosya indirme yönetimi.

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use alloc::collections::BTreeMap;
use spin::Mutex;

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};
use crate::gui::widgets::Widget;
use crate::gui::Rect;

// ============================================================================
// BROWSER CONSTANTS
// ============================================================================
// Tarayıcı arayüzünün piksel boyutları.
// Bu değerler, Chrome'un "browser chrome" (tarayıcı krom) katmanını —
// sekme çubuğu, araç çubuğu ve durum çubuğu — oluşturur.

/// Sekme çubuğunun piksel cinsinden yüksekliği
pub const TAB_BAR_HEIGHT: usize = 32;

/// Araç çubuğunun (adres çubuğu ve gezinme butonları) yüksekliği
pub const TOOLBAR_HEIGHT: usize = 40;

/// Yer imleri çubuğunun yüksekliği
pub const BOOKMARK_BAR_HEIGHT: usize = 28;

/// Durum çubuğunun yüksekliği (bağlantı URL'i ve yükleme durumunu gösterir)
pub const STATUS_BAR_HEIGHT: usize = 24;

// ============================================================================
// BROWSER TAB
// ============================================================================
// Her browser sekmesi (tab), bağımsız bir URL, yükleme durumu ve geçmiş tutar.
// `Option<T>` türü, bir değerin var olup olmadığını Rust'ta güvenle ifade eder:
//   - `Some(değer)`: Değer mevcut
//   - `None`: Değer yok (null referans güvensizliği olmadan)

/// Tek bir tarayıcı sekmesinin durumunu ve içeriğini temsil eder
#[derive(Clone, Debug)]
pub struct BrowserTab {
    /// Sekmenin benzersiz kimliği
    pub id: u32,
    /// Sekmede yüklü olan sayfanın URL'i
    pub url: String,
    /// Sayfanın başlığı (HTML `<title>` etiketinden gelir)
    pub title: String,
    /// Yükleme ilerleme oranı (0.0 = başlangıç, 1.0 = tamamlandı)
    pub loading_progress: f32,
    /// Sayfa şu an yükleniyor mu?
    pub loading: bool,
    /// Bağlantı HTTPS (güvenli) mi?
    pub secure: bool,
    /// Sayfanın favicon (küçük simge) URL'i
    pub favicon: String,
    /// Yatay kaydırma pozisyonu (piksel)
    pub scroll_x: usize,
    /// Dikey kaydırma pozisyonu (piksel)
    pub scroll_y: usize,
    /// Sayfa yakınlaştırma seviyesi (1.0 = %100)
    pub zoom: f32,
    /// Geri gidilebilir durum var mı?
    pub can_back: bool,
    /// İleri gidilebilir durum var mı?
    pub can_forward: bool,
    /// URL gezinme geçmişi listesi
    pub history: Vec<String>,
    /// Geçmiş listesinde şu anki konum
    pub history_pos: usize,
    /// Sayfa içeriği (basitleştirilmiş model)
    pub content: PageContent,
    /// Okuma modu mevcut mu? (reklamları gizler)
    pub reader_mode: bool,
    /// Okuma modu aktif mi?
    pub reader_active: bool,
}

#[derive(Clone, Debug)]
pub struct PageContent {
    /// Page elements
    pub elements: Vec<PageElement>,
    /// Background color
    pub bg_color: u32,
    /// Text color
    pub text_color: u32,
    /// Links
    pub links: Vec<LinkInfo>,
    /// Images
    pub images: Vec<ImageInfo>,
    /// Forms
    pub forms: Vec<FormInfo>,
}

#[derive(Clone, Debug)]
pub struct PageElement {
    pub element_type: ElementType,
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub content: String,
    pub style: ElementStyle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElementType {
    Heading,
    Paragraph,
    Link,
    Image,
    Button,
    Input,
    List,
    Divider,
    Code,
    Quote,
}

#[derive(Clone, Debug)]
pub struct ElementStyle {
    pub font_size: usize,
    pub font_weight: usize,
    pub color: u32,
    pub bg_color: u32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

#[derive(Clone, Debug)]
pub struct LinkInfo {
    pub url: String,
    pub text: String,
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

#[derive(Clone, Debug)]
pub struct ImageInfo {
    pub src: String,
    pub alt: String,
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

#[derive(Clone, Debug)]
pub struct FormInfo {
    pub action: String,
    pub method: String,
    pub inputs: Vec<FormField>,
}

#[derive(Clone, Debug)]
pub struct FormField {
    pub name: String,
    pub field_type: String,
    pub value: String,
    pub placeholder: String,
}

impl BrowserTab {
    pub fn new(id: u32) -> Self {
        BrowserTab {
            id,
            url: String::from("about:blank"),
            title: String::from("New Tab"),
            loading_progress: 0.0,
            loading: false,
            secure: false,
            favicon: String::new(),
            scroll_x: 0,
            scroll_y: 0,
            zoom: 1.0,
            can_back: false,
            can_forward: false,
            history: Vec::new(),
            history_pos: 0,
            content: PageContent::default(),
            reader_mode: false,
            reader_active: false,
        }
    }
    
    pub fn navigate(&mut self, url: &str) {
        // Add to history
        if self.history_pos < self.history.len() - 1 {
            self.history.truncate(self.history_pos + 1);
        }
        self.history.push(String::from(url));
        self.history_pos = self.history.len() - 1;
        
        self.url = String::from(url);
        self.update_can_navigate();
        self.start_loading();
    }
    
    pub fn go_back(&mut self) -> bool {
        if self.can_back {
            self.history_pos -= 1;
            self.url = self.history[self.history_pos].clone();
            self.update_can_navigate();
            self.start_loading();
            return true;
        }
        false
    }
    
    pub fn go_forward(&mut self) -> bool {
        if self.can_forward {
            self.history_pos += 1;
            self.url = self.history[self.history_pos].clone();
            self.update_can_navigate();
            self.start_loading();
            return true;
        }
        false
    }
    
    fn update_can_navigate(&mut self) {
        self.can_back = self.history_pos > 0;
        self.can_forward = self.history_pos < self.history.len() - 1;
    }
    
    fn start_loading(&mut self) {
        self.loading = true;
        self.loading_progress = 0.0;
        self.title = String::from("Loading...");
    }
    
    pub fn reload(&mut self) {
        self.start_loading();
    }
    
    pub fn zoom_in(&mut self) {
        self.zoom = (self.zoom + 0.1).min(3.0);
    }
    
    pub fn zoom_out(&mut self) {
        self.zoom = (self.zoom - 0.1).max(0.3);
    }
    
    pub fn reset_zoom(&mut self) {
        self.zoom = 1.0;
    }
}

impl PageContent {
    pub fn default() -> Self {
        PageContent {
            elements: Vec::new(),
            bg_color: 0xFFFFFF,
            text_color: 0x333333,
            links: Vec::new(),
            images: Vec::new(),
            forms: Vec::new(),
        }
    }
    
    pub fn from_html(_html: &str) -> Self {
        // Would parse HTML - for now return default
        Self::default()
    }
}

// ============================================================================
// BOOKMARK
// ============================================================================
// Yer imi (bookmark), kullanıcının sık ziyaret ettiği URL'leri kaydetmesini sağlar.
// Yer imleri klasörler halinde gruplanabilir (folder alanı ile).

/// Kaydedilmiş bir web adresi (yer imi)
#[derive(Clone, Debug)]
pub struct Bookmark {
    /// Yer iminin benzersiz kimliği
    pub id: u32,
    /// Kaydedilen web adresi
    pub url: String,
    /// Kullanıcıya gösterilen başlık
    pub title: String,
    /// Sitenin favicon URL'i
    pub favicon: String,
    /// Yer iminin ait olduğu klasör adı
    pub folder: String,
    /// Yer iminin eklenme zamanı (UNIX timestamp)
    pub added: u64,
}

impl Bookmark {
    pub fn new(id: u32, url: &str, title: &str) -> Self {
        Bookmark {
            id,
            url: String::from(url),
            title: String::from(title),
            favicon: String::new(),
            folder: String::from("Bookmarks"),
            added: 0,
        }
    }
}

// ============================================================================
// DOWNLOAD
// ============================================================================
// İndirme yöneticisi: Dosya indirme işlemlerini temsil eden veri yapısı.
// Gerçek bir indirme işlemi için asenkron I/O ve ilerleme takibi gerekir.
// Bu yapı, UI katmanında indirme durumunu göstermek için tasarlanmıştır.

/// Devam eden veya tamamlanmış bir dosya indirme işlemi
#[derive(Clone, Debug)]
pub struct DownloadItem {
    /// İndirmenin benzersiz kimliği
    pub id: u32,
    /// İndirilen dosyanın kaynak URL'i
    pub url: String,
    /// Dosya adı
    pub filename: String,
    /// Toplam dosya boyutu (bayt); bilinmiyorsa 0
    pub total_size: u64,
    /// Şu ana kadar indirilen bayt miktarı
    pub downloaded: u64,
    /// İlerleme oranı (0.0 - 1.0)
    pub progress: f32,
    /// İndirme tamamlandı mı?
    pub complete: bool,
    /// Kullanıcı tarafından duraklatıldı mı?
    pub paused: bool,
    /// İptal edildi mi?
    pub cancelled: bool,
    /// Anlık indirme hızı (bayt/saniye)
    pub speed: u64,
    /// Dosyanın kaydedileceği hedef dizin yolu
    pub destination: String,
}

impl DownloadItem {
    pub fn new(id: u32, url: &str, filename: &str) -> Self {
        DownloadItem {
            id,
            url: String::from(url),
            filename: String::from(filename),
            total_size: 0,
            downloaded: 0,
            progress: 0.0,
            complete: false,
            paused: false,
            cancelled: false,
            speed: 0,
            destination: String::from("/home/downloads"),
        }
    }
    
    pub fn format_size(&self) -> String {
        if self.total_size < 1024 {
            format!("{} B", self.total_size)
        } else if self.total_size < 1024 * 1024 {
            format!("{:.1} KB", self.total_size as f64 / 1024.0)
        } else if self.total_size < 1024 * 1024 * 1024 {
            format!("{:.1} MB", self.total_size as f64 / (1024.0 * 1024.0))
        } else {
            format!("{:.1} GB", self.total_size as f64 / (1024.0 * 1024.0 * 1024.0))
        }
    }
    
    pub fn format_speed(&self) -> String {
        if self.speed < 1024 {
            format!("{} B/s", self.speed)
        } else if self.speed < 1024 * 1024 {
            format!("{:.0} KB/s", self.speed as f64 / 1024.0)
        } else {
            format!("{:.1} MB/s", self.speed as f64 / (1024.0 * 1024.0))
        }
    }
}

// ============================================================================
// BROWSER WINDOW
// ============================================================================
// Ana tarayıcı penceresi: tüm sekmeleri, yer imlerini ve indirmeleri yönetir.
// Rust'ta birden fazla yer'den erişilen veriler için `Arc<Mutex<T>>` veya
// global statik `Mutex<T>` kalıpları yaygın olarak kullanılır.

/// Tüm sekmeleri ve tarayıcı durumunu yöneten ana pencere yapısı
pub struct BrowserWindow {
    /// Pencerenin ekrandaki konumu ve boyutu
    pub rect: Rect,
    /// Açık sekmelerin listesi
    pub tabs: Vec<BrowserTab>,
    /// Aktif (görünen) sekmenin indeksi
    pub active_tab: usize,
    /// Kaydedilmiş yer imleri
    pub bookmarks: Vec<Bookmark>,
    /// Devam eden ve tamamlanan indirmeler
    pub downloads: Vec<DownloadItem>,
    /// Yer imleri çubuğu görünür mü?
    pub show_bookmark_bar: bool,
    /// İndirmeler paneli açık mı?
    pub show_downloads: bool,
    /// Geçmiş paneli açık mı?
    pub show_history: bool,
    /// Adres çubuğunun mevcut metin içeriği
    pub address_bar: String,
    /// Adres çubuğu odak aldı mı? (klavye girişi buraya gider)
    pub address_focused: bool,
    /// Kullanılan varsayılan arama motoru URL'i (sorgu parametresiyle)
    pub search_engine: String,
    /// Fare imlecinin üzerinde olduğu sekme indeksi
    pub hovered_tab: Option<usize>,
    /// Fare imlecinin üzerinde olduğu yer imi indeksi
    pub hovered_bookmark: Option<usize>,
    /// Fare imlecinin üzerinde olduğu link indeksi
    pub hovered_link: Option<usize>,
    /// Yeni sekme oluşturulurken kullanılacak sonraki ID
    pub next_tab_id: u32,
    /// Yeni yer imi oluşturulurken kullanılacak sonraki ID
    pub next_bookmark_id: u32,
    /// Yeni indirme başlatılırken kullanılacak sonraki ID
    pub next_download_id: u32,
}

impl BrowserWindow {
    pub fn new(rect: Rect) -> Self {
        let mut browser = BrowserWindow {
            rect,
            tabs: Vec::new(),
            active_tab: 0,
            bookmarks: Vec::new(),
            downloads: Vec::new(),
            show_bookmark_bar: true,
            show_downloads: false,
            show_history: false,
            address_bar: String::new(),
            address_focused: false,
            search_engine: String::from("https://duckduckgo.com/?q="),
            hovered_tab: None,
            hovered_bookmark: None,
            hovered_link: None,
            next_tab_id: 1,
            next_bookmark_id: 1,
            next_download_id: 1,
        };
        
        browser.add_default_bookmarks();
        browser.new_tab();
        
        browser
    }
    
    fn add_default_bookmarks(&mut self) {
        self.bookmarks.push(Bookmark::new(self.next_bookmark_id, "https://echos.local", "echOS Home"));
        self.next_bookmark_id += 1;
        
        self.bookmarks.push(Bookmark::new(self.next_bookmark_id, "https://duckduckgo.com", "DuckDuckGo"));
        self.next_bookmark_id += 1;
        
        self.bookmarks.push(Bookmark::new(self.next_bookmark_id, "https://github.com", "GitHub"));
        self.next_bookmark_id += 1;
    }
    
    pub fn new_tab(&mut self) {
        let tab = BrowserTab::new(self.next_tab_id);
        self.next_tab_id += 1;
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        self.address_bar = String::new();
    }
    
    pub fn close_tab(&mut self, index: usize) {
        if self.tabs.len() > 1 {
            self.tabs.remove(index);
            if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len() - 1;
            }
            self.sync_address_bar();
        }
    }
    
    pub fn select_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active_tab = index;
            self.sync_address_bar();
        }
    }
    
    fn sync_address_bar(&mut self) {
        self.address_bar = self.tabs[self.active_tab].url.clone();
    }
    
    /// Verilen URL'e git. URL'nin formatına göre uygun işlem yapılır:
    /// - http:// veya https:// ile başlıyorsa doğrudan yükle
    /// - Nokta içeriyor ve boşluk yoksa başına "https://" ekle
    /// - Aksi halde arama motoruna yönlendir
    pub fn navigate(&mut self, url: &str) {
        let url = if url.starts_with("http://") || url.starts_with("https://") {
            String::from(url)
        } else if url.contains('.') && !url.contains(' ') {
            format!("https://{}", url)
        } else {
            // Search
            format!("{}{}", self.search_engine, url)
        };
        
        self.tabs[self.active_tab].navigate(&url);
        self.address_bar = url;
    }
    
    pub fn go_back(&mut self) {
        if self.tabs[self.active_tab].go_back() {
            self.sync_address_bar();
        }
    }
    
    pub fn go_forward(&mut self) {
        if self.tabs[self.active_tab].go_forward() {
            self.sync_address_bar();
        }
    }
    
    pub fn reload(&mut self) {
        self.tabs[self.active_tab].reload();
    }
    
    pub fn add_bookmark(&mut self) {
        let tab = &self.tabs[self.active_tab];
        if !tab.url.is_empty() && tab.url != "about:blank" {
            let bookmark = Bookmark::new(self.next_bookmark_id, &tab.url, &tab.title);
            self.next_bookmark_id += 1;
            self.bookmarks.push(bookmark);
        }
    }
    
    pub fn remove_bookmark(&mut self, index: usize) {
        if index < self.bookmarks.len() {
            self.bookmarks.remove(index);
        }
    }
    
    pub fn toggle_reader_mode(&mut self) {
        let tab = &mut self.tabs[self.active_tab];
        if tab.reader_mode {
            tab.reader_active = !tab.reader_active;
        }
    }
    
    pub fn zoom_in(&mut self) {
        self.tabs[self.active_tab].zoom_in();
    }
    
    pub fn zoom_out(&mut self) {
        self.tabs[self.active_tab].zoom_out();
    }
    
    pub fn reset_zoom(&mut self) {
        self.tabs[self.active_tab].reset_zoom();
    }
    
    /// Update loading
    pub fn update(&mut self, dt: f32) {
        for tab in &mut self.tabs {
            if tab.loading {
                tab.loading_progress += dt * 0.5;
                if tab.loading_progress >= 1.0 {
                    tab.loading_progress = 1.0;
                    tab.loading = false;
                    tab.title = Self::extract_title(&tab.url);
                    tab.secure = tab.url.starts_with("https://");
                    tab.content = Self::generate_sample_content(&tab.url);
                    tab.reader_mode = !tab.content.elements.is_empty();
                }
            }
        }
    }
    
    fn extract_title(url: &str) -> String {
        // Would extract from actual page
        let url = url.trim_start_matches("https://").trim_start_matches("http://");
        let domain = url.split('/').next().unwrap_or("Page");
        domain.to_string()
    }
    
    fn generate_sample_content(url: &str) -> PageContent {
        let mut content = PageContent::default();
        
        // Generate sample content based on URL
        let domain = url.trim_start_matches("https://").trim_start_matches("http://");
        
        // Add heading
        content.elements.push(PageElement {
            element_type: ElementType::Heading,
            x: 20,
            y: 20,
            width: 400,
            height: 40,
            content: format!("Welcome to {}", domain),
            style: ElementStyle {
                font_size: 24,
                font_weight: 700,
                color: 0x333333,
                bg_color: 0xFFFFFF,
                bold: true,
                italic: false,
                underline: false,
            },
        });
        
        // Add paragraphs
        for i in 0..5 {
            content.elements.push(PageElement {
                element_type: ElementType::Paragraph,
                x: 20,
                y: 80 + i * 60,
                width: 600,
                height: 50,
                content: format!("This is sample paragraph {} for demonstration purposes. In a real browser, this would be actual content from the webpage.", i + 1),
                style: ElementStyle {
                    font_size: 14,
                    font_weight: 400,
                    color: 0x333333,
                    bg_color: 0xFFFFFF,
                    bold: false,
                    italic: false,
                    underline: false,
                },
            });
        }
        
        // Add links
        content.links.push(LinkInfo {
            url: format!("https://{}/about", domain),
            text: String::from("About Us"),
            x: 20,
            y: 400,
            width: 80,
            height: 20,
        });
        
        content
    }
    
    /// Draw browser window
    pub fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let w = self.rect.width as usize;
        let h = self.rect.height as usize;
        
        // Window background
        fb.draw_rect(x, y, w, h, Theme::WINDOW_BG.to_u32());
        fb.draw_rect_outline(x, y, w, h, Theme::BORDER.to_u32());
        
        // Tab bar
        fb.draw_rect(x, y, w, TAB_BAR_HEIGHT, Theme::SIDEBAR_BG.to_u32());
        self.draw_tabs(fb, x, y, w);
        
        // Toolbar
        let toolbar_y = y + TAB_BAR_HEIGHT;
        fb.draw_rect(x, toolbar_y, w, TOOLBAR_HEIGHT, Theme::TOOLBAR_BG.to_u32());
        self.draw_toolbar(fb, x, toolbar_y, w);
        
        // Bookmark bar
        let content_y = if self.show_bookmark_bar {
            let bm_y = toolbar_y + TOOLBAR_HEIGHT;
            fb.draw_rect(x, bm_y, w, BOOKMARK_BAR_HEIGHT, Theme::SIDEBAR_BG.to_u32());
            self.draw_bookmark_bar(fb, x, bm_y, w);
            bm_y + BOOKMARK_BAR_HEIGHT
        } else {
            toolbar_y + TOOLBAR_HEIGHT
        };
        
        // Content area
        let content_h = h - TAB_BAR_HEIGHT - TOOLBAR_HEIGHT - STATUS_BAR_HEIGHT 
            - if self.show_bookmark_bar { BOOKMARK_BAR_HEIGHT } else { 0 };
        
        self.draw_content(fb, x, content_y, w, content_h);
        
        // Status bar
        let status_y = y + h - STATUS_BAR_HEIGHT;
        fb.draw_rect(x, status_y, w, STATUS_BAR_HEIGHT, Theme::TOOLBAR_BG.to_u32());
        self.draw_status_bar(fb, x, status_y, w);
        
        // Downloads panel
        if self.show_downloads {
            self.draw_downloads_panel(fb, x + w - 320, content_y, 300, content_h);
        }
    }
    
    fn draw_tabs(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize) {
        let tab_width = 180.min(w / self.tabs.len().max(1));
        let mut tab_x = x + 8;
        
        for (i, tab) in self.tabs.iter().enumerate() {
            let is_active = i == self.active_tab;
            let is_hovered = self.hovered_tab == Some(i);
            
            let bg = if is_active { Theme::WINDOW_BG.to_u32() }
                     else if is_hovered { Theme::LIST_ITEM_HOVER.to_u32() }
                     else { Theme::SIDEBAR_BG.to_u32() };
            
            fb.draw_rect(tab_x, y, tab_width, TAB_BAR_HEIGHT, bg);
            
            // Favicon/security indicator
            if tab.secure {
                fb.draw_string(tab_x + 8, y + 8, "🔒", Theme::TEXT_PRIMARY.to_u32());
            } else if tab.loading {
                // Loading spinner
                fb.draw_string(tab_x + 8, y + 8, "◌", Theme::ACCENT_PRIMARY.to_u32());
            } else {
                fb.draw_string(tab_x + 8, y + 8, "🌐", Theme::TEXT_SECONDARY.to_u32());
            }
            
            // Title
            let title = if tab.title.len() > 14 { format!("{}...", &tab.title[..11]) } else { tab.title.clone() };
            fb.draw_string(tab_x + 28, y + 8, &title, Theme::TEXT_PRIMARY.to_u32());
            
            // Close button
            fb.draw_string(tab_x + tab_width - 20, y + 8, "×", Theme::TEXT_SECONDARY.to_u32());
            
            // Loading progress bar
            if tab.loading && tab.loading_progress > 0.0 {
                let progress_width = (tab_width as f32 * tab.loading_progress) as usize;
                fb.draw_rect(tab_x, y + TAB_BAR_HEIGHT - 2, progress_width, 2, Theme::ACCENT_PRIMARY.to_u32());
            }
            
            tab_x += tab_width + 2;
        }
        
        // New tab button
        fb.draw_string(tab_x + 4, y + 8, "+", Theme::TEXT_SECONDARY.to_u32());
    }
    
    fn draw_toolbar(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize) {
        let mut btn_x = x + 8;
        
        // Back button
        let can_back = self.tabs[self.active_tab].can_back;
        let color = if can_back { Theme::TEXT_PRIMARY.to_u32() } else { Theme::TEXT_DISABLED.to_u32() };
        fb.draw_rect(btn_x, y + 8, 28, 24, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(btn_x + 6, y + 12, "◀", color);
        btn_x += 32;
        
        // Forward button
        let can_forward = self.tabs[self.active_tab].can_forward;
        let color = if can_forward { Theme::TEXT_PRIMARY.to_u32() } else { Theme::TEXT_DISABLED.to_u32() };
        fb.draw_rect(btn_x, y + 8, 28, 24, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(btn_x + 6, y + 12, "▶", color);
        btn_x += 36;
        
        // Address bar
        let addr_width = w - 200;
        fb.draw_rect(btn_x, y + 6, addr_width, 28, Theme::SIDEBAR_BG.to_u32());
        
        // Security indicator
        if self.tabs[self.active_tab].secure {
            fb.draw_string(btn_x + 8, y + 10, "🔒", 0xFF00B894);
        } else {
            fb.draw_string(btn_x + 8, y + 10, "ℹ", Theme::TEXT_SECONDARY.to_u32());
        }
        
        // URL text
        let url_text = if self.address_focused { &self.address_bar } else { &self.tabs[self.active_tab].url };
        fb.draw_string(btn_x + 28, y + 10, url_text, Theme::TEXT_PRIMARY.to_u32());
        
        // Cursor if focused
        if self.address_focused {
            let cursor_x = btn_x + 28 + self.address_bar.len() * 8;
            fb.draw_rect(cursor_x, y + 10, 2, 16, Theme::TEXT_PRIMARY.to_u32());
        }
        
        // Refresh button
        btn_x = x + w - 100;
        fb.draw_rect(btn_x, y + 8, 28, 24, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(btn_x + 6, y + 12, "↻", Theme::TEXT_PRIMARY.to_u32());
        btn_x += 32;
        
        // Share/bookmark button
        fb.draw_rect(btn_x, y + 8, 28, 24, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(btn_x + 6, y + 12, "☆", Theme::TEXT_PRIMARY.to_u32());
        btn_x += 32;
        
        // Downloads button
        let dl_color = if !self.downloads.is_empty() { Theme::ACCENT_PRIMARY.to_u32() } else { Theme::TEXT_SECONDARY.to_u32() };
        fb.draw_rect(btn_x, y + 8, 28, 24, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(btn_x + 6, y + 12, "⬇", dl_color);
    }
    
    fn draw_bookmark_bar(&self, fb: &mut Framebuffer, x: usize, y: usize, _w: usize) {
        let mut bm_x = x + 8;
        
        for (i, bookmark) in self.bookmarks.iter().enumerate() {
            let is_hovered = self.hovered_bookmark == Some(i);
            let bg = if is_hovered { Theme::LIST_ITEM_HOVER.to_u32() } else { Theme::TRANSPARENT.to_u32() };
            
            fb.draw_rect(bm_x, y + 4, bookmark.title.len() * 8 + 24, 20, bg);
            fb.draw_string(bm_x + 8, y + 6, "📄", Theme::TEXT_SECONDARY.to_u32());
            fb.draw_string(bm_x + 24, y + 6, &bookmark.title, Theme::TEXT_PRIMARY.to_u32());
            
            bm_x += bookmark.title.len() * 8 + 32;
        }
    }
    
    fn draw_content(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        let tab = &self.tabs[self.active_tab];
        
        // Background
        let bg_color = if tab.reader_active { 0xFFFFF8E7 } else { tab.content.bg_color };
        fb.draw_rect(x, y, w, h, bg_color);
        
        if tab.loading {
            // Loading indicator
            let center_x = x + w / 2;
            let center_y = y + h / 2;
            
            fb.draw_string(center_x - 40, center_y - 8, "Loading...", Theme::TEXT_SECONDARY.to_u32());
            
            // Progress bar
            let bar_width = 200;
            let bar_x = center_x - bar_width / 2;
            let bar_y = center_y + 20;
            
            fb.draw_rect(bar_x, bar_y, bar_width, 4, Theme::BORDER.to_u32());
            fb.draw_rect(bar_x, bar_y, (bar_width as f32 * tab.loading_progress) as usize, 4, Theme::ACCENT_PRIMARY.to_u32());
        } else if tab.url == "about:blank" {
            // Blank page
            let center_x = x + w / 2;
            let center_y = y + h / 2;
            
            fb.draw_string(center_x - 40, center_y - 20, "echOS Browser", Theme::TEXT_SECONDARY.to_u32());
            fb.draw_string(center_x - 80, center_y + 4, "Enter a URL or search term above", Theme::TEXT_SECONDARY.to_u32());
        } else {
            // Draw page content
            let scroll_y = tab.scroll_y;
            let zoom = tab.zoom;
            
            for element in &tab.content.elements {
                let elem_y = y + (element.y as f32 * zoom) as usize - scroll_y;
                
                if elem_y < y || elem_y > y + h {
                    continue;
                }
                
                let elem_x = x + (element.x as f32 * zoom) as usize;
                let elem_w = (element.width as f32 * zoom) as usize;
                
                match element.element_type {
                    ElementType::Heading => {
                        fb.draw_string(elem_x, elem_y, &element.content, element.style.color);
                    }
                    ElementType::Paragraph => {
                        // Word wrap
                        let words: Vec<&str> = element.content.split_whitespace().collect();
                        let mut line = String::new();
                        let mut line_y = elem_y;
                        
                        for word in words {
                            if line.len() + word.len() > elem_w / 8 {
                                fb.draw_string(elem_x, line_y, &line, element.style.color);
                                line_y += 16;
                                line.clear();
                            }
                            if !line.is_empty() {
                                line.push(' ');
                            }
                            line.push_str(word);
                        }
                        if !line.is_empty() {
                            fb.draw_string(elem_x, line_y, &line, element.style.color);
                        }
                    }
                    _ => {
                        fb.draw_string(elem_x, elem_y, &element.content, element.style.color);
                    }
                }
            }
            
            // Draw links
            for link in &tab.content.links {
                let link_y = y + (link.y as f32 * zoom) as usize - scroll_y;
                let link_x = x + (link.x as f32 * zoom) as usize;
                
                if link_y >= y && link_y < y + h {
                    let color = if self.hovered_link == Some(0) { Theme::ACCENT_SUCCESS.to_u32() } else { Theme::ACCENT_PRIMARY.to_u32() };
                    fb.draw_string(link_x, link_y, &link.text, color);
                    fb.draw_rect(link_x, link_y + 14, link.text.len() * 8, 1, color);
                }
            }
        }
    }
    
    fn draw_status_bar(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize) {
        let tab = &self.tabs[self.active_tab];
        
        // Zoom level
        if tab.zoom != 1.0 {
            fb.draw_string(x + 8, y + 4, &format!("{}%", (tab.zoom * 100.0) as u32), Theme::TEXT_SECONDARY.to_u32());
        }
        
        // URL on hover (would show link URL)
        if let Some(_link_idx) = self.hovered_link {
            fb.draw_string(x + w / 2 - 40, y + 4, "https://example.com", Theme::TEXT_SECONDARY.to_u32());
        }
    }
    
    fn draw_downloads_panel(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        // Panel background
        fb.draw_rect(x, y, w, h, Theme::WINDOW_BG.to_u32());
        fb.draw_rect_outline(x, y, w, h, Theme::BORDER.to_u32());
        
        // Header
        fb.draw_rect(x, y, w, 32, Theme::TOOLBAR_BG.to_u32());
        fb.draw_string(x + 8, y + 8, "Downloads", Theme::TEXT_PRIMARY.to_u32());
        
        // Downloads list
        let mut dl_y = y + 40;
        
        for dl in &self.downloads {
            if dl_y + 60 > y + h {
                break;
            }
            
            // Item background
            fb.draw_rect(x + 4, dl_y, w - 8, 56, Theme::SIDEBAR_BG.to_u32());
            
            // Filename
            fb.draw_string(x + 12, dl_y + 4, &dl.filename, Theme::TEXT_PRIMARY.to_u32());
            
            // Progress bar
            let bar_width = w - 24;
            fb.draw_rect(x + 12, dl_y + 24, bar_width, 8, Theme::BORDER.to_u32());
            fb.draw_rect(x + 12, dl_y + 24, (bar_width as f32 * dl.progress) as usize, 8, Theme::ACCENT_PRIMARY.to_u32());
            
            // Status
            let status = if dl.complete {
                String::from("Complete")
            } else if dl.paused {
                String::from("Paused")
            } else {
                format!("{} - {}", dl.format_speed(), dl.format_size())
            };
            fb.draw_string(x + 12, dl_y + 36, &status, Theme::TEXT_SECONDARY.to_u32());
            
            dl_y += 64;
        }
        
        if self.downloads.is_empty() {
            fb.draw_string(x + 40, y + h / 2, "No downloads", Theme::TEXT_SECONDARY.to_u32());
        }
    }
    
    /// Handle click
    pub fn on_click(&mut self, mx: i32, my: i32) -> BrowserAction {
        let x = self.rect.x;
        let y = self.rect.y;
        let w = self.rect.width;
        
        // Tab bar
        if my >= y && my < y + TAB_BAR_HEIGHT as i32 {
            let tab_width = 180.min(w as usize / self.tabs.len().max(1)) as i32;
            let mut tab_x = x + 8;
            
            for i in 0..self.tabs.len() {
                if mx >= tab_x && mx < tab_x + tab_width {
                    // Close button
                    if mx > tab_x + tab_width - 20 {
                        self.close_tab(i);
                    } else {
                        self.select_tab(i);
                    }
                    return BrowserAction::None;
                }
                tab_x += tab_width + 2;
            }
            
            // New tab
            if mx >= tab_x {
                self.new_tab();
            }
        }
        
        // Toolbar
        let toolbar_y = y + TAB_BAR_HEIGHT as i32;
        if my >= toolbar_y + 8 && my < toolbar_y + 32 {
            let mut btn_x = x + 8;
            
            // Back
            if mx >= btn_x && mx < btn_x + 28 {
                self.go_back();
                return BrowserAction::None;
            }
            btn_x += 32;
            
            // Forward
            if mx >= btn_x && mx < btn_x + 28 {
                self.go_forward();
                return BrowserAction::None;
            }
            btn_x += 36;
            
            // Address bar
            let addr_width = w - 200;
            if mx >= btn_x && mx < btn_x + addr_width {
                self.address_focused = true;
                self.address_bar = self.tabs[self.active_tab].url.clone();
                return BrowserAction::None;
            }
            
            // Refresh
            btn_x = x + w - 100;
            if mx >= btn_x && mx < btn_x + 28 {
                self.reload();
                return BrowserAction::None;
            }
            btn_x += 32;
            
            // Bookmark
            if mx >= btn_x && mx < btn_x + 28 {
                self.add_bookmark();
                return BrowserAction::None;
            }
            btn_x += 32;
            
            // Downloads
            if mx >= btn_x && mx < btn_x + 28 {
                self.show_downloads = !self.show_downloads;
            }
        }
        
        // Bookmark bar
        if self.show_bookmark_bar {
            let bm_y = toolbar_y + TOOLBAR_HEIGHT as i32;
            if my >= bm_y && my < bm_y + BOOKMARK_BAR_HEIGHT as i32 {
                let mut bm_x = x + 8;
                
                for bookmark in &self.bookmarks {
                    let bm_width = (bookmark.title.len() * 8 + 24) as i32;
                    if mx >= bm_x && mx < bm_x + bm_width {
                        let url = bookmark.url.clone();
                        self.navigate(&url);
                        return BrowserAction::None;
                    }
                    bm_x += bm_width + 8;
                }
            }
        }
        
        BrowserAction::None
    }
    
    /// Handle key press
    pub fn on_key_press(&mut self, c: char) -> BrowserAction {
        if self.address_focused {
            if c == '\x1b' { // Escape
                self.address_focused = false;
                self.address_bar = self.tabs[self.active_tab].url.clone();
            } else if c == '\n' { // Enter
                self.address_focused = false;
                let url = self.address_bar.clone();
                self.navigate(&url);
            } else if c == '\x08' { // Backspace
                self.address_bar.pop();
            } else if !c.is_control() {
                self.address_bar.push(c);
            }
            return BrowserAction::None;
        }
        
        // Keyboard shortcuts
        match c {
            'r' if self.tabs[self.active_tab].url != "about:blank" => {
                self.reload();
            }
            't' => {
                self.new_tab();
            }
            'w' if self.tabs.len() > 1 => {
                self.close_tab(self.active_tab);
            }
            'l' => {
                self.address_focused = true;
                self.address_bar = self.tabs[self.active_tab].url.clone();
            }
            _ => {}
        }
        
        BrowserAction::None
    }
    
    /// Resize
    pub fn resize(&mut self, width: usize, height: usize) {
        self.rect.width = width as i32;
        self.rect.height = height as i32;
    }
}

/// Browser actions
#[derive(Clone, Debug)]
pub enum BrowserAction {
    None,
    Navigate(String),
    OpenInNewTab(String),
    Download(String),
    BookmarkAdded(String),
}

// ============================================================================
// GLOBAL BROWSER
// ============================================================================
// Tek bir global tarayıcı örneği. `lazy_static!` ile ilk erişimde oluşturulur.
// `Mutex` sayesinde çoklu çekirdek erişimi güvenli hale gelir.
// `get_browser()` fonksiyonu ile bu örneğe her yerden ulaşılabilir.

lazy_static::lazy_static! {
    static ref BROWSER: Mutex<BrowserWindow> = Mutex::new(BrowserWindow::new(Rect {
        x: 100,
        y: 100,
        width: 1000,
        height: 700,
    }));
}

/// Initialize browser
pub fn init() {
    crate::serial_println!("[GUI] Browser initialized");
}

/// Get browser
pub fn get_browser() -> &'static Mutex<BrowserWindow> {
    &BROWSER
}
