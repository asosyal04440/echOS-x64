//! # echOS Menu Widget'ları
//!
//! Açılır (dropdown) ve bağlam (sağ tık) menüleri için gerekli
//! `Menu`, `ContextMenu` ve `MenuItem` yapılarını içerir.
//!
//! ## Yapı Hiyerarşisi
//! - [`MenuItem`]   — tek bir menü kalemi (metin, kısayol, alt menü)
//! - [`MenuBar`]    — pencerenin üstündeki yatay menü çubuğu
//! - [`ContextMenu`] — farenin sağ tuşuyla açılan bağlam menüsü
//!
//! ## Tasarım Notu
//! Menü etkileşimi `Widget` trait'i üzerinden yönetilir:
//! `on_click` tıklamayı, `on_hover` fare hareketini işler.

use super::{
    border_rect_objects, draw_render_objects, solid_rect_object, text_render_object_with_width,
    Rect, Widget,
};
use crate::gop::framebuffer::Framebuffer;
use crate::gui::protocol::{DamageLane, RenderObject};
use crate::gui::theme::Theme;
use alloc::string::String;
use alloc::vec::Vec;

/// Menüde yer alan tek bir öğeyi temsil eder.
///
/// Bir menü kalemi; etiket metni, benzersiz bir kimlik (id),
/// isteğe bağlı klavye kısayolu ve alt menü içerebilir.
/// `separator: true` ise bu kalem görsel bir ayraç çizgisidir.
#[derive(Clone)]
pub struct MenuItem {
    pub text: String,
    pub id: u32,
    pub shortcut: String,
    pub enabled: bool,
    pub separator: bool,
    pub submenu: Option<Vec<MenuItem>>,
}

impl MenuItem {
    /// Temel bir menü kalemi oluşturur.
    /// `id`, seçim olayında hangi eylemin tetikleneceğini belirtir.
    pub fn new(id: u32, text: &str) -> Self {
        Self {
            text: String::from(text),
            id,
            shortcut: String::new(),
            enabled: true,
            separator: false,
            submenu: None,
        }
    }

    /// Builder kalıbıyla klavye kısayolu ekler.
    /// Örnek: `MenuItem::new(1, "Kaydet").with_shortcut("Ctrl+S")`
    pub fn with_shortcut(mut self, shortcut: &str) -> Self {
        self.shortcut = String::from(shortcut);
        self
    }

    /// Görsel ayraç çizgisi oluşturur.
    /// Metin veya id içermez; yalnızca menü bölümlerini ayırmak için kullanılır.
    pub fn separator() -> Self {
        Self {
            text: String::new(),
            id: 0,
            shortcut: String::new(),
            enabled: true,
            separator: true,
            submenu: None,
        }
    }

    /// Alt menü ekler; fare kalem üzerindeyken ">" oku gösterilir.
    pub fn with_submenu(mut self, items: Vec<MenuItem>) -> Self {
        self.submenu = Some(items);
        self
    }

    /// Kalemi devre dışı bırakır; gri gösterilir, tıklanamaz.
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }
}

/// Pencerenin üst kısmına yerleştirilen yatay menü çubuğu.
/// Her menü başlığına tıklandığında altında açılır liste görünür.
/// `open_menu` hangi menünün açık olduğunu, `hovered_menu` ise
/// farenin üzerinde durduğu menüyü tutar.
pub struct MenuBar {
    rect: Rect,
    menus: Vec<(String, Vec<MenuItem>)>,
    open_menu: Option<usize>,
    hovered_menu: Option<usize>,
    on_select: Option<fn(u32)>,
}

impl MenuBar {
    /// Yeni bir menü çubuğu oluşturur.
    /// Yükseklik sabit 28 piksel olarak ayarlanmıştır — standart başlık çubuğu yüksekliğiyle uyumludur.
    pub fn new(x: i32, y: i32, width: i32) -> Self {
        Self {
            rect: Rect::new(x, y, width, 28),
            menus: Vec::new(),
            open_menu: None,
            hovered_menu: None,
            on_select: None,
        }
    }

    /// Çubuğa yeni bir açılır menü ekler.
    /// `title` menü çubuğunda görünür; `items` açılır listeyi oluşturur.
    pub fn add_menu(&mut self, title: &str, items: Vec<MenuItem>) {
        self.menus.push((String::from(title), items));
    }

    /// Bir menü kalemi seçildiğinde çağrılacak işlevi ayarlar.
    /// Fonksiyon, seçilen kalemin `id` değerini parametre olarak alır.
    pub fn with_select_handler(mut self, handler: fn(u32)) -> Self {
        self.on_select = Some(handler);
        self
    }

    /// Verilen X koordinatının hangi menü başlığının üzerinde olduğunu hesaplar.
    /// Her menü başlığının genişliği; karakter sayısı × 8 piksel + 16 px kenar boşluğudur.
    fn menu_at(&self, x: i32) -> Option<usize> {
        let mut menu_x = self.rect.x + 5;
        for (i, (title, _)) in self.menus.iter().enumerate() {
            let menu_width = (title.len() * 8 + 16) as i32;
            if x >= menu_x && x < menu_x + menu_width {
                return Some(i);
            }
            menu_x += menu_width;
        }
        None
    }

    /// Belirtilen indeksteki menünün piksel genişliğini döndürür.
    fn menu_width(&self, index: usize) -> i32 {
        if index < self.menus.len() {
            (self.menus[index].0.len() * 8 + 16) as i32
        } else {
            0
        }
    }

    /// Belirtilen indeksteki menünün ekrandaki X başlangıç koordinatını hesaplar.
    /// Soldan sağa doğru önceki menülerin genişlikleri toplanır.
    fn menu_x(&self, index: usize) -> i32 {
        let mut x = self.rect.x + 5;
        for i in 0..index {
            x += self.menu_width(i);
        }
        x
    }

    fn render_bounds(&self) -> Rect {
        if let Some(menu_idx) = self.open_menu {
            let dropdown_x = self.menu_x(menu_idx);
            let dropdown_y = self.rect.y + self.rect.height;
            let dropdown_h = (self.menus[menu_idx].1.len() * 24) as i32;
            let left = self.rect.x.min(dropdown_x);
            let right = (self.rect.x + self.rect.width).max(dropdown_x + 200);
            let bottom = (self.rect.y + self.rect.height).max(dropdown_y + dropdown_h);
            Rect::new(left, self.rect.y, right - left, bottom - self.rect.y)
        } else {
            self.rect
        }
    }

    fn render_primitives(&self) -> Vec<RenderObject> {
        let mut objects = Vec::new();
        let base_id = ((self.rect.x as u64) << 32) ^ (self.rect.y as u64) ^ 0x1000;
        objects.push(solid_rect_object(
            base_id,
            self.rect,
            Theme::TITLEBAR_BG.to_u32(),
            DamageLane::Window,
            0,
        ));
        objects.push(solid_rect_object(
            base_id ^ 1,
            Rect::new(
                self.rect.x,
                self.rect.y + self.rect.height - 1,
                self.rect.width,
                1,
            ),
            Theme::BORDER.to_u32(),
            DamageLane::Window,
            1,
        ));

        let mut menu_x = self.rect.x + 5;
        for (i, (title, _)) in self.menus.iter().enumerate() {
            let menu_w = title.len() as i32 * 8 + 16;
            if self.open_menu == Some(i) || self.hovered_menu == Some(i) {
                objects.push(solid_rect_object(
                    base_id ^ 0x10 ^ i as u64,
                    Rect::new(menu_x, self.rect.y, menu_w, self.rect.height),
                    if self.open_menu == Some(i) {
                        Theme::ACCENT_PRIMARY.to_u32()
                    } else {
                        Theme::BUTTON_HOVER.to_u32()
                    },
                    DamageLane::Window,
                    1,
                ));
            }
            objects.push(text_render_object_with_width(
                base_id ^ 0x80 ^ i as u64,
                Rect::new(
                    menu_x + 8,
                    self.rect.y + ((self.rect.height - 16) / 2),
                    menu_w.max(1),
                    18,
                ),
                title,
                if self.open_menu == Some(i) {
                    Theme::DESKTOP_BG.to_u32()
                } else {
                    Theme::TEXT_PRIMARY.to_u32()
                },
                false,
                DamageLane::Text,
                2,
            ));
            menu_x += menu_w;
        }

        if let Some(menu_idx) = self.open_menu {
            let (_, items) = &self.menus[menu_idx];
            let dropdown_x = self.menu_x(menu_idx);
            let dropdown_y = self.rect.y + self.rect.height;
            let dropdown_rect = Rect::new(dropdown_x, dropdown_y, 200, (items.len() * 24) as i32);
            objects.push(solid_rect_object(
                base_id ^ 0x200,
                dropdown_rect,
                Theme::WINDOW_BG.to_u32(),
                DamageLane::Shell,
                3,
            ));
            objects.extend(border_rect_objects(
                base_id ^ 0x220,
                dropdown_rect,
                Theme::BORDER.to_u32(),
                DamageLane::Shell,
                4,
            ));
            for (i, item) in items.iter().enumerate() {
                let item_y = dropdown_y + i as i32 * 24;
                if item.separator {
                    objects.push(solid_rect_object(
                        base_id ^ 0x240 ^ i as u64,
                        Rect::new(dropdown_x + 5, item_y + 12, 190, 1),
                        Theme::BORDER.to_u32(),
                        DamageLane::Shell,
                        5,
                    ));
                } else {
                    objects.push(text_render_object_with_width(
                        base_id ^ 0x260 ^ i as u64,
                        Rect::new(dropdown_x + 8, item_y + 4, 184, 18),
                        &item.text,
                        if item.enabled {
                            Theme::TEXT_PRIMARY.to_u32()
                        } else {
                            Theme::TEXT_SECONDARY.to_u32()
                        },
                        false,
                        DamageLane::Text,
                        5,
                    ));
                    if !item.shortcut.is_empty() {
                        let shortcut_x = dropdown_x + 200 - item.shortcut.len() as i32 * 8 - 8;
                        objects.push(text_render_object_with_width(
                            base_id ^ 0x280 ^ i as u64,
                            Rect::new(
                                shortcut_x,
                                item_y + 4,
                                (200 - (shortcut_x - dropdown_x)).max(1),
                                18,
                            ),
                            &item.shortcut,
                            Theme::TEXT_SECONDARY.to_u32(),
                            false,
                            DamageLane::Text,
                            5,
                        ));
                    }
                    if item.submenu.is_some() {
                        objects.push(text_render_object_with_width(
                            base_id ^ 0x2A0 ^ i as u64,
                            Rect::new(dropdown_x + 184, item_y + 4, 8, 18),
                            ">",
                            Theme::TEXT_SECONDARY.to_u32(),
                            false,
                            DamageLane::Text,
                            5,
                        ));
                    }
                }
            }
        }

        objects
    }
}

impl Widget for MenuBar {
    /// Menü çubuğunu ve varsa açık olan açılır listeyi çizer.
    /// Arka plan ve alt kenarlık çizildikten sonra her menü başlığı
    /// sırayla yerleştirilir; açık ya da üzerine gelinen menü vurgulanır.
    fn draw(&self, fb: &mut Framebuffer) {
        draw_render_objects(fb, self.render_bounds(), &self.render_primitives());
    }

    /// Tıklama olayını işler.
    /// Önce menü başlık çubuğuna tıklanıp tıklanmadığı kontrol edilir;
    /// sonra açık bir dropdown varsa içindeki kaleme tıklanıp tıklanmadığına bakılır.
    fn on_click(&mut self, x: i32, y: i32) -> bool {
        // Menü başlığına tıklandı mı? Açıksa kapat, kapalıysa aç (toggle)
        if y >= self.rect.y && y < self.rect.y + self.rect.height {
            if let Some(menu_idx) = self.menu_at(x) {
                if self.open_menu == Some(menu_idx) {
                    self.open_menu = None;
                } else {
                    self.open_menu = Some(menu_idx);
                }
                return true;
            }
        }

        // Açık dropdown içinde bir kaleme tıklandı mı?
        if let Some(menu_idx) = self.open_menu {
            let dropdown_x = self.menu_x(menu_idx);
            let dropdown_y = self.rect.y + self.rect.height;
            let (_, items) = &self.menus[menu_idx];
            let item_height = 24;
            let dropdown_w = 200;

            if x >= dropdown_x && x < dropdown_x + dropdown_w {
                let relative_y = y - dropdown_y;
                if relative_y >= 0 {
                    let item_idx = (relative_y / item_height) as usize;
                    if item_idx < items.len() {
                        let item = &items[item_idx];
                        // Yalnızca etkin ve ayraç olmayan kalemlerde olay tetiklenir
                        if item.enabled && !item.separator {
                            if let Some(handler) = self.on_select {
                                handler(item.id);
                            }
                        }
                    }
                }
            }
            self.open_menu = None;
            return true;
        }

        // Menü dışına tıklandıysa tüm açık menüleri kapat
        self.open_menu = None;
        false
    }

    /// Fare hareketi olayını işler.
    /// Eğer başka bir menü açıkken farklı bir menü başlığının üzerine gelinirse,
    /// açık menü otomatik olarak yeni menüye geçer (hover-switch davranışı).
    fn on_hover(&mut self, x: i32, y: i32) -> bool {
        let old_hovered = self.hovered_menu;

        if y >= self.rect.y && y < self.rect.y + self.rect.height {
            self.hovered_menu = self.menu_at(x);

            // Menü açıkken farklı bir başlığın üzerine gelinirse otomatik geçiş yap
            if self.open_menu.is_some() && self.hovered_menu != old_hovered {
                self.open_menu = self.hovered_menu;
            }
        } else {
            self.hovered_menu = None;
        }

        old_hovered != self.hovered_menu
    }

    /// Widget sınırlarını döndürür.
    fn bounds(&self) -> Rect {
        self.rect
    }

    fn render_objects(&self) -> Vec<RenderObject> {
        self.render_primitives()
    }
}

/// Sağ tık menüsü (bağlam menüsü).
///
/// Kullanıcı sağ tıkladığında fare koordinatlarında açılır.
/// `show(x, y)` ile görünür hale gelir, dışına tıklandığında veya
/// bir kalem seçildiğinde `hide()` ile gizlenir.
pub struct ContextMenu {
    items: Vec<MenuItem>,
    visible: bool,
    x: i32,
    y: i32,
    width: i32,
    hovered_index: Option<usize>,
    on_select: Option<fn(u32)>,
}

impl ContextMenu {
    /// Boş, gizli bir bağlam menüsü oluşturur.
    /// Kalemler `add_item` ile eklenir; menü `show` ile açılır.
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            visible: false,
            x: 0,
            y: 0,
            width: 200,
            hovered_index: None,
            on_select: None,
        }
    }

    /// Menüye bir kalem ekler.
    pub fn add_item(&mut self, item: MenuItem) {
        self.items.push(item);
    }

    /// Menüyü belirtilen koordinatlarda görünür yapar.
    /// Genellikle `on_right_click` olayında çağrılır.
    pub fn show(&mut self, x: i32, y: i32) {
        self.x = x;
        self.y = y;
        self.visible = true;
    }

    /// Menüyü gizler ve vurgulama durumunu sıfırlar.
    pub fn hide(&mut self) {
        self.visible = false;
        self.hovered_index = None;
    }

    /// Menünün o an görünür olup olmadığını döndürür.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Kalem seçildiğinde çağrılacak işlevi ayarlar.
    pub fn with_select_handler(mut self, handler: fn(u32)) -> Self {
        self.on_select = Some(handler);
        self
    }

    /// Y koordinatına göre hangi kalemin üzerinde olunduğunu hesaplar.
    /// Her kalem 24 piksel yüksekliğindedir; `(y - self.y) / 24` formülü indeksi verir.
    fn item_at(&self, y: i32) -> Option<usize> {
        let relative_y = y - self.y;
        if relative_y >= 0 {
            let index = (relative_y / 24) as usize;
            if index < self.items.len() {
                return Some(index);
            }
        }
        None
    }

    fn render_primitives(&self) -> Vec<RenderObject> {
        if !self.visible {
            return Vec::new();
        }
        let mut objects = Vec::new();
        let bounds = self.bounds();
        let base_id = ((bounds.x as u64) << 32) ^ (bounds.y as u64) ^ 0x5000;
        objects.push(solid_rect_object(
            base_id,
            Rect::new(bounds.x + 4, bounds.y + 4, bounds.width, bounds.height),
            Theme::SHADOW.to_u32(),
            DamageLane::Shell,
            0,
        ));
        objects.push(solid_rect_object(
            base_id ^ 1,
            bounds,
            Theme::WINDOW_BG.to_u32(),
            DamageLane::Shell,
            1,
        ));
        objects.extend(border_rect_objects(
            base_id ^ 2,
            bounds,
            Theme::BORDER.to_u32(),
            DamageLane::Shell,
            2,
        ));

        for (i, item) in self.items.iter().enumerate() {
            let item_y = self.y + i as i32 * 24;
            if item.separator {
                objects.push(solid_rect_object(
                    base_id ^ 0x10 ^ i as u64,
                    Rect::new(self.x + 5, item_y + 12, self.width - 10, 1),
                    Theme::BORDER.to_u32(),
                    DamageLane::Shell,
                    3,
                ));
            } else {
                if self.hovered_index == Some(i) && item.enabled {
                    objects.push(solid_rect_object(
                        base_id ^ 0x20 ^ i as u64,
                        Rect::new(self.x + 1, item_y, self.width - 2, 24),
                        Theme::BUTTON_HOVER.to_u32(),
                        DamageLane::Shell,
                        3,
                    ));
                }
                objects.push(text_render_object_with_width(
                    base_id ^ 0x40 ^ i as u64,
                    Rect::new(self.x + 8, item_y + 4, self.width - 16, 18),
                    &item.text,
                    if item.enabled {
                        Theme::TEXT_PRIMARY.to_u32()
                    } else {
                        Theme::TEXT_SECONDARY.to_u32()
                    },
                    false,
                    DamageLane::Text,
                    4,
                ));
                if !item.shortcut.is_empty() {
                    let shortcut_x = self.x + self.width - item.shortcut.len() as i32 * 8 - 8;
                    objects.push(text_render_object_with_width(
                        base_id ^ 0x60 ^ i as u64,
                        Rect::new(
                            shortcut_x,
                            item_y + 4,
                            (self.width - (shortcut_x - self.x)).max(1),
                            18,
                        ),
                        &item.shortcut,
                        Theme::TEXT_SECONDARY.to_u32(),
                        false,
                        DamageLane::Text,
                        4,
                    ));
                }
            }
        }
        objects
    }
}

impl Default for ContextMenu {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for ContextMenu {
    /// Bağlam menüsünü çizer.
    /// Görünür değilse erken çıkılır (early return).
    /// Gölge, arka plan, kenarlık ve kalemler sırayla çizilir.
    fn draw(&self, fb: &mut Framebuffer) {
        if !self.visible {
            return;
        }
        draw_render_objects(fb, self.bounds(), &self.render_primitives());
    }

    /// Tıklama olayını işler.
    /// Menü dışına tıklandıysa gizle; menü içinde bir kaleme tıklandıysa
    /// kalem etkinse seçim işleyicisini çağır, sonra menüyü kapat.
    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if !self.visible {
            return false;
        }

        // Menü dışına tıklandığında kapat
        if x < self.x || x >= self.x + self.width || y < self.y {
            self.hide();
            return false;
        }

        // Kaleme tıklandığında seçim işleyicisini tetikle
        if let Some(index) = self.item_at(y) {
            let item = &self.items[index];
            if item.enabled && !item.separator {
                if let Some(handler) = self.on_select {
                    handler(item.id);
                }
            }
        }
        self.hide();
        true
    }

    /// Fare hareketi olayını işler.
    /// Menü sınırları içindeyse hangi kalem üzerinde olunduğu güncellenir.
    fn on_hover(&mut self, x: i32, y: i32) -> bool {
        if !self.visible {
            return false;
        }

        let old_hovered = self.hovered_index;
        if x >= self.x && x < self.x + self.width && y >= self.y {
            self.hovered_index = self.item_at(y);
        } else {
            self.hovered_index = None;
        }
        old_hovered != self.hovered_index
    }

    /// Widget sınırlarını hesaplar.
    /// Yükseklik, kalem sayısı × 24 piksel formülüyle dinamik olarak belirlenir.
    fn bounds(&self) -> Rect {
        Rect::new(self.x, self.y, self.width, (self.items.len() * 24) as i32)
    }

    fn render_objects(&self) -> Vec<RenderObject> {
        self.render_primitives()
    }
}
