//! # Önizleme Uygulaması
//!
//! Birden fazla belge formatını destekleyen hızlı görüntüleyici.
//! Zoom, döndürme ve açıklama (annotation) araçlarıyla donatılmış
//! macOS Quick Look benzeri bir önizleme penceresi.
//!
//! ## Desteklenen Türler
//! - Resim (PNG, JPG, GIF, BMP, SVG, vb.)
//! - PDF ve Office belgeleri
//! - Düz metin, kaynak kodu ve Markdown
//! - HTML ve RTF
//!
//! ## Mimari
//! - `PreviewDocument`: Açık belgenin tüm durumunu tutan yapı
//! - `PreviewPage`: Tek sayfa verisi (küçük resim + içerik + açıklamalar)
//! - `Annotation`: Sayfaya eklenen işaretlemeler (vurgulama, altı çizgi, vb.)
//! - `PreviewWindow`: Araç çubuğu, kenar çubuğu ve içerik alanından oluşan pencere

use alloc::boxed::Box;
use alloc::string::String;
use alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};
use crate::gui::widgets::Widget;
use crate::gui::Rect;

// ============================================================================
// ÖNİZLEME SABİTLERİ
// ============================================================================

/// Araç çubuğunun piksel cinsinden yüksekliği.
pub const TOOLBAR_HEIGHT: usize = 44;

/// Küçük resim kenar çubuğunun piksel cinsinden genişliği.
pub const SIDEBAR_WIDTH: usize = 160;

/// Durum çubuğunun piksel cinsinden yüksekliği.
pub const STATUS_BAR_HEIGHT: usize = 24;

/// Varsayılan yakınlaştırma oranı (1.0 = %100).
pub const DEFAULT_ZOOM: f32 = 1.0;

/// İzin verilen minimum yakınlaştırma oranı (%10).
pub const MIN_ZOOM: f32 = 0.1;

/// İzin verilen maksimum yakınlaştırma oranı (%1000).
pub const MAX_ZOOM: f32 = 10.0;

// ============================================================================
// BELGE TÜRÜ — DocumentType
// ============================================================================

/// Desteklenen belge türleri.
///
/// Dosya uzantısına göre belirlenir ve içeriğin nasıl
/// gösterileceğini yönetir. `from_extension` metodu, tüm
/// bilinmeyen uzantıları `Unknown` varyantına eşler; bu sayede
/// uygulamanın derleme zamanında bilinmeyen dosyalara karşı
/// güvenli biçimde çalışması sağlanır.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentType {
    Image,
    PDF,
    Text,
    Code,
    Markdown,
    HTML,
    RTF,
    Office,
    Unknown,
}

impl DocumentType {
    /// Dosya uzantısından belge türünü belirler.
    ///
    /// `|` operatörü birden çok deseni aynı kola eşler.
    /// Örneğin `"png" | "jpg" | "jpeg"` → `Image` gibi.
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg" | "tiff" | "ico" => DocumentType::Image,
            "pdf" => DocumentType::PDF,
            "txt" => DocumentType::Text,
            "rs" | "c" | "cpp" | "h" | "hpp" | "py" | "js" | "ts" | "go" | "java" | "kt" | "swift" | "rb" | "php" | "css" | "scss" | "json" | "xml" | "yaml" | "yml" | "toml" => DocumentType::Code,
            "md" | "markdown" => DocumentType::Markdown,
            "html" | "htm" => DocumentType::HTML,
            "rtf" => DocumentType::RTF,
            "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" => DocumentType::Office,
            _ => DocumentType::Unknown,
        }
    }

    /// Belge türüne ait emoji simgesi döndürür (UI'da görüntülenir).
    pub fn icon(&self) -> &'static str {
        match self {
            DocumentType::Image => "🖼",
            DocumentType::PDF => "📄",
            DocumentType::Text => "📝",
            DocumentType::Code => "💻",
            DocumentType::Markdown => "📑",
            DocumentType::HTML => "🌐",
            DocumentType::RTF => "📄",
            DocumentType::Office => "📊",
            DocumentType::Unknown => "📄",
        }
    }
}

// ============================================================================
// BELGE SAYFASI — PreviewPage
// ============================================================================

/// Bir belge içindeki tek sayfayı temsil eder.
///
/// `thumbnail` alanı `Option<Vec<u32>>` türündedir:
///  - `None` → küçük resim henüz oluşturulmadı
///  - `Some(data)` → ARGB piksel verisi hazır (genişlik × yükseklik adet u32)
///
/// `content` alanı da `Option<String>` türündedir;
/// yalnızca metin tabanlı belgeler için doldurulur.
#[derive(Clone, Debug)]
pub struct PreviewPage {
    /// Sayfa numarası (1'den başlar)
    pub number: usize,
    /// Sayfanın orijinal piksel genişliği
    pub width: usize,
    /// Sayfanın orijinal piksel yüksekliği
    pub height: usize,
    /// Küçük resim piksel verisi (hesaplanmışsa)
    pub thumbnail: Option<Vec<u32>>,
    /// Metin belgeleri için sayfa içeriği
    pub content: Option<String>,
    /// Sayfaya eklenen açıklamalar listesi
    pub annotations: Vec<Annotation>,
}

impl PreviewPage {
    pub fn new(number: usize, width: usize, height: usize) -> Self {
        PreviewPage {
            number,
            width,
            height,
            thumbnail: None,
            content: None,
            annotations: Vec::new(),
        }
    }

    /// Letter boyutunda (612×792 pt) bir metin sayfası oluşturur.
    ///
    /// Standart Letter boyutu: 8.5 × 11 inç = 612 × 792 nokta (72 dpi).
    pub fn text_page(number: usize, content: String) -> Self {
        PreviewPage {
            number,
            width: 612,  // Letter genişliği (nokta)
            height: 792, // Letter yüksekliği (nokta)
            thumbnail: None,
            content: Some(content),
            annotations: Vec::new(),
        }
    }
}

// ============================================================================
// AÇIKLAMA (ANNOTATION)
// ============================================================================

/// Sayfa üzerine eklenen işaretleme (annotation).
///
/// `rect` alanı `(x, y, genişlik, yükseklik)` biçiminde
/// float koordinatlar içerir; böylece her zoom seviyesinde
/// orantılı biçimde ölçeklenebilir.
/// `created` alanı Unix zaman damgası olarak saklanır (`u64` saniye).
#[derive(Clone, Debug)]
pub struct Annotation {
    /// Benzersiz kimlik numarası
    pub id: u32,
    /// Açıklama türü (vurgulama, altı çizgi, metin, vb.)
    pub annotation_type: AnnotationType,
    /// Konumu ve boyutu: (x, y, genişlik, yükseklik) — belge koordinatlarında
    pub rect: (f32, f32, f32, f32),
    /// Ait olduğu sayfa numarası
    pub page: usize,
    /// Metin veya not içeriği
    pub content: String,
    /// Açıklama rengi (0xAARRGGBB)
    pub color: u32,
    /// Açıklamayı oluşturan kullanıcı
    pub author: String,
    /// Oluşturulma zamanı (Unix timestamp, saniye)
    pub created: u64,
}

/// Açıklama türü seçenekleri.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnotationType {
    /// Metin vurgulama (yarı saydam renk karıştırma)
    Highlight,
    /// Metin altı çizgi
    Underline,
    /// Metin üstü çizgi
    Strikeout,
    /// Metin notu baloncuğu
    Text,
    /// Serbest el çizimi
    Freehand,
    /// Şekil (dikdörtgen, elips, vb.)
    Shape,
    /// Damga / mühür
    Stamp,
}

// ============================================================================
// BELGE — PreviewDocument
// ============================================================================

/// Önizleme penceresinde açık olan belgenin tüm durumu.
///
/// `zoom`, `rotation`, `scroll_x/y` alanları görüntüleme
/// dönüşüm parametrelerini tutar; kaynak veri değişmez.
/// Bu "non-destructive editing" (yıkıcı olmayan düzenleme) prensibidir.
#[derive(Clone, Debug)]
pub struct PreviewDocument {
    /// Dosya yolu
    pub path: String,
    /// Dosya adı (yolun son bileşeni)
    pub name: String,
    /// Belge türü
    pub doc_type: DocumentType,
    /// Toplam sayfa sayısı
    pub page_count: usize,
    /// Görüntülenen sayfa indeksi (0'dan başlar)
    pub current_page: usize,
    /// Sayfa verileri
    pub pages: Vec<PreviewPage>,
    /// Yakınlaştırma oranı (1.0 = %100)
    pub zoom: f32,
    /// Döndürme açısı: 0, 90, 180 veya 270 derece
    pub rotation: u16,
    /// Yatay kaydırma ofseti (piksel)
    pub scroll_x: usize,
    /// Dikey kaydırma ofseti (piksel)
    pub scroll_y: usize,
    /// Belge yükleniyor mu?
    pub loading: bool,
    /// Yükleme ilerleme oranı (0.0 – 1.0)
    pub loading_progress: f32,
    /// Belge değiştirildi mi?
    pub modified: bool,
    /// Meta veri (başlık, yazar, boyut, vb.)
    pub metadata: DocumentMetadata,
}

/// Belgeye ait meta veriler.
#[derive(Clone, Debug)]
pub struct DocumentMetadata {
    pub title: String,
    pub author: String,
    pub subject: String,
    pub creator: String,
    pub created: String,
    pub modified: String,
    /// Dosya boyutu (bayt cinsinden)
    pub file_size: u64,
}

impl PreviewDocument {
    /// Yeni bir belge nesnesi oluşturur; uzantıdan tür belirlenir.
    pub fn new(path: &str) -> Self {
        let name = path.rsplit('/').next().unwrap_or(path);
        let ext = name.rsplit('.').next().unwrap_or("");
        let doc_type = DocumentType::from_extension(ext);

        PreviewDocument {
            path: String::from(path),
            name: String::from(name),
            doc_type,
            page_count: 1,
            current_page: 0,
            pages: Vec::new(),
            zoom: DEFAULT_ZOOM,
            rotation: 0,
            scroll_x: 0,
            scroll_y: 0,
            loading: true,
            loading_progress: 0.0,
            modified: false,
            metadata: DocumentMetadata::default(),
        }
    }

    /// Belirtilen boyutlarda bir resim belgesi oluşturur.
    pub fn image(path: &str, width: usize, height: usize) -> Self {
        let mut doc = Self::new(path);
        doc.page_count = 1;
        doc.pages.push(PreviewPage::new(1, width, height));
        doc.loading = false;
        doc
    }

    /// Metin içerikli bir belge oluşturur.
    pub fn text(path: &str, content: &str) -> Self {
        let mut doc = Self::new(path);
        doc.page_count = 1;
        doc.pages.push(PreviewPage::text_page(1, String::from(content)));
        doc.loading = false;
        doc
    }

    /// Zoom oranını %25 artırır (maksimum `MAX_ZOOM` ile sınırlıdır).
    pub fn zoom_in(&mut self) {
        self.zoom = (self.zoom * 1.25).min(MAX_ZOOM);
    }

    /// Zoom oranını %20 azaltır (minimum `MIN_ZOOM` ile sınırlıdır).
    pub fn zoom_out(&mut self) {
        self.zoom = (self.zoom / 1.25).max(MIN_ZOOM);
    }

    /// Zoom oranını varsayılan değere (%100) sıfırlar.
    pub fn reset_zoom(&mut self) {
        self.zoom = DEFAULT_ZOOM;
    }

    /// Zoom oranını pencereye tam sığacak şekilde ayarlar.
    ///
    /// X ve Y eksenlerindeki ölçek faktörlerinin minimumu alınır;
    /// bu sayede sayfa tüm penceredeki alana tam oturur, kırpılmaz.
    pub fn fit_to_window(&mut self, window_width: usize, window_height: usize) {
        if let Some(page) = self.pages.get(self.current_page) {
            let (pw, ph) = self.get_rotated_size(page.width, page.height);
            let scale_x = window_width as f32 / pw as f32;
            let scale_y = window_height as f32 / ph as f32;
            self.zoom = scale_x.min(scale_y);
        }
    }

    /// Belgeyi sola (saat yönünün tersine) 90 derece döndürür.
    pub fn rotate_left(&mut self) {
        self.rotation = (self.rotation + 270) % 360;
    }

    /// Belgeyi sağa (saat yönünde) 90 derece döndürür.
    pub fn rotate_right(&mut self) {
        self.rotation = (self.rotation + 90) % 360;
    }

    /// Döndürme açısını dikkate alarak sayfanın efektif boyutunu hesaplar.
    ///
    /// 90° ve 270°'de genişlik ve yükseklik yer değiştirir.
    fn get_rotated_size(&self, width: usize, height: usize) -> (usize, usize) {
        match self.rotation {
            90 | 270 => (height, width),
            _ => (width, height),
        }
    }

    /// Bir sonraki sayfaya geçer; kaydırma ofseti sıfırlanır.
    pub fn next_page(&mut self) {
        if self.current_page < self.page_count - 1 {
            self.current_page += 1;
            self.scroll_y = 0;
        }
    }

    /// Önceki sayfaya geri döner; kaydırma ofseti sıfırlanır.
    pub fn prev_page(&mut self) {
        if self.current_page > 0 {
            self.current_page -= 1;
            self.scroll_y = 0;
        }
    }

    /// Belirli bir sayfaya doğrudan atlar.
    pub fn go_to_page(&mut self, page: usize) {
        if page < self.page_count {
            self.current_page = page;
            self.scroll_y = 0;
        }
    }
}

impl DocumentMetadata {
    pub fn default() -> Self {
        DocumentMetadata {
            title: String::new(),
            author: String::new(),
            subject: String::new(),
            creator: String::new(),
            created: String::new(),
            modified: String::new(),
            file_size: 0,
        }
    }
}

// ============================================================================
// ÖNİZLEME PENCERESİ — PreviewWindow
// ============================================================================

/// Önizleme uygulamasının ana penceresi.
///
/// Üç ana görüntü bölgesinden oluşur:
/// 1. **Araç çubuğu** (`TOOLBAR_HEIGHT` yüksekliğinde): gezinme, zoom, döndürme ve paylaşım
/// 2. **Kenar çubuğu** (`SIDEBAR_WIDTH` genişliğinde): küçük resim listesi
/// 3. **İçerik alanı**: belgeyi gösteren ana bölge
///
/// Slayt gösterisi modu etkinleştirildiğinde `slideshow_timer`
/// `slideshow_interval` süresine ulaşınca otomatik olarak
/// sonraki sayfaya geçilir.
pub struct PreviewWindow {
    /// Pencerenin ekran konumu ve boyutu
    pub rect: Rect,
    /// Şu an açık olan belge (yoksa None)
    pub document: Option<PreviewDocument>,
    /// Küçük resim kenar çubuğu görünür mü?
    pub show_sidebar: bool,
    /// Araç çubuğu görünür mü?
    pub show_toolbar: bool,
    /// Durum çubuğu görünür mü?
    pub show_status_bar: bool,
    /// Etkin açıklama modu (None = kapalı)
    pub annotation_mode: Option<AnnotationType>,
    /// Açıklama rengi
    pub annotation_color: u32,
    /// Tam ekran modu aktif mi?
    pub fullscreen: bool,
    /// Slayt gösterisi çalışıyor mu?
    pub slideshow: bool,
    /// Slayt gösterisi sayfa geçiş aralığı (saniye)
    pub slideshow_interval: f32,
    /// Geçen süre sayacı (saniye)
    pub slideshow_timer: f32,
    /// Fare üzerinde olan araç çubuğu düğmesi indeksi
    pub hovered_button: Option<usize>,
    /// Sürükleme aktif mi?
    pub dragging: bool,
    /// Sürükleme başlangıç noktası
    pub drag_start: (i32, i32),
}

impl PreviewWindow {
    pub fn new(rect: Rect) -> Self {
        PreviewWindow {
            rect,
            document: None,
            show_sidebar: true,
            show_toolbar: true,
            show_status_bar: true,
            annotation_mode: None,
            annotation_color: 0xFFFFFF00, // Sarı (vurgulama için)
            fullscreen: false,
            slideshow: false,
            slideshow_interval: 3.0,
            slideshow_timer: 0.0,
            hovered_button: None,
            dragging: false,
            drag_start: (0, 0),
        }
    }

    /// Belirtilen dosyayı açar; uzantıya göre uygun belge nesnesi oluşturulur.
    pub fn open_file(&mut self, path: &str) {
        // Uzantıya göre örnek belge oluştur
        let ext = path.rsplit('.').next().unwrap_or("");

        self.document = Some(match DocumentType::from_extension(ext) {
            DocumentType::Image => {
                PreviewDocument::image(path, 1920, 1080)
            }
            DocumentType::Text | DocumentType::Code | DocumentType::Markdown => {
                PreviewDocument::text(path, "Sample document content for preview.\n\nThis is a text file that would be displayed in the preview window.\n\nYou can scroll, zoom, and navigate through the content.")
            }
            _ => {
                PreviewDocument::new(path)
            }
        });
    }

    /// Belgeyi kapatır ve ilgili durumu temizler.
    pub fn close(&mut self) {
        self.document = None;
        self.fullscreen = false;
        self.slideshow = false;
    }

    /// Her kare çağrılır; slayt gösterisi ve yükleme animasyonunu günceller.
    ///
    /// `dt` (delta time): önceki kareden bu yana geçen süre (saniye).
    /// Slayt gösterisinde zamanlayıcı `dt` ile artırılır; eşik
    /// geçilince otomatik sayfa ilerlemesi tetiklenir.
    pub fn update(&mut self, dt: f32) {
        // Slayt gösterisi sayacını güncelle
        if self.slideshow {
            self.slideshow_timer += dt;
            if self.slideshow_timer >= self.slideshow_interval {
                self.slideshow_timer = 0.0;
                if let Some(ref mut doc) = self.document {
                    if doc.current_page < doc.page_count - 1 {
                        doc.next_page();
                    } else {
                        self.slideshow = false;
                    }
                }
            }
        }

        // Yükleme animasyonunu güncelle (simüle edilmiş ilerleme)
        if let Some(ref mut doc) = self.document {
            if doc.loading {
                doc.loading_progress += dt * 0.3;
                if doc.loading_progress >= 1.0 {
                    doc.loading = false;
                    doc.loading_progress = 1.0;
                }
            }
        }
    }

    /// Önizleme penceresini tamamen çizer.
    ///
    /// Çizim sırası:
    /// 1. Arka plan
    /// 2. Araç çubuğu (isteğe bağlı)
    /// 3. Kenar çubuğu (küçük resimler, isteğe bağlı)
    /// 4. İçerik alanı (belge içeriği)
    /// 5. Durum çubuğu (isteğe bağlı)
    pub fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let w = self.rect.width as usize;
        let h = self.rect.height as usize;

        // Arka plan
        fb.draw_rect(x, y, w, h, Theme::WINDOW_BG.to_u32());

        if self.document.is_none() {
            // Boş durum mesajı
            fb.draw_string(x + w / 2 - 40, y + h / 2 - 20, "No document open", Theme::TEXT_SECONDARY.to_u32());
            fb.draw_string(x + w / 2 - 60, y + h / 2, "Open a file to preview it", Theme::TEXT_SECONDARY.to_u32());
            return;
        }

        let doc = self.document.as_ref().unwrap();

        // Araç çubuğu
        if self.show_toolbar {
            fb.draw_rect(x, y, w, TOOLBAR_HEIGHT, Theme::TOOLBAR_BG.to_u32());
            self.draw_toolbar(fb, x, y, w, doc);
        }

        // Kenar çubuğu (küçük resim listesi)
        let content_x = if self.show_sidebar {
            fb.draw_rect(x, y + TOOLBAR_HEIGHT, SIDEBAR_WIDTH, h - TOOLBAR_HEIGHT - STATUS_BAR_HEIGHT, Theme::SIDEBAR_BG.to_u32());
            self.draw_sidebar(fb, x, y + TOOLBAR_HEIGHT, doc);
            x + SIDEBAR_WIDTH
        } else {
            x
        };

        let content_w = if self.show_sidebar { w - SIDEBAR_WIDTH } else { w };
        let content_h = h - TOOLBAR_HEIGHT - STATUS_BAR_HEIGHT;
        let content_y = y + TOOLBAR_HEIGHT;

        // İçerik alanı
        self.draw_content(fb, content_x, content_y, content_w, content_h, doc);

        // Durum çubuğu
        if self.show_status_bar {
            let status_y = y + h - STATUS_BAR_HEIGHT;
            fb.draw_rect(x, status_y, w, STATUS_BAR_HEIGHT, Theme::TOOLBAR_BG.to_u32());
            self.draw_status_bar(fb, x, status_y, w, doc);
        }
    }

    /// Araç çubuğunu çizer: kapat, gezinti, zoom, döndürme ve sağ taraf düğmeleri.
    fn draw_toolbar(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, doc: &PreviewDocument) {
        let mut btn_x = x + 8;

        // Kapat düğmesi
        fb.draw_rect(btn_x, y + 8, 28, 28, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(btn_x + 6, y + 12, "×", Theme::TEXT_PRIMARY.to_u32());
        btn_x += 36;

        // Gezinti düğmeleri (önceki / sonraki sayfa)
        let nav_buttons = [("◀", "prev"), ("▶", "next")];
        for (icon, _) in nav_buttons {
            fb.draw_rect(btn_x, y + 8, 28, 28, Theme::SIDEBAR_BG.to_u32());
            fb.draw_string(btn_x + 6, y + 12, icon, Theme::TEXT_PRIMARY.to_u32());
            btn_x += 32;
        }

        // Zoom küçült düğmesi
        btn_x += 8;
        fb.draw_rect(btn_x, y + 8, 28, 28, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(btn_x + 6, y + 12, "−", Theme::TEXT_PRIMARY.to_u32());
        btn_x += 32;

        // Zoom yüzde göstergesi (%100 gibi)
        let zoom_text = format!("{:.0}%", doc.zoom * 100.0);
        fb.draw_string(btn_x, y + 12, &zoom_text, Theme::TEXT_PRIMARY.to_u32());
        btn_x += zoom_text.len() * 8 + 8;

        // Zoom büyüt düğmesi
        fb.draw_rect(btn_x, y + 8, 28, 28, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(btn_x + 6, y + 12, "+", Theme::TEXT_PRIMARY.to_u32());
        btn_x += 36;

        // Sola döndür düğmesi
        fb.draw_rect(btn_x, y + 8, 28, 28, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(btn_x + 4, y + 12, "↺", Theme::TEXT_PRIMARY.to_u32());
        btn_x += 32;

        // Sağa döndür düğmesi
        fb.draw_rect(btn_x, y + 8, 28, 28, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(btn_x + 4, y + 12, "↻", Theme::TEXT_PRIMARY.to_u32());
        btn_x += 40;

        // Belge başlığını ortada göster
        let title = if doc.name.len() > 20 { format!("{}...", &doc.name[..17]) } else { doc.name.clone() };
        fb.draw_string(x + w / 2 - title.len() * 4, y + 12, &title, Theme::TEXT_PRIMARY.to_u32());

        // Sağ taraf düğmeleri
        btn_x = x + w - 140;

        // Paylaş düğmesi
        fb.draw_rect(btn_x, y + 8, 28, 28, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(btn_x + 4, y + 12, "⬆", Theme::TEXT_PRIMARY.to_u32());
        btn_x += 32;

        // Açıklama araçları düğmesi
        fb.draw_rect(btn_x, y + 8, 28, 28, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(btn_x + 4, y + 12, "✎", Theme::TEXT_PRIMARY.to_u32());
        btn_x += 32;

        // Tam ekran düğmesi
        fb.draw_rect(btn_x, y + 8, 28, 28, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(btn_x + 4, y + 12, "⛶", Theme::TEXT_PRIMARY.to_u32());
    }

    /// Kenar çubuğuna sayfa küçük resimlerini çizer.
    ///
    /// Her küçük resim için yer tutucu çizgiler gösterilir (gerçek
    /// işleme yapılmadığından). Seçili sayfa aksan rengiyle vurgulanır.
    fn draw_sidebar(&self, fb: &mut Framebuffer, x: usize, y: usize, doc: &PreviewDocument) {
        let thumb_height = 100;
        let thumb_width = SIDEBAR_WIDTH - 16;

        for (i, page) in doc.pages.iter().enumerate() {
            let thumb_y = y + i * (thumb_height + 8) + 8;

            if thumb_y + thumb_height > y + (self.rect.height as usize) - TOOLBAR_HEIGHT - STATUS_BAR_HEIGHT {
                break;
            }

            let is_selected = i == doc.current_page;
            let bg = if is_selected { Theme::ACCENT_PRIMARY.to_u32() } else { Theme::WINDOW_BG.to_u32() };

            // Küçük resim arka planı
            fb.draw_rect(x + 8, thumb_y, thumb_width, thumb_height, bg);

            // Metin satırlarını temsil eden yer tutucu çizgiler
            let thumb_content_y = thumb_y + 4;
            let thumb_content_h = thumb_height - 20;

            for line in 0..6 {
                let line_y = thumb_content_y + line * 12;
                let line_width = thumb_width - 16 - (line % 3) * 20;
                fb.draw_rect(x + 16, line_y, line_width, 8, Theme::TEXT_SECONDARY.to_u32());
            }

            // Sayfa numarası etiketi
            let page_text = format!("Page {}", i + 1);
            fb.draw_string(x + 12, thumb_y + thumb_height - 14, &page_text, Theme::TEXT_SECONDARY.to_u32());
        }

        if doc.pages.is_empty() {
            fb.draw_string(x + 20, y + 20, "No pages", Theme::TEXT_SECONDARY.to_u32());
        }
    }

    /// İçerik alanını belge türüne göre çizer.
    ///
    /// Yükleniyor durumundaysa ilerleme çubuğu gösterilir.
    /// Yüklenme tamamlandığında belge türüne göre
    /// farklı çizim metotları çağrılır.
    fn draw_content(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize, doc: &PreviewDocument) {
        // Tür bazlı arka plan rengi: resimler için koyu, metin için normal
        let bg_color = if doc.doc_type == DocumentType::Image { 0xFF333333 } else { Theme::WINDOW_BG.to_u32() };
        fb.draw_rect(x, y, w, h, bg_color);

        if doc.loading {
            // Yükleme göstergesi
            let center_x = x + w / 2;
            let center_y = y + h / 2;

            fb.draw_string(center_x - 40, center_y - 8, "Loading...", Theme::TEXT_SECONDARY.to_u32());

            // İlerleme çubuğu
            let bar_width = 200;
            let bar_x = center_x - bar_width / 2;
            let bar_y = center_y + 20;

            fb.draw_rect(bar_x, bar_y, bar_width, 4, Theme::BORDER.to_u32());
            fb.draw_rect(bar_x, bar_y, (bar_width as f32 * doc.loading_progress) as usize, 4, Theme::ACCENT_PRIMARY.to_u32());

            return;
        }

        // Belge türüne göre içerik çiz
        match doc.doc_type {
            DocumentType::Image => {
                self.draw_image_content(fb, x, y, w, h, doc);
            }
            DocumentType::Text | DocumentType::Code | DocumentType::Markdown => {
                self.draw_text_content(fb, x, y, w, h, doc);
            }
            _ => {
                // Genel belge bilgi ekranı
                self.draw_generic_content(fb, x, y, w, h, doc);
            }
        }

        // Açıklamaları üste çiz
        for annotation in doc.pages.get(doc.current_page).map(|p| p.annotations.as_slice()).unwrap_or(&[]) {
            self.draw_annotation(fb, x, y, annotation, doc.zoom);
        }
    }

    /// Resim içeriğini çizer.
    ///
    /// Resim, zoom oranına göre ölçeklenir ve içerik alanının
    /// ortasına yerleştirilir (center-fit). Şeffaflığı belirtmek
    /// için dama tahtası deseni kullanılır (grafik editörlerinin
    /// klasik şeffaf arkaplan gösterimidir).
    fn draw_image_content(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize, doc: &PreviewDocument) {
        if let Some(page) = doc.pages.get(doc.current_page) {
            let (img_w, img_h) = doc.get_rotated_size(page.width, page.height);

            // Zoom uygulanmış boyutlar
            let scaled_w = (img_w as f32 * doc.zoom) as usize;
            let scaled_h = (img_h as f32 * doc.zoom) as usize;

            // Görüntüyü görüntü alanında ortala
            let draw_x = x + (w - scaled_w) / 2;
            let draw_y = y + (h - scaled_h) / 2;

            // Yer tutucu resim: gerçek resim verisi yerine dama deseni
            // Dama deseni: grafik editörlerinde şeffaf piksel gösteriminin standardıdır
            for py in 0..scaled_h.min(h) {
                for px in 0..scaled_w.min(w) {
                    let screen_x = draw_x + px;
                    let screen_y = draw_y + py;

                    if screen_x < x + w && screen_y < y + h {
                        // 8×8 piksellik dama karesi
                        let checker = ((px / 8) + (py / 8)) % 2 == 0;
                        let color = if checker { 0xFF404040 } else { 0xFF505050 };
                        fb.plot_pixel(screen_x, screen_y, color);
                    }
                }
            }

            // Resmin çerçevesini çiz
            fb.draw_rect_outline(draw_x, draw_y, scaled_w.min(w), scaled_h.min(h), Theme::BORDER.to_u32());
        }
    }

    /// Metin veya kaynak kodu içeriğini çizer.
    ///
    /// Sözcük kaydırma (word wrap) uygulanır: satır izin verilen
    /// karakter genişliğini aşarsa yeni satıra geçilir.
    /// Gerçek uygulamada sözdizimi vurgulama da burada yapılır.
    fn draw_text_content(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize, doc: &PreviewDocument) {
        if let Some(page) = doc.pages.get(doc.current_page) {
            if let Some(content) = &page.content {
                // Metin çizim parametreleri
                let margin = 20;
                let text_x = x + margin;
                let mut text_y = y + margin;
                let line_height = 18;
                let char_width = 8;
                let max_chars = (w - margin * 2) / char_width;

                for line in content.lines() {
                    if text_y + line_height > y + h - margin {
                        break;
                    }

                    // Sözcük kaydırma: `split_whitespace` ile sözcüklere ayır
                    let mut current_line = String::new();
                    for word in line.split_whitespace() {
                        if current_line.len() + word.len() + 1 > max_chars {
                            fb.draw_string(text_x, text_y, &current_line, Theme::TEXT_PRIMARY.to_u32());
                            text_y += line_height;
                            current_line = String::from(word);
                            current_line.push(' ');
                        } else {
                            current_line.push_str(word);
                            current_line.push(' ');
                        }

                        if text_y + line_height > y + h - margin {
                            break;
                        }
                    }

                    if !current_line.is_empty() && text_y + line_height <= y + h - margin {
                        fb.draw_string(text_x, text_y, &current_line, Theme::TEXT_PRIMARY.to_u32());
                        text_y += line_height;
                    }
                }
            }
        }
    }

    /// Bilinmeyen veya önizlenemeyen belge türleri için bilgi ekranı gösterir.
    fn draw_generic_content(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize, doc: &PreviewDocument) {
        let center_y = y + h / 2;

        fb.draw_string(x + w / 2 - 20, center_y - 40, doc.doc_type.icon(), Theme::TEXT_PRIMARY.to_u32());
        fb.draw_string(x + w / 2 - doc.name.len() * 4, center_y, &doc.name, Theme::TEXT_PRIMARY.to_u32());
        fb.draw_string(x + w / 2 - 40, center_y + 20, "Preview not available", Theme::TEXT_SECONDARY.to_u32());
    }

    /// Sayfa üzerindeki açıklamayı çizer.
    ///
    /// Vurgulama (Highlight) için alfa karıştırma (alpha blending) kullanılır:
    /// mevcut piksel rengiyle açıklama rengi 0.3 saydamlıkla karıştırılır.
    /// `unsafe` bloğu: framebuffer belleğine doğrudan erişim gerektirir.
    fn draw_annotation(&self, fb: &mut Framebuffer, x: usize, y: usize, annotation: &Annotation, zoom: f32) {
        let ax = x + (annotation.rect.0 * zoom) as usize;
        let ay = y + (annotation.rect.1 * zoom) as usize;
        let aw = (annotation.rect.2 * zoom) as usize;
        let ah = (annotation.rect.3 * zoom) as usize;

        match annotation.annotation_type {
            AnnotationType::Highlight => {
                // Yarı saydam vurgulama: alfa karıştırma ile mevcut pikseli değiştir
                for py in 0..ah {
                    for px in 0..aw {
                        let ptr = unsafe {
                            (fb.base_addr as *mut u32).add((ay + py) * fb.pixels_per_scan_line + ax + px)
                        };
                        let bg = unsafe { *ptr };
                        unsafe { *ptr = Self::blend_color(bg, annotation.color, 0.3); }
                    }
                }
            }
            AnnotationType::Underline => {
                fb.draw_rect(ax, ay + ah - 2, aw, 2, annotation.color);
            }
            AnnotationType::Strikeout => {
                fb.draw_rect(ax, ay + ah / 2 - 1, aw, 2, annotation.color);
            }
            AnnotationType::Text => {
                // Metin notu simgesi
                fb.draw_rect(ax, ay, 20, 20, annotation.color);
                fb.draw_string(ax + 4, ay + 2, "💬", Theme::TEXT_PRIMARY.to_u32());
            }
            _ => {}
        }
    }

    /// İki rengi verilen alfa değeriyle karıştırır.
    ///
    /// Formül: `result = bg * (1 - alpha) + fg * alpha`
    /// Bu lineer interpolasyon (LERP) formülüdür. `alpha = 0.0` → tam arka plan,
    /// `alpha = 1.0` → tam ön plan.
    fn blend_color(bg: u32, fg: u32, alpha: f32) -> u32 {
        let br = ((bg >> 16) & 0xFF) as f32;
        let bg_ = ((bg >> 8) & 0xFF) as f32;
        let bb = (bg & 0xFF) as f32;

        let fr = ((fg >> 16) & 0xFF) as f32;
        let fg_ = ((fg >> 8) & 0xFF) as f32;
        let fb = (fg & 0xFF) as f32;

        let r = (br * (1.0 - alpha) + fr * alpha) as u32;
        let g = (bg_ * (1.0 - alpha) + fg_ * alpha) as u32;
        let b = (bb * (1.0 - alpha) + fb * alpha) as u32;

        (r << 16) | (g << 8) | b
    }

    /// Durum çubuğunu çizer: sayfa bilgisi, dosya boyutu ve zoom oranı.
    fn draw_status_bar(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, doc: &PreviewDocument) {
        // Sayfa bilgisi (sol taraf)
        let page_info = format!("Page {} of {}", doc.current_page + 1, doc.page_count);
        fb.draw_string(x + 8, y + 4, &page_info, Theme::TEXT_SECONDARY.to_u32());

        // Dosya boyutu (orta)
        if doc.metadata.file_size > 0 {
            let size_text = Self::format_size(doc.metadata.file_size);
            fb.draw_string(x + w / 2 - size_text.len() * 4, y + 4, &size_text, Theme::TEXT_SECONDARY.to_u32());
        }

        // Zoom oranı (sağ taraf)
        let zoom_text = format!("{:.0}%", doc.zoom * 100.0);
        fb.draw_string(x + w - zoom_text.len() * 8 - 8, y + 4, &zoom_text, Theme::TEXT_SECONDARY.to_u32());
    }

    /// Bayt cinsinden boyutu insan tarafından okunabilir biçime çevirir.
    ///
    /// SI önek sistemi: 1 KB = 1024 B, 1 MB = 1024 KB, 1 GB = 1024 MB.
    /// Bilgisayarlarda ikili prefix'ler kullanılır (IEC standardı: KiB, MiB, GiB),
    /// ancak bu uygulama daha yaygın olan "KB/MB/GB" gösterimini tercih eder.
    fn format_size(size: u64) -> String {
        if size < 1024 {
            format!("{} B", size)
        } else if size < 1024 * 1024 {
            format!("{:.1} KB", size as f64 / 1024.0)
        } else if size < 1024 * 1024 * 1024 {
            format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
        } else {
            format!("{:.1} GB", size as f64 / (1024.0 * 1024.0 * 1024.0))
        }
    }

    /// Fare tıklaması olayını işler.
    ///
    /// Araç çubuğu düğmeleri, kenar çubuğu küçük resimleri ve
    /// tam ekran geçişi bu metot üzerinden yönetilir.
    pub fn on_click(&mut self, mx: i32, my: i32) -> PreviewAction {
        let x = self.rect.x;
        let y = self.rect.y;
        let w = self.rect.width;

        // Araç çubuğu bölgesi
        if my >= (y + 8) as i32 && my < (y + 36) as i32 {
            let mut btn_x = x + 8;

            // Kapat düğmesi
            if mx >= btn_x as i32 && mx < (btn_x + 28) as i32 {
                self.close();
                return PreviewAction::Close;
            }
            btn_x += 36;

            // Önceki sayfa
            if mx >= btn_x as i32 && mx < (btn_x + 28) as i32 {
                if let Some(ref mut doc) = self.document {
                    doc.prev_page();
                }
                return PreviewAction::None;
            }
            btn_x += 32;

            // Sonraki sayfa
            if mx >= btn_x as i32 && mx < (btn_x + 28) as i32 {
                if let Some(ref mut doc) = self.document {
                    doc.next_page();
                }
                return PreviewAction::None;
            }
            btn_x += 40;

            // Zoom küçült
            if mx >= btn_x as i32 && mx < (btn_x + 28) as i32 {
                if let Some(ref mut doc) = self.document {
                    doc.zoom_out();
                }
                return PreviewAction::None;
            }
            btn_x += 32;

            // Zoom metin alanını atla
            btn_x += 40;

            // Zoom büyüt
            if mx >= btn_x as i32 && mx < (btn_x + 28) as i32 {
                if let Some(ref mut doc) = self.document {
                    doc.zoom_in();
                }
                return PreviewAction::None;
            }
            btn_x += 36;

            // Sola döndür
            if mx >= btn_x as i32 && mx < (btn_x + 28) as i32 {
                if let Some(ref mut doc) = self.document {
                    doc.rotate_left();
                }
                return PreviewAction::None;
            }
            btn_x += 32;

            // Sağa döndür
            if mx >= btn_x as i32 && mx < (btn_x + 28) as i32 {
                if let Some(ref mut doc) = self.document {
                    doc.rotate_right();
                }
                return PreviewAction::None;
            }

            // Sağ taraf düğmeleri
            btn_x = x + w - 140;

            // Paylaş
            if mx >= btn_x as i32 && mx < (btn_x + 28) as i32 {
                return PreviewAction::Share;
            }
            btn_x += 32;

            // Açıklama modu aç/kapat
            if mx >= btn_x as i32 && mx < (btn_x + 28) as i32 {
                self.annotation_mode = if self.annotation_mode.is_some() { None } else { Some(AnnotationType::Highlight) };
                return PreviewAction::None;
            }
            btn_x += 32;

            // Tam ekran geçişi
            if mx >= btn_x as i32 && mx < (btn_x + 28) as i32 {
                self.fullscreen = !self.fullscreen;
                return PreviewAction::ToggleFullscreen;
            }
        }

        // Kenar çubuğu: küçük resme tıklama ile sayfa geçişi
        if self.show_sidebar && mx >= x && mx < x + SIDEBAR_WIDTH as i32 {
            let thumb_height = 100;
            let content_y = y + TOOLBAR_HEIGHT as i32;

            if let Some(ref doc) = self.document {
                for i in 0..doc.pages.len() {
                    let thumb_y = content_y + (i * (thumb_height + 8) + 8) as i32;

                    if my >= thumb_y && my < thumb_y + thumb_height as i32 {
                        if let Some(ref mut doc) = self.document {
                            doc.go_to_page(i);
                        }
                        return PreviewAction::None;
                    }
                }
            }
        }

        PreviewAction::None
    }

    /// Klavye tuşu olayını işler.
    ///
    /// `+`/`-`: zoom artır/azalt
    /// `0`: zoom sıfırla
    /// `[`/`]`: döndür
    /// `Escape`: tam ekrandan çık veya kapat
    /// `Space`: sonraki sayfaya geç
    pub fn on_key_press(&mut self, c: char) -> PreviewAction {
        match c {
            '+' | '=' => {
                if let Some(ref mut doc) = self.document {
                    doc.zoom_in();
                }
            }
            '-' => {
                if let Some(ref mut doc) = self.document {
                    doc.zoom_out();
                }
            }
            '0' => {
                if let Some(ref mut doc) = self.document {
                    doc.reset_zoom();
                }
            }
            '[' => {
                if let Some(ref mut doc) = self.document {
                    doc.rotate_left();
                }
            }
            ']' => {
                if let Some(ref mut doc) = self.document {
                    doc.rotate_right();
                }
            }
            '\x1b' => { // Escape
                if self.fullscreen {
                    self.fullscreen = false;
                    return PreviewAction::ToggleFullscreen;
                } else {
                    self.close();
                    return PreviewAction::Close;
                }
            }
            ' ' => { // Space: sonraki sayfaya geç veya slayt gösterisini başlat
                if let Some(ref mut doc) = self.document {
                    if doc.current_page < doc.page_count - 1 {
                        doc.next_page();
                    }
                }
            }
            _ => {}
        }

        PreviewAction::None
    }

    /// Pencereyi yeniden boyutlandırır.
    pub fn resize(&mut self, width: usize, height: usize) {
        self.rect.width = width as i32;
        self.rect.height = height as i32;
    }
}

/// Önizleme uygulamasından yayılan eylemler.
///
/// Eylem tabanlı mimari: UI bileşeni kendi durumunu değiştirmek
/// yerine ne yapılması gerektiğini bildiren bir değer döndürür.
#[derive(Clone, Debug)]
pub enum PreviewAction {
    None,
    OpenFile(String),
    Close,
    Share,
    ToggleFullscreen,
    Annotate(AnnotationType),
}

// ============================================================================
// GLOBAL ÖNİZLEME PENCERESİ — Tek Örnek (Singleton)
// ============================================================================

/// `lazy_static!` ile programın tamamında paylaşılan tekil
/// önizleme penceresi örneği tanımlanır. `Mutex<PreviewWindow>`
/// yapısı spin kilidiyle çok çekirdekli güvenli erişim sağlar.
lazy_static::lazy_static! {
    static ref PREVIEW: Mutex<PreviewWindow> = Mutex::new(PreviewWindow::new(Rect {
        x: 100,
        y: 100,
        width: 900,
        height: 700,
    }));
}

/// Önizleme modülünü başlatır.
pub fn init() {
    crate::serial_println!("[GUI] Preview initialized");
}

/// Global önizleme penceresi örneğine referans döndürür.
pub fn get_preview() -> &'static Mutex<PreviewWindow> {
    &PREVIEW
}
