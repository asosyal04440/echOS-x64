//! ANSI Escape Sequence İşleyici
//!
//! VT100/ANSI terminal escape sequence desteği.
//! Renkler, imleç kontrolü, ekran temizleme vb.
//!
//! ## ANSI Escape Sequence Nedir?
//!
//! Terminal uygulamaları, düz metin dışında imleç hareketi, renk değişimi
//! gibi özel komutları bambaşka bir mekanizmayla iletir: "escape sequences".
//! Bu diziler ESC karakteri (0x1B, yani '\x1B') ile başlar.
//!
//! ## Escape Sequence Formatı (ASCII Diyagramı)
//!
//! ```
//!  ESC  [   param1 ; param2   final_byte
//!  0x1B 0x5B  (sayılar)       (harf)
//!  ──── ───── ──────────────  ──────────
//!   |    |         |               |
//!   |    |     Noktalı virgülle   Hangi işlem yapılacağını belirler:
//!   |    |    ayrılmış değerler    A=yukarı, B=aşağı, H=konum, m=renk...
//!   |    |
//!   |   CSI (Control Sequence Introducer) = Kontrol dizisi başlangıcı
//!   |
//!  ESC = 0x1B, kaçış karakteri
//!
//!  Örnekler:
//!   ESC[31m       --> Kırmızı ön plan rengi (SGR: Select Graphic Rendition)
//!   ESC[2J        --> Ekranı temizle (Erase in Display: tümü)
//!   ESC[10;20H    --> İmleci satır 10, sütun 20'ye taşı
//!   ESC[?25l      --> İmleci gizle (Private Mode: cursor invisible)
//!   ESC]0;başlıkBEL --> Pencere başlığını değiştir (OSC: Operating System Command)
//! ```
//!
//! ## Parser Durum Makinesi
//!
//! ```
//!  Normal --> ESC alındı --> CSI --> CsiParams --> İşle
//!    |           |           |
//!    |           |           +--> ESC]  --> OSC --> OscParam
//!    |           |
//!    |           +--> Bilinmeyen --> Normal (Unknown sequence)
//!    |
//!   Kontrol karakterleri (BEL, BS, HT, LF, CR) doğrudan işlenir
//! ```

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::fmt::Write;

/// ANSI Renk sistemi — 3/4-bit, 8-bit (256), ve 24-bit (True Color) destekli.
///
/// Standart 16 renk + 256-renk paleti + RGB kullanılabilir.
/// SGR (Select Graphic Rendition) komutu ile ayarlanır:
/// - `ESC[38;5;Nm`  → 256-renk ön plan
/// - `ESC[48;5;Nm`  → 256-renk arka plan
/// - `ESC[38;2;R;G;Bm` → RGB ön plan (True Color)
/// - `ESC[48;2;R;G;Bm` → RGB arka plan (True Color)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    Black = 0,
    Red = 1,
    Green = 2,
    Yellow = 3,
    Blue = 4,
    Magenta = 5,
    Cyan = 6,
    White = 7,
    BrightBlack = 8,
    BrightRed = 9,
    BrightGreen = 10,
    BrightYellow = 11,
    BrightBlue = 12,
    BrightMagenta = 13,
    BrightCyan = 14,
    BrightWhite = 15,
    /// 256-renk paleti (0-255 indeks)
    Palette256 = 16,
    /// 24-bit RGB (True Color)
    Rgb = 17,
    Default = 255,
}

/// Genişletilmiş renk bilgisi — 256-renk ve RGB değerlerini taşır
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtendedColor {
    /// Temel renk türü
    pub color: Color,
    /// 256-renk indeksi (0-255) — Color::Palette256 için
    pub index: u8,
    /// RGB bileşenleri — Color::Rgb için
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl ExtendedColor {
    pub const fn default_fg() -> Self {
        Self {
            color: Color::Default,
            index: 0,
            r: 255,
            g: 255,
            b: 255,
        }
    }
    pub const fn default_bg() -> Self {
        Self {
            color: Color::Default,
            index: 0,
            r: 0,
            g: 0,
            b: 0,
        }
    }
    pub const fn from_standard(c: Color) -> Self {
        Self {
            color: c,
            index: c as u8,
            r: 0,
            g: 0,
            b: 0,
        }
    }
    pub const fn from_256(index: u8) -> Self {
        Self {
            color: Color::Palette256,
            index,
            r: 0,
            g: 0,
            b: 0,
        }
    }
    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self {
            color: Color::Rgb,
            index: 0,
            r,
            g,
            b,
        }
    }
    /// 256 paletten veya RGB'den 32-bit ARGB değerine dönüştür
    pub fn to_argb(&self) -> u32 {
        match self.color {
            Color::Rgb => 0xFF000000 | (self.r as u32) << 16 | (self.g as u32) << 8 | self.b as u32,
            Color::Palette256 => palette_256_to_argb(self.index),
            _ => standard_color_to_argb(self.color),
        }
    }
}

/// 256-renk paletinden ARGB'ye çevir
fn palette_256_to_argb(idx: u8) -> u32 {
    match idx {
        0 => 0xFF000000,
        1 => 0xFFAA0000,
        2 => 0xFF00AA00,
        3 => 0xFFAA5500,
        4 => 0xFF0000AA,
        5 => 0xFFAA00AA,
        6 => 0xFF00AAAA,
        7 => 0xFFAAAAAA,
        8 => 0xFF555555,
        9 => 0xFFFF5555,
        10 => 0xFF55FF55,
        11 => 0xFFFFFF55,
        12 => 0xFF5555FF,
        13 => 0xFFFF55FF,
        14 => 0xFF55FFFF,
        15 => 0xFFFFFFFF,
        // 16-231: 6×6×6 renk küpü
        16..=231 => {
            let n = (idx - 16) as u32;
            let b = (n % 6) * 51;
            let g = ((n / 6) % 6) * 51;
            let r = (n / 36) * 51;
            0xFF000000 | (r << 16) | (g << 8) | b
        }
        // 232-255: Gri tonları
        _ => {
            let v = ((idx as u32 - 232) * 10 + 8).min(255);
            0xFF000000 | (v << 16) | (v << 8) | v
        }
    }
}

/// Standart 16 renk → ARGB
fn standard_color_to_argb(c: Color) -> u32 {
    match c {
        Color::Black => 0xFF000000,
        Color::Red => 0xFFAA0000,
        Color::Green => 0xFF00AA00,
        Color::Yellow => 0xFFAA5500,
        Color::Blue => 0xFF0000AA,
        Color::Magenta => 0xFFAA00AA,
        Color::Cyan => 0xFF00AAAA,
        Color::White => 0xFFAAAAAA,
        Color::BrightBlack => 0xFF555555,
        Color::BrightRed => 0xFFFF5555,
        Color::BrightGreen => 0xFF55FF55,
        Color::BrightYellow => 0xFFFFFF55,
        Color::BrightBlue => 0xFF5555FF,
        Color::BrightMagenta => 0xFFFF55FF,
        Color::BrightCyan => 0xFF55FFFF,
        Color::BrightWhite => 0xFFFFFFFF,
        _ => 0xFFAAAAAA,
    }
}

/// ANSI Escape Sequence tipleri
///
/// Her varyant, terminal tarafından desteklenen bir kontrol komutunu temsil eder.
/// Bu enum, parser'dan çıkan sonuçları ve builder'a verilen girdileri tanımlar.
#[derive(Clone, Debug, PartialEq)]
pub enum EscapeSequence {
    /// Cursor Position: ESC[<row>;<col>H
    /// İmleci belirtilen satır ve sütuna taşır (1-tabanlı indeksleme)
    CursorPosition { row: u16, col: u16 },
    /// Cursor Up: ESC[<n>A
    /// İmleci n satır yukarı taşır
    CursorUp(u16),
    /// Cursor Down: ESC[<n>B
    /// İmleci n satır aşağı taşır
    CursorDown(u16),
    /// Cursor Forward: ESC[<n>C
    /// İmleci n sütun ileri (sağa) taşır
    CursorForward(u16),
    /// Cursor Back: ESC[<n>D
    /// İmleci n sütun geri (sola) taşır
    CursorBack(u16),
    /// Cursor Next Line: ESC[<n>E
    /// İmleci n satır aşağıya ve satır başına taşır
    CursorNextLine(u16),
    /// Cursor Previous Line: ESC[<n>F
    /// İmleci n satır yukarıya ve satır başına taşır
    CursorPreviousLine(u16),
    /// Cursor Horizontal Absolute: ESC[<n>G
    /// İmleci mevcut satırın n. sütununa taşır
    CursorHorizontalAbsolute(u16),
    /// Erase in Display: ESC[<n>J
    /// n=0: imlecden ekran sonuna, n=1: baştan imlece, n=2: tüm ekran
    EraseInDisplay(u8),
    /// Erase in Line: ESC[<n>K
    /// n=0: imlecden satır sonuna, n=1: satır başından imlece, n=2: tüm satır
    EraseInLine(u8),
    /// Scroll Up: ESC[<n>S
    /// Ekranı n satır yukarı kaydırır (üst satırlar kaybolur)
    ScrollUp(u16),
    /// Scroll Down: ESC[<n>T
    /// Ekranı n satır aşağı kaydırır (alt satırlar kaybolur)
    ScrollDown(u16),
    /// Select Graphic Rendition (renkler ve stiller)
    /// ESC[<params>m formatında; params yerine renk/stil kodları gelir
    SelectGraphicRendition(Vec<u8>),
    /// Set Title: ESC]0;<title>BEL
    /// Terminal pencere başlığını değiştirir (OSC komutu)
    SetTitle(String),
    /// Save Cursor Position: ESC[s
    /// Mevcut imleç konumunu kaydeder (daha sonra geri yüklenebilir)
    SaveCursorPosition,
    /// Restore Cursor Position: ESC[u
    /// Daha önce kaydedilen imleç konumunu geri yükler
    RestoreCursorPosition,
    /// Show Cursor: ESC[?25h
    /// İmleci görünür hale getirir (Private Mode set)
    ShowCursor,
    /// Hide Cursor: ESC[?25l
    /// İmleci gizler (Private Mode reset)
    HideCursor,
    /// Enable Alternative Screen Buffer: ESC[?1049h
    /// Alternatif ekran tamponunu etkinleştirir (vim, less gibi uygulamalar kullanır)
    EnableAltScreen,
    /// Disable Alternative Screen Buffer: ESC[?1049l
    /// Alternatif ekran tamponunu devre dışı bırakır, normal ekrana döner
    DisableAltScreen,
    /// Bell: BEL (0x07)
    /// Zil sesi çalar veya terminal uyarısı verir
    Bell,
    /// Backspace: BS (0x08)
    /// İmleci bir karakter geri taşır (karakteri silmez!)
    Backspace,
    /// Tab: HT (0x09)
    /// Bir sonraki sekme durağına (tab stop) ilerler
    Tab,
    /// Line Feed: LF (0x0A)
    /// Yeni satıra geçer (Unix'te yeni satır karakteri olarak da kullanılır)
    LineFeed,
    /// Carriage Return: CR (0x0D)
    /// İmleci satırın başına taşır (Windows'ta CR+LF çifti yeni satır demektir)
    CarriageReturn,
    /// Unknown/Unsupported
    /// Tanınmayan ya da desteklenmeyen escape sequence
    Unknown(Vec<u8>),
}

/// ANSI Parser Durum Makinesi Durumları
///
/// Parser, gelen byte akışını aşağıdaki durumlar arasında geçiş yaparak işler.
/// Her durum, hangi karakterlerin bekleneceğini ve ne yapılacağını belirler.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ParserState {
    /// Normal metin modu: düz karakterler işlenir
    Normal,
    /// ESC karakteri alındı, sonraki karaktere bakılıyor
    Escape,
    /// ESC[ alındı - CSI (Control Sequence Introducer) başladı
    /// Parametreler ya da final byte bekleniyor
    Csi,
    /// ESC[<params> alındı - parametreler biriktirildi, final byte bekleniyor
    CsiParams,
    /// ESC] alındı - OSC (Operating System Command) başladı
    /// BEL veya ESC ile sonlanacak
    Osc,
    /// ESC]<param> alındı - OSC parametresi işleniyor
    OscParam,
}

/// ANSI Escape Sequence Ayrıştırıcı (Parser)
///
/// Byte tabanlı stream'den ANSI escape sequence'larını ayrıştırır.
/// Her `feed()` çağrısı bir byte alır ve tamamlanan sequence'ları döndürür.
///
/// Kullanım örneği:
/// ```
/// let mut parser = AnsiParser::new();
/// for byte in data.iter() {
///     if let Some(seq) = parser.feed(*byte) {
///         // Tamamlanan sequence işlendi
///     }
/// }
/// ```
pub struct AnsiParser {
    /// Mevcut parser durumu (durum makinesi)
    state: ParserState,
    /// Genel byte tamponu (CSI parametreleri için karakter setleri)
    buffer: Vec<u8>,
    /// CSI parametre byte'ları biriktirilir (örn: "10;20" -> [49,48,59,50,48])
    params: Vec<u8>,
    /// OSC (pencere başlığı vb.) içeriği biriktirilir
    osc_buffer: Vec<u8>,
}

impl AnsiParser {
    pub fn new() -> Self {
        Self {
            state: ParserState::Normal,
            buffer: Vec::new(),
            params: Vec::new(),
            osc_buffer: Vec::new(),
        }
    }

    /// Byte'ı parse eder ve tamamlanan sequence'ları döndürür.
    ///
    /// Durum makinesinin temel metodudur. Her byte için bir kez çağrılır.
    /// Sequence tamamlandığında `Some(EscapeSequence)` döndürür,
    /// aksi hâlde `None` döndürür (daha fazla byte bekleniyor).
    pub fn feed(&mut self, byte: u8) -> Option<EscapeSequence> {
        match self.state {
            ParserState::Normal => {
                match byte {
                    0x1B => {
                        // ESC - escape sequence başlıyor
                        self.state = ParserState::Escape;
                        self.buffer.clear();
                        self.params.clear();
                        None
                    }
                    0x07 => Some(EscapeSequence::Bell),
                    0x08 => Some(EscapeSequence::Backspace),
                    0x09 => Some(EscapeSequence::Tab),
                    0x0A => Some(EscapeSequence::LineFeed),
                    0x0D => Some(EscapeSequence::CarriageReturn),
                    _ => None,
                }
            }
            ParserState::Escape => {
                match byte {
                    b'[' => {
                        // ESC[ -> CSI başladı (en yaygın escape sequence tipi)
                        self.state = ParserState::Csi;
                        None
                    }
                    b']' => {
                        // ESC] -> OSC başladı (pencere başlığı, renk paleti vb.)
                        self.state = ParserState::Osc;
                        self.osc_buffer.clear();
                        None
                    }
                    b'(' | b')' | b'*' | b'+' => {
                        // Character set selection - ignore next char
                        // Karakter seti seçimi (G0/G1/G2/G3 karakter setleri)
                        // VT100 döneminden kalan, artık nadiren kullanılan özellik
                        self.buffer.push(byte);
                        None
                    }
                    _ => {
                        // Bilinmeyen escape sequence - normale dön
                        self.state = ParserState::Normal;
                        Some(EscapeSequence::Unknown(vec![0x1B, byte]))
                    }
                }
            }
            ParserState::Csi => {
                match byte {
                    b'0'..=b'9' | b';' | b'?' => {
                        // Parametre baytları biriktirildi, CsiParams durumuna geç
                        self.params.push(byte);
                        self.state = ParserState::CsiParams;
                        None
                    }
                    b'A' => {
                        self.state = ParserState::Normal;
                        Some(EscapeSequence::CursorUp(self.parse_single_param(1)))
                    }
                    b'B' => {
                        self.state = ParserState::Normal;
                        Some(EscapeSequence::CursorDown(self.parse_single_param(1)))
                    }
                    b'C' => {
                        self.state = ParserState::Normal;
                        Some(EscapeSequence::CursorForward(self.parse_single_param(1)))
                    }
                    b'D' => {
                        self.state = ParserState::Normal;
                        Some(EscapeSequence::CursorBack(self.parse_single_param(1)))
                    }
                    b'E' => {
                        self.state = ParserState::Normal;
                        Some(EscapeSequence::CursorNextLine(self.parse_single_param(1)))
                    }
                    b'F' => {
                        self.state = ParserState::Normal;
                        Some(EscapeSequence::CursorPreviousLine(
                            self.parse_single_param(1),
                        ))
                    }
                    b'G' => {
                        self.state = ParserState::Normal;
                        Some(EscapeSequence::CursorHorizontalAbsolute(
                            self.parse_single_param(1),
                        ))
                    }
                    b'H' | b'f' => {
                        // H ve f aynı anlama gelir: imleç konumlandırma
                        self.state = ParserState::Normal;
                        let (row, col) = self.parse_cursor_position();
                        Some(EscapeSequence::CursorPosition { row, col })
                    }
                    b'J' => {
                        self.state = ParserState::Normal;
                        Some(EscapeSequence::EraseInDisplay(
                            self.parse_single_param(0) as u8
                        ))
                    }
                    b'K' => {
                        self.state = ParserState::Normal;
                        Some(EscapeSequence::EraseInLine(self.parse_single_param(0) as u8))
                    }
                    b'S' => {
                        self.state = ParserState::Normal;
                        Some(EscapeSequence::ScrollUp(self.parse_single_param(1)))
                    }
                    b'T' => {
                        self.state = ParserState::Normal;
                        Some(EscapeSequence::ScrollDown(self.parse_single_param(1)))
                    }
                    b'm' => {
                        // SGR - Select Graphic Rendition: renk ve stil kodları
                        // Örnek: ESC[1;31m = kalın + kırmızı
                        self.state = ParserState::Normal;
                        Some(EscapeSequence::SelectGraphicRendition(
                            self.parse_sgr_params(),
                        ))
                    }
                    b's' => {
                        self.state = ParserState::Normal;
                        Some(EscapeSequence::SaveCursorPosition)
                    }
                    b'u' => {
                        self.state = ParserState::Normal;
                        Some(EscapeSequence::RestoreCursorPosition)
                    }
                    b'h' | b'l' => {
                        // h = set mode (etkinleştir), l = reset mode (devre dışı bırak)
                        // ? ile birlikte kullanılır: ESC[?25h = imleç göster
                        self.state = ParserState::Normal;
                        self.parse_mode(byte == b'h')
                    }
                    _ => {
                        self.state = ParserState::Normal;
                        Some(EscapeSequence::Unknown(self.params.clone()))
                    }
                }
            }
            ParserState::CsiParams => {
                match byte {
                    b'0'..=b'9' | b';' | b'?' => {
                        // Parametre baytları biriktirilmeye devam ediyor
                        self.params.push(byte);
                        None
                    }
                    _ => {
                        // Final byte - sequence tamamlandı, işle
                        let params = self.params.clone();
                        self.state = ParserState::Normal;
                        self.process_csi_final(byte, params)
                    }
                }
            }
            ParserState::Osc => {
                match byte {
                    0x07 | 0x1B => {
                        // OSC terminated by BEL or ESC
                        // OSC, BEL (0x07) karakteri ya da ESC ile sonlanır
                        self.state = ParserState::Normal;
                        let title = self.parse_osc_title();
                        Some(EscapeSequence::SetTitle(title))
                    }
                    _ => {
                        // OSC içeriği biriktirilmeye devam ediyor
                        self.osc_buffer.push(byte);
                        None
                    }
                }
            }
            ParserState::OscParam => {
                // OscParam durumu: basit geçiş, tam uygulama gelecek sürümde
                self.state = ParserState::Normal;
                None
            }
        }
    }

    /// Tek parametre parse eder.
    ///
    /// Parametre buffer'ında sayı yoksa verilen `default` değeri döner.
    /// Parametreler ASCII rakam olarak saklanır: "42" -> [0x34, 0x32] -> 42
    fn parse_single_param(&self, default: u16) -> u16 {
        let mut num: u16 = 0;
        let mut found = false;
        for &b in &self.params {
            if b >= b'0' && b <= b'9' {
                num = num.saturating_mul(10).saturating_add((b - b'0') as u16);
                found = true;
            } else if b == b';' {
                break;
            }
        }
        if found {
            num
        } else {
            default
        }
    }

    /// Cursor position parse eder.
    ///
    /// "satır;sütun" formatındaki parametreden (row, col) çifti çıkarır.
    /// Değer 0 ise 1 kabul edilir (ANSI standardı gereği 1-tabanlı indeks).
    fn parse_cursor_position(&self) -> (u16, u16) {
        let mut row: u16 = 1;
        let mut col: u16 = 1;
        let mut current: u16 = 0;
        let mut first_param = true;

        for &b in &self.params {
            if b >= b'0' && b <= b'9' {
                current = current.saturating_mul(10).saturating_add((b - b'0') as u16);
            } else if b == b';' {
                if first_param {
                    row = if current == 0 { 1 } else { current };
                    current = 0;
                    first_param = false;
                }
            }
        }
        col = if current == 0 { 1 } else { current };

        (row, col)
    }

    /// SGR (Select Graphic Rendition) parametrelerini parse eder.
    ///
    /// SGR, terminal metin özelliklerini (renk, kalın, altı çizili vb.) ayarlar.
    /// Parametreler noktalı virgülle ayrılır: ESC[1;31;42m -> [1, 31, 42]
    /// - 0: Sıfırla (varsayılana dön)
    /// - 1: Kalın, 2: Soluk, 3: İtalik, 4: Altı çizili
    /// - 30-37: Ön plan rengi (standart), 90-97: Ön plan rengi (parlak)
    /// - 40-47: Arka plan rengi (standart), 100-107: Arka plan rengi (parlak)
    /// - 38;5;n: 256 renk ön plan, 38;2;r;g;b: Gerçek renk (True Color) ön plan
    fn parse_sgr_params(&self) -> Vec<u8> {
        let mut result = Vec::new();
        let mut current: u8 = 0;
        let mut found = false;

        for &b in &self.params {
            if b >= b'0' && b <= b'9' {
                current = current.saturating_mul(10).saturating_add(b - b'0');
                found = true;
            } else if b == b';' {
                result.push(current);
                current = 0;
            }
        }
        if found {
            result.push(current);
        }
        if result.is_empty() {
            result.push(0); // Reset - parametresiz ESC[m sıfırlama anlamına gelir
        }
        result
    }

    /// Mode parse eder.
    ///
    /// ESC[?<n>h = private mode set (etkinleştir)
    /// ESC[?<n>l = private mode reset (devre dışı bırak)
    /// En yaygın private mode'lar:
    /// - ?25: İmleç görünürlüğü
    /// - ?1049: Alternatif ekran tamponu
    fn parse_mode(&mut self, set: bool) -> Option<EscapeSequence> {
        let params = self.params.clone();
        if params.starts_with(b"?25") {
            if set {
                Some(EscapeSequence::ShowCursor)
            } else {
                Some(EscapeSequence::HideCursor)
            }
        } else if params.starts_with(b"?1049") {
            if set {
                Some(EscapeSequence::EnableAltScreen)
            } else {
                Some(EscapeSequence::DisableAltScreen)
            }
        } else {
            Some(EscapeSequence::Unknown(params))
        }
    }

    /// OSC title parse eder.
    ///
    /// OSC format: ESC]<komut>;<içerik>BEL
    /// Komut 0 = pencere başlığı ve ikon adı
    /// Komut 2 = yalnızca pencere başlığı
    /// Örnek: ESC]0;echOS TerminalBEL -> "echOS Terminal"
    fn parse_osc_title(&self) -> String {
        // OSC format: ]0;title<BEL>
        let s = String::from_utf8_lossy(&self.osc_buffer);
        if let Some(pos) = s.find(';') {
            s[pos + 1..].to_string()
        } else {
            s.to_string()
        }
    }

    /// CSI final byte işleme.
    ///
    /// Parametre biriktirilmesi tamamlandıktan sonra "final byte" (harf)
    /// ile sequence tamamlanır. Bu metod, kümülatif parametrelerle
    /// doğru `EscapeSequence` varyantını oluşturur.
    fn process_csi_final(&mut self, byte: u8, params: Vec<u8>) -> Option<EscapeSequence> {
        match byte {
            b'A' => Some(EscapeSequence::CursorUp(self.parse_single_param(1))),
            b'B' => Some(EscapeSequence::CursorDown(self.parse_single_param(1))),
            b'C' => Some(EscapeSequence::CursorForward(self.parse_single_param(1))),
            b'D' => Some(EscapeSequence::CursorBack(self.parse_single_param(1))),
            b'H' | b'f' => {
                let (row, col) = self.parse_cursor_position();
                Some(EscapeSequence::CursorPosition { row, col })
            }
            b'J' => Some(EscapeSequence::EraseInDisplay(
                self.parse_single_param(0) as u8
            )),
            b'K' => Some(EscapeSequence::EraseInLine(self.parse_single_param(0) as u8)),
            b'm' => Some(EscapeSequence::SelectGraphicRendition(
                self.parse_sgr_params(),
            )),
            _ => Some(EscapeSequence::Unknown(params)),
        }
    }
}

impl Default for AnsiParser {
    fn default() -> Self {
        Self::new()
    }
}

/// ANSI escape sequence oluşturucu (Builder).
///
/// String tabanlı terminal komutları üretir.
/// Bu struct, terminal protokolünün "yazma" tarafıdır;
/// `AnsiParser` ise "okuma" tarafıdır.
///
/// Kullanım:
/// - `AnsiBuilder::cursor_position(5, 10)` -> `"\x1B[5;10H"` string döndürür
/// - `AnsiBuilder::fg_color(Color::Red)` -> `"\x1B[31m"` string döndürür
pub struct AnsiBuilder;

impl AnsiBuilder {
    /// ESC karakteri (0x1B)
    /// Tüm ANSI escape sequence'larının başladığı özel kontrol karakteri
    pub const ESC: u8 = 0x1B;

    /// Cursor position: ESC[row;colH
    /// İmleci belirlenen satır ve sütuna taşır (1-tabanlı)
    pub fn cursor_position(row: u16, col: u16) -> String {
        alloc::format!("\x1B[{};{}H", row, col)
    }

    /// Cursor up: ESC[nA
    /// İmleci n satır yukarı taşır
    pub fn cursor_up(n: u16) -> String {
        alloc::format!("\x1B[{}A", n)
    }

    /// Cursor down: ESC[nB
    /// İmleci n satır aşağı taşır
    pub fn cursor_down(n: u16) -> String {
        alloc::format!("\x1B[{}B", n)
    }

    /// Cursor forward: ESC[nC
    /// İmleci n sütun ileri (sağa) taşır
    pub fn cursor_forward(n: u16) -> String {
        alloc::format!("\x1B[{}C", n)
    }

    /// Cursor back: ESC[nD
    /// İmleci n sütun geri (sola) taşır
    pub fn cursor_back(n: u16) -> String {
        alloc::format!("\x1B[{}D", n)
    }

    /// Erase display: ESC[nJ
    /// n=0: imlecden sona, n=1: baştan imlece, n=2: tüm ekran siler
    pub fn erase_display(mode: u8) -> String {
        alloc::format!("\x1B[{}J", mode)
    }

    /// Clear screen: ESC[2J + ESC[H
    /// Önce ekranı tamamen siler, sonra imleci sol üste (1,1) taşır
    pub fn clear_screen() -> String {
        "\x1B[2J\x1B[H".to_string()
    }

    /// Erase line: ESC[nK
    /// n=0: imlecden satır sonuna, n=1: satır başından imlece, n=2: tüm satır
    pub fn erase_line(mode: u8) -> String {
        alloc::format!("\x1B[{}K", mode)
    }

    /// Foreground color (standard): ESC[30-37m
    /// Standart 8 renkten ön plan rengini seçer
    pub fn fg_color(color: Color) -> String {
        let code = match color {
            Color::Default => 39,
            c => 30 + c as u8,
        };
        alloc::format!("\x1B[{}m", code)
    }

    /// Background color (standard): ESC[40-47m
    /// Standart 8 renkten arka plan rengini seçer
    pub fn bg_color(color: Color) -> String {
        let code = match color {
            Color::Default => 49,
            c => 40 + c as u8,
        };
        alloc::format!("\x1B[{}m", code)
    }

    /// Foreground color (bright): ESC[90-97m
    /// Parlak (yüksek yoğunluklu) versiyonu seçer
    pub fn fg_color_bright(color: Color) -> String {
        let code = match color {
            Color::Default => 39,
            c => {
                if c as u8 >= 8 {
                    90 + (c as u8 - 8)
                } else {
                    30 + c as u8
                }
            }
        };
        alloc::format!("\x1B[{}m", code)
    }

    /// 256-color foreground: ESC[38;5;<n>m
    /// xterm 256 renk paletinden ön plan rengi seçer (0-255 arası)
    pub fn fg_color_256(n: u8) -> String {
        alloc::format!("\x1B[38;5;{}m", n)
    }

    /// 256-color background: ESC[48;5;<n>m
    /// xterm 256 renk paletinden arka plan rengi seçer (0-255 arası)
    pub fn bg_color_256(n: u8) -> String {
        alloc::format!("\x1B[48;5;{}m", n)
    }

    /// True color foreground: ESC[38;2;<r>;<g>;<b>m
    /// 24-bit RGB ön plan rengi (modern terminaller destekler)
    pub fn fg_color_rgb(r: u8, g: u8, b: u8) -> String {
        alloc::format!("\x1B[38;2;{};{};{}m", r, g, b)
    }

    /// True color background: ESC[48;2;<r>;<g>;<b>m
    /// 24-bit RGB arka plan rengi (modern terminaller destekler)
    pub fn bg_color_rgb(r: u8, g: u8, b: u8) -> String {
        alloc::format!("\x1B[48;2;{};{};{}m", r, g, b)
    }

    /// Reset all attributes: ESC[0m
    /// Tüm metin özelliklerini (renk, kalın, italik vb.) varsayılana döndürür
    pub fn reset() -> String {
        "\x1B[0m".to_string()
    }

    /// Bold: ESC[1m
    /// Metni kalın (bold) yapar
    pub fn bold() -> String {
        "\x1B[1m".to_string()
    }

    /// Dim/Faint: ESC[2m
    /// Metni soluk (dim) yapar - bazı terminallerde düşük parlaklık anlamına gelir
    pub fn dim() -> String {
        "\x1B[2m".to_string()
    }

    /// Italic: ESC[3m
    /// Metni italik yapar (tüm terminaller desteklemez)
    pub fn italic() -> String {
        "\x1B[3m".to_string()
    }

    /// Underline: ESC[4m
    /// Metnin altına çizgi çizer
    pub fn underline() -> String {
        "\x1B[4m".to_string()
    }

    /// Blink: ESC[5m
    /// Metni yanıp söner hale getirir (çoğu modern terminalde devre dışı)
    pub fn blink() -> String {
        "\x1B[5m".to_string()
    }

    /// Reverse: ESC[7m
    /// Ön plan ve arka plan renklerini yer değiştirir (vurgulama için kullanılır)
    pub fn reverse() -> String {
        "\x1B[7m".to_string()
    }

    /// Hidden: ESC[8m
    /// Metni gizler (şifre girişleri için kullanılır)
    pub fn hidden() -> String {
        "\x1B[8m".to_string()
    }

    /// Strikethrough: ESC[9m
    /// Metnin üzerini çizer
    pub fn strikethrough() -> String {
        "\x1B[9m".to_string()
    }

    /// Save cursor position: ESC[s
    /// İmleç konumunu kaydeder (yalnızca bir konum saklanabilir, iç içe çalışmaz)
    pub fn save_cursor() -> String {
        "\x1B[s".to_string()
    }

    /// Restore cursor position: ESC[u
    /// Kaydedilen imleç konumunu geri yükler
    pub fn restore_cursor() -> String {
        "\x1B[u".to_string()
    }

    /// Show cursor: ESC[?25h
    /// İmleci görünür hale getirir (private mode 25 set)
    pub fn show_cursor() -> String {
        "\x1B[?25h".to_string()
    }

    /// Hide cursor: ESC[?25l
    /// İmleci gizler (private mode 25 reset) - animasyonlar ve çizim için kullanılır
    pub fn hide_cursor() -> String {
        "\x1B[?25l".to_string()
    }

    /// Set title: ESC]0;<title>BEL
    /// Terminal pencere/sekme başlığını değiştirir (OSC 0 komutu)
    pub fn set_title(title: &str) -> String {
        alloc::format!("\x1B]0;{}\x07", title)
    }

    /// Colored text (helper)
    /// Metni belirlenen ön plan ve arka plan renkleriyle sarar, sonunda sıfırlar.
    pub fn colored(text: &str, fg: Color, bg: Color) -> String {
        alloc::format!(
            "{}{}{}{}",
            Self::fg_color(fg),
            Self::bg_color(bg),
            text,
            Self::reset()
        )
    }

    /// Styled text (helper)
    /// Metni verilen stil dizisiyle sarar ve sonunda ESC[0m ile sıfırlar.
    pub fn styled(text: &str, style: &str) -> String {
        alloc::format!("{}{}\x1B[0m", style, text)
    }
}

/// Terminal Durumu (Cursor position, colors, etc.)
///
/// Terminal emülatörünün anlık durumunu tutar.
/// `AnsiParser`'dan gelen her sequence bu struct'a uygulanarak
/// terminal görüntüsü güncellenir.
///
/// ```
/// Terminal Ekran Koordinat Sistemi:
///
///  (1,1)─────────────────────────(1,80)
///    │  r o w = 1,  c o l = 1..80 │
///    │                             │
///    │  satır (row): 1'den başlar  │
///    │  sütun (col): 1'den başlar  │
///    │                             │
///  (24,1)────────────────────────(24,80)
/// ```
#[derive(Clone, Debug)]
pub struct TerminalState {
    /// Mevcut imleç satırı (1-tabanlı)
    pub cursor_row: u16,
    /// Mevcut imleç sütunu (1-tabanlı)
    pub cursor_col: u16,
    /// ESC[s ile kaydedilen imleç satırı
    pub saved_cursor_row: u16,
    /// ESC[s ile kaydedilen imleç sütunu
    pub saved_cursor_col: u16,
    /// Ön plan rengi (metni bu renkte göster) — 256-renk ve RGB destekli
    pub fg_color: ExtendedColor,
    /// Arka plan rengi (metin arkasını bu renkle doldur) — 256-renk ve RGB destekli
    pub bg_color: ExtendedColor,
    /// Kalın metin aktif mi?
    pub bold: bool,
    /// Soluk metin aktif mi?
    pub dim: bool,
    /// İtalik metin aktif mi?
    pub italic: bool,
    /// Altı çizili aktif mi?
    pub underline: bool,
    /// Yanıp sönme aktif mi?
    pub blink: bool,
    /// Renk tersine çevirme aktif mi?
    pub reverse: bool,
    /// Metin gizleme aktif mi?
    pub hidden: bool,
    /// Üstü çizili aktif mi?
    pub strikethrough: bool,
    /// İmleç görünür mü?
    pub cursor_visible: bool,
    /// Ekran satır sayısı (varsayılan: 24)
    pub screen_rows: u16,
    /// Ekran sütun sayısı (varsayılan: 80)
    pub screen_cols: u16,
    /// Kaydırma bölgesi üst sınırı (scroll region top)
    pub scroll_region_start: u16,
    /// Kaydırma bölgesi alt sınırı (scroll region bottom)
    pub scroll_region_end: u16,
}

impl Default for TerminalState {
    fn default() -> Self {
        Self {
            cursor_row: 1,
            cursor_col: 1,
            saved_cursor_row: 1,
            saved_cursor_col: 1,
            fg_color: ExtendedColor::default_fg(),
            bg_color: ExtendedColor::default_bg(),
            bold: false,
            dim: false,
            italic: false,
            underline: false,
            blink: false,
            reverse: false,
            hidden: false,
            strikethrough: false,
            cursor_visible: true,
            screen_rows: 24,
            screen_cols: 80,
            scroll_region_start: 1,
            scroll_region_end: 24,
        }
    }
}

impl TerminalState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Escape sequence'i terminal durumuna uygular.
    ///
    /// Bu metod, parser'dan gelen her sequence'ı alır ve
    /// terminal durumunu günceller. Ekran çizimi bu struct'ın
    /// değerlerine bakarak yapılır.
    pub fn apply(&mut self, seq: &EscapeSequence) {
        match seq {
            EscapeSequence::CursorPosition { row, col } => {
                // Sınır kontrolü: ekran dışına çıkmasın
                self.cursor_row = (*row).min(self.screen_rows).max(1);
                self.cursor_col = (*col).min(self.screen_cols).max(1);
            }
            EscapeSequence::CursorUp(n) => {
                // saturating_sub: 0'ın altına düşmesini engeller
                self.cursor_row = self.cursor_row.saturating_sub(*n).max(1);
            }
            EscapeSequence::CursorDown(n) => {
                self.cursor_row = (self.cursor_row + *n).min(self.screen_rows);
            }
            EscapeSequence::CursorForward(n) => {
                self.cursor_col = (self.cursor_col + *n).min(self.screen_cols);
            }
            EscapeSequence::CursorBack(n) => {
                self.cursor_col = self.cursor_col.saturating_sub(*n).max(1);
            }
            EscapeSequence::SaveCursorPosition => {
                // Mevcut konumu kaydet (yalnızca bir konum saklanabilir)
                self.saved_cursor_row = self.cursor_row;
                self.saved_cursor_col = self.cursor_col;
            }
            EscapeSequence::RestoreCursorPosition => {
                // Kaydedilen konuma dön
                self.cursor_row = self.saved_cursor_row;
                self.cursor_col = self.saved_cursor_col;
            }
            EscapeSequence::ShowCursor => {
                self.cursor_visible = true;
            }
            EscapeSequence::HideCursor => {
                self.cursor_visible = false;
            }
            EscapeSequence::SelectGraphicRendition(params) => {
                // SGR parametrelerini tek tek uygula
                self.apply_sgr(params);
            }
            _ => {}
        }
    }

    /// SGR parametrelerini uygular.
    ///
    /// SGR kod tablosu:
    /// - 0     : Tümünü sıfırla
    /// - 1-9   : Stil aktifleştir (kalın, soluk, italik, altı çizili, vb.)
    /// - 22-29 : Stil devre dışı bırak
    /// - 30-37 : Standart ön plan rengi
    /// - 38    : Genişletilmiş ön plan (38;5;n veya 38;2;r;g;b)
    /// - 39    : Varsayılan ön plan rengine dön
    /// - 40-47 : Standart arka plan rengi
    /// - 48    : Genişletilmiş arka plan (48;5;n veya 48;2;r;g;b)
    /// - 49    : Varsayılan arka plan rengine dön
    /// - 90-97 : Parlak ön plan renkleri
    /// - 100-107: Parlak arka plan renkleri
    fn apply_sgr(&mut self, params: &[u8]) {
        let mut i = 0;
        while i < params.len() {
            match params[i] {
                0 => {
                    // Reset all - tüm özellikler varsayılana dönüyor
                    self.fg_color = ExtendedColor::default_fg();
                    self.bg_color = ExtendedColor::default_bg();
                    self.bold = false;
                    self.dim = false;
                    self.italic = false;
                    self.underline = false;
                    self.blink = false;
                    self.reverse = false;
                    self.hidden = false;
                    self.strikethrough = false;
                }
                1 => self.bold = true,
                2 => self.dim = true,
                3 => self.italic = true,
                4 => self.underline = true,
                5 | 6 => self.blink = true,
                7 => self.reverse = true,
                8 => self.hidden = true,
                9 => self.strikethrough = true,
                22 => {
                    self.bold = false;
                    self.dim = false;
                }
                23 => self.italic = false,
                24 => self.underline = false,
                25 => self.blink = false,
                27 => self.reverse = false,
                28 => self.hidden = false,
                29 => self.strikethrough = false,
                30..=37 => {
                    self.fg_color = ExtendedColor::from_standard(Color::from_sgr(params[i] - 30))
                }
                38 => {
                    // Extended foreground color - genişletilmiş ön plan rengi
                    if i + 2 < params.len() && params[i + 1] == 5 {
                        // 256-color modu: 38;5;<n>
                        self.fg_color = ExtendedColor::from_256(params[i + 2]);
                        i += 2;
                    } else if i + 4 < params.len() && params[i + 1] == 2 {
                        // RGB modu: 38;2;<r>;<g>;<b>
                        self.fg_color =
                            ExtendedColor::from_rgb(params[i + 2], params[i + 3], params[i + 4]);
                        i += 4;
                    }
                }
                39 => self.fg_color = ExtendedColor::default_fg(),
                40..=47 => {
                    self.bg_color = ExtendedColor::from_standard(Color::from_sgr(params[i] - 40))
                }
                48 => {
                    // Extended background color - genişletilmiş arka plan rengi
                    if i + 2 < params.len() && params[i + 1] == 5 {
                        // 256-color modu: 48;5;<n>
                        self.bg_color = ExtendedColor::from_256(params[i + 2]);
                        i += 2;
                    } else if i + 4 < params.len() && params[i + 1] == 2 {
                        // RGB modu: 48;2;<r>;<g>;<b>
                        self.bg_color =
                            ExtendedColor::from_rgb(params[i + 2], params[i + 3], params[i + 4]);
                        i += 4;
                    }
                }
                49 => self.bg_color = ExtendedColor::default_bg(),
                90..=97 => {
                    self.fg_color =
                        ExtendedColor::from_standard(Color::from_sgr_bright(params[i] - 90))
                }
                100..=107 => {
                    self.bg_color =
                        ExtendedColor::from_standard(Color::from_sgr_bright(params[i] - 100))
                }
                _ => {}
            }
            i += 1;
        }
    }
}

impl Color {
    /// SGR kodundan renk oluşturur (30-37, 40-47).
    ///
    /// ESC[3Xm ve ESC[4Xm formatlarındaki standart 8 rengi dönüştürür.
    /// X değeri (0-7) Color enum değerine karşılık gelir.
    pub fn from_sgr(code: u8) -> Self {
        match code {
            0 => Color::Black,
            1 => Color::Red,
            2 => Color::Green,
            3 => Color::Yellow,
            4 => Color::Blue,
            5 => Color::Magenta,
            6 => Color::Cyan,
            7 => Color::White,
            _ => Color::Default,
        }
    }

    /// SGR bright kodundan renk oluşturur (90-97, 100-107).
    ///
    /// ESC[9Xm ve ESC[10Xm formatlarındaki parlak/yüksek yoğunluklu
    /// 8 rengi dönüştürür. Standart renklerden daha parlak görünür.
    pub fn from_sgr_bright(code: u8) -> Self {
        match code {
            0 => Color::BrightBlack,
            1 => Color::BrightRed,
            2 => Color::BrightGreen,
            3 => Color::BrightYellow,
            4 => Color::BrightBlue,
            5 => Color::BrightMagenta,
            6 => Color::BrightCyan,
            7 => Color::BrightWhite,
            _ => Color::Default,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cursor_position() {
        let seq = AnsiBuilder::cursor_position(10, 20);
        assert_eq!(seq, "\x1B[10;20H");
    }

    #[test]
    fn test_colors() {
        let seq = AnsiBuilder::fg_color(Color::Red);
        assert_eq!(seq, "\x1B[31m");

        let seq = AnsiBuilder::bg_color(Color::Blue);
        assert_eq!(seq, "\x1B[44m");
    }

    #[test]
    fn test_clear_screen() {
        let seq = AnsiBuilder::clear_screen();
        assert_eq!(seq, "\x1B[2J\x1B[H");
    }

    #[test]
    fn test_parser() {
        let mut parser = AnsiParser::new();

        // Test cursor position
        for &b in b"\x1B[10;20H" {
            parser.feed(b);
        }
        // Should produce CursorPosition { row: 10, col: 20 }
    }
}
