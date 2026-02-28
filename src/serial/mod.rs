//! # echOS Serial Port Modülü
//!
//! Debug çıktısı için serial port (COM1) desteği.
//! `serial_print!` ve `serial_println!` makroları için altyapı.
//!
//! ## Serial Port (RS-232) Nedir?
//!
//! Seri port, bilgisayarın harici haberleşme portlarından biridir.
//! Veri bitlerini tek bir hat üzerinden sırayla (seriali olarak) gönderir.
//! QEMU ve gerçek donanımda hata ayıklama için hâlâ yaygın kullanılmaktadır.
//!
//! ## echOS'ta Serial Port Kullanımı
//!
//! ```
//!  Kernel kodu    serial_println!("...")  -->  UART sürücüsü (COM1)  -->  0x3F8
//!                                                       |
//!                                              QEMU -serial stdio
//!                                                       |
//!                                              Host terminali ekranı
//! ```
//!
//! QEMU başlatılırken `-serial stdio` parametresi verildiğinde,
//! tüm serial çıktı doğrudan host terminal ekranına gider (debug amacıyla).
//! Bu sayede kernel panikleri, boot mesajları ve debug logları görüntülenebilir.

/// UART sürücüsü ve makrolar.
/// 16550 UART donanımını yöneten düşük seviyeli kod burada bulunur.
pub mod uart;

/// Serial port'u başlatır.
///
/// UART register'larını yapılandırır:
/// - Baud hızı: 38400 bps
/// - Veri bitleri: 8
/// - Parity: Yok (None)
/// - Stop bitleri: 1 (8N1 konfigürasyonu)
///
/// Detaylar için `uart::init()` fonksiyonuna bakınız.
pub fn init() {
    uart::init();
}
