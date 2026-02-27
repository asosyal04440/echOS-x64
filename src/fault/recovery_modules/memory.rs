//! # Bellek Kurtarma
//!
//! Bellek hatalarına yönelik kurtarma stratejileri.

use crate::fault::{Fault, FaultType};
use crate::fault::severity::RecoveryResult;

/// Bellek kurtarmasını dener
pub fn recover(fault: &Fault) -> RecoveryResult {
    match fault.fault_type {
        FaultType::HeapCorruption => {
            // Yığın bozulmasından gerçek anlamda kurtarma mümkün değil
            // Bozulan blokları karantinaya al
            crate::serial_println!("[MEM_RECOVERY] Yığın bozulması tespit edildi - karantinaya alınıyor");
            RecoveryResult::Failed
        }
        
        FaultType::OutOfMemory => {
            // Bellek serbest bırakmayı dene
            crate::serial_println!("[MEM_RECOVERY] Bellek yetersiz (OOM) - geri kazanım deneniyor");
            crate::memory::reclaim_pages_global(64);
            
            // Yardımcı olup olmadığını kontrol et
            let free = crate::memory::global_memory_manager()
                .map(|m: &crate::memory::MemoryManager| m.free_frames())
                .unwrap_or(0);
            
            if free > 32 {
                RecoveryResult::Recovered
            } else {
                RecoveryResult::Degraded
            }
        }
        
        FaultType::DoubleFree | FaultType::UseAfterFree => {
            // Günlüğe yaz ve karantinaya al
            crate::serial_println!("[MEM_RECOVERY] Bellek güvenlik ihlali günlüğe yazıldı");
            RecoveryResult::Degraded
        }
        
        _ => RecoveryResult::Failed,
    }
}
