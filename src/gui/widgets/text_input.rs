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

use super::{
    border_rect_objects, draw_render_objects, solid_rect_object, text_render_object_with_width,
    AccessRole, AccessState, AccessibilityInfo, Rect, Widget, MOD_CTRL, MOD_SHIFT,
};
use crate::gop::framebuffer::Framebuffer;
use crate::gui::protocol::{DamageLane, RenderObject};
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
    /// Seçim başlangıç konumu (Some ise seçim aktif)
    selection_start: Option<usize>,
    /// Seçim bitiş konumu (imleç konumuna eşittir)
    selection_end: Option<usize>,
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
            selection_start: None,
            selection_end: None,
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

    /// Seçim aktif mi?
    pub fn has_selection(&self) -> bool {
        if let (Some(s), Some(e)) = (self.selection_start, self.selection_end) {
            s != e
        } else {
            false
        }
    }

    /// Seçili metnin (start, end) aralığını normalleştirilmiş olarak döndürür.
    fn selection_range(&self) -> Option<(usize, usize)> {
        if let (Some(s), Some(e)) = (self.selection_start, self.selection_end) {
            if s != e {
                Some((s.min(e), s.max(e)))
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Seçili metni döndürür.
    pub fn selected_text(&self) -> &str {
        if let Some((start, end)) = self.selection_range() {
            &self.text[start..end]
        } else {
            ""
        }
    }

    /// Seçili metni siler; imleç seçimin başına taşınır.
    fn delete_selection(&mut self) {
        if let Some((start, end)) = self.selection_range() {
            self.text.drain(start..end);
            self.cursor_pos = start;
            self.clear_selection();
            self.update_scroll();
        }
    }

    /// Seçimi temizler.
    fn clear_selection(&mut self) {
        self.selection_start = None;
        self.selection_end = None;
    }

    /// Shift basılıyken seçim başlatır veya genişletir.
    fn extend_selection(&mut self) {
        if self.selection_start.is_none() {
            // Yeni seçim başlat — anchor noktası mevcut imleç konumu
            self.selection_start = Some(self.cursor_pos);
        }
        // selection_end her zaman cursor_pos'u takip eder
        self.selection_end = Some(self.cursor_pos);
    }

    fn render_primitives(&self) -> Vec<RenderObject> {
        let mut objects = Vec::new();
        let base_id = ((self.rect.x as u64) << 32) ^ (self.rect.y as u64);
        let bg_color = if self.focused {
            Theme::WINDOW_BG.to_u32()
        } else {
            Theme::BUTTON_BG.to_u32()
        };
        let border_color = if self.focused {
            Theme::ACCENT_PRIMARY.to_u32()
        } else {
            Theme::BORDER.to_u32()
        };
        let text_y = self.rect.y + ((self.rect.height - 16).max(0) / 2);
        let text_x = self.rect.x + 5;

        objects.push(solid_rect_object(
            base_id,
            self.rect,
            bg_color,
            DamageLane::Window,
            0,
        ));
        objects.extend(border_rect_objects(
            base_id ^ 0x10,
            self.rect,
            border_color,
            DamageLane::Window,
            1,
        ));

        if self.text.is_empty() && !self.focused {
            objects.push(text_render_object_with_width(
                base_id ^ 0x20,
                Rect::new(text_x, text_y, (self.rect.width - 10).max(1), 18),
                &self.placeholder,
                Theme::TEXT_SECONDARY.to_u32(),
                false,
                DamageLane::Text,
                3,
            ));
        } else {
            let display_text = if self.password_mode {
                alloc::string::ToString::to_string(&"*".repeat(self.text.len()))
            } else {
                let start = self.scroll_offset;
                let end = (start + ((self.rect.width.max(0) as usize).saturating_sub(10) / 8))
                    .min(self.text.len());
                alloc::string::ToString::to_string(&self.text[start..end])
            };

            if let Some((sel_s, sel_e)) = self.selection_range() {
                let vis_start = self.scroll_offset;
                let vis_end = vis_start + ((self.rect.width.max(0) as usize).saturating_sub(10) / 8);
                let hl_start = sel_s.max(vis_start);
                let hl_end = sel_e.min(vis_end);
                if hl_start < hl_end {
                    objects.push(solid_rect_object(
                        base_id ^ 0x30,
                        Rect::new(
                            text_x + ((hl_start - vis_start) as i32 * 8),
                            text_y,
                            ((hl_end - hl_start) as i32 * 8).max(1),
                            16,
                        ),
                        Theme::ACCENT_PRIMARY.to_u32(),
                        DamageLane::Window,
                        2,
                    ));
                }
            }

            objects.push(text_render_object_with_width(
                base_id ^ 0x40,
                Rect::new(text_x, text_y, (self.rect.width - 10).max(1), 18),
                &display_text,
                Theme::TEXT_PRIMARY.to_u32(),
                false,
                DamageLane::Text,
                3,
            ));
        }

        if self.focused {
            let cursor_char_pos = self.cursor_pos.saturating_sub(self.scroll_offset) as i32;
            let cursor_x = text_x + cursor_char_pos * 8;
            if cursor_x < self.rect.x + self.rect.width - 5 {
                objects.push(solid_rect_object(
                    base_id ^ 0x50,
                    Rect::new(cursor_x, text_y, 1, 16),
                    Theme::TEXT_PRIMARY.to_u32(),
                    DamageLane::Cursor,
                    4,
                ));
            }
        }

        objects
    }
}

impl Widget for TextBox {
    /// Metin kutusunu çizer.
    /// Sırasıyla: arka plan → kenarlık (odak durumuna göre renkli) → metin/placeholder → imleç.
    fn draw(&self, fb: &mut Framebuffer) {
        let objects = self.render_primitives();
        draw_render_objects(fb, self.rect, &objects);
    }

    /// Tıklama olayını işler.
    /// Kutu sınırları içindeyse odaklanır ve fare X konumundan imleç pozisyonu hesaplanır.
    /// Dışına tıklandıysa odak kaybedilir.
    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if self.rect.contains(x, y) {
            self.focused = true;
            self.clear_selection();
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

        let shift = (modifiers & MOD_SHIFT) != 0;

        match scancode {
            0x0E => {
                // Backspace: seçim varsa seçimi sil, yoksa normal backspace
                if self.has_selection() {
                    self.delete_selection();
                } else {
                    self.backspace();
                }
            }
            0x53 => {
                // Delete: seçim varsa seçimi sil, yoksa normal delete
                if self.has_selection() {
                    self.delete_selection();
                } else {
                    self.delete_char();
                }
            }
            0x4B => {
                // Left arrow
                if shift {
                    if self.selection_start.is_none() {
                        self.selection_start = Some(self.cursor_pos);
                    }
                    self.move_cursor_left();
                    self.selection_end = Some(self.cursor_pos);
                } else {
                    self.clear_selection();
                    self.move_cursor_left();
                }
            }
            0x4D => {
                // Right arrow
                if shift {
                    if self.selection_start.is_none() {
                        self.selection_start = Some(self.cursor_pos);
                    }
                    self.move_cursor_right();
                    self.selection_end = Some(self.cursor_pos);
                } else {
                    self.clear_selection();
                    self.move_cursor_right();
                }
            }
            0x47 => {
                // Home
                if shift {
                    if self.selection_start.is_none() {
                        self.selection_start = Some(self.cursor_pos);
                    }
                    self.move_cursor_home();
                    self.selection_end = Some(self.cursor_pos);
                } else {
                    self.clear_selection();
                    self.move_cursor_home();
                }
            }
            0x4F => {
                // End
                if shift {
                    if self.selection_start.is_none() {
                        self.selection_start = Some(self.cursor_pos);
                    }
                    self.move_cursor_end();
                    self.selection_end = Some(self.cursor_pos);
                } else {
                    self.clear_selection();
                    self.move_cursor_end();
                }
            }
            _ => {
                // Ctrl basılı değilse ve geçerli bir karakter geldiyse ekle
                if key != '\0' && (modifiers & MOD_CTRL) == 0 {
                    // Seçim varsa önce sil, sonra yeni karakteri ekle
                    if self.has_selection() {
                        self.delete_selection();
                    }
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

    fn can_focus(&self) -> bool {
        true
    }

    fn accessibility_info(&self) -> AccessibilityInfo<'_> {
        let mut state = AccessState::empty();
        if self.focused {
            state = state.with(AccessState::FOCUSED);
        }
        AccessibilityInfo {
            role: AccessRole::TextInput,
            label: if self.placeholder.is_empty() {
                "textbox"
            } else {
                &self.placeholder
            },
            value: &self.text,
            state,
        }
    }

    fn render_objects(&self) -> Vec<RenderObject> {
        self.render_primitives()
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

    fn render_primitives(&self) -> Vec<RenderObject> {
        let mut objects = Vec::new();
        let base_id = ((self.rect.x as u64) << 32) ^ (self.rect.y as u64) ^ 0x9000_0000;
        let border_color = if self.focused {
            Theme::ACCENT_PRIMARY.to_u32()
        } else {
            Theme::BORDER.to_u32()
        };
        let text_x = self.rect.x + 5;
        let visible_lines = self.visible_lines();
        let visible_cols = self.visible_cols();

        objects.push(solid_rect_object(
            base_id,
            self.rect,
            Theme::WINDOW_BG.to_u32(),
            DamageLane::Window,
            0,
        ));
        objects.extend(border_rect_objects(
            base_id ^ 0x10,
            self.rect,
            border_color,
            DamageLane::Window,
            1,
        ));

        for i in 0..visible_lines {
            let line_idx = self.scroll_line + i;
            if line_idx >= self.lines.len() {
                break;
            }
            let line = &self.lines[line_idx];
            let start = self.scroll_col.min(line.len());
            let end = (start + visible_cols).min(line.len());
            let display = &line[start..end];
            objects.push(text_render_object_with_width(
                base_id ^ 0x1000 ^ line_idx as u64,
                Rect::new(
                    text_x,
                    self.rect.y + 5 + (i * self.line_height) as i32,
                    (self.rect.width - 10).max(1),
                    self.line_height as i32,
                ),
                display,
                Theme::TEXT_PRIMARY.to_u32(),
                false,
                DamageLane::Text,
                2,
            ));
        }

        if self.focused {
            let cursor_screen_line = self.cursor_line.saturating_sub(self.scroll_line);
            let cursor_screen_col = self.cursor_col.saturating_sub(self.scroll_col);
            let cursor_x = text_x + (cursor_screen_col as i32 * 8);
            let cursor_y = self.rect.y + 5 + (cursor_screen_line * self.line_height) as i32;
            if cursor_x < self.rect.x + self.rect.width - 5 && cursor_y < self.rect.y + self.rect.height - 5 {
                objects.push(solid_rect_object(
                    base_id ^ 0x2000,
                    Rect::new(cursor_x, cursor_y, 1, 16),
                    Theme::TEXT_PRIMARY.to_u32(),
                    DamageLane::Cursor,
                    3,
                ));
            }
        }

        objects
    }
}

impl Widget for TextArea {
    /// Metin alanını çizer.
    /// Sırasıyla: arka plan → kenarlık → görünür satırlar → imleç.
    /// Yalnızca `scroll_line`'dan itibaren görünür satır sayısı kadar satır çizilir.
    fn draw(&self, fb: &mut Framebuffer) {
        let objects = self.render_primitives();
        draw_render_objects(fb, self.rect, &objects);
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
            0x48 => {
                // Up arrow — yukarı satıra geç; sütunu mevcut satırın uzunluğuna kırp
                if self.cursor_line > 0 {
                    self.cursor_line -= 1;
                    self.cursor_col = self.cursor_col.min(self.lines[self.cursor_line].len());
                    self.update_scroll();
                }
            }
            0x50 => {
                // Down arrow — aşağı satıra geç; sütunu kırp
                if self.cursor_line < self.lines.len() - 1 {
                    self.cursor_line += 1;
                    self.cursor_col = self.cursor_col.min(self.lines[self.cursor_line].len());
                    self.update_scroll();
                }
            }
            0x4B => {
                // Left arrow — solu sola bir karakter kaydır
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                    self.update_scroll();
                }
            }
            0x4D => {
                // Right arrow — sağa bir karakter kaydır
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

    fn can_focus(&self) -> bool {
        true
    }

    fn accessibility_info(&self) -> AccessibilityInfo<'_> {
        let mut state = AccessState::empty();
        if self.focused {
            state = state.with(AccessState::FOCUSED);
        }
        AccessibilityInfo {
            role: AccessRole::TextInput,
            label: "textarea",
            value: "",
            state,
        }
    }

    fn render_objects(&self) -> Vec<RenderObject> {
        self.render_primitives()
    }
}
