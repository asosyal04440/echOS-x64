//! TTY Lock-Free Halka Tamponu (Ring Buffer)
//!
//! Interrupt handler'lar (Klavye IRQ) ile user-space (sys_read) arasında
//! veri aktarımı için kullanılan asenkron, kilitsiz (lock-free) dairesel tampon.
//!
//! ## Ring Buffer (Halka Tampon) Nedir?
//!
//! Sabit boyutlu, başı ve sonu birbirine bağlı dairesel bir bellek alanıdır.
//! FIFO (First In, First Out) mantığıyla çalışır.
//!
//! ```
//! Ring Buffer Yapısı (TTY_BUF_SIZE = 4096 byte):
//!
//!        tail (okuma noktası)
//!          |
//!  ┌───────v─────────────────────────────┐
//!  │ . . . D A T A   D A T A . . . . .  │
//!  └─────────────────^───────────────────┘
//!                    |
//!                   head (yazma noktası)
//!
//! - tail == head  : Tampon BOŞ
//! - (head+1) == tail : Tampon DOLU (bir slot boş bırakılır)
//! - Yazma: head'e yaz, head++
//! - Okuma: tail'den oku, tail++
//! - Modüler aritmetik ile dairesel döngü sağlanır
//! ```
//!
//! ## Lock-Free (Kilitsiz) Tasarım
//!
//! SPSC (Single Producer, Single Consumer) modelinde mutex gerekmez.
//! - Üretici (producer): Klavye IRQ handler - head'i günceller
//! - Tüketici (consumer): Shell/sys_read - tail'i günceller
//! - `Ordering::Acquire/Release` ile donanım düzeyinde hafıza senkronizasyonu
//!
//! ## Neden UnsafeCell?
//!
//! Rust'ın sahiplik kuralları normalde birden fazla referans üzerinden
//! mutasyon izin vermez. UnsafeCell, bu kısıtlamayı bilinçli olarak
//! aşmak için kullanılan en temel "interior mutability" ilkeldir.
//! Ring buffer indekslerini (head, tail) atomik olarak güncellerken
//! data alanına ham erişim için zorunludur.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};

/// TTY tamponu için sabit boyut (4096 byte = 4 KB).
/// Bu boyut, bir terminal oturumunda makul miktarda girdi için yeterlidir.
pub const TTY_BUF_SIZE: usize = 4096;

/// SPSC (Single Producer, Single Consumer) tarzı sıfır kopya (zero copy)
/// veya asgari kopya ring buffer yapısı.
///
/// ## Alan Açıklamaları
///
/// - `data`: Karakter verilerini tutan ham bellek alanı (UnsafeCell ile sarılmış)
/// - `head`: Bir sonraki yazma pozisyonu - yalnızca üretici (IRQ) tarafından güncellenir
/// - `tail`: Bir sonraki okuma pozisyonu - yalnızca tüketici (shell) tarafından güncellenir
///
/// ```
/// Atomic ordering stratejisi:
///  - store(..., Release) : Bu yazmadan önce tüm diğer yazmalar görünür hale gelir
///  - load(..., Acquire)  : Bu okumadan sonraki tüm okumalar, Release'den sonraki
///                          verilerle senkronize edilir
///  - load(..., Relaxed)  : Senkronizasyon gerektirmeyen basit okumalar için
/// ```
pub struct TtyBuffer {
    data: UnsafeCell<[u8; TTY_BUF_SIZE]>,
    /// Bir sonraki yazma indeksi (üretici: klavye IRQ)
    head: AtomicUsize,
    /// Bir sonraki okuma indeksi (tüketici: sys_read)
    tail: AtomicUsize,
}

// Interrupt ve çekirdekler arası güvenli geçiş için.
// Bu struct hem IRQ bağlamında hem de normal kodda kullanılacağından
// Sync ve Send trait'lerini elle implement etmek gerekiyor.
// Güvenlik: SPSC modelinde head ve tail farklı taraflarca güncellenir.
unsafe impl Sync for TtyBuffer {}
unsafe impl Send for TtyBuffer {}

impl TtyBuffer {
    /// Yeni, boş bir TtyBuffer oluşturur (`const fn` olduğu için statik olarak da kullanılabilir).
    pub const fn new() -> Self {
        Self {
            data: UnsafeCell::new([0; TTY_BUF_SIZE]),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    /// TTY buffer'ına karakter yazar. Buffer doluysa `Err(())` döner.
    ///
    /// ## Çalışma Mantığı
    ///
    /// 1. `head` indeksini Relaxed ile oku (yerel kopyamız, başkası güncellemez)
    /// 2. `next_head = (head + 1) % TTY_BUF_SIZE` hesapla (modüler artırma)
    /// 3. `tail`'i Acquire ile oku - tüketicinin son okuduğu konumu gör
    /// 4. Eğer `next_head == tail` ise tampon dolu, `Err(())` döndür
    /// 5. Veriyi `data[head]` konumuna yaz
    /// 6. `head`'i Release ile güncelle - tüketici artık yeni veriyi görebilir
    pub fn push(&self, val: u8) -> Result<(), ()> {
        let head = self.head.load(Ordering::Relaxed);
        let next_head = (head + 1) % TTY_BUF_SIZE;

        if next_head == self.tail.load(Ordering::Acquire) {
            return Err(()); // Buffer dolu (overflow) - karakter düşürülüyor
        }

        unsafe {
            (*self.data.get())[head] = val;
        }
        // Release: veri yazmadan ÖNCE yukarıdaki store görünür olsun
        self.head.store(next_head, Ordering::Release);
        Ok(())
    }

    /// TTY buffer'ından bir karakter çeker. Buffer boşsa `None` döner.
    ///
    /// ## Çalışma Mantığı
    ///
    /// 1. `tail` indeksini Relaxed ile oku (yerel kopyamız, başkası güncellemez)
    /// 2. `head`'i Acquire ile oku - üreticinin son yazdığı konumu gör
    /// 3. Eğer `tail == head` ise tampon boş, `None` döndür
    /// 4. `data[tail]` konumundan veriyi oku
    /// 5. `tail`'i Release ile güncelle - üretici artık bu slotu yeniden kullanabilir
    pub fn pop(&self) -> Option<u8> {
        let tail = self.tail.load(Ordering::Relaxed);
        if tail == self.head.load(Ordering::Acquire) {
            return None; // Buffer boş
        }

        let val = unsafe { (*self.data.get())[tail] };
        // Release: veri okuduktan SONRA tail güncellenmeli
        self.tail
            .store((tail + 1) % TTY_BUF_SIZE, Ordering::Release);
        Some(val)
    }

    /// Buffer'ın en son yazılan karakterini siler (Backspace işlevi için).
    ///
    /// Klavye sürücüsü backspace tuşuna basıldığında bu metodu çağırır.
    /// Son yazılan karakteri, head'i bir geri alarak "yazar bozar".
    ///
    /// CAS (Compare-And-Swap) kullanılmasının sebebi: teorik olarak
    /// birden fazla çağrının aynı anda yarışmasını önlemek.
    /// Tekil üretici varsa `store` da yeterli olabilir,
    /// ama CAS daha güvenlidir ve LLVM bunu optimize eder.
    pub fn unpush(&self) -> bool {
        loop {
            let head = self.head.load(Ordering::Acquire);
            let tail = self.tail.load(Ordering::Acquire);
            if head == tail {
                return false; // Silebilecek karakter yok (tampon boş)
            }

            // Modüler geri alma: 0 ise TTY_BUF_SIZE-1'e sar
            let prev_head = if head == 0 {
                TTY_BUF_SIZE - 1
            } else {
                head - 1
            };

            // Atomik olarak head'i bir geri alıyoruz (CAS işlemi önerilir ama
            // tekil üretici varsa directly store yeterli olabilir, yine de CAS daha güvenli)
            if self
                .head
                .compare_exchange_weak(head, prev_head, Ordering::Release, Ordering::Relaxed)
                .is_ok()
            {
                return true;
            }
            // CAS başarısız olursa döngü tekrar dener (spin)
        }
    }

    /// Tamponun boş olup olmadığını kontrol eder.
    ///
    /// Hem `head` hem de `tail`, `Acquire` ordering ile okunur;
    /// bu sayede bir önceki tüm yazma işlemleri görünür olur.
    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::Acquire) == self.tail.load(Ordering::Acquire)
    }
}
