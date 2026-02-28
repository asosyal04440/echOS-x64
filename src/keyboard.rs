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
use core::sync::atomic::{AtomicBool, Ordering};
use pc_keyboard::DecodedKey;
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
/// ## İki Katmanlı İletim
/// 1. **TTY satır disiplini**: TTY hazırsa tuşu satır disiplinine gönderir.
///    Satır disiplini: satır yenileme, echo, backspace işleme yapar.
/// 2. **Klavye tamponu**: Uygulamaların doğrudan okuması için tampona ekler.
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
        KEYBOARD_BUFFER.lock().push(key);
    });
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
