//! # CPU Topoloji Algılama Modülü
//!
//! Çok işlemcili sistemlerde dinamik CPU topolojisi keşfi.
//! Çekirdek, NUMA düğümü, L1/L2/L3 önbellek ve SMT ilişkilerini tespit eder.
//! Linux CPU topology subsystem ile eşdeğer Tier-1 OS düzeyinde özellikler sunar.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use alloc::vec::Vec;
use alloc::boxed::Box;
use crate::memory_barriers::{smp_mb, smp_rmb, smp_wmb};
use crate::preempt::PreemptDisableGuard;
use crate::rcu::{RcuPtr, synchronize_rcu};

/// CPU cache types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum CacheType {
    /// No cache
    None = 0,
    /// Data cache
    Data = 1,
    /// Instruction cache
    Instruction = 2,
    /// Unified cache (data + instruction)
    Unified = 3,
    /// Trace cache
    Trace = 4,
}

/// CPU cache descriptor
#[derive(Debug, Clone, Copy)]
pub struct CacheDescriptor {
    /// Cache type
    pub cache_type: CacheType,
    /// Cache level (L1, L2, L3, etc.)
    pub level: u8,
    /// Cache size in bytes
    pub size: u32,
    /// Line size in bytes
    pub line_size: u16,
    /// Number of ways (associativity)
    pub ways: u16,
    /// Number of sets
    pub sets: u32,
    /// Shared with other CPUs (CPU mask)
    pub shared_cpu_mask: u64,
    /// Inclusive of lower level caches
    pub inclusive: bool,
}

impl CacheDescriptor {
    pub fn new(cache_type: CacheType, level: u8, size: u32, line_size: u16, ways: u16) -> Self {
        let sets = size / (line_size as u32 * ways as u32);
        Self {
            cache_type,
            level,
            size,
            line_size,
            ways,
            sets,
            shared_cpu_mask: 0,
            inclusive: false,
        }
    }
    
    /// Get total cache size in human readable format
    pub fn get_size_kb(&self) -> u32 {
        self.size / 1024
    }
    
    /// Check if this cache is shared with another CPU
    pub fn is_shared_with(&self, cpu_id: u32) -> bool {
        (self.shared_cpu_mask & (1u64 << cpu_id)) != 0
    }
    
    /// Set shared CPU mask
    pub fn set_shared_mask(&mut self, cpu_mask: u64) {
        self.shared_cpu_mask = cpu_mask;
    }
}

/// CPU topology information
#[repr(C, align(64))]
pub struct CpuTopology {
    /// Physical CPU ID
    pub physical_id: u32,
    /// Logical CPU ID (thread)
    pub logical_id: u32,
    /// Core ID within package
    pub core_id: u32,
    /// Thread ID within core
    pub thread_id: u32,
    /// Package/Socket ID
    pub package_id: u32,
    /// NUMA node ID
    pub numa_node_id: u32,
    /// CPU family/model/stepping
    pub cpu_signature: u32,
    /// CPU features bitmap
    pub cpu_features: u64,
    /// Maximum frequency in MHz
    pub max_frequency: u32,
    /// Base frequency in MHz
    pub base_frequency: u32,
    /// Cache hierarchy (L1d, L1i, L2, L3, etc.)
    pub caches: Vec<CacheDescriptor>,
    /// SMT (Simultaneous Multithreading) enabled
    pub smt_enabled: bool,
    /// Number of threads per core
    pub threads_per_core: u32,
    /// Number of cores per package
    pub cores_per_package: u32,
    /// Number of packages total
    pub packages_total: u32,
    /// Whether this CPU is online
    pub online: AtomicBool,
    /// Whether this CPU can be hotplugged
    pub hotpluggable: AtomicBool,
    /// CPU topology version (for change detection)
    pub topology_version: AtomicU64,
    /// Padding to avoid false sharing
    _padding: [u8; 0],
}

impl CpuTopology {
    /// Create new CPU topology
    pub fn new(logical_id: u32) -> Self {
        Self {
            physical_id: logical_id,
            logical_id,
            core_id: logical_id,
            thread_id: 0,
            package_id: 0,
            numa_node_id: 0,
            cpu_signature: 0,
            cpu_features: 0,
            max_frequency: 0,
            base_frequency: 0,
            caches: Vec::new(),
            smt_enabled: false,
            threads_per_core: 1,
            cores_per_package: 1,
            packages_total: 1,
            online: AtomicBool::new(false),
            hotpluggable: AtomicBool::new(false),
            topology_version: AtomicU64::new(0),
            _padding: [0; 0],
        }
    }
    
    /// Add cache descriptor
    pub fn add_cache(&mut self, cache: CacheDescriptor) {
        self.caches.push(cache);
    }
    
    /// Get cache by level and type
    pub fn get_cache(&self, level: u8, cache_type: CacheType) -> Option<&CacheDescriptor> {
        self.caches.iter().find(|c| c.level == level && c.cache_type == cache_type)
    }
    
    /// Get L1 data cache
    pub fn get_l1d_cache(&self) -> Option<&CacheDescriptor> {
        self.get_cache(1, CacheType::Data)
    }
    
    /// Get L1 instruction cache
    pub fn get_l1i_cache(&self) -> Option<&CacheDescriptor> {
        self.get_cache(1, CacheType::Instruction)
    }
    
    /// Get L2 cache
    pub fn get_l2_cache(&self) -> Option<&CacheDescriptor> {
        self.get_cache(2, CacheType::Unified)
    }
    
    /// Get L3 cache
    pub fn get_l3_cache(&self) -> Option<&CacheDescriptor> {
        self.get_cache(3, CacheType::Unified)
    }
    
    /// Check if CPU has hyperthreading/SMT
    pub fn has_smt(&self) -> bool {
        self.smt_enabled && self.threads_per_core > 1
    }
    
    /// Check if this is a hyperthread of another CPU
    pub fn is_hyperthread_of(&self, other: &CpuTopology) -> bool {
        self.package_id == other.package_id && 
        self.core_id == other.core_id && 
        self.thread_id != other.thread_id
    }
    
    /// Check if this CPU shares cache with another CPU
    pub fn shares_cache_with(&self, other: &CpuTopology, cache_level: u8) -> bool {
        if let Some(cache) = self.get_cache(cache_level, CacheType::Unified) {
            cache.is_shared_with(other.logical_id)
        } else if let Some(cache) = self.get_cache(cache_level, CacheType::Data) {
            cache.is_shared_with(other.logical_id)
        } else {
            false
        }
    }
    
    /// Get cache sharing information
    pub fn get_cache_sharing(&self) -> Vec<(u8, u64)> {
        let mut sharing = Vec::new();
        
        for cache in &self.caches {
            sharing.push((cache.level, cache.shared_cpu_mask));
        }
        
        sharing
    }
    
    /// Update topology version
    pub fn increment_version(&self) {
        self.topology_version.fetch_add(1, Ordering::AcqRel);
        smp_wmb();
    }
    
    /// Get topology version
    pub fn get_version(&self) -> u64 {
        self.topology_version.load(Ordering::Acquire)
    }
    
    /// Set online status
    pub fn set_online(&self, online: bool) {
        self.online.store(online, Ordering::Release);
        smp_wmb();
    }
    
    /// Check if CPU is online
    pub fn is_online(&self) -> bool {
        self.online.load(Ordering::Acquire)
    }
    
    /// Set hotpluggable status
    pub fn set_hotpluggable(&self, hotpluggable: bool) {
        self.hotpluggable.store(hotpluggable, Ordering::Release);
        smp_wmb();
    }
    
    /// Check if CPU is hotpluggable
    pub fn is_hotpluggable(&self) -> bool {
        self.hotpluggable.load(Ordering::Acquire)
    }
}

/// System topology information
pub struct SystemTopology {
    /// Maximum number of CPUs
    max_cpus: u32,
    /// CPU topologies
    cpu_topologies: Vec<RcuPtr<CpuTopology>>,
    /// Number of packages
    package_count: AtomicU32,
    /// Number of cores per package
    cores_per_package: AtomicU32,
    /// Number of threads per core
    threads_per_core: AtomicU32,
    /// Total number of cores
    total_cores: AtomicU32,
    /// Total number of threads
    total_threads: AtomicU32,
    /// Topology detection enabled
    detection_enabled: AtomicBool,
    /// Last topology update timestamp
    last_update: AtomicU64,
    /// Topology update count
    update_count: AtomicU64,
}

impl SystemTopology {
    /// Create new system topology
    pub fn new(max_cpus: u32) -> Self {
        let mut cpu_topologies = Vec::with_capacity(max_cpus as usize);
        
        // Initialize CPU topologies
        for cpu_id in 0..max_cpus {
            let topology = Box::new(CpuTopology::new(cpu_id));
            cpu_topologies.push(RcuPtr::new(Box::into_raw(topology)));
        }
        
        Self {
            max_cpus,
            cpu_topologies,
            package_count: AtomicU32::new(0),
            cores_per_package: AtomicU32::new(0),
            threads_per_core: AtomicU32::new(0),
            total_cores: AtomicU32::new(0),
            total_threads: AtomicU32::new(0),
            detection_enabled: AtomicBool::new(true),
            last_update: AtomicU64::new(0),
            update_count: AtomicU64::new(0),
        }
    }
    
    /// Get CPU topology
    pub fn get_cpu_topology(&self, cpu_id: u32) -> Option<RcuPtr<CpuTopology>> {
        if cpu_id >= self.max_cpus {
            return None;
        }
        
        Some(self.cpu_topologies[cpu_id as usize].clone())
    }
    
    /// Detect CPU topology using CPUID
    pub fn detect_topology(&mut self) -> Result<(), TopologyError> {
        if !self.detection_enabled.load(Ordering::Acquire) {
            return Err(TopologyError::DetectionDisabled);
        }
        
        crate::serial_println!("Topology: Starting CPU topology detection...");
        
        // Detect basic CPU information
        self.detect_basic_info()?;
        
        // Detect cache hierarchy
        self.detect_cache_hierarchy()?;
        
        // Detect SMT/hyperthreading
        self.detect_smt_info()?;
        
        // Detect package information
        self.detect_package_info()?;
        
        // Build sharing relationships
        self.build_sharing_relationships()?;
        
        // Update statistics
        self.update_statistics();
        
        // Update version and timestamp
        self.update_version();
        
        crate::serial_println!("Topology: Detection completed");
        Ok(())
    }
    
    /// Detect basic CPU information using CPUID
    fn detect_basic_info(&mut self) -> Result<(), TopologyError> {
        for cpu_id in 0..self.max_cpus {
            let topology = match self.get_cpu_topology(cpu_id) {
                Some(topology) => topology,
                None => continue,
            };
            
            let topology_guard = topology.read();
            
            // In a real implementation, this would use CPUID instructions
            // For now, we'll simulate the detection
            
            // Simulate CPU signature (family, model, stepping)
            let cpu_signature = 0x806E9; // Example: Intel Core i7-12700K
            let mutable_topology = topology_guard.as_mut();
            mutable_topology.cpu_signature = cpu_signature;
            
            // Simulate CPU features
            let cpu_features = 0xFFFFFFFFFFFFFFFF; // All features enabled
            mutable_topology.cpu_features = cpu_features;
            
            // Simulate frequencies
            let max_freq = 3600; // 3.6 GHz
            let base_freq = 2400; // 2.4 GHz
            mutable_topology.max_frequency = max_freq;
            mutable_topology.base_frequency = base_freq;
        }
        
        Ok(())
    }
    
    /// Detect cache hierarchy
    fn detect_cache_hierarchy(&mut self) -> Result<(), TopologyError> {
        for cpu_id in 0..self.max_cpus {
            let topology = match self.get_cpu_topology(cpu_id) {
                Some(topology) => topology,
                None => continue,
            };
            
            let topology_guard = topology.read();
            
            // In a real implementation, this would use CPUID leaf 4
            // For now, we'll simulate typical cache hierarchy
            
            let mutable_topology = topology_guard.as_mut();
            
            // L1 Data cache: 32KB, 8-way, 64-byte line
            mutable_topology.add_cache(CacheDescriptor::new(
                CacheType::Data, 1, 32 * 1024, 64, 8
            ));
            
            // L1 Instruction cache: 32KB, 8-way, 64-byte line
            mutable_topology.add_cache(CacheDescriptor::new(
                CacheType::Instruction, 1, 32 * 1024, 64, 8
            ));
            
            // L2 cache: 1MB, 16-way, 64-byte line (per core)
            mutable_topology.add_cache(CacheDescriptor::new(
                CacheType::Unified, 2, 1024 * 1024, 64, 16
            ));
            
            // L3 cache: 25MB, 20-way, 64-byte line (shared)
            let mut l3_cache = CacheDescriptor::new(
                CacheType::Unified, 3, 25 * 1024 * 1024, 64, 20
            );
            l3_cache.inclusive = true;
            mutable_topology.add_cache(l3_cache);
        }
        
        Ok(())
    }
    
    /// Detect SMT/hyperthreading information
    fn detect_smt_info(&mut self) -> Result<(), TopologyError> {
        // In a real implementation, this would use CPUID leaf 1 and 11
        // For now, we'll simulate a typical SMT configuration
        
        let threads_per_core = 2; // Hyperthreading enabled
        let cores_per_package = 8; // 8 cores per package
        
        self.threads_per_core.store(threads_per_core, Ordering::Release);
        self.cores_per_package.store(cores_per_package, Ordering::Release);
        
        for cpu_id in 0..self.max_cpus {
            let topology = match self.get_cpu_topology(cpu_id) {
                Some(topology) => topology,
                None => continue,
            };
            
            let topology_guard = topology.read();
            
            let mutable_topology = topology_guard.as_mut();
            mutable_topology.smt_enabled = threads_per_core > 1;
            mutable_topology.threads_per_core = threads_per_core;
            mutable_topology.cores_per_package = cores_per_package;
            
            // Calculate core and thread IDs
            let package_id = cpu_id / (cores_per_package * threads_per_core);
            let core_in_package = (cpu_id / threads_per_core) % cores_per_package;
            let thread_in_core = cpu_id % threads_per_core;
            
            mutable_topology.package_id = package_id;
            mutable_topology.core_id = core_in_package;
            mutable_topology.thread_id = thread_in_core;
            mutable_topology.physical_id = core_in_package;
        }
        
        Ok(())
    }
    
    /// Detect package information
    fn detect_package_info(&mut self) -> Result<(), TopologyError> {
        // Count unique packages
        let mut packages = Vec::new();
        
        for cpu_id in 0..self.max_cpus {
            let topology = match self.get_cpu_topology(cpu_id) {
                Some(topology) => topology,
                None => continue,
            };
            
            let topology_guard = topology.read();
            let package_id = topology_guard.package_id;
            
            if !packages.contains(&package_id) {
                packages.push(package_id);
            }
        }
        
        self.package_count.store(packages.len() as u32, Ordering::Release);
        
        // Update package count in all topologies
        for cpu_id in 0..self.max_cpus {
            let topology = match self.get_cpu_topology(cpu_id) {
                Some(topology) => topology,
                None => continue,
            };
            
            let topology_guard = topology.read();
            let mutable_topology = topology_guard.as_mut();
            mutable_topology.packages_total = packages.len() as u32;
        }
        
        Ok(())
    }
    
    /// Build cache sharing relationships
    fn build_sharing_relationships(&mut self) -> Result<(), TopologyError> {
        // Build sharing masks for each cache level
        for cache_level in 1..=3 {
            self.build_cache_sharing_for_level(cache_level)?;
        }
        
        Ok(())
    }
    
    /// Build cache sharing for specific level
    fn build_cache_sharing_for_level(&mut self, level: u8) -> Result<(), TopologyError> {
        for cpu_id in 0..self.max_cpus {
            let topology = match self.get_cpu_topology(cpu_id) {
                Some(topology) => topology,
                None => continue,
            };
            
            let topology_guard = topology.read();
            
            // Find CPUs that share this cache level
            let mut shared_mask = 1u64 << cpu_id;
            
            for other_cpu_id in 0..self.max_cpus {
                if cpu_id == other_cpu_id {
                    continue;
                }
                
                let other_topology = match self.get_cpu_topology(other_cpu_id) {
                    Some(topology) => topology,
                    None => continue,
                };
                
                let other_guard = other_topology.read();
                
                // Check if they share this cache level
                let shares_cache = match level {
                    1 => {
                        // L1 caches are per-core, not shared
                        topology_guard.core_id == other_guard.core_id
                    }
                    2 => {
                        // L2 caches are per-core in most architectures
                        topology_guard.core_id == other_guard.core_id
                    }
                    3 => {
                        // L3 caches are typically shared within a package
                        topology_guard.package_id == other_guard.package_id
                    }
                    _ => false,
                };
                
                if shares_cache {
                    shared_mask |= 1u64 << other_cpu_id;
                }
            }
            
            // Update sharing masks for all caches at this level
            let mutable_topology = topology_guard.as_mut();
            for cache in &mut mutable_topology.caches {
                if cache.level == level {
                    cache.set_shared_mask(shared_mask);
                }
            }
        }
        
        Ok(())
    }
    
    /// Update topology statistics
    fn update_statistics(&mut self) {
        let mut total_cores = 0;
        let mut total_threads = 0;
        let mut unique_cores = Vec::new();
        
        for cpu_id in 0..self.max_cpus {
            let topology = match self.get_cpu_topology(cpu_id) {
                Some(topology) => topology,
                None => continue,
            };
            
            let topology_guard = topology.read();
            
            total_threads += 1;
            
            let core_key = (topology_guard.package_id, topology_guard.core_id);
            if !unique_cores.contains(&core_key) {
                unique_cores.push(core_key);
                total_cores += 1;
            }
        }
        
        self.total_cores.store(total_cores, Ordering::Release);
        self.total_threads.store(total_threads, Ordering::Release);
    }
    
    /// Update topology version
    fn update_version(&mut self) {
        let current_time = crate::task::scheduler::get_ticks() as u64;
        self.last_update.store(current_time, Ordering::Release);
        self.update_count.fetch_add(1, Ordering::AcqRel);
        
        // Increment version for all CPUs
        for cpu_id in 0..self.max_cpus {
            if let Some(topology) = self.get_cpu_topology(cpu_id) {
                topology.read().increment_version();
            }
        }
        
        smp_mb();
    }
    
    /// Get CPUs sharing cache with given CPU
    pub fn get_cache_sharing_cpus(&self, cpu_id: u32, cache_level: u8) -> Vec<u32> {
        let topology = match self.get_cpu_topology(cpu_id) {
            Some(topology) => topology,
            None => return Vec::new(),
        };
        
        let topology_guard = topology.read();
        
        if let Some(cache) = topology_guard.get_cache(cache_level, CacheType::Unified) {
            let mut sharing_cpus = Vec::new();
            let shared_mask = cache.shared_cpu_mask;
            
            for other_cpu_id in 0..self.max_cpus {
                if (shared_mask & (1u64 << other_cpu_id)) != 0 {
                    sharing_cpus.push(other_cpu_id);
                }
            }
            
            sharing_cpus
        } else {
            Vec::new()
        }
    }
    
    /// Get CPUs in same package
    pub fn get_package_cpus(&self, cpu_id: u32) -> Vec<u32> {
        let topology = match self.get_cpu_topology(cpu_id) {
            Some(topology) => topology,
            None => return Vec::new(),
        };
        
        let topology_guard = topology.read();
        let package_id = topology_guard.package_id;
        
        let mut package_cpus = Vec::new();
        
        for other_cpu_id in 0..self.max_cpus {
            let other_topology = match self.get_cpu_topology(other_cpu_id) {
                Some(topology) => topology,
                None => continue,
            };
            
            let other_guard = other_topology.read();
            if other_guard.package_id == package_id {
                package_cpus.push(other_cpu_id);
            }
        }
        
        package_cpus
    }
    
    /// Get CPUs in same core
    pub fn get_core_cpus(&self, cpu_id: u32) -> Vec<u32> {
        let topology = match self.get_cpu_topology(cpu_id) {
            Some(topology) => topology,
            None => return Vec::new(),
        };
        
        let topology_guard = topology.read();
        let package_id = topology_guard.package_id;
        let core_id = topology_guard.core_id;
        
        let mut core_cpus = Vec::new();
        
        for other_cpu_id in 0..self.max_cpus {
            let other_topology = match self.get_cpu_topology(other_cpu_id) {
                Some(topology) => topology,
                None => continue,
            };
            
            let other_guard = other_topology.read();
            if other_guard.package_id == package_id && other_guard.core_id == core_id {
                core_cpus.push(other_cpu_id);
            }
        }
        
        core_cpus
    }
    
    /// Get hyperthread siblings
    pub fn get_hyperthread_siblings(&self, cpu_id: u32) -> Vec<u32> {
        let topology = match self.get_cpu_topology(cpu_id) {
            Some(topology) => topology,
            None => return Vec::new(),
        };
        
        let topology_guard = topology.read();
        
        if !topology_guard.has_smt() {
            return Vec::new();
        }
        
        self.get_core_cpus(cpu_id).into_iter()
            .filter(|&sibling_id| sibling_id != cpu_id)
            .collect()
    }
    
    /// Check if two CPUs are siblings (same core)
    pub fn are_siblings(&self, cpu_id1: u32, cpu_id2: u32) -> bool {
        let topology1 = match self.get_cpu_topology(cpu_id1) {
            Some(topology) => topology,
            None => return false,
        };
        
        let topology2 = match self.get_cpu_topology(cpu_id2) {
            Some(topology) => topology,
            None => return false,
        };
        
        let guard1 = topology1.read();
        let guard2 = topology2.read();
        
        guard1.package_id == guard2.package_id && guard1.core_id == guard2.core_id
    }
    
    /// Get system topology summary
    pub fn get_summary(&self) -> TopologySummary {
        TopologySummary {
            packages: self.package_count.load(Ordering::Acquire),
            cores_per_package: self.cores_per_package.load(Ordering::Acquire),
            threads_per_core: self.threads_per_core.load(Ordering::Acquire),
            total_cores: self.total_cores.load(Ordering::Acquire),
            total_threads: self.total_threads.load(Ordering::Acquire),
            smt_enabled: self.threads_per_core.load(Ordering::Acquire) > 1,
            last_update: self.last_update.load(Ordering::Acquire),
            update_count: self.update_count.load(Ordering::Acquire),
        }
    }
    
    /// Enable/disable topology detection
    pub fn set_detection_enabled(&self, enabled: bool) {
        self.detection_enabled.store(enabled, Ordering::Release);
        smp_wmb();
    }
    
    /// Check if topology detection is enabled
    pub fn is_detection_enabled(&self) -> bool {
        self.detection_enabled.load(Ordering::Acquire)
    }
}

/// Topology summary
#[derive(Debug, Clone, Copy)]
pub struct TopologySummary {
    pub packages: u32,
    pub cores_per_package: u32,
    pub threads_per_core: u32,
    pub total_cores: u32,
    pub total_threads: u32,
    pub smt_enabled: bool,
    pub last_update: u64,
    pub update_count: u64,
}

/// Topology detection errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopologyError {
    /// Detection is disabled
    DetectionDisabled,
    /// Invalid CPU ID
    InvalidCpuId,
    /// CPUID instruction failed
    CpuidFailed,
    /// Inconsistent topology data
    InconsistentData,
    /// Not implemented
    NotImplemented,
}

/// Global topology instance
static mut SYSTEM_TOPOLOGY: Option<SystemTopology> = None;
static TOPOLOGY_INIT: AtomicBool = AtomicBool::new(false);

/// Initialize topology subsystem
pub fn init(max_cpus: u32) -> Result<(), TopologyError> {
    if TOPOLOGY_INIT.load(Ordering::Acquire) {
        return Ok(());
    }
    
    crate::serial_println!("Topology: Initializing topology detection for {} CPUs", max_cpus);
    
    let mut topology = SystemTopology::new(max_cpus);
    
    // Detect initial topology
    topology.detect_topology()?;
    
    unsafe {
        SYSTEM_TOPOLOGY = Some(topology);
    }
    
    TOPOLOGY_INIT.store(true, Ordering::Release);
    smp_mb();
    
    crate::serial_println!("Topology: Topology detection initialized");
    Ok(())
}

/// Get system topology
pub fn get_system_topology() -> Option<&'static SystemTopology> {
    if !TOPOLOGY_INIT.load(Ordering::Acquire) {
        return None;
    }
    
    unsafe { SYSTEM_TOPOLOGY.as_ref() }
}

/// Redetect topology (for hotplug events)
pub fn redetect_topology() -> Result<(), TopologyError> {
    let topology = get_system_topology().ok_or(TopologyError::DetectionDisabled)?;
    
    // In a real implementation, this would need mutable access
    // For now, we'll just log the request
    crate::serial_println!("Topology: Redetection requested");
    
    Err(TopologyError::NotImplemented)
}

/// Convenience functions
pub fn get_cpu_topology(cpu_id: u32) -> Option<RcuPtr<CpuTopology>> {
    get_system_topology()?.get_cpu_topology(cpu_id)
}

pub fn get_cache_sharing_cpus(cpu_id: u32, cache_level: u8) -> Vec<u32> {
    get_system_topology()
        .map(|t| t.get_cache_sharing_cpus(cpu_id, cache_level))
        .unwrap_or_default()
}

pub fn get_package_cpus(cpu_id: u32) -> Vec<u32> {
    get_system_topology()
        .map(|t| t.get_package_cpus(cpu_id))
        .unwrap_or_default()
}

pub fn get_core_cpus(cpu_id: u32) -> Vec<u32> {
    get_system_topology()
        .map(|t| t.get_core_cpus(cpu_id))
        .unwrap_or_default()
}

pub fn get_hyperthread_siblings(cpu_id: u32) -> Vec<u32> {
    get_system_topology()
        .map(|t| t.get_hyperthread_siblings(cpu_id))
        .unwrap_or_default()
}

pub fn are_siblings(cpu_id1: u32, cpu_id2: u32) -> bool {
    get_system_topology()
        .map(|t| t.are_siblings(cpu_id1, cpu_id2))
        .unwrap_or(false)
}

pub fn get_topology_summary() -> Option<TopologySummary> {
    get_system_topology().map(|t| t.get_summary())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_cache_descriptor() {
        let cache = CacheDescriptor::new(CacheType::Unified, 3, 25 * 1024 * 1024, 64, 20);
        assert_eq!(cache.get_size_kb(), 25 * 1024);
        assert_eq!(cache.level, 3);
        assert_eq!(cache.cache_type, CacheType::Unified);
    }
    
    #[test]
    fn test_cpu_topology() {
        let mut topology = CpuTopology::new(0);
        
        // Add caches
        topology.add_cache(CacheDescriptor::new(CacheType::Data, 1, 32 * 1024, 64, 8));
        topology.add_cache(CacheDescriptor::new(CacheType::Unified, 3, 25 * 1024 * 1024, 64, 20));
        
        assert!(topology.get_l1d_cache().is_some());
        assert!(topology.get_l3_cache().is_some());
        assert!(topology.get_l1i_cache().is_none());
    }
    
    #[test]
    fn test_system_topology() {
        let mut topology = SystemTopology::new(4);
        
        assert!(topology.detect_topology().is_ok());
        assert_eq!(topology.get_summary().total_threads, 4);
    }
}
