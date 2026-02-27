//! # Zamanlayıcı Sağlık Monitörü
//!
//! Görev zamanlayıcısı sağlığını, çalıştırma kuyruğunu ve görev sızıntılarını izler.
//! Zombie görev birikimi ve çalıştırılabilir görev taşmasini tespit eder.

use core::sync::atomic::{AtomicU32, AtomicUsize, AtomicBool, Ordering};

use crate::fault::{Fault, FaultSource, FaultType, HealthStatus, ModuleHealth};

pub struct SchedulerMonitor {
    /// Görev sızıntısı sayısı
    task_leaks: AtomicU32,
    /// Açlık (starvation) olay sayısı
    starvation_events: AtomicU32,
    /// Çalıştırma kuyruğu anomali sayısı
    queue_anomalies: AtomicU32,
    /// Son kontrol zaman damgası
    last_check: AtomicUsize,
    /// Monitör etkin mi?
    enabled: AtomicBool,
}

impl SchedulerMonitor {
    pub const fn new() -> Self {
        Self {
            task_leaks: AtomicU32::new(0),
            starvation_events: AtomicU32::new(0),
            queue_anomalies: AtomicU32::new(0),
            last_check: AtomicUsize::new(0),
            enabled: AtomicBool::new(true),
        }
    }
    
    /// Görev sızıntısı kaydeder
    pub fn record_task_leak(&self) {
        self.task_leaks.fetch_add(1, Ordering::SeqCst);
    }
    
    /// Açlık (starvation) olayı kaydeder
    pub fn record_starvation(&self) {
        self.starvation_events.fetch_add(1, Ordering::SeqCst);
    }
    
    /// Zamanlayıcı sağlığını kontrol eder — zombie birikimi ve yüksek kuyruk
    fn check_scheduler(&self) -> Option<Fault> {
        let stats = crate::task::scheduler::get_stats();
        
        // Zombie birikimsini kontrol et
        if stats.zombie_count > 50 {
            self.record_task_leak();
            return Some(Fault::new(
                FaultSource::Scheduler,
                FaultType::TaskLeak,
                &alloc::format!("High zombie task count: {}", stats.zombie_count)
            ));
        }
        
        // Çalıştırma kuyruğu sorunlarını kontrol et
        if stats.runnable_tasks > 1000 {
            return Some(Fault::new(
                FaultSource::Scheduler,
                FaultType::Starvation,
                &alloc::format!("High runnable task count: {}", stats.runnable_tasks)
            ));
        }
        
        None
    }
}

impl super::HealthMonitor for SchedulerMonitor {
    fn name(&self) -> &'static str {
        "scheduler"
    }
    
    fn check(&self) -> Option<Fault> {
        if !self.enabled.load(Ordering::SeqCst) {
            return None;
        }
        
        self.last_check.store(
            crate::task::scheduler::get_ticks(),
            Ordering::SeqCst
        );
        
        self.check_scheduler()
    }
    
    fn health(&self) -> HealthStatus {
        let leaks = self.task_leaks.load(Ordering::SeqCst);
        let starvation = self.starvation_events.load(Ordering::SeqCst);
        
        if leaks > 10 || starvation > 5 {
            HealthStatus::Degraded
        } else if leaks > 0 || starvation > 0 {
            HealthStatus::Warning
        } else {
            HealthStatus::Healthy
        }
    }
    
    fn module_health(&self) -> ModuleHealth {
        ModuleHealth {
            name: self.name(),
            status: self.health(),
            fault_count: self.task_leaks.load(Ordering::SeqCst) + self.starvation_events.load(Ordering::SeqCst),
            recovery_count: 0,
            last_fault_tick: self.last_check.load(Ordering::SeqCst),
            uptime_ticks: crate::task::scheduler::get_ticks(),
            is_critical: true,
            can_restart: false,
            has_fallback: false,
        }
    }
    
    fn reset(&self) {
        self.task_leaks.store(0, Ordering::SeqCst);
        self.starvation_events.store(0, Ordering::SeqCst);
        self.queue_anomalies.store(0, Ordering::SeqCst);
    }
}

pub static SCHEDULER_MONITOR: SchedulerMonitor = SchedulerMonitor::new();
