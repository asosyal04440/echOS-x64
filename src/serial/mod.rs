//! # echOS Serial Port Modülü
//! 
//! Debug çıktısı için serial port (COM1) desteği.
//! `serial_print!` ve `serial_println!` makroları için altyapı.

/// UART sürücüsü ve makrolar
pub mod uart;

/// Serial port'u başlatır.
pub fn init() {
    uart::init();
}
