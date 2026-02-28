//! # echOS GUI Teması
//!
//! Modern, karanlık tonlu renk paleti ve tema tanımları.
//! VS Code ve JetBrains IDE'lerinden esinlenilmiştir.

/// RGB Renk Yapısı
#[derive(Debug, Clone, Copy)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// 32-bit framebuffer formatına (0xRRGGBB) çevirir.
    pub const fn to_u32(&self) -> u32 {
        ((self.r as u32) << 16) | ((self.g as u32) << 8) | (self.b as u32)
    }
}

/// Tema Renkleri
pub struct Theme;

impl Theme {
    // Arkaplan Renkleri (Koyu Griler)
    pub const DESKTOP_BG: Color = Color::rgb(22, 22, 26); // Neredeyse Siyah
    pub const WINDOW_BG: Color = Color::rgb(30, 30, 34); // Koyu Antrasit
    pub const TITLEBAR_BG: Color = Color::rgb(40, 40, 44); // Orta Gri
    pub const TITLEBAR_ACTIVE: Color = Color::rgb(0, 122, 204); // VS Code Mavisi
    pub const SIDEBAR_BG: Color = Color::rgb(25, 25, 28);
    pub const TOOLBAR_BG: Color = Color::rgb(35, 35, 38);

    // Metin Renkleri
    pub const TEXT_PRIMARY: Color = Color::rgb(212, 212, 212); // Açık Gri
    pub const TEXT_SECONDARY: Color = Color::rgb(128, 128, 128); // Sönük Gri
    pub const TEXT_ACCENT: Color = Color::rgb(86, 156, 214); // Parlak Mavi
    pub const TEXT_ON_ACCENT: Color = Color::rgb(255, 255, 255);
    pub const TEXT_DISABLED: Color = Color::rgb(90, 90, 90);

    // Vurgu (Accent) Renkleri
    pub const ACCENT_PRIMARY: Color = Color::rgb(0, 150, 200); // Camgöbeği (Cyan)
    pub const ACCENT_SUCCESS: Color = Color::rgb(78, 201, 176); // Teal
    pub const ACCENT_WARNING: Color = Color::rgb(220, 180, 100); // Altın
    pub const ACCENT_ERROR: Color = Color::rgb(244, 71, 71); // Kırmızı
    pub const ERROR: Color = Color::rgb(244, 71, 71);

    // Kenarlık ve Gölgeler
    pub const BORDER: Color = Color::rgb(60, 60, 64);
    pub const SHADOW: Color = Color::rgb(10, 10, 12);
    pub const TRANSPARENT: Color = Color::rgb(0, 0, 0);

    // Taskbar
    pub const TASKBAR_BG: Color = Color::rgb(18, 18, 20);

    // Buton Kontrolleri
    pub const BUTTON_BG: Color = Color::rgb(50, 50, 54);
    pub const BUTTON_HOVER: Color = Color::rgb(70, 70, 75);
    pub const BUTTON_TEXT: Color = Color::rgb(220, 220, 220);
    
    // Kaydırma Çubuğu (Scrollbar) Renkleri
    // Kaydırma çubuğunun arka plan ve sürükleme topuzu (thumb) renkleridir.
    // Thumb, kullanıcının tıklayıp sürüklediği hareketli bölümdür.
    pub const SCROLLBAR_BG: Color = Color::rgb(40, 40, 44);
    pub const SCROLLBAR_THUMB: Color = Color::rgb(80, 80, 85);

    // Seçim (Selection) Rengi
    // Metin ya da liste öğesi seçildiğinde gösterilen arka plan rengidir.
    // VS Code'un seçim mavisiyle aynı tondadır.
    pub const SELECTION_BG: Color = Color::rgb(38, 79, 120);

    // Liste Öğesi (List Item) Renkleri
    // Fare üzerine geldiğinde (hover) ve seçildiğinde (selected) uygulanan
    // arka plan renkleridir. Hover daha soluk, selected daha belirgindir.
    pub const LIST_ITEM_HOVER: Color = Color::rgb(45, 45, 50);
    pub const LIST_ITEM_SELECTED: Color = Color::rgb(50, 80, 120);

    // Giriş Alanı (Input Field) Renkleri
    // Metin kutusu ve arama çubuğu gibi giriş bileşenlerinde kullanılır.
    // INPUT_FOCUS, odaklanıldığında kenarlığı vurgular (VS Code'un mavi odak rengi).
    pub const INPUT_BG: Color = Color::rgb(35, 35, 40);
    pub const INPUT_BORDER: Color = Color::rgb(60, 60, 65);
    pub const INPUT_FOCUS: Color = Color::rgb(0, 122, 204);

    // Menü (Menu) Renkleri
    // Açılır menülerin (dropdown/context menu) arka plan ve hover renkleridir.
    pub const MENU_BG: Color = Color::rgb(30, 30, 34);
    pub const MENU_ITEM_HOVER: Color = Color::rgb(50, 50, 55);

    // İlerleme Çubuğu (Progress Bar) Renkleri
    // PROGRESS_BG boş/dolu ayrımını belirtir; PROGRESS_FG dolgu rengidir (camgöbeği).
    pub const PROGRESS_BG: Color = Color::rgb(40, 40, 44);
    pub const PROGRESS_FG: Color = Color::rgb(0, 150, 200);
}
