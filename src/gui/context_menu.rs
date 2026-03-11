//! # Context Menu (Sağ Tık Menüsü)
//!
//! Floating Z-order katmanında görüntülenen bağlam menüsü sistemi.
//! Masaüstü, dosya gezgini ve uygulamalar için ortak altyapı sağlar.
//!
//! ## Kullanım
//!
//! ```rust,ignore
//! use crate::gui::context_menu::{ContextMenu, MenuItem};
//!
//! let menu = ContextMenu::new()
//!     .add_item("Aç", MenuAction::Open)
//!     .add_item("Kopyala", MenuAction::Copy)
//!     .add_separator()
//!     .add_item("Sil", MenuAction::Delete);
//!
//! menu.show(mouse_x, mouse_y);
//! ```

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::Theme;
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ============================================================================
// SABİTLER
// ============================================================================

/// Menü öğesi yüksekliği
pub const ITEM_HEIGHT: i32 = 24;
/// Ayırıcı yüksekliği
pub const SEPARATOR_HEIGHT: i32 = 8;
/// Menü minimum genişliği
pub const MIN_WIDTH: i32 = 150;
/// Menü dolgusu (padding)
pub const PADDING: i32 = 4;
/// Simge alanı genişliği
pub const ICON_WIDTH: i32 = 24;
/// Alt menü ok genişliği
pub const ARROW_WIDTH: i32 = 16;

// ============================================================================
// MENÜ EYLEM TİPLERİ
// ============================================================================

/// Menü eylem türleri
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuAction {
    /// Özel eylem kimliği
    Custom(u32),
    /// Aç
    Open,
    /// Yeni pencerede aç
    OpenInNewWindow,
    /// Bilgi göster
    GetInfo,
    /// Kopyala
    Copy,
    /// Kes
    Cut,
    /// Yapıştır
    Paste,
    /// Sil
    Delete,
    /// Yeniden adlandır
    Rename,
    /// Çöp kutusuna taşı
    MoveToTrash,
    /// Yeni klasör
    NewFolder,
    /// Yeni dosya
    NewFile,
    /// Görünümü değiştir
    ChangeView,
    /// Sırala
    SortBy(SortOption),
    /// Yenile
    Refresh,
    /// Seçili tümünü seç
    SelectAll,
    /// Seçimi kaldır
    DeselectAll,
    /// Alt menü aç
    Submenu(String),
    /// İptal
    Cancel,
}

/// Sıralama seçenekleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortOption {
    Name,
    Size,
    Date,
    Type,
}

// ============================================================================
// MENÜ ÖĞESİ
// ============================================================================

/// Menü öğesi türü
#[derive(Clone, Debug)]
pub enum MenuItemType {
    /// Normal öğe
    Action(MenuAction),
    /// Ayırıcı çizgi
    Separator,
    /// Alt menü
    Submenu(Vec<MenuItem>),
}

/// Tek bir menü öğesi
#[derive(Clone, Debug)]
pub struct MenuItem {
    /// Öğe metni
    pub label: String,
    /// Öğe türü
    pub item_type: MenuItemType,
    /// Kısayol tuşu (görüntüleme için)
    pub shortcut: Option<String>,
    /// Simge karakteri
    pub icon: Option<char>,
    /// Devre dışı mı?
    pub disabled: bool,
    /// İşaretli mi? (checkbox)
    pub checked: bool,
}

impl MenuItem {
    /// Eylemli öğe oluştur
    pub fn action(label: &str, action: MenuAction) -> Self {
        MenuItem {
            label: label.to_string(),
            item_type: MenuItemType::Action(action),
            shortcut: None,
            icon: None,
            disabled: false,
            checked: false,
        }
    }

    /// Ayırıcı oluştur
    pub fn separator() -> Self {
        MenuItem {
            label: String::new(),
            item_type: MenuItemType::Separator,
            shortcut: None,
            icon: None,
            disabled: false,
            checked: false,
        }
    }

    /// Alt menü oluştur
    pub fn submenu(label: &str, items: Vec<MenuItem>) -> Self {
        MenuItem {
            label: label.to_string(),
            item_type: MenuItemType::Submenu(items),
            shortcut: None,
            icon: None,
            disabled: false,
            checked: false,
        }
    }

    /// Kısayol ekle
    pub fn with_shortcut(mut self, shortcut: &str) -> Self {
        self.shortcut = Some(shortcut.to_string());
        self
    }

    /// Simge ekle
    pub fn with_icon(mut self, icon: char) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Devre dışı yap
    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }

    /// İşaretli yap
    pub fn checked(mut self) -> Self {
        self.checked = true;
        self
    }

    /// Öğe yüksekliği
    pub fn height(&self) -> i32 {
        match self.item_type {
            MenuItemType::Separator => SEPARATOR_HEIGHT,
            _ => ITEM_HEIGHT,
        }
    }
}

// ============================================================================
// CONTEXT MENU
// ============================================================================

/// Bağlam menüsü
pub struct ContextMenu {
    /// Menü öğeleri
    pub items: Vec<MenuItem>,
    /// Menü konumu X
    pub x: i32,
    /// Menü konumu Y
    pub y: i32,
    /// Menü genişliği
    pub width: i32,
    /// Menü yüksekliği
    pub height: i32,
    /// Görünür mü?
    pub visible: bool,
    /// Hover edilen öğe indeksi
    pub hover_index: Option<usize>,
    /// Seçilen eylem
    pub selected_action: Option<MenuAction>,
    /// Aktif alt menü
    pub active_submenu: Option<Box<ContextMenu>>,
    /// Alt menü parent indeksi
    pub submenu_parent_idx: Option<usize>,
}

impl ContextMenu {
    /// Yeni boş menü oluştur
    pub fn new() -> Self {
        ContextMenu {
            items: Vec::new(),
            x: 0,
            y: 0,
            width: MIN_WIDTH,
            height: 0,
            visible: false,
            hover_index: None,
            selected_action: None,
            active_submenu: None,
            submenu_parent_idx: None,
        }
    }

    /// Öğe ekle
    pub fn add_item(mut self, label: &str, action: MenuAction) -> Self {
        self.items.push(MenuItem::action(label, action));
        self.recalculate_size();
        self
    }

    /// Ayırıcı ekle
    pub fn add_separator(mut self) -> Self {
        self.items.push(MenuItem::separator());
        self.recalculate_size();
        self
    }

    /// Alt menü ekle
    pub fn add_submenu(mut self, label: &str, items: Vec<MenuItem>) -> Self {
        self.items.push(MenuItem::submenu(label, items));
        self.recalculate_size();
        self
    }

    /// Öğe ekle (mutable)
    pub fn push_item(&mut self, item: MenuItem) {
        self.items.push(item);
        self.recalculate_size();
    }

    /// Boyutu yeniden hesapla
    fn recalculate_size(&mut self) {
        // Yükseklik
        self.height = PADDING * 2;
        for item in &self.items {
            self.height += item.height();
        }

        // Genişlik (en uzun metin + shortcut)
        let mut max_width = MIN_WIDTH;
        for item in &self.items {
            let label_width = (item.label.len() as i32) * 8 + ICON_WIDTH + PADDING * 2;
            let shortcut_width = item
                .shortcut
                .as_ref()
                .map(|s| (s.len() as i32) * 8 + 16)
                .unwrap_or(0);
            let submenu_arrow = match item.item_type {
                MenuItemType::Submenu(_) => ARROW_WIDTH,
                _ => 0,
            };
            let total = label_width + shortcut_width + submenu_arrow;
            if total > max_width {
                max_width = total;
            }
        }
        self.width = max_width;
    }

    /// Menüyü göster
    pub fn show(&mut self, x: i32, y: i32) {
        self.x = x;
        self.y = y;
        self.visible = true;
        self.hover_index = None;
        self.selected_action = None;
        self.active_submenu = None;
    }

    /// Menüyü gizle
    pub fn hide(&mut self) {
        self.visible = false;
        self.hover_index = None;
        self.active_submenu = None;
    }

    /// Fare hareketi
    pub fn on_mouse_move(&mut self, mx: i32, my: i32) {
        if !self.visible {
            return;
        }

        // Alt menü kontrolü
        if let Some(ref mut submenu) = self.active_submenu {
            if mx >= submenu.x
                && mx < submenu.x + submenu.width
                && my >= submenu.y
                && my < submenu.y + submenu.height
            {
                submenu.on_mouse_move(mx, my);
                return;
            }
        }

        // Ana menü hover
        let rel_x = mx - self.x;
        let rel_y = my - self.y - PADDING;

        if rel_x < 0 || rel_x >= self.width || rel_y < 0 {
            self.hover_index = None;
            return;
        }

        let mut y_offset = 0;
        for (i, item) in self.items.iter().enumerate() {
            let item_height = item.height();
            if rel_y >= y_offset && rel_y < y_offset + item_height {
                if !item.disabled && !matches!(item.item_type, MenuItemType::Separator) {
                    self.hover_index = Some(i);

                    // Alt menü açma
                    if let MenuItemType::Submenu(ref sub_items) = item.item_type {
                        self.open_submenu(i, sub_items.clone());
                    } else {
                        self.close_submenu();
                    }
                } else {
                    self.hover_index = None;
                }
                return;
            }
            y_offset += item_height;
        }

        self.hover_index = None;
    }

    /// Alt menü aç
    fn open_submenu(&mut self, parent_idx: usize, items: Vec<MenuItem>) {
        if self.submenu_parent_idx == Some(parent_idx) {
            return; // Zaten açık
        }

        let mut submenu = ContextMenu::new();
        submenu.items = items;
        submenu.recalculate_size();

        // Alt menü konumunu hesapla
        let mut y_offset = PADDING;
        for (i, item) in self.items.iter().enumerate() {
            if i == parent_idx {
                break;
            }
            y_offset += item.height();
        }

        submenu.x = self.x + self.width - 4;
        submenu.y = self.y + y_offset;
        submenu.visible = true;

        self.active_submenu = Some(Box::new(submenu));
        self.submenu_parent_idx = Some(parent_idx);
    }

    /// Alt menü kapat
    fn close_submenu(&mut self) {
        self.active_submenu = None;
        self.submenu_parent_idx = None;
    }

    /// Fare tıklaması
    pub fn on_click(&mut self, mx: i32, my: i32) -> Option<MenuAction> {
        if !self.visible {
            return None;
        }

        // Alt menü tıklaması
        if let Some(ref mut submenu) = self.active_submenu {
            if mx >= submenu.x
                && mx < submenu.x + submenu.width
                && my >= submenu.y
                && my < submenu.y + submenu.height
            {
                let result = submenu.on_click(mx, my);
                if result.is_some() {
                    self.hide();
                }
                return result;
            }
        }

        // Ana menü tıklaması
        if let Some(idx) = self.hover_index {
            if let Some(item) = self.items.get(idx) {
                if !item.disabled {
                    if let MenuItemType::Action(ref action) = item.item_type {
                        let result = action.clone();
                        self.selected_action = Some(result.clone());
                        self.hide();
                        return Some(result);
                    }
                }
            }
        }

        // Menü dışı tıklama - kapat
        if mx < self.x || mx >= self.x + self.width || my < self.y || my >= self.y + self.height {
            self.hide();
            return Some(MenuAction::Cancel);
        }

        None
    }

    /// Menü çizimi
    pub fn draw(&self, fb: &mut Framebuffer) {
        if !self.visible {
            return;
        }

        // Gölge
        Self::fill_rect(
            fb,
            self.x + 3,
            self.y + 3,
            self.width,
            self.height,
            0x20000000,
        );

        // Arka plan
        Self::fill_rect(
            fb,
            self.x,
            self.y,
            self.width,
            self.height,
            Theme::WINDOW_BG.to_argb(),
        );

        // Kenarlık
        Self::draw_rect(
            fb,
            self.x,
            self.y,
            self.width,
            self.height,
            Theme::BORDER.to_argb(),
        );

        // Öğeler
        let mut y_offset = self.y + PADDING;
        for (i, item) in self.items.iter().enumerate() {
            match item.item_type {
                MenuItemType::Separator => {
                    // Ayırıcı çizgi
                    let sep_y = y_offset + SEPARATOR_HEIGHT / 2;
                    Self::fill_rect(
                        fb,
                        self.x + 8,
                        sep_y,
                        self.width - 16,
                        1,
                        Theme::BORDER.to_argb(),
                    );
                }
                _ => {
                    // Hover arka planı
                    if self.hover_index == Some(i) {
                        Self::fill_rect(
                            fb,
                            self.x + 2,
                            y_offset,
                            self.width - 4,
                            ITEM_HEIGHT,
                            Theme::SELECTION_BG.to_argb(),
                        );
                    }

                    // Metin rengi
                    let text_color = if item.disabled {
                        Theme::TEXT_DISABLED.to_argb()
                    } else {
                        Theme::TEXT_PRIMARY.to_argb()
                    };

                    // Simge
                    if let Some(icon) = item.icon {
                        self.draw_char(fb, self.x + 6, y_offset + 4, icon, text_color);
                    }

                    // İşaret (checkmark)
                    if item.checked {
                        self.draw_char(fb, self.x + 6, y_offset + 4, '✓', text_color);
                    }

                    // Metin
                    self.draw_text(
                        fb,
                        self.x + ICON_WIDTH,
                        y_offset + 4,
                        &item.label,
                        text_color,
                    );

                    // Kısayol
                    if let Some(ref shortcut) = item.shortcut {
                        let shortcut_x = self.x + self.width - (shortcut.len() as i32) * 8 - 12;
                        self.draw_text(
                            fb,
                            shortcut_x,
                            y_offset + 4,
                            shortcut,
                            Theme::TEXT_SECONDARY.to_argb(),
                        );
                    }

                    // Alt menü oku
                    if matches!(item.item_type, MenuItemType::Submenu(_)) {
                        let arrow_x = self.x + self.width - 14;
                        self.draw_char(fb, arrow_x, y_offset + 4, '▶', text_color);
                    }
                }
            }

            y_offset += item.height();
        }

        // Alt menü çiz
        if let Some(ref submenu) = self.active_submenu {
            submenu.draw(fb);
        }
    }

    /// Dikdörtgen doldur
    fn fill_rect(fb: &mut Framebuffer, x: i32, y: i32, w: i32, h: i32, color: u32) {
        let fb_w = fb.width as i32;
        let fb_h = fb.height as i32;
        let stride = fb.pixels_per_scan_line;

        let x0 = x.max(0) as usize;
        let y0 = y.max(0) as usize;
        let x1 = (x + w).min(fb_w) as usize;
        let y1 = (y + h).min(fb_h) as usize;

        let buffer = fb.buffer_mut();
        for row in y0..y1 {
            let start = row * stride + x0;
            let end = row * stride + x1;
            if end <= buffer.len() {
                for pixel in &mut buffer[start..end] {
                    *pixel = color;
                }
            }
        }
    }

    /// Dikdörtgen kenarlık çiz
    fn draw_rect(fb: &mut Framebuffer, x: i32, y: i32, w: i32, h: i32, color: u32) {
        // Üst
        Self::fill_rect(fb, x, y, w, 1, color);
        // Alt
        Self::fill_rect(fb, x, y + h - 1, w, 1, color);
        // Sol
        Self::fill_rect(fb, x, y, 1, h, color);
        // Sağ
        Self::fill_rect(fb, x + w - 1, y, 1, h, color);
    }

    /// Metin çiz
    fn draw_text(&self, fb: &mut Framebuffer, x: i32, y: i32, text: &str, color: u32) {
        let mut cursor_x = x;
        for c in text.chars() {
            self.draw_char(fb, cursor_x, y, c, color);
            cursor_x += 8;
        }
    }

    /// Karakter çiz
    fn draw_char(&self, fb: &mut Framebuffer, x: i32, y: i32, c: char, color: u32) {
        let glyph = crate::font::vga_font::get_font_data(c);
        let fb_w = fb.width as i32;
        let fb_h = fb.height as i32;
        let stride = fb.pixels_per_scan_line;
        let buffer = fb.buffer_mut();

        for row in 0..16 {
            let byte = glyph[row];
            for col in 0..8 {
                if (byte >> (7 - col)) & 1 == 1 {
                    let px = x + col;
                    let py = y + row as i32;
                    if px >= 0 && px < fb_w && py >= 0 && py < fb_h {
                        let offset = (py as usize) * stride + (px as usize);
                        if offset < buffer.len() {
                            buffer[offset] = color;
                        }
                    }
                }
            }
        }
    }

    /// Seçilen eylemi al ve sıfırla
    pub fn take_action(&mut self) -> Option<MenuAction> {
        self.selected_action.take()
    }
}

// ============================================================================
// HAZIR MENÜLER
// ============================================================================

/// Masaüstü bağlam menüsü
pub fn desktop_context_menu() -> ContextMenu {
    ContextMenu::new()
        .add_item("Yeni Klasör", MenuAction::NewFolder)
        .add_item("Yeni Dosya", MenuAction::NewFile)
        .add_separator()
        .add_submenu(
            "Görünüm",
            vec![
                MenuItem::action("Simgeler", MenuAction::ChangeView),
                MenuItem::action("Liste", MenuAction::ChangeView),
                MenuItem::action("Sütunlar", MenuAction::ChangeView),
            ],
        )
        .add_submenu(
            "Sırala",
            vec![
                MenuItem::action("Ada Göre", MenuAction::SortBy(SortOption::Name)),
                MenuItem::action("Boyuta Göre", MenuAction::SortBy(SortOption::Size)),
                MenuItem::action("Tarihe Göre", MenuAction::SortBy(SortOption::Date)),
                MenuItem::action("Türe Göre", MenuAction::SortBy(SortOption::Type)),
            ],
        )
        .add_separator()
        .add_item("Yapıştır", MenuAction::Paste)
        .add_separator()
        .add_item("Yenile", MenuAction::Refresh)
}

/// Dosya bağlam menüsü
pub fn file_context_menu() -> ContextMenu {
    ContextMenu::new()
        .add_item("Aç", MenuAction::Open)
        .add_item("Yeni Pencerede Aç", MenuAction::OpenInNewWindow)
        .add_separator()
        .add_item("Bilgi Al", MenuAction::GetInfo)
        .add_separator()
        .add_item("Kopyala", MenuAction::Copy)
        .add_item("Kes", MenuAction::Cut)
        .add_item("Yeniden Adlandır", MenuAction::Rename)
        .add_separator()
        .add_item("Çöp Kutusuna Taşı", MenuAction::MoveToTrash)
}

/// Klasör bağlam menüsü
pub fn folder_context_menu() -> ContextMenu {
    ContextMenu::new()
        .add_item("Aç", MenuAction::Open)
        .add_item("Yeni Pencerede Aç", MenuAction::OpenInNewWindow)
        .add_separator()
        .add_item("Bilgi Al", MenuAction::GetInfo)
        .add_separator()
        .add_item("Kopyala", MenuAction::Copy)
        .add_item("Kes", MenuAction::Cut)
        .add_item("Yeniden Adlandır", MenuAction::Rename)
        .add_separator()
        .add_item("Yeni Klasör", MenuAction::NewFolder)
        .add_item("Yeni Dosya", MenuAction::NewFile)
        .add_separator()
        .add_item("Çöp Kutusuna Taşı", MenuAction::MoveToTrash)
}

// ============================================================================
// GLOBAL CONTEXT MENU
// ============================================================================

use lazy_static::lazy_static;
use spin::Mutex;

lazy_static! {
    /// Global bağlam menüsü
    pub static ref CONTEXT_MENU: Mutex<ContextMenu> = Mutex::new(ContextMenu::new());
}

/// Masaüstü bağlam menüsünü göster
pub fn show_desktop_menu(x: i32, y: i32) {
    let mut menu = CONTEXT_MENU.lock();
    *menu = desktop_context_menu();
    menu.show(x, y);
}

/// Dosya bağlam menüsünü göster
pub fn show_file_menu(x: i32, y: i32) {
    let mut menu = CONTEXT_MENU.lock();
    *menu = file_context_menu();
    menu.show(x, y);
}

/// Klasör bağlam menüsünü göster
pub fn show_folder_menu(x: i32, y: i32) {
    let mut menu = CONTEXT_MENU.lock();
    *menu = folder_context_menu();
    menu.show(x, y);
}

/// Bağlam menüsünü çiz
pub fn draw_context_menu(fb: &mut Framebuffer) {
    CONTEXT_MENU.lock().draw(fb);
}

/// Bağlam menüsü fare tıklaması
pub fn context_menu_click(x: i32, y: i32) -> Option<MenuAction> {
    CONTEXT_MENU.lock().on_click(x, y)
}

/// Bağlam menüsü fare hareketi
pub fn context_menu_move(x: i32, y: i32) {
    CONTEXT_MENU.lock().on_mouse_move(x, y);
}

/// Bağlam menüsü görünür mü?
pub fn is_context_menu_visible() -> bool {
    CONTEXT_MENU.lock().visible
}

/// Bağlam menüsünü gizle
pub fn hide_context_menu() {
    CONTEXT_MENU.lock().hide();
}
