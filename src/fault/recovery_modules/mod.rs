//! # Kurtarma Modülleri
//!
//! Modüle özgü kurtarma uygulamaları.
//!
//! ## Kurtarma Mimarisi
//!
//! Bir hata (Fault) raporlandığında FaultHub, ilgili kurtarma modülünü
//! çağırarak düzeltilebilir hataları otomatik olarak gidermeye çalışır.
//!
//! ```
//! FaultHub::report(source, fault_type, message)
//!        |
//!        v
//! Kurtarma modülü seçimi (FaultSource'a göre)
//!        |
//!        +-- Memory   --> recovery_modules::memory::recover()
//!        +-- Driver   --> recovery_modules::driver::recover()
//!        +-- Fs       --> recovery_modules::fs::recover()
//!        +-- Network  --> recovery_modules::network::recover()
//!        |
//!        v
//! RecoveryResult döner
//!        |
//!        +-- Recovered  --> Hata giderildi, normal çalışmaya dön
//!        +-- Degraded   --> Kısmen kurtarıldı, performans düşük
//!        +-- Failed     --> Kurtarma başarısız, panic veya kapatma
//! ```
//!
//! ## RecoveryResult Anlamları
//!
//! ```
//! Recovered  --> İşlem başarılı, sistem tam kapasitede
//! Degraded   --> Sistem çalışıyor ama azaltılmış modda
//!               (örn: salt okunur dosya sistemi, eksik sürücü)
//! Failed     --> Kurtarma mümkün değil
//!               (örn: heap bozulması, kritik donanım hatası)
//! ```
//!
//! ## Modül Bağımlılıkları
//!
//! Her kurtarma modülü bağımsızdır; birbirini çağırmaz.
//! Bu tasarım, kurtarma sırasında zincirleme hataları önler.

pub mod driver;
pub mod fs;
pub mod memory;
pub mod network;

use core::sync::atomic::{AtomicBool, Ordering};

// init(): Kurtarma modülleri başlatma fonksiyonu.
// Şu an kurtarma modülleri global durum tutmadığından, sadece log basar.
// İleride her modülün kendi başlatma adımları olabilir (örn: watchdog kayıt).
pub fn init() {
    crate::serial_println!("[RECOVERY_MODULES] Başlatıldı");
}
