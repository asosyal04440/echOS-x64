//! # echOS Preemption and Interrupt Safety Module
//!
//! Tier 1 OS seviyesinde preempt_count ve interrupt safety
//! Linux preempt_count ile aynı mantık, Rust optimizasyonları

use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use crate::memory_barriers::{smp_mb, smp_rmb, smp_wmb};

/// Per-CPU preemption counter
static mut PREEMPT_COUNT: [AtomicU32; 8192] = [const { AtomicU32::new(0) }; 8192];

/// Preemption disable flags (Linux ile uyumlu)
pub const PREEMPT_DISABLE_BITS: u32 = 1 << 0;      // PREEMPT_DISABLE
pub const PREEMPT_NEED_RESCHED: u32 = 1 << 1;     // NEED_RESCHED
pub const PREEMPT_HARDIRQ: u32 = 1 << 2;          // HARDIRQ
pub const PREEMPT_SOFTIRQ: u32 = 1 << 3;          // SOFTIRQ
pub const PREEMPT_NMI: u32 = 1 << 4;              // NMI
pub const PREEMPT_COUNT_OFFSET: u32 = 1 << 5;    // COUNT_OFFSET

/// Interrupt context levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptContext {
    None,
    HardIRQ,
    SoftIRQ,
    NMI,
}

/// Preemption context guard
pub struct PreemptDisableGuard {
    cpu_id: u32,
    old_count: u32,
}

impl PreemptDisableGuard {
    /// Disable preemption
    pub fn new() -> Self {
        let cpu_id = crate::cpu::smp::current_cpu_id();
        let old_count = preempt_count_inc(cpu_id, PREEMPT_DISABLE_BITS);
        
        Self { cpu_id, old_count }
    }
    
    /// Check if preemption is disabled
    pub fn is_disabled(&self) -> bool {
        let current_count = get_preempt_count(self.cpu_id);
        (current_count & PREEMPT_DISABLE_BITS) != 0
    }
}

impl Drop for PreemptDisableGuard {
    fn drop(&mut self) {
        preempt_count_dec(self.cpu_id, PREEMPT_DISABLE_BITS);
    }
}

/// HardIRQ context guard
pub struct HardIRQGuard {
    cpu_id: u32,
    old_count: u32,
}

impl HardIRQGuard {
    /// Enter HardIRQ context
    pub fn new() -> Self {
        let cpu_id = crate::cpu::smp::current_cpu_id();
        let old_count = preempt_count_inc(cpu_id, PREEMPT_HARDIRQ);
        
        // Memory barrier to ensure ordering
        smp_mb();
        
        Self { cpu_id, old_count }
    }
    
    /// Get current context level
    pub fn context_level(&self) -> InterruptContext {
        InterruptContext::HardIRQ
    }
}

impl Drop for HardIRQGuard {
    fn drop(&mut self) {
        preempt_count_dec(self.cpu_id, PREEMPT_HARDIRQ);
        smp_mb();
    }
}

/// SoftIRQ context guard
pub struct SoftIRQGuard {
    cpu_id: u32,
    old_count: u32,
}

impl SoftIRQGuard {
    /// Enter SoftIRQ context
    pub fn new() -> Self {
        let cpu_id = crate::cpu::smp::current_cpu_id();
        let old_count = preempt_count_inc(cpu_id, PREEMPT_SOFTIRQ);
        
        smp_mb();
        
        Self { cpu_id, old_count }
    }
    
    /// Get current context level
    pub fn context_level(&self) -> InterruptContext {
        InterruptContext::SoftIRQ
    }
}

impl Drop for SoftIRQGuard {
    fn drop(&mut self) {
        preempt_count_dec(self.cpu_id, PREEMPT_SOFTIRQ);
        smp_mb();
    }
}

/// NMI context guard
pub struct NMIGuard {
    cpu_id: u32,
    old_count: u32,
}

impl NMIGuard {
    /// Enter NMI context
    pub fn new() -> Self {
        let cpu_id = crate::cpu::smp::current_cpu_id();
        let old_count = preempt_count_inc(cpu_id, PREEMPT_NMI);
        
        smp_mb();
        
        Self { cpu_id, old_count }
    }
    
    /// Get current context level
    pub fn context_level(&self) -> InterruptContext {
        InterruptContext::NMI
    }
}

impl Drop for NMIGuard {
    fn drop(&mut self) {
        preempt_count_dec(self.cpu_id, PREEMPT_NMI);
        smp_mb();
    }
}

/// Get current preemption count for CPU
pub fn get_preempt_count(cpu_id: u32) -> u32 {
    unsafe {
        PREEMPT_COUNT[cpu_id as usize].load(Ordering::Relaxed)
    }
}

/// Increment preemption count
pub fn preempt_count_inc(cpu_id: u32, bits: u32) -> u32 {
    unsafe {
        let old_count = PREEMPT_COUNT[cpu_id as usize].fetch_add(bits, Ordering::Relaxed);
        smp_wmb();
        old_count
    }
}

/// Decrement preemption count
pub fn preempt_count_dec(cpu_id: u32, bits: u32) -> u32 {
    unsafe {
        let old_count = PREEMPT_COUNT[cpu_id as usize].fetch_sub(bits, Ordering::Relaxed);
        smp_wmb();
        old_count
    }
}

/// Check if preemption is enabled
pub fn preempt_enabled() -> bool {
    let cpu_id = crate::cpu::smp::current_cpu_id();
    let count = get_preempt_count(cpu_id);
    (count & PREEMPT_DISABLE_BITS) == 0
}

/// Check if we're in interrupt context
pub fn in_interrupt() -> bool {
    let cpu_id = crate::cpu::smp::current_cpu_id();
    let count = get_preempt_count(cpu_id);
    (count & (PREEMPT_HARDIRQ | PREEMPT_SOFTIRQ | PREEMPT_NMI)) != 0
}

/// Get current interrupt context level
pub fn get_interrupt_context() -> InterruptContext {
    let cpu_id = crate::cpu::smp::current_cpu_id();
    let count = get_preempt_count(cpu_id);
    
    if (count & PREEMPT_NMI) != 0 {
        InterruptContext::NMI
    } else if (count & PREEMPT_HARDIRQ) != 0 {
        InterruptContext::HardIRQ
    } else if (count & PREEMPT_SOFTIRQ) != 0 {
        InterruptContext::SoftIRQ
    } else {
        InterruptContext::None
    }
}

/// Check if we're in NMI context
pub fn in_nmi() -> bool {
    let cpu_id = crate::cpu::smp::current_cpu_id();
    let count = get_preempt_count(cpu_id);
    (count & PREEMPT_NMI) != 0
}

/// Check if we're in HardIRQ context
pub fn in_hardirq() -> bool {
    let cpu_id = crate::cpu::smp::current_cpu_id();
    let count = get_preempt_count(cpu_id);
    (count & PREEMPT_HARDIRQ) != 0
}

/// Check if we're in SoftIRQ context
pub fn in_softirq() -> bool {
    let cpu_id = crate::cpu::smp::current_cpu_id();
    let count = get_preempt_count(cpu_id);
    (count & PREEMPT_SOFTIRQ) != 0
}

/// Set need reschedule flag
pub fn set_need_resched() {
    let cpu_id = crate::cpu::smp::current_cpu_id();
    unsafe {
        PREEMPT_COUNT[cpu_id as usize].fetch_or(PREEMPT_NEED_RESCHED, Ordering::Relaxed);
    }
    smp_mb();
}

/// Check if reschedule is needed
pub fn need_resched() -> bool {
    let cpu_id = crate::cpu::smp::current_cpu_id();
    let count = get_preempt_count(cpu_id);
    (count & PREEMPT_NEED_RESCHED) != 0
}

/// Clear need reschedule flag
pub fn clear_need_resched() {
    let cpu_id = crate::cpu::smp::current_cpu_id();
    unsafe {
        PREEMPT_COUNT[cpu_id as usize].fetch_and(!PREEMPT_NEED_RESCHED, Ordering::Relaxed);
    }
    smp_mb();
}

/// Check if it's safe to preempt
pub fn preemptible() -> bool {
    !in_interrupt() && preempt_enabled()
}

/// Check if it's safe to schedule
pub fn schedulable() -> bool {
    !in_nmi() && !in_hardirq()
}

/// Preemption statistics
#[derive(Debug, Clone, Copy)]
pub struct PreemptStats {
    pub cpu_id: u32,
    pub preempt_count: u32,
    pub preempt_disabled: bool,
    pub in_interrupt: bool,
    pub interrupt_context: InterruptContext,
    pub need_resched: bool,
}

impl PreemptStats {
    pub fn current() -> Self {
        let cpu_id = crate::cpu::smp::current_cpu_id();
        let count = get_preempt_count(cpu_id);
        
        Self {
            cpu_id,
            preempt_count: count,
            preempt_disabled: (count & PREEMPT_DISABLE_BITS) != 0,
            in_interrupt: (count & (PREEMPT_HARDIRQ | PREEMPT_SOFTIRQ | PREEMPT_NMI)) != 0,
            interrupt_context: get_interrupt_context(),
            need_resched: (count & PREEMPT_NEED_RESCHED) != 0,
        }
    }
    
    pub fn for_cpu(cpu_id: u32) -> Self {
        let count = get_preempt_count(cpu_id);
        
        Self {
            cpu_id,
            preempt_count: count,
            preempt_disabled: (count & PREEMPT_DISABLE_BITS) != 0,
            in_interrupt: (count & (PREEMPT_HARDIRQ | PREEMPT_SOFTIRQ | PREEMPT_NMI)) != 0,
            interrupt_context: if (count & PREEMPT_NMI) != 0 {
                InterruptContext::NMI
            } else if (count & PREEMPT_HARDIRQ) != 0 {
                InterruptContext::HardIRQ
            } else if (count & PREEMPT_SOFTIRQ) != 0 {
                InterruptContext::SoftIRQ
            } else {
                InterruptContext::None
            },
            need_resched: (count & PREEMPT_NEED_RESCHED) != 0,
        }
    }
}

/// Initialize preemption subsystem
pub fn init() {
    crate::serial_println!("Preempt: Initializing preemption subsystem");
    
    let cpu_count = crate::cpu::smp::get_cpu_count();
    for cpu_id in 0..cpu_count {
        unsafe {
            PREEMPT_COUNT[cpu_id as usize].store(0, Ordering::Relaxed);
        }
    }
    
    crate::serial_println!("Preempt: Initialized for {} CPUs", cpu_count);
}

/// Preemption debug utilities
pub mod debug {
    use super::*;
    
    /// Print current preemption state
    pub fn print_preempt_state() {
        let stats = PreemptStats::current();
        crate::serial_println!("Preempt State:");
        crate::serial_println!("  CPU: {}", stats.cpu_id);
        crate::serial_println!("  Count: 0x{:x}", stats.preempt_count);
        crate::serial_println!("  Disabled: {}", stats.preempt_disabled);
        crate::serial_println!("  In Interrupt: {}", stats.in_interrupt);
        crate::serial_println!("  Context: {:?}", stats.interrupt_context);
        crate::serial_println!("  Need Resched: {}", stats.need_resched);
    }
    
    /// Print all CPU preemption states
    pub fn print_all_cpu_states() {
        let cpu_count = crate::cpu::smp::get_cpu_count();
        
        crate::serial_println!("=== All CPU Preempt States ===");
        for cpu_id in 0..cpu_count {
            let stats = PreemptStats::for_cpu(cpu_id);
            crate::serial_println!("CPU {}: count=0x{:x}, disabled={}, interrupt={:?}, need_resched={}", 
                cpu_id, stats.preempt_count, stats.preempt_disabled, 
                stats.interrupt_context, stats.need_resched);
        }
        crate::serial_println!("=== End CPU States ===");
    }
    
    /// Validate preemption consistency
    pub fn validate_preempt_state() -> bool {
        let cpu_count = crate::cpu::smp::get_cpu_count();
        let mut valid = true;
        
        for cpu_id in 0..cpu_count {
            let stats = PreemptStats::for_cpu(cpu_id);
            
            // Check for invalid state combinations
            if stats.in_interrupt && stats.preempt_disabled {
                crate::serial_println!("Preempt Warning: CPU {} has both interrupt and disabled", cpu_id);
                valid = false;
            }
            
            if stats.interrupt_context == InterruptContext::None && stats.in_interrupt {
                crate::serial_println!("Preempt Error: CPU {} inconsistent interrupt state", cpu_id);
                valid = false;
            }
        }
        
        valid
    }
}

/// Preemption-safe sleep
pub fn preemptible_sleep(ticks: usize) {
    if preemptible() {
        crate::task::scheduler::sleep(ticks);
    } else {
        // Can't sleep, just spin
        for _ in 0..ticks {
            core::hint::spin_loop();
        }
    }
}

/// Preemption-safe schedule
pub fn preemptible_schedule() {
    if schedulable() {
        crate::task::scheduler::schedule();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_preempt_disable() {
        let _guard = PreemptDisableGuard::new();
        assert!(!preempt_enabled());
    }
    
    #[test]
    fn test_interrupt_context() {
        let _guard = HardIRQGuard::new();
        assert_eq!(get_interrupt_context(), InterruptContext::HardIRQ);
        assert!(in_interrupt());
        assert!(in_hardirq());
    }
    
    #[test]
    fn test_need_resched() {
        set_need_resched();
        assert!(need_resched());
        clear_need_resched();
        assert!(!need_resched());
    }
}
