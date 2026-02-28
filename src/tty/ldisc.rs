//! Satır Disiplini (Line Discipline - N_TTY)
//!
//! Klavye sürücüsünden gelen ham karakterlerin tamponlanması, "pişirilmesi" (cooking)
//! ve özel tuş kombinasyonlarının (Ctrl+C, Backspace vb.) işlenmesinden sorumlu modül.
//!
//! ## Line Discipline Nedir?
//!
//! Linux'ta `N_TTY` olarak bilinen katman; ham klavye girdisini kabul edilebilir
//! terminal davranışına dönüştürür. Buna "pişirme" (cooking) denir çünkü
//! ham (raw) girdiyi işlenmiş (cooked) hale getirir.
//!
//! ## Veri Akışı (ASCII Diyagramı)
//!
//! ```
//!  ┌─────────────────────────────────────────────────────────┐
//!  │                   LINE DISCIPLINE (N_TTY)               │
//!  │                                                         │
//!  │  Klavye IRQ                                             │
//!  │  ─────────                                              │
//!  │  receive_key()                                          │
//!  │       │                                                 │
//!  │       ├── '\x08' (Backspace) ──> input_buf.unpush()    │
//!  │       │                     ──> output_buf.push(BS+SP+BS) │
//!  │       │                                                  │
//!  │       ├── '\x03' (Ctrl+C) ───> SIGINT sinyali           │
//!  │       │                   ───> output_buf.push("^C\n")  │
//!  │       │                                                  │
//!  │       └── Normal karakter ──> input_buf.push(c)         │
//!  │                          ──> output_buf.push(c) (echo)  │
//!  │                                                          │
//!  │  ─────────────────────────────────────────────────────  │
//!  │                                                          │
//!  │  sys_read() (User-space thread'i buradan okur)          │
//!  │       │                                                  │
//!  │       └── input_buf.pop() ──> buffer'a kopyala          │
//!  │           '\n' görününce satır pişmiş, dön              │
//!  └─────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Echo (Yankı) Mekanizması
//!
//! Kullanıcı bir tuşa bastığında, karakter hem:
//! - `input_buf`'a eklenir (shell tarafından okunacak)
//! - `output_buf`'a eklenir (ekranda görünmesi için)
//!
//! Bu sayede kullanıcı yazdığını görebilir (echo-on modu).
//! Şifre girişlerinde echo kapatılır (echo-off modu, henüz implement edilmedi).

use super::buffer::TtyBuffer;
use pc_keyboard::DecodedKey;

/// Satır Disiplini yapısı.
///
/// İki ayrı tampon kullanır:
/// - `input_buf`: Klavyeden gelen ve shell için bekleyen karakterler
/// - `output_buf`: Ekrana yazdırılacak karakterler (echo + özel sekanslar)
pub struct LineDiscipline {
    /// Giriş tamponu: shell'in sys_read ile okuyacağı karakterler
    pub input_buf: TtyBuffer,
    /// Çıkış tamponu: framebuffer/terminal sürücüsünün okuyacağı karakterler (echo)
    pub output_buf: TtyBuffer,
}

impl LineDiscipline {
    /// Yeni bir LineDiscipline instance'ı oluşturur (const fn - statik kullanım için uygun).
    pub const fn new() -> Self {
        Self {
            input_buf: TtyBuffer::new(),
            output_buf: TtyBuffer::new(),
        }
    }

    /// Klavye interrupt handler'ından gelen tuş basmalarını işler.
    ///
    /// Bu metod IRQ bağlamında çağrıldığından:
    /// - Mutex kilitleme yapılamaz (deadlock riski)
    /// - Lock-free buffer kullanılır
    /// - İşlem mümkün olduğunca kısa tutulur
    ///
    /// ## İşlenen Özel Tuşlar
    ///
    /// | Tuş         | Kod    | İşlem                              |
    /// |-------------|--------|------------------------------------|
    /// | Backspace   | 0x08   | Son karakteri sil, echo BS+SPC+BS  |
    /// | Ctrl+C      | 0x03   | SIGINT sinyali (^C ekrana yaz)     |
    /// | Enter (\n)  | 0x0A   | Satır pişti, shell okuyabilir      |
    /// | Diğer       | -      | input_buf ve output_buf'a ekle     |
    pub fn receive_key(&self, key: DecodedKey) {
        match key {
            DecodedKey::Unicode(c) => {
                // Backspace (0x08) - önceki karakteri sil
                if c == '\x08' {
                    if self.input_buf.unpush() {
                        // Echo olarak da backspace yollayıp karakter üzerine boşluk basalım (siliş efekti)
                        // Teknik: BS (geri git) + SPC (üzerine boşluk yaz) + BS (tekrar geri git)
                        let _ = self.output_buf.push(0x08);
                        let _ = self.output_buf.push(0x20); // Boşluk karakteri
                        let _ = self.output_buf.push(0x08); // İmleci geri al
                    }
                }
                // Ctrl+C (0x03) - SIGINT sinyali
                else if c == '\x03' {
                    crate::serial_println!("[TTY] Ctrl+C Received - Sinyal yollanacak!");
                    // Ekrana "^C" ve yeni satır yaz (Linux terminal davranışını taklit et)
                    let _ = self.output_buf.push(b'^');
                    let _ = self.output_buf.push(b'C');
                    let _ = self.output_buf.push(b'\n');
                }
                // Normal tuş basımı - input ve output buffer'a ekle
                else {
                    let _ = self.input_buf.push(c as u8);
                    // Echo (Ekranda görünmesi için output_buf'a yansıt)
                    // Bu, kullanıcının yazdığının ekranda görünmesini sağlar
                    let _ = self.output_buf.push(c as u8);

                    if c == '\n' {
                        // Yeni satır karakteri geldiyse satır pişmiştir.
                        // Bekleyen "read" sys_call varsa, io_uring fırlatıp okutabiliriz.
                        // TODO: Gelecekte blocking sys_read için wakeup mekanizması eklenecek
                    }
                }
            }
            DecodedKey::RawKey(_k) => {
                // Yön tuşları veya özel tuşlar (F1, F2, Home, End vb.)
                // Şu an işlenmiyor; gelecekte ANSI escape sequence'a dönüştürülecek
                // Örnek: ArrowUp -> "\x1B[A", ArrowDown -> "\x1B[B"
            }
        }
    }

    /// User-space thread'lerin sys_read sistem çağrısı aracılığıyla
    /// karakter okumasını sağlar.
    ///
    /// ## Satır Tamponlama (Line Buffering)
    ///
    /// "Canonical mode" (pişirilmiş mod) kullanıldığında, sys_read
    /// bir tam satır (yani '\n' ile biten bir dizi) tamamlanana kadar
    /// geriye karakter dönmez. Bu, Bash gibi uygulamaların varsayılan davranışıdır.
    ///
    /// Şu anki implementasyon non-blocking'dir (bloke olmaz):
    /// - Buffer boşsa hemen döner (0 karakter okundu)
    /// - '\n' görününce satır sonu sayılır ve döner
    ///
    /// ## Dönüş Değeri
    ///
    /// Okunan bayt sayısı. 0 ise buffer boştu.
    pub fn sys_read(&self, buffer: &mut [u8]) -> usize {
        let mut count = 0;
        while count < buffer.len() {
            if let Some(byte) = self.input_buf.pop() {
                buffer[count] = byte;
                count += 1;
                // Satır pişirme kuralı: '\n' görününce buffer sonunu kapat
                // Kullanıcı Enter'a bastıysa komut tamamdır
                if byte == b'\n' {
                    break;
                }
            } else {
                break; // Şimdilik non-blocking gibi kırıyoruz
            }
        }
        count
    }
}
