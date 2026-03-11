//! # Sürücü Kurtarma
//!
//! Sürücü hatalarına yönelik kurtarma stratejileri.
//!
//! ## Sürücü Kurtarma Stratejileri
//!
//! ```
//! DeviceTimeout  --> Sürücüyü sıfırla, isteği yeniden gönder
//!                    Başarısızsa: Cihazı devre dışı bırak (Degraded)
//!
//! DeviceError    --> Hata kodunu logla, cihazı devre dışı bırak
//!                    Stub/null driver ile devam et (Degraded)
//!
//! DmaCorruption  --> Bellek bütünlüğü tehlikede, DMA durdur
//!                    Kurtarma mümkün değil (Failed)
//!
//! DriverCrash    --> Sürücüyü yeniden yükle/başlat
//!                    Başarısıza degrade modda devam et
//! ```
//!
//! ## Sürücü Kurtarma Katmanları
//!
//! ```
//! Katman 1: Yazılımsal sıfırlama (driver reset)
//!   Sürücünün iç durumunu başlangıç değerlerine geri döndürür.
//!   Donanıma RESET komutu gönderir.
//!
//! Katman 2: Yeniden başlatma (restart)
//!   Sürücüyü tamamen kapatıp yeniden açar.
//!   Tüm bekleyen I/O iptal edilir.
//!
//! Katman 3: Degrade mod
//!   Sürücü devre dışı bırakılır, stub (boş) impl devreye girer.
//!   Sistem çalışmaya devam eder ama o donanım kullanılamaz.
//! ```
//!
//! ## match Deyimi Kullanımı
//!
//! Rust'ta match, C'deki switch'in çok daha güçlü versiyonudur.
//! Tüm enum kolları kapsanmalıdır (exhaustive matching).
//! `_` joker ifadesi, ele alınmayan tüm durumları yakalar.

use crate::fault::severity::RecoveryResult;
use crate::fault::{Fault, FaultType};

/// Sürücü kurtarmasını dener
pub fn recover(fault: &Fault) -> RecoveryResult {
    // fault.fault_type enum değerine göre strateji seç
    match fault.fault_type {
        FaultType::DeviceTimeout => {
            crate::serial_println!("[DRV_RECOVERY] Cihaz zaman aşımı - sürücü sıfırlanacak");
            // Gerçek uygulamada sürücü sıfırlama çağrılır
            // Şimdilik: Degraded döndür — cihaz yeniden denemeyi bekliyor
            RecoveryResult::Degraded
        }

        FaultType::DeviceError => {
            crate::serial_println!("[DRV_RECOVERY] Cihaz hatası - cihaz devre dışı bırakılıyor");
            // Hatalı cihazı devre dışı bırak, sistem devam edebilir
            RecoveryResult::Degraded
        }

        FaultType::DmaCorruption => {
            crate::serial_println!("[DRV_RECOVERY] DMA bozulması - kritik hata");
            // DMA bozulması bellek bütünlüğünü tehdit eder — kurtarma yok
            RecoveryResult::Failed
        }

        FaultType::DriverCrash => {
            crate::serial_println!("[DRV_RECOVERY] Sürücü çöktü - yeniden başlatılacak");
            // Sürücü yeniden başlatılabilir; aygıt tekrar kullanılabilir olabilir
            RecoveryResult::Degraded
        }

        // Bilinmeyen hata türleri için varsayılan: kurtarma dene, başarısız olursa Failed
        _ => RecoveryResult::Failed,
    }
}

/// Belirli bir sürücüyü sıfırlar
pub fn reset_driver(name: &str) -> bool {
    crate::serial_println!("[DRV_RECOVERY] Sürücü sıfırlanıyor: {}", name);

    // Sürücü adına göre ilgili sıfırlama fonksiyonunu çağır.
    // Her sürücünün sıfırlama prosedürü farklıdır (donanım spesifik).
    match name {
        "virtio-net" => {
            // virtio_net sıfırlama çağrılacak
            false
        }
        "nvme" => {
            // nvme sıfırlama çağrılacak
            false
        }
        // Tanınmayan sürücü → sıfırlanamaz
        _ => false,
    }
}
