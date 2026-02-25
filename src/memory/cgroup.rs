//! # Memory Cgroups
//!
//! Resource control for process groups (cgroups v2 memory controller).

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use spin::{Mutex, RwLock};

// ============================================================================
// CGROUP CONSTANTS
// ============================================================================

/// Maximum cgroups
pub const CGROUP_MAX: usize = 4096;
/// Default memory limit (unlimited)
pub const MEMORY_LIMIT_UNLIMITED: u64 = u64::MAX;
/// Default oom_score_adj
pub const OOM_SCORE_ADJ_DEFAULT: i32 = 0;
/// OOM score adjustment min/max
pub const OOM_SCORE_ADJ_MIN: i32 = -1000;
pub const OOM_SCORE_ADJ_MAX: i32 = 1000;

// ============================================================================
// MEMORY CGROUP
// ============================================================================

/// Memory cgroup
#[derive(Debug)]
pub struct MemoryCgroup {
    /// Cgroup ID
    pub id: u64,
    /// Name/path
    pub name: String,
    /// Parent cgroup
    pub parent: Option<u64>,
    /// Children
    pub children: Mutex<Vec<u64>>,
    /// Processes in this cgroup
    pub processes: Mutex<Vec<u64>>,
    /// Memory limit
    pub limit: AtomicU64,
    /// Current usage
    pub usage: AtomicU64,
    /// Peak usage
    pub peak_usage: AtomicU64,
    /// Soft limit
    pub soft_limit: AtomicU64,
    /// Swap limit
    pub swap_limit: AtomicU64,
    /// Swap usage
    pub swap_usage: AtomicU64,
    /// OOM score adjustment
    pub oom_score_adj: AtomicI64,
    /// OOM kill enabled
    pub oom_kill_enable: AtomicBool,
    /// Memory events
    pub events: Mutex<MemoryEvents>,
    /// Stats
    pub stats: Mutex<MemoryStats>,
    /// Is root cgroup
    pub is_root: bool,
}

/// Memory events
#[derive(Clone, Debug, Default)]
pub struct MemoryEvents {
    /// Low events (below low threshold)
    pub low: u64,
    /// High events (above high threshold)
    pub high: u64,
    /// Max events (at limit)
    pub max: u64,
    /// OOM events
    pub oom: u64,
    /// OOM kill events
    pub oom_kill: u64,
    /// OOM group kill events
    pub oom_group_kill: u64,
}

/// Memory statistics
#[derive(Clone, Debug, Default)]
pub struct MemoryStats {
    /// Anonymous memory
    pub anon: u64,
    /// File cache
    pub file: u64,
    /// Kernel memory
    pub kernel: u64,
    /// Kernel stack
    pub kernel_stack: u64,
    /// Page tables
    pub pgtable: u64,
    /// Swap cache
    pub swap_cache: u64,
    /// Active anon
    pub active_anon: u64,
    /// Inactive anon
    pub inactive_anon: u64,
    /// Active file
    pub active_file: u64,
    /// Inactive file
    pub inactive_file: u64,
    /// Unevictable
    pub unevictable: u64,
    /// Slab reclaimable
    pub slab_reclaimable: u64,
    /// Slab unreclaimable
    pub slab_unreclaimable: u64,
    /// Workingset refault
    pub workingset_refault: u64,
    /// Workingset activate
    pub workingset_activate: u64,
    /// Workingset nodereclaim
    pub workingset_nodereclaim: u64,
    /// Page fault count
    pub pgfault: u64,
    /// Major fault count
    pub pgmajfault: u64,
    /// Refault count
    pub refault: u64,
}

impl MemoryCgroup {
    pub fn new(id: u64, name: &str, parent: Option<u64>, is_root: bool) -> Self {
        Self {
            id,
            name: String::from(name),
            parent,
            children: Mutex::new(Vec::new()),
            processes: Mutex::new(Vec::new()),
            limit: AtomicU64::new(MEMORY_LIMIT_UNLIMITED),
            usage: AtomicU64::new(0),
            peak_usage: AtomicU64::new(0),
            soft_limit: AtomicU64::new(MEMORY_LIMIT_UNLIMITED),
            swap_limit: AtomicU64::new(MEMORY_LIMIT_UNLIMITED),
            swap_usage: AtomicU64::new(0),
            oom_score_adj: AtomicI64::new(OOM_SCORE_ADJ_DEFAULT as i64),
            oom_kill_enable: AtomicBool::new(true),
            events: Mutex::new(MemoryEvents::default()),
            stats: Mutex::new(MemoryStats::default()),
            is_root,
        }
    }

    /// Add process to cgroup
    pub fn add_process(&self, pid: u64) {
        let mut procs = self.processes.lock();
        if !procs.contains(&pid) {
            procs.push(pid);
        }
    }

    /// Remove process from cgroup
    pub fn remove_process(&self, pid: u64) {
        self.processes.lock().retain(|&p| p != pid);
    }

    /// Charge memory to this cgroup
    pub fn charge(&self, bytes: u64) -> Result<(), CgroupError> {
        let limit = self.limit.load(Ordering::SeqCst);
        let current = self.usage.load(Ordering::SeqCst);
        let new_usage = current + bytes;
        
        // Check limit
        if limit != MEMORY_LIMIT_UNLIMITED && new_usage > limit {
            // Memory limit exceeded
            self.events.lock().max += 1;
            
            // Try to reclaim
            if self.try_reclaim(bytes) {
                return Ok(());
            }
            
            // Check if OOM kill is enabled
            if self.oom_kill_enable.load(Ordering::SeqCst) {
                self.trigger_oom();
            }
            
            return Err(CgroupError::MemoryLimitExceeded);
        }
        
        self.usage.store(new_usage, Ordering::SeqCst);
        
        // Update peak
        let peak = self.peak_usage.load(Ordering::SeqCst);
        if new_usage > peak {
            self.peak_usage.store(new_usage, Ordering::SeqCst);
        }
        
        // Propagate to parent
        // if let Some(parent_id) = self.parent {
        //     if let Some(parent) = CGROUP_MANAGER.get_cgroup(parent_id) {
        //         parent.charge(bytes);
        //     }
        // }
        
        Ok(())
    }

    /// Uncharge memory
    pub fn uncharge(&self, bytes: u64) {
        let current = self.usage.load(Ordering::SeqCst);
        let new_usage = current.saturating_sub(bytes);
        self.usage.store(new_usage, Ordering::SeqCst);
    }

    /// Try to reclaim memory
    fn try_reclaim(&self, needed: u64) -> bool {
        // Trigger memory reclaim for this cgroup
        // Would call into memory management
        false
    }

    /// Trigger OOM kill
    fn trigger_oom(&self) {
        let mut events = self.events.lock();
        events.oom += 1;
        events.oom_kill += 1;
        
        crate::serial_println!(
            "[CGROUP] OOM kill triggered for cgroup '{}' (usage: {}, limit: {})",
            self.name,
            self.usage.load(Ordering::SeqCst),
            self.limit.load(Ordering::SeqCst)
        );
        
        // Call OOM killer for this cgroup's processes
        // crate::memory::oom::oom_kill_cgroup(self);
    }

    /// Set memory limit
    pub fn set_limit(&self, limit: u64) {
        self.limit.store(limit, Ordering::SeqCst);
        
        // Check if we're already over limit
        let usage = self.usage.load(Ordering::SeqCst);
        if limit != MEMORY_LIMIT_UNLIMITED && usage > limit {
            self.try_reclaim(usage - limit);
        }
    }

    /// Get process count
    pub fn process_count(&self) -> usize {
        self.processes.lock().len()
    }

    /// Get child count
    pub fn child_count(&self) -> usize {
        self.children.lock().len()
    }
}

// ============================================================================
// CGROUP MANAGER
// ============================================================================

/// Cgroup manager
pub struct CgroupManager {
    /// All cgroups
    cgroups: RwLock<BTreeMap<u64, MemoryCgroup>>,
    /// Next cgroup ID
    next_id: AtomicU64,
    /// Root cgroup
    root_id: AtomicU64,
    /// Process to cgroup mapping
    proc_to_cgroup: Mutex<BTreeMap<u64, u64>>,
}

impl CgroupManager {
    pub const fn new() -> Self {
        Self {
            cgroups: RwLock::new(BTreeMap::new()),
            next_id: AtomicU64::new(1),
            root_id: AtomicU64::new(0),
            proc_to_cgroup: Mutex::new(BTreeMap::new()),
        }
    }

    /// Initialize with root cgroup
    pub fn init(&self) {
        let root = MemoryCgroup::new(0, "/", None, true);
        self.cgroups.write().insert(0, root);
        self.root_id.store(0, Ordering::SeqCst);
        
        crate::serial_println!("[CGROUP] Initialized root cgroup");
    }

    /// Create new cgroup
    pub fn create_cgroup(&self, name: &str, parent_id: u64) -> Result<u64, CgroupError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        
        // Verify parent exists
        {
            let cgroups = self.cgroups.read();
            if !cgroups.contains_key(&parent_id) {
                return Err(CgroupError::ParentNotFound);
            }
        }
        
        let cgroup = MemoryCgroup::new(id, name, Some(parent_id), false);
        
        // Add to parent's children
        {
            let cgroups = self.cgroups.read();
            if let Some(parent) = cgroups.get(&parent_id) {
                parent.children.lock().push(id);
            }
        }
        
        self.cgroups.write().insert(id, cgroup);
        
        crate::serial_println!("[CGROUP] Created cgroup '{}' (id={})", name, id);
        
        Ok(id)
    }

    /// Remove cgroup
    pub fn remove_cgroup(&self, id: u64) -> Result<(), CgroupError> {
        let cgroups = self.cgroups.read();
        
        if let Some(cgroup) = cgroups.get(&id) {
            // Cannot remove if has processes
            if cgroup.process_count() > 0 {
                return Err(CgroupError::NotEmpty);
            }
            
            // Cannot remove if has children
            if cgroup.child_count() > 0 {
                return Err(CgroupError::HasChildren);
            }
            
            // Remove from parent
            if let Some(parent_id) = cgroup.parent {
                if let Some(parent) = cgroups.get(&parent_id) {
                    parent.children.lock().retain(|&c| c != id);
                }
            }
        }
        
        drop(cgroups);
        self.cgroups.write().remove(&id);
        
        Ok(())
    }

    /// Get cgroup by ID
    pub fn get_cgroup(&self, id: u64) -> Option<MemoryCgroup> {
        self.cgroups.read().get(&id).cloned()
    }

    /// Move process to cgroup
    pub fn move_process(&self, pid: u64, cgroup_id: u64) -> Result<(), CgroupError> {
        // Remove from old cgroup
        if let Some(old_id) = self.proc_to_cgroup.lock().get(&pid).copied() {
            if let Some(old_cgroup) = self.get_cgroup(old_id) {
                old_cgroup.remove_process(pid);
            }
        }
        
        // Add to new cgroup
        let cgroup = self.get_cgroup(cgroup_id).ok_or(CgroupError::NotFound)?;
        cgroup.add_process(pid);
        self.proc_to_cgroup.lock().insert(pid, cgroup_id);
        
        Ok(())
    }

    /// Get cgroup for process
    pub fn get_cgroup_for_process(&self, pid: u64) -> Option<u64> {
        self.proc_to_cgroup.lock().get(&pid).copied()
    }

    /// Charge memory for process
    pub fn charge_process(&self, pid: u64, bytes: u64) -> Result<(), CgroupError> {
        let cgroup_id = self.get_cgroup_for_process(pid).unwrap_or(0);
        
        if let Some(cgroup) = self.get_cgroup(cgroup_id) {
            cgroup.charge(bytes)?;
        }
        
        Ok(())
    }

    /// Uncharge memory for process
    pub fn uncharge_process(&self, pid: u64, bytes: u64) {
        let cgroup_id = self.get_cgroup_for_process(pid).unwrap_or(0);
        
        if let Some(cgroup) = self.get_cgroup(cgroup_id) {
            cgroup.uncharge(bytes);
        }
    }

    /// Get all cgroups
    pub fn list_cgroups(&self) -> Vec<(u64, String)> {
        self.cgroups.read()
            .iter()
            .map(|(id, cg)| (*id, cg.name.clone()))
            .collect()
    }
}

// Clone implementation for MemoryCgroup (needed for get_cgroup)
impl Clone for MemoryCgroup {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            name: self.name.clone(),
            parent: self.parent,
            children: Mutex::new(self.children.lock().clone()),
            processes: Mutex::new(self.processes.lock().clone()),
            limit: AtomicU64::new(self.limit.load(Ordering::SeqCst)),
            usage: AtomicU64::new(self.usage.load(Ordering::SeqCst)),
            peak_usage: AtomicU64::new(self.peak_usage.load(Ordering::SeqCst)),
            soft_limit: AtomicU64::new(self.soft_limit.load(Ordering::SeqCst)),
            swap_limit: AtomicU64::new(self.swap_limit.load(Ordering::SeqCst)),
            swap_usage: AtomicU64::new(self.swap_usage.load(Ordering::SeqCst)),
            oom_score_adj: AtomicI64::new(self.oom_score_adj.load(Ordering::SeqCst)),
            oom_kill_enable: AtomicBool::new(self.oom_kill_enable.load(Ordering::SeqCst)),
            events: Mutex::new(self.events.lock().clone()),
            stats: Mutex::new(self.stats.lock().clone()),
            is_root: self.is_root,
        }
    }
}

lazy_static::lazy_static! {
    /// Global cgroup manager
    pub static ref CGROUP_MANAGER: CgroupManager = CgroupManager::new();
}

// ============================================================================
// ERROR TYPE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CgroupError {
    NotFound,
    ParentNotFound,
    NotEmpty,
    HasChildren,
    MemoryLimitExceeded,
    PermissionDenied,
}

// ============================================================================
// INITIALIZATION
// ============================================================================

/// Initialize cgroup subsystem
pub fn init() {
    CGROUP_MANAGER.init();
    crate::serial_println!("[CGROUP] Subsystem initialized");
}

/// Create cgroup
pub fn create(name: &str, parent: u64) -> Result<u64, CgroupError> {
    CGROUP_MANAGER.create_cgroup(name, parent)
}

/// Get cgroup stats
pub fn get_stats(cgroup_id: u64) -> Option<MemoryStats> {
    CGROUP_MANAGER.get_cgroup(cgroup_id).map(|cg| cg.stats.lock().clone())
}
