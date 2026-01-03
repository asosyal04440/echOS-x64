//! # echOS Debug Analyzer
//! 
//! Sistem izleme ve hata ayıklama yardımcı araçları.
//! Şu an için basit loglama fonksiyonları içerir.

/// Bir izleme (trace) mesajı kaydeder.
/// İleride bu mesajlar bir flight recorder buffer'ına yazılabilir.
pub fn trace(msg: &str) {
    use crate::serial_println;
    // Basit senkron loglama
    serial_println!("[TRACE] {}", msg);
}

/// Kolay kullanım makrosu
#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {
        $crate::debug::analyzer::trace(&format!($($arg)*));
    };
}
