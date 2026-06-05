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
use core::sync::atomic::{AtomicBool, Ordering};
use pc_keyboard::DecodedKey;

// ============================================================================
// TERMIOS BAYRAKLARI (ldisc seviyesinde)
// ============================================================================

/// LineDiscipline termios durumu, atomik olarak güncellenir.
/// Tam Termios struct (tty/pty.rs) bu basit bayrakların kaynağıdır.
pub struct LdiscFlags {
    /// ICANON: Canonical (satır tamlama) modu
    pub canonical: AtomicBool,
    /// ECHO: Karakter yankılama
    pub echo: AtomicBool,
    /// ISIG: Ctrl+C/Z/\\ ile sinyal üretme
    pub isig: AtomicBool,
    /// ICRNL: CR→NL dönüşümü (input)
    pub icrnl: AtomicBool,
    /// ONLCR: NL→CR+NL dönüşümü (output)
    pub onlcr: AtomicBool,
}

impl LdiscFlags {
    pub const fn new() -> Self {
        Self {
            canonical: AtomicBool::new(true),
            echo: AtomicBool::new(true),
            isig: AtomicBool::new(true),
            icrnl: AtomicBool::new(true),
            onlcr: AtomicBool::new(true),
        }
    }
}

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
    /// Termios bayrakları (ICANON, ECHO, ISIG, ICRNL, ONLCR)
    pub flags: LdiscFlags,
    /// Foreground process group ID — POSIX: tcgetpgrp/tcsetpgrp için
    /// SIGINT/SIGTSTZ/SIGQUIT bu group'a dağıtılır
    pub foreground_pgid: core::sync::atomic::AtomicUsize,
}

impl LineDiscipline {
    /// Yeni bir LineDiscipline instance'ı oluşturur (const fn - statik kullanım için uygun).
    pub const fn new() -> Self {
        Self {
            input_buf: TtyBuffer::new(),
            output_buf: TtyBuffer::new(),
            flags: LdiscFlags::new(),
            foreground_pgid: core::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// tcgetpgrp(): foreground process group ID'yi döndür
    pub fn get_foreground_pgid(&self) -> usize {
        self.foreground_pgid.load(core::sync::atomic::Ordering::SeqCst)
    }

    /// tcsetpgrp(): foreground process group ID'yi ayarla
    pub fn set_foreground_pgid(&self, pgid: usize) {
        self.foreground_pgid.store(pgid, core::sync::atomic::Ordering::SeqCst);
    }

    /// Canonical modu aç/kapa (raw mode toggle).
    /// Raw modda her karakter anında okunabilir, satır tamponlama yapılmaz.
    pub fn set_canonical(&self, on: bool) {
        self.flags.canonical.store(on, Ordering::Relaxed);
    }

    /// Echo aç/kapa (şifre girişleri için).
    pub fn set_echo(&self, on: bool) {
        self.flags.echo.store(on, Ordering::Relaxed);
    }

    /// Sinyal üretimini aç/kapa.
    pub fn set_isig(&self, on: bool) {
        self.flags.isig.store(on, Ordering::Relaxed);
    }

    /// Klavye interrupt handler'ından gelen tuş basmalarını işler.
    ///
    /// Termios bayraklarına göre:
    /// - ISIG aktifse Ctrl+C→SIGINT, Ctrl+Z→SIGTSTP, Ctrl+\\→SIGQUIT
    /// - ECHO aktifse karakter output_buf'a yansıtılır
    /// - ICANON aktifse satır tamponlama yapılır, değilse raw mode
    pub fn receive_key(&self, key: DecodedKey) {
        let echo = self.flags.echo.load(Ordering::Relaxed);
        let isig = self.flags.isig.load(Ordering::Relaxed);
        let icrnl = self.flags.icrnl.load(Ordering::Relaxed);

        match key {
            DecodedKey::Unicode(c) => {
                // Backspace (0x08) - önceki karakteri sil
                if c == '\x08' || c == '\x7F' {
                    if self.input_buf.unpush() && echo {
                        let _ = self.output_buf.push(0x08);
                        let _ = self.output_buf.push(0x20);
                        let _ = self.output_buf.push(0x08);
                    }
                }
                // Ctrl+C (0x03) - SIGINT sinyali
                else if c == '\x03' && isig {
                    if echo {
                        let _ = self.output_buf.push(b'^');
                        let _ = self.output_buf.push(b'C');
                        let _ = self.output_buf.push(b'\n');
                    }
                    // Foreground process grubuna SIGINT gönder
                    crate::task::signal::send_signal_all(crate::task::signal::Signal::SIGINT).ok();
                }
                // Ctrl+Z (0x1A) - SIGTSTP sinyali
                else if c == '\x1A' && isig {
                    if echo {
                        let _ = self.output_buf.push(b'^');
                        let _ = self.output_buf.push(b'Z');
                        let _ = self.output_buf.push(b'\n');
                    }
                    crate::task::signal::send_signal_all(crate::task::signal::Signal::SIGTSTP).ok();
                }
                // Ctrl+\ (0x1C) - SIGQUIT sinyali
                else if c == '\x1C' && isig {
                    if echo {
                        let _ = self.output_buf.push(b'^');
                        let _ = self.output_buf.push(b'\\');
                        let _ = self.output_buf.push(b'\n');
                    }
                    crate::task::signal::send_signal_all(crate::task::signal::Signal::SIGQUIT).ok();
                }
                // Ctrl+D (0x04) - EOF
                else if c == '\x04' {
                    // EOF — canonical modda 0 bayt okuma döner
                    // input_buf'a özel EOF işaretçisi olarak '\x04' ekle
                    let _ = self.input_buf.push(0x04);
                }
                // Ctrl+U (0x15) - satır sil (VKILL)
                else if c == '\x15' {
                    // Tüm input buffer'ı temizle
                    while self.input_buf.pop().is_some() {
                        if echo {
                            let _ = self.output_buf.push(0x08);
                            let _ = self.output_buf.push(0x20);
                            let _ = self.output_buf.push(0x08);
                        }
                    }
                }
                // Ctrl+W (0x17) - son kelimeyi sil (VWERASE)
                else if c == '\x17' {
                    // Basitleştirilmiş: boşluk + karakter sil
                    // Tam implementasyon input_buf'ın son kelimesini çıkarır
                }
                // Normal tuş basımı
                else {
                    let mut ch = c;
                    // CR→NL dönüşümü
                    if ch == '\r' && icrnl {
                        ch = '\n';
                    }
                    let _ = self.input_buf.push(ch as u8);
                    if echo {
                        if ch == '\n' && self.flags.onlcr.load(Ordering::Relaxed) {
                            let _ = self.output_buf.push(b'\r');
                        }
                        let _ = self.output_buf.push(ch as u8);
                    }
                }
            }
            DecodedKey::RawKey(k) => {
                // Özel tuşları ANSI escape sequence'a dönüştür
                let seq: &[u8] = match k {
                    pc_keyboard::KeyCode::ArrowUp => b"\x1B[A",
                    pc_keyboard::KeyCode::ArrowDown => b"\x1B[B",
                    pc_keyboard::KeyCode::ArrowRight => b"\x1B[C",
                    pc_keyboard::KeyCode::ArrowLeft => b"\x1B[D",
                    pc_keyboard::KeyCode::Home => b"\x1B[H",
                    pc_keyboard::KeyCode::End => b"\x1B[F",
                    pc_keyboard::KeyCode::PageUp => b"\x1B[5~",
                    pc_keyboard::KeyCode::PageDown => b"\x1B[6~",
                    pc_keyboard::KeyCode::Delete => b"\x1B[3~",
                    pc_keyboard::KeyCode::Insert => b"\x1B[2~",
                    pc_keyboard::KeyCode::F1 => b"\x1BOP",
                    pc_keyboard::KeyCode::F2 => b"\x1BOQ",
                    pc_keyboard::KeyCode::F3 => b"\x1BOR",
                    pc_keyboard::KeyCode::F4 => b"\x1BOS",
                    pc_keyboard::KeyCode::F5 => b"\x1B[15~",
                    pc_keyboard::KeyCode::F6 => b"\x1B[17~",
                    pc_keyboard::KeyCode::F7 => b"\x1B[18~",
                    pc_keyboard::KeyCode::F8 => b"\x1B[19~",
                    pc_keyboard::KeyCode::F9 => b"\x1B[20~",
                    pc_keyboard::KeyCode::F10 => b"\x1B[21~",
                    pc_keyboard::KeyCode::F11 => b"\x1B[23~",
                    pc_keyboard::KeyCode::F12 => b"\x1B[24~",
                    _ => b"",
                };
                for &byte in seq {
                    let _ = self.input_buf.push(byte);
                    if echo {
                        let _ = self.output_buf.push(byte);
                    }
                }
            }
        }
    }

    /// User-space sys_read.
    /// Canonical modda '\n' veya EOF'a kadar bekler.
    /// Raw modda mevcut karakterleri hemen döner.
    pub fn sys_read(&self, buffer: &mut [u8]) -> usize {
        let canonical = self.flags.canonical.load(Ordering::Relaxed);
        let mut count = 0;
        while count < buffer.len() {
            if let Some(byte) = self.input_buf.pop() {
                // EOF kontrolü (Ctrl+D)
                if byte == 0x04 {
                    break;
                }
                buffer[count] = byte;
                count += 1;
                if canonical && byte == b'\n' {
                    break;
                }
                if !canonical {
                    // Raw modda tek karakter al ve hemen dön
                    break;
                }
            } else {
                break;
            }
        }
        count
    }
}
