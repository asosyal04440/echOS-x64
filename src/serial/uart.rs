//! # echOS UART Sürücüsü
//! 
//! 16550 UART serial port sürücüsü (COM1 - 0x3F8).
//! Debug çıktısı için kullanılır.

use x86_64::instructions::port::Port;
use core::fmt;
use spin::Mutex;

/// Serial port yapısı.
/// 16550 UART register'larını içerir.
pub struct SerialPort {
    /// Data register (okuma/yazma)
    data: Port<u8>,
    /// Interrupt enable register
    int_en: Port<u8>,
    /// FIFO control register
    fifo_ctrl: Port<u8>,
    /// Line control register
    line_ctrl: Port<u8>,
    /// Modem control register
    modem_ctrl: Port<u8>,
    /// Line status register
    line_sts: Port<u8>,
}

impl SerialPort {
    /// Yeni bir SerialPort instance oluşturur.
    pub const unsafe fn new(base: u16) -> Self {
        Self {
            data: Port::new(base),
            int_en: Port::new(base + 1),
            fifo_ctrl: Port::new(base + 2),
            line_ctrl: Port::new(base + 3),
            modem_ctrl: Port::new(base + 4),
            line_sts: Port::new(base + 5),
        }
    }

    /// Serial port'u yapılandırır (38400 baud, 8N1).
    pub fn init(&mut self) {
        unsafe {
            self.int_en.write(0x00);    // Tüm interrupt'ları kapat
            self.line_ctrl.write(0x80); // DLAB aktif (baud rate ayarı)
            self.data.write(0x03);      // Divisor low byte (38400 baud)
            self.int_en.write(0x00);    // Divisor high byte
            self.line_ctrl.write(0x03); // 8 bit, parity yok, 1 stop bit
            self.fifo_ctrl.write(0xC7); // FIFO aktif, 14 byte threshold
            self.modem_ctrl.write(0x0B); // IRQ aktif, RTS/DSR set
        }
    }

    /// Line status register'ı okur.
    fn line_sts(&mut self) -> u8 {
        unsafe { self.line_sts.read() }
    }

    /// Bir byte gönderir (blocking).
    pub fn send(&mut self, data: u8) {
        // Transmit buffer boş olana kadar bekle
        while self.line_sts() & 0x20 == 0 {}
        unsafe { self.data.write(data) }
    }
}

impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            self.send(byte);
        }
        Ok(())
    }
}

/// Global COM1 serial port (0x3F8)
pub static SERIAL1: Mutex<SerialPort> = Mutex::new(unsafe { SerialPort::new(0x3F8) });

/// Serial port'u başlatır.
pub fn init() {
    SERIAL1.lock().init();
}

/// İç kullanım için print fonksiyonu.
#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    use x86_64::instructions::interrupts;

    interrupts::without_interrupts(|| {
        SERIAL1.lock().write_fmt(args).unwrap();
    });
}

/// Serial porta formatlı çıktı yazdırır.
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {
        $crate::serial::uart::_print(format_args!($($arg)*));
    };
}

/// Serial porta formatlı çıktı yazdırır (yeni satır ekler).
#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($fmt:expr) => ($crate::serial_print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => ($crate::serial_print!(concat!($fmt, "\n"), $($arg)*));
}
