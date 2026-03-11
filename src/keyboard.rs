//! # echOS Klavye Tamponu (Keyboard Buffer)
//!
//! Klavye girişi için halka tamponu (ring buffer).
//! Donanım kesinti işleyicisinden (interrupt handler) gelen tuş olaylarını saklar
//! ve üst katmanlara (TTY, uygulama) iletir.
//!
//! ## Tasarım Kararı: Neden Ring Buffer?
//! Klavye kesmesi herhangi bir anda tetiklenebilir — uygulama o andaki
//! tuşa hazır olmayabilir. Ring buffer bu "zaman uyumsuzluğunu" çözer:
//! - **Interrupt handler**: Tuşu hızla buffer'a yazar ve döner.
//! - **Uygulama/TTY**: Uygun olduğunda buffer'dan okur.
//!
//! ## Veri Akışı
//! ```text
//! PS/2 / USB Klavye
//!       │
//!       ▼ (donanım kesmesi)
//! interrupt_handler()
//!       │
//!       ▼
//! push_key(DecodedKey)
//!   ├── TTY_READY? → tty::receive_key() [satır disiplini]
//!   └── KEYBOARD_BUFFER.push(key) [uygulama için sakla]
//!       │
//!       ▼ (uygulama okuma)
//! read_key() → Option<DecodedKey>
//! ```
//!
//! ## TTY Satır Disiplini
//! TTY (Teletypewriter), ham tuş kodlarını düzenlenmiş karakter akışlarına
//! dönüştürür. Backspace silme, echo (yansıma), satır sonu işleme gibi
//! özellikler TTY katmanında gerçekleşir.

use alloc::collections::VecDeque;
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use pc_keyboard::{DecodedKey, KeyCode, KeyEvent, KeyState};
use spin::Mutex;
use x86_64::instructions::interrupts;

/// Klavye tamponunun maksimum kapasitesi (tuş sayısı).
/// 128 tuş yeterlidir — hızlı yazım sırasında en fazla bu kadar
/// tuş işlenmeden birikebilir. Taşma durumunda yeni tuşlar sessizce atılır.
const BUFFER_SIZE: usize = 128;

/// TTY katmanının hazır olup olmadığını izleyen atomik bayrak.
///
/// Önyükleme sırasında `lazy_static` nesneleri henüz hazır olmayabilir.
/// TTY `push_key` tarafından kullanıldığından, TTY hazır olmadan çağrı
/// yapılırsa sayfa hatası (PAGE FAULT) oluşabilir. Bu bayrak bu sorunu önler.
///
/// `SeqCst` (Sequentially Consistent) bellek sıralaması: en güçlü sıralama
/// garantisi — tüm thread'ler aynı yazma sırasını görür.
static TTY_READY: AtomicBool = AtomicBool::new(false);

static MOD_STATE: AtomicU8 = AtomicU8::new(0);

/// TTY katmanının hazır olduğunu işaretler.
///
/// TTY alt sistemi başlatıldığında çağrılır. Bu çağrıdan sonra
/// `push_key`, tuşları hem buffer'a hem de TTY'ye iletir.
pub fn mark_tty_ready() {
    TTY_READY.store(true, Ordering::SeqCst);
}

/// Klavye tuş tamponu — FIFO (İlk Giren İlk Çıkar) kuyruk yapısı.
///
/// `VecDeque`, çift uçlu kuyruk (deque) yapısıdır:
/// - `push_back`: interrupt handler tuşu arkaya ekler
/// - `pop_front`: uygulama önden tuşu çeker
///
/// Bu yapı, kesme bağlamında (interrupt context) ve normal bağlamda
/// (process context) eşzamanlı erişim için `Mutex` ile korunur.
pub struct KeyboardBuffer {
    buffer: VecDeque<DecodedKey>,
}

impl KeyboardBuffer {
    /// Boş bir klavye tamponu oluşturur.
    /// `VecDeque` kapasitesi `BUFFER_SIZE` ile önceden ayrılır.
    pub fn new() -> Self {
        Self {
            buffer: VecDeque::with_capacity(BUFFER_SIZE),
        }
    }

    /// Tampona bir tuş olayı ekler.
    ///
    /// Tampon doluysa tuş sessizce atılır (buffer overflow koruması).
    /// Gerçek sistemlerde bu durumda klavye zili çalabilir veya
    /// bir hata bayrağı ayarlanabilir.
    pub fn push(&mut self, key: DecodedKey) {
        if self.buffer.len() < BUFFER_SIZE {
            self.buffer.push_back(key);
        }
    }

    /// Tamponun önünden bir tuş olayı çıkarır (FIFO düzeni).
    ///
    /// Tampon boşsa `None` döner. Blocking değil — uygulama
    /// kendi döngüsünde `has_key()` ile kontrol edip okuyabilir.
    pub fn pop(&mut self) -> Option<DecodedKey> {
        self.buffer.pop_front()
    }

    /// Tamponun boş olup olmadığını döner.
    /// `has_key()` wrapper'ı tarafından kullanılır.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }
}

lazy_static::lazy_static! {
    /// Global klavye tamponu — tüm sistem için tek örnek (singleton).
    ///
    /// `lazy_static` ile geç başlatılır: ilk erişimde Mutex + VecDeque oluşturulur.
    /// `Mutex<KeyboardBuffer>` sayesinde kesme bağlamı ve uygulama bağlamı
    /// arasında güvenli paylaşım sağlanır.
    static ref KEYBOARD_BUFFER: Mutex<KeyboardBuffer> = Mutex::new(KeyboardBuffer::new());
}

/// Klavye kesme işleyicisinden çağrılır — çözümlenmiş tuşu sisteme iletir.
///
/// ## Üç Katmanlı İletim
/// 1. **TTY satır disiplini**: TTY hazırsa tuşu satır disiplinine gönderir.
///    Satır disiplini: satır yenileme, echo, backspace işleme yapar.
/// 2. **Klavye tamponu**: Uygulamaların doğrudan okuması için tampona ekler.
/// 3. **Window Server**: Odaklanmış pencereye WinEvent olarak iletir.
///
/// ## Kesme Bağlamı Güvenliği
/// `without_interrupts` ile tampon erişimi sırasında kesintiler devre dışı
/// bırakılır. Bu, kesme işleyicisi tampon kilidini (Mutex) tutarken yeniden
/// kesme gelmesini ve kilitlenmeyi (deadlock) önler.
pub fn push_key(key: DecodedKey) {
    // TTY Line Discipline'e yolla - sadece TTY hazır olduğunda
    // Bu, lazy_static initialization sırasında PAGE FAULT'u önler
    if TTY_READY.load(Ordering::SeqCst) {
        crate::tty::DEFAULT_TTY.receive_key(key.clone());
    }

    interrupts::without_interrupts(|| {
        KEYBOARD_BUFFER.lock().push(key.clone());
    });
}

pub fn dispatch_key_event(key_event: KeyEvent, decoded: Option<DecodedKey>) {
    let pressed = matches!(key_event.state, KeyState::Down);
    let _mods = update_mods(key_event.code, pressed);
    let scancode = raw_key_to_scancode(key_event.code) as u16;

    // Titan Input Dispatch: Push to unified SPSC queue
    crate::drivers::input::push_event(crate::drivers::input::InputEvent::Keyboard {
        decoded: decoded.clone(),
        scan_code: scancode,
        modifiers: MOD_STATE.load(Ordering::SeqCst),
        state: key_event.state,
    });

    if pressed {
        if let Some(key) = decoded {
            push_key(key);
        }
    }
}

/// pc_keyboard::KeyCode → u32 scancode dönüşümü
fn update_mods(code: KeyCode, pressed: bool) -> u8 {
    let bit = match code {
        KeyCode::LShift | KeyCode::RShift => 0b0000_0001,
        KeyCode::LControl | KeyCode::RControl => 0b0000_0010,
        KeyCode::LAlt | KeyCode::RAltGr => 0b0000_0100,
        KeyCode::LWin | KeyCode::RWin => 0b0000_1000,
        _ => 0,
    };
    if bit != 0 {
        if pressed {
            MOD_STATE.fetch_or(bit, Ordering::SeqCst);
        } else {
            MOD_STATE.fetch_and(!bit, Ordering::SeqCst);
        }
    }
    MOD_STATE.load(Ordering::SeqCst)
}

fn raw_key_to_scancode(raw: KeyCode) -> u32 {
    match raw {
        KeyCode::Escape => 0x01,
        KeyCode::Key1 => 0x02,
        KeyCode::Key2 => 0x03,
        KeyCode::Key3 => 0x04,
        KeyCode::Key4 => 0x05,
        KeyCode::Key5 => 0x06,
        KeyCode::Key6 => 0x07,
        KeyCode::Key7 => 0x08,
        KeyCode::Key8 => 0x09,
        KeyCode::Key9 => 0x0A,
        KeyCode::Key0 => 0x0B,
        KeyCode::OemMinus => 0x0C,
        KeyCode::OemPlus => 0x0D,
        KeyCode::F1 => 0x3B,
        KeyCode::F2 => 0x3C,
        KeyCode::F3 => 0x3D,
        KeyCode::F4 => 0x3E,
        KeyCode::F5 => 0x3F,
        KeyCode::F6 => 0x40,
        KeyCode::F7 => 0x41,
        KeyCode::F8 => 0x42,
        KeyCode::F9 => 0x43,
        KeyCode::F10 => 0x44,
        KeyCode::F11 => 0x57,
        KeyCode::F12 => 0x58,
        KeyCode::Backspace => 0x0E,
        KeyCode::Tab => 0x0F,
        KeyCode::Q => 0x10,
        KeyCode::W => 0x11,
        KeyCode::E => 0x12,
        KeyCode::R => 0x13,
        KeyCode::T => 0x14,
        KeyCode::Y => 0x15,
        KeyCode::U => 0x16,
        KeyCode::I => 0x17,
        KeyCode::O => 0x18,
        KeyCode::P => 0x19,
        KeyCode::Oem4 => 0x1A,
        KeyCode::Oem6 => 0x1B,
        KeyCode::Oem5 => 0x2B,
        KeyCode::Oem7 => 0x56,
        KeyCode::Return => 0x1C,
        KeyCode::LControl | KeyCode::RControl => 0x1D,
        KeyCode::A => 0x1E,
        KeyCode::S => 0x1F,
        KeyCode::D => 0x20,
        KeyCode::F => 0x21,
        KeyCode::G => 0x22,
        KeyCode::H => 0x23,
        KeyCode::J => 0x24,
        KeyCode::K => 0x25,
        KeyCode::L => 0x26,
        KeyCode::Oem1 => 0x27,
        KeyCode::Oem3 => 0x28,
        KeyCode::Oem8 => 0x29,
        KeyCode::LShift => 0x2A,
        KeyCode::Z => 0x2C,
        KeyCode::X => 0x2D,
        KeyCode::C => 0x2E,
        KeyCode::V => 0x2F,
        KeyCode::B => 0x30,
        KeyCode::N => 0x31,
        KeyCode::M => 0x32,
        KeyCode::OemComma => 0x33,
        KeyCode::OemPeriod => 0x34,
        KeyCode::Oem2 => 0x35,
        KeyCode::RShift => 0x36,
        KeyCode::LAlt | KeyCode::RAltGr => 0x38,
        KeyCode::Spacebar => 0x39,
        KeyCode::CapsLock => 0x3A,
        KeyCode::ArrowUp => 0x48,
        KeyCode::ArrowDown => 0x50,
        KeyCode::ArrowLeft => 0x4B,
        KeyCode::ArrowRight => 0x4D,
        KeyCode::Home => 0x47,
        KeyCode::End => 0x4F,
        KeyCode::PageUp => 0x49,
        KeyCode::PageDown => 0x51,
        KeyCode::Insert => 0x52,
        KeyCode::Delete => 0x53,
        KeyCode::PrintScreen => 0x37,
        KeyCode::NumpadLock => 0x45,
        KeyCode::ScrollLock => 0x46,
        KeyCode::LWin | KeyCode::RWin => 0x5B,
        _ => 0x00, // Bilinmeyen tuş
    }
}

/// Tampondan bir tuş olayı okur — engellemeyen (non-blocking).
///
/// Tuş var ise `Some(DecodedKey)`, yoksa `None` döner.
/// Uygulama bu fonksiyonu döngüde çağırarak klavye girişini işleyebilir.
///
/// `without_interrupts`: Okuma sırasında kesintiler devre dışı bırakılır
/// (Mutex kilidi alınırken kesme gelmesi kilitlenmeye neden olabilir).
pub fn read_key() -> Option<DecodedKey> {
    interrupts::without_interrupts(|| KEYBOARD_BUFFER.lock().pop())
}

/// Tamponda bekleyen tuş olayı olup olmadığını kontrol eder.
///
/// Uygulamalar `read_key()` çağırmadan önce bunu kontrol ederek
/// gereksiz kilit alma işleminden kaçınabilir.
pub fn has_key() -> bool {
    interrupts::without_interrupts(|| !KEYBOARD_BUFFER.lock().is_empty())
}
