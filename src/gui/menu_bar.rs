//! # Global Menü Çubuğu
//!
//! macOS tarzı global menü çubuğu; ekranın üst kısmında gösterilir.
//! Uygulama menüsü, Dosya, Düzenle, Görünüm vb. menüleri içerir.
//!
//! ## Mimari
//! - `MenuItem`: Etiket, eylem, klavye kısayolu, etkinlik durumu ve onay kutusu içeren menü öğesi
//! - `Menu`: Başlık, öğe listesi ve açık/kapalı durumu yöneten açılır menü
//! - `MenuBar`: Tüm menüleri ve sağ taraf durum simgelerini yöneten genel menü çubuğu
//!
//! ## Çizim Algoritması
//! Menü çubuğu `MENU_BAR_HEIGHT` yüksekliğinde `TITLEBAR_BG` rengiyle çizilir.
//! Açık menüler `MENU_BAR_HEIGHT` altına açılır; her öğe `MENU_ITEM_HEIGHT` piksel yüksekliğindedir.
//! Sağ taraftaki öğeler `screen_width`'ten geriye doğru sıralanır.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};

// ============================================================================
// MENÜ ÇUBUĞU SABİTLERİ
// ============================================================================

/// Menü çubuğu yüksekliği (piksel)
pub const MENU_BAR_HEIGHT: usize = 25;

/// Menü öğesi yüksekliği (piksel)
pub const MENU_ITEM_HEIGHT: usize = 22;

/// Menü iç boşluğu (piksel)
pub const MENU_PADDING: usize = 8;

// ============================================================================
// MENÜ ÖĞESİ
// ============================================================================

/// Bir menü öğesi
#[derive(Clone, Debug)]
pub struct MenuItem {
    /// Görüntü metni
    pub label: String,
    /// Eylem
    pub action: MenuAction,
    /// Klavye kısayolu
    pub shortcut: String,
    /// Etkin mi
    pub enabled: bool,
    /// Ayırıcı mı
    pub separator: bool,
    /// Alt menüsü var mı
    pub has_submenu: bool,
    /// Alt menü öğeleri
    pub submenu: Vec<MenuItem>,
    /// Onay kutusu durumu (None = onay kutusu yok, Some(true) = işaretli, Some(false) = işaretsiz)
    pub checked: Option<bool>,
    /// Simge (isteğe bağlı)
    pub icon: Option<String>,
}

#[derive(Clone, Debug)]
pub enum MenuAction {
    None,
    NewFile,
    OpenFile,
    SaveFile,
    SaveAs,
    Close,
    Quit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Find,
    Replace,
    Preferences,
    About,
    HideApp,
    HideOthers,
    ShowAll,
    Zoom,
    Minimize,
    FullScreen,
    Custom(String),
}

impl MenuItem {
    pub fn new(label: &str) -> Self {
        MenuItem {
            label: String::from(label),
            action: MenuAction::None,
            shortcut: String::new(),
            enabled: true,
            separator: false,
            has_submenu: false,
            submenu: Vec::new(),
            checked: None,
            icon: None,
        }
    }

    pub fn action(label: &str, action: MenuAction) -> Self {
        MenuItem {
            label: String::from(label),
            action,
            shortcut: String::new(),
            enabled: true,
            separator: false,
            has_submenu: false,
            submenu: Vec::new(),
            checked: None,
            icon: None,
        }
    }

    pub fn shortcut(label: &str, action: MenuAction, shortcut: &str) -> Self {
        let mut item = Self::action(label, action);
        item.shortcut = String::from(shortcut);
        item
    }

    pub fn separator() -> Self {
        MenuItem {
            label: String::new(),
            action: MenuAction::None,
            shortcut: String::new(),
            enabled: false,
            separator: true,
            has_submenu: false,
            submenu: Vec::new(),
            checked: None,
            icon: None,
        }
    }

    pub fn submenu(label: &str, items: Vec<MenuItem>) -> Self {
        MenuItem {
            label: String::from(label),
            action: MenuAction::None,
            shortcut: String::new(),
            enabled: true,
            separator: false,
            has_submenu: true,
            submenu: items,
            checked: None,
            icon: None,
        }
    }

    pub fn checked(label: &str, action: MenuAction, checked: bool) -> Self {
        let mut item = Self::action(label, action);
        item.checked = Some(checked);
        item
    }

    pub fn disabled(label: &str) -> Self {
        let mut item = Self::new(label);
        item.enabled = false;
        item
    }
}

// ============================================================================
// MENÜ
// ============================================================================

/// Açılır menü
pub struct Menu {
    /// Menü başlığı
    pub title: String,
    /// Menü öğeleri
    pub items: Vec<MenuItem>,
    /// Menünün X konumu
    pub x: usize,
    /// Özel menü mü (uygulama menüsü)
    pub is_app_menu: bool,
    /// Açık mı
    pub open: bool,
    /// Üzerine gelinen öğe indeksi
    pub hovered_item: Option<usize>,
    /// Açık alt menü indeksi
    pub open_submenu: Option<usize>,
}

impl Menu {
    pub fn new(title: &str, x: usize) -> Self {
        Menu {
            title: String::from(title),
            items: Vec::new(),
            x,
            is_app_menu: false,
            open: false,
            hovered_item: None,
            open_submenu: None,
        }
    }

    pub fn app_menu(x: usize) -> Self {
        let mut menu = Menu::new("", x);
        menu.is_app_menu = true;
        menu.items = vec![
            MenuItem::action("About echOS", MenuAction::About),
            MenuItem::separator(),
            MenuItem::shortcut("Preferences...", MenuAction::Preferences, "⌘,"),
            MenuItem::separator(),
            MenuItem::action("Hide echOS", MenuAction::HideApp),
            MenuItem::action("Hide Others", MenuAction::HideOthers),
            MenuItem::action("Show All", MenuAction::ShowAll),
            MenuItem::separator(),
            MenuItem::shortcut("Quit echOS", MenuAction::Quit, "⌘Q"),
        ];
        menu
    }

    pub fn file_menu(x: usize) -> Self {
        let mut menu = Menu::new("File", x);
        menu.items = vec![
            MenuItem::shortcut("New", MenuAction::NewFile, "⌘N"),
            MenuItem::shortcut("Open...", MenuAction::OpenFile, "⌘O"),
            MenuItem::submenu("Open Recent", vec![
                MenuItem::action("Document1.txt", MenuAction::Custom("open_recent".into())),
                MenuItem::action("Document2.txt", MenuAction::Custom("open_recent".into())),
                MenuItem::separator(),
                MenuItem::action("Clear Menu", MenuAction::None),
            ]),
            MenuItem::separator(),
            MenuItem::shortcut("Close", MenuAction::Close, "⌘W"),
            MenuItem::shortcut("Save", MenuAction::SaveFile, "⌘S"),
            MenuItem::shortcut("Save As...", MenuAction::SaveAs, "⇧⌘S"),
            MenuItem::separator(),
            MenuItem::shortcut("Print...", MenuAction::Custom("print".into()), "⌘P"),
        ];
        menu
    }

    pub fn edit_menu(x: usize) -> Self {
        let mut menu = Menu::new("Edit", x);
        menu.items = vec![
            MenuItem::shortcut("Undo", MenuAction::Undo, "⌘Z"),
            MenuItem::shortcut("Redo", MenuAction::Redo, "⇧⌘Z"),
            MenuItem::separator(),
            MenuItem::shortcut("Cut", MenuAction::Cut, "⌘X"),
            MenuItem::shortcut("Copy", MenuAction::Copy, "⌘C"),
            MenuItem::shortcut("Paste", MenuAction::Paste, "⌘V"),
            MenuItem::shortcut("Select All", MenuAction::SelectAll, "⌘A"),
            MenuItem::separator(),
            MenuItem::shortcut("Find...", MenuAction::Find, "⌘F"),
            MenuItem::shortcut("Find and Replace...", MenuAction::Replace, "⇧⌘F"),
        ];
        menu
    }

    pub fn view_menu(x: usize) -> Self {
        let mut menu = Menu::new("View", x);
        menu.items = vec![
            MenuItem::checked("Show Toolbar", MenuAction::Custom("show_toolbar".into()), true),
            MenuItem::checked("Show Sidebar", MenuAction::Custom("show_sidebar".into()), true),
            MenuItem::checked("Show Status Bar", MenuAction::Custom("show_status".into()), true),
            MenuItem::separator(),
            MenuItem::shortcut("Enter Full Screen", MenuAction::FullScreen, "⌃⌘F"),
        ];
        menu
    }

    pub fn window_menu(x: usize) -> Self {
        let mut menu = Menu::new("Window", x);
        menu.items = vec![
            MenuItem::shortcut("Minimize", MenuAction::Minimize, "⌘M"),
            MenuItem::shortcut("Zoom", MenuAction::Zoom, ""),
            MenuItem::separator(),
            MenuItem::action("Bring All to Front", MenuAction::Custom("bring_front".into())),
        ];
        menu
    }

    pub fn help_menu(x: usize) -> Self {
        let mut menu = Menu::new("Help", x);
        menu.items = vec![
            MenuItem::action("echOS Help", MenuAction::Custom("help".into())),
            MenuItem::separator(),
            MenuItem::action("Report a Bug", MenuAction::Custom("bug".into())),
            MenuItem::action("Send Feedback", MenuAction::Custom("feedback".into())),
        ];
        menu
    }
}

// ============================================================================
// GLOBAL MENÜ ÇUBUĞU
// ============================================================================

/// Global menü çubuğu
pub struct MenuBar {
    /// Menüler
    menus: Vec<Menu>,
    /// Aktif uygulama adı
    app_name: String,
    /// Uygulama menüsü simgesi (özel uygulama menüsü için)
    app_icon: String,
    /// Ekran genişliği
    screen_width: usize,
    /// Menü açık mı
    menu_open: bool,
    /// Üzerine gelinen menü indeksi
    hovered_menu: Option<usize>,
    /// Sağ taraf öğeleri (durum simgeleri)
    right_items: Vec<MenuBarRightItem>,
}

#[derive(Clone, Debug)]
pub struct MenuBarRightItem {
    pub icon: String,
    pub text: String,
    pub action: RightItemAction,
}

#[derive(Clone, Debug)]
pub enum RightItemAction {
    None,
    OpenControlCenter,
    OpenNotificationCenter,
    OpenSpotlight,
    ShowBattery,
    ShowWifi,
    ShowVolume,
    ShowClock,
}

impl MenuBar {
    pub fn new(screen_width: usize) -> Self {
        let mut menubar = MenuBar {
            menus: Vec::new(),
            app_name: String::from("echOS"),
            app_icon: String::from("🍎"),
            screen_width,
            menu_open: false,
            hovered_menu: None,
            right_items: Vec::new(),
        };

        menubar.add_default_menus();
        menubar.add_default_right_items();
        menubar
    }

    fn add_default_menus(&mut self) {
        let mut x = MENU_PADDING;

        // Uygulama menüsü (özel)
        self.menus.push(Menu::app_menu(x));
        x += 30;

        // Standart menüler
        self.menus.push(Menu::file_menu(x));
        x += 50;

        self.menus.push(Menu::edit_menu(x));
        x += 50;

        self.menus.push(Menu::view_menu(x));
        x += 55;

        self.menus.push(Menu::window_menu(x));
        x += 70;

        self.menus.push(Menu::help_menu(x));
    }

    fn add_default_right_items(&mut self) {
        self.right_items = vec![
            MenuBarRightItem {
                icon: String::from("🔋"),
                text: String::from("100%"),
                action: RightItemAction::ShowBattery
            },
            MenuBarRightItem {
                icon: String::from("📶"),
                text: String::from("echOS-WiFi"),
                action: RightItemAction::ShowWifi
            },
            MenuBarRightItem {
                icon: String::from("🔊"),
                text: String::new(),
                action: RightItemAction::ShowVolume
            },
            MenuBarRightItem {
                icon: String::from("🔍"),
                text: String::new(),
                action: RightItemAction::OpenSpotlight
            },
            MenuBarRightItem {
                icon: String::from("⌘"),
                text: String::new(),
                action: RightItemAction::OpenControlCenter
            },
            MenuBarRightItem {
                icon: String::from("🕐"),
                text: String::from("Mon 12:00"),
                action: RightItemAction::ShowClock
            },
        ];
    }

    /// Menü çubuğunu çiz
    pub fn draw(&self, fb: &mut Framebuffer) {
        // Arka plan
        fb.draw_rect(0, 0, self.screen_width, MENU_BAR_HEIGHT, Theme::TITLEBAR_BG.to_u32());

        // Menüleri çiz
        for (i, menu) in self.menus.iter().enumerate() {
            let is_open = menu.open;
            let is_hovered = self.hovered_menu == Some(i) && self.menu_open;

            let text_color = if is_open || is_hovered {
                Theme::TEXT_ON_ACCENT.to_u32()
            } else {
                Theme::TEXT_PRIMARY.to_u32()
            };

            let bg_color = if is_open || is_hovered {
                Theme::ACCENT_PRIMARY.to_u32()
            } else {
                Theme::TRANSPARENT.to_u32()
            };

            // Metin genişliğini hesapla
            let text = if menu.is_app_menu {
                &self.app_icon
            } else {
                &menu.title
            };

            let text_width = text.len() * 8;

            // Arka planı çiz
            if bg_color != Theme::TRANSPARENT.to_u32() {
                fb.draw_rect(menu.x, 0, text_width + MENU_PADDING * 2, MENU_BAR_HEIGHT, bg_color);
            }

            // Metni çiz
            fb.draw_string(menu.x + MENU_PADDING, 4, text, text_color);

            // Açık menünün açılır listesini çiz
            if is_open {
                self.draw_menu_dropdown(fb, menu);
            }
        }

        // Sağ taraf öğelerini çiz
        let mut x = self.screen_width - MENU_PADDING;

        for item in self.right_items.iter().rev() {
            let text = if item.text.is_empty() {
                item.icon.clone()
            } else {
                format!("{} {}", item.icon, item.text)
            };

            let text_width = text.len() * 8;
            x -= text_width + MENU_PADDING;

            fb.draw_string(x, 4, &text, Theme::TEXT_PRIMARY.to_u32());
        }
    }

    fn draw_menu_dropdown(&self, fb: &mut Framebuffer, menu: &Menu) {
        if menu.items.is_empty() {
            return;
        }

        // Menü genişliği ve yüksekliğini hesapla
        let mut max_width = 150;
        for item in &menu.items {
            let item_width = item.label.len() * 8 + MENU_PADDING * 4;
            if !item.shortcut.is_empty() {
                max_width = max_width.max(item_width + item.shortcut.len() * 8 + 20);
            } else {
                max_width = max_width.max(item_width);
            }
        }

        let height = menu.items.len() * MENU_ITEM_HEIGHT;
        let x = menu.x;
        let y = MENU_BAR_HEIGHT;

        // Arka plan
        fb.draw_rect(x, y, max_width, height + 4, Theme::WINDOW_BG.to_u32());

        // Kenarlık
        fb.draw_rect_outline(x, y, max_width, height + 4, Theme::BORDER.to_u32());

        // Öğeleri çiz
        for (i, item) in menu.items.iter().enumerate() {
            let item_y = y + i * MENU_ITEM_HEIGHT;

            if item.separator {
                fb.draw_rect(x + 4, item_y + MENU_ITEM_HEIGHT / 2, max_width - 8, 1, Theme::BORDER.to_u32());
                continue;
            }

            let is_hovered = menu.hovered_item == Some(i);

            // Vurgulama
            if is_hovered {
                fb.draw_rect(x + 1, item_y, max_width - 2, MENU_ITEM_HEIGHT, Theme::ACCENT_PRIMARY.to_u32());
            }

            let text_color = if !item.enabled {
                Theme::TEXT_DISABLED.to_u32()
            } else if is_hovered {
                Theme::TEXT_ON_ACCENT.to_u32()
            } else {
                Theme::TEXT_PRIMARY.to_u32()
            };

            // Onay kutusu
            if let Some(checked) = item.checked {
                let check_x = x + MENU_PADDING;
                let check_text = if checked { "✓" } else { " " };
                fb.draw_string(check_x, item_y + 3, check_text, text_color);
            }

            // Simge
            let icon_offset = if item.checked.is_some() { 16 } else { 0 };
            if let Some(ref icon) = item.icon {
                fb.draw_string(x + MENU_PADDING + icon_offset, item_y + 3, icon, text_color);
            }

            // Etiket
            let label_x = x + MENU_PADDING + icon_offset + if item.icon.is_some() { 16 } else { 0 };
            fb.draw_string(label_x, item_y + 3, &item.label, text_color);

            // Kısayol
            if !item.shortcut.is_empty() {
                let shortcut_x = x + max_width - item.shortcut.len() * 8 - MENU_PADDING;
                fb.draw_string(shortcut_x, item_y + 3, &item.shortcut, Theme::TEXT_SECONDARY.to_u32());
            }

            // Alt menü oku
            if item.has_submenu {
                fb.draw_string(x + max_width - 16, item_y + 3, "▶", text_color);
            }
        }
    }

    /// Fare hareketi olayını işle
    pub fn on_mouse_move(&mut self, mx: i32, my: i32) -> MenuBarEvent {
        // Menü çubuğunda mı kontrol et
        if my >= 0 && my < MENU_BAR_HEIGHT as i32 {
            // Menüleri kontrol et
            for (i, menu) in self.menus.iter().enumerate() {
                let text = if menu.is_app_menu { &self.app_icon } else { &menu.title };
                let text_width = text.len() * 8 + MENU_PADDING * 2;

                if mx >= menu.x as i32 && mx < (menu.x + text_width) as i32 {
                    if self.menu_open && self.hovered_menu != Some(i) {
                        // Geçerli menüyü kapat, yenisini aç
                        if let Some(current) = self.hovered_menu {
                            self.menus[current].open = false;
                        }
                        self.menus[i].open = true;
                        self.hovered_menu = Some(i);
                    }
                    return MenuBarEvent::None;
                }
            }

            // Sağ taraf öğelerini kontrol et
            let mut x = self.screen_width as i32 - MENU_PADDING as i32;
            for item in self.right_items.iter().rev() {
                let text = if item.text.is_empty() {
                    item.icon.clone()
                } else {
                    format!("{} {}", item.icon, item.text)
                };
                let text_width = text.len() as i32 * 8 + MENU_PADDING as i32;
                x -= text_width;

                if mx >= x && mx < x + text_width {
                    return MenuBarEvent::RightItemHovered(item.action.clone());
                }
            }
        }

        // Açılır menüde mi kontrol et
        if self.menu_open {
            if let Some(menu_idx) = self.hovered_menu {
                let menu = &self.menus[menu_idx];
                if menu.open {
                    let mut max_width = 150;
                    for item in &menu.items {
                        let item_width = item.label.len() * 8 + MENU_PADDING * 4;
                        if !item.shortcut.is_empty() {
                            max_width = max_width.max(item_width + item.shortcut.len() * 8 + 20);
                        } else {
                            max_width = max_width.max(item_width);
                        }
                    }

                    if mx >= menu.x as i32 && mx < (menu.x + max_width) as i32
                        && my >= MENU_BAR_HEIGHT as i32
                        && my < (MENU_BAR_HEIGHT + menu.items.len() * MENU_ITEM_HEIGHT) as i32 {

                        let item_idx = ((my - MENU_BAR_HEIGHT as i32) / MENU_ITEM_HEIGHT as i32) as usize;
                        if item_idx < menu.items.len() {
                            // Üzerine gelinen öğeyi güncelle
                            let menu = &mut self.menus[menu_idx];
                            if !menu.items[item_idx].separator && menu.items[item_idx].enabled {
                                menu.hovered_item = Some(item_idx);
                            }
                        }
                    }
                }
            }
        }

        MenuBarEvent::None
    }

    /// Fare basımı olayını işle
    pub fn on_mouse_down(&mut self, mx: i32, my: i32) -> MenuBarEvent {
        // Menü çubuğunda mı kontrol et
        if my >= 0 && my < MENU_BAR_HEIGHT as i32 {
            // Menüleri kontrol et
            for (i, menu) in self.menus.iter().enumerate() {
                let text = if menu.is_app_menu { &self.app_icon } else { &menu.title };
                let text_width = text.len() * 8 + MENU_PADDING * 2;

                if mx >= menu.x as i32 && mx < (menu.x + text_width) as i32 {
                    // Menüyü aç/kapat
                    if self.menus[i].open {
                        self.close_all_menus();
                    } else {
                        self.open_menu(i);
                    }
                    return MenuBarEvent::None;
                }
            }

            // Sağ taraf öğelerini kontrol et
            let mut x = self.screen_width as i32 - MENU_PADDING as i32;
            for item in self.right_items.iter().rev() {
                let text = if item.text.is_empty() {
                    item.icon.clone()
                } else {
                    format!("{} {}", item.icon, item.text)
                };
                let text_width = text.len() as i32 * 8 + MENU_PADDING as i32;
                x -= text_width;

                if mx >= x && mx < x + text_width {
                    return MenuBarEvent::RightItemClicked(item.action.clone());
                }
            }
        }

        // Menü öğesine tıklandı mı kontrol et
        if self.menu_open {
            if let Some(menu_idx) = self.hovered_menu {
                let menu = &self.menus[menu_idx];
                if menu.open {
                    if let Some(item_idx) = menu.hovered_item {
                        let item = &menu.items[item_idx];
                        if item.enabled && !item.separator {
                            let action = item.action.clone();
                            self.close_all_menus();
                            return MenuBarEvent::MenuItemSelected(action);
                        }
                    }
                }
            }
        }

        // Dışarı tıklandı — menüleri kapat
        self.close_all_menus();
        MenuBarEvent::None
    }

    /// Menü aç
    pub fn open_menu(&mut self, index: usize) {
        self.close_all_menus();
        if index < self.menus.len() {
            self.menus[index].open = true;
            self.menu_open = true;
            self.hovered_menu = Some(index);
        }
    }

    /// Tüm menüleri kapat
    pub fn close_all_menus(&mut self) {
        for menu in &mut self.menus {
            menu.open = false;
            menu.hovered_item = None;
        }
        self.menu_open = false;
        self.hovered_menu = None;
    }

    /// Aktif uygulamayı ayarla
    pub fn set_active_app(&mut self, name: &str) {
        self.app_name = String::from(name);
    }

    /// Uygulama menülerini güncelle
    pub fn update_app_menus(&mut self, menus: Vec<Menu>) {
        // Uygulama menüsünü koru, diğerlerini değiştir
        self.menus.truncate(1);
        self.menus.extend(menus);
    }

    /// Yeniden boyutlandır
    pub fn resize(&mut self, width: usize) {
        self.screen_width = width;
    }

    /// Yüksekliği al
    pub fn height(&self) -> usize {
        MENU_BAR_HEIGHT
    }
}

/// Menü çubuğu olayları
#[derive(Clone, Debug)]
pub enum MenuBarEvent {
    None,
    MenuItemSelected(MenuAction),
    RightItemClicked(RightItemAction),
    RightItemHovered(RightItemAction),
}

// ============================================================================
// GLOBAL MENÜ ÇUBUĞU
// ============================================================================

lazy_static::lazy_static! {
    static ref MENU_BAR: Mutex<MenuBar> = Mutex::new(MenuBar::new(1920));
}

/// Menü çubuğunu başlat
pub fn init(width: usize) {
    let mut menubar = MENU_BAR.lock();
    menubar.resize(width);
    crate::serial_println!("[GUI] Menu Bar initialized ({}px)", width);
}

/// Menü çubuğuna erişim sağla
pub fn get_menu_bar() -> &'static Mutex<MenuBar> {
    &MENU_BAR
}
