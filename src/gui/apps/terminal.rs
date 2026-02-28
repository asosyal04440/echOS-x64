//! # Terminal Uygulaması
//!
//! Sekmeli terminal — tema desteği, scrollback tamponu ve kabuk entegrasyonu.
//! Birden fazla terminal oturumunu özelleştirilebilir görünüm seçenekleriyle sunar.
//!
//! ## Mimari
//! - `TerminalTheme`: Renk teması (16 ANSI rengi + seçim/arka plan renkleri)
//! - `TerminalCell`: Tek bir terminal hücresi (karakter + renk + nitelikler)
//! - `TerminalTab`: Tek bir terminal sekmesi/oturumu (ızgara + scrollback)
//! - `TerminalWindow`: Ana pencere; sekmeler ve temalar yönetilir
//!
//! ## Önemli Tasarım Kararları
//! - Scrollback tamponu: Yukarı kaydırılan satırlar `VecDeque` yerine `Vec`'te
//!   saklanır; `SCROLLBACK_LINES` sabiti ile sınırlandırılır.
//! - Kursor yanıp sönmesi: Delta zamanı (`dt`) kullanılarak 0.5 saniyelik
//!   döngü ile kursor görünürlüğü değiştirilir.
//! - Komut işleme: Terminal-yerel komutlar (tema, çıkış) burada işlenir;
//!   diğer tüm komutlar `crate::shell::run_command()` ile kernel shell'e aktarılır.

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};
use crate::gui::widgets::Widget;
use crate::gui::Rect;

// ============================================================================
// TERMINAL CONSTANTS
// ============================================================================

/// Sekme çubuğu yüksekliği (piksel).
/// Her sekme bu yükseklikte gösterilir.
pub const TAB_BAR_HEIGHT: usize = 28;

/// Varsayılan yazı tipi boyutu (piksel).
pub const DEFAULT_FONT_SIZE: usize = 14;

/// Varsayılan sütun sayısı (standart 80 sütun terminal).
pub const DEFAULT_COLS: usize = 80;

/// Varsayılan satır sayısı (standart 24 satır terminal).
pub const DEFAULT_ROWS: usize = 24;

/// Scrollback tamponu için maksimum satır sayısı.
/// Bu sınırı aşan eski satırlar silinir (FIFO mantığı).
pub const SCROLLBACK_LINES: usize = 1000;

// ============================================================================
// TERMINAL THEME
// ============================================================================

/// Terminal renk teması.
///
/// ANSI terminaller 16 standart renk kullanır: 8 normal + 8 parlak (bright).
/// Bu yapı hem temel renkleri hem de arka plan, ön plan, kursor gibi
/// özel renkleri bir arada saklar.
///
/// `#[derive(Clone, Debug)]`: `Clone` kopyalamaya, `Debug` yazdırmaya izin verir.
/// Tema tüm sekmelere kopyalanabilir olması için `Clone` gereklidir.
#[derive(Clone, Debug)]
pub struct TerminalTheme {
    /// Tema adı (kullanıcıya gösterilir)
    pub name: String,
    /// Terminal arka plan rengi (0xRRGGBB)
    pub background: u32,
    /// Metin ön plan rengi
    pub foreground: u32,
    /// Kursor rengi
    pub cursor: u32,
    /// Seçim arka planı
    pub selection_bg: u32,
    /// Seçim ön planı
    pub selection_fg: u32,
    /// ANSI renk 0: Siyah (normal)
    pub black: u32,
    /// ANSI renk 1: Kırmızı (normal)
    pub red: u32,
    /// ANSI renk 2: Yeşil (normal)
    pub green: u32,
    /// ANSI renk 3: Sarı (normal)
    pub yellow: u32,
    /// ANSI renk 4: Mavi (normal)
    pub blue: u32,
    /// ANSI renk 5: Eflatun (normal)
    pub magenta: u32,
    /// ANSI renk 6: Camgöbeği (normal)
    pub cyan: u32,
    /// ANSI renk 7: Beyaz (normal)
    pub white: u32,
    /// ANSI renk 8: Parlak Siyah (gri)
    pub bright_black: u32,
    /// ANSI renk 9: Parlak Kırmızı
    pub bright_red: u32,
    /// ANSI renk 10: Parlak Yeşil
    pub bright_green: u32,
    /// ANSI renk 11: Parlak Sarı
    pub bright_yellow: u32,
    /// ANSI renk 12: Parlak Mavi
    pub bright_blue: u32,
    /// ANSI renk 13: Parlak Eflatun
    pub bright_magenta: u32,
    /// ANSI renk 14: Parlak Camgöbeği
    pub bright_cyan: u32,
    /// ANSI renk 15: Parlak Beyaz
    pub bright_white: u32,
}

impl TerminalTheme {
    /// Klasik siyah/beyaz terminal teması.
    /// En eski ve en evrensel terminal renk şemasıdır.
    pub fn basic() -> Self {
        TerminalTheme {
            name: String::from("Basic"),
            background: 0x000000,
            foreground: 0xFFFFFF,
            cursor: 0xFFFFFF,
            selection_bg: 0x333333,
            selection_fg: 0xFFFFFF,
            black: 0x000000,
            red: 0x800000,
            green: 0x008000,
            yellow: 0x808000,
            blue: 0x000080,
            magenta: 0x800080,
            cyan: 0x008080,
            white: 0xC0C0C0,
            bright_black: 0x808080,
            bright_red: 0xFF0000,
            bright_green: 0x00FF00,
            bright_yellow: 0xFFFF00,
            bright_blue: 0x0000FF,
            bright_magenta: 0xFF00FF,
            bright_cyan: 0x00FFFF,
            bright_white: 0xFFFFFF,
        }
    }

    /// Solarized Dark teması.
    /// Ethan Schoonover tarafından tasarlanmış; göz yormayan renk dengesiyle
    /// popüler bir tema. Koyu yeşil taban üzerine ılımlı renkler kullanır.
    pub fn solarized_dark() -> Self {
        TerminalTheme {
            name: String::from("Solarized Dark"),
            background: 0x002B36,
            foreground: 0x839496,
            cursor: 0x839496,
            selection_bg: 0x073642,
            selection_fg: 0x839496,
            black: 0x073642,
            red: 0xDC322F,
            green: 0x859900,
            yellow: 0xB58900,
            blue: 0x268BD2,
            magenta: 0xD33682,
            cyan: 0x2AA198,
            white: 0xEEE8D5,
            bright_black: 0x002B36,
            bright_red: 0xCB4B16,
            bright_green: 0x586E75,
            bright_yellow: 0x657B83,
            bright_blue: 0x839496,
            bright_magenta: 0x6C71C4,
            bright_cyan: 0x93A1A1,
            bright_white: 0xFDF6E3,
        }
    }

    /// Dracula teması.
    /// Şeffaf lavanta arka plan ve canlı morla tanınan popüler karanlık tema.
    pub fn dracula() -> Self {
        TerminalTheme {
            name: String::from("Dracula"),
            background: 0x282A36,
            foreground: 0xF8F8F2,
            cursor: 0xF8F8F2,
            selection_bg: 0x44475A,
            selection_fg: 0xF8F8F2,
            black: 0x000000,
            red: 0xFF5555,
            green: 0x50FA7B,
            yellow: 0xF1FA8C,
            blue: 0xBD93F9,
            magenta: 0xFF79C6,
            cyan: 0x8BE9FD,
            white: 0xBFBFBF,
            bright_black: 0x282A36,
            bright_red: 0xFF5555,
            bright_green: 0x50FA7B,
            bright_yellow: 0xF1FA8C,
            bright_blue: 0xBD93F9,
            bright_magenta: 0xFF79C6,
            bright_cyan: 0x8BE9FD,
            bright_white: 0xF8F8F2,
        }
    }

    /// Gruvbox Dark teması.
    /// Sıcak turuncu/sarı tonları ile doğal bir his veren retro renk şeması.
    pub fn gruvbox_dark() -> Self {
        TerminalTheme {
            name: String::from("Gruvbox Dark"),
            background: 0x282828,
            foreground: 0xEBDBB2,
            cursor: 0xEBDBB2,
            selection_bg: 0x3C3836,
            selection_fg: 0xEBDBB2,
            black: 0x282828,
            red: 0xCC241D,
            green: 0x98971A,
            yellow: 0xD79921,
            blue: 0x458588,
            magenta: 0xB16286,
            cyan: 0x689D6A,
            white: 0xA89984,
            bright_black: 0x928374,
            bright_red: 0xFB4934,
            bright_green: 0xB8BB26,
            bright_yellow: 0xFABD2F,
            bright_blue: 0x83A598,
            bright_magenta: 0xD3869B,
            bright_cyan: 0x8EC07C,
            bright_white: 0xEBDBB2,
        }
    }

    /// Monokai teması.
    /// Sublime Text editörünün ikonik renk şeması; canlı yeşil ve pembeyle tanınır.
    pub fn monokai() -> Self {
        TerminalTheme {
            name: String::from("Monokai"),
            background: 0x272822,
            foreground: 0xF8F8F2,
            cursor: 0xF8F8F0,
            selection_bg: 0x49483E,
            selection_fg: 0xF8F8F2,
            black: 0x272822,
            red: 0xF92672,
            green: 0xA6E22E,
            yellow: 0xF4BF75,
            blue: 0x66D9EF,
            magenta: 0xAE81FF,
            cyan: 0xA1EFE4,
            white: 0xF8F8F2,
            bright_black: 0x75715E,
            bright_red: 0xF92672,
            bright_green: 0xA6E22E,
            bright_yellow: 0xF4BF75,
            bright_blue: 0x66D9EF,
            bright_magenta: 0xAE81FF,
            bright_cyan: 0xA1EFE4,
            bright_white: 0xF9F8F5,
        }
    }

    /// Nord teması.
    /// İskandinav tasarım ilhamlı soğuk mavi/gri tonları; sakin ve profesyonel görünüm.
    pub fn nord() -> Self {
        TerminalTheme {
            name: String::from("Nord"),
            background: 0x2E3440,
            foreground: 0xD8DEE9,
            cursor: 0xD8DEE9,
            selection_bg: 0x434C5E,
            selection_fg: 0xD8DEE9,
            black: 0x3B4252,
            red: 0xBF616A,
            green: 0xA3BE8C,
            yellow: 0xEBCB8B,
            blue: 0x81A1C1,
            magenta: 0xB48EAD,
            cyan: 0x88C0D0,
            white: 0xE5E9F0,
            bright_black: 0x4C566A,
            bright_red: 0xBF616A,
            bright_green: 0xA3BE8C,
            bright_yellow: 0xEBCB8B,
            bright_blue: 0x81A1C1,
            bright_magenta: 0xB48EAD,
            bright_cyan: 0x8FBCBB,
            bright_white: 0xECEFF4,
        }
    }

    /// ANSI renk koduna karşılık gelen rengi döndürür.
    ///
    /// Standart ANSI terminallerde renkler 0–15 arası numaralandırılır.
    /// `_` (wildcard) varyantı: bilinmeyen kodlar için ön plan rengi döner.
    pub fn get_color(&self, code: u8) -> u32 {
        match code {
            0 => self.black,
            1 => self.red,
            2 => self.green,
            3 => self.yellow,
            4 => self.blue,
            5 => self.magenta,
            6 => self.cyan,
            7 => self.white,
            8 => self.bright_black,
            9 => self.bright_red,
            10 => self.bright_green,
            11 => self.bright_yellow,
            12 => self.bright_blue,
            13 => self.bright_magenta,
            14 => self.bright_cyan,
            15 => self.bright_white,
            _ => self.foreground,
        }
    }
}

// ============================================================================
// TERMINAL CELL
// ============================================================================

/// Tek bir terminal hücresi.
///
/// Terminal ızgarası `rows × cols` adet `TerminalCell`'den oluşur.
/// Her hücre bir karakter, iki renk (ön plan + arka plan) ve
/// dört görsel nitelik (kalın, italik, altı çizili, ters video) saklar.
///
/// `#[derive(Clone, Copy)]`: `Copy` türetmesi bu yapının yığında kopyalanmasını
/// sağlar; `Vec::resize` ve ızgara başlatma operasyonları için kritiktir.
#[derive(Clone, Copy, Debug)]
pub struct TerminalCell {
    /// Hücrede gösterilen karakter
    pub char: char,
    /// Ön plan (metin) rengi — 0xRRGGBB formatında
    pub fg_color: u32,
    /// Arka plan rengi
    pub bg_color: u32,
    /// Kalın metin niteliği
    pub bold: bool,
    /// İtalik metin niteliği
    pub italic: bool,
    /// Altı çizili niteliği
    pub underline: bool,
    /// Ters video: ön plan ve arka plan renklerini değiştirir
    pub reverse: bool,
}

impl TerminalCell {
    /// Belirtilen renklerde yeni bir hücre oluşturur.
    /// Tüm nitelikler `false` (varsayılan) olarak başlatılır.
    pub fn new(char: char, fg: u32, bg: u32) -> Self {
        TerminalCell {
            char,
            fg_color: fg,
            bg_color: bg,
            bold: false,
            italic: false,
            underline: false,
            reverse: false,
        }
    }

    /// Temadan varsayılan boş hücre oluşturur.
    /// Boşluk karakteri ve tema renkleriyle başlatılır.
    pub fn default(theme: &TerminalTheme) -> Self {
        TerminalCell {
            char: ' ',
            fg_color: theme.foreground,
            bg_color: theme.background,
            bold: false,
            italic: false,
            underline: false,
            reverse: false,
        }
    }
}

// ============================================================================
// TERMINAL TAB
// ============================================================================

/// Bir terminal sekmesi/oturumu.
///
/// `grid: Vec<Vec<TerminalCell>>`: 2D hücre ızgarası.
/// `rows` satır, her satır `cols` hücre içerir.
/// `scrollback: Vec<Vec<TerminalCell>>`: görünür alandan yukarı kayan
/// satırların tarihsel tamponu. Kullanıcı tekerlek kaydırarak buraya erişebilir.
///
/// `history: Vec<String>`: komut geçmişi (yukarı/aşağı ok tuşuyla gezilir).
/// `history_pos`: geçmişte şu an hangi konumdasın.
#[derive(Clone, Debug)]
pub struct TerminalTab {
    /// Sekmenin benzersiz ID'si
    pub id: u32,
    /// Sekme çubuğunda gösterilen başlık
    pub title: String,
    /// Mevcut çalışma dizini
    pub cwd: String,
    /// Kabuk sürecinin ID'si (gerçek PID olacak)
    pub shell_pid: u32,
    /// Terminal hücre ızgarası [satır][sütun]
    pub grid: Vec<Vec<TerminalCell>>,
    /// Kursor X konumu (sütun, 0 tabanlı)
    pub cursor_x: usize,
    /// Kursor Y konumu (satır, 0 tabanlı)
    pub cursor_y: usize,
    /// Kursor görünür mü?
    pub cursor_visible: bool,
    /// Kaydırma konumu (scrollback'te kaç satır yukarıda)
    pub scroll_offset: usize,
    /// Scrollback tamponu (görünür alandan çıkan eski satırlar)
    pub scrollback: Vec<Vec<TerminalCell>>,
    /// Metin seçimi başlangıç noktası (sütun, satır)
    pub selection_start: Option<(usize, usize)>,
    /// Metin seçimi bitiş noktası
    pub selection_end: Option<(usize, usize)>,
    /// Zil çalıyor mu? (BEL karakteri `\x07`)
    pub bell: bool,
    /// Zil geri sayım zamanlayıcısı (saniye)
    pub bell_timer: f32,
    /// Terminal genişliği (sütun sayısı)
    pub cols: usize,
    /// Terminal yüksekliği (satır sayısı)
    pub rows: usize,
    /// Yazı tipi boyutu
    pub font_size: usize,
    /// Satır yüksekliği (piksel)
    pub line_height: usize,
    /// Karakter genişliği (piksel)
    pub char_width: usize,
    /// Kullanıcının yazdığı ama henüz çalıştırılmayan giriş tamponu
    pub input_buffer: String,
    /// Komut geçmişi listesi
    pub history: Vec<String>,
    /// Geçmişte bulunulan konum (0 = en eski, len() = güncel)
    pub history_pos: usize,
    /// Bu sekme için kullanılan tema indeksi
    pub theme_index: usize,
}

impl TerminalTab {
    /// Yeni terminal sekmesi oluşturur.
    ///
    /// Izgara `rows × cols` adet boş hücreyle doldurulur.
    /// İç içe `Vec::with_capacity` ile gereksiz yeniden tahsis önlenir:
    /// kapasite önceden ayrılır, ekleme sırasında bellek taşması olmaz.
    pub fn new(id: u32, theme: &TerminalTheme) -> Self {
        let cols = DEFAULT_COLS;
        let rows = DEFAULT_ROWS;

        // Initialize grid with empty cells
        let mut grid = Vec::with_capacity(rows);
        for _ in 0..rows {
            let mut row = Vec::with_capacity(cols);
            for _ in 0..cols {
                row.push(TerminalCell::default(theme));
            }
            grid.push(row);
        }

        TerminalTab {
            id,
            title: String::from("Terminal"),
            cwd: String::from("/home"),
            shell_pid: 0,
            grid,
            cursor_x: 0,
            cursor_y: 0,
            cursor_visible: true,
            scroll_offset: 0,
            scrollback: Vec::new(),
            selection_start: None,
            selection_end: None,
            bell: false,
            bell_timer: 0.0,
            cols,
            rows,
            font_size: DEFAULT_FONT_SIZE,
            line_height: DEFAULT_FONT_SIZE + 2,
            char_width: DEFAULT_FONT_SIZE / 2 + 2,
            input_buffer: String::new(),
            history: Vec::new(),
            history_pos: 0,
            theme_index: 0,
        }
    }

    /// Kursor konumuna tek karakter yazar.
    ///
    /// Kursor sütun sınırına ulaştığında otomatik satır atlanır (soft wrap).
    pub fn write_char(&mut self, c: char, theme: &TerminalTheme) {
        if self.cursor_x >= self.cols {
            self.newline(theme);
        }

        if self.cursor_y < self.rows {
            self.grid[self.cursor_y][self.cursor_x] = TerminalCell::new(
                c,
                theme.foreground,
                theme.background,
            );
            self.cursor_x += 1;
        }
    }

    /// Bir string yazar; özel karakterleri işler.
    ///
    /// - `'\n'`: yeni satır — `newline()` çağrısıyla satır atla + kaydır
    /// - `'\r'`: satır başı — kursoru sütun 0'a götür (üzerine yaz)
    /// - `'\t'`: sekme — `(cursor_x + 8) & !7` formülü ile 8 sütunluk
    ///   sekme durağına ilerler. `& !7` bit maskesiyle 8'in katına yuvarlar.
    /// - `'\x08'`: geri tuşu — kursoru bir sola götür
    pub fn write(&mut self, s: &str, theme: &TerminalTheme) {
        for c in s.chars() {
            match c {
                '\n' => self.newline(theme),
                '\r' => self.cursor_x = 0,
                '\t' => {
                    // Tab to next 8-column boundary
                    self.cursor_x = (self.cursor_x + 8) & !7;
                    if self.cursor_x >= self.cols {
                        self.newline(theme);
                    }
                }
                '\x08' => { // Backspace
                    if self.cursor_x > 0 {
                        self.cursor_x -= 1;
                    }
                }
                _ => self.write_char(c, theme),
            }
        }
    }

    /// Yeni satıra geçer.
    ///
    /// Son satırda değilse kursoru bir satır aşağı iner.
    /// Son satırdaysa `scroll_up()` ile ekranı bir satır yukarı kaydırır;
    /// alt kısımda boş bir satır belirtir.
    pub fn newline(&mut self, theme: &TerminalTheme) {
        self.cursor_x = 0;

        if self.cursor_y < self.rows - 1 {
            self.cursor_y += 1;
        } else {
            // Scroll up
            self.scroll_up(theme);
        }
    }

    /// Ekranı bir satır yukarı kaydırır.
    ///
    /// En üst satır scrollback tamponuna taşınır.
    /// `SCROLLBACK_LINES` aşıldığında en eski scrollback satırı silinir.
    /// Alt kısıma boş yeni bir satır eklenir.
    ///
    /// `grid.remove(0)`: O(n) işlemidir çünkü tüm elemanları sola kaydırır.
    /// Büyük terminallerde performans sorunu yaratabilir; VecDeque ile optimize
    /// edilebilir.
    pub fn scroll_up(&mut self, theme: &TerminalTheme) {
        // Move top line to scrollback
        let top_line = self.grid.remove(0);
        self.scrollback.push(top_line);

        // Limit scrollback size
        if self.scrollback.len() > SCROLLBACK_LINES {
            self.scrollback.remove(0);
        }

        // Add new empty line at bottom
        let mut new_line = Vec::with_capacity(self.cols);
        for _ in 0..self.cols {
            new_line.push(TerminalCell::default(theme));
        }
        self.grid.push(new_line);
    }

    /// Ekranı temizler ve kursoru sol üste götürür.
    ///
    /// `for row in &mut self.grid`: değiştirilebilir yineleyici;
    /// her satıra değiştirilebilir referans (`&mut Vec<TerminalCell>`) verir.
    /// `*cell = ...`: dereference ile hücrenin değerini günceller.
    pub fn clear(&mut self, theme: &TerminalTheme) {
        for row in &mut self.grid {
            for cell in row {
                *cell = TerminalCell::default(theme);
            }
        }
        self.cursor_x = 0;
        self.cursor_y = 0;
    }

    /// Zil çalar (BEL karakteri işlenir).
    /// `bell_timer = 0.2`: 0.2 saniye boyunca zil durumu aktif kalır.
    pub fn ring_bell(&mut self) {
        self.bell = true;
        self.bell_timer = 0.2;
    }

    /// Terminal boyutunu yeniden ayarlar.
    ///
    /// `Vec::resize(new_len, value)`: dizinin uzunluğunu değiştirir.
    /// Kısalırsa sondan keser; uzarsa `value` klonlarıyla doldurur.
    /// `Copy` türetmesi `TerminalCell`'in klonsuz kopyalanmasını sağlar.
    pub fn resize(&mut self, cols: usize, rows: usize, theme: &TerminalTheme) {
        // Resize grid
        self.grid.resize(rows, vec![TerminalCell::default(theme); cols]);
        for row in &mut self.grid {
            row.resize(cols, TerminalCell::default(theme));
        }

        self.cols = cols;
        self.rows = rows;

        // Clamp cursor
        self.cursor_x = self.cursor_x.min(cols - 1);
        self.cursor_y = self.cursor_y.min(rows - 1);
    }

    /// Giriş tamponundaki komutu işler (Enter'a basıldığında çağrılır).
    ///
    /// Giriş tamponu ekroya yansıtılır ve komut geçmişine eklenir.
    /// `history_pos = history.len()`: geçmişin sonuna tutanç kurar;
    /// sonraki yukarı-ok komutu en son girilen komutu gösterir.
    pub fn process_input(&mut self, theme: &TerminalTheme) {
        // Echo input
        let input = self.input_buffer.clone();
        self.write(&input, theme);

        // Add to history
        if !self.input_buffer.is_empty() {
            self.history.push(self.input_buffer.clone());
            self.history_pos = self.history.len();
        }

        self.input_buffer.clear();
    }

    /// Komut geçmişinde bir önceki komuta gider (yukarı ok).
    ///
    /// `history_pos > 0` kontrolü: listenin başının ötesine geçilmez.
    pub fn history_up(&mut self) {
        if self.history_pos > 0 {
            self.history_pos -= 1;
            if self.history_pos < self.history.len() {
                self.input_buffer = self.history[self.history_pos].clone();
            }
        }
    }

    /// Komut geçmişinde bir sonraki komuta gider (aşağı ok).
    ///
    /// Listenin sonuna gelince giriş tamponu temizlenir (yeni komut durumu).
    pub fn history_down(&mut self) {
        if self.history_pos < self.history.len() {
            self.history_pos += 1;
            if self.history_pos < self.history.len() {
                self.input_buffer = self.history[self.history_pos].clone();
            } else {
                self.input_buffer.clear();
            }
        }
    }
}

// ============================================================================
// TERMINAL WINDOW
// ============================================================================

/// Terminal penceresi.
///
/// Birden fazla sekmeyi ve temayı yönetir.
/// `cursor_blink_timer: f32` — delta zamanı ile kursor yanıp sönmesini
/// sağlar. 0.5 saniyede bir `cursor_visible` değiştirilir.
/// `next_tab_id: u32` — monoton artan sekme ID üreteci.
pub struct TerminalWindow {
    /// Pencerenin konumu ve boyutu
    pub rect: Rect,
    /// Açık terminal sekmeleri
    pub tabs: Vec<TerminalTab>,
    /// Aktif sekme indeksi
    pub active_tab: usize,
    /// Mevcut temalar listesi
    pub themes: Vec<TerminalTheme>,
    /// Kursor yanıp sönme zamanlayıcısı (saniye)
    pub cursor_blink_timer: f32,
    /// Kursor şu an görünür mü?
    pub cursor_visible: bool,
    /// Bir sonraki sekmeye atanacak ID
    pub next_tab_id: u32,
    /// Üzerine gelinmiş sekme indeksi
    pub hovered_tab: Option<usize>,
    /// Denetleyici (inspector) görünüyor mu?
    pub show_inspector: bool,
    /// Yazı tipi boyutu
    pub font_size: usize,
}

impl TerminalWindow {
    /// Yeni terminal penceresi oluşturur.
    ///
    /// Tüm yerleşik temalar `themes` vektörüne eklenir.
    /// Başlangıçta bir sekme oluşturulur ve hoşgeldiniz mesajı yazılır.
    pub fn new(rect: Rect) -> Self {
        let themes = vec![
            TerminalTheme::basic(),
            TerminalTheme::solarized_dark(),
            TerminalTheme::dracula(),
            TerminalTheme::gruvbox_dark(),
            TerminalTheme::monokai(),
            TerminalTheme::nord(),
        ];

        let mut terminal = TerminalWindow {
            rect,
            tabs: Vec::new(),
            active_tab: 0,
            themes,
            cursor_blink_timer: 0.0,
            cursor_visible: true,
            next_tab_id: 1,
            hovered_tab: None,
            show_inspector: false,
            font_size: DEFAULT_FONT_SIZE,
        };

        terminal.new_tab();
        terminal.show_welcome();

        terminal
    }

    /// Yeni sekme açıldığında hoşgeldiniz mesajı yazar.
    /// `&self.themes[0]`: ilk temayla (Basic) başlatılır.
    fn show_welcome(&mut self) {
        let theme = &self.themes[0];
        let tab = &mut self.tabs[0];

        tab.write("echOS Terminal v1.0\n", theme);
        tab.write("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\n", theme);
        tab.write("Welcome to echOS Terminal!\n\n", theme);
        tab.write("Built-in commands:\n", theme);
        tab.write("  help     - Show this help message\n", theme);
        tab.write("  clear    - Clear the terminal\n", theme);
        tab.write("  theme    - Change color theme\n", theme);
        tab.write("  ls       - List directory contents\n", theme);
        tab.write("  cd       - Change directory\n", theme);
        tab.write("  pwd      - Print working directory\n", theme);
        tab.write("  echo     - Print text\n", theme);
        tab.write("  date     - Show current date/time\n", theme);
        tab.write("  uname    - Show system info\n", theme);
        tab.write("\n", theme);
        tab.write("$ ", theme);
    }

    /// Yeni terminal sekmesi açar.
    /// `next_tab_id` artırılarak benzersiz ID garanti edilir.
    pub fn new_tab(&mut self) {
        let theme = &self.themes[0];
        let tab = TerminalTab::new(self.next_tab_id, theme);
        self.next_tab_id += 1;
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
    }

    /// Belirtilen sekmeyi kapatır.
    /// En az bir sekme kalması garanti edilir (`tabs.len() > 1` kontrolü).
    /// Aktif sekme kapatılırsa en sona gidilir.
    pub fn close_tab(&mut self, index: usize) {
        if self.tabs.len() > 1 {
            self.tabs.remove(index);
            if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len() - 1;
            }
        }
    }

    /// Belirtilen sekmeyi aktif yapar.
    pub fn select_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active_tab = index;
        }
    }

    /// Bir sonraki sekmeye geçer (dairesel).
    /// `% self.tabs.len()`: Rust'ta modülo operatörü, sona ulaşıldığında
    /// başa döner.
    pub fn next_tab(&mut self) {
        self.active_tab = (self.active_tab + 1) % self.tabs.len();
    }

    /// Bir önceki sekmeye geçer (dairesel).
    pub fn prev_tab(&mut self) {
        self.active_tab = if self.active_tab == 0 { self.tabs.len() - 1 } else { self.active_tab - 1 };
    }

    /// Aktif sekme için temayı değiştirir.
    pub fn set_theme(&mut self, theme_index: usize) {
        if theme_index < self.themes.len() {
            self.tabs[self.active_tab].theme_index = theme_index;
        }
    }

    /// Aktif sekmenin temasını sırayla değiştirir.
    /// `(current + 1) % self.themes.len()`: tema listesinde dairesel dolaşım.
    pub fn cycle_theme(&mut self) {
        let current = self.tabs[self.active_tab].theme_index;
        let next = (current + 1) % self.themes.len();
        self.set_theme(next);
    }

    /// Terminal durumunu günceller (her kare çağrılır).
    ///
    /// `dt` (delta time): son kare ile bu kare arasındaki süre (saniye).
    /// Kursor yanıp sönmesi ve zil zamanlayıcısı bu değere göre güncellenir.
    /// Delta zamanı kullanmak, farklı kare hızlarında tutarlı animasyon sağlar.
    pub fn update(&mut self, dt: f32) {
        // Cursor blink
        self.cursor_blink_timer += dt;
        if self.cursor_blink_timer >= 0.5 {
            self.cursor_blink_timer = 0.0;
            self.cursor_visible = !self.cursor_visible;
        }

        // Bell timer
        for tab in &mut self.tabs {
            if tab.bell {
                tab.bell_timer -= dt;
                if tab.bell_timer <= 0.0 {
                    tab.bell = false;
                }
            }
        }
    }

    /// Terminal penceresini framebuffer'a çizer.
    ///
    /// WM (pencere yöneticisi) başlık çubuğunu kendisi sağlar;
    /// bu yüzden dahili sekme çubuğu çizilmez.
    /// Tüm içerik `draw_content()` ile çizilir.
    pub fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let w = self.rect.width as usize;
        let h = self.rect.height as usize;

        let tab = &self.tabs[self.active_tab];
        let theme = &self.themes[tab.theme_index];

        // Window background (terminal bg)
        fb.draw_rect(x, y, w, h, theme.background);
        fb.draw_rect_outline(x, y, w, h, theme.background);

        // Terminal content — no internal tab bar; WM Cyber titlebar is the chrome
        let content_y = y;
        let content_h = h;

        self.draw_content(fb, x, content_y, w, content_h, theme, tab);
    }

    /// Sekme çubuğunu çizer.
    ///
    /// Her sekme için `tab_width` hesaplanır; toplam genişlik `cols` sekmeye eşit bölünür.
    /// `blend_color` kullanılarak hover efekti uygulanır.
    fn draw_tabs(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, theme: &TerminalTheme) {
        let tab_width = 150.min(w / self.tabs.len().max(1));
        let mut tab_x = x + 8;

        for (i, tab) in self.tabs.iter().enumerate() {
            let is_active = i == self.active_tab;
            let is_hovered = self.hovered_tab == Some(i);

            let bg = if is_active { theme.background }
                     else if is_hovered { Self::blend_color(theme.background, theme.foreground, 0.1) }
                     else { Self::blend_color(theme.background, theme.foreground, 0.05) };

            fb.draw_rect(tab_x, y, tab_width, TAB_BAR_HEIGHT, bg);

            // Tab title
            let title = if tab.title.len() > 12 { format!("{}...", &tab.title[..9]) } else { tab.title.clone() };
            fb.draw_string(tab_x + 8, y + 6, &title, theme.foreground);

            // Close button
            if self.tabs.len() > 1 {
                fb.draw_string(tab_x + tab_width - 20, y + 6, "×", theme.foreground);
            }

            // Bell indicator
            if tab.bell {
                fb.draw_string(tab_x + tab_width - 36, y + 6, "🔔", 0xFFFFAA00);
            }

            tab_x += tab_width + 2;
        }

        // New tab button
        fb.draw_string(tab_x + 4, y + 6, "+", theme.foreground);
    }

    /// Terminal içeriğini (hücreler + kursor + seçim) çizer.
    ///
    /// Her hücre için:
    /// 1. Arka plan rengi tema ile aynıysa atlanır (optimizasyon)
    /// 2. Karakter varsa (boşluk değil) çizilir
    /// 3. `reverse` niteliği: ön plan ve arka plan renkleri değiştirilir
    ///
    /// Seçim çizimi: `unsafe` blok kullanarak framebuffer piksellerine
    /// doğrudan erişilir ve alfa karışımı (blend) uygulanır.
    fn draw_content(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize, theme: &TerminalTheme, tab: &TerminalTab) {
        let char_w = tab.char_width;
        let line_h = tab.line_height;

        // Draw cells
        for (row_idx, row) in tab.grid.iter().enumerate() {
            let row_y = y + row_idx * line_h;

            if row_y + line_h > y + h {
                break;
            }

            for (col_idx, cell) in row.iter().enumerate() {
                let cell_x = x + col_idx * char_w;

                if cell_x + char_w > x + w {
                    break;
                }

                // Background
                if cell.bg_color != theme.background {
                    fb.draw_rect(cell_x, row_y, char_w, line_h, cell.bg_color);
                }

                // Character
                if cell.char != ' ' {
                    let fg = if cell.reverse { cell.bg_color } else { cell.fg_color };
                    fb.draw_char(cell_x, row_y, cell.char, fg);
                }
            }
        }

        // Draw cursor
        if self.cursor_visible && tab.cursor_visible {
            let cursor_x = x + tab.cursor_x * char_w;
            let cursor_y = y + tab.cursor_y * line_h;

            // Cursor block
            fb.draw_rect(cursor_x, cursor_y, char_w, line_h, theme.cursor);
        }

        // Draw selection
        if let (Some(start), Some(end)) = (tab.selection_start, tab.selection_end) {
            let (sx, sy) = start;
            let (ex, ey) = end;

            let min_y = sy.min(ey);
            let max_y = sy.max(ey);

            for row_y in min_y..=max_y {
                let min_x = if row_y == min_y { sx.min(ex) } else { 0 };
                let max_x = if row_y == max_y { sx.max(ex) } else { tab.cols };

                let sel_x = x + min_x * char_w;
                let sel_y = y + row_y * line_h;
                let sel_w = (max_x - min_x) * char_w;

                // Selection overlay
                for py in 0..line_h {
                    for px in 0..sel_w {
                        let ptr = unsafe {
                            (fb.base_addr as *mut u32).add((sel_y + py) * fb.pixels_per_scan_line + sel_x + px)
                        };
                        let bg = unsafe { *ptr };
                        unsafe { *ptr = Self::blend_color(bg, theme.selection_bg, 0.5); }
                    }
                }
            }
        }
    }

    /// İki rengi verilen alfa değeriyle karıştırır (LERP).
    ///
    /// Formül: `result = bg * (1 - alpha) + fg * alpha`
    /// R, G, B kanalları ayrı ayrı hesaplanır, ardından birleştirilir.
    /// `>> 16`, `>> 8`, `& 0xFF`: bit kaydırma ile renk kanalları ayrıştırılır.
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

    /// Klavye girişini işler.
    ///
    /// - Enter (`'\n'`/`'\r'`): komutu çalıştır → `execute_command()`
    /// - Geri tuşu (`'\x08'`): son karakteri sil ve ızgaradan kaldır
    /// - Kaçış (`'\x1b'`): şimdilik yok sayılır (ANSI kaçış dizileri için yorumlanacak)
    /// - Diğer yazdırılabilir karakterler: giriş tamponuna ve ızgaraya ekle
    pub fn on_key_press(&mut self, c: char) -> TerminalAction {
        let theme = self.themes[self.tabs[self.active_tab].theme_index].clone();

        match c {
            '\n' | '\r' => {
                // Execute command
                let cmd = self.tabs[self.active_tab].input_buffer.trim().to_string();

                if !cmd.is_empty() {
                    self.tabs[self.active_tab].write("\n", &theme);
                    self.execute_command(&cmd, &theme);
                } else {
                    self.tabs[self.active_tab].write("\n$ ", &theme);
                }

                self.tabs[self.active_tab].input_buffer.clear();
            }
            '\x08' => { // Backspace
                let tab = &mut self.tabs[self.active_tab];
                if !tab.input_buffer.is_empty() {
                    tab.input_buffer.pop();
                    if tab.cursor_x > 0 {
                        tab.cursor_x -= 1;
                        tab.grid[tab.cursor_y][tab.cursor_x] = TerminalCell::default(&theme);
                    }
                }
            }
            '\x1b' => { // Escape - could be start of escape sequence
                // For now, just ignore
            }
            _ if !c.is_control() => {
                let tab = &mut self.tabs[self.active_tab];
                tab.input_buffer.push(c);
                tab.write_char(c, &theme);
            }
            _ => {}
        }

        TerminalAction::None
    }

    /// Komutu işler ve çıktıyı terminal ekranına yazar.
    ///
    /// İki katmanlı işleme:
    /// 1. Terminal-yerel komutlar (tema yönetimi, pwd, exit): burada işlenir
    /// 2. Diğer komutlar: `crate::shell::run_command(cmd)` ile kernel shell'e aktarılır
    ///
    /// `__CLEAR__` özel çıktısı: kernel shell'in ekranı temizle sinyali.
    fn execute_command(&mut self, cmd: &str, theme: &TerminalTheme) {
        let parts: Vec<&str> = cmd.split_whitespace().collect();

        // Terminal-yerel komutlar (tema yönetimi ve çıkış — kernel shell bilmez)
        if let Some(&first) = parts.first() {
            match first {
                "theme" => {
                    let tab = &mut self.tabs[self.active_tab];
                    if parts.len() > 1 {
                        if let Ok(idx) = parts[1].parse::<usize>() {
                            if idx < self.themes.len() {
                                tab.theme_index = idx;
                                tab.write(&format!("Tema degistirildi: {}\n$ ", self.themes[idx].name), theme);
                            } else {
                                tab.write("Gecersiz tema indeksi\n$ ", theme);
                            }
                        }
                    } else {
                        tab.write("Temalar:\n", theme);
                        let theme_list: alloc::vec::Vec<alloc::string::String> = self.themes
                            .iter()
                            .enumerate()
                            .map(|(i, t)| format!("  {} - {}\n", i, t.name))
                            .collect();
                        for s in theme_list { tab.write(&s, theme); }
                        tab.write("$ ", theme);
                    }
                    return;
                }
                "pwd" => {
                    let cwd = self.tabs[self.active_tab].cwd.clone();
                    let tab = &mut self.tabs[self.active_tab];
                    tab.write(&format!("{}\n$ ", cwd), theme);
                    return;
                }
                "exit" | "quit" => {
                    let tab = &mut self.tabs[self.active_tab];
                    tab.write("Gorusuruz!\n", theme);
                    return;
                }
                _ => {}
            }
        }

        // Kernel Shell engine'e devret (Faz 7 köprüsü)
        let output = crate::shell::run_command(cmd);

        let tab = &mut self.tabs[self.active_tab];
        match output {
            None => {
                // Komut bir çıktı üretmedi (örn. set, export)
            }
            Some(ref o) if o == "__CLEAR__" => {
                tab.clear(theme);
                // Prompt'u clear sonrası tekrar yaz
                tab.write("$ ", theme);
                return;
            }
            Some(o) => {
                tab.write(&o, theme);
                tab.write("\n", theme);
            }
        }
        tab.write("$ ", theme);
    }

    /// Fare tıklamasını işler.
    ///
    /// Sekme çubuğu tıklamaları: sekme seç veya kapat.
    /// Yeni sekme düğmesi: artı `+` simgesi.
    pub fn on_click(&mut self, mx: i32, my: i32) -> TerminalAction {
        let x = self.rect.x;
        let y = self.rect.y;
        let w = self.rect.width;

        // Tab bar
        if my >= y && my < y + TAB_BAR_HEIGHT as i32 {
            let tab_width = 150.min(w as usize / self.tabs.len().max(1)) as i32;
            let mut tab_x = x + 8;

            for i in 0..self.tabs.len() {
                if mx >= tab_x && mx < tab_x + tab_width {
                    // Close button
                    if mx > tab_x + tab_width - 20 && self.tabs.len() > 1 {
                        self.close_tab(i);
                    } else {
                        self.select_tab(i);
                    }
                    return TerminalAction::None;
                }
                tab_x += tab_width + 2;
            }

            // New tab
            if mx >= tab_x {
                self.new_tab();
                let theme = self.themes[self.tabs[self.active_tab].theme_index].clone();
                self.tabs[self.active_tab].write("$ ", &theme);
            }
        }

        TerminalAction::None
    }

    /// Pencereyi yeniden boyutlandırır.
    ///
    /// Piksel boyutundan hücre boyutuna dönüşüm:
    /// `cols = width / 8` (sabit 8 piksel karakter genişliği)
    /// `rows = content_h / 16` (sabit 16 piksel satır yüksekliği)
    pub fn resize(&mut self, width: usize, height: usize) {
        self.rect.width = width as i32;
        self.rect.height = height as i32;

        // Recalculate terminal size
        let content_h = height - TAB_BAR_HEIGHT;
        let theme = self.themes[0].clone();

        let cols = width / 8;
        let rows = content_h / 16;

        for tab in &mut self.tabs {
            tab.resize(cols, rows, &theme);
        }
    }
}

/// Terminal uygulamasından dönen olaylar.
///
/// `CommandExecuted(String)`: çalıştırılan komut metnini taşır.
/// `TabClosed(u32)`: kapatılan sekmenin ID'sini taşır.
/// `ExitRequested`: terminal kapatma isteği.
#[derive(Clone, Debug)]
pub enum TerminalAction {
    None,
    CommandExecuted(String),
    TabClosed(u32),
    ExitRequested,
}

// ============================================================================
// GLOBAL TERMINAL
// ============================================================================

/// Global terminal penceresi singleton'ı.
///
/// `lazy_static!` + `spin::Mutex`: ilk kullanımda başlatılan global
/// değişken; `no_std` ortamında çoklu çekirdek güvenli erişim.
/// Başlangıç pencere konumu ve boyutu `Rect` struct literal ile verilir.
lazy_static::lazy_static! {
    static ref TERMINAL: Mutex<TerminalWindow> = Mutex::new(TerminalWindow::new(Rect {
        x: 100,
        y: 100,
        width: 800,
        height: 500,
    }));
}

/// Modülü başlatır ve seri porta log yazar.
pub fn init() {
    crate::serial_println!("[GUI] Terminal initialized");
}

/// Global terminal penceresine referans döndürür.
pub fn get_terminal() -> &'static Mutex<TerminalWindow> {
    &TERMINAL
}
