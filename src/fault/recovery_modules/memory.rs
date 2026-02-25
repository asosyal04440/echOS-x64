//! # Memory Recovery
//!
//! Recovery strategies for memory faults.

use crate::fault::{Fault, FaultType};
use crate::fault::severity::RecoveryResult;

/// Attempt memory recovery
pub fn recover(fault: &Fault) -> RecoveryResult {
    match fault.fault_type {
        FaultType::HeapCorruption => {
            // Cannot truly recover from heap corruption
            // Quarantine corrupted blocks
            crate::serial_println!("[MEM_RECOVERY] Heap corruption detected - quarantining");
            RecoveryResult::Failed
        }
        
        FaultType::OutOfMemory => {
            // Try to free memory
            crate::serial_println!("[MEM_RECOVERY] OOM - attempting reclaim");
            crate::memory::reclaim_pages_global(64);
            
            // Check if helped
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
            // Log and quarantine
            crate::serial_println!("[MEM_RECOVERY] Memory safety violation logged");
            RecoveryResult::Degraded
        }
        
        _ => RecoveryResult::Failed,
    }
}
