//! # echOS CPU Affinity and Scheduling Integration Module
//!
//! Tier 1 OS seviyesinde CPU affinity ve scheduling entegrasyonu
//! Linux CPU affinity ile aynı seviyede yetenekler

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use alloc::vec::Vec;
use alloc::boxed::Box;
use alloc::collections::BTreeSet;
use crate::memory_barriers::{smp_mb, smp_rmb, smp_wmb};
use crate::preempt::{PreemptDisableGuard, preempt_enabled};
use crate::rcu::{RcuPtr, synchronize_rcu};
use crate::topology::{get_system_topology, get_cache_sharing_cpus, get_package_cpus, get_core_cpus};

/// CPU affinity mask type
pub type CpuMask = u64; // Support up to 64 CPUs, can be extended

/// CPU affinity policies
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum AffinityPolicy {
    /// No affinity restriction (can run on any CPU)
    Any = 0,
    /// Must run on specific CPUs
    Fixed = 1,
    /// Prefer specific CPUs but can run elsewhere
    Preferred = 2,
    /// Avoid specific CPUs
    Avoid = 3,
    /// NUMA-aware affinity
    Numa = 4,
    /// Cache-aware affinity
    Cache = 5,
    /// Package-aware affinity
    Package = 6,
}

/// Task affinity descriptor
#[repr(C, align(64))]
pub struct TaskAffinity {
    /// Task ID
    pub task_id: u64,
    /// Current affinity policy
    pub policy: AtomicU32, // AffinityPolicy as u32
    /// CPU affinity mask (bitmask of allowed CPUs)
    pub cpu_mask: AtomicU64,
    /// Preferred CPU mask (for Preferred policy)
    pub preferred_mask: AtomicU64,
    /// Avoid CPU mask (for Avoid policy)
    pub avoid_mask: AtomicU64,
    /// NUMA node preference
    pub numa_node: AtomicU32,
    /// Cache level preference
    pub cache_level: AtomicU32,
    /// Package preference
    pub package_id: AtomicU32,
    /// Last CPU this task ran on
    pub last_cpu: AtomicU32,
    /// Migration count
    pub migrations: AtomicU64,
    /// Affinity changes count
    pub affinity_changes: AtomicU64,
    /// Load balancing enabled
    pub load_balance: AtomicBool,
    /// Sticky affinity (prefer last CPU)
    pub sticky: AtomicBool,
    /// Padding to avoid false sharing
    _padding: [u8; 64 - 56],
}

impl TaskAffinity {
    /// Create new task affinity
    pub fn new(task_id: u64) -> Self {
        Self {
            task_id,
            policy: AtomicU32::new(AffinityPolicy::Any as u32),
            cpu_mask: AtomicU64::new(!0u64), // All CPUs allowed
            preferred_mask: AtomicU64::new(0),
            avoid_mask: AtomicU64::new(0),
            numa_node: AtomicU32::new(0),
            cache_level: AtomicU32::new(0),
            package_id: AtomicU32::new(0),
            last_cpu: AtomicU32::new(0),
            migrations: AtomicU64::new(0),
            affinity_changes: AtomicU64::new(0),
            load_balance: AtomicBool::new(true),
            sticky: AtomicBool::new(true),
            _padding: [0; 64 - 56],
        }
    }
    
    /// Get current affinity policy
    pub fn get_policy(&self) -> AffinityPolicy {
        match self.policy.load(Ordering::Acquire) {
            0 => AffinityPolicy::Any,
            1 => AffinityPolicy::Fixed,
            2 => AffinityPolicy::Preferred,
            3 => AffinityPolicy::Avoid,
            4 => AffinityPolicy::Numa,
            5 => AffinityPolicy::Cache,
            6 => AffinityPolicy::Package,
            _ => AffinityPolicy::Any,
        }
    }
    
    /// Set affinity policy
    pub fn set_policy(&self, policy: AffinityPolicy) {
        self.policy.store(policy as u32, Ordering::Release);
        self.affinity_changes.fetch_add(1, Ordering::Relaxed);
        smp_wmb();
    }
    
    /// Get CPU mask
    pub fn get_cpu_mask(&self) -> CpuMask {
        self.cpu_mask.load(Ordering::Acquire)
    }
    
    /// Set CPU mask
    pub fn set_cpu_mask(&self, mask: CpuMask) {
        self.cpu_mask.store(mask, Ordering::Release);
        self.affinity_changes.fetch_add(1, Ordering::Relaxed);
        smp_wmb();
    }
    
    /// Get preferred CPU mask
    pub fn get_preferred_mask(&self) -> CpuMask {
        self.preferred_mask.load(Ordering::Acquire)
    }
    
    /// Set preferred CPU mask
    pub fn set_preferred_mask(&self, mask: CpuMask) {
        self.preferred_mask.store(mask, Ordering::Release);
        smp_wmb();
    }
    
    /// Get avoid CPU mask
    pub fn get_avoid_mask(&self) -> CpuMask {
        self.avoid_mask.load(Ordering::Acquire)
    }
    
    /// Set avoid CPU mask
    pub fn set_avoid_mask(&self, mask: CpuMask) {
        self.avoid_mask.store(mask, Ordering::Release);
        smp_wmb();
    }
    
    /// Get NUMA node preference
    pub fn get_numa_node(&self) -> u32 {
        self.numa_node.load(Ordering::Acquire)
    }
    
    /// Set NUMA node preference
    pub fn set_numa_node(&self, node: u32) {
        self.numa_node.store(node, Ordering::Release);
        smp_wmb();
    }
    
    /// Get cache level preference
    pub fn get_cache_level(&self) -> u32 {
        self.cache_level.load(Ordering::Acquire)
    }
    
    /// Set cache level preference
    pub fn set_cache_level(&self, level: u32) {
        self.cache_level.store(level, Ordering::Release);
        smp_wmb();
    }
    
    /// Get package preference
    pub fn get_package_id(&self) -> u32 {
        self.package_id.load(Ordering::Acquire)
    }
    
    /// Set package preference
    pub fn set_package_id(&self, package: u32) {
        self.package_id.store(package, Ordering::Release);
        smp_wmb();
    }
    
    /// Get last CPU
    pub fn get_last_cpu(&self) -> u32 {
        self.last_cpu.load(Ordering::Acquire)
    }
    
    /// Set last CPU
    pub fn set_last_cpu(&self, cpu: u32) {
        let old_cpu = self.last_cpu.load(Ordering::Acquire);
        if old_cpu != cpu {
            self.last_cpu.store(cpu, Ordering::Release);
            self.migrations.fetch_add(1, Ordering::Relaxed);
            smp_wmb();
        }
    }
    
    /// Get migration count
    pub fn get_migration_count(&self) -> u64 {
        self.migrations.load(Ordering::Acquire)
    }
    
    /// Check if CPU is allowed by affinity
    pub fn is_cpu_allowed(&self, cpu: u32) -> bool {
        let policy = self.get_policy();
        let cpu_bit = 1u64 << cpu;
        
        match policy {
            AffinityPolicy::Any => true,
            AffinityPolicy::Fixed => (self.get_cpu_mask() & cpu_bit) != 0,
            AffinityPolicy::Preferred => {
                let preferred = self.get_preferred_mask();
                let avoid = self.get_avoid_mask();
                (preferred & cpu_bit) != 0 || ((preferred == 0) && ((avoid & cpu_bit) == 0))
            }
            AffinityPolicy::Avoid => (self.get_avoid_mask() & cpu_bit) == 0,
            AffinityPolicy::Numa => self.is_numa_cpu(cpu),
            AffinityPolicy::Cache => self.is_cache_cpu(cpu),
            AffinityPolicy::Package => self.is_package_cpu(cpu),
        }
    }
    
    /// Check if CPU belongs to preferred NUMA node
    fn is_numa_cpu(&self, cpu: u32) -> bool {
        if let Some(topology) = get_system_topology() {
            if let Some(cpu_topology) = topology.get_cpu_topology(cpu) {
                let guard = cpu_topology.read();
                return guard.numa_node_id == self.get_numa_node();
            }
        }
        false
    }
    
    /// Check if CPU shares preferred cache level
    fn is_cache_cpu(&self, cpu: u32) -> bool {
        let cache_level = self.get_cache_level();
        if cache_level == 0 {
            return true; // No preference
        }
        
        // Check if CPU shares cache with last CPU
        let last_cpu = self.get_last_cpu();
        if last_cpu == cpu {
            return true;
        }
        
        if let Some(topology) = get_system_topology() {
            let cache_level_u8 = cache_level.min(u8::MAX as u32) as u8;
            let sharing_cpus = topology.get_cache_sharing_cpus(last_cpu, cache_level_u8);
            return sharing_cpus.contains(&cpu);
        }
        
        false
    }
    
    /// Check if CPU is in preferred package
    fn is_package_cpu(&self, cpu: u32) -> bool {
        let package_id = self.get_package_id();
        if package_id == 0 {
            return true; // No preference
        }
        
        if let Some(topology) = get_system_topology() {
            if let Some(cpu_topology) = topology.get_cpu_topology(cpu) {
                let guard = cpu_topology.read();
                return guard.package_id == package_id;
            }
        }
        
        false
    }
    
    /// Get best CPU for this task
    pub fn get_best_cpu(&self, available_cpus: &[u32]) -> Option<u32> {
        let policy = self.get_policy();
        
        match policy {
            AffinityPolicy::Any => self.get_any_cpu(available_cpus),
            AffinityPolicy::Fixed => self.get_fixed_cpu(available_cpus),
            AffinityPolicy::Preferred => self.get_preferred_cpu(available_cpus),
            AffinityPolicy::Avoid => self.get_avoid_cpu(available_cpus),
            AffinityPolicy::Numa => self.get_numa_cpu(available_cpus),
            AffinityPolicy::Cache => self.get_cache_cpu(available_cpus),
            AffinityPolicy::Package => self.get_package_cpu(available_cpus),
        }
    }
    
    /// Get any available CPU
    fn get_any_cpu(&self, available_cpus: &[u32]) -> Option<u32> {
        if self.sticky.load(Ordering::Acquire) {
            let last_cpu = self.get_last_cpu();
            if available_cpus.contains(&last_cpu) {
                return Some(last_cpu);
            }
        }
        
        // Return first available CPU
        available_cpus.first().copied()
    }
    
    /// Get fixed affinity CPU
    fn get_fixed_cpu(&self, available_cpus: &[u32]) -> Option<u32> {
        let mask = self.get_cpu_mask();
        
        // Try last CPU first if sticky
        if self.sticky.load(Ordering::Acquire) {
            let last_cpu = self.get_last_cpu();
            if available_cpus.contains(&last_cpu) && ((mask >> last_cpu) & 1) != 0 {
                return Some(last_cpu);
            }
        }
        
        // Find first allowed CPU
        for &cpu in available_cpus {
            if ((mask >> cpu) & 1) != 0 {
                return Some(cpu);
            }
        }
        
        None
    }
    
    /// Get preferred CPU
    fn get_preferred_cpu(&self, available_cpus: &[u32]) -> Option<u32> {
        let preferred_mask = self.get_preferred_mask();
        let avoid_mask = self.get_avoid_mask();
        
        // Try last CPU first if sticky and not avoided
        if self.sticky.load(Ordering::Acquire) {
            let last_cpu = self.get_last_cpu();
            if available_cpus.contains(&last_cpu) && ((avoid_mask >> last_cpu) & 1) == 0 {
                return Some(last_cpu);
            }
        }
        
        // Try preferred CPUs
        for &cpu in available_cpus {
            if ((preferred_mask >> cpu) & 1) != 0 && ((avoid_mask >> cpu) & 1) == 0 {
                return Some(cpu);
            }
        }
        
        // Try non-avoided CPUs
        for &cpu in available_cpus {
            if ((avoid_mask >> cpu) & 1) == 0 {
                return Some(cpu);
            }
        }
        
        None
    }
    
    /// Get avoid CPU
    fn get_avoid_cpu(&self, available_cpus: &[u32]) -> Option<u32> {
        let avoid_mask = self.get_avoid_mask();
        
        // Try last CPU first if sticky and not avoided
        if self.sticky.load(Ordering::Acquire) {
            let last_cpu = self.get_last_cpu();
            if available_cpus.contains(&last_cpu) && ((avoid_mask >> last_cpu) & 1) == 0 {
                return Some(last_cpu);
            }
        }
        
        // Find first non-avoided CPU
        for &cpu in available_cpus {
            if ((avoid_mask >> cpu) & 1) == 0 {
                return Some(cpu);
            }
        }
        
        None
    }
    
    /// Get NUMA-aware CPU
    fn get_numa_cpu(&self, available_cpus: &[u32]) -> Option<u32> {
        let numa_node = self.get_numa_node();
        
        // Find CPUs in preferred NUMA node
        let numa_cpus: Vec<u32> = available_cpus.iter()
            .filter(|&&cpu| self.is_numa_cpu(cpu))
            .copied()
            .collect();
        
        if !numa_cpus.is_empty() {
            return self.get_any_cpu(&numa_cpus);
        }
        
        // Fallback to any CPU
        self.get_any_cpu(available_cpus)
    }
    
    /// Get cache-aware CPU
    fn get_cache_cpu(&self, available_cpus: &[u32]) -> Option<u32> {
        let cache_level = self.get_cache_level();
        if cache_level == 0 {
            return self.get_any_cpu(available_cpus);
        }
        
        let last_cpu = self.get_last_cpu();
        let cache_level_u8 = cache_level.min(u8::MAX as u32) as u8;
        let cache_cpus = get_cache_sharing_cpus(last_cpu, cache_level_u8);
        
        // Find CPUs sharing cache
        let shared_cpus: Vec<u32> = available_cpus.iter()
            .filter(|&&cpu| cache_cpus.contains(&cpu))
            .copied()
            .collect();
        
        if !shared_cpus.is_empty() {
            return self.get_any_cpu(&shared_cpus);
        }
        
        // Fallback to any CPU
        self.get_any_cpu(available_cpus)
    }
    
    /// Get package-aware CPU
    fn get_package_cpu(&self, available_cpus: &[u32]) -> Option<u32> {
        let package_id = self.get_package_id();
        if package_id == 0 {
            return self.get_any_cpu(available_cpus);
        }
        
        let package_cpus = get_package_cpus(self.get_last_cpu());
        
        // Find CPUs in same package
        let same_package_cpus: Vec<u32> = available_cpus.iter()
            .filter(|&&cpu| package_cpus.contains(&cpu))
            .copied()
            .collect();
        
        if !same_package_cpus.is_empty() {
            return self.get_any_cpu(&same_package_cpus);
        }
        
        // Fallback to any CPU
        self.get_any_cpu(available_cpus)
    }
    
    /// Set load balancing
    pub fn set_load_balance(&self, enabled: bool) {
        self.load_balance.store(enabled, Ordering::Release);
        smp_wmb();
    }
    
    /// Set sticky affinity
    pub fn set_sticky(&self, enabled: bool) {
        self.sticky.store(enabled, Ordering::Release);
        smp_wmb();
    }
    
    /// Get affinity statistics
    pub fn get_stats(&self) -> AffinityStats {
        AffinityStats {
            policy: self.get_policy(),
            cpu_mask: self.get_cpu_mask(),
            preferred_mask: self.get_preferred_mask(),
            avoid_mask: self.get_avoid_mask(),
            numa_node: self.get_numa_node(),
            cache_level: self.get_cache_level(),
            package_id: self.get_package_id(),
            last_cpu: self.get_last_cpu(),
            migrations: self.get_migration_count(),
            affinity_changes: self.affinity_changes.load(Ordering::Relaxed),
            load_balance: self.load_balance.load(Ordering::Acquire),
            sticky: self.sticky.load(Ordering::Acquire),
        }
    }
}

/// Affinity statistics
#[derive(Debug, Clone, Copy)]
pub struct AffinityStats {
    pub policy: AffinityPolicy,
    pub cpu_mask: CpuMask,
    pub preferred_mask: CpuMask,
    pub avoid_mask: CpuMask,
    pub numa_node: u32,
    pub cache_level: u32,
    pub package_id: u32,
    pub last_cpu: u32,
    pub migrations: u64,
    pub affinity_changes: u64,
    pub load_balance: bool,
    pub sticky: bool,
}

/// CPU affinity manager
pub struct AffinityManager {
    /// Maximum number of CPUs
    max_cpus: u32,
    /// Task affinity descriptors
    task_affinities: Vec<RcuPtr<TaskAffinity>>,
    /// CPU load tracking
    cpu_loads: Vec<AtomicU32>,
    /// Global affinity policy
    global_policy: AtomicU32, // AffinityPolicy as u32
    /// Load balancing enabled
    load_balance_enabled: AtomicBool,
    /// Migration threshold
    migration_threshold: AtomicU32,
    /// Statistics
    stats: AffinityManagerStats,
}

/// Affinity manager statistics
#[derive(Debug)]
pub struct AffinityManagerStats {
    pub total_affinities: AtomicU64,
    pub total_migrations: AtomicU64,
    pub load_balancing_events: AtomicU64,
    pub affinity_changes: AtomicU64,
}

impl AffinityManagerStats {
    pub const fn new() -> Self {
        Self {
            total_affinities: AtomicU64::new(0),
            total_migrations: AtomicU64::new(0),
            load_balancing_events: AtomicU64::new(0),
            affinity_changes: AtomicU64::new(0),
        }
    }
    
    pub fn record_affinity(&self) {
        self.total_affinities.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn record_migration(&self) {
        self.total_migrations.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn record_load_balance(&self) {
        self.load_balancing_events.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn record_affinity_change(&self) {
        self.affinity_changes.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn get_stats(&self) -> (u64, u64, u64, u64) {
        (
            self.total_affinities.load(Ordering::Relaxed),
            self.total_migrations.load(Ordering::Relaxed),
            self.load_balancing_events.load(Ordering::Relaxed),
            self.affinity_changes.load(Ordering::Relaxed),
        )
    }
}

impl AffinityManager {
    /// Create new affinity manager
    pub fn new(max_cpus: u32) -> Self {
        let mut task_affinities = Vec::new();
        let mut cpu_loads = Vec::new();
        
        // Initialize CPU load tracking
        for _ in 0..max_cpus {
            cpu_loads.push(AtomicU32::new(0));
        }
        
        Self {
            max_cpus,
            task_affinities,
            cpu_loads,
            global_policy: AtomicU32::new(AffinityPolicy::Any as u32),
            load_balance_enabled: AtomicBool::new(true),
            migration_threshold: AtomicU32::new(80), // 80% load threshold
            stats: AffinityManagerStats::new(),
        }
    }
    
    /// Create task affinity
    pub fn create_task_affinity(&mut self, task_id: u64) -> RcuPtr<TaskAffinity> {
        let affinity = Box::new(TaskAffinity::new(task_id));
        let affinity_ptr = RcuPtr::new(Box::into_raw(affinity));
        
        // Ensure vector is large enough
        while self.task_affinities.len() <= task_id as usize {
            self.task_affinities.push(RcuPtr::new(core::ptr::null_mut()));
        }
        
        self.task_affinities[task_id as usize] = affinity_ptr.clone();
        self.stats.record_affinity();
        
        affinity_ptr
    }
    
    /// Get task affinity
    pub fn get_task_affinity(&self, task_id: u64) -> Option<RcuPtr<TaskAffinity>> {
        if task_id as usize >= self.task_affinities.len() {
            return None;
        }
        
        let affinity_ptr = self.task_affinities[task_id as usize].clone();
        if affinity_ptr.read().as_ptr().is_null() {
            return None;
        }
        
        Some(affinity_ptr)
    }
    
    /// Remove task affinity
    pub fn remove_task_affinity(&mut self, task_id: u64) {
        if (task_id as usize) < self.task_affinities.len() {
            self.task_affinities[task_id as usize] = RcuPtr::new(core::ptr::null_mut());
        }
    }
    
    /// Get best CPU for task
    pub fn get_best_cpu_for_task(&self, task_id: u64) -> Option<u32> {
        let affinity = match self.get_task_affinity(task_id) {
            Some(affinity) => affinity,
            None => return None,
        };
        
        // Get available CPUs (online and not overloaded)
        let available_cpus = self.get_available_cpus();
        
        if available_cpus.is_empty() {
            return None;
        }
        
        let best_cpu = affinity.read().get_best_cpu(&available_cpus);
        
        if let Some(cpu) = best_cpu {
            // Update last CPU
            affinity.read().set_last_cpu(cpu);
            
            // Update CPU load
            self.update_cpu_load(cpu, 1);
        }
        
        best_cpu
    }
    
    /// Get available CPUs
    fn get_available_cpus(&self) -> Vec<u32> {
        let mut available = Vec::new();
        
        for cpu_id in 0..self.max_cpus {
            // Check if CPU is online
            if !self.is_cpu_online(cpu_id) {
                continue;
            }
            
            // Check if CPU is overloaded (if load balancing is enabled)
            if self.load_balance_enabled.load(Ordering::Acquire) {
                let load = self.cpu_loads[cpu_id as usize].load(Ordering::Acquire);
                let threshold = self.migration_threshold.load(Ordering::Acquire);
                
                if load > threshold {
                    continue;
                }
            }
            
            available.push(cpu_id);
        }
        
        available
    }
    
    /// Check if CPU is online
    fn is_cpu_online(&self, cpu_id: u32) -> bool {
        // This would check with hotplug manager
        // For now, assume all CPUs are online
        cpu_id < self.max_cpus
    }
    
    /// Update CPU load
    fn update_cpu_load(&self, cpu_id: u32, delta: i32) {
        if cpu_id as usize >= self.cpu_loads.len() {
            return;
        }
        
        let current_load = self.cpu_loads[cpu_id as usize].load(Ordering::Acquire);
        let new_load = if delta > 0 {
            current_load.saturating_add(delta as u32)
        } else {
            current_load.saturating_sub((-delta) as u32)
        };
        
        self.cpu_loads[cpu_id as usize].store(new_load, Ordering::Release);
        smp_wmb();
    }
    
    /// Balance load across CPUs
    pub fn balance_load(&self) {
        if !self.load_balance_enabled.load(Ordering::Acquire) {
            return;
        }
        
        let threshold = self.migration_threshold.load(Ordering::Acquire);
        let mut overloaded_cpus = Vec::new();
        let mut underloaded_cpus = Vec::new();
        
        // Find overloaded and underloaded CPUs
        for cpu_id in 0..self.max_cpus {
            let load = self.cpu_loads[cpu_id as usize].load(Ordering::Acquire);
            
            if load > threshold {
                overloaded_cpus.push((cpu_id, load));
            } else if load < threshold / 2 {
                underloaded_cpus.push(cpu_id);
            }
        }
        
        // Migrate tasks from overloaded to underloaded CPUs
        for &(overloaded_cpu, _) in &overloaded_cpus {
            if underloaded_cpus.is_empty() {
                break;
            }
            
            // Find tasks that can be migrated
            let migratable_tasks = self.find_migratable_tasks(overloaded_cpu);
            
            for task_id in migratable_tasks {
                if underloaded_cpus.is_empty() {
                    break;
                }
                
                if let Some(target_cpu) = underloaded_cpus.pop() {
                    if self.migrate_task(task_id, target_cpu) {
                        self.stats.record_load_balance();
                    }
                }
            }
        }
    }
    
    /// Find tasks that can be migrated from CPU
    fn find_migratable_tasks(&self, cpu_id: u32) -> Vec<u64> {
        let mut migratable = Vec::new();
        
        // This would find tasks running on the specified CPU
        // For now, return empty list
        migratable
    }
    
    /// Migrate task to different CPU
    fn migrate_task(&self, task_id: u64, target_cpu: u32) -> bool {
        let affinity = match self.get_task_affinity(task_id) {
            Some(affinity) => affinity,
            None => return false,
        };
        
        // Check if task can run on target CPU
        if !affinity.read().is_cpu_allowed(target_cpu) {
            return false;
        }
        
        // Update CPU loads
        let current_cpu = affinity.read().get_last_cpu();
        self.update_cpu_load(current_cpu, -1);
        self.update_cpu_load(target_cpu, 1);
        
        // Update affinity
        affinity.read().set_last_cpu(target_cpu);
        
        // Record migration
        affinity.read().migrations.fetch_add(1, Ordering::Relaxed);
        self.stats.record_migration();
        
        crate::serial_println!("Affinity: Migrated task {} from CPU {} to CPU {}", 
            task_id, current_cpu, target_cpu);
        
        true
    }
    
    /// Set global affinity policy
    pub fn set_global_policy(&self, policy: AffinityPolicy) {
        self.global_policy.store(policy as u32, Ordering::Release);
        smp_wmb();
    }
    
    /// Get global affinity policy
    pub fn get_global_policy(&self) -> AffinityPolicy {
        match self.global_policy.load(Ordering::Acquire) {
            0 => AffinityPolicy::Any,
            1 => AffinityPolicy::Fixed,
            2 => AffinityPolicy::Preferred,
            3 => AffinityPolicy::Avoid,
            4 => AffinityPolicy::Numa,
            5 => AffinityPolicy::Cache,
            6 => AffinityPolicy::Package,
            _ => AffinityPolicy::Any,
        }
    }
    
    /// Enable/disable load balancing
    pub fn set_load_balance_enabled(&self, enabled: bool) {
        self.load_balance_enabled.store(enabled, Ordering::Release);
        smp_wmb();
    }
    
    /// Set migration threshold
    pub fn set_migration_threshold(&self, threshold: u32) {
        self.migration_threshold.store(threshold, Ordering::Release);
        smp_wmb();
    }
    
    /// Get CPU load
    pub fn get_cpu_load(&self, cpu_id: u32) -> Option<u32> {
        if cpu_id as usize >= self.cpu_loads.len() {
            return None;
        }
        
        Some(self.cpu_loads[cpu_id as usize].load(Ordering::Acquire))
    }
    
    /// Get all CPU loads
    pub fn get_all_cpu_loads(&self) -> Vec<(u32, u32)> {
        let mut loads = Vec::new();
        
        for cpu_id in 0..self.max_cpus {
            if let Some(load) = self.get_cpu_load(cpu_id) {
                loads.push((cpu_id, load));
            }
        }
        
        loads
    }
    
    /// Get manager statistics
    pub fn get_stats(&self) -> (u64, u64, u64, u64) {
        self.stats.get_stats()
    }
}

/// Global affinity manager instance
static mut AFFINITY_MANAGER: Option<AffinityManager> = None;
static AFFINITY_INIT: AtomicBool = AtomicBool::new(false);

/// Initialize affinity subsystem
pub fn init(max_cpus: u32) {
    if AFFINITY_INIT.load(Ordering::Acquire) {
        return;
    }
    
    crate::serial_println!("Affinity: Initializing CPU affinity for {} CPUs", max_cpus);
    
    let manager = AffinityManager::new(max_cpus);
    
    unsafe {
        AFFINITY_MANAGER = Some(manager);
    }
    
    AFFINITY_INIT.store(true, Ordering::Release);
    smp_mb();
    
    crate::serial_println!("Affinity: CPU affinity initialized");
}

/// Get affinity manager
pub fn get_manager() -> Option<&'static AffinityManager> {
    if !AFFINITY_INIT.load(Ordering::Acquire) {
        return None;
    }
    
    unsafe { AFFINITY_MANAGER.as_ref() }
}

/// Convenience functions
pub fn create_task_affinity(task_id: u64) -> Option<RcuPtr<TaskAffinity>> {
    if let Some(manager) = get_manager() {
        // This would need mutable access in real implementation
        // For now, return None
        None
    } else {
        None
    }
}

pub fn get_task_affinity(task_id: u64) -> Option<RcuPtr<TaskAffinity>> {
    get_manager()?.get_task_affinity(task_id)
}

pub fn get_best_cpu_for_task(task_id: u64) -> Option<u32> {
    get_manager()?.get_best_cpu_for_task(task_id)
}

pub fn balance_load() {
    if let Some(manager) = get_manager() {
        manager.balance_load();
    }
}

pub fn get_cpu_load(cpu_id: u32) -> Option<u32> {
    get_manager()?.get_cpu_load(cpu_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_task_affinity() {
        let affinity = TaskAffinity::new(123);
        
        assert_eq!(affinity.get_policy(), AffinityPolicy::Any);
        assert_eq!(affinity.get_cpu_mask(), !0u64);
        
        affinity.set_policy(AffinityPolicy::Fixed);
        affinity.set_cpu_mask(0b1010);
        
        assert_eq!(affinity.get_policy(), AffinityPolicy::Fixed);
        assert_eq!(affinity.get_cpu_mask(), 0b1010);
        assert!(affinity.is_cpu_allowed(1));
        assert!(!affinity.is_cpu_allowed(2));
    }
    
    #[test]
    fn test_affinity_manager() {
        let manager = AffinityManager::new(4);
        
        assert_eq!(manager.get_global_policy(), AffinityPolicy::Any);
        assert!(manager.load_balance_enabled.load(Ordering::Acquire));
        
        manager.set_global_policy(AffinityPolicy::Numa);
        assert_eq!(manager.get_global_policy(), AffinityPolicy::Numa);
    }
}
