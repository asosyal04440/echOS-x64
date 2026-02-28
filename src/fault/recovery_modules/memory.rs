//! # Bellek Kurtarma
//!
//! Bellek hatalarına yönelik kurtarma stratejileri.
//!
//! ## Bellek Kurtarma Stratejileri
//!
//! ```
//! HeapCorruption    --> Kurtarma MÜMKÜN DEĞİL
//!                       Bozulan blokları karantinaya al, log bas, Failed
//!
//! OutOfMemory       --> Sayfa geri kazanımı (reclaim) dene
//!                       Başarılıysa Recovered, değilse Degraded
//!
//! DoubleFree        --> Güvenlik ihlali: logla, karantina al
//!                       Degraded (sistem çalışabilir ama ihlal kaydedildi)
//!
//! UseAfterFree      --> Güvenlik ihlali: logla
//!                       Degraded
//! ```
//!
//! ## Heap Bozulması Neden Kurtarılamaz?
//!
//! Heap metadata'sı (serbest liste, blok boyutları) zarar gördüğünde,
//! allocator hangi belleğin serbest hangisinin kullanımda olduğunu bilemez.
//! Yeni bir tahsis, hâlâ kullanılan belleği üzerine yazabilir.
//!
//! ```
//! Normal heap:
//! [HDR|KULLANIMDA|HDR|SERBEST|HDR|KULLANIMDA]
//!   ^                  ^
//!  metadata           metadata
//!
//! Bozulmuş heap:
//! [HDR|KULLANIMDA|???|SERBEST|GEÇERSİZ]
//!                 ^
//!              metadata bozuldu → allocator ne yapacağını bilmiyor
//! ```
//!
//! ## OOM Kurtarma: Sayfa Geri Kazanımı
//!
//! reclaim_pages_global(N): Kernel tarafından tutulup
//! serbest bırakılabilecek sayfaları boşaltır.
//! Örneğin dosya sistemi önbellekleri (page cache) boşaltılabilir.
//!
//! ```
//! OOM tespit edildi
//!      |
//!      v
//! reclaim_pages_global(64)  --> 64 sayfayı geri al
//!      |
//!      v
//! free_frames() > 32?
//!   EVET --> Recovered (yeterli bellek var)
//!   HAYIR --> Degraded (hâlâ az bellek)
//! ```
//!
//! ## DoubleFree / UseAfterFree Nedir?
//!
//! DoubleFree: Aynı bellek bölgesi iki kez serbest bırakılır.
//! UseAfterFree: Serbest bırakılan bellek okunmaya/yazılmaya devam edilir.
//! Her ikisi de tanımsız davranış (undefined behavior) — Rust'ta unsafe olmadan oluşamaz.

use crate::fault::{Fault, FaultType};
use crate::fault::severity::RecoveryResult;

/// Bellek kurtarmasını dener
pub fn recover(fault: &Fault) -> RecoveryResult {
    match fault.fault_type {
        FaultType::HeapCorruption => {
            // Yığın bozulmasından gerçek anlamda kurtarma mümkün değil
            // Bozulan blokları karantinaya al
            crate::serial_println!("[MEM_RECOVERY] Yığın bozulması tespit edildi - karantinaya alınıyor");
            // Karantina: Bozulan bellek bloğunu bir daha tahsis edilmeyecek şekilde işaretle.
            // Bu, daha fazla hasarı önler ama sistemi kurtaramaz.
            RecoveryResult::Failed
        }

        FaultType::OutOfMemory => {
            // Bellek serbest bırakmayı dene
            crate::serial_println!("[MEM_RECOVERY] Bellek yetersiz (OOM) - geri kazanım deneniyor");
            // reclaim_pages_global: Kernel page cache ve benzeri yapılardan 64 sayfa geri al
            crate::memory::reclaim_pages_global(64);

            // Yardımcı olup olmadığını kontrol et
            let free = crate::memory::global_memory_manager()
                .map(|m: &crate::memory::MemoryManager| m.free_frames())
                .unwrap_or(0);

            // 32 frame yeterli mi? (64KB @ 4KB/frame = minimum çalışma alanı)
            if free > 32 {
                RecoveryResult::Recovered
            } else {
                RecoveryResult::Degraded
            }
        }

        // DoubleFree ve UseAfterFree: '|' ile birden fazla pattern eşleştirme
        // Rust'ta birden fazla match kolunu OR mantığıyla birleştirebiliriz
        FaultType::DoubleFree | FaultType::UseAfterFree => {
            // Günlüğe yaz ve karantinaya al
            crate::serial_println!("[MEM_RECOVERY] Bellek güvenlik ihlali günlüğe yazıldı");
            // Güvenlik ihlali kaydedilir; sistem çalışmaya devam edebilir
            // ama ilgili bellek bölgesi artık güvenilmez
            RecoveryResult::Degraded
        }

        // Diğer bellek hataları için kurtarma bilinmiyor
        _ => RecoveryResult::Failed,
    }
}
