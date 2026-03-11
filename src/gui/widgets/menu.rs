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

use super::{Rect, Widget};
use crate::gop::framebuffer::Framebuffer;
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
}

impl Widget for MenuBar {
    /// Menü çubuğunu ve varsa açık olan açılır listeyi çizer.
    /// Arka plan ve alt kenarlık çizildikten sonra her menü başlığı
    /// sırayla yerleştirilir; açık ya da üzerine gelinen menü vurgulanır.
    fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let w = self.rect.width as usize;
        let h = self.rect.height as usize;

        // Menü çubuğunun arka plan rengi
        fb.draw_rect(x, y, w, h, Theme::TITLEBAR_BG.to_u32());

        // Alt kenarlık çizgisi — menü çubuğunu içerik alanından ayırır
        for col in x..(x + w) {
            fb.plot_pixel(col, y + h - 1, Theme::BORDER.to_u32());
        }

        // Menü başlıklarını sırayla çiz
        let mut menu_x = x + 5;
        for (i, (title, _)) in self.menus.iter().enumerate() {
            let menu_w = title.len() * 8 + 16;

            // Açık menü aksent renginde, sadece üzerine gelinen menü hover renginde gösterilir
            if self.open_menu == Some(i) {
                fb.draw_rect(menu_x, y, menu_w, h, Theme::ACCENT_PRIMARY.to_u32());
            } else if self.hovered_menu == Some(i) {
                fb.draw_rect(menu_x, y, menu_w, h, Theme::BUTTON_HOVER.to_u32());
            }

            // Başlık metni — açık menünün metni ters renkte (okunabilirlik için)
            let text_x = menu_x + 8;
            let text_y = y + (h - 16) / 2;
            let text_color = if self.open_menu == Some(i) {
                Theme::DESKTOP_BG.to_u32()
            } else {
                Theme::TEXT_PRIMARY.to_u32()
            };
            fb.draw_string(text_x, text_y, title, text_color);

            menu_x += menu_w;
        }

        // Açık menünün açılır listesini çiz
        if let Some(menu_idx) = self.open_menu {
            let (_, items) = &self.menus[menu_idx];
            let dropdown_x = self.menu_x(menu_idx) as usize;
            let dropdown_y = y + h;
            let item_height = 24;
            let dropdown_w = 200;
            let dropdown_h = items.len() * item_height;

            // Açılır listenin arka planı
            fb.draw_rect(
                dropdown_x,
                dropdown_y,
                dropdown_w,
                dropdown_h,
                Theme::WINDOW_BG.to_u32(),
            );

            // Dört kenar kenarlık çizgisi — piksel piksel üst/alt ve sol/sağ kenarlar
            for col in dropdown_x..(dropdown_x + dropdown_w) {
                fb.plot_pixel(col, dropdown_y, Theme::BORDER.to_u32());
                fb.plot_pixel(col, dropdown_y + dropdown_h - 1, Theme::BORDER.to_u32());
            }
            for row in dropdown_y..(dropdown_y + dropdown_h) {
                fb.plot_pixel(dropdown_x, row, Theme::BORDER.to_u32());
                fb.plot_pixel(dropdown_x + dropdown_w - 1, row, Theme::BORDER.to_u32());
            }

            // Menü kalemlerini listele
            for (i, item) in items.iter().enumerate() {
                let item_y = dropdown_y + i * item_height;

                if item.separator {
                    // Ayraç çizgisi: soldan ve sağdan 5 piksel içeriden çizilir
                    for col in (dropdown_x + 5)..(dropdown_x + dropdown_w - 5) {
                        fb.plot_pixel(col, item_y + item_height / 2, Theme::BORDER.to_u32());
                    }
                } else {
                    // Kalem metni — devre dışı kalemler soluk renkte gösterilir
                    let text_color = if item.enabled {
                        Theme::TEXT_PRIMARY.to_u32()
                    } else {
                        Theme::TEXT_SECONDARY.to_u32()
                    };
                    fb.draw_string(dropdown_x + 8, item_y + 4, &item.text, text_color);

                    // Klavye kısayolu — sağa hizalı olarak listenin sonuna yazılır
                    if !item.shortcut.is_empty() {
                        let shortcut_x = dropdown_x + dropdown_w - item.shortcut.len() * 8 - 8;
                        fb.draw_string(
                            shortcut_x,
                            item_y + 4,
                            &item.shortcut,
                            Theme::TEXT_SECONDARY.to_u32(),
                        );
                    }

                    // Alt menü oku — sağ tarafta ">" karakteriyle gösterilir
                    if item.submenu.is_some() {
                        fb.draw_string(
                            dropdown_x + dropdown_w - 16,
                            item_y + 4,
                            ">",
                            Theme::TEXT_SECONDARY.to_u32(),
                        );
                    }
                }
            }
        }
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

        let x = self.x as usize;
        let y = self.y as usize;
        let w = self.width as usize;
        let h = self.items.len() * 24;
        let item_height = 24usize;

        // Gölge efekti — menüyü 4 piksel sağa/aşağı kaydırılmış koyu dikdörtgen
        fb.draw_rect(x + 4, y + 4, w, h, Theme::SHADOW.to_u32());

        // Menü arka planı
        fb.draw_rect(x, y, w, h, Theme::WINDOW_BG.to_u32());

        // Dört kenar kenarlık çizgisi
        for col in x..(x + w) {
            fb.plot_pixel(col, y, Theme::BORDER.to_u32());
            fb.plot_pixel(col, y + h - 1, Theme::BORDER.to_u32());
        }
        for row in y..(y + h) {
            fb.plot_pixel(x, row, Theme::BORDER.to_u32());
            fb.plot_pixel(x + w - 1, row, Theme::BORDER.to_u32());
        }

        // Kalemleri çiz
        for (i, item) in self.items.iter().enumerate() {
            let item_y = y + i * item_height;

            if item.separator {
                // Ayraç çizgisi — kalemin orta yüksekliğinde yatay çizgi
                for col in (x + 5)..(x + w - 5) {
                    fb.plot_pixel(col, item_y + item_height / 2, Theme::BORDER.to_u32());
                }
            } else {
                // Fare üzerindeyse ve kalem etkinse hover vurgusu uygula
                if self.hovered_index == Some(i) && item.enabled {
                    fb.draw_rect(
                        x + 1,
                        item_y,
                        w - 2,
                        item_height,
                        Theme::BUTTON_HOVER.to_u32(),
                    );
                }

                // Kalem metni — devre dışı olanlar soluk renkte
                let text_color = if item.enabled {
                    Theme::TEXT_PRIMARY.to_u32()
                } else {
                    Theme::TEXT_SECONDARY.to_u32()
                };
                fb.draw_string(x + 8, item_y + 4, &item.text, text_color);

                // Klavye kısayolu — sağa hizalı
                if !item.shortcut.is_empty() {
                    let shortcut_x = x + w - item.shortcut.len() * 8 - 8;
                    fb.draw_string(
                        shortcut_x,
                        item_y + 4,
                        &item.shortcut,
                        Theme::TEXT_SECONDARY.to_u32(),
                    );
                }
            }
        }
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
}
