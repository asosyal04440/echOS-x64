//! # echOS USB HID Sürücüsü (Human Interface Device)
//!
//! Klavye, fare ve oyun kolu gibi kullanıcı giriş cihazları için USB HID sürücüsü.
//! USB HID spesifikasyonu 1.11 ve önyükleme (boot) protokolü desteği içerir.
//!
//! ## HID Protokol Katmanları
//!
//! ```
//!  ┌─────────────────────────────────────────────────────────┐
//!  │  Uygulama: read_key() / has_key() / KEYBOARD_QUEUE     │
//!  ├─────────────────────────────────────────────────────────┤
//!  │  HidDriver: process_report() → HidEvent                │
//!  ├─────────────────────────────────────────────────────────┤
//!  │  HidDeviceState: klavye / fare rapor durumu             │
//!  ├─────────────────────────────────────────────────────────┤
//!  │  USB Interrupt IN Endpoint: periyodik rapor alımı       │
//!  │  (poll_interval_ms: 10ms varsayılan)                    │
//!  └─────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Boot Protokolü vs Rapor Protokolü
//!
//! **Boot Protokolü** (HID_PROTOCOL_BOOT = 0x00):
//! - BIOS/UEFI başlatma öncesi basit ve sabit format kullanır
//! - Klavye: 8 byte sabit format (modifier + reserved + 6 key)
//! - Fare: 3-4 byte sabit format (buttons + x + y)
//! - Sürücü gerektirmez; donanım doğrudan okuyabilir
//!
//! **Rapor Protokolü** (HID_PROTOCOL_REPORT = 0x01):
//! - Esnek HID rapor tanımlayıcı ile tanımlanan format
//! - Gamepad, dokunmatik yüzey, özel düğmeler için gerekli
//!
//! ## Klavye Boot Raporu Formatı
//!
//! ```
//! Byte 0: Modifier bitler (bit0=LCtrl, bit1=LShift, bit2=LAlt,
//!                           bit3=LGUI, bit4=RCtrl, bit5=RShift,
//!                           bit6=RAlt, bit7=RGUI)
//! Byte 1: Rezerve (0x00)
//! Byte 2..7: Aynı anda basılan tuşlar (HID kullanım kodu, 0=boş)
//! Toplam: 8 byte
//! ```
//!
//! ## LED Kontrolü (Klavye)
//!
//! Klavye LED'leri (NumLock, CapsLock, ScrollLock) SET_REPORT isteğiyle kontrol edilir:
//! ```
//! Byte 0: LED bits (bit0=NumLock, bit1=CapsLock, bit2=ScrollLock)
//! ```
//!
//! ## HID Kullanım Tabloları
//!
//! HID cihazlar "Usage Page" + "Usage" çiftleriyle tanımlanır:
//! - `UsagePage::GenericDesktop (0x01)`: fare, klavye, joystick
//! - `UsagePage::Keyboard (0x07)`: klavye tuşları
//! - `UsagePage::LED (0x08)`: klavye LED'leri
//! - `UsagePage::Button (0x09)`: fare/joystick düğmeleri

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

use super::{UsbDevice, UsbDirection, UsbEndpoint, UsbError, UsbSetupPacket, UsbTransferType};

// ============================================================================
// HID SINIF İSTEKLERİ
// USB kontrol aktarımıyla gönderilen HID sınıfına özgü komutlar
// ============================================================================

/// Giriş/Çıkış/Özellik raporu al (cihazdan okuma)
const HID_GET_REPORT: u8 = 0x01;
/// Boşta kalma süresini al (cihaz ne kadar sürede bir rapor gönderir)
const HID_GET_IDLE: u8 = 0x02;
/// Mevcut protokolü al (boot=0 veya report=1)
const HID_GET_PROTOCOL: u8 = 0x03;
/// Rapor gönder (host → cihaz, örn. LED kontrolü)
const HID_SET_REPORT: u8 = 0x09;
/// Boşta kalma süresini ayarla (periyodik rapor aralığı)
const HID_SET_IDLE: u8 = 0x0A;
/// Protokolü değiştir (boot ↔ report, yalnızca klavye/fare)
const HID_SET_PROTOCOL: u8 = 0x0B;

/// Boot protokolü: BIOS ile uyumlu basit rapor formatı
const HID_PROTOCOL_BOOT: u8 = 0x00;
/// Rapor protokolü: HID tanımlayıcısıyla belirlenen esnek format
const HID_PROTOCOL_REPORT: u8 = 0x01;

/// Giriş raporu tipi (cihazdan host'a veri: basılan tuşlar, fare hareketi)
const HID_REPORT_INPUT: u8 = 0x01;
/// Çıkış raporu tipi (host'tan cihaza: klavye LED'leri)
const HID_REPORT_OUTPUT: u8 = 0x02;
/// Özellik raporu tipi (iki yönlü: cihaz yapılandırması)
const HID_REPORT_FEATURE: u8 = 0x03;

// ============================================================================
// HID BOOT PROTOKOLÜ RAPORLARI
// BIOS ile uyumlu sabit formatlı klavye ve fare raporları
// ============================================================================

/// Standart klavye boot protokolü raporu (8 byte).
///
/// ## Rapor Yorumu
///
/// ```
/// Byte 0: Modifier bitleri
///   bit0: Sol Ctrl
///   bit1: Sol Shift
///   bit2: Sol Alt
///   bit3: Sol GUI (Windows/Meta tuşu)
///   bit4: Sağ Ctrl
///   bit5: Sağ Shift
///   bit6: Sağ Alt (AltGr)
///   bit7: Sağ GUI
///
/// Byte 1: Rezerve (0x00)
///
/// Byte 2-7: Aynı anda basılan en fazla 6 tuşun HID kullanım kodu
///   Örnek: 0x04=A, 0x28=Enter, 0x2C=Space
///   0x00 = boş slot (5 tuş basılıysa son slot(lar) 0x00)
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct KeyboardBootReport {
    /// Modifier tuşlar (Ctrl, Shift, Alt, GUI) bit maskeleri
    pub modifiers: u8,
    /// Rezerve (her zaman 0x00)
    pub reserved: u8,
    /// Aynı anda basılan tuşların HID kullanım kodları (en fazla 6)
    pub keys: [u8; 6],
}

impl KeyboardBootReport {
    pub fn new() -> Self {
        Self::default()
    }

    /// Belirtilen modifier tuşun basılı olup olmadığını kontrol eder.
    ///
    /// `KeyboardModifier` değeri AND maskesi olarak kullanılır.
    /// Örnek: `modifier_pressed(KeyboardModifier::LeftShift)` → bit1 set mi?
    pub fn modifier_pressed(&self, modifier: KeyboardModifier) -> bool {
        (self.modifiers & modifier as u8) != 0
    }

    /// Basılı olan tüm tuşların HID kullanım kodlarını döndürür.
    ///
    /// 0 değerindeki slotlar boş anlamında; bunlar filtrelenir.
    pub fn pressed_keys(&self) -> Vec<u8> {
        self.keys.iter().filter(|&&k| k != 0).copied().collect()
    }

    /// Belirtilen tuş kodunun basılı olup olmadığını kontrol eder.
    pub fn key_pressed(&self, key_code: u8) -> bool {
        self.keys.contains(&key_code)
    }
}

/// Klavye modifier (değiştirici) tuş bit maskeleri.
///
/// USB HID klavye raporu byte 0'ında her tuş bir bit olarak temsil edilir.
/// Birden fazla modifier aynı anda basılabilir (OR kombinasyonu).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum KeyboardModifier {
    LeftCtrl = 0x01,   // bit0: Sol Ctrl
    LeftShift = 0x02,  // bit1: Sol Shift
    LeftAlt = 0x04,    // bit2: Sol Alt
    LeftGUI = 0x08,    // bit3: Sol GUI (Windows/Meta)
    RightCtrl = 0x10,  // bit4: Sağ Ctrl
    RightShift = 0x20, // bit5: Sağ Shift
    RightAlt = 0x40,   // bit6: Sağ Alt (AltGr)
    RightGUI = 0x80,   // bit7: Sağ GUI
}

/// Standart fare boot protokolü raporu (3-4 byte).
///
/// ```
/// Byte 0: Düğme bitleri
///   bit0: Sol düğme
///   bit1: Sağ düğme
///   bit2: Orta düğme (tekerlek tuşu)
///
/// Byte 1: X ekseni (işaretli 8-bit, negatif=sol, pozitif=sağ)
/// Byte 2: Y ekseni (işaretli 8-bit, negatif=yukarı, pozitif=aşağı)
/// Byte 3: (opsiyonel) Tekerlek hareketi (i8)
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MouseBootReport {
    /// Düğme durumları (bit maskeleri: bit0=sol, bit1=sağ, bit2=orta)
    pub buttons: u8,
    /// X hareketi (göreli, -127..+127)
    pub x: i8,
    /// Y hareketi (göreli, -127..+127)
    pub y: i8,
}

impl MouseBootReport {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sol fare düğmesi basılı mı?
    pub fn left_button(&self) -> bool {
        (self.buttons & 0x01) != 0
    }

    /// Sağ fare düğmesi basılı mı?
    pub fn right_button(&self) -> bool {
        (self.buttons & 0x02) != 0
    }

    /// Orta fare düğmesi (tekerlek tuşu) basılı mı?
    pub fn middle_button(&self) -> bool {
        (self.buttons & 0x04) != 0
    }
}

// ============================================================================
// HID KULLANIM TABLOLARI
// HID Usage Page ve Usage değerleri (USB HID spesifikasyonu Bölüm 3.4)
// ============================================================================

/// HID Kullanım Sayfaları (Usage Pages).
///
/// Her giriş öğesi bir (Usage Page, Usage) çiftiyle tanımlanır.
/// Örnek: `(GenericDesktop, Mouse)` → fare; `(Keyboard, 0x04)` → 'A' tuşu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum UsagePage {
    Undefined = 0x00,
    GenericDesktop = 0x01, // Fare, klavye, joystick, gamepad
    Simulation = 0x02,     // Uçuş simülasyonu
    VR = 0x03,             // Sanal gerçeklik
    Sport = 0x04,          // Spor cihazları
    Game = 0x05,           // Oyun cihazları
    Keyboard = 0x07,       // Klavye/keypad
    LED = 0x08,            // LED göstergeleri (NumLock, CapsLock)
    Button = 0x09,         // Fiziksel düğmeler
    Ordinal = 0x0A,
    Telephony = 0x0B, // Telefon tuşları
    Consumer = 0x0C,  // Medya kontrol düğmeleri (ses kısmalt/artır)
    Digitizer = 0x0D, // Dijital kalem/dokunmatik ekran
    PID = 0x0F,       // Kuvvet geri bildirimi
    Unicode = 0x10,
    AlphaNumeric = 0x14,
    Medical = 0x40,
    Monitor = 0x80,
    Power = 0x84,
    VendorDefined = 0xFF00,
}

/// HID Genel Masaüstü Kullanımları (Generic Desktop Usages).
///
/// `GenericDesktop` kullanım sayfası altındaki cihaz ve eksen tanımları.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum GenericDesktopUsage {
    Undefined = 0x00,
    Pointer = 0x01, // İşaretçi (fare)
    Mouse = 0x02,
    Joystick = 0x04,
    Gamepad = 0x05,
    Keyboard = 0x06,
    Keypad = 0x07,
    X = 0x30,            // X ekseni (fare, joystick)
    Y = 0x31,            // Y ekseni
    Z = 0x32,            // Z ekseni (3D)
    Rx = 0x33,           // X etrafında dönüş
    Ry = 0x34,           // Y etrafında dönüş
    Rz = 0x35,           // Z etrafında dönüş
    Slider = 0x36,       // Sürgü
    Dial = 0x37,         // Döner kontrol
    Wheel = 0x38,        // Tekerlek (fare scroll)
    HatSwitch = 0x39,    // D-pad yön düğmesi
    MotionWakeup = 0x46, // Hareket uyandırma
    Start = 0x47,        // Başlat düğmesi
    Select = 0x48,       // Seç düğmesi
}

/// HID Klavye Kullanım Kodları (kısmi liste).
///
/// USB HID Klavye/Keypad kullanım sayfasından (0x07) tuş kodları.
/// Bu kodlar fiziksel tuş konumunu temsil eder, karakter değil!
/// `hid_to_ascii()` ile ASCII'ye çevrilir.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum KeyboardUsage {
    NoEvent = 0x00,       // Tuş yok (boş slot)
    ErrorRollOver = 0x01, // 6'dan fazla tuş aynı anda basıldı
    A = 0x04,
    B = 0x05,
    C = 0x06,
    D = 0x07,
    E = 0x08,
    F = 0x09,
    G = 0x0A,
    H = 0x0B,
    I = 0x0C,
    J = 0x0D,
    K = 0x0E,
    L = 0x0F,
    M = 0x10,
    N = 0x11,
    O = 0x12,
    P = 0x13,
    Q = 0x14,
    R = 0x15,
    S = 0x16,
    T = 0x17,
    U = 0x18,
    V = 0x19,
    W = 0x1A,
    X = 0x1B,
    Y = 0x1C,
    Z = 0x1D,
    Digit1 = 0x1E,
    Digit2 = 0x1F,
    Digit3 = 0x20,
    Digit4 = 0x21,
    Digit5 = 0x22,
    Digit6 = 0x23,
    Digit7 = 0x24,
    Digit8 = 0x25,
    Digit9 = 0x26,
    Digit0 = 0x27,
    Enter = 0x28,
    Escape = 0x29,
    Backspace = 0x2A,
    Tab = 0x2B,
    Space = 0x2C,
    Minus = 0x2D,      // '-' veya '_'
    Equal = 0x2E,      // '=' veya '+'
    LeftBrace = 0x2F,  // '[' veya '{'
    RightBrace = 0x30, // ']' veya '}'
    Backslash = 0x31,  // '\' veya '|'
    Semicolon = 0x33,  // ';' veya ':'
    Quote = 0x34,      // '\'' veya '"'
    Grave = 0x35,      // '`' veya '~'
    Comma = 0x36,      // ',' veya '<'
    Period = 0x37,     // '.' veya '>'
    Slash = 0x38,      // '/' veya '?'
    CapsLock = 0x39,
    F1 = 0x3A,
    F2 = 0x3B,
    F3 = 0x3C,
    F4 = 0x3D,
    F5 = 0x3E,
    F6 = 0x3F,
    F7 = 0x40,
    F8 = 0x41,
    F9 = 0x42,
    F10 = 0x43,
    F11 = 0x44,
    F12 = 0x45,
    PrintScreen = 0x46,
    ScrollLock = 0x47,
    Pause = 0x48,
    Insert = 0x49,
    Home = 0x4A,
    PageUp = 0x4B,
    Delete = 0x4C,
    End = 0x4D,
    PageDown = 0x4E,
    RightArrow = 0x4F,
    LeftArrow = 0x50,
    DownArrow = 0x51,
    UpArrow = 0x52,
    NumLock = 0x53,
    KeypadSlash = 0x54,
    KeypadAsterisk = 0x55,
    KeypadMinus = 0x56,
    KeypadPlus = 0x57,
    KeypadEnter = 0x58,
    Keypad1 = 0x59,
    Keypad2 = 0x5A,
    Keypad3 = 0x5B,
    Keypad4 = 0x5C,
    Keypad5 = 0x5D,
    Keypad6 = 0x5E,
    Keypad7 = 0x5F,
    Keypad8 = 0x60,
    Keypad9 = 0x61,
    Keypad0 = 0x62,
    KeypadPeriod = 0x63,
    // Modifier tuşlar boot protokolünde ayrı olarak rapor edilir
}

/// HID kullanım kodunu ASCII karaktere çevirir (ABD klavye düzeni).
///
/// `shift=true` ise üst karakter dönüştürülür (örn. '1' → '!', 'a' → 'A').
///
/// ## Dönüşüm Mantığı
///
/// HID kullanım kodları karakter değil fiziksel tuş konumudur.
/// Bu eşleme ABD (QWERTY) klavye düzeni için geçerlidir.
/// Türkçe/Azerbaycanca Q veya F klavye için farklı tablo gerekir.
///
/// `None` döndürür: tuşun ASCII karşılığı yoksa (fonksiyon tuşları, oklar vb.)
pub fn hid_to_ascii(usage: u8, shift: bool) -> Option<char> {
    match usage {
        0x04 => Some(if shift { 'A' } else { 'a' }),
        0x05 => Some(if shift { 'B' } else { 'b' }),
        0x06 => Some(if shift { 'C' } else { 'c' }),
        0x07 => Some(if shift { 'D' } else { 'd' }),
        0x08 => Some(if shift { 'E' } else { 'e' }),
        0x09 => Some(if shift { 'F' } else { 'f' }),
        0x0A => Some(if shift { 'G' } else { 'g' }),
        0x0B => Some(if shift { 'H' } else { 'h' }),
        0x0C => Some(if shift { 'I' } else { 'i' }),
        0x0D => Some(if shift { 'J' } else { 'j' }),
        0x0E => Some(if shift { 'K' } else { 'k' }),
        0x0F => Some(if shift { 'L' } else { 'l' }),
        0x10 => Some(if shift { 'M' } else { 'm' }),
        0x11 => Some(if shift { 'N' } else { 'n' }),
        0x12 => Some(if shift { 'O' } else { 'o' }),
        0x13 => Some(if shift { 'P' } else { 'p' }),
        0x14 => Some(if shift { 'Q' } else { 'q' }),
        0x15 => Some(if shift { 'R' } else { 'r' }),
        0x16 => Some(if shift { 'S' } else { 's' }),
        0x17 => Some(if shift { 'T' } else { 't' }),
        0x18 => Some(if shift { 'U' } else { 'u' }),
        0x19 => Some(if shift { 'V' } else { 'v' }),
        0x1A => Some(if shift { 'W' } else { 'w' }),
        0x1B => Some(if shift { 'X' } else { 'x' }),
        0x1C => Some(if shift { 'Y' } else { 'y' }),
        0x1D => Some(if shift { 'Z' } else { 'z' }),
        0x1E => Some(if shift { '!' } else { '1' }),
        0x1F => Some(if shift { '@' } else { '2' }),
        0x20 => Some(if shift { '#' } else { '3' }),
        0x21 => Some(if shift { '$' } else { '4' }),
        0x22 => Some(if shift { '%' } else { '5' }),
        0x23 => Some(if shift { '^' } else { '6' }),
        0x24 => Some(if shift { '&' } else { '7' }),
        0x25 => Some(if shift { '*' } else { '8' }),
        0x26 => Some(if shift { '(' } else { '9' }),
        0x27 => Some(if shift { ')' } else { '0' }),
        0x28 => Some('\n'),   // Enter → yeni satır
        0x29 => Some('\x1B'), // Escape → ESC karakteri
        0x2A => Some('\x08'), // Backspace → geri al
        0x2B => Some('\t'),   // Tab → sekme
        0x2C => Some(' '),    // Space → boşluk
        0x2D => Some(if shift { '_' } else { '-' }),
        0x2E => Some(if shift { '+' } else { '=' }),
        0x2F => Some(if shift { '{' } else { '[' }),
        0x30 => Some(if shift { '}' } else { ']' }),
        0x31 => Some(if shift { '|' } else { '\\' }),
        0x33 => Some(if shift { ':' } else { ';' }),
        0x34 => Some(if shift { '"' } else { '\'' }),
        0x35 => Some(if shift { '~' } else { '`' }),
        0x36 => Some(if shift { '<' } else { ',' }),
        0x37 => Some(if shift { '>' } else { '.' }),
        0x38 => Some(if shift { '?' } else { '/' }),
        _ => None, // Fonksiyon tuşları, navigation, vb. → ASCII yok
    }
}

// ============================================================================
// HID CİHAZ DURUMU
// Klavye ve fare mevcut rapor durumu ve değişim tespiti
// ============================================================================

/// HID cihaz tipi.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HidDeviceType {
    Keyboard, // Klavye (boot raporu: 8 byte)
    Mouse,    // Fare (boot raporu: 3+ byte)
    Gamepad,  // Oyun kolu (rapor protokolü)
    Generic,  // Diğer HID cihazlar
    Unknown,  // Henüz belirlenmemiş
}

/// HID cihaz durum kaydı.
///
/// ## Değişim Tespiti
///
/// `keyboard` ve `prev_keyboard` karşılaştırılarak yeni basılan/bırakılan tuşlar tespit edilir:
/// - `new_keys()`: `keyboard.keys` içinde olup `prev_keyboard.keys` içinde olmayan tuşlar
/// - `released_keys()`: `prev_keyboard.keys` içinde olup `keyboard.keys` içinde olmayan tuşlar
///
/// ## Fare Birikimi
///
/// `mouse_x` / `mouse_y` alanları mutlak pozisyon biriktirici olarak çalışır.
/// Her raporda göreli hareket (dx, dy) bu değerlere eklenir.
#[derive(Clone, Debug)]
pub struct HidDeviceState {
    /// Cihaz tipi (klavye, fare, gamepad)
    pub device_type: HidDeviceType,
    /// Mevcut klavye raporu
    pub keyboard: KeyboardBootReport,
    /// Önceki klavye raporu (değişim tespiti için)
    pub prev_keyboard: KeyboardBootReport,
    /// Mevcut fare raporu
    pub mouse: MouseBootReport,
    /// Mutlak fare X konumu (birikimli)
    pub mouse_x: i32,
    /// Mutlak fare Y konumu (birikimli)
    pub mouse_y: i32,
    /// LED durumu (bit0=NumLock, bit1=CapsLock, bit2=ScrollLock)
    pub leds: u8,
    /// Rapor gönderme aralığı (milisaniye)
    pub poll_interval_ms: u8,
    /// Boot protokolü etkin mi?
    pub boot_protocol: bool,
}

impl HidDeviceState {
    pub fn new(device_type: HidDeviceType) -> Self {
        Self {
            device_type,
            keyboard: KeyboardBootReport::new(),
            prev_keyboard: KeyboardBootReport::new(),
            mouse: MouseBootReport::new(),
            mouse_x: 0,
            mouse_y: 0,
            leds: 0,
            poll_interval_ms: 10, // 10ms = 100 rapor/saniye
            boot_protocol: false,
        }
    }

    /// Gelen 8-byte klavye raporunu işler.
    ///
    /// Önceki rapor saklanır (değişim tespiti için), ardından yeni rapor yüklenir.
    /// Format: [modifiers][reserved][key0][key1][key2][key3][key4][key5]
    pub fn update_keyboard(&mut self, report: &[u8]) {
        if report.len() >= 8 {
            self.prev_keyboard = self.keyboard; // Öncekini sakla
            self.keyboard.modifiers = report[0];
            // report[1] rezerve → atlanır
            self.keyboard.keys.copy_from_slice(&report[2..8]);
        }
    }

    /// Gelen 3-byte fare raporunu işler.
    ///
    /// Göreli hareket (x, y) birikimli konuma eklenir.
    /// `i8 as i32`: işaretli genişletme (örn. -128..+127)
    pub fn update_mouse(&mut self, report: &[u8]) {
        if report.len() >= 3 {
            self.mouse.buttons = report[0];
            self.mouse.x = report[1] as i8; // i8: işaretli göreli hareket
            self.mouse.y = report[2] as i8;
            self.mouse_x += self.mouse.x as i32; // Mutlak konuma ekle
            self.mouse_y += self.mouse.y as i32;
        }
    }

    /// Bu raporda yeni basılan tuşları döndürür.
    ///
    /// Algoritma: mevcut rapordaki, önceki raporda olmayan tuşlar.
    pub fn new_keys(&self) -> Vec<u8> {
        let mut new = Vec::new();
        for key in self.keyboard.keys.iter() {
            if *key != 0 && !self.prev_keyboard.keys.contains(key) {
                new.push(*key);
            }
        }
        new
    }

    /// Bu raporda bırakılan tuşları döndürür.
    ///
    /// Algoritma: önceki rapordaki, mevcut raporda olmayan tuşlar.
    pub fn released_keys(&self) -> Vec<u8> {
        let mut released = Vec::new();
        for key in self.prev_keyboard.keys.iter() {
            if *key != 0 && !self.keyboard.keys.contains(key) {
                released.push(*key);
            }
        }
        released
    }
}

// ============================================================================
// HID SÜRÜCÜSÜ
// Cihaz başlatma, protokol ayarı, LED kontrolü ve rapor işleme
// ============================================================================

/// HID sürücü örneği.
///
/// Her HID arabirimi için bir örnek oluşturulur.
/// `state: Mutex<HidDeviceState>`: çok çekirdekli güvenli durum erişimi.
/// `initialized: AtomicBool`: başlatma durumu (kesme işleyicilerinden güvenli okuma).
pub struct HidDriver {
    /// USB cihaz referansı
    pub device: UsbDevice,
    /// Arabirim numarası
    pub interface: u8,
    /// Interrupt IN uç noktası (cihazdan periyodik rapor)
    pub interrupt_in: Option<UsbEndpoint>,
    /// Interrupt OUT uç noktası (opsiyonel; LED kontrolü için)
    pub interrupt_out: Option<UsbEndpoint>,
    /// Cihaz durum kaydı (Mutex ile korunur)
    pub state: Mutex<HidDeviceState>,
    /// Başlatıldı mı bayrak
    pub initialized: AtomicBool,
}

impl HidDriver {
    /// Yeni HID sürücüsü oluşturur.
    pub fn new(device: UsbDevice, interface: u8) -> Self {
        Self {
            device,
            interface,
            interrupt_in: None,
            interrupt_out: None,
            state: Mutex::new(HidDeviceState::new(HidDeviceType::Unknown)),
            initialized: AtomicBool::new(false),
        }
    }

    /// HID cihazını başlatır.
    ///
    /// Adımlar:
    /// 1. Arabirim uç noktalarını bul (Interrupt IN + opsiyonel OUT)
    /// 2. Boot protokolünü etkinleştir (`SET_PROTOCOL`)
    /// 3. Boşta kalma süresini ayarla (`SET_IDLE`)
    /// 4. `initialized = true` yap
    pub fn init(&mut self) -> Result<(), UsbError> {
        // Interrupt uç noktalarını bul
        for iface in &self.device.interfaces {
            if iface.interface_number == self.interface {
                let mut state = self.state.lock();
                state.device_type = match (iface.subclass, iface.protocol) {
                    (1, 1) => HidDeviceType::Keyboard,
                    (1, 2) => HidDeviceType::Mouse,
                    _ => HidDeviceType::Generic,
                };
                drop(state);

                for ep in &iface.endpoints {
                    if ep.transfer_type == UsbTransferType::Interrupt {
                        if ep.direction == UsbDirection::In {
                            self.interrupt_in = Some(*ep); // Rapor alma
                        } else {
                            self.interrupt_out = Some(*ep); // LED gönderme
                        }
                    }
                }
                break;
            }
        }

        // Boot protokolünü etkinleştir (BIOS uyumlu sabit format)
        self.set_boot_protocol(true)?;

        // Boşta kalma süresini ayarla: 0=değişiklikte rapor gönder, 10=10ms
        self.set_idle(0, 10)?;

        self.initialized.store(true, Ordering::SeqCst);
        crate::serial_println!(
            "[HID] Device initialized on interface {} (type: {:?})",
            self.interface,
            self.state.lock().device_type
        );

        Ok(())
    }

    /// Boot veya rapor protokolünü seçer (SET_PROTOCOL isteği).
    ///
    /// `boot=true` → Protokol 0 (Boot): BIOS uyumlu sabit 8-byte rapor
    /// `boot=false` → Protokol 1 (Report): HID tanımlayıcılı esnek format
    ///
    /// `request_type=0x21`: Host→Device, Class, Interface
    pub fn set_boot_protocol(&self, boot: bool) -> Result<(), UsbError> {
        let protocol = if boot {
            HID_PROTOCOL_BOOT
        } else {
            HID_PROTOCOL_REPORT
        };

        let setup = UsbSetupPacket {
            request_type: 0x21, // Host→Device | Sınıf | Arabirim
            request: HID_SET_PROTOCOL,
            value: protocol as u16,
            index: self.interface as u16,
            length: 0,
        };

        let mut device = self.device.clone();
        device.control_transfer(setup, None)?;

        self.state.lock().boot_protocol = boot;
        Ok(())
    }

    /// Boşta kalma süresini ayarlar (SET_IDLE isteği).
    ///
    /// `duration_ms=0`: yalnızca değişiklikte rapor gönder (güç tasarrufu)
    /// `duration_ms=N`: N ms'de bir rapor gönder (periyodik sorgulama)
    ///
    /// wValue: `duration_ms << 8 | report_id` formatında paketlenir.
    pub fn set_idle(&self, report_id: u8, duration_ms: u8) -> Result<(), UsbError> {
        let setup = UsbSetupPacket {
            request_type: 0x21,
            request: HID_SET_IDLE,
            value: ((duration_ms as u16) << 8) | (report_id as u16),
            index: self.interface as u16,
            length: 0,
        };

        let mut device = self.device.clone();
        device.control_transfer(setup, None)
    }

    /// Klavye LED'lerini ayarlar (SET_REPORT çıkış raporu).
    ///
    /// `leds` bit maskesi: bit0=NumLock, bit1=CapsLock, bit2=ScrollLock
    ///
    /// Host→Device yönünde, 1-byte çıkış raporu olarak gönderilir.
    /// Interrupt OUT uç noktası varsa oradan; yoksa Control aktarımı kullanılır.
    pub fn set_leds(&self, leds: u8) -> Result<(), UsbError> {
        let mut state = self.state.lock();
        state.leds = leds;

        // Çıkış raporu gönder (SET_REPORT → OUTPUT type)
        let setup = UsbSetupPacket {
            request_type: 0x21,
            request: HID_SET_REPORT,
            value: ((HID_REPORT_OUTPUT as u16) << 8) | 0, // Rapor ID 0
            index: self.interface as u16,
            length: 1,
        };

        let mut report = [leds];
        drop(state);
        let mut device = self.device.clone();
        device.control_transfer(setup, Some(&mut report))?;

        // Interrupt OUT uç noktası varsa oradan gönder
        // self.send_output_report(&[leds])?;

        Ok(())
    }

    /// NumLock LED durumunu değiştirir (toggle).
    pub fn toggle_num_lock(&self) -> Result<(), UsbError> {
        let mut state = self.state.lock();
        state.leds ^= 0x01; // bit0 tersine çevir
        let leds = state.leds;
        drop(state);
        self.set_leds(leds)
    }

    /// CapsLock LED durumunu değiştirir (toggle).
    pub fn toggle_caps_lock(&self) -> Result<(), UsbError> {
        let mut state = self.state.lock();
        state.leds ^= 0x02; // bit1 tersine çevir
        let leds = state.leds;
        drop(state);
        self.set_leds(leds)
    }

    /// ScrollLock LED durumunu değiştirir (toggle).
    pub fn toggle_scroll_lock(&self) -> Result<(), UsbError> {
        let mut state = self.state.lock();
        state.leds ^= 0x04; // bit2 tersine çevir
        let leds = state.leds;
        drop(state);
        self.set_leds(leds)
    }

    /// Cihazı sorgular ve giriş olayını döndürür.
    ///
    /// Gerçek implementasyonda Interrupt IN uç noktasından okunur.
    /// Uç nokta sorgulaması periyodik zamanlayıcıya veya kesme işleyicisine bağlanmalıdır.
    pub fn poll(&self) -> Result<HidEvent, UsbError> {
        let endpoint = self.interrupt_in.ok_or(UsbError::NoDevice)?;
        let mut device = self.device.clone();
        if let Some(report) = device.interrupt_transfer_in(endpoint)? {
            return Ok(self.process_report(&report));
        }
        Ok(HidEvent::None)
    }

    /// Alınan rapor verisini işler ve HID olayına dönüştürür.
    ///
    /// Cihaz tipine göre doğru rapor işleme fonksiyonu çağrılır.
    /// Yeni basılan tuşlar `KeyPress(Vec<u8>)` olarak döndürülür.
    /// Fare hareketi `MouseMove { dx, dy, buttons }` olarak döndürülür.
    pub fn process_report(&self, data: &[u8]) -> HidEvent {
        let mut state = self.state.lock();

        match state.device_type {
            HidDeviceType::Keyboard => {
                state.update_keyboard(data);
                let new_keys = state.new_keys();
                let released = state.released_keys();

                if !new_keys.is_empty() {
                    HidEvent::KeyPress(new_keys)
                } else if !released.is_empty() {
                    HidEvent::KeyRelease(released)
                } else {
                    HidEvent::None
                }
            }
            HidDeviceType::Mouse => {
                state.update_mouse(data);
                HidEvent::MouseMove {
                    dx: state.mouse.x as i32,
                    dy: state.mouse.y as i32,
                    buttons: state.mouse.buttons,
                }
            }
            _ => HidEvent::None,
        }
    }
}

// ============================================================================
// HID OLAYI
// Klavye, fare ve oyun kolu olaylarını temsil eden enum
// ============================================================================

/// HID giriş olayı.
///
/// `process_report()` tarafından üretilir.
/// Rust enum'un veri taşıyabilmesi (ADT - Algebraic Data Type) burada kullanılır:
/// her varyant farklı türde veri taşır.
#[derive(Clone, Debug)]
pub enum HidEvent {
    /// Olay yok (değişiklik yok)
    None,
    /// Tuş basma olayı (yeni basılan HID kullanım kodları)
    KeyPress(Vec<u8>),
    /// Tuş bırakma olayı (bırakılan HID kullanım kodları)
    KeyRelease(Vec<u8>),
    /// Fare hareketi olayı
    MouseMove {
        dx: i32,     // X göreli hareket
        dy: i32,     // Y göreli hareket
        buttons: u8, // Düğme durumları (bit maskeleri)
    },
    /// Oyun kolu olayı
    Gamepad {
        buttons: u16,   // 16 adet dijital düğme (bit maskeleri)
        axes: [i16; 6], // 6 adet analog eksen değerleri
    },
}

// ============================================================================
// GLOBAL HID SÜRÜCÜ KAYIT DEFTERİ
// BTreeMap: arabirim numarasına göre sıralamalı sürücü erişimi
// ============================================================================

use alloc::collections::BTreeMap;

/// Global HID sürücü kayıt defteri.
///
/// `BTreeMap<arabirim_no, Arc<Mutex<HidDriver>>>`:
/// - `BTreeMap`: sıralı erişim, arabirim numarasına göre
/// - `Arc`: paylaşımlı sahiplik (birden fazla referans, referans sayacı)
/// - `Mutex<HidDriver>`: tek anda yalnızca bir thread erişir
lazy_static::lazy_static! {
    static ref HID_DRIVERS: Mutex<BTreeMap<u8, Arc<Mutex<HidDriver>>>> = Mutex::new(BTreeMap::new());
}

/// HID sürücüsü kaydeder ve kimlik döndürür.
///
/// Arabirim numarası, sürücü için benzersiz kimlik olarak kullanılır.
pub fn register_hid_driver(device: UsbDevice, interface: u8) -> Result<u8, UsbError> {
    let driver = HidDriver::new(device, interface);
    let id = interface; // Arabirim numarasını kimlik olarak kullan

    HID_DRIVERS.lock().insert(id, Arc::new(Mutex::new(driver)));
    Ok(id)
}

/// Kimliğe göre HID sürücüsü döndürür.
///
/// `cloned()`: `Arc` referans sayacını artırır; kilitlenme olmaz.
pub fn get_hid_driver(id: u8) -> Option<Arc<Mutex<HidDriver>>> {
    HID_DRIVERS.lock().get(&id).cloned()
}

/// Tüm HID cihazları sorgular ve olayları döndürür.
///
/// Her cihaz `poll()` ile sorgulanır; `HidEvent::None` dışındaki olaylar toplanır.
pub fn poll_all_hid() -> Vec<(u8, HidEvent)> {
    let mut events = Vec::new();
    let drivers = HID_DRIVERS.lock();

    for (id, driver) in drivers.iter() {
        if let Ok(event) = driver.lock().poll() {
            // Sadece gerçek olayları topla (None değil)
            if !matches!(event, HidEvent::None) {
                events.push((*id, event));
            }
        }
    }

    events
}

/// Kayıtlı tüm HID cihazları başlatır.
pub fn init_all_hid() {
    let drivers = HID_DRIVERS.lock();
    for (id, driver) in drivers.iter() {
        if let Err(e) = driver.lock().init() {
            crate::serial_println!("[HID] Failed to init device {}: {:?}", id, e);
        }
    }
}

// ============================================================================
// KLAVYESİ GİRİŞ KUYRUĞU
// Kesme işleyicisiyle uygulama arasında tuş olayı tamponu
// ============================================================================

use alloc::collections::VecDeque;

/// Klavye girişi kuyruğu.
///
/// Interrupt Service Routine (ISR) → kuyruk → uygulama akışı:
/// 1. USB HID kesme ISR'ı gelir → `push(key_code)` çağrılır
/// 2. Uygulama `read_key()` veya `try_read_key()` ile okur
///
/// `VecDeque`: FIFO (First In First Out) kuyruk; çift uçlu, O(1) push/pop.
/// Tuş kodları HID kullanım kodları (ASCII değil!); ASCII için `hid_to_ascii()` kullanılır.
pub struct KeyboardQueue {
    queue: Mutex<VecDeque<u8>>,
}

impl KeyboardQueue {
    /// Derleme zamanı oluşturma (`const fn`).
    pub const fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
        }
    }

    /// Kuyruğa tuş kodu ekler (ISR tarafından çağrılır).
    pub fn push(&self, key: u8) {
        self.queue.lock().push_back(key);
    }

    /// Kuyruktan tuş kodu çıkarır (FIFO sırası).
    pub fn pop(&self) -> Option<u8> {
        self.queue.lock().pop_front()
    }

    /// Kuyruk boş mu?
    pub fn is_empty(&self) -> bool {
        self.queue.lock().is_empty()
    }

    /// Kuyruktaki tuş sayısı.
    pub fn len(&self) -> usize {
        self.queue.lock().len()
    }
}

/// Global klavye kuyruğu (statik; tüm sistem için tek örnek).
///
/// `const fn new()` sayesinde `lazy_static!` gerektirmeden statik olarak tanımlanabilir.
pub static KEYBOARD_QUEUE: KeyboardQueue = KeyboardQueue::new();

/// Klavye kuyruğundan tuş okur (engelleyici / blocking).
///
/// Kuyruk boşsa `spin_loop()` ile döner (busy-wait).
/// Gerçek işletim sisteminde görev zamanlayıcısı uyku durumunda beklemeyi sağlar.
pub fn read_key() -> u8 {
    loop {
        if let Some(key) = KEYBOARD_QUEUE.pop() {
            return key;
        }
        // Zamanlayıcıya devret (gerçek uygulamada mevcut görevi uyuttur)
        core::hint::spin_loop();
    }
}

/// Tuş var mı? (engellemeden kontrol)
pub fn has_key() -> bool {
    !KEYBOARD_QUEUE.is_empty()
}

/// Engellemeden tuş okuma denemesi.
///
/// `Some(key_code)`: tuş varsa döndürür
/// `None`: kuyruk boşsa hemen döner
pub fn try_read_key() -> Option<u8> {
    KEYBOARD_QUEUE.pop()
}
