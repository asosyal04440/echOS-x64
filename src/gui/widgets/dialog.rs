//! # echOS Diyalog Widget'ları
//!
//! Kullanıcı etkileşimi için Dialog, MessageBox ve FileDialog bileşenleri.
//!
//! ## Modal Diyalog Nedir?
//!
//! Modal diyalog, kullanıcının kapatmadan diğer pencerelere geçemeyeceği
//! açılır pencerelerdir. `visible` bayrağı ile görünürlük kontrol edilir;
//! görünür olmayan diyaloglar `draw` ve `on_click` çağrılarını işlemez.
//!
//! ## DialogResult Enum'u
//!
//! Kullanıcının hangi butona tıkladığını temsil eder. `#[derive(Copy)]` sayesinde
//! sonuç değeri kopyalanabilir; bu, callback ve döndürme değeri olarak
//! kullanımı kolaylaştırır.
//!
//! ## Sürüklenebilir Pencereler
//!
//! `dragging` ve `drag_offset` alanları başlık çubuğundan sürükleme için
//! kullanılır. `on_drag(dx, dy)` çağrıldığında pencere konumu güncellenir.
//! `drag_offset` tıklama noktasının pencere sol üst köşesine göre farkını
//! tutar; bu sayede sürükleme sırasında pencere imleç altında sabit kalır.

use super::{
    border_rect_objects, draw_render_objects, solid_rect_object, text_render_object_with_width,
    Rect, Widget, MOD_CTRL,
};
use crate::gop::framebuffer::Framebuffer;
use crate::gui::protocol::{DamageLane, RenderObject};
use crate::gui::theme::Theme;
use crate::gui::widgets::button::Button;
use crate::gui::widgets::label::Label;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Diyalog işlem sonucu; kullanıcının hangi butona bastığını temsil eder.
///
/// `None`: Diyalog henüz kapatılmamış veya bir işlem seçilmemiş.
/// `Copy + Clone`: Enum varyantları heap allocation gerektirmez; değer semantiğiyle
/// serbestçe kopyalanabilir. Bu, callback parametresi ve döndürme değeri olarak
/// kullanımı kolaylaştırır.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogResult {
    Ok,
    Cancel,
    Yes,
    No,
    Retry,
    Abort,
    Ignore,
    None,
}

/// Mesaj kutusu türü; ikon ve buton yapısını belirler.
///
/// `Info` ve `Warning`: tek "OK" butonu gösterir.
/// `Error`: kullanıcıyı bir hata hakkında bilgilendirir, tek "OK" butonu.
/// `Question`: "Yes" ve "No" butonları gösterir; kullanıcıdan onay ister.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageBoxType {
    Info,
    Warning,
    Error,
    Question,
}

/// Modal diyalog widget'ı; özelleştirilebilir içerik ve butonlarla açılır pencere.
///
/// `buttons: Vec<(String, DialogResult)>` dinamik buton listesi tutar; her öğe
/// buton metni ve tıklandığında üretilecek sonucu içerir. Bu yapı, diyalogu
/// oluşturan kodu diyalog sonucunu işleyen koddan ayırır.
///
/// `content_widgets: Vec<Box<dyn Widget + 'a>>` gövdeye özel widget'lar ekler;
/// bu diyaloğun içini tamamen özelleştirilebilir kılar.
pub struct Dialog<'a> {
    rect: Rect,
    title: String,
    visible: bool,
    dragging: bool,
    drag_offset: (i32, i32),
    buttons: Vec<(String, DialogResult)>,
    result: DialogResult,
    content_widgets: Vec<Box<dyn Widget + 'a>>,
    on_close: Option<fn(DialogResult)>,
    /// Modal arka plan karartması etkin mi
    modal_overlay: bool,
    /// Ekran boyutları (modal overlay için)
    screen_width: usize,
    screen_height: usize,
    /// Odaklı buton indeksi (Tab ile geçiş)
    focused_button: usize,
}

impl<'a> Dialog<'a> {
    /// Yeni diyalog oluşturur; ekranda gizli başlar.
    ///
    /// `show()` çağrılana kadar `visible = false`; bu sayede diyalog önce
    /// yapılandırılır sonra gösterilir. `drag_offset: (0, 0)` tuple sözdizimi
    /// iki i32 değerini gruplandırır.
    pub fn new(title: &str, width: i32, height: i32) -> Self {
        Self {
            rect: Rect::new(0, 0, width, height),
            title: String::from(title),
            visible: false,
            dragging: false,
            drag_offset: (0, 0),
            buttons: Vec::new(),
            result: DialogResult::None,
            content_widgets: Vec::new(),
            on_close: None,
            modal_overlay: true,
            screen_width: 0,
            screen_height: 0,
            focused_button: 0,
        }
    }

    /// Builder: diyaloğa buton ekler; metin ve tıklama sonucuyla.
    pub fn add_button(mut self, text: &str, result: DialogResult) -> Self {
        self.buttons.push((String::from(text), result));
        self
    }

    /// Builder: gövdeye özel widget ekler.
    pub fn add_widget(mut self, widget: Box<dyn Widget + 'a>) -> Self {
        self.content_widgets.push(widget);
        self
    }

    /// Builder: kapanma callback'i ekler.
    pub fn with_close_handler(mut self, handler: fn(DialogResult)) -> Self {
        self.on_close = Some(handler);
        self
    }

    /// Diyaloğu ekranda ortaya hizalayarak gösterir.
    ///
    /// `(ekran_genişliği - diyalog_genişliği) / 2` formülü yatay merkez verir.
    /// `.max(0)` ile negatif koordinat engellenir; diyalog küçük ekranlarda
    /// sol üst köşeden taşmaz.
    pub fn show(&mut self, screen_width: usize, screen_height: usize) {
        // Center on screen
        self.rect.x = ((screen_width as i32 - self.rect.width) / 2).max(0);
        self.rect.y = ((screen_height as i32 - self.rect.height) / 2).max(0);
        self.visible = true;
        self.result = DialogResult::None;
        self.screen_width = screen_width;
        self.screen_height = screen_height;
        self.focused_button = 0;
    }

    /// Diyaloğu gizler.
    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// Diyalog görünür mü?
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Son kullanıcı işleminin sonucunu döndürür.
    pub fn result(&self) -> DialogResult {
        self.result
    }

    /// Başlık çubuğu piksel yüksekliği sabit değer döndürür.
    fn titlebar_height(&self) -> i32 {
        28
    }

    /// Verilen koordinat başlık çubuğu alanında mı?
    ///
    /// Pencerenin sürüklenip sürüklenmeyeceğini belirler; yalnızca başlık
    /// çubuğuna tıklanırsa sürükleme başlar.
    fn is_titlebar_hit(&self, x: i32, y: i32) -> bool {
        y >= self.rect.y
            && y < self.rect.y + self.titlebar_height()
            && x >= self.rect.x
            && x < self.rect.x + self.rect.width
    }

    /// Kapatma düğmesinin dikdörtgenini hesaplar.
    ///
    /// Sağ üst köşeye 24 piksel soldan yerleştirilir, 4 piksel üstten offset ile.
    fn close_button_rect(&self) -> Rect {
        Rect::new(self.rect.x + self.rect.width - 24, self.rect.y + 4, 20, 20)
    }

    /// Belirtilen indeksteki butonun dikdörtgenini hesaplar.
    ///
    /// Butonlar yatayda ortaya hizalanır. Her buton 80 piksel genişliğinde,
    /// aralarında 10 piksel boşluk var. `total_width` hesaplaması:
    /// `buton_sayısı * genişlik + (buton_sayısı - 1) * boşluk`.
    /// `start_x` tüm grubu ortalar. Bu yaygın "centered button row" gerçeklenimidir.
    fn button_rect(&self, index: usize) -> Rect {
        let button_width = 80;
        let button_height = 28;
        let spacing = 10;
        let total_width = (self.buttons.len() as i32 * button_width)
            + ((self.buttons.len() - 1) as i32 * spacing);
        let start_x = self.rect.x + (self.rect.width - total_width) / 2;
        let button_y = self.rect.y + self.rect.height - button_height - 15;

        Rect::new(
            start_x + (index as i32 * (button_width + spacing)),
            button_y,
            button_width,
            button_height,
        )
    }

    fn draw_bounds(&self) -> Rect {
        if self.visible && self.modal_overlay && self.screen_width > 0 && self.screen_height > 0 {
            Rect::new(0, 0, self.screen_width as i32, self.screen_height as i32)
        } else {
            self.rect
        }
    }

    fn render_primitives(&self) -> Vec<RenderObject> {
        if !self.visible {
            return Vec::new();
        }

        let mut objects = Vec::new();
        let base_id = ((self.rect.x as u64) << 32) ^ (self.rect.y as u64);

        if self.modal_overlay && self.screen_width > 0 && self.screen_height > 0 {
            objects.push(solid_rect_object(
                base_id ^ 0x01,
                Rect::new(0, 0, self.screen_width as i32, self.screen_height as i32),
                0x00181818,
                DamageLane::Shell,
                0,
            ));
        }

        objects.push(solid_rect_object(
            base_id ^ 0x02,
            Rect::new(
                self.rect.x + 6,
                self.rect.y + 6,
                self.rect.width,
                self.rect.height,
            ),
            Theme::SHADOW.to_u32(),
            DamageLane::Shell,
            1,
        ));
        objects.push(solid_rect_object(
            base_id ^ 0x03,
            self.rect,
            Theme::WINDOW_BG.to_u32(),
            DamageLane::Shell,
            2,
        ));
        objects.push(solid_rect_object(
            base_id ^ 0x04,
            Rect::new(
                self.rect.x,
                self.rect.y,
                self.rect.width,
                self.titlebar_height(),
            ),
            Theme::TITLEBAR_ACTIVE.to_u32(),
            DamageLane::Shell,
            3,
        ));
        objects.extend(border_rect_objects(
            base_id ^ 0x05,
            self.rect,
            Theme::BORDER.to_u32(),
            DamageLane::Shell,
            4,
        ));
        objects.push(text_render_object_with_width(
            base_id ^ 0x06,
            Rect::new(
                self.rect.x + 10,
                self.rect.y + 6,
                (self.rect.width - 20).max(1),
                18,
            ),
            &self.title,
            Theme::TEXT_PRIMARY.to_u32(),
            false,
            DamageLane::Text,
            5,
        ));

        let close_rect = self.close_button_rect();
        objects.push(solid_rect_object(
            base_id ^ 0x07,
            close_rect,
            Theme::ACCENT_ERROR.to_u32(),
            DamageLane::Shell,
            5,
        ));
        objects.push(text_render_object_with_width(
            base_id ^ 0x08,
            Rect::new(
                close_rect.x + 6,
                close_rect.y + 2,
                close_rect.width.max(1),
                18,
            ),
            "X",
            Theme::TEXT_PRIMARY.to_u32(),
            false,
            DamageLane::Text,
            6,
        ));

        for (i, (text, _)) in self.buttons.iter().enumerate() {
            let btn_rect = self.button_rect(i);
            let bg = if i == self.focused_button {
                Theme::ACCENT_PRIMARY.to_u32()
            } else {
                Theme::BUTTON_BG.to_u32()
            };
            objects.push(solid_rect_object(
                base_id ^ 0x100 ^ i as u64,
                btn_rect,
                bg,
                DamageLane::Shell,
                5,
            ));
            objects.extend(border_rect_objects(
                base_id ^ 0x180 ^ i as u64,
                btn_rect,
                Theme::BORDER.to_u32(),
                DamageLane::Shell,
                6,
            ));
            let text_x =
                btn_rect.x + ((btn_rect.width - (text.chars().count() as i32 * 8)).max(0) / 2);
            let text_y = btn_rect.y + ((btn_rect.height - 16).max(0) / 2);
            objects.push(text_render_object_with_width(
                base_id ^ 0x200 ^ i as u64,
                Rect::new(text_x, text_y, btn_rect.width.max(1), 18),
                text,
                Theme::TEXT_PRIMARY.to_u32(),
                false,
                DamageLane::Text,
                7,
            ));
        }

        objects
    }
}

impl<'a> Widget for Dialog<'a> {
    fn draw(&self, fb: &mut Framebuffer) {
        if !self.visible {
            return;
        }
        draw_render_objects(fb, self.draw_bounds(), &self.render_primitives());

        for widget in &self.content_widgets {
            widget.draw(fb);
        }
    }

    fn on_click(&mut self, x: i32, y: i32) -> bool {
        // Görünmez diyalog tıklamaları işlemez
        if !self.visible {
            return false;
        }

        // Kapatma düğmesine tıklandı: Cancel sonucu üret, callback tetikle, gizle
        if self.close_button_rect().contains(x, y) {
            self.result = DialogResult::Cancel;
            if let Some(handler) = self.on_close {
                handler(self.result);
            }
            self.hide();
            return true;
        }

        // Alt butonları kontrol et: hangi butona tıklandıysa sonucu ayarla
        for (i, (_, result)) in self.buttons.iter().enumerate() {
            if self.button_rect(i).contains(x, y) {
                self.result = *result;
                if let Some(handler) = self.on_close {
                    handler(self.result);
                }
                self.hide();
                return true;
            }
        }

        // Başlık çubuğuna tıklanırsa sürükleme başlat; offset'i kaydet
        if self.is_titlebar_hit(x, y) {
            self.dragging = true;
            self.drag_offset = (x - self.rect.x, y - self.rect.y);
            return true;
        }

        // İçerik widget'larına tıklamayı ilet
        for widget in &mut self.content_widgets {
            if widget.on_click(x, y) {
                return true;
            }
        }

        self.rect.contains(x, y)
    }

    /// Sürükleme ile pencereyi taşır.
    ///
    /// `dx`/`dy` delta değerleridir; `rect.x` ve `rect.y` doğrudan güncellenir.
    /// İlerleyen geliştirmelerde ekran sınırlarını aşmamak için clamp eklenebilir.
    fn on_drag(&mut self, dx: i32, dy: i32) -> bool {
        if self.dragging {
            self.rect.x += dx;
            self.rect.y += dy;
            true
        } else {
            false
        }
    }

    /// Klavye olayını işler.
    /// ESC: diyaloğu Cancel ile kapatır.
    /// Tab: butonlar arasında odak döngüsü (focus trap).
    /// Enter/Space: odaklı butonu tetikler.
    fn on_key(&mut self, _key: char, _modifiers: u8, scancode: u8) -> bool {
        if !self.visible {
            return false;
        }

        match scancode {
            0x01 => {
                // ESC: DialogResult::Cancel ile kapat
                self.result = DialogResult::Cancel;
                if let Some(handler) = self.on_close {
                    handler(self.result);
                }
                self.hide();
                true
            }
            0x0F => {
                // Tab: butonlar arasında döngü (focus trap)
                if !self.buttons.is_empty() {
                    self.focused_button = (self.focused_button + 1) % self.buttons.len();
                }
                true
            }
            0x1C => {
                // Enter: odaklı butonu tetikle
                if self.focused_button < self.buttons.len() {
                    self.result = self.buttons[self.focused_button].1;
                    if let Some(handler) = self.on_close {
                        handler(self.result);
                    }
                    self.hide();
                }
                true
            }
            _ => {
                // İçerik widget'larına ilet
                for widget in &mut self.content_widgets {
                    if widget.on_key(_key, _modifiers, scancode) {
                        return true;
                    }
                }
                // Diyalog görünürken tüm tuşları yutar (focus trap)
                true
            }
        }
    }

    fn bounds(&self) -> Rect {
        self.rect
    }

    fn render_objects(&self) -> Vec<RenderObject> {
        self.render_primitives()
    }
}

/// Hazır mesaj kutusu widget'ı; bilgi/uyarı/hata/soru diyaloğu.
///
/// `Dialog<'static>` kullanılır çünkü dahili butonlar sabit string literallerinden
/// ('static lifetime) oluşur. `MessageBox`, `Dialog`'u sarmalar ve mesaj türüne
/// göre standart butonları otomatik ekler; bu "facade" tasarım kalıbıdır.
pub struct MessageBox {
    dialog: Dialog<'static>,
    message: String,
    msg_type: MessageBoxType,
}

impl MessageBox {
    /// Yeni mesaj kutusu oluşturur; türe uygun butonları otomatik ekler.
    ///
    /// `match msg_type` ile bilgi/uyarı/hata türleri tek "OK" butonu alırken
    /// soru türü "Yes"/"No" buton çifti alır. Bu otomatik buton ataması
    /// standart işletim sistemi mesaj kutusu davranışını taklit eder.
    pub fn new(title: &str, message: &str, msg_type: MessageBoxType) -> Self {
        let width = 400;
        let height = 150;

        let mut dialog = Dialog::new(title, width, height);

        // Mesaj türüne göre uygun butonları ekle
        match msg_type {
            MessageBoxType::Info | MessageBoxType::Warning | MessageBoxType::Error => {
                dialog = dialog.add_button("OK", DialogResult::Ok);
            }
            MessageBoxType::Question => {
                dialog = dialog.add_button("Yes", DialogResult::Yes);
                dialog = dialog.add_button("No", DialogResult::No);
            }
        }

        Self {
            dialog,
            message: String::from(message),
            msg_type,
        }
    }

    /// Ekranda gösterir.
    pub fn show(&mut self, screen_width: usize, screen_height: usize) {
        self.dialog.show(screen_width, screen_height);
    }

    /// Gizler.
    pub fn hide(&mut self) {
        self.dialog.hide();
    }

    /// Görünür mü?
    pub fn is_visible(&self) -> bool {
        self.dialog.is_visible()
    }

    /// Son sonucu döndürür.
    pub fn result(&self) -> DialogResult {
        self.dialog.result()
    }

    /// Mesaj türüne göre ikon karakterini döndürür.
    ///
    /// `'static str` döndürür; string literalleri 'static ömürlüdür.
    /// Gerçek GUI'lerde vektörel ikon SVG veya bitmap kullanılır; burada
    /// tek karakter basit temsil sağlar.
    fn icon_char(&self) -> &'static str {
        match self.msg_type {
            MessageBoxType::Info => "i",
            MessageBoxType::Warning => "!",
            MessageBoxType::Error => "X",
            MessageBoxType::Question => "?",
        }
    }

    /// Mesaj türüne göre ikon rengini döndürür.
    fn icon_color(&self) -> u32 {
        match self.msg_type {
            MessageBoxType::Info => Theme::ACCENT_PRIMARY.to_u32(),
            MessageBoxType::Warning => Theme::ACCENT_WARNING.to_u32(),
            MessageBoxType::Error => Theme::ACCENT_ERROR.to_u32(),
            MessageBoxType::Question => Theme::TEXT_ACCENT.to_u32(),
        }
    }
}

impl Widget for MessageBox {
    fn draw(&self, fb: &mut Framebuffer) {
        if !self.dialog.is_visible() {
            return;
        }
        draw_render_objects(fb, self.dialog.draw_bounds(), &self.render_objects());
    }

    fn on_click(&mut self, x: i32, y: i32) -> bool {
        self.dialog.on_click(x, y)
    }

    fn on_drag(&mut self, dx: i32, dy: i32) -> bool {
        self.dialog.on_drag(dx, dy)
    }

    fn bounds(&self) -> Rect {
        self.dialog.bounds()
    }

    fn render_objects(&self) -> Vec<RenderObject> {
        if !self.dialog.is_visible() {
            return Vec::new();
        }

        let mut objects = self.dialog.render_primitives();
        let base_id = ((self.dialog.rect.x as u64) << 32) ^ (self.dialog.rect.y as u64) ^ 0x4000;
        let icon_rect = Rect::new(self.dialog.rect.x + 20, self.dialog.rect.y + 50, 32, 32);
        objects.push(solid_rect_object(
            base_id,
            icon_rect,
            self.icon_color(),
            DamageLane::Shell,
            8,
        ));
        objects.push(text_render_object_with_width(
            base_id ^ 1,
            Rect::new(icon_rect.x + 12, icon_rect.y + 8, 12, 18),
            self.icon_char(),
            Theme::TEXT_PRIMARY.to_u32(),
            false,
            DamageLane::Text,
            9,
        ));

        let max_chars = ((self.dialog.rect.width - 85).max(8) / 8) as usize;
        let mut line_y = self.dialog.rect.y + 50;
        for line in self.message.split('\n') {
            let mut start = 0usize;
            while start < line.len() {
                let end = (start + max_chars).min(line.len());
                objects.push(text_render_object_with_width(
                    base_id ^ 0x100 ^ line_y as u64,
                    Rect::new(
                        self.dialog.rect.x + 65,
                        line_y,
                        (self.dialog.rect.width - 85).max(1),
                        18,
                    ),
                    &line[start..end],
                    Theme::TEXT_PRIMARY.to_u32(),
                    false,
                    DamageLane::Text,
                    9,
                ));
                line_y += 18;
                start = end;
            }
            if line.is_empty() {
                line_y += 18;
            }
        }
        objects
    }
}

/// Dosya diyaloğu türü: açma, kaydetme veya klasör seçimi.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileDialogType {
    Open,
    Save,
    SelectFolder,
}

/// Dosya diyaloğu widget'ı.
///
/// Diyalog mevcut yolu gerçek dosya sisteminden okuyup satırları dizin ve dosya
/// olarak yayınlar. Dizinler `/` sonekiyle gösterilir ve tıklanınca içine
/// girilir; dosyalar seçilip ad alanına kopyalanır.
pub struct FileDialog {
    dialog: Dialog<'static>,
    dialog_type: FileDialogType,
    current_path: String,
    files: Vec<String>,
    selected_file: Option<usize>,
    filename_input: String,
}

impl FileDialog {
    /// Yeni dosya diyaloğu oluşturur; türe göre başlık ve butonlar ayarlanır.
    pub fn new(dialog_type: FileDialogType) -> Self {
        let title = match dialog_type {
            FileDialogType::Open => "Open File",
            FileDialogType::Save => "Save File",
            FileDialogType::SelectFolder => "Select Folder",
        };

        let mut dialog = Dialog::new(title, 500, 400);
        dialog = dialog.add_button("Cancel", DialogResult::Cancel);
        dialog = dialog.add_button("Open", DialogResult::Ok);

        let mut dialog_widget = Self {
            dialog,
            dialog_type,
            current_path: String::from("/"),
            files: Vec::new(),
            selected_file: None,
            filename_input: String::new(),
        };
        dialog_widget.reload_filesystem_entries();
        dialog_widget
    }

    /// Mevcut dizin yolunu ayarlar.
    pub fn set_path(&mut self, path: &str) {
        self.current_path = normalize_dialog_path(path);
        self.selected_file = None;
        if self.dialog_type == FileDialogType::SelectFolder {
            self.filename_input = self.current_path.clone();
        } else {
            self.filename_input.clear();
        }
        self.reload_filesystem_entries();
    }

    /// Gösterilecek dosya listesini ayarlar.
    pub fn set_files(&mut self, files: Vec<String>) {
        self.files = files;
        self.selected_file = None;
    }

    /// Seçili dosya adını döndürür; seçim yapılmadıysa None.
    ///
    /// `and_then(|i| self.files.get(i))`: Option zinciri; indeks geçerliyse
    /// dosya adına erişir. `map(|s| s.as_str())`: String'i &str'ye dönüştürür.
    pub fn selected_file(&self) -> Option<&str> {
        self.selected_file.and_then(|i| self.files.get(i)).map(|s| {
            if let Some(value) = s.strip_suffix('/') {
                value
            } else {
                s.as_str()
            }
        })
    }

    /// Dosya adı giriş alanındaki metni döndürür.
    pub fn filename(&self) -> &str {
        &self.filename_input
    }

    /// Diyaloğu ekranda gösterir.
    pub fn show(&mut self, screen_width: usize, screen_height: usize) {
        self.reload_filesystem_entries();
        self.dialog.show(screen_width, screen_height);
    }

    /// Diyaloğu gizler.
    pub fn hide(&mut self) {
        self.dialog.hide();
    }

    /// Diyalog görünür mü?
    pub fn is_visible(&self) -> bool {
        self.dialog.is_visible()
    }

    /// Son sonucu döndürür.
    pub fn result(&self) -> DialogResult {
        self.dialog.result()
    }

    /// Dosya listesinin görüntülendiği dikdörtgeni hesaplar.
    fn file_list_rect(&self) -> Rect {
        Rect::new(
            self.dialog.rect.x + 10,
            self.dialog.rect.y + 60,
            self.dialog.rect.width - 20,
            250,
        )
    }

    /// Dosya adı metin girişinin dikdörtgenini hesaplar.
    fn filename_rect(&self) -> Rect {
        Rect::new(
            self.dialog.rect.x + 100,
            self.dialog.rect.y + self.dialog.rect.height - 60,
            self.dialog.rect.width - 110,
            24,
        )
    }

    fn reload_filesystem_entries(&mut self) {
        let mut entries = crate::fs::read_dir(&self.current_path)
            .unwrap_or_default()
            .into_iter()
            .map(|(name, is_dir)| if is_dir { format!("{}/", name) } else { name })
            .collect::<Vec<_>>();
        entries.sort();
        if self.current_path != "/" {
            entries.insert(0, String::from("../"));
        }
        self.files = entries;
        if self.dialog_type == FileDialogType::SelectFolder && self.filename_input.is_empty() {
            self.filename_input = self.current_path.clone();
        }
    }

    fn activate_entry(&mut self, index: usize) {
        let Some(entry) = self.files.get(index).cloned() else {
            return;
        };
        self.selected_file = Some(index);
        if entry == "../" {
            self.current_path = dialog_parent_path(&self.current_path);
            self.filename_input = if self.dialog_type == FileDialogType::SelectFolder {
                self.current_path.clone()
            } else {
                String::new()
            };
            self.selected_file = None;
            self.reload_filesystem_entries();
            return;
        }

        if let Some(name) = entry.strip_suffix('/') {
            self.current_path = dialog_join_path(&self.current_path, name);
            self.filename_input = if self.dialog_type == FileDialogType::SelectFolder {
                self.current_path.clone()
            } else {
                String::new()
            };
            self.selected_file = None;
            self.reload_filesystem_entries();
            return;
        }

        self.filename_input = dialog_join_path(&self.current_path, &entry);
    }
}

fn normalize_dialog_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        String::from("/")
    } else if trimmed.starts_with('/') {
        trimmed.trim_end_matches('/').to_string().if_empty_root()
    } else {
        format!("/{}", trimmed.trim_end_matches('/')).if_empty_root()
    }
}

fn dialog_parent_path(path: &str) -> String {
    let normalized = normalize_dialog_path(path);
    if normalized == "/" {
        return normalized;
    }
    match normalized.rfind('/') {
        Some(0) | None => String::from("/"),
        Some(index) => normalized[..index].to_string(),
    }
}

fn dialog_join_path(parent: &str, name: &str) -> String {
    let parent = normalize_dialog_path(parent);
    let name = name.trim_matches('/');
    if parent == "/" {
        format!("/{}", name)
    } else {
        format!("{}/{}", parent, name)
    }
}

trait FileDialogPathExt {
    fn if_empty_root(self) -> String;
}

impl FileDialogPathExt for String {
    fn if_empty_root(self) -> String {
        if self.is_empty() {
            String::from("/")
        } else {
            self
        }
    }
}

impl Widget for FileDialog {
    fn draw(&self, fb: &mut Framebuffer) {
        if !self.dialog.is_visible() {
            return;
        }
        draw_render_objects(fb, self.dialog.draw_bounds(), &self.render_objects());
    }

    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if !self.dialog.is_visible() {
            return false;
        }

        // Dosya listesine tıklanırsa seçimi güncelle ve dosya adını kopyala
        let list_rect = self.file_list_rect();
        if list_rect.contains(x, y) {
            let relative_y = y - list_rect.y - 5;
            let index = (relative_y / 20) as usize;
            if index < self.files.len() {
                self.activate_entry(index);
            }
            return true;
        }

        self.dialog.on_click(x, y)
    }

    /// Klavye girişini işler: dosya adı alanına karakter ekler/siler.
    ///
    /// `scancode == 0x0E`: IBM PC PS/2 klavye standardında Backspace tuşunun
    /// tarama kodu. `pop()` son karakteri siler ve döndürür (Option<char>).
    /// `MOD_CTRL` kontrolü: Ctrl kombinasyonları (Ctrl+C, Ctrl+V vb.) karakter
    /// olarak eklenmez; yalnızca düz karakterler girilir.
    fn on_key(&mut self, key: char, modifiers: u8, scancode: u8) -> bool {
        if !self.dialog.is_visible() {
            return false;
        }

        // Dosya adı giriş alanına klavye girişi işle
        if self
            .dialog
            .rect
            .contains(self.filename_rect().x, self.filename_rect().y)
        {
            if scancode == 0x0E && !self.filename_input.is_empty() {
                // Backspace
                self.filename_input.pop();
                return true;
            } else if key != '\0' && (modifiers & MOD_CTRL) == 0 {
                self.filename_input.push(key);
                return true;
            }
        }
        false
    }

    fn bounds(&self) -> Rect {
        self.dialog.bounds()
    }

    fn render_objects(&self) -> Vec<RenderObject> {
        if !self.dialog.is_visible() {
            return Vec::new();
        }

        let mut objects = self.dialog.render_primitives();
        let base_id = ((self.dialog.rect.x as u64) << 32) ^ (self.dialog.rect.y as u64) ^ 0x8000;
        let path_rect = Rect::new(
            self.dialog.rect.x + 10,
            self.dialog.rect.y + 35,
            self.dialog.rect.width - 20,
            20,
        );
        objects.push(solid_rect_object(
            base_id,
            path_rect,
            Theme::BUTTON_BG.to_u32(),
            DamageLane::Shell,
            8,
        ));
        objects.push(text_render_object_with_width(
            base_id ^ 1,
            Rect::new(
                path_rect.x + 5,
                path_rect.y + 2,
                (path_rect.width - 10).max(1),
                18,
            ),
            &self.current_path,
            Theme::TEXT_SECONDARY.to_u32(),
            false,
            DamageLane::Text,
            9,
        ));

        let list_rect = self.file_list_rect();
        objects.push(solid_rect_object(
            base_id ^ 2,
            list_rect,
            Theme::BUTTON_BG.to_u32(),
            DamageLane::Shell,
            8,
        ));
        let mut file_y = list_rect.y + 5;
        for (i, file) in self.files.iter().enumerate() {
            if file_y + 18 > list_rect.y + list_rect.height {
                break;
            }
            let row_rect = Rect::new(list_rect.x + 2, file_y, list_rect.width - 4, 18);
            let bg_color = if self.selected_file == Some(i) {
                Theme::ACCENT_PRIMARY.to_u32()
            } else {
                Theme::BUTTON_BG.to_u32()
            };
            let text_color = if self.selected_file == Some(i) {
                Theme::DESKTOP_BG.to_u32()
            } else {
                Theme::TEXT_PRIMARY.to_u32()
            };
            objects.push(solid_rect_object(
                base_id ^ 0x100 ^ i as u64,
                row_rect,
                bg_color,
                DamageLane::Shell,
                9,
            ));
            objects.push(text_render_object_with_width(
                base_id ^ 0x180 ^ i as u64,
                Rect::new(
                    row_rect.x + 3,
                    row_rect.y + 1,
                    (row_rect.width - 6).max(1),
                    18,
                ),
                file,
                text_color,
                false,
                DamageLane::Text,
                10,
            ));
            file_y += 20;
        }

        objects.push(text_render_object_with_width(
            base_id ^ 3,
            Rect::new(
                self.dialog.rect.x + 10,
                self.dialog.rect.y + self.dialog.rect.height - 55,
                90,
                18,
            ),
            "Filename:",
            Theme::TEXT_PRIMARY.to_u32(),
            false,
            DamageLane::Text,
            9,
        ));
        let filename_rect = self.filename_rect();
        objects.push(solid_rect_object(
            base_id ^ 4,
            filename_rect,
            Theme::WINDOW_BG.to_u32(),
            DamageLane::Shell,
            8,
        ));
        objects.extend(border_rect_objects(
            base_id ^ 5,
            filename_rect,
            Theme::BORDER.to_u32(),
            DamageLane::Shell,
            9,
        ));
        objects.push(text_render_object_with_width(
            base_id ^ 6,
            Rect::new(
                filename_rect.x + 5,
                filename_rect.y + 4,
                (filename_rect.width - 10).max(1),
                18,
            ),
            &self.filename_input,
            Theme::TEXT_PRIMARY.to_u32(),
            false,
            DamageLane::Text,
            10,
        ));
        objects
    }
}
