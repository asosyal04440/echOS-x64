//! # Filesystem Health Monitor
//!
//! Monitors filesystem integrity, I/O errors, and disk space.

use core::sync::atomic::{AtomicU32, AtomicUsize, AtomicBool, Ordering};

use crate::fault::{Fault, FaultSource, FaultType, HealthStatus, ModuleHealth};

pub struct FsMonitor {
    /// I/O errors
    io_errors: AtomicU32,
    /// Metadata errors
    metadata_errors: AtomicU32,
    /// Disk full events
    disk_full_events: AtomicU32,
    /// Last check timestamp
    last_check: AtomicUsize,
    /// Monitor enabled
    enabled: AtomicBool,
}

impl FsMonitor {
    pub const fn new() -> Self {
        Self {
            io_errors: AtomicU32::new(0),
            metadata_errors: AtomicU32::new(0),
            disk_full_events: AtomicU32::new(0),
            last_check: AtomicUsize::new(0),
            enabled: AtomicBool::new(true),
        }
    }
    
    /// Record I/O error
    pub fn record_io_error(&self) {
        self.io_errors.fetch_add(1, Ordering::SeqCst);
    }
    
    /// Record metadata error
    pub fn record_metadata_error(&self) {
        self.metadata_errors.fetch_add(1, Ordering::SeqCst);
    }
    
    /// Record disk full
    pub fn record_disk_full(&self) {
        self.disk_full_events.fetch_add(1, Ordering::SeqCst);
    }
    
    /// Check filesystem health
    fn check_fs(&self) -> Option<Fault> {
        let io = self.io_errors.load(Ordering::SeqCst);
        let meta = self.metadata_errors.load(Ordering::SeqCst);
        
        if meta > 0 {
            return Some(Fault::new(
                FaultSource::Filesystem,
                FaultType::MetadataCorruption,
                &alloc::format!("Metadata errors detected: {}", meta)
            ));
        }
        
        if io > 10 {
            return Some(Fault::new(
                FaultSource::Filesystem,
                FaultType::IoError,
                &alloc::format!("High I/O error count: {}", io)
            ));
        }
        
        None
    }
}

impl super::HealthMonitor for FsMonitor {
    fn name(&self) -> &'static str {
        "filesystem"
    }
    
    fn check(&self) -> Option<Fault> {
        if !self.enabled.load(Ordering::SeqCst) {
            return None;
        }
        
        self.last_check.store(
            crate::task::scheduler::get_ticks(),
            Ordering::SeqCst
        );
        
        self.check_fs()
    }
    
    fn health(&self) -> HealthStatus {
        let meta = self.metadata_errors.load(Ordering::SeqCst);
        let io = self.io_errors.load(Ordering::SeqCst);
        
        if meta > 2 {
            HealthStatus::Failed
        } else if meta > 0 || io > 20 {
            HealthStatus::Degraded
        } else if io > 5 {
            HealthStatus::Warning
        } else {
            HealthStatus::Healthy
        }
    }
    
    fn module_health(&self) -> ModuleHealth {
        ModuleHealth {
            name: self.name(),
            status: self.health(),
            fault_count: self.io_errors.load(Ordering::SeqCst) + self.metadata_errors.load(Ordering::SeqCst),
            recovery_count: 0,
            last_fault_tick: self.last_check.load(Ordering::SeqCst),
            uptime_ticks: crate::task::scheduler::get_ticks(),
            is_critical: false,
            can_restart: true,
            has_fallback: true,
        }
    }
    
    fn reset(&self) {
        self.io_errors.store(0, Ordering::SeqCst);
        self.metadata_errors.store(0, Ordering::SeqCst);
        self.disk_full_events.store(0, Ordering::SeqCst);
    }
}

pub static FS_MONITOR: FsMonitor = FsMonitor::new();
