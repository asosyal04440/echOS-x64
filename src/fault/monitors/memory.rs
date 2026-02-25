//! # Memory Health Monitor
//!
//! Monitors heap integrity, OOM conditions, and memory corruption.

use core::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, AtomicBool, Ordering};

use crate::fault::{Fault, FaultSource, FaultType, HealthStatus, ModuleHealth};

// ============================================================================
// MEMORY MONITOR STATE
// ============================================================================

/// Memory monitor state
pub struct MemoryMonitor {
    /// Heap corruption count
    corruption_count: AtomicU32,
    /// OOM events
    oom_events: AtomicU32,
    /// Page fault count
    page_faults: AtomicU64,
    /// Last check timestamp
    pub last_check_tick: AtomicUsize,
    /// Monitor enabled
    enabled: AtomicBool,
    /// Warning threshold for corruption
    corruption_warning_threshold: u32,
    /// Critical threshold for corruption
    corruption_critical_threshold: u32,
}

impl MemoryMonitor {
    pub const fn new() -> Self {
        Self {
            corruption_count: AtomicU32::new(0),
            oom_events: AtomicU32::new(0),
            page_faults: AtomicU64::new(0),
            last_check_tick: AtomicUsize::new(0),
            enabled: AtomicBool::new(true),
            corruption_warning_threshold: 1,
            corruption_critical_threshold: 3,
        }
    }
    
    /// Record a heap corruption event
    pub fn record_corruption(&self) {
        self.corruption_count.fetch_add(1, Ordering::SeqCst);
    }
    
    /// Record an OOM event
    pub fn record_oom(&self) {
        self.oom_events.fetch_add(1, Ordering::SeqCst);
    }
    
    /// Record a page fault
    pub fn record_page_fault(&self) {
        self.page_faults.fetch_add(1, Ordering::SeqCst);
    }
    
    /// Check heap integrity
    fn check_heap(&self) -> Option<Fault> {
        // Check TLSF integrity
        let corruption = crate::allocator::check_heap_integrity();
        
        if corruption > 0 {
            self.record_corruption();
            return Some(Fault::new(
                FaultSource::Memory,
                FaultType::HeapCorruption,
                "Heap integrity check failed"
            ));
        }
        
        None
    }
    
    /// Check memory pressure
    fn check_memory_pressure(&self) -> Option<Fault> {
        // Get memory stats
        let free = crate::memory::global_memory_manager()
            .map(|m: &crate::memory::MemoryManager| m.free_frames())
            .unwrap_or(0);
        let total = crate::memory::global_memory_manager()
            .map(|m: &crate::memory::MemoryManager| m.total_frames())
            .unwrap_or(1);
        
        let free_percent = (free * 100) / total;
        
        if free_percent < 5 {
            return Some(Fault::new(
                FaultSource::Memory,
                FaultType::OutOfMemory,
                &alloc::format!("Critical memory pressure: {}% free", free_percent)
            ));
        } else if free_percent < 15 {
            return Some(Fault::new(
                FaultSource::Memory,
                FaultType::OutOfMemory,
                &alloc::format!("Low memory: {}% free", free_percent)
            ));
        }
        
        None
    }
    
    /// Check allocation patterns
    fn check_allocations(&self) -> Option<Fault> {
        // Check for allocation anomalies
        let stats = crate::allocator::get_alloc_stats();
        
        // Check for suspicious allocation count
        if stats.active_allocations > 10000 {
            return Some(Fault::new(
                FaultSource::Memory,
                FaultType::TaskLeak, // Reusing for allocation leak
                &alloc::format!("High allocation count: {}", stats.active_allocations)
            ));
        }
        
        None
    }
}

impl super::HealthMonitor for MemoryMonitor {
    fn name(&self) -> &'static str {
        "memory"
    }
    
    fn check(&self) -> Option<Fault> {
        if !self.enabled.load(Ordering::SeqCst) {
            return None;
        }
        
        self.last_check_tick.store(
            crate::task::scheduler::get_ticks(),
            Ordering::SeqCst
        );
        
        // Check heap integrity
        if let Some(fault) = self.check_heap() {
            return Some(fault);
        }
        
        // Check memory pressure
        if let Some(fault) = self.check_memory_pressure() {
            return Some(fault);
        }
        
        // Check allocations
        if let Some(fault) = self.check_allocations() {
            return Some(fault);
        }
        
        None
    }
    
    fn health(&self) -> HealthStatus {
        let corruption = self.corruption_count.load(Ordering::SeqCst);
        
        if corruption >= self.corruption_critical_threshold {
            HealthStatus::Failed
        } else if corruption >= self.corruption_warning_threshold {
            HealthStatus::Warning
        } else if self.oom_events.load(Ordering::SeqCst) > 0 {
            HealthStatus::Warning
        } else {
            HealthStatus::Healthy
        }
    }
    
    fn module_health(&self) -> ModuleHealth {
        ModuleHealth {
            name: self.name(),
            status: self.health(),
            fault_count: self.corruption_count.load(Ordering::SeqCst) + self.oom_events.load(Ordering::SeqCst) as u32,
            recovery_count: 0,
            last_fault_tick: self.last_check_tick.load(Ordering::SeqCst),
            uptime_ticks: crate::task::scheduler::get_ticks(),
            is_critical: true,
            can_restart: false,
            has_fallback: false,
        }
    }
    
    fn reset(&self) {
        self.corruption_count.store(0, Ordering::SeqCst);
        self.oom_events.store(0, Ordering::SeqCst);
        self.page_faults.store(0, Ordering::SeqCst);
    }
}

// ============================================================================
// GLOBAL INSTANCE
// ============================================================================

pub static MEMORY_MONITOR: MemoryMonitor = MemoryMonitor::new();
