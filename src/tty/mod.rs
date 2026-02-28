//! # TTY (TeleTYpewriter) Alt Sistemi
//!
//! TTY, işletim sistemlerinin klavye/ekran iletişimini soyutlayan bir katmandır.
//! Linux'ta /dev/tty0, /dev/pts/0 gibi cihazlar bu sistem üzerinden çalışır.
//!
//! ## TTY Veri Akışı (ASCII Diyagramı)
//!
//! ```
//!  Klavye IRQ           Line Discipline            User Space
//!  ─────────          ──────────────────        ──────────────
//!  [Tuş basımı]  -->  [receive_key()]      -->  [sys_read()]
//!       |             [input_buf (ring)]         [shell/uygulama]
//!       |             [output_buf (echo)]
//!       |                    |
//!       v                    v
//!  [UART / PS2]      [Backspace/Ctrl+C     ]
//!                    [işleme (cooking)     ]
//!
//!  NOT: "Cooking" = ham tuş girdilerini POSIX uyumlu satır tamponu haline
//!  getirme sürecine verilen isim (N_TTY line discipline).
//! ```
//!
//! ## Modül Yapısı
//!
//! - `buffer`  : Lock-free SPSC ring buffer (IRQ <-> user-space arası)
//! - `ldisc`   : Line Discipline - N_TTY uygulaması
//! - `pty`     : Pseudo Terminal - SSH/tmux için sanallaştırılmış TTY çifti
//! - `ansi`    : ANSI/VT100 escape sequence ayrıştırıcı ve oluşturucu

pub mod buffer;
pub mod ldisc;
pub mod pty;
pub mod ansi;

use lazy_static::lazy_static;

lazy_static! {
    /// Sistem genelindeki varsayılan TTY Line Discipline instance'ı.
    /// `lazy_static!` ile tanımlandığı için ilk erişimde initialize edilir.
    /// Sonradan `prewarm` ile boot sırasında önceden başlatılır.
    pub static ref DEFAULT_TTY: ldisc::LineDiscipline = ldisc::LineDiscipline::new();
}

/// TTY alt sistemini başlatır.
///
/// Başlatma sırası önemlidir:
/// 1. PTY yöneticisi başlatılır (`/dev/pts/` sanal dosya sistemi için)
/// 2. DEFAULT_TTY lazy instance'ı sıcak tutulur (prewarm)
/// 3. Klavye IRQ handler'ına TTY'nin hazır olduğu bildirilir
pub fn init() {
    pty::init();
    // DEFAULT_TTY lazy instance'ını boot sırasında prewarm et.
    // Böylece ilk keyboard IRQ içinde lazy init tetiklenmez.
    // Lazy init, IRQ bağlamında çağrılırsa deadlock veya panic'e yol açabilir.
    let _ = &*DEFAULT_TTY;
    // Klavye interrupt handler'ının TTY'ye güvenle yazabileceğini işaretle
    crate::keyboard::mark_tty_ready();
    crate::serial_println!("[TTY] Subsystem initialized");
}

/// Re-export convenience items
/// Dışarıdan kullanım kolaylığı için sık kullanılan tipleri yeniden dışa aktarır.
pub use ansi::{AnsiBuilder, AnsiParser, Color, EscapeSequence, TerminalState};
pub use pty::{PtyManager, PtyMaster, PtyPair, PtySlave, Termios, Winsize, PTY_MANAGER};
