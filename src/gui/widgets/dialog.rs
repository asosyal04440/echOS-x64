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

use super::{Rect, Widget, MOD_CTRL};
use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::Theme;
use crate::gui::widgets::button::Button;
use crate::gui::widgets::label::Label;
use alloc::boxed::Box;
use alloc::string::String;
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
        y >= self.rect.y && y < self.rect.y + self.titlebar_height()
            && x >= self.rect.x && x < self.rect.x + self.rect.width
    }

    /// Kapatma düğmesinin dikdörtgenini hesaplar.
    ///
    /// Sağ üst köşeye 24 piksel soldan yerleştirilir, 4 piksel üstten offset ile.
    fn close_button_rect(&self) -> Rect {
        Rect::new(
            self.rect.x + self.rect.width - 24,
            self.rect.y + 4,
            20,
            20,
        )
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
        let total_width = (self.buttons.len() as i32 * button_width) + ((self.buttons.len() - 1) as i32 * spacing);
        let start_x = self.rect.x + (self.rect.width - total_width) / 2;
        let button_y = self.rect.y + self.rect.height - button_height - 15;

        Rect::new(
            start_x + (index as i32 * (button_width + spacing)),
            button_y,
            button_width,
            button_height,
        )
    }
}

impl<'a> Widget for Dialog<'a> {
    fn draw(&self, fb: &mut Framebuffer) {
        // Görünmez diyalog hiçbir şey çizmez; erken çıkış optimizasyonu
        if !self.visible {
            return;
        }

        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let w = self.rect.width as usize;
        let h = self.rect.height as usize;
        let titlebar_h = self.titlebar_height() as usize;

        // Gölge efekti: diyaloğun sağ ve alt tarafına 6 piksel offset ile
        // koyu renk dikdörtgen çizilir; derinlik hissi yaratır.
        fb.draw_rect(x + 6, y + 6, w, h, Theme::SHADOW.to_u32());

        // Arka plan
        fb.draw_rect(x, y, w, h, Theme::WINDOW_BG.to_u32());

        // Başlık çubuğu: aktif pencere rengini kullanır
        fb.draw_rect(x, y, w, titlebar_h, Theme::TITLEBAR_ACTIVE.to_u32());

        // Başlık metni: sol kenara 10 piksel iç boşlukla
        fb.draw_string(x + 10, y + 6, &self.title, Theme::TEXT_PRIMARY.to_u32());

        // Kapatma düğmesi: kırmızı arka plan, "X" metni
        let close_rect = self.close_button_rect();
        fb.draw_rect(
            close_rect.x as usize,
            close_rect.y as usize,
            close_rect.width as usize,
            close_rect.height as usize,
            Theme::ACCENT_ERROR.to_u32(),
        );
        fb.draw_string(
            close_rect.x as usize + 6,
            close_rect.y as usize + 2,
            "X",
            Theme::TEXT_PRIMARY.to_u32(),
        );

        // Kenarlık: diyaloğun tüm çevresini çevreler
        for col in x..(x + w) {
            fb.plot_pixel(col, y, Theme::BORDER.to_u32());
            fb.plot_pixel(col, y + h - 1, Theme::BORDER.to_u32());
        }
        for row in y..(y + h) {
            fb.plot_pixel(x, row, Theme::BORDER.to_u32());
            fb.plot_pixel(x + w - 1, row, Theme::BORDER.to_u32());
        }

        // İçerik widget'larını çiz: her biri kendi konumunda gösterilir
        for widget in &self.content_widgets {
            widget.draw(fb);
        }

        // Alt butonları çiz: her buton için dikdörtgen, kenarlık ve metin
        for (i, (text, _)) in self.buttons.iter().enumerate() {
            let btn_rect = self.button_rect(i);
            fb.draw_rect(
                btn_rect.x as usize,
                btn_rect.y as usize,
                btn_rect.width as usize,
                btn_rect.height as usize,
                Theme::BUTTON_BG.to_u32(),
            );

            // Buton kenarlığı
            for col in btn_rect.x as usize..(btn_rect.x as usize + btn_rect.width as usize) {
                fb.plot_pixel(col, btn_rect.y as usize, Theme::BORDER.to_u32());
                fb.plot_pixel(col, btn_rect.y as usize + btn_rect.height as usize - 1, Theme::BORDER.to_u32());
            }
            for row in btn_rect.y as usize..(btn_rect.y as usize + btn_rect.height as usize) {
                fb.plot_pixel(btn_rect.x as usize, row, Theme::BORDER.to_u32());
                fb.plot_pixel(btn_rect.x as usize + btn_rect.width as usize - 1, row, Theme::BORDER.to_u32());
            }

            // Buton metni ortaya hizalı
            let text_x = btn_rect.x as usize + (btn_rect.width as usize - text.len() * 8) / 2;
            let text_y = btn_rect.y as usize + (btn_rect.height as usize - 16) / 2;
            fb.draw_string(text_x, text_y, text, Theme::TEXT_PRIMARY.to_u32());
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

    fn bounds(&self) -> Rect {
        self.rect
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

        // Temel diyaloğu çiz (arka plan, başlık, kenarlık, butonlar)
        self.dialog.draw(fb);

        // İkon: renkli kare içinde merkeze hizalı tek karakter
        let icon_x = self.dialog.rect.x + 20;
        let icon_y = self.dialog.rect.y + 50;
        fb.draw_rect(icon_x as usize, icon_y as usize, 32, 32, self.icon_color());
        fb.draw_string(icon_x as usize + 12, icon_y as usize + 8, self.icon_char(), Theme::TEXT_PRIMARY.to_u32());

        // Mesaj metni: ikonun sağında, otomatik satır kırmalı (word wrap)
        let msg_x = self.dialog.rect.x + 65;
        let msg_y = self.dialog.rect.y + 50;

        // Kelime kaydırma: `max_width` pikseli geçen satırlar bölünür.
        // `split('\n')` manuel satır sonlarını korur.
        // `start..end` slice sözdizimi ile alt dizeyi byte sınırında kesmek
        // gerekir; ASCII metinlerde her karakter 1 byte olduğundan güvenlidir.
        let max_width = self.dialog.rect.width - 85;
        let mut line_y = msg_y;
        for line in self.message.split('\n') {
            if line.len() * 8 > max_width as usize {
                // Need to wrap
                let mut start = 0;
                while start < line.len() {
                    let end = (start + (max_width as usize / 8)).min(line.len());
                    fb.draw_string(msg_x as usize, line_y as usize, &line[start..end], Theme::TEXT_PRIMARY.to_u32());
                    line_y += 18;
                    start = end;
                }
            } else {
                fb.draw_string(msg_x as usize, line_y as usize, line, Theme::TEXT_PRIMARY.to_u32());
                line_y += 18;
            }
        }
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
}

/// Dosya diyaloğu türü: açma, kaydetme veya klasör seçimi.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileDialogType {
    Open,
    Save,
    SelectFolder,
}

/// Dosya diyaloğu widget'ı (basitleştirilmiş; gerçek dosya sistemi entegrasyonu yok).
///
/// `files: Vec<String>` gösterilecek dosya adlarını tutar; bu liste dışarıdan
/// `set_files()` ile doldurulur. Gerçek bir OS'ta bu liste `/` veya seçilen
/// dizinin içeriğini okuyarak doldurulur.
///
/// `filename_input: String` kullanıcının klavyede yazdığı dosya adını tutar;
/// `on_key` metodunda karakter ekleme/silme işlenir.
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

        Self {
            dialog,
            dialog_type,
            current_path: String::from("/"),
            files: Vec::new(),
            selected_file: None,
            filename_input: String::new(),
        }
    }

    /// Mevcut dizin yolunu ayarlar.
    pub fn set_path(&mut self, path: &str) {
        self.current_path = String::from(path);
    }

    /// Gösterilecek dosya listesini ayarlar.
    pub fn set_files(&mut self, files: Vec<String>) {
        self.files = files;
    }

    /// Seçili dosya adını döndürür; seçim yapılmadıysa None.
    ///
    /// `and_then(|i| self.files.get(i))`: Option zinciri; indeks geçerliyse
    /// dosya adına erişir. `map(|s| s.as_str())`: String'i &str'ye dönüştürür.
    pub fn selected_file(&self) -> Option<&str> {
        self.selected_file.and_then(|i| self.files.get(i)).map(|s| s.as_str())
    }

    /// Dosya adı giriş alanındaki metni döndürür.
    pub fn filename(&self) -> &str {
        &self.filename_input
    }

    /// Diyaloğu ekranda gösterir.
    pub fn show(&mut self, screen_width: usize, screen_height: usize) {
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
}

impl Widget for FileDialog {
    fn draw(&self, fb: &mut Framebuffer) {
        if !self.dialog.is_visible() {
            return;
        }

        self.dialog.draw(fb);

        // Yol çubuğu: mevcut dizin yolunu gösterir
        let path_y = self.dialog.rect.y + 35;
        fb.draw_rect(
            self.dialog.rect.x as usize + 10,
            path_y as usize,
            (self.dialog.rect.width - 20) as usize,
            20,
            Theme::BUTTON_BG.to_u32(),
        );
        fb.draw_string(
            self.dialog.rect.x as usize + 15,
            path_y as usize + 2,
            &self.current_path,
            Theme::TEXT_SECONDARY.to_u32(),
        );

        // Dosya listesi: her dosya 20 piksel satır yüksekliğiyle sıralanır
        let list_rect = self.file_list_rect();
        fb.draw_rect(
            list_rect.x as usize,
            list_rect.y as usize,
            list_rect.width as usize,
            list_rect.height as usize,
            Theme::BUTTON_BG.to_u32(),
        );

        // Dosyaları çiz: liste alanı dışına taşanlar atlanır
        let mut file_y = list_rect.y + 5;
        for (i, file) in self.files.iter().enumerate() {
            if file_y + 18 > list_rect.y + list_rect.height {
                break;
            }

            // Seçili dosya vurgu rengiyle gösterilir
            let bg_color = if self.selected_file == Some(i) {
                Theme::ACCENT_PRIMARY.to_u32()
            } else {
                Theme::BUTTON_BG.to_u32()
            };

            fb.draw_rect(
                list_rect.x as usize + 2,
                file_y as usize,
                (list_rect.width - 4) as usize,
                18,
                bg_color,
            );

            // Seçili satırda metin rengi ters olur (koyu zemin üzerinde açık metin)
            let text_color = if self.selected_file == Some(i) {
                Theme::DESKTOP_BG.to_u32()
            } else {
                Theme::TEXT_PRIMARY.to_u32()
            };
            fb.draw_string(list_rect.x as usize + 5, file_y as usize + 1, file, text_color);

            file_y += 20;
        }

        // Dosya adı etiketi ve giriş alanı
        fb.draw_string(
            self.dialog.rect.x as usize + 10,
            self.dialog.rect.y as usize + self.dialog.rect.height as usize - 55,
            "Filename:",
            Theme::TEXT_PRIMARY.to_u32(),
        );

        // Dosya adı giriş kutusu: kullanıcının yazdığı metni gösterir
        let filename_rect = self.filename_rect();
        fb.draw_rect(
            filename_rect.x as usize,
            filename_rect.y as usize,
            filename_rect.width as usize,
            filename_rect.height as usize,
            Theme::WINDOW_BG.to_u32(),
        );
        fb.draw_string(
            filename_rect.x as usize + 5,
            filename_rect.y as usize + 4,
            &self.filename_input,
            Theme::TEXT_PRIMARY.to_u32(),
        );
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
                self.selected_file = Some(index);
                self.filename_input = self.files[index].clone();
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
        if self.dialog.rect.contains(self.filename_rect().x, self.filename_rect().y) {
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
}
