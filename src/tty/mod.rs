pub mod buffer;
pub mod ldisc;
pub mod pty;
pub mod ansi;

use lazy_static::lazy_static;

lazy_static! {
    /// Sistem genelindeki varsayılan TTY Line Discipline instance'ı.
    pub static ref DEFAULT_TTY: ldisc::LineDiscipline = ldisc::LineDiscipline::new();
}

/// TTY alt sistemini başlatır.
pub fn init() {
    pty::init();
    // DEFAULT_TTY lazy instance'ını boot sırasında prewarm et.
    // Böylece ilk keyboard IRQ içinde lazy init tetiklenmez.
    let _ = &*DEFAULT_TTY;
    // Klavye interrupt handler'ının TTY'ye güvenle yazabileceğini işaretle
    crate::keyboard::mark_tty_ready();
    crate::serial_println!("[TTY] Subsystem initialized");
}

/// Re-export convenience items
pub use ansi::{AnsiBuilder, AnsiParser, Color, EscapeSequence, TerminalState};
pub use pty::{PtyManager, PtyMaster, PtyPair, PtySlave, Termios, Winsize, PTY_MANAGER};
