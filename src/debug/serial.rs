//! # echOS Basit Seri Port (Emergency Serial)
//!
//! Kernel panic veya kilitlenme durumlarında kullanılmak üzere
//! interrupt gerektirmeyen, doğrudan port erişimi sağlayan basit sürücü.
//! Normal loglama için `serial/uart.rs` tercih edilmelidir.

use core::fmt;
use x86_64::instructions::port::Port;

/// Acil durum seri port yapısı
pub struct SimpleSerial {
    data: Port<u8>,
    l_sr: Port<u8>,
}

impl SimpleSerial {
    /// Port adresi ile yeni instance oluşturur (Genelde 0x3F8 = COM1).
    pub const unsafe fn new(base: u16) -> Self {
        Self {
            data: Port::new(base),
            l_sr: Port::new(base + 5),
        }
    }

    /// Gönderim tamponunun (transmit buffer) boş olup olmadığını kontrol eder.
    pub fn is_transmit_empty(&mut self) -> bool {
        unsafe { self.l_sr.read() & 0x20 != 0 }
    }

    /// Bir byte gönderir (Buffer boşalana kadar meşgul bekleme yapar).
    pub fn write_byte(&mut self, byte: u8) {
        let mut spins = 1_000_000u32;
        while !self.is_transmit_empty() {
            if spins == 0 {
                break;
            }
            spins = spins.saturating_sub(1);
            core::hint::spin_loop();
        }
        unsafe {
            self.data.write(byte);
        }
    }

    /// String gönderir.
    pub fn force_write_str(&mut self, s: &str) {
        for byte in s.bytes() {
            self.write_byte(byte);
        }
    }
}

/// Global acil durum seri portu.
pub static mut EMERGENCY_SERIAL: SimpleSerial = unsafe { SimpleSerial::new(0x3F8) };

/// Seri portu varsayılan ayarlarla başlatır.
pub fn init() {
    unsafe {
        let base = 0x3F8;
        let mut int_en = Port::<u8>::new(base + 1);
        let mut fifo = Port::<u8>::new(base + 2);
        let mut lcr = Port::<u8>::new(base + 3);
        let mut mcr = Port::<u8>::new(base + 4);

        int_en.write(0x00); // Interruptları kapat
        lcr.write(0x80); // DLAB bitini aç (Baud rate ayarı için)
        Port::<u8>::new(base).write(0x03); // Divisor Low: 3 (38400 baud)
        Port::<u8>::new(base + 1).write(0x00); // Divisor High
        lcr.write(0x03); // 8 bit, No parity, 1 stop bit
        fifo.write(0xC7); // FIFO temizle ve aç
        mcr.write(0x0B); // AUX çıkışları ve interrupt enable
    }
}

/// Ham formattan veri yazdırır.
pub fn trace_raw(args: fmt::Arguments) {
    use core::fmt::Write;
    // Güvenli değil ama panic sırasında kilitlenme olmamalı.
    // Her çağrıda yeni port oluşturuyoruz (stateless).
    let mut s = unsafe { SimpleSerial::new(0x3F8) };
    let _ = s.write_fmt(args);
}

impl fmt::Write for SimpleSerial {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.force_write_str(s);
        Ok(())
    }
}
