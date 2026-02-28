//! # Metin Editörü Uygulaması
//!
//! Sözdizimi vurgulama ve temel düzenleme özelliklerine sahip basit bir metin editörü.
//! Arama, değiştirme ve birden fazla dosya formatını destekler.
//!
//! ## Mimari
//! - `TextBuffer`: Satır tabanlı metin depolama ve undo/redo yığınları
//! - `Cursor`: İmleç konumu, istenilen sütun ve seçim çıpası
//! - `SyntaxHighlighter`: Dil tespiti ve renkli sözdizimi çözümleme
//! - `TextEditor`: Tüm bileşenleri birleştiren ana editör yapısı
//!
//! ## `no_std` Ortamı
//! `alloc` crate'inden `Vec`, `String`, `VecDeque` ve `format!` kullanılır.
//! Standart kütüphane yoktur; bellek yığın (heap) üzerinde manuel yönetilir.

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use alloc::collections::VecDeque;
use spin::Mutex;

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};
use crate::gui::widgets::{Widget, Rect};

// ============================================================================
// METİN TAMPONU
// ============================================================================

/// Satır tabanlı metin tamponu.
///
/// Metin, `Vec<String>` olarak satır satır saklanır.
/// Bu yaklaşım satır ekleme/silme işlemlerini kolaylaştırır;
/// ancak büyük dosyalarda `Vec::insert` O(n) olduğundan
/// yavaş olabilir — rope veri yapısı alternatif bir çözümdür.
pub struct TextBuffer {
    /// Her satırı ayrı bir `String` olarak tutan vektör.
    /// `Vec<String>` dinamik boyutlu; eleman eklendikçe heap'te büyür.
    lines: Vec<String>,
    /// Dosyanın son kaydedilmesinden bu yana değişip değişmediğini gösterir.
    modified: bool,
    /// Geçerli dosya yolu. Boşsa "Untitled" olarak kabul edilir.
    file_path: String,
    /// Geri alma yığını — son yapılan işlemleri sırasıyla tutar.
    /// `VecDeque`: çift uçlu kuyruk; hem baştan hem sondan O(1) ekleme/silme sağlar.
    undo_stack: VecDeque<EditAction>,
    /// Yeniden yapma yığını — geri alınan işlemleri saklar.
    redo_stack: VecDeque<EditAction>,
    /// Undo geçmişinde tutulacak maksimum işlem sayısı.
    /// Bu sınırın aşılması en eski işlemlerin silinmesine neden olur.
    max_undo: usize,
}

/// Bir düzenleme işlemini temsil eden yapı.
///
/// `#[derive(Clone, Debug)]`:
/// - `Clone`: Bu yapı kopyalanabilir (undo/redo yığınları için gerekli).
/// - `Debug`: `{:?}` formatıyla yazdırılabilir (hata ayıklama için).
#[derive(Clone, Debug)]
pub struct EditAction {
    /// İşlem türü: ekleme, silme veya değiştirme.
    action_type: ActionType,
    /// İşlemin gerçekleştiği satır numarası (0-indeksli).
    line: usize,
    /// İşlemin gerçekleştiği sütun numarası (0-indeksli).
    column: usize,
    /// Eklenen veya yeni metin.
    text: String,
    /// İşlemden önceki eski metin (geri alma için gerekli).
    old_text: String,
}

/// Düzenleme işlemi türleri.
///
/// `#[derive(Clone, Copy, Debug, PartialEq, Eq)]`:
/// - `Copy`: Bu enum, ekleme/silme/değiştirme gibi basit değerler içerdiğinden
///   yığında kopyalanabilir; `Clone`'a gerek kalmaz.
/// - `PartialEq + Eq`: `==` operatörüyle karşılaştırılabilir.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionType {
    /// Metin ekleme işlemi.
    Insert,
    /// Metin silme işlemi.
    Delete,
    /// Metin değiştirme işlemi (silme + ekleme kombinasyonu).
    Replace,
}

impl TextBuffer {
    /// Boş bir metin tamponu oluşturur.
    ///
    /// `vec![String::new()]`: En az bir boş satır içerir;
    /// bu sayede sıfır satırlı geçersiz durum oluşmaz.
    pub fn new() -> Self {
        TextBuffer {
            lines: vec![String::new()],
            modified: false,
            file_path: String::new(),
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            max_undo: 100,
        }
    }

    /// Bir metni tampona yükler.
    ///
    /// `text.lines()`: Platformdan bağımsız satır ayırıcı tanır (`\n`, `\r\n`).
    /// `map(String::from)`: Her `&str` dilimini sahipli `String`'e dönüştürür.
    /// `collect()`: Yineleyiciyi `Vec<String>`'e toplar.
    pub fn load(&mut self, text: &str) {
        self.lines = text.lines().map(String::from).collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.modified = false;
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    /// Bir dosyadan metin yükler.
    ///
    /// Not: VFS (Virtual File System) henüz `no_std` ortamında kullanılamadığından
    /// bu fonksiyon şu an yalnızca dosya yolunu kaydeder ve `false` döndürür.
    /// Gelecekte VFS entegrasyonu ile gerçek dosya okuması yapılacaktır.
    pub fn load_file(&mut self, path: &str) -> bool {
        self.file_path = String::from(path);

        // VFS not available in no_std yet
        false
    }

    /// Tamponu mevcut dosya yoluna kaydeder.
    ///
    /// Dosya yolu boşsa `false` döndürür.
    /// Yoksa `save_as` çağırarak kaydeder.
    pub fn save(&mut self) -> bool {
        if self.file_path.is_empty() {
            return false;
        }

        let path = self.file_path.clone();
        self.save_as(&path)
    }

    /// Tamponu yeni bir dosya yoluna kaydeder.
    ///
    /// `to_string()`: Satırları `\n` ile birleştirerek tek bir `String` oluşturur.
    /// Not: VFS henüz mevcut olmadığından daima `false` döndürür.
    pub fn save_as(&mut self, path: &str) -> bool {
        let text = self.to_string();

        // VFS not available in no_std yet
        false
    }

    /// Tamponu tek bir dizeye dönüştürür.
    ///
    /// `join("\n")`: Tüm satırları `\n` karakteriyle birleştirir.
    /// Bu, Unix satır sonu standardını kullanır.
    pub fn to_string(&self) -> String {
        self.lines.join("\n")
    }

    /// Toplam satır sayısını döndürür.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Belirtilen indeksteki satırı döndürür.
    ///
    /// `get(line)`: Sınır dışı erişimde `None` döner; `unwrap_or("")` ile güvenli hale gelir.
    /// `as_str()`: `String`'den `&str` dilimi elde eder; ömür süresi `self`'e bağlıdır.
    pub fn get_line(&self, line: usize) -> &str {
        self.lines.get(line).map(|s| s.as_str()).unwrap_or("")
    }

    /// Belirtilen satırın karakter uzunluğunu döndürür.
    pub fn line_length(&self, line: usize) -> usize {
        self.lines.get(line).map(|s| s.len()).unwrap_or(0)
    }

    /// Bir satıra tek karakter ekler.
    ///
    /// `line_text.insert(col, c)`: `String::insert`, verilen bayt konumuna
    /// bir karakter ekler. Bu, UTF-8 güvenli bir konumda olunduğunu varsayar.
    pub fn insert_char(&mut self, line: usize, col: usize, c: char) {
        if line < self.lines.len() {
            let line_text = &mut self.lines[line];
            if col <= line_text.len() {
                line_text.insert(col, c);
                self.modified = true;
            }
        }
    }

    /// Bir satıra birden fazla karakter (dize) ekler.
    ///
    /// Tek satır ekleme ile çok satırlı eklemeyi ayrı ayrı ele alır:
    /// - Tek satır: `insert_str` ile doğrudan ekleme.
    /// - Çok satır: Mevcut satırı böler, yeni satırları araya ekler.
    pub fn insert_str(&mut self, line: usize, col: usize, text: &str) {
        if line < self.lines.len() {
            let lines: Vec<&str> = text.split('\n').collect();

            if lines.len() == 1 {
                // Tek satır ekleme — `col.min(max_col)` ile sınır dışına çıkma önlenir.
                let max_col = self.lines[line].len();
                self.lines[line].insert_str(col.min(max_col), text);
            } else {
                // Çok satırlı ekleme:
                // 1. Mevcut satırı imleç konumundan ikiye böl.
                // 2. İlk parçanın sonuna ilk yeni satırı ekle.
                // 3. Orta satırları araya ekle.
                // 4. Son satırla eski satırın kalan kısmını birleştir.
                let current_line = &mut self.lines[line];
                let after_cursor = current_line[col..].to_string();
                current_line.truncate(col);
                current_line.push_str(lines[0]);

                for i in 1..lines.len() - 1 {
                    self.lines.insert(line + i, String::from(lines[i]));
                }

                let last_line = format!("{}{}", lines.last().unwrap_or(&""), after_cursor);
                self.lines.insert(line + lines.len() - 1, last_line);
            }

            self.modified = true;
        }
    }

    /// Bir konumdaki karakteri siler.
    ///
    /// - `col < satır uzunluğu`: Karakteri siler (`remove(col)`).
    /// - `col == satır uzunluğu` ve sonraki satır var: Sonraki satırı bu satırla birleştirir.
    ///   Bu davranış, satır sonunda Backspace tuşuna basıldığında satır birleştirmeyi sağlar.
    pub fn delete_char(&mut self, line: usize, col: usize) {
        if line < self.lines.len() {
            let line_text = &mut self.lines[line];
            if col < line_text.len() {
                line_text.remove(col);
                self.modified = true;
            } else if line < self.lines.len() - 1 {
                // Sonraki satırı bu satırla birleştir
                let next_line = self.lines.remove(line + 1);
                self.lines[line].push_str(&next_line);
                self.modified = true;
            }
        }
    }

    /// Bir aralıktaki metni siler.
    ///
    /// - Aynı satır: `replace_range` ile satır içi silme.
    /// - Farklı satırlar: Başlangıç satırının sol kısmını ve bitiş satırının
    ///   sağ kısmını birleştirerek aradaki satırları siler.
    /// `end_col.min(line.len())` ile satır uzunluğunun aşılması önlenir.
    pub fn delete_range(&mut self, start_line: usize, start_col: usize, end_line: usize, end_col: usize) {
        if start_line == end_line {
            if start_line < self.lines.len() {
                let line = &mut self.lines[start_line];
                line.replace_range(start_col..end_col.min(line.len()), "");
                self.modified = true;
            }
        } else if start_line < end_line {
            // Birden fazla satıra yayılan silme işlemi:
            // Başlangıç satırının sol kısmını ve bitiş satırının sağ kısmını al.
            let start = self.lines[start_line][..start_col].to_string();
            let end = if end_line < self.lines.len() {
                self.lines[end_line][end_col..].to_string()
            } else {
                String::new()
            };

            // Aradaki satırları kaldır
            for _ in start_line..end_line {
                self.lines.remove(start_line + 1);
            }

            // Başı ve sonu birleştir
            self.lines[start_line] = format!("{}{}", start, end);
            self.modified = true;
        }
    }

    /// İmleç konumuna yeni satır ekler.
    ///
    /// Mevcut satırı `col` konumundan ikiye böler:
    /// - Sol kısım mevcut satırda kalır.
    /// - Sağ kısım yeni bir satır olarak eklenir.
    /// `lines.insert(line + 1, after)`: `Vec::insert` O(n) — büyük dosyalarda yavaş olabilir.
    pub fn insert_newline(&mut self, line: usize, col: usize) {
        if line < self.lines.len() {
            let current = &mut self.lines[line];
            let after = current[col..].to_string();
            current.truncate(col);

            self.lines.insert(line + 1, after);
            self.modified = true;
        }
    }

    /// Tamponun değiştirilip değiştirilmediğini döndürür.
    pub fn is_modified(&self) -> bool {
        self.modified
    }

    /// Geçerli dosya yolunu döndürür.
    ///
    /// `&str`: Sahipliği aktarmak yerine ödünç verme (borrow) — sıfır kopya.
    pub fn file_path(&self) -> &str {
        &self.file_path
    }

    /// Son yapılan işlemi geri alır.
    ///
    /// `pop_back()`: `VecDeque`'nin sonundan (en yeni) eleman çıkarır.
    /// Şu an undo mantığı uygulanmamakta; yalnızca işlem döndürülmektedir.
    pub fn undo(&mut self) -> Option<EditAction> {
        self.undo_stack.pop_back()
    }

    /// Son geri alınan işlemi yeniden yapar.
    ///
    /// `pop_back()`: Redo yığınının sonundan en yeni geri alınan işlemi çıkarır.
    pub fn redo(&mut self) -> Option<EditAction> {
        self.redo_stack.pop_back()
    }
}

/// `Default` trait'i, `TextBuffer::new()` yerine `Default::default()` çağırılmasını sağlar.
/// Rust'ta `#[derive(Default)]` kullanamadığımız durumlarda manuel implementasyon gerekir.
impl Default for TextBuffer {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// İMLEÇ VE SEÇİM
// ============================================================================

/// İmleç konumu ve metin seçimi.
///
/// `desired_column`: Dikey hareket sırasında hedeflenen sütun.
/// Örneğin: 10. sütunda yukarı taşıdığınızda 5 karakterlik bir satıra gelirseniz
/// imleç 5. sütuna gider; ama yukarı/aşağı tekrar basıldığında hâlâ 10. sütun hedeflenir.
///
/// `selection_anchor`: `Option<(usize, usize)>`:
/// - `None` — aktif seçim yok.
/// - `Some((satır, sütun))` — seçimin başlangıç noktası (çıpa).
pub struct Cursor {
    /// Mevcut satır numarası (0-indeksli).
    pub line: usize,
    /// Mevcut sütun numarası (0-indeksli).
    pub column: usize,
    /// Dikey harekette hedeflenen sütun.
    /// Satırlar kısa olduğunda imleç göründüğünden daha sağda "hatırlanır".
    pub desired_column: usize,
    /// Seçim çıpası — seçim başlangıç noktası.
    /// `None` ise aktif seçim yoktur.
    pub selection_anchor: Option<(usize, usize)>,
}

impl Cursor {
    /// Varsayılan konumda (0, 0) yeni bir imleç oluşturur.
    pub fn new() -> Self {
        Cursor {
            line: 0,
            column: 0,
            desired_column: 0,
            selection_anchor: None,
        }
    }

    /// İmleci bir satır yukarı taşır.
    ///
    /// `desired_column.min(satır_uzunluğu)`: Hedeflenen sütun, satır uzunluğunu aşamaz.
    /// Bu sayede kısa satırlardan geçerken sütun konumu kaybolmaz.
    pub fn move_up(&mut self, buffer: &TextBuffer) {
        if self.line > 0 {
            self.line -= 1;
            self.column = self.desired_column.min(buffer.line_length(self.line));
        }
    }

    /// İmleci bir satır aşağı taşır.
    ///
    /// `line_count() - 1`: Son satırın ötesine geçmeyi engeller.
    pub fn move_down(&mut self, buffer: &TextBuffer) {
        if self.line < buffer.line_count() - 1 {
            self.line += 1;
            self.column = self.desired_column.min(buffer.line_length(self.line));
        }
    }

    /// İmleci bir karakter sola taşır.
    ///
    /// Satır başındaysa bir önceki satırın sonuna atlar.
    /// Bu, metin editörlerindeki standart "satır sarma" davranışıdır.
    pub fn move_left(&mut self, buffer: &TextBuffer) {
        if self.column > 0 {
            self.column -= 1;
            self.desired_column = self.column;
        } else if self.line > 0 {
            // Satır başında — bir önceki satırın sonuna geç
            self.line -= 1;
            self.column = buffer.line_length(self.line);
            self.desired_column = self.column;
        }
    }

    /// İmleci bir karakter sağa taşır.
    ///
    /// Satır sonundaysa bir sonraki satırın başına atlar.
    pub fn move_right(&mut self, buffer: &TextBuffer) {
        if self.column < buffer.line_length(self.line) {
            self.column += 1;
            self.desired_column = self.column;
        } else if self.line < buffer.line_count() - 1 {
            // Satır sonunda — bir sonraki satırın başına geç
            self.line += 1;
            self.column = 0;
            self.desired_column = 0;
        }
    }

    /// İmleci satır başına taşır (Home tuşu).
    pub fn move_home(&mut self) {
        self.column = 0;
        self.desired_column = 0;
    }

    /// İmleci satır sonuna taşır (End tuşu).
    pub fn move_end(&mut self, buffer: &TextBuffer) {
        self.column = buffer.line_length(self.line);
        self.desired_column = self.column;
    }

    /// Seçimi başlatır; çıpayı mevcut konuma ayarlar.
    ///
    /// Shift tuşuyla harekette çağrılır.
    /// `Some((self.line, self.column))`: Mevcut konum çıpa olarak kaydedilir.
    pub fn start_selection(&mut self) {
        self.selection_anchor = Some((self.line, self.column));
    }

    /// Seçimi bitirir; çıpayı temizler.
    pub fn end_selection(&mut self) {
        self.selection_anchor = None;
    }

    /// Aktif bir seçim olup olmadığını döndürür.
    pub fn has_selection(&self) -> bool {
        self.selection_anchor.is_some()
    }

    /// Seçim aralığını sıralı şekilde döndürür.
    ///
    /// `map(|(anchor_line, anchor_col)| ...)`: `Option` üzerinde dönüşüm.
    /// Seçim çıpadan imleçe veya imleçten çıpaya doğru olabilir;
    /// bu fonksiyon daima `(başlangıç, bitiş)` sırasını garanti eder.
    pub fn get_selection(&self) -> Option<((usize, usize), (usize, usize))> {
        self.selection_anchor.map(|(anchor_line, anchor_col)| {
            if (anchor_line, anchor_col) <= (self.line, self.column) {
                ((anchor_line, anchor_col), (self.line, self.column))
            } else {
                ((self.line, self.column), (anchor_line, anchor_col))
            }
        })
    }

    /// İmleci belirli bir konuma ayarlar.
    ///
    /// `desired_column`'u da günceller; böylece yukarı/aşağı harekette
    /// hedeflenen sütun doğru kalır.
    pub fn set_position(&mut self, line: usize, column: usize) {
        self.line = line;
        self.column = column;
        self.desired_column = column;
    }
}

/// `Default` trait implementasyonu — `Cursor::new()` ile aynı.
impl Default for Cursor {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// SÖZDİZİMİ VURGULAMASI
// ============================================================================

/// Sözdizimi vurgulayıcı.
///
/// Dile göre anahtar kelimeleri, yorumları ve dizeleri renklendirir.
/// `&'static str`: Derleme zamanında belirlenmiş, program ömrü boyunca yaşayan string dilimleri.
/// Bu sayede heap tahsisi yapılmaz; string sabitler `.rodata` segmentinde saklanır.
pub struct SyntaxHighlighter {
    /// Aktif programlama dili.
    language: Language,
    /// Bu dilin anahtar kelime listesi.
    /// `Vec<&'static str>`: Anahtar kelimeler statik string dilimleri olarak saklanır.
    keywords: Vec<&'static str>,
    /// Satır yorumunun başlangıç belirteci (ör: `//`, `#`).
    comment_start: &'static str,
    /// Yorum sonu belirteci (genellikle `\n` — satır sonu).
    comment_end: &'static str,
    /// String sınırlayıcıları — açma ve kapama karakterleri.
    string_delimiters: (&'static str, &'static str),
}

/// Desteklenen programlama dilleri.
///
/// `#[derive(Clone, Copy, Debug, PartialEq, Eq)]`:
/// - `Copy + Clone`: Fonksiyonlar arası geçişte kopyalanabilir; heap gerekmez.
/// - `PartialEq + Eq`: `==` ile karşılaştırılabilir (match ifadelerinde gerekli).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    /// Düz metin — sözdizimi vurgulaması yok.
    PlainText,
    /// Rust programlama dili.
    Rust,
    /// C/C++ programlama dilleri.
    C,
    /// JavaScript/TypeScript programlama dilleri.
    JavaScript,
    /// Python programlama dili.
    Python,
    /// Yapılandırma dosyaları (TOML, INI, CFG).
    Config,
}

impl Language {
    /// Dosya uzantısından dili tespit eder.
    ///
    /// `to_lowercase()`: Büyük/küçük harf bağımsız karşılaştırma için küçük harfe dönüştürür.
    /// `as_str()`: `String`'i `&str`'e dönüştürür — match ifadesinde string karşılaştırması için.
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "rs" => Language::Rust,
            "c" | "h" | "cpp" | "hpp" => Language::C,
            "js" | "ts" => Language::JavaScript,
            "py" => Language::Python,
            "cfg" | "conf" | "ini" | "toml" => Language::Config,
            _ => Language::PlainText,
        }
    }
}

impl SyntaxHighlighter {
    /// Belirli bir dil için sözdizimi vurgulayıcı oluşturur.
    ///
    /// Her dil için anahtar kelimeler, yorum ve string sınırlayıcıları tanımlanır.
    /// `match language { ... }`: Rust'ın exhaustive (tamamen kapsayıcı) match'i —
    /// tüm enum varyantları ele alınmazsa derleme hatası verir.
    pub fn new(language: Language) -> Self {
        let (keywords, comment_start, comment_end, string_delims) = match language {
            Language::Rust => (
                vec!["fn", "let", "mut", "if", "else", "match", "struct", "enum", "impl", "pub",
                     "use", "mod", "crate", "self", "super", "const", "static", "type", "trait",
                     "where", "for", "while", "loop", "break", "continue", "return", "as", "in",
                     "true", "false", "None", "Some", "Ok", "Err", "async", "await", "move"],
                "//", "\n", ("\"", "\"")
            ),
            Language::C => (
                vec!["int", "char", "void", "if", "else", "for", "while", "do", "switch", "case",
                     "break", "continue", "return", "struct", "typedef", "enum", "union",
                     "const", "static", "extern", "volatile", "sizeof", "NULL", "true", "false"],
                "//", "\n", ("\"", "\"")
            ),
            Language::JavaScript => (
                vec!["function", "var", "let", "const", "if", "else", "for", "while", "do",
                     "switch", "case", "break", "continue", "return", "class", "extends",
                     "new", "this", "super", "import", "export", "default", "async", "await",
                     "true", "false", "null", "undefined", "typeof", "instanceof"],
                "//", "\n", ("\"", "\"")
            ),
            Language::Python => (
                vec!["def", "class", "if", "elif", "else", "for", "while", "try", "except",
                     "finally", "with", "as", "import", "from", "return", "yield", "lambda",
                     "True", "False", "None", "and", "or", "not", "in", "is", "pass", "break",
                     "continue", "global", "nonlocal", "async", "await"],
                "#", "\n", ("\"", "\"")
            ),
            Language::Config => (
                vec!["true", "false", "yes", "no", "on", "off"],
                "#", "\n", ("\"", "\"")
            ),
            Language::PlainText => (
                vec![],
                "", "", ("", "")
            ),
        };

        SyntaxHighlighter {
            language,
            keywords,
            comment_start,
            comment_end,
            string_delimiters: string_delims,
        }
    }

    /// Bir satırı renk kodlu segmentlere ayırır.
    ///
    /// Döndürülen `Vec<(String, u32)>` listesindeki her eleman:
    /// - `String`: Metin segmenti
    /// - `u32`: ARGB renk değeri
    ///
    /// ## Algoritma
    /// 1. Yorum başlangıcı tespit edilirse kalan metin yorum rengiyle eklenir.
    /// 2. String sınırlayıcı bulunursa string modu açılır/kapanır.
    /// 3. Kelime sonu geldiğinde kelime anahtar kelime listesiyle karşılaştırılır.
    pub fn highlight_line(&self, line: &str) -> Vec<(String, u32)> {
        let mut result = Vec::new();

        // Düz metin için vurgulama yapılmaz — tüm metin tek renkte döner.
        if self.language == Language::PlainText {
            result.push((String::from(line), Theme::TEXT_PRIMARY.to_u32()));
            return result;
        }

        let mut current = String::new();
        let mut in_string = false;
        let mut in_comment = false;
        // `chars().collect()`: UTF-8 karakterleri indeksle erişilebilir `Vec<char>`'a dönüştürür.
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            // Yorum başlangıcı kontrolü — string içindeyken yorum tanınmaz.
            if !in_string && !in_comment && self.comment_start.len() > 0 {
                let remaining: String = chars[i..].iter().collect();
                if remaining.starts_with(self.comment_start) {
                    if !current.is_empty() {
                        result.push((current.clone(), Theme::TEXT_PRIMARY.to_u32()));
                        current.clear();
                    }

                    // Satırın kalan kısmı yorumdur — yorum rengiyle eklenir.
                    let comment: String = chars[i..].iter().collect();
                    result.push((comment, Theme::TEXT_SECONDARY.to_u32()));
                    return result;
                }
            }

            // String sınırlayıcı kontrolü — yorum içindeyken string tanınmaz.
            if !in_comment && self.string_delimiters.0.len() > 0 {
                if chars[i] == self.string_delimiters.0.chars().next().unwrap() {
                    if !current.is_empty() {
                        result.push((current.clone(), Theme::TEXT_PRIMARY.to_u32()));
                        current.clear();
                    }

                    // String modu aç/kapat (toggle)
                    in_string = !in_string;
                    current.push(chars[i]);
                    i += 1;
                    continue;
                }
            }

            current.push(chars[i]);

            // Anahtar kelime kontrolü — string veya yorum içindeyken kontrol edilmez.
            if !in_string && !in_comment {
                // Kelime sonu tespiti: sonraki karakter harf/rakam/alt çizgi değilse kelime bitti.
                let is_word_end = i + 1 >= chars.len() || !chars[i + 1].is_alphanumeric() && chars[i + 1] != '_';

                if is_word_end && self.keywords.contains(&current.as_str()) {
                    // Anahtar kelime bulundu — vurgu rengiyle ekle.
                    result.push((current.clone(), Theme::TEXT_ACCENT.to_u32()));
                    current.clear();
                }
            }

            i += 1;
        }

        // Kalan metin — string içindeyse vurgu rengi, değilse normal renk.
        if !current.is_empty() {
            let color = if in_string { Theme::TEXT_ACCENT.to_u32() } else { Theme::TEXT_PRIMARY.to_u32() };
            result.push((current, color));
        }

        result
    }
}

// ============================================================================
// METİN EDİTÖRÜ
// ============================================================================

/// Tam özellikli Metin Editörü uygulaması.
///
/// ## Bileşenler
/// - `TextBuffer`: Metin verisi ve undo/redo geçmişi
/// - `Cursor`: İmleç konumu ve seçim yönetimi
/// - `SyntaxHighlighter`: Dile göre sözdizimi vurgulaması
/// - Kaydırma, arama/değiştirme, satır numaraları
///
/// ## Koordinat Sistemi
/// `rect`: Editörün ekrandaki konumu ve boyutu.
/// İçerik alanı, başlık çubuğu (32px) ve menü çubuğu (24px) çıkarıldıktan sonra başlar.
pub struct TextEditor {
    /// Editör penceresinin konum ve boyutu.
    rect: Rect,
    /// Düzenlenecek metin verisi.
    buffer: TextBuffer,
    /// Metin imleci.
    cursor: Cursor,
    /// Sözdizimi vurgulayıcı.
    highlighter: SyntaxHighlighter,
    /// Dikey kaydırma ofseti (satır sayısı cinsinden).
    scroll_y: usize,
    /// Yatay kaydırma ofseti (sütun sayısı cinsinden).
    scroll_x: usize,
    /// Tab genişliği (boşluk sayısı).
    tab_size: usize,
    /// Satır numaralarını göster/gizle.
    show_line_numbers: bool,
    /// Satır numaraları sütununun piksel genişliği.
    line_number_width: usize,
    /// Yazı tipi boyutu (piksel).
    font_size: usize,
    /// Satır yüksekliği (piksel).
    line_height: usize,
    /// Arama sorgusu.
    search_query: String,
    /// Arama sonuçları — `(satır, sütun)` çiftleri listesi.
    search_results: Vec<(usize, usize)>,
    /// Mevcut seçili arama sonucunun indeksi.
    /// `Option<usize>`: Sonuç yoksa `None`.
    search_index: Option<usize>,
    /// Değiştirme dizesi.
    replace_str: String,
    /// Arama çubuğunu göster/gizle.
    show_search: bool,
    /// Dosyanın değiştirilip değiştirilmediği (`buffer.modified`'dan bağımsız yerel kopya).
    file_modified: bool,
}

impl TextEditor {
    /// Varsayılan ayarlarla yeni bir metin editörü oluşturur.
    ///
    /// `Rect::new(180, 80, 900, 600)`: Desktop üzerinde 900×600px boyutunda bir pencere.
    /// `tab_size: 4`: 4 boşluklu tab — yaygın programlama standardı.
    /// `line_height: 18`: 14px yazı tipi için 4px satır aralığı.
    pub fn new() -> Self {
        TextEditor {
            rect: Rect::new(180, 80, 900, 600),
            buffer: TextBuffer::new(),
            cursor: Cursor::new(),
            highlighter: SyntaxHighlighter::new(Language::PlainText),
            scroll_y: 0,
            scroll_x: 0,
            tab_size: 4,
            show_line_numbers: true,
            line_number_width: 48,
            font_size: 14,
            line_height: 18,
            search_query: String::new(),
            search_results: Vec::new(),
            search_index: None,
            replace_str: String::new(),
            show_search: false,
            file_modified: false,
        }
    }

    /// Dosya yoluyla bir metin dosyasını yükler.
    ///
    /// Dosya uzantısına göre sözdizimi vurgulayıcıyı otomatik ayarlar.
    /// `rsplit('.')`: Noktadan sağa böler; son eleman uzantıdır.
    /// `next().unwrap_or("")`: Uzantı yoksa boş dize kullanılır.
    pub fn load_file(&mut self, path: &str) -> bool {
        if self.buffer.load_file(path) {
            // Uzantıya göre sözdizimi dilini belirle
            let ext = path.rsplit('.').next().unwrap_or("");
            self.highlighter = SyntaxHighlighter::new(Language::from_extension(ext));
            self.cursor = Cursor::new();
            self.scroll_y = 0;
            self.scroll_x = 0;
            self.file_modified = false;
            true
        } else {
            false
        }
    }

    /// Dosyayı mevcut yola kaydeder.
    pub fn save_file(&mut self) -> bool {
        if self.buffer.save() {
            self.file_modified = false;
            true
        } else {
            false
        }
    }

    /// Dosyayı yeni bir yola kaydeder.
    pub fn save_as(&mut self, path: &str) -> bool {
        if self.buffer.save_as(path) {
            self.file_modified = false;
            true
        } else {
            false
        }
    }

    /// Editörü framebuffer'a çizer.
    ///
    /// ## Çizim Katmanları (yukarıdan aşağıya)
    /// 1. Pencere arka planı
    /// 2. Başlık çubuğu (32px) + kapat butonu
    /// 3. Menü çubuğu (24px)
    /// 4. Arama çubuğu (32px, `show_search` açıksa)
    /// 5. Satır numaraları sütunu (48px, `show_line_numbers` açıksa)
    /// 6. Metin içeriği (sözdizimi vurgulamalı)
    /// 7. Durum çubuğu (24px, alt)
    pub fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let width = self.rect.width as usize;
        let height = self.rect.height as usize;

        // Pencere arka planı
        fb.draw_rect(x, y, width, height, Theme::WINDOW_BG.to_u32());

        // Başlık çubuğu
        fb.draw_rect(x, y, width, 32, Theme::TITLEBAR_BG.to_u32());

        // Başlık metni: dosya adı + değiştirilme işareti (*)
        let title = if self.buffer.file_path().is_empty() {
            String::from("Text Editor - Untitled")
        } else {
            let name = self.buffer.file_path().rsplit('/').next().unwrap_or(self.buffer.file_path());
            let modified = if self.buffer.is_modified() { " *" } else { "" };
            format!("Text Editor - {}{}", name, modified)
        };
        fb.draw_string(x + 12, y + 8, &title, Theme::TEXT_PRIMARY.to_u32());

        // Kapat butonu — kırmızı arka planı olan × işareti
        fb.draw_rect(x + width - 28, y + 4, 24, 24, Theme::ERROR.to_u32());
        fb.draw_string(x + width - 20, y + 8, "×", Theme::TEXT_ON_ACCENT.to_u32());

        // Menü çubuğu
        let menu_y = y + 32;
        fb.draw_rect(x, menu_y, width, 24, Theme::TOOLBAR_BG.to_u32());

        // Menü öğeleri — her öğe 8px/karakter genişliğinde + 16px boşluk
        let menus = ["File", "Edit", "View", "Search", "Help"];
        let mut menu_x = x + 8;
        for menu in &menus {
            fb.draw_string(menu_x, menu_y + 4, menu, Theme::TEXT_PRIMARY.to_u32());
            menu_x += menu.len() * 8 + 16;
        }

        // Arama çubuğu — `show_search` açıksa gösterilir
        if self.show_search {
            let search_y = menu_y + 24;
            fb.draw_rect(x, search_y, width, 32, Theme::TOOLBAR_BG.to_u32());

            // Arama giriş alanı
            fb.draw_rect(x + 8, search_y + 4, 200, 24, Theme::INPUT_BG.to_u32());
            fb.draw_string(x + 12, search_y + 8, &self.search_query, Theme::TEXT_PRIMARY.to_u32());

            // Değiştirme giriş alanı
            fb.draw_rect(x + 220, search_y + 4, 200, 24, Theme::INPUT_BG.to_u32());
            fb.draw_string(x + 224, search_y + 8, &self.replace_str, Theme::TEXT_SECONDARY.to_u32());

            // Eylem butonları
            fb.draw_string(x + 440, search_y + 8, "Find", Theme::ACCENT_PRIMARY.to_u32());
            fb.draw_string(x + 480, search_y + 8, "Replace", Theme::ACCENT_PRIMARY.to_u32());
            fb.draw_string(x + 540, search_y + 8, "Replace All", Theme::ACCENT_PRIMARY.to_u32());
        }

        // İçerik alanı hesaplama — başlık + menü + (arama) + durum çubuğu çıkarılır
        let content_y = y + 32 + 24 + if self.show_search { 32 } else { 0 };
        let content_height = height - 32 - 24 - if self.show_search { 32 } else { 0 } - 24;

        // Satır numaraları sütunu
        if self.show_line_numbers {
            fb.draw_rect(x, content_y, self.line_number_width, content_height, Theme::SIDEBAR_BG.to_u32());

            // Görünür satır sayısını hesapla
            let visible_lines = content_height / self.line_height;
            for i in 0..visible_lines {
                let line_num = self.scroll_y + i + 1;
                if line_num <= self.buffer.line_count() {
                    // `{:4}`: 4 karakterlik sağa hizalı sayı formatı
                    let num_str = format!("{:4}", line_num);
                    let num_y = content_y + i * self.line_height;
                    fb.draw_string(x + 4, num_y + 2, &num_str, Theme::TEXT_SECONDARY.to_u32());
                }
            }
        }

        // Metin içeriği alanı — satır numaraları varsa sola ofset eklenir
        let text_x = x + if self.show_line_numbers { self.line_number_width } else { 0 };
        let text_width = width - if self.show_line_numbers { self.line_number_width } else { 0 };

        fb.draw_rect(text_x, content_y, text_width, content_height, Theme::WINDOW_BG.to_u32());

        // Sözdizimi vurgulamalı metni çiz
        self.draw_text(fb, text_x, content_y, text_width, content_height);

        // Durum çubuğu — en alta yapıştırılmış
        let status_y = y + height - 24;
        fb.draw_rect(x, status_y, width, 24, Theme::TOOLBAR_BG.to_u32());

        // İmleç konumu, satır sayısı ve dil bilgisi
        let status = format!("Ln {}, Col {} | {} lines | {}",
            self.cursor.line + 1,
            self.cursor.column + 1,
            self.buffer.line_count(),
            self.highlighter.language_name()
        );
        fb.draw_string(x + 12, status_y + 4, &status, Theme::TEXT_SECONDARY.to_u32());

        // Dosya durumu — sağ kenara hizalanmış
        let file_status = if self.buffer.is_modified() { "Modified" } else { "Saved" };
        fb.draw_string(x + width - 100, status_y + 4, file_status, Theme::TEXT_SECONDARY.to_u32());
    }

    /// Sözdizimi vurgulamalı metin içeriğini çizer.
    ///
    /// ## İmleç Satırı Vurgulaması
    /// İmlecin bulunduğu satır `SELECTION_BG` rengiyle arka plan alır.
    ///
    /// ## Seçim Vurgulaması
    /// `get_selection()`: Seçim aralığı sorgulanır.
    /// Çok satırlı seçimlerde her satır için doğru başlangıç/bitiş sütunu hesaplanır.
    ///
    /// ## Sözdizimi Vurgulaması
    /// `highlight_line()`: Her satır `(metin, renk)` çiftlerine ayrılır.
    /// `char_x`: Karakter başına 8 piksel ilerlenir (monospace fontu varsayar).
    fn draw_text(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize, height: usize) {
        let visible_lines = height / self.line_height;

        for i in 0..visible_lines {
            let line_idx = self.scroll_y + i;
            if line_idx >= self.buffer.line_count() {
                break;
            }

            let line = self.buffer.get_line(line_idx);
            let line_y = y + i * self.line_height;

            // İmleç satırını vurgula
            if line_idx == self.cursor.line {
                fb.draw_rect(x, line_y, width, self.line_height, Theme::SELECTION_BG.to_u32());
            }

            // Metin seçimini vurgula
            if let Some(((start_line, start_col), (end_line, end_col))) = self.cursor.get_selection() {
                if line_idx >= start_line && line_idx <= end_line {
                    // Seçim sütunlarını bu satır için hesapla
                    let (sel_start, sel_end) = if start_line == end_line {
                        (start_col, end_col)
                    } else if line_idx == start_line {
                        (start_col, line.len())
                    } else if line_idx == end_line {
                        (0, end_col)
                    } else {
                        (0, line.len())
                    };

                    // Seçim dikdörtgeni — yatay kaydırma ofseti çıkarılır
                    let sel_x = x + sel_start * 8 - self.scroll_x * 8;
                    let sel_width = (sel_end - sel_start) * 8;
                    fb.draw_rect(sel_x, line_y, sel_width, self.line_height, Theme::SELECTION_BG.to_u32());
                }
            }

            // Sözdizimi vurgulamalı metin çiz
            let highlighted = self.highlighter.highlight_line(line);
            let mut char_x = x;

            for (segment, color) in &highlighted {
                for c in segment.chars() {
                    if char_x >= x + width {
                        break; // Görünür alanın sağını aşan karakterleri çizme
                    }

                    if char_x >= x {
                        // Yatay kaydırma ofseti uygulanır: `scroll_x * 8` piksel sola kaydırır
                        fb.draw_char(char_x - self.scroll_x * 8, line_y + 2, c, *color);
                    }
                    char_x += 8; // Monospace: her karakter 8px genişliğinde
                }
            }

            // İmleci çiz — ince dikey çizgi (2px genişlik)
            if line_idx == self.cursor.line {
                let cursor_x = x + self.cursor.column * 8 - self.scroll_x * 8;
                if cursor_x >= x && cursor_x < x + width {
                    // 2px genişliğinde dikey imleç çizgisi
                    fb.draw_rect(cursor_x, line_y, 2, self.line_height, Theme::TEXT_PRIMARY.to_u32());
                }
            }
        }
    }

    /// Klavye tuş basışını işler.
    ///
    /// `modifiers`: Bit maskesi — bit 0 = Ctrl, bit 1 = Shift.
    /// `(modifiers & 0x01) != 0`: Bitwise AND ile Ctrl tuşu kontrol edilir.
    ///
    /// ## Özel Karakter Kodları
    /// - `'\n'` / `'\r'` (0x0A / 0x0D): Enter tuşu
    /// - `'\t'` (0x09): Tab tuşu
    /// - `'\x08'` (0x08): Backspace tuşu
    /// - `'\x7F'` (0x7F): Delete tuşu
    ///
    /// ## Tab Genişletme
    /// Tab, bir sonraki tab dur(ağ)una tamamlanacak kadar boşluk eklenir.
    /// `tab_size - (col % tab_size)`: Bir sonraki tab durağına olan mesafe.
    pub fn on_key_press(&mut self, c: char, modifiers: u8) -> EditorAction {
        let ctrl = (modifiers & 0x01) != 0;
        let shift = (modifiers & 0x02) != 0;

        match c {
            '\n' | '\r' => {
                // Enter: imlecin önüne yeni satır ekle, imleci başa taşı
                self.buffer.insert_newline(self.cursor.line, self.cursor.column);
                self.cursor.line += 1;
                self.cursor.column = 0;
                self.cursor.desired_column = 0;
                self.file_modified = true;
            }
            '\t' => {
                // Tab: bir sonraki tab durağına kadar boşluk ekle
                let spaces = self.tab_size - (self.cursor.column % self.tab_size);
                for _ in 0..spaces {
                    self.buffer.insert_char(self.cursor.line, self.cursor.column, ' ');
                    self.cursor.column += 1;
                }
                self.cursor.desired_column = self.cursor.column;
                self.file_modified = true;
            }
            '\x08' => {
                // Backspace: imlecin solundaki karakteri sil
                if self.cursor.column > 0 {
                    self.cursor.column -= 1;
                    self.buffer.delete_char(self.cursor.line, self.cursor.column);
                    self.cursor.desired_column = self.cursor.column;
                } else if self.cursor.line > 0 {
                    // Satır başında — önceki satırın sonuna geç ve satırları birleştir
                    self.cursor.line -= 1;
                    self.cursor.column = self.buffer.line_length(self.cursor.line);
                    self.buffer.delete_char(self.cursor.line, self.cursor.column);
                    self.cursor.desired_column = self.cursor.column;
                }
                self.file_modified = true;
            }
            '\x7F' => {
                // Delete: imlecin sağındaki (üzerindeki) karakteri sil
                self.buffer.delete_char(self.cursor.line, self.cursor.column);
                self.file_modified = true;
            }
            c if !c.is_control() => {
                // Yazdırılabilir karakter — kontrol karakterleri hariç tutulur
                if ctrl {
                    // Ctrl kısayolları
                    match c {
                        's' | 'S' => {
                            // Ctrl+S: Kaydet
                            return EditorAction::Save;
                        }
                        'f' | 'F' => {
                            // Ctrl+F: Arama çubuğunu aç/kapat
                            self.show_search = !self.show_search;
                            return EditorAction::None;
                        }
                        'z' | 'Z' => {
                            // Ctrl+Z: Geri al
                            if let Some(_action) = self.buffer.undo() {
                                // Apply undo
                            }
                            return EditorAction::None;
                        }
                        'y' | 'Y' => {
                            // Ctrl+Y: Yeniden yap
                            if let Some(_action) = self.buffer.redo() {
                                // Apply redo
                            }
                            return EditorAction::None;
                        }
                        'a' | 'A' => {
                            // Ctrl+A: Tümünü seç
                            self.cursor.set_position(0, 0);
                            self.cursor.selection_anchor = Some((0, 0));
                            self.cursor.line = self.buffer.line_count() - 1;
                            self.cursor.column = self.buffer.line_length(self.cursor.line);
                            return EditorAction::None;
                        }
                        _ => {}
                    }
                } else {
                    // Normal karakter ekleme
                    self.buffer.insert_char(self.cursor.line, self.cursor.column, c);
                    self.cursor.column += 1;
                    self.cursor.desired_column = self.cursor.column;
                    self.file_modified = true;
                }
            }
            _ => {}
        }

        // İmlecin görünür alanda kalmasını sağla
        self.ensure_cursor_visible();

        EditorAction::None
    }

    /// Ok tuşları ve özel tuşları işler.
    ///
    /// Shift tuşuyla birlikte kullanıldığında seçim başlatılır/genişletilir.
    /// Shift olmadan hareket edilirse mevcut seçim iptal edilir.
    pub fn on_special_key(&mut self, key: SpecialKey, shift: bool) -> EditorAction {
        if shift && !self.cursor.has_selection() {
            // Shift + hareket — seçim henüz başlamamışsa başlat
            self.cursor.start_selection();
        } else if !shift {
            // Shift olmadan hareket — seçimi iptal et
            self.cursor.end_selection();
        }

        match key {
            SpecialKey::Up => self.cursor.move_up(&self.buffer),
            SpecialKey::Down => self.cursor.move_down(&self.buffer),
            SpecialKey::Left => self.cursor.move_left(&self.buffer),
            SpecialKey::Right => self.cursor.move_right(&self.buffer),
            SpecialKey::Home => self.cursor.move_home(),
            SpecialKey::End => self.cursor.move_end(&self.buffer),
            SpecialKey::PageUp => {
                // Page Up: 20 satır yukarı atla
                let page_size = 20;
                for _ in 0..page_size {
                    self.cursor.move_up(&self.buffer);
                }
            }
            SpecialKey::PageDown => {
                // Page Down: 20 satır aşağı atla
                let page_size = 20;
                for _ in 0..page_size {
                    self.cursor.move_down(&self.buffer);
                }
            }
            _ => {}
        }

        self.ensure_cursor_visible();
        EditorAction::None
    }

    /// İmlecin görünür alanda kalmasını sağlar; gerekirse kaydırır.
    ///
    /// ## Dikey Kaydırma
    /// İmleç görünür alanın üstüne veya altına çıkarsa `scroll_y` güncellenir.
    ///
    /// ## Yatay Kaydırma
    /// İmleç görünür alanın soluna veya sağına çıkarsa `scroll_x` güncellenir.
    ///
    /// `rect.height` ve `rect.width`: `i32` türünde — `usize`'a dönüştürmek için
    /// `/` operatörü `as usize` ile birlikte kullanılır.
    fn ensure_cursor_visible(&mut self) {
        // Görünür satır sayısını hesapla (başlık + menü + durum çubuğu çıkarılır)
        let visible_lines = ((self.rect.height - 80) / self.line_height as i32) as usize;

        if self.cursor.line < self.scroll_y {
            // İmleç görünür alanın üstünde — kaydır
            self.scroll_y = self.cursor.line;
        } else if self.cursor.line >= self.scroll_y + visible_lines {
            // İmleç görünür alanın altında — kaydır
            self.scroll_y = self.cursor.line - visible_lines + 1;
        }

        // Görünür sütun sayısını hesapla (satır numaraları alanı çıkarılır)
        let visible_cols = ((self.rect.width - self.line_number_width as i32) / 8) as usize;

        if self.cursor.column < self.scroll_x {
            // İmleç görünür alanın solunda — kaydır
            self.scroll_x = self.cursor.column;
        } else if self.cursor.column >= self.scroll_x + visible_cols {
            // İmleç görünür alanın sağında — kaydır
            self.scroll_x = self.cursor.column - visible_cols + 1;
        }
    }

    /// Fare tıklamasını işler.
    ///
    /// ## Kapat Butonu
    /// Sağ üst köşedeki × butonuna tıklanırsa `EditorAction::Close` döner.
    ///
    /// ## Metin Alanı Tıklaması
    /// Piksel koordinatlarından satır/sütun hesaplanır:
    /// - `(my - text_y) / line_height`: Hangi satıra tıklandı?
    /// - `(mx - text_x) / 8`: Hangi sütuna tıklandı? (8px/karakter)
    /// Kaydırma ofseti de eklenir.
    pub fn on_click(&mut self, mx: i32, my: i32) -> EditorAction {
        // Kapat butonu kontrolü
        let close_x = self.rect.x + self.rect.width - 28;
        if mx >= close_x && mx < close_x + 24 && my >= self.rect.y + 4 && my < self.rect.y + 28 {
            return EditorAction::Close;
        }

        // Metin alanı tıklaması
        let text_x = self.rect.x + if self.show_line_numbers { self.line_number_width as i32 } else { 0 };
        let text_y = self.rect.y + 56 + if self.show_search { 32 } else { 0 };

        if mx >= text_x && my >= text_y {
            // Piksel → satır/sütun dönüşümü
            let line = self.scroll_y + ((my - text_y) as usize / self.line_height);
            let column = self.scroll_x + ((mx - text_x) as usize / 8);

            if line < self.buffer.line_count() {
                // `column.min(satır_uzunluğu)`: Satır sonunun ötesine tıklamayı engelle
                self.cursor.set_position(line, column.min(self.buffer.line_length(line)));
            }
        }

        EditorAction::None
    }

    /// Fare tekerleği kaydırmasını işler.
    ///
    /// `delta > 0`: Aşağı kaydır.
    /// `delta < 0`: Yukarı kaydır.
    /// `saturating_add / saturating_sub`: Taşma olmadan güvenli aritmetik.
    /// `min(line_count - 1)`: Son satırın ötesine kaydırmayı engeller.
    pub fn on_scroll(&mut self, delta: i32) {
        if delta > 0 {
            self.scroll_y = self.scroll_y.saturating_add(delta as usize);
        } else {
            self.scroll_y = self.scroll_y.saturating_sub((-delta) as usize);
        }

        // Son satırın ötesine kaydırmayı engelle
        self.scroll_y = self.scroll_y.min(self.buffer.line_count().saturating_sub(1));
    }

    /// Metin araması yapar.
    ///
    /// Her satırda sorguyu arar ve tüm eşleşme konumlarını `search_results`'a ekler.
    /// `line[start..].find(query)`: Alt dize araması; `start` ofseti eklenerek mutlak konum bulunur.
    /// `start = start + pos + query.len()`: Bir sonraki aramayı eşleşmeden sonra başlat.
    /// İlk sonuca atlanır ve imleç oraya konumlandırılır.
    pub fn search(&mut self, query: &str) {
        self.search_query = String::from(query);
        self.search_results.clear();

        if query.is_empty() {
            return;
        }

        for (line_idx, line) in self.buffer.lines.iter().enumerate() {
            let mut start = 0;
            while let Some(pos) = line[start..].find(query) {
                self.search_results.push((line_idx, start + pos));
                start = start + pos + query.len();
            }
        }

        // Sonuç varsa ilkini seç
        self.search_index = if !self.search_results.is_empty() { Some(0) } else { None };

        // İlk sonuca atla
        if let Some(idx) = self.search_index {
            let (line, col) = self.search_results[idx];
            self.cursor.set_position(line, col);
            self.ensure_cursor_visible();
        }
    }

    /// Mevcut arama sonucunu değiştirir.
    ///
    /// 1. Mevcut eşleşme silinir (`delete_range`).
    /// 2. Yerine `replace_str` eklenir (`insert_str`).
    /// 3. Arama yenilenir (sonuç konumları güncellenir).
    pub fn replace_next(&mut self) {
        if let Some(idx) = self.search_index {
            let (line, col) = self.search_results[idx];
            let len = self.search_query.len();

            self.buffer.delete_range(line, col, line, col + len);
            self.buffer.insert_str(line, col, &self.replace_str);

            // Arama sonuçlarını güncelle
            let query = self.search_query.clone();
            self.search(&query);
        }
    }

    /// Tüm arama sonuçlarını değiştirir.
    ///
    /// Döngü: Sonuç listesi boşalana kadar `replace_next()` çağrılır.
    /// Her değiştirme sonrasında `search()` yeniden çalışır — bu O(n²) bir işlemdir.
    /// Büyük dosyalarda tek geçişli bir implementasyon daha verimli olurdu.
    pub fn replace_all(&mut self) {
        while !self.search_results.is_empty() {
            self.replace_next();
        }
    }

    /// Editör penceresinin dikdörtgenini döndürür.
    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// Editör penceresinin dikdörtgenini ayarlar (yeniden boyutlandırma).
    pub fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }

    /// Açık dosyanın yolunu döndürür.
    pub fn file_path(&self) -> &str {
        self.buffer.file_path()
    }

    /// Dosyanın değiştirilip değiştirilmediğini döndürür.
    pub fn is_modified(&self) -> bool {
        self.buffer.is_modified()
    }
}

/// Özel tuş kodları — ok tuşları, sayfa tuşları ve diğerleri.
///
/// `#[derive(Clone, Copy, Debug, PartialEq, Eq)]`: Fonksiyonlar arası geçişte kopyalanabilir.
/// Bu enum, klavye sürücüsünden gelen ham scan kodlarının üst seviye temsilidir.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecialKey {
    /// Yukarı ok tuşu.
    Up,
    /// Aşağı ok tuşu.
    Down,
    /// Sola ok tuşu.
    Left,
    /// Sağa ok tuşu.
    Right,
    /// Satır başı (Home tuşu).
    Home,
    /// Satır sonu (End tuşu).
    End,
    /// Sayfa yukarı (Page Up tuşu).
    PageUp,
    /// Sayfa aşağı (Page Down tuşu).
    PageDown,
    /// Ekleme modu (Insert tuşu).
    Insert,
    /// Silme (Delete tuşu).
    Delete,
    /// Kaçış (Escape tuşu).
    Escape,
    /// Sekme (Tab tuşu).
    Tab,
    /// Enter tuşu.
    Enter,
}

/// Editör eylemleri — editörden üst katmana iletilen komutlar.
///
/// `#[derive(Clone, Debug)]`: Eylemler klonlanabilir ve yazdırılabilir.
/// Bu enum, GUI olay döngüsünün editör üzerinde gerçekleştireceği eylemleri temsil eder.
/// Rust'ta bu pattern "Return/Result-based event system" olarak bilinir.
#[derive(Clone, Debug)]
pub enum EditorAction {
    /// Herhangi bir eylem gerekmez.
    None,
    /// Editörü kapat.
    Close,
    /// Mevcut dosyayı kaydet.
    Save,
    /// Farklı kaydet — yeni dosya yoluyla.
    SaveAs(String),
    /// Yeni dosya aç — verilen yoldan.
    Open(String),
    /// Yeni boş dosya oluştur.
    New,
}

impl SyntaxHighlighter {
    /// Aktif dilin adını döndürür.
    ///
    /// `&'static str`: Derleme zamanında belirlenmiş statik dize — heap tahsisi yok.
    fn language_name(&self) -> &'static str {
        match self.language {
            Language::Rust => "Rust",
            Language::C => "C/C++",
            Language::JavaScript => "JavaScript",
            Language::Python => "Python",
            Language::Config => "Config",
            Language::PlainText => "Plain Text",
        }
    }
}

/// `Default` trait implementasyonu — `TextEditor::new()` ile aynı.
impl Default for TextEditor {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// GLOBAL METİN EDİTÖRÜ
// ============================================================================

/// `lazy_static!`: Global `TEXT_EDITOR` değişkeni ilk kullanımda başlatılır.
///
/// `Mutex<TextEditor>`: `spin::Mutex` kullanılır çünkü:
/// 1. `no_std` ortamında OS muteks primitifleri yoktur.
/// 2. Spinlock, kısa süreli kritik bölümler için kabul edilebilir.
/// 3. Tek çekirdekli kullanımda performans kaybı minimumdur.
///
/// `static ref TEXT_EDITOR`: Program boyunca yaşayan tek bir editör örneği.
lazy_static::lazy_static! {
    static ref TEXT_EDITOR: Mutex<TextEditor> = Mutex::new(TextEditor::new());
}

/// Global metin editörüne referans döndürür.
///
/// `&'static Mutex<TextEditor>`: Statik ömürlü referans — kalıcı olarak geçerlidir.
/// Çağıran kod `lock()` ile kilit alarak editöre erişir.
pub fn get_editor() -> &'static Mutex<TextEditor> {
    &TEXT_EDITOR
}

/// Metin editörü modülünü başlatır.
///
/// `serial_println!`: UART üzerinden seri konsola mesaj yazar.
/// Bu fonksiyon, kernel başlangıç dizisinde GUI bileşenlerini kayıt eder.
pub fn init() {
    crate::serial_println!("[GUI] Text Editor initialized");
}
