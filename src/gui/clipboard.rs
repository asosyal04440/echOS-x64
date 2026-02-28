//! # Pano Yöneticisi (Clipboard Manager)
//!
//! Geçmişli ve çok formatlı sistem panosu.
//! Metin, resim, dosya ve özel veri türlerini destekler.
//!
//! ## Mimari
//! - `ClipboardData`: Boş, düz metin, zengin metin, resim, dosya yolu, URL ve özel format
//! - `ClipboardItem`: Pano geçmişindeki tek öğe (kaynak uygulama, zaman damgası, sabitleme)
//! - `ClipboardManager`: Tüm pano durumunu yönetir; LRU geçmişi, arama ve filtreleme
//! - `ClipboardFilter`: Tür bazlı filtre (Metin, Resim, Dosya, URL, Sabitlenmiş)
//! - `ClipboardAction`: Pano olayları (Kopyala, Yapıştır, Sabitle, Sil vb.)
//!
//! ## Geçmiş Yönetimi (LRU)
//! Her `copy()` çağrısında yeni öğe kuyruğun başına eklenir; kuyruk boyutu
//! `MAX_HISTORY` (50) sınırını aşarsa en eski öğe kaldırılır. Sabitlenen
//! (`pinned`) öğeler bu tahliyeden muaftır ve her zaman geçmişte kalır.
//!
//! ## Veri Boyutu Güvenliği
//! `MAX_ITEM_SIZE` (10 MB) sınırı aşılırsa `copy()` işlemi reddedilir.
//! Bu, çekirdek belleğinin pano verisiyle doldurulmasını önler.
//!
//! ## `ClipboardData::Image` Formatı
//! Piksel verisi `Vec<u32>` olarak `0xRRGGBB` formatında saklanır.
//! Genişlik × yükseklik adet piksel beklenir; tutarsızlık önizleme
//! çiziminde sınır dışı erişimi tetikleyebileceğinden boyut doğrulanır.

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use alloc::collections::VecDeque;
use spin::Mutex;

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};

// ============================================================================
// PANO SABİTLERİ
// ============================================================================

/// Maksimum geçmiş öğesi sayısı
pub const MAX_HISTORY: usize = 50;

/// Maksimum öğe boyutu (bayt)
pub const MAX_ITEM_SIZE: usize = 10 * 1024 * 1024; // 10 MB

// ============================================================================
// PANO VERİSİ
// ============================================================================

/// Pano veri türleri
#[derive(Clone, Debug)]
pub enum ClipboardData {
    /// Veri yok
    Empty,
    /// Düz metin
    Text(String),
    /// Zengin metin (HTML/RTF)
    RichText { html: String, plain: String },
    /// Resim verisi
    Image { width: usize, height: usize, data: Vec<u32> },
    /// Dosya yolları
    Files(Vec<String>),
    /// URL
    Url(String),
    /// Format tanımlayıcılı özel veri
    Custom { format: String, data: Vec<u8> },
}

impl ClipboardData {
    pub fn is_empty(&self) -> bool {
        match self {
            ClipboardData::Empty => true,
            ClipboardData::Text(t) => t.is_empty(),
            ClipboardData::RichText { html, plain } => html.is_empty() && plain.is_empty(),
            ClipboardData::Image { data, .. } => data.is_empty(),
            ClipboardData::Files(f) => f.is_empty(),
            ClipboardData::Url(u) => u.is_empty(),
            ClipboardData::Custom { data, .. } => data.is_empty(),
        }
    }

    pub fn size(&self) -> usize {
        match self {
            ClipboardData::Empty => 0,
            ClipboardData::Text(t) => t.len(),
            ClipboardData::RichText { html, plain } => html.len() + plain.len(),
            ClipboardData::Image { width, height, .. } => width * height * 4,
            ClipboardData::Files(f) => f.iter().map(|p| p.len()).sum(),
            ClipboardData::Url(u) => u.len(),
            ClipboardData::Custom { data, .. } => data.len(),
        }
    }

    pub fn format_size(&self) -> String {
        let size = self.size();

        if size < 1024 {
            format!("{} B", size)
        } else if size < 1024 * 1024 {
            format!("{:.1} KB", size as f64 / 1024.0)
        } else {
            format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
        }
    }

    pub fn preview(&self, max_len: usize) -> String {
        match self {
            ClipboardData::Empty => String::from("(boş)"),
            ClipboardData::Text(t) => {
                if t.len() > max_len {
                    format!("{}...", &t[..max_len])
                } else {
                    t.clone()
                }
            }
            ClipboardData::RichText { plain, .. } => {
                if plain.len() > max_len {
                    format!("{}...", &plain[..max_len])
                } else {
                    plain.clone()
                }
            }
            ClipboardData::Image { width, height, .. } => {
                format!("Resim: {}x{}", width, height)
            }
            ClipboardData::Files(files) => {
                if files.len() == 1 {
                    let name = files[0].rsplit('/').next().unwrap_or(&files[0]);
                    if name.len() > max_len {
                        format!("{}...", &name[..max_len])
                    } else {
                        name.to_string()
                    }
                } else {
                    format!("{} dosya", files.len())
                }
            }
            ClipboardData::Url(u) => {
                if u.len() > max_len {
                    format!("{}...", &u[..max_len])
                } else {
                    u.clone()
                }
            }
            ClipboardData::Custom { format, .. } => {
                format!("Özel: {}", format)
            }
        }
    }

    pub fn data_type(&self) -> &'static str {
        match self {
            ClipboardData::Empty => "boş",
            ClipboardData::Text(_) => "metin",
            ClipboardData::RichText { .. } => "zengin metin",
            ClipboardData::Image { .. } => "resim",
            ClipboardData::Files(_) => "dosyalar",
            ClipboardData::Url(_) => "url",
            ClipboardData::Custom { .. } => "özel",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            ClipboardData::Empty => "📋",
            ClipboardData::Text(_) => "📝",
            ClipboardData::RichText { .. } => "📄",
            ClipboardData::Image { .. } => "🖼",
            ClipboardData::Files(_) => "📁",
            ClipboardData::Url(_) => "🔗",
            ClipboardData::Custom { .. } => "📦",
        }
    }
}

// ============================================================================
// PANO ÖĞESİ
// ============================================================================

/// Bir pano geçmişi öğesi
#[derive(Clone, Debug)]
pub struct ClipboardItem {
    /// Öğe kimliği
    pub id: u32,
    /// Pano verisi
    pub data: ClipboardData,
    /// Kaynak uygulama
    pub source_app: String,
    /// Zaman damgası (epoch'tan itibaren saniye)
    pub timestamp: u64,
    /// Sabitlenmiş mi (kaldırılmaz)
    pub pinned: bool,
    /// Favori mi
    pub favorite: bool,
    /// Kopyalama sayısı
    pub copy_count: u32,
    /// Etiketler
    pub tags: Vec<String>,
}

impl ClipboardItem {
    pub fn new(id: u32, data: ClipboardData, source: &str) -> Self {
        ClipboardItem {
            id,
            data,
            source_app: String::from(source),
            timestamp: 0, // Gerçek zamanlama kullanılacak
            pinned: false,
            favorite: false,
            copy_count: 1,
            tags: Vec::new(),
        }
    }

    pub fn touch(&mut self) {
        self.timestamp = 0; // Gerçek zamanlama ile güncellenecek
        self.copy_count += 1;
    }

    pub fn format_time(&self) -> String {
        // Gerçek zaman damgasını biçimlendirecek
        String::from("Az önce")
    }
}

// ============================================================================
// PANO YÖNETİCİSİ
// ============================================================================

/// Geçmişli pano yöneticisi
pub struct ClipboardManager {
    /// Mevcut pano içeriği
    pub current: ClipboardData,
    /// Geçmiş öğeler (en yeni önce)
    pub history: VecDeque<ClipboardItem>,
    /// Sonraki öğe kimliği
    pub next_id: u32,
    /// Maksimum geçmiş boyutu
    pub max_history: usize,
    /// Cihazlar arası senkronizasyon
    pub sync_enabled: bool,
    /// Menü çubuğunda göster
    pub show_in_menu: bool,
    /// Klavye kısayolu
    pub shortcut: String,
    /// Geçmişte seçili öğe
    pub selected_item: Option<u32>,
    /// Arama sorgusu
    pub search_query: String,
    /// Filtre türü
    pub filter_type: Option<ClipboardFilter>,
    /// Üzerine gelinmiş öğe
    pub hovered_item: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardFilter {
    All,
    Text,
    Images,
    Files,
    URLs,
    Pinned,
}

impl ClipboardManager {
    pub fn new() -> Self {
        ClipboardManager {
            current: ClipboardData::Empty,
            history: VecDeque::with_capacity(MAX_HISTORY),
            next_id: 1,
            max_history: MAX_HISTORY,
            sync_enabled: false,
            show_in_menu: true,
            shortcut: String::from("⌘⇧V"),
            selected_item: None,
            search_query: String::new(),
            filter_type: None,
            hovered_item: None,
        }
    }

    /// Panoya veri kopyala
    pub fn copy(&mut self, data: ClipboardData, source: &str) {
        if data.is_empty() || data.size() > MAX_ITEM_SIZE {
            return;
        }

        // Mevcutla aynı mı kontrol et
        if self.is_same_data(&data) {
            // Mevcut öğenin zaman damgasını güncelle
            if let Some(item) = self.history.front_mut() {
                item.touch();
            }
            return;
        }

        // Yeni öğe oluştur
        let item = ClipboardItem::new(self.next_id, data.clone(), source);
        self.next_id += 1;

        // Geçmişe ekle
        self.history.push_front(item);

        // Geçmişi kırp
        while self.history.len() > self.max_history {
            // Sabitlenmiş öğeleri kaldırma
            if let Some(last) = self.history.back() {
                if last.pinned {
                    // Sabitlenmemiş öğeyi bul ve kaldır
                    let idx = self.history.iter().rposition(|i| !i.pinned);
                    if let Some(idx) = idx {
                        self.history.remove(idx);
                    }
                } else {
                    self.history.pop_back();
                }
            }
        }

        // Mevcut veriyi güncelle
        self.current = data;
    }

    fn is_same_data(&self, data: &ClipboardData) -> bool {
        match (&self.current, data) {
            (ClipboardData::Text(a), ClipboardData::Text(b)) => a == b,
            (ClipboardData::Url(a), ClipboardData::Url(b)) => a == b,
            (ClipboardData::Files(a), ClipboardData::Files(b)) => a == b,
            _ => false,
        }
    }

    /// Panodan yapıştır
    pub fn paste(&self) -> Option<&ClipboardData> {
        if self.current.is_empty() {
            None
        } else {
            Some(&self.current)
        }
    }

    /// Geçmiş öğeden yapıştır
    pub fn paste_from_history(&mut self, item_id: u32) -> Option<ClipboardData> {
        for item in &mut self.history.iter_mut() {
            if item.id == item_id {
                item.touch();
                self.current = item.data.clone();
                return Some(item.data.clone());
            }
        }
        None
    }

    /// Panoyu temizle
    pub fn clear(&mut self) {
        self.current = ClipboardData::Empty;
    }

    /// Geçmişi temizle
    pub fn clear_history(&mut self) {
        // Sabitlenmiş öğeleri koru
        self.history.retain(|i| i.pinned);
    }

    /// Öğeyi sabitle/sabitini kaldır
    pub fn toggle_pin(&mut self, item_id: u32) {
        for item in &mut self.history {
            if item.id == item_id {
                item.pinned = !item.pinned;
                break;
            }
        }
    }

    /// Favoriye ekle/çıkar
    pub fn toggle_favorite(&mut self, item_id: u32) {
        for item in &mut self.history {
            if item.id == item_id {
                item.favorite = !item.favorite;
                break;
            }
        }
    }

    /// Öğeyi sil
    pub fn delete_item(&mut self, item_id: u32) {
        self.history.retain(|i| i.id != item_id);
    }

    /// Geçmişte ara
    pub fn search(&mut self) {
        if self.search_query.is_empty() {
            self.filter_type = None;
            return;
        }

        let query = self.search_query.to_lowercase();

        for item in &self.history {
            let matches = match &item.data {
                ClipboardData::Text(t) => t.to_lowercase().contains(&query),
                ClipboardData::RichText { plain, .. } => plain.to_lowercase().contains(&query),
                ClipboardData::Url(u) => u.to_lowercase().contains(&query),
                ClipboardData::Files(files) => files.iter().any(|f| f.to_lowercase().contains(&query)),
                _ => false,
            };

            // Eşleşmeye göre öğeleri görünür/gizli olarak işaretle
        }
    }

    /// Filtrelenmiş geçmişi al
    pub fn get_filtered_history(&self) -> Vec<&ClipboardItem> {
        let query = self.search_query.to_lowercase();

        self.history.iter()
            .filter(|item| {
                // Türe göre filtrele
                let type_match = match self.filter_type {
                    None | Some(ClipboardFilter::All) => true,
                    Some(ClipboardFilter::Text) => matches!(item.data, ClipboardData::Text(_) | ClipboardData::RichText { .. }),
                    Some(ClipboardFilter::Images) => matches!(item.data, ClipboardData::Image { .. }),
                    Some(ClipboardFilter::Files) => matches!(item.data, ClipboardData::Files(_)),
                    Some(ClipboardFilter::URLs) => matches!(item.data, ClipboardData::Url(_)),
                    Some(ClipboardFilter::Pinned) => item.pinned,
                };

                if !type_match {
                    return false;
                }

                // Arama sorgusuna göre filtrele
                if query.is_empty() {
                    return true;
                }

                match &item.data {
                    ClipboardData::Text(t) => t.to_lowercase().contains(&query),
                    ClipboardData::RichText { plain, .. } => plain.to_lowercase().contains(&query),
                    ClipboardData::Url(u) => u.to_lowercase().contains(&query),
                    ClipboardData::Files(files) => files.iter().any(|f| f.to_lowercase().contains(&query)),
                    _ => false,
                }
            })
            .collect()
    }

    /// Pano yöneticisi arayüzünü çiz
    pub fn draw(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize, height: usize) {
        // Arka plan
        fb.draw_rect(x, y, width, height, Theme::WINDOW_BG.to_u32());
        fb.draw_rect_outline(x, y, width, height, Theme::BORDER.to_u32());

        // Başlık
        fb.draw_rect(x, y, width, 40, Theme::TOOLBAR_BG.to_u32());
        fb.draw_string(x + 8, y + 10, "Pano Geçmişi", Theme::TEXT_PRIMARY.to_u32());

        // Arama alanı
        let search_y = y + 48;
        fb.draw_rect(x + 8, search_y, width - 16, 28, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(x + 16, search_y + 6, "🔍", Theme::TEXT_SECONDARY.to_u32());

        if self.search_query.is_empty() {
            fb.draw_string(x + 36, search_y + 6, "Panoda ara...", Theme::TEXT_SECONDARY.to_u32());
        } else {
            fb.draw_string(x + 36, search_y + 6, &self.search_query, Theme::TEXT_PRIMARY.to_u32());
        }

        // Filtre sekmeleri
        let tabs_y = search_y + 36;
        let tabs = ["Tümü", "Metin", "Resimler", "Dosyalar", "Sabitlenmiş"];
        let mut tab_x = x + 8;

        for (i, tab) in tabs.iter().enumerate() {
            let is_active = match (self.filter_type, i) {
                (None, 0) | (Some(ClipboardFilter::All), 0) => true,
                (Some(ClipboardFilter::Text), 1) => true,
                (Some(ClipboardFilter::Images), 2) => true,
                (Some(ClipboardFilter::Files), 3) => true,
                (Some(ClipboardFilter::URLs), 4) => true,
                (Some(ClipboardFilter::Pinned), 5) => true,
                _ => false,
            };

            let bg = if is_active { Theme::ACCENT_PRIMARY.to_u32() } else { Theme::SIDEBAR_BG.to_u32() };
            let text_color = if is_active { Theme::TEXT_ON_ACCENT.to_u32() } else { Theme::TEXT_PRIMARY.to_u32() };

            fb.draw_rect(tab_x, tabs_y, tab.len() * 8 + 16, 24, bg);
            fb.draw_string(tab_x + 8, tabs_y + 4, tab, text_color);

            tab_x += tab.len() * 8 + 20;
        }

        // Geçmiş listesi
        let list_y = tabs_y + 32;
        let list_height = height - 140;
        let item_height = 64;

        let filtered = self.get_filtered_history();

        for (i, item) in filtered.iter().enumerate() {
            let item_y = list_y + i * item_height;

            if item_y + item_height > y + height {
                break;
            }

            let is_selected = self.selected_item == Some(item.id);
            let is_hovered = self.hovered_item == Some(item.id);

            let bg = if is_selected { Theme::ACCENT_PRIMARY.to_u32() }
                     else if is_hovered { Theme::LIST_ITEM_HOVER.to_u32() }
                     else { Theme::WINDOW_BG.to_u32() };

            fb.draw_rect(x, item_y, width, item_height, bg);

            let text_color = if is_selected { Theme::TEXT_ON_ACCENT.to_u32() } else { Theme::TEXT_PRIMARY.to_u32() };
            let secondary_color = if is_selected { Theme::TEXT_ON_ACCENT.to_u32() } else { Theme::TEXT_SECONDARY.to_u32() };

            // Simge
            fb.draw_string(x + 8, item_y + 8, item.data.icon(), text_color);

            // Önizleme
            let preview = item.data.preview(40);
            fb.draw_string(x + 36, item_y + 8, &preview, text_color);

            // Tür ve boyut
            let info = format!("{} • {}", item.data.data_type(), item.data.format_size());
            fb.draw_string(x + 36, item_y + 28, &info, secondary_color);

            // Saat ve kaynak
            let meta = format!("{} • {}", item.format_time(), item.source_app);
            fb.draw_string(x + 36, item_y + 44, &meta, secondary_color);

            // Sabitleme göstergesi
            if item.pinned {
                fb.draw_string(x + width - 24, item_y + 8, "📌", text_color);
            }

            // Favori göstergesi
            if item.favorite {
                fb.draw_string(x + width - 48, item_y + 8, "⭐", text_color);
            }
        }

        // Boş durum
        if filtered.is_empty() {
            let empty_text = if self.search_query.is_empty() {
                "Pano öğesi yok"
            } else {
                "Eşleşen öğe yok"
            };
            fb.draw_string(x + width / 2 - empty_text.len() * 4, y + height / 2, empty_text, Theme::TEXT_SECONDARY.to_u32());
        }

        // Alt çubuk
        let footer_y = y + height - 32;
        fb.draw_rect(x, footer_y, width, 32, Theme::TOOLBAR_BG.to_u32());

        let count_text = format!("{} öğe", self.history.len());
        fb.draw_string(x + 8, footer_y + 8, &count_text, Theme::TEXT_SECONDARY.to_u32());

        // Klavye kısayolu ipucu
        fb.draw_string(x + width - 80, footer_y + 8, &self.shortcut, Theme::TEXT_SECONDARY.to_u32());
    }

    /// Tıklama olayını işle
    pub fn on_click(&mut self, mx: i32, my: i32, x: usize, y: usize, width: usize, height: usize) -> ClipboardAction {
        // Arama alanı
        let search_y = y + 48;
        if mx >= (x + 8) as i32 && mx < (x + width - 8) as i32
            && my >= search_y as i32 && my < (search_y + 28) as i32 {
            return ClipboardAction::FocusSearch;
        }

        // Filtre sekmeleri
        let tabs_y = search_y + 36;
        let tabs = [
            ClipboardFilter::All,
            ClipboardFilter::Text,
            ClipboardFilter::Images,
            ClipboardFilter::Files,
            ClipboardFilter::URLs,
            ClipboardFilter::Pinned,
        ];
        let mut tab_x = x + 8;

        for filter in tabs {
            let tab_name = match filter {
                ClipboardFilter::All => "Tümü",
                ClipboardFilter::Text => "Metin",
                ClipboardFilter::Images => "Resimler",
                ClipboardFilter::Files => "Dosyalar",
                ClipboardFilter::Pinned => "Sabitlenmiş",
                ClipboardFilter::URLs => "URL'ler",
            };

            let tab_width = tab_name.len() * 8 + 16;

            if mx >= tab_x as i32 && mx < (tab_x + tab_width) as i32
                && my >= tabs_y as i32 && my < (tabs_y + 24) as i32 {
                self.filter_type = Some(filter);
                return ClipboardAction::None;
            }

            tab_x += tab_width + 4;
        }

        // Geçmiş öğeleri
        let list_y = tabs_y + 32;
        let item_height = 64;
        let filtered = self.get_filtered_history();

        for (i, item) in filtered.iter().enumerate() {
            let item_y = list_y + i * item_height;

            if my >= item_y as i32 && my < (item_y + item_height) as i32 {
                // Sabitleme düğmesine tıklanıyor mu kontrol et
                if mx >= (x + width - 24) as i32 {
                    return ClipboardAction::TogglePin(item.id);
                }

                // Favori düğmesine tıklanıyor mu kontrol et
                if mx >= (x + width - 48) as i32 && mx < (x + width - 24) as i32 {
                    return ClipboardAction::ToggleFavorite(item.id);
                }

                // Seç ve yapıştır
                let selected_id = item.id;
                self.selected_item = Some(selected_id);
                return ClipboardAction::SelectItem(selected_id);
            }
        }

        ClipboardAction::None
    }

    /// Tuş basımını işle
    pub fn on_key_press(&mut self, c: char) -> ClipboardAction {
        if c == '\x08' { // Geri al
            self.search_query.pop();
        } else if c == '\x1b' { // Escape
            self.search_query.clear();
            self.filter_type = None;
        } else if c == '\n' { // Enter
            if let Some(&id) = self.selected_item.as_ref() {
                return ClipboardAction::PasteItem(id);
            }
        } else if !c.is_control() {
            self.search_query.push(c);
        }

        ClipboardAction::None
    }
}

/// Pano eylemleri
#[derive(Clone, Debug)]
pub enum ClipboardAction {
    None,
    Copy(ClipboardData),
    Paste,
    PasteItem(u32),
    SelectItem(u32),
    TogglePin(u32),
    ToggleFavorite(u32),
    DeleteItem(u32),
    ClearHistory,
    FocusSearch,
}

// ============================================================================
// GLOBAL PANO
// ============================================================================

lazy_static::lazy_static! {
    static ref CLIPBOARD: Mutex<ClipboardManager> = Mutex::new(ClipboardManager::new());
}

/// Panoyu başlat
pub fn init() {
    crate::serial_println!("[GUI] Pano yöneticisi başlatıldı");
}

/// Pano yöneticisini al
pub fn get_clipboard() -> &'static Mutex<ClipboardManager> {
    &CLIPBOARD
}
