//! # echOS Metin Giriş Widget'ları
//!
//! Tek satırlı (`TextBox`) ve çok satırlı (`TextArea`) metin giriş bileşenlerini içerir.
//!
//! ## İçerilen Widget'lar
//! - [`TextBox`]  — tek satır metin kutusu; şifre modu ve placeholder desteğiyle
//! - [`TextArea`] — çok satırlı metin alanı; yatay ve dikey kaydırma destekler
//!
//! ## Klavye Tarama Kodları (Scancode)
//! Bu dosyada PS/2 klavye tarama kodları doğrudan kullanılır:
//! - `0x0E` → Backspace    - `0x53` → Delete
//! - `0x4B` → Sol ok       - `0x4D` → Sağ ok
//! - `0x47` → Home         - `0x4F` → End
//! - `0x48` → Yukarı ok    - `0x50` → Aşağı ok
//!
//! ## İmleç ve Kaydırma
//! İmleç konumu (`cursor_pos`) her zaman metin dizisindeki bayt indeksini gösterir.
//! `scroll_offset`, görünür pencerenin metnin başından ne kadar ofsetlendiğini tutar;
//! imleç ekran dışına çıktığında `update_scroll` bunu otomatik ayarlar.

use super::{Rect, Widget, MOD_CTRL};
use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::Theme;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// Tek satırlı metin giriş kutusu.
///
/// Girilen text `text` alanında tutulur.
/// `scroll_offset`, uzun metinlerin görünür pencereye sığdırılması için
/// metnin başından kayan bir ofset tutar.
/// `password_mode` aktifken tüm karakterler "*" olarak gösterilir.
pub struct TextBox {
    rect: Rect,
    text: String,
    /// Kutu boşken gösterilen gri ipucu metni
    placeholder: String,
    /// Geçerli imleç konumu (metin dizisindeki bayt indeksi)
    cursor_pos: usize,
    /// Odaklanma durumu; false iken klavye olayları görmezden gelinir
    focused: bool,
    /// Görünür pencerede metnin başından kaç karakter atlandığı
    scroll_offset: usize,
    /// İzin verilen maksimum karakter sayısı
    max_length: usize,
    /// Şifre modu; true ise karakterler "*" olarak maskelenir
    password_mode: bool,
}

impl TextBox {
    /// Yeni bir metin kutusu oluşturur.
    /// Varsayılan değerler: boş metin, max 256 karakter, şifre modu kapalı.
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            rect: Rect::new(x, y, width, height),
            text: String::new(),
            placeholder: String::new(),
            cursor_pos: 0,
            focused: false,
            scroll_offset: 0,
            max_length: 256,
            password_mode: false,
        }
    }

    /// Builder kalıbıyla placeholder metni ayarlar.
    pub fn with_placeholder(mut self, placeholder: &str) -> Self {
        self.placeholder = String::from(placeholder);
        self
    }

    /// Builder kalıbıyla maksimum karakter sınırı ayarlar.
    pub fn with_max_length(mut self, max: usize) -> Self {
        self.max_length = max;
        self
    }

    /// Şifre modunu etkinleştirir veya devre dışı bırakır.
    pub fn set_password_mode(&mut self, enabled: bool) {
        self.password_mode = enabled;
    }

    /// Mevcut metin içeriğini döndürür.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Metin içeriğini programatik olarak ayarlar.
    /// İmleç metnin sonuna taşınır ve kaydırma yeniden hesaplanır.
    pub fn set_text(&mut self, text: &str) {
        self.text = String::from(text);
        self.cursor_pos = self.text.len().min(self.max_length);
        self.update_scroll();
    }

    /// Metin kutusunu temizler; imleç ve kaydırma ofseti sıfırlanır.
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor_pos = 0;
        self.scroll_offset = 0;
    }

    /// İmleç ekran içinde kalacak şekilde kaydırma ofsetini günceller.
    ///
    /// Görünür karakter sayısı `(genişlik - 10) / 8` formülüyle hesaplanır.
    /// İmleç görünür alanın soluna geçmişse offset azaltılır;
    /// sağına geçmişse artırılır.
    fn update_scroll(&mut self) {
        let char_width = 8;
        let visible_chars = (self.rect.width as usize - 10) / char_width;

        if self.cursor_pos < self.scroll_offset {
            self.scroll_offset = self.cursor_pos;
        } else if self.cursor_pos > self.scroll_offset + visible_chars {
            self.scroll_offset = self.cursor_pos - visible_chars;
        }
    }

    /// İmlecin bulunduğu konuma bir karakter ekler.
    /// Yalnızca ASCII yazdırılabilir karakterler ve boşluk kabul edilir.
    fn insert_char(&mut self, c: char) {
        if self.text.len() < self.max_length && c.is_ascii_graphic() || c == ' ' {
            self.text.insert(self.cursor_pos, c);
            self.cursor_pos += 1;
            self.update_scroll();
        }
    }

    /// İmlecin sağındaki karakteri siler (Delete tuşu davranışı).
    fn delete_char(&mut self) {
        if self.cursor_pos < self.text.len() {
            self.text.remove(self.cursor_pos);
        }
    }

    /// İmlecin solundaki karakteri siler ve imleci bir geri taşır (Backspace davranışı).
    fn backspace(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
            self.text.remove(self.cursor_pos);
            self.update_scroll();
        }
    }

    /// İmleci bir karakter sola kaydırır.
    fn move_cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
            self.update_scroll();
        }
    }

    /// İmleci bir karakter sağa kaydırır.
    fn move_cursor_right(&mut self) {
        if self.cursor_pos < self.text.len() {
            self.cursor_pos += 1;
            self.update_scroll();
        }
    }

    /// İmleci metnin başına taşır (Home tuşu davranışı).
    fn move_cursor_home(&mut self) {
        self.cursor_pos = 0;
        self.scroll_offset = 0;
    }

    /// İmleci metnin sonuna taşır (End tuşu davranışı).
    fn move_cursor_end(&mut self) {
        self.cursor_pos = self.text.len();
        self.update_scroll();
    }
}

impl Widget for TextBox {
    /// Metin kutusunu çizer.
    /// Sırasıyla: arka plan → kenarlık (odak durumuna göre renkli) → metin/placeholder → imleç.
    fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let w = self.rect.width as usize;
        let h = self.rect.height as usize;

        // Odakta pencere arka planı; odak dışında daha koyu düğme arka planı
        let bg_color = if self.focused {
            Theme::WINDOW_BG.to_u32()
        } else {
            Theme::BUTTON_BG.to_u32()
        };
        fb.draw_rect(x, y, w, h, bg_color);

        // Kenarlık rengi: odaktayken aksent rengi, değilse normal kenarlık rengi
        let border_color = if self.focused {
            Theme::ACCENT_PRIMARY.to_u32()
        } else {
            Theme::BORDER.to_u32()
        };

        for col in x..(x + w) {
            fb.plot_pixel(col, y, border_color);
            fb.plot_pixel(col, y + h - 1, border_color);
        }
        for row in y..(y + h) {
            fb.plot_pixel(x, row, border_color);
            fb.plot_pixel(x + w - 1, row, border_color);
        }

        // Metin dikey olarak ortalanır (5 px sol iç boşlukla)
        let text_y = y + (h - 16) / 2;
        let text_x = x + 5;

        if self.text.is_empty() && !self.focused {
            // Boş ve odak dışındaysa placeholder soluk renkte gösterilir
            fb.draw_string(text_x, text_y, &self.placeholder, Theme::TEXT_SECONDARY.to_u32());
        } else {
            // Şifre modunda gerçek karakterler yerine "*" kullanılır
            let display_text = if self.password_mode {
                alloc::string::ToString::to_string(&"*".repeat(self.text.len()))
            } else {
                // Görünür pencereye sığan karakter dilimini al
                let start = self.scroll_offset;
                let end = (start + (w - 10) / 8).min(self.text.len());
                alloc::string::ToString::to_string(&self.text[start..end])
            };
            fb.draw_string(text_x, text_y, &display_text, Theme::TEXT_PRIMARY.to_u32());
        }

        // İmleç: yalnızca odaktayken dikey çizgi olarak gösterilir
        if self.focused {
            let cursor_char_pos = self.cursor_pos.saturating_sub(self.scroll_offset);
            let cursor_x = text_x + cursor_char_pos * 8;
            // Görünür alan dışına çıkan imleci çizme
            if cursor_x < x + w - 5 {
                for dy in 0..16 {
                    fb.plot_pixel(cursor_x, text_y + dy, Theme::TEXT_PRIMARY.to_u32());
                }
            }
        }
    }

    /// Tıklama olayını işler.
    /// Kutu sınırları içindeyse odaklanır ve fare X konumundan imleç pozisyonu hesaplanır.
    /// Dışına tıklandıysa odak kaybedilir.
    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if self.rect.contains(x, y) {
            self.focused = true;
            // Fare X'ten imleç pozisyonunu hesapla: (x - text_x) / 8 karakter
            let text_x = self.rect.x + 5;
            let click_offset = ((x - text_x) / 8) as usize;
            self.cursor_pos = (self.scroll_offset + click_offset).min(self.text.len());
            true
        } else {
            self.focused = false;
            false
        }
    }

    /// Klavye olayını işler.
    /// Odak yoksa false döner (olay tüketilmez).
    /// Tarama kodlarına göre imleç hareketi veya karakter girişi yapılır.
    /// MOD_CTRL basılıysa karakter girilmez (kısayol koruması).
    fn on_key(&mut self, key: char, modifiers: u8, scancode: u8) -> bool {
        if !self.focused {
            return false;
        }

        match scancode {
            0x0E => self.backspace(),           // Backspace
            0x53 => self.delete_char(),          // Delete
            0x4B => self.move_cursor_left(),     // Left arrow
            0x4D => self.move_cursor_right(),    // Right arrow
            0x47 => self.move_cursor_home(),     // Home
            0x4F => self.move_cursor_end(),      // End
            _ => {
                // Ctrl basılı değilse ve geçerli bir karakter geldiyse ekle
                if key != '\0' && (modifiers & MOD_CTRL) == 0 {
                    self.insert_char(key);
                }
            }
        }
        true
    }

    /// Widget sınırlarını döndürür.
    fn bounds(&self) -> Rect {
        self.rect
    }

    /// Odak durumunu döndürür.
    fn is_focused(&self) -> bool {
        self.focused
    }

    /// Odak durumunu programatik olarak ayarlar.
    fn set_focus(&mut self, focused: bool) {
        self.focused = focused;
    }
}

/// Çok satırlı metin alanı.
///
/// Metin, bir `Vec<String>` (satır listesi) olarak saklanır.
/// İmleç `(cursor_line, cursor_col)` ile satır ve sütun olarak takip edilir.
/// `scroll_line` / `scroll_col` ile hem dikey hem yatay kaydırma desteklenir.
/// Enter tuşu mevcut satırı ikiye böler; Backspace satır başındayken
/// önceki satırla birleştirir.
pub struct TextArea {
    rect: Rect,
    /// Her bir satırı tutan liste; en az bir boş satır içerir
    lines: Vec<String>,
    /// İmlecin bulunduğu satır indeksi
    cursor_line: usize,
    /// İmlecin bulunduğu sütun (bayt) indeksi
    cursor_col: usize,
    /// Görüntünün kaçıncı satırdan başladığı (dikey kaydırma)
    scroll_line: usize,
    /// Görüntünün kaçıncı sütundan başladığı (yatay kaydırma)
    scroll_col: usize,
    /// Odak durumu
    focused: bool,
    /// Satır yüksekliği piksel cinsinden (varsayılan 18 px)
    line_height: usize,
}

impl TextArea {
    /// Yeni bir metin alanı oluşturur.
    /// Başlangıçta tek boş satır içerir.
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            rect: Rect::new(x, y, width, height),
            lines: vec![String::new()],
            cursor_line: 0,
            cursor_col: 0,
            scroll_line: 0,
            scroll_col: 0,
            focused: false,
            line_height: 18,
        }
    }

    /// Tüm satırları `\n` ile birleştirerek tek bir String döndürür.
    pub fn text(&self) -> String {
        alloc::string::ToString::to_string(&self.lines.join("\n"))
    }

    /// Çok satırlı metni `\n` sınırından bölerek yükler.
    /// İmleç ve kaydırma sıfırlanır; boş metin için en az bir satır garantilenir.
    pub fn set_text(&mut self, text: &str) {
        self.lines = text.lines().map(String::from).collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.cursor_line = 0;
        self.cursor_col = 0;
        self.scroll_line = 0;
        self.scroll_col = 0;
    }

    /// Tüm içeriği temizler; imleç ve kaydırma sıfırlanır.
    pub fn clear(&mut self) {
        self.lines.clear();
        self.lines.push(String::new());
        self.cursor_line = 0;
        self.cursor_col = 0;
        self.scroll_line = 0;
        self.scroll_col = 0;
    }

    /// Görünür satır sayısını hesaplar.
    /// `(yükseklik - 10) / line_height` formülü kullanılır (5 px üst+alt boşluk).
    fn visible_lines(&self) -> usize {
        (self.rect.height as usize - 10) / self.line_height
    }

    /// Görünür sütun (karakter) sayısını hesaplar.
    /// Her karakter 8 piksel genişliğindedir.
    fn visible_cols(&self) -> usize {
        (self.rect.width as usize - 10) / 8
    }

    /// İmlecin bulunduğu konuma bir karakter veya yeni satır ekler.
    ///
    /// `\n` (Enter) → mevcut satırı imleç noktasından ikiye böler.
    /// ASCII yazdırılabilir, boşluk veya tab → satıra karakter ekler.
    fn insert_char(&mut self, c: char) {
        if c == '\n' {
            // Split line at cursor
            let current_line = self.lines[self.cursor_line].clone();
            let after_cursor: String = current_line[self.cursor_col..].into();
            self.lines[self.cursor_line].truncate(self.cursor_col);
            self.lines.insert(self.cursor_line + 1, after_cursor);
            self.cursor_line += 1;
            self.cursor_col = 0;
        } else if c.is_ascii_graphic() || c == ' ' || c == '\t' {
            self.lines[self.cursor_line].insert(self.cursor_col, c);
            self.cursor_col += 1;
        }
        self.update_scroll();
    }

    /// Backspace işlemini gerçekleştirir.
    /// Eğer sütun 0'dan büyükse bir karakter siler.
    /// Sütun 0 ve satır 0'dan büyükse önceki satırla birleştirir.
    fn backspace(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
            self.lines[self.cursor_line].remove(self.cursor_col);
        } else if self.cursor_line > 0 {
            // Merge with previous line
            self.cursor_line -= 1;
            self.cursor_col = self.lines[self.cursor_line].len();
            let current = self.lines.remove(self.cursor_line + 1);
            self.lines[self.cursor_line].push_str(&current);
        }
        self.update_scroll();
    }

    /// İmleç görünür pencere içinde kalacak şekilde kaydırma ofsetlerini günceller.
    /// Hem dikey (satır) hem yatay (sütun) kaydırma ayrı ayrı kontrol edilir.
    fn update_scroll(&mut self) {
        let visible_lines = self.visible_lines();
        let visible_cols = self.visible_cols();

        // Dikey kaydırma — imleç görünür alanın dışına çıkmışsa offset ayarla
        if self.cursor_line < self.scroll_line {
            self.scroll_line = self.cursor_line;
        } else if self.cursor_line >= self.scroll_line + visible_lines {
            self.scroll_line = self.cursor_line - visible_lines + 1;
        }

        // Yatay kaydırma — uzun satırlarda imleç sağa çıkınca offset artırılır
        if self.cursor_col < self.scroll_col {
            self.scroll_col = self.cursor_col;
        } else if self.cursor_col >= self.scroll_col + visible_cols {
            self.scroll_col = self.cursor_col - visible_cols + 1;
        }
    }
}

impl Widget for TextArea {
    /// Metin alanını çizer.
    /// Sırasıyla: arka plan → kenarlık → görünür satırlar → imleç.
    /// Yalnızca `scroll_line`'dan itibaren görünür satır sayısı kadar satır çizilir.
    fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let w = self.rect.width as usize;
        let h = self.rect.height as usize;

        // Sabit pencere arka planı
        let bg_color = Theme::WINDOW_BG.to_u32();
        fb.draw_rect(x, y, w, h, bg_color);

        // Odak durumuna göre kenarlık rengi
        let border_color = if self.focused {
            Theme::ACCENT_PRIMARY.to_u32()
        } else {
            Theme::BORDER.to_u32()
        };

        for col in x..(x + w) {
            fb.plot_pixel(col, y, border_color);
            fb.plot_pixel(col, y + h - 1, border_color);
        }
        for row in y..(y + h) {
            fb.plot_pixel(x, row, border_color);
            fb.plot_pixel(x + w - 1, row, border_color);
        }

        // Görünür satırları çiz — yatay kaydırma da dikkate alınır
        let text_x = x + 5;
        let mut text_y = y + 5;
        let visible_lines = self.visible_lines();
        let visible_cols = self.visible_cols();

        for i in 0..visible_lines {
            let line_idx = self.scroll_line + i;
            if line_idx >= self.lines.len() {
                break;
            }

            let line = &self.lines[line_idx];
            // Yatay kaydırma ofseti uygulanır; satır kısa olabilir
            let start = self.scroll_col.min(line.len());
            let end = (start + visible_cols).min(line.len());
            let display = &line[start..end];

            fb.draw_string(text_x, text_y, display, Theme::TEXT_PRIMARY.to_u32());
            text_y += self.line_height;
        }

        // İmleç — yalnızca odaktayken çizilir; görünür pencere içinde olduğu kontrol edilir
        if self.focused {
            let cursor_screen_line = self.cursor_line - self.scroll_line;
            let cursor_screen_col = self.cursor_col.saturating_sub(self.scroll_col);
            let cursor_x = text_x + cursor_screen_col * 8;
            let cursor_y = y + 5 + cursor_screen_line * self.line_height;

            // İmleç görünür alanın sınırlarına uymuyorsa çizme
            if cursor_x < x + w - 5 && cursor_y < y + h - 5 {
                for dy in 0..16 {
                    fb.plot_pixel(cursor_x, cursor_y + dy, Theme::TEXT_PRIMARY.to_u32());
                }
            }
        }
    }

    /// Tıklama olayını işler.
    /// Fare konumundan satır ve sütun hesaplanır; değerler geçerli aralığa kırpılır.
    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if self.rect.contains(x, y) {
            self.focused = true;

            // Calculate cursor position
            let text_x = self.rect.x + 5;
            let text_y = self.rect.y + 5;

            // Tıklanan Y'den satır, tıklanan X'ten sütun hesapla
            self.cursor_line = self.scroll_line + ((y - text_y) as usize / self.line_height);
            self.cursor_col = self.scroll_col + ((x - text_x) as usize / 8);

            // Sınır aşımını sınırla
            self.cursor_line = self.cursor_line.min(self.lines.len() - 1);
            self.cursor_col = self.cursor_col.min(self.lines[self.cursor_line].len());

            true
        } else {
            self.focused = false;
            false
        }
    }

    /// Klavye olayını işler.
    /// Ok tuşları imleç hareketini, Backspace silmeyi,
    /// diğer karakterler ise `insert_char` üzerinden eklemeyi sağlar.
    fn on_key(&mut self, key: char, modifiers: u8, scancode: u8) -> bool {
        if !self.focused {
            return false;
        }

        match scancode {
            0x0E => self.backspace(),
            0x48 => { // Up arrow — yukarı satıra geç; sütunu mevcut satırın uzunluğuna kırp
                if self.cursor_line > 0 {
                    self.cursor_line -= 1;
                    self.cursor_col = self.cursor_col.min(self.lines[self.cursor_line].len());
                    self.update_scroll();
                }
            }
            0x50 => { // Down arrow — aşağı satıra geç; sütunu kırp
                if self.cursor_line < self.lines.len() - 1 {
                    self.cursor_line += 1;
                    self.cursor_col = self.cursor_col.min(self.lines[self.cursor_line].len());
                    self.update_scroll();
                }
            }
            0x4B => { // Left arrow — solu sola bir karakter kaydır
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                    self.update_scroll();
                }
            }
            0x4D => { // Right arrow — sağa bir karakter kaydır
                if self.cursor_col < self.lines[self.cursor_line].len() {
                    self.cursor_col += 1;
                    self.update_scroll();
                }
            }
            _ => {
                // Ctrl basılı değilse karakteri ekle (Enter dahil \n gibi özel karakterler de buraya gelir)
                if key != '\0' && (modifiers & MOD_CTRL) == 0 {
                    self.insert_char(key);
                }
            }
        }
        true
    }

    /// Widget sınırlarını döndürür.
    fn bounds(&self) -> Rect {
        self.rect
    }

    /// Odak durumunu döndürür.
    fn is_focused(&self) -> bool {
        self.focused
    }

    /// Odak durumunu programatik olarak ayarlar.
    fn set_focus(&mut self, focused: bool) {
        self.focused = focused;
    }
}
