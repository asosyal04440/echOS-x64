//! ANSI Escape Sequence Handler
//!
//! VT100/ANSI terminal escape sequence desteği.
//! Renkler, imleç kontrolü, ekran temizleme vb.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;
use core::fmt::Write;

/// ANSI Renk kodları (3/4-bit)
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
    Default = 255, // Changed from 9 to avoid conflict
}

/// ANSI Escape Sequence tipleri
#[derive(Clone, Debug, PartialEq)]
pub enum EscapeSequence {
    /// Cursor Position: ESC[<row>;<col>H
    CursorPosition { row: u16, col: u16 },
    /// Cursor Up: ESC[<n>A
    CursorUp(u16),
    /// Cursor Down: ESC[<n>B
    CursorDown(u16),
    /// Cursor Forward: ESC[<n>C
    CursorForward(u16),
    /// Cursor Back: ESC[<n>D
    CursorBack(u16),
    /// Cursor Next Line: ESC[<n>E
    CursorNextLine(u16),
    /// Cursor Previous Line: ESC[<n>F
    CursorPreviousLine(u16),
    /// Cursor Horizontal Absolute: ESC[<n>G
    CursorHorizontalAbsolute(u16),
    /// Erase in Display: ESC[<n>J
    EraseInDisplay(u8),
    /// Erase in Line: ESC[<n>K
    EraseInLine(u8),
    /// Scroll Up: ESC[<n>S
    ScrollUp(u16),
    /// Scroll Down: ESC[<n>T
    ScrollDown(u16),
    /// Select Graphic Rendition (renkler ve stiller)
    SelectGraphicRendition(Vec<u8>),
    /// Set Title: ESC]0;<title>BEL
    SetTitle(String),
    /// Save Cursor Position: ESC[s
    SaveCursorPosition,
    /// Restore Cursor Position: ESC[u
    RestoreCursorPosition,
    /// Show Cursor: ESC[?25h
    ShowCursor,
    /// Hide Cursor: ESC[?25l
    HideCursor,
    /// Enable Alternative Screen Buffer: ESC[?1049h
    EnableAltScreen,
    /// Disable Alternative Screen Buffer: ESC[?1049l
    DisableAltScreen,
    /// Bell: BEL (0x07)
    Bell,
    /// Backspace: BS (0x08)
    Backspace,
    /// Tab: HT (0x09)
    Tab,
    /// Line Feed: LF (0x0A)
    LineFeed,
    /// Carriage Return: CR (0x0D)
    CarriageReturn,
    /// Unknown/Unsupported
    Unknown(Vec<u8>),
}

/// ANSI Parser State
#[derive(Clone, Copy, Debug, PartialEq)]
enum ParserState {
    Normal,
    Escape,      // ESC karakteri alındı
    Csi,         // ESC[ alındı
    CsiParams,   // ESC[<params> alındı
    Osc,         // ESC] alındı (Operating System Command)
    OscParam,    // ESC]<param> alındı
}

/// ANSI Escape Sequence Parser
pub struct AnsiParser {
    state: ParserState,
    buffer: Vec<u8>,
    params: Vec<u8>,
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
    
    /// Byte'ı parse eder ve tamamlanan sequence'ları döndürür
    pub fn feed(&mut self, byte: u8) -> Option<EscapeSequence> {
        match self.state {
            ParserState::Normal => {
                match byte {
                    0x1B => { // ESC
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
                        self.state = ParserState::Csi;
                        None
                    }
                    b']' => {
                        self.state = ParserState::Osc;
                        self.osc_buffer.clear();
                        None
                    }
                    b'(' | b')' | b'*' | b'+' => {
                        // Character set selection - ignore next char
                        self.buffer.push(byte);
                        None
                    }
                    _ => {
                        self.state = ParserState::Normal;
                        Some(EscapeSequence::Unknown(vec![0x1B, byte]))
                    }
                }
            }
            ParserState::Csi => {
                match byte {
                    b'0'..=b'9' | b';' | b'?' => {
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
                        Some(EscapeSequence::CursorPreviousLine(self.parse_single_param(1)))
                    }
                    b'G' => {
                        self.state = ParserState::Normal;
                        Some(EscapeSequence::CursorHorizontalAbsolute(self.parse_single_param(1)))
                    }
                    b'H' | b'f' => {
                        self.state = ParserState::Normal;
                        let (row, col) = self.parse_cursor_position();
                        Some(EscapeSequence::CursorPosition { row, col })
                    }
                    b'J' => {
                        self.state = ParserState::Normal;
                        Some(EscapeSequence::EraseInDisplay(self.parse_single_param(0) as u8))
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
                        self.state = ParserState::Normal;
                        Some(EscapeSequence::SelectGraphicRendition(self.parse_sgr_params()))
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
                        self.params.push(byte);
                        None
                    }
                    _ => {
                        // Final byte - process the sequence
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
                        self.state = ParserState::Normal;
                        let title = self.parse_osc_title();
                        Some(EscapeSequence::SetTitle(title))
                    }
                    _ => {
                        self.osc_buffer.push(byte);
                        None
                    }
                }
            }
            ParserState::OscParam => {
                self.state = ParserState::Normal;
                None
            }
        }
    }
    
    /// Tek parametre parse eder
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
        if found { num } else { default }
    }
    
    /// Cursor position parse eder
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
    
    /// SGR (Select Graphic Rendition) parametrelerini parse eder
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
            result.push(0); // Reset
        }
        result
    }
    
    /// Mode parse eder
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
    
    /// OSC title parse eder
    fn parse_osc_title(&self) -> String {
        // OSC format: ]0;title<BEL>
        let s = String::from_utf8_lossy(&self.osc_buffer);
        if let Some(pos) = s.find(';') {
            s[pos + 1..].to_string()
        } else {
            s.to_string()
        }
    }
    
    /// CSI final byte işleme
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
            b'J' => Some(EscapeSequence::EraseInDisplay(self.parse_single_param(0) as u8)),
            b'K' => Some(EscapeSequence::EraseInLine(self.parse_single_param(0) as u8)),
            b'm' => Some(EscapeSequence::SelectGraphicRendition(self.parse_sgr_params())),
            _ => Some(EscapeSequence::Unknown(params)),
        }
    }
}

impl Default for AnsiParser {
    fn default() -> Self {
        Self::new()
    }
}

/// ANSI escape sequence oluşturucu
pub struct AnsiBuilder;

impl AnsiBuilder {
    /// ESC karakteri
    pub const ESC: u8 = 0x1B;
    
    /// Cursor position: ESC[row;colH
    pub fn cursor_position(row: u16, col: u16) -> String {
        alloc::format!("\x1B[{};{}H", row, col)
    }
    
    /// Cursor up: ESC[nA
    pub fn cursor_up(n: u16) -> String {
        alloc::format!("\x1B[{}A", n)
    }
    
    /// Cursor down: ESC[nB
    pub fn cursor_down(n: u16) -> String {
        alloc::format!("\x1B[{}B", n)
    }
    
    /// Cursor forward: ESC[nC
    pub fn cursor_forward(n: u16) -> String {
        alloc::format!("\x1B[{}C", n)
    }
    
    /// Cursor back: ESC[nD
    pub fn cursor_back(n: u16) -> String {
        alloc::format!("\x1B[{}D", n)
    }
    
    /// Erase display: ESC[nJ (0=cursor to end, 1=start to cursor, 2=entire screen)
    pub fn erase_display(mode: u8) -> String {
        alloc::format!("\x1B[{}J", mode)
    }
    
    /// Clear screen: ESC[2J + ESC[H
    pub fn clear_screen() -> String {
        "\x1B[2J\x1B[H".to_string()
    }
    
    /// Erase line: ESC[nK (0=cursor to end, 1=start to cursor, 2=entire line)
    pub fn erase_line(mode: u8) -> String {
        alloc::format!("\x1B[{}K", mode)
    }
    
    /// Foreground color (standard): ESC[30-37m
    pub fn fg_color(color: Color) -> String {
        let code = match color {
            Color::Default => 39,
            c => 30 + c as u8,
        };
        alloc::format!("\x1B[{}m", code)
    }
    
    /// Background color (standard): ESC[40-47m
    pub fn bg_color(color: Color) -> String {
        let code = match color {
            Color::Default => 49,
            c => 40 + c as u8,
        };
        alloc::format!("\x1B[{}m", code)
    }
    
    /// Foreground color (bright): ESC[90-97m
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
    pub fn fg_color_256(n: u8) -> String {
        alloc::format!("\x1B[38;5;{}m", n)
    }
    
    /// 256-color background: ESC[48;5;<n>m
    pub fn bg_color_256(n: u8) -> String {
        alloc::format!("\x1B[48;5;{}m", n)
    }
    
    /// True color foreground: ESC[38;2;<r>;<g>;<b>m
    pub fn fg_color_rgb(r: u8, g: u8, b: u8) -> String {
        alloc::format!("\x1B[38;2;{};{};{}m", r, g, b)
    }
    
    /// True color background: ESC[48;2;<r>;<g>;<b>m
    pub fn bg_color_rgb(r: u8, g: u8, b: u8) -> String {
        alloc::format!("\x1B[48;2;{};{};{}m", r, g, b)
    }
    
    /// Reset all attributes: ESC[0m
    pub fn reset() -> String {
        "\x1B[0m".to_string()
    }
    
    /// Bold: ESC[1m
    pub fn bold() -> String {
        "\x1B[1m".to_string()
    }
    
    /// Dim/Faint: ESC[2m
    pub fn dim() -> String {
        "\x1B[2m".to_string()
    }
    
    /// Italic: ESC[3m
    pub fn italic() -> String {
        "\x1B[3m".to_string()
    }
    
    /// Underline: ESC[4m
    pub fn underline() -> String {
        "\x1B[4m".to_string()
    }
    
    /// Blink: ESC[5m
    pub fn blink() -> String {
        "\x1B[5m".to_string()
    }
    
    /// Reverse: ESC[7m
    pub fn reverse() -> String {
        "\x1B[7m".to_string()
    }
    
    /// Hidden: ESC[8m
    pub fn hidden() -> String {
        "\x1B[8m".to_string()
    }
    
    /// Strikethrough: ESC[9m
    pub fn strikethrough() -> String {
        "\x1B[9m".to_string()
    }
    
    /// Save cursor position: ESC[s
    pub fn save_cursor() -> String {
        "\x1B[s".to_string()
    }
    
    /// Restore cursor position: ESC[u
    pub fn restore_cursor() -> String {
        "\x1B[u".to_string()
    }
    
    /// Show cursor: ESC[?25h
    pub fn show_cursor() -> String {
        "\x1B[?25h".to_string()
    }
    
    /// Hide cursor: ESC[?25l
    pub fn hide_cursor() -> String {
        "\x1B[?25l".to_string()
    }
    
    /// Set title: ESC]0;<title>BEL
    pub fn set_title(title: &str) -> String {
        alloc::format!("\x1B]0;{}\x07", title)
    }
    
    /// Colored text (helper)
    pub fn colored(text: &str, fg: Color, bg: Color) -> String {
        alloc::format!("{}{}{}{}", 
            Self::fg_color(fg),
            Self::bg_color(bg),
            text,
            Self::reset()
        )
    }
    
    /// Styled text (helper)
    pub fn styled(text: &str, style: &str) -> String {
        alloc::format!("{}{}\x1B[0m", style, text)
    }
}

/// Terminal state (cursor position, colors, etc.)
#[derive(Clone, Debug)]
pub struct TerminalState {
    pub cursor_row: u16,
    pub cursor_col: u16,
    pub saved_cursor_row: u16,
    pub saved_cursor_col: u16,
    pub fg_color: Color,
    pub bg_color: Color,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub blink: bool,
    pub reverse: bool,
    pub hidden: bool,
    pub strikethrough: bool,
    pub cursor_visible: bool,
    pub screen_rows: u16,
    pub screen_cols: u16,
    pub scroll_region_start: u16,
    pub scroll_region_end: u16,
}

impl Default for TerminalState {
    fn default() -> Self {
        Self {
            cursor_row: 1,
            cursor_col: 1,
            saved_cursor_row: 1,
            saved_cursor_col: 1,
            fg_color: Color::Default,
            bg_color: Color::Default,
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
    
    /// Escape sequence'i uygular
    pub fn apply(&mut self, seq: &EscapeSequence) {
        match seq {
            EscapeSequence::CursorPosition { row, col } => {
                self.cursor_row = (*row).min(self.screen_rows).max(1);
                self.cursor_col = (*col).min(self.screen_cols).max(1);
            }
            EscapeSequence::CursorUp(n) => {
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
                self.saved_cursor_row = self.cursor_row;
                self.saved_cursor_col = self.cursor_col;
            }
            EscapeSequence::RestoreCursorPosition => {
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
                self.apply_sgr(params);
            }
            _ => {}
        }
    }
    
    /// SGR parametrelerini uygular
    fn apply_sgr(&mut self, params: &[u8]) {
        let mut i = 0;
        while i < params.len() {
            match params[i] {
                0 => {
                    // Reset all
                    self.fg_color = Color::Default;
                    self.bg_color = Color::Default;
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
                22 => { self.bold = false; self.dim = false; }
                23 => self.italic = false,
                24 => self.underline = false,
                25 => self.blink = false,
                27 => self.reverse = false,
                28 => self.hidden = false,
                29 => self.strikethrough = false,
                30..=37 => self.fg_color = Color::from_sgr(params[i] - 30),
                38 => {
                    // Extended foreground color
                    if i + 2 < params.len() && params[i + 1] == 5 {
                        // 256-color
                        let _color_256 = params[i + 2];
                        i += 2;
                    } else if i + 4 < params.len() && params[i + 1] == 2 {
                        // RGB
                        let _r = params[i + 2];
                        let _g = params[i + 3];
                        let _b = params[i + 4];
                        i += 4;
                    }
                }
                39 => self.fg_color = Color::Default,
                40..=47 => self.bg_color = Color::from_sgr(params[i] - 40),
                48 => {
                    // Extended background color
                    if i + 2 < params.len() && params[i + 1] == 5 {
                        // 256-color
                        let _color_256 = params[i + 2];
                        i += 2;
                    } else if i + 4 < params.len() && params[i + 1] == 2 {
                        // RGB
                        let _r = params[i + 2];
                        let _g = params[i + 3];
                        let _b = params[i + 4];
                        i += 4;
                    }
                }
                49 => self.bg_color = Color::Default,
                90..=97 => self.fg_color = Color::from_sgr_bright(params[i] - 90),
                100..=107 => self.bg_color = Color::from_sgr_bright(params[i] - 100),
                _ => {}
            }
            i += 1;
        }
    }
}

impl Color {
    /// SGR kodundan renk oluşturur (30-37, 40-47)
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
    
    /// SGR bright kodundan renk oluşturur (90-97, 100-107)
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