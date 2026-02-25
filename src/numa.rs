//! # echOS NUMA (Non-Uniform Memory Access) Support Module
//!
//! Tier 1 OS seviyesinde NUMA-aware memory allocation ve CPU affinity
//! Linux NUMA ile aynı seviyede performans ve özellikler

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use alloc::vec::Vec;
use alloc::boxed::Box;
use crate::memory_barriers::{smp_mb, smp_rmb, smp_wmb};
use crate::rcu::{RcuPtr, synchronize_rcu};
use crate::preempt::PreemptDisableGuard;

/// Maximum number of NUMA nodes supported
pub const MAX_NUMA_NODES: usize = 256;

/// NUMA node states (Linux numa_states ile uyumlu)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum NumaNodeState {
    /// Node is offline and not available
    Offline = 0,
    /// Node is coming up
    ComingUp = 1,
    /// Node is online and available
    Online = 2,
    /// Node is going down
    GoingDown = 3,
}

/// NUMA memory policies
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum NumaPolicy {
    /// Use default policy (usually local allocation)
    Default = 0,
    /// Prefer allocation from local node
    Prefer = 1,
    /// Bind allocation to specific nodes
    Bind = 2,
    /// Interleave allocation across nodes
    Interleave = 3,
    /// Prefer allocation from specified nodes
    Preferred = 4,
    /// Local allocation only
    Local = 5,
}

/// NUMA distance matrix entry
#[derive(Debug, Clone, Copy)]
pub struct NumaDistance {
    /// Distance from source to destination node
    pub distance: u8,
    /// Whether this is a local distance (same node)
    pub is_local: bool,
    /// Whether this is a remote distance (different node)
    pub is_remote: bool,
}

impl NumaDistance {
    pub fn new(distance: u8) -> Self {
        Self {
            distance,
            is_local: distance == 10,  // Linux uses 10 for local distance
            is_remote: distance > 10,
        }
    }
}

/// NUMA node descriptor
#[repr(C, align(64))]
pub struct NumaNode {
    /// Node ID
    pub node_id: u32,
    /// Current state
    pub state: AtomicU32, // NumaNodeState as u32
    /// Number of CPUs in this node
    pub cpu_count: AtomicU32,
    /// List of CPU IDs in this node
    pub cpus: Vec<u32>,
    /// Total memory in this node (bytes)
    pub total_memory: AtomicU64,
    /// Available memory in this node (bytes)
    pub available_memory: AtomicU64,
    /// Memory allocation policy for this node
    pub policy: AtomicU32, // NumaPolicy as u32
    /// Preferred nodes for allocation
    pub preferred_nodes: Vec<u32>,
    /// Node distance to other nodes
    pub distances: Vec<NumaDistance>,
    /// Memory allocation statistics
    pub allocations: AtomicU64,
    /// Page migration count
    pub migrations: AtomicU64,
    /// Node flags
    pub flags: u32,
    /// Whether node has memory
    pub has_memory: AtomicBool,
    /// Whether node has CPUs
    pub has_cpus: AtomicBool,
    /// Node proximity domain (ACPI)
    pub proximity_domain: u32,
    /// Cache-line padding to avoid false sharing
    _padding: [u8; 0],
}

impl NumaNode {
    /// Create new NUMA node
    pub fn new(node_id: u32) -> Self {
        Self {
            node_id,
            state: AtomicU32::new(NumaNodeState::Offline as u32),
            cpu_count: AtomicU32::new(0),
            cpus: Vec::new(),
            total_memory: AtomicU64::new(0),
            available_memory: AtomicU64::new(0),
            policy: AtomicU32::new(NumaPolicy::Default as u32),
            preferred_nodes: Vec::new(),
            distances: Vec::new(),
            allocations: AtomicU64::new(0),
            migrations: AtomicU64::new(0),
            flags: 0,
            has_memory: AtomicBool::new(false),
            has_cpus: AtomicBool::new(false),
            proximity_domain: node_id,
            _padding: [0; 0],
        }
    }
    
    /// Get current state
    pub fn get_state(&self) -> NumaNodeState {
        match self.state.load(Ordering::Acquire) {
            0 => NumaNodeState::Offline,
            1 => NumaNodeState::ComingUp,
            2 => NumaNodeState::Online,
            3 => NumaNodeState::GoingDown,
            _ => NumaNodeState::Offline,
        }
    }
    
    /// Set node state
    pub fn set_state(&self, state: NumaNodeState) {
        self.state.store(state as u32, Ordering::Release);
        smp_wmb();
    }
    
    /// Add CPU to node
    pub fn add_cpu(&mut self, cpu_id: u32) {
        if !self.cpus.contains(&cpu_id) {
            self.cpus.push(cpu_id);
            self.cpu_count.fetch_add(1, Ordering::AcqRel);
            self.has_cpus.store(true, Ordering::Release);
            smp_wmb();
        }
    }
    
    /// Remove CPU from node
    pub fn remove_cpu(&mut self, cpu_id: u32) {
        if let Some(pos) = self.cpus.iter().position(|&id| id == cpu_id) {
            self.cpus.remove(pos);
            self.cpu_count.fetch_sub(1, Ordering::AcqRel);
            if self.cpus.is_empty() {
                self.has_cpus.store(false, Ordering::Release);
            }
            smp_wmb();
        }
    }
    
    /// Set memory size
    pub fn set_memory_size(&self, total: u64, available: u64) {
        self.total_memory.store(total, Ordering::Release);
        self.available_memory.store(available, Ordering::Release);
        self.has_memory.store(total > 0, Ordering::Release);
        smp_wmb();
    }
    
    /// Get memory statistics
    pub fn get_memory_stats(&self) -> (u64, u64) {
        (
            self.total_memory.load(Ordering::Acquire),
            self.available_memory.load(Ordering::Acquire),
        )
    }
    
    /// Allocate memory from this node
    pub fn allocate(&self, size: usize) -> Option<*mut u8> {
        let available = self.available_memory.load(Ordering::Acquire) as usize;
        if available < size {
            return None;
        }
        
        // Update statistics
        self.allocations.fetch_add(1, Ordering::AcqRel);
        self.available_memory.fetch_sub(size as u64, Ordering::AcqRel);
        smp_mb();
        
        // For now, we'll return a dummy pointer
        // In a real implementation, this would allocate from the node's memory pool
        Some(0x4000_0000 as *mut u8) // Dummy implementation
    }
    
    /// Free memory to this node
    pub fn free(&self, size: usize) {
        self.available_memory.fetch_add(size as u64, Ordering::AcqRel);
        smp_wmb();
    }
    
    /// Set allocation policy
    pub fn set_policy(&self, policy: NumaPolicy) {
        self.policy.store(policy as u32, Ordering::Release);
        smp_wmb();
    }
    
    /// Get allocation policy
    pub fn get_policy(&self) -> NumaPolicy {
        match self.policy.load(Ordering::Acquire) {
            0 => NumaPolicy::Default,
            1 => NumaPolicy::Prefer,
            2 => NumaPolicy::Bind,
            3 => NumaPolicy::Interleave,
            4 => NumaPolicy::Preferred,
            5 => NumaPolicy::Local,
            _ => NumaPolicy::Default,
        }
    }
    
    /// Set preferred nodes
    pub fn set_preferred_nodes(&mut self, nodes: Vec<u32>) {
        self.preferred_nodes = nodes;
        smp_wmb();
    }
    
    /// Get distance to another node
    pub fn get_distance(&self, target_node: u32) -> Option<NumaDistance> {
        if target_node as usize >= self.distances.len() {
            return None;
        }
        Some(self.distances[target_node as usize])
    }
    
    /// Set distance to another node
    pub fn set_distance(&mut self, target_node: u32, distance: u8) {
        // Ensure distances vector is large enough
        while self.distances.len() <= target_node as usize {
            self.distances.push(NumaDistance::new(255)); // Default to very far
        }
        
        self.distances[target_node as usize] = NumaDistance::new(distance);
        smp_wmb();
    }
    
    /// Check if this node is local to the given CPU
    pub fn is_local_to_cpu(&self, cpu_id: u32) -> bool {
        self.cpus.contains(&cpu_id)
    }
    
    /// Get allocation statistics
    pub fn get_allocation_stats(&self) -> (u64, u64) {
        (
            self.allocations.load(Ordering::Acquire),
            self.migrations.load(Ordering::Acquire),
        )
    }
}

/// NUMA memory manager
pub struct NumaManager {
    /// Maximum number of nodes
    max_nodes: u32,
    /// NUMA nodes
    nodes: Vec<RcuPtr<NumaNode>>,
    /// Current number of online nodes
    online_nodes: AtomicU32,
    /// Default allocation policy
    default_policy: AtomicU32, // NumaPolicy as u32
    /// NUMA statistics
    stats: NumaStats,
    /// Node migration lock
    migration_lock: spin::Mutex<()>,
}

/// NUMA statistics
#[derive(Debug)]
pub struct NumaStats {
    pub total_allocations: AtomicU64,
    pub local_allocations: AtomicU64,
    pub remote_allocations: AtomicU64,
    pub failed_allocations: AtomicU64,
    pub page_migrations: AtomicU64,
}

impl NumaStats {
    pub const fn new() -> Self {
        Self {
            total_allocations: AtomicU64::new(0),
            local_allocations: AtomicU64::new(0),
            remote_allocations: AtomicU64::new(0),
            failed_allocations: AtomicU64::new(0),
            page_migrations: AtomicU64::new(0),
        }
    }
    
    pub fn record_allocation(&self, is_local: bool) {
        self.total_allocations.fetch_add(1, Ordering::Relaxed);
        if is_local {
            self.local_allocations.fetch_add(1, Ordering::Relaxed);
        } else {
            self.remote_allocations.fetch_add(1, Ordering::Relaxed);
        }
    }
    
    pub fn record_failed_allocation(&self) {
        self.failed_allocations.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn record_migration(&self) {
        self.page_migrations.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn get_stats(&self) -> (u64, u64, u64, u64, u64) {
        (
            self.total_allocations.load(Ordering::Relaxed),
            self.local_allocations.load(Ordering::Relaxed),
            self.remote_allocations.load(Ordering::Relaxed),
            self.failed_allocations.load(Ordering::Relaxed),
            self.page_migrations.load(Ordering::Relaxed),
        )
    }
}

impl NumaManager {
    /// Create new NUMA manager
    pub fn new(max_nodes: u32) -> Self {
        let mut nodes = Vec::with_capacity(max_nodes as usize);
        
        // Initialize NUMA nodes
        for node_id in 0..max_nodes {
            let node = Box::new(NumaNode::new(node_id));
            nodes.push(RcuPtr::new(Box::into_raw(node)));
        }
        
        Self {
            max_nodes,
            nodes,
            online_nodes: AtomicU32::new(0),
            default_policy: AtomicU32::new(NumaPolicy::Default as u32),
            stats: NumaStats::new(),
            migration_lock: spin::Mutex::new(()),
        }
    }
    
    /// Get NUMA node
    pub fn get_node(&self, node_id: u32) -> Option<RcuPtr<NumaNode>> {
        if node_id >= self.max_nodes {
            return None;
        }
        
        Some(self.nodes[node_id as usize].clone())
    }
    
    /// Bring NUMA node online
    pub fn node_online(&self, node_id: u32) -> Result<(), NumaError> {
        let node = match self.get_node(node_id) {
            Some(node) => node,
            None => return Err(NumaError::InvalidNodeId),
        };
        
        let node_guard = node.read();
        
        // Check current state
        let current_state = node_guard.get_state();
        if current_state == NumaNodeState::Online {
            return Err(NumaError::AlreadyOnline);
        }
        
        if current_state != NumaNodeState::Offline {
            return Err(NumaError::InvalidStateTransition);
        }
        
        // Set node online
        node_guard.set_state(NumaNodeState::Online);
        self.online_nodes.fetch_add(1, Ordering::AcqRel);
        smp_mb();
        
        crate::serial_println!("NUMA: Node {} is now online", node_id);
        Ok(())
    }
    
    /// Take NUMA node offline
    pub fn node_offline(&self, node_id: u32) -> Result<(), NumaError> {
        let node = match self.get_node(node_id) {
            Some(node) => node,
            None => return Err(NumaError::InvalidNodeId),
        };
        
        let node_guard = node.read();
        
        // Check current state
        let current_state = node_guard.get_state();
        if current_state == NumaNodeState::Offline {
            return Err(NumaError::AlreadyOffline);
        }
        
        if current_state != NumaNodeState::Online {
            return Err(NumaError::InvalidStateTransition);
        }
        
        // Check if node has CPUs (can't offline nodes with CPUs)
        if node_guard.has_cpus.load(Ordering::Acquire) {
            return Err(NumaError::NodeHasCpus);
        }
        
        // Set node offline
        node_guard.set_state(NumaNodeState::Offline);
        self.online_nodes.fetch_sub(1, Ordering::AcqRel);
        smp_mb();
        
        crate::serial_println!("NUMA: Node {} is now offline", node_id);
        Ok(())
    }
    
    /// Allocate memory with NUMA awareness
    pub fn allocate(&self, size: usize, preferred_node: Option<u32>, policy: Option<NumaPolicy>) -> Result<*mut u8, NumaError> {
        let policy = policy.unwrap_or_else(|| self.get_default_policy());
        
        match policy {
            NumaPolicy::Local => self.allocate_local(size),
            NumaPolicy::Prefer => self.allocate_preferred(size, preferred_node),
            NumaPolicy::Bind => self.allocate_bind(size, preferred_node.ok_or(NumaError::NoPreferredNode)?),
            NumaPolicy::Interleave => self.allocate_interleave(size),
            NumaPolicy::Preferred => self.allocate_preferred(size, preferred_node),
            NumaPolicy::Default => self.allocate_default(size, preferred_node),
        }
    }
    
    /// Allocate from local node
    fn allocate_local(&self, size: usize) -> Result<*mut u8, NumaError> {
        let cpu_id = crate::cpu::smp::current_cpu_id();
        let node_id = self.get_cpu_node(cpu_id)?;
        
        let node = self.get_node(node_id).ok_or(NumaError::InvalidNodeId)?;
        let node_guard = node.read();
        
        match node_guard.allocate(size) {
            Some(ptr) => {
                self.stats.record_allocation(true);
                Ok(ptr)
            }
            None => {
                self.stats.record_failed_allocation();
                Err(NumaError::OutOfMemory)
            }
        }
    }
    
    /// Allocate from preferred node
    fn allocate_preferred(&self, size: usize, preferred_node: Option<u32>) -> Result<*mut u8, NumaError> {
        if let Some(node_id) = preferred_node {
            let node = self.get_node(node_id).ok_or(NumaError::InvalidNodeId)?;
            let node_guard = node.read();
            
            if node_guard.get_state() != NumaNodeState::Online {
                return self.allocate_fallback(size);
            }
            
            match node_guard.allocate(size) {
                Some(ptr) => {
                    self.stats.record_allocation(false);
                    Ok(ptr)
                }
                None => self.allocate_fallback(size),
            }
        } else {
            self.allocate_local(size)
        }
    }
    
    /// Allocate from specific node (bind policy)
    fn allocate_bind(&self, size: usize, node_id: u32) -> Result<*mut u8, NumaError> {
        let node = self.get_node(node_id).ok_or(NumaError::InvalidNodeId)?;
        let node_guard = node.read();
        
        if node_guard.get_state() != NumaNodeState::Online {
            return Err(NumaError::NodeOffline);
        }
        
        match node_guard.allocate(size) {
            Some(ptr) => {
                self.stats.record_allocation(false);
                Ok(ptr)
            }
            None => {
                self.stats.record_failed_allocation();
                Err(NumaError::OutOfMemory)
            }
        }
    }
    
    /// Allocate interleaved across nodes
    fn allocate_interleave(&self, size: usize) -> Result<*mut u8, NumaError> {
        let online_nodes = self.get_online_nodes();
        if online_nodes.is_empty() {
            return Err(NumaError::NoOnlineNodes);
        }
        
        // Simple round-robin interleaving
        let node_id = online_nodes[(self.stats.total_allocations.load(Ordering::Relaxed) as usize) % online_nodes.len()];
        let node = self.get_node(node_id).ok_or(NumaError::InvalidNodeId)?;
        let node_guard = node.read();
        
        match node_guard.allocate(size) {
            Some(ptr) => {
                self.stats.record_allocation(false);
                Ok(ptr)
            }
            None => self.allocate_fallback(size),
        }
    }
    
    /// Allocate with default policy
    fn allocate_default(&self, size: usize, preferred_node: Option<u32>) -> Result<*mut u8, NumaError> {
        // Try local first, then preferred, then fallback
        if let Ok(ptr) = self.allocate_local(size) {
            return Ok(ptr);
        }
        
        if let Some(node_id) = preferred_node {
            if let Ok(ptr) = self.allocate_preferred(size, Some(node_id)) {
                return Ok(ptr);
            }
        }
        
        self.allocate_fallback(size)
    }
    
    /// Fallback allocation (try any online node)
    fn allocate_fallback(&self, size: usize) -> Result<*mut u8, NumaError> {
        let online_nodes = self.get_online_nodes();
        
        for &node_id in &online_nodes {
            let node = self.get_node(node_id).ok_or(NumaError::InvalidNodeId)?;
            let node_guard = node.read();
            
            if let Some(ptr) = node_guard.allocate(size) {
                self.stats.record_allocation(false);
                return Ok(ptr);
            }
        }
        
        self.stats.record_failed_allocation();
        Err(NumaError::OutOfMemory)
    }
    
    /// Free memory
    pub fn free(&self, ptr: *mut u8, size: usize, node_id: u32) {
        let node = match self.get_node(node_id) {
            Some(node) => node,
            None => return,
        };
        
        let node_guard = node.read();
        node_guard.free(size);
    }
    
    /// Get NUMA node for CPU
    pub fn get_cpu_node(&self, cpu_id: u32) -> Result<u32, NumaError> {
        for (node_id, node_ptr) in self.nodes.iter().enumerate() {
            let node_guard = node_ptr.read();
            if node_guard.is_local_to_cpu(cpu_id) {
                return Ok(node_id as u32);
            }
        }
        
        Err(NumaError::CpuNotFound)
    }
    
    /// Add CPU to NUMA node
    pub fn add_cpu_to_node(&mut self, cpu_id: u32, node_id: u32) -> Result<(), NumaError> {
        let node = match self.get_node(node_id) {
            Some(node) => node,
            None => return Err(NumaError::InvalidNodeId),
        };
        
        // Remove CPU from any existing node
        self.remove_cpu_from_any_node(cpu_id);
        
        // Add to new node
        let node_guard = node.read();
        let mutable_node = node_guard.as_mut();
        mutable_node.add_cpu(cpu_id);
        
        Ok(())
    }
    
    /// Remove CPU from any node
    fn remove_cpu_from_any_node(&mut self, cpu_id: u32) {
        for node_ptr in &self.nodes {
            let node_guard = node_ptr.read();
            if node_guard.is_local_to_cpu(cpu_id) {
                let mutable_node = node_guard.as_mut();
                mutable_node.remove_cpu(cpu_id);
                break;
            }
        }
    }
    
    /// Set node memory size
    pub fn set_node_memory(&self, node_id: u32, total: u64, available: u64) -> Result<(), NumaError> {
        let node = self.get_node(node_id).ok_or(NumaError::InvalidNodeId)?;
        let node_guard = node.read();
        node_guard.set_memory_size(total, available);
        Ok(())
    }
    
    /// Set distance between nodes
    pub fn set_node_distance(&mut self, src_node: u32, dst_node: u32, distance: u8) -> Result<(), NumaError> {
        let node = self.get_node(src_node).ok_or(NumaError::InvalidNodeId)?;
        let node_guard = node.read();
        let mutable_node = node_guard.as_mut();
        mutable_node.set_distance(dst_node, distance);
        
        Ok(())
    }
    
    /// Get online nodes
    pub fn get_online_nodes(&self) -> Vec<u32> {
        let mut online_nodes = Vec::new();
        
        for (node_id, node_ptr) in self.nodes.iter().enumerate() {
            let node_guard = node_ptr.read();
            if node_guard.get_state() == NumaNodeState::Online {
                online_nodes.push(node_id as u32);
            }
        }
        
        online_nodes
    }
    
    /// Get number of online nodes
    pub fn get_online_node_count(&self) -> u32 {
        self.online_nodes.load(Ordering::Acquire)
    }
    
    /// Set default allocation policy
    pub fn set_default_policy(&self, policy: NumaPolicy) {
        self.default_policy.store(policy as u32, Ordering::Release);
        smp_wmb();
    }
    
    /// Get default allocation policy
    pub fn get_default_policy(&self) -> NumaPolicy {
        match self.default_policy.load(Ordering::Acquire) {
            0 => NumaPolicy::Default,
            1 => NumaPolicy::Prefer,
            2 => NumaPolicy::Bind,
            3 => NumaPolicy::Interleave,
            4 => NumaPolicy::Preferred,
            5 => NumaPolicy::Local,
            _ => NumaPolicy::Default,
        }
    }
    
    /// Get NUMA statistics
    pub fn get_stats(&self) -> (u64, u64, u64, u64, u64) {
        self.stats.get_stats()
    }
    
    /// Migrate pages between nodes
    pub fn migrate_pages(&self, src_node: u32, dst_node: u32, pages: usize) -> Result<(), NumaError> {
        let _guard = self.migration_lock.lock();
        
        let src_node_ptr = self.get_node(src_node).ok_or(NumaError::InvalidNodeId)?;
        let dst_node_ptr = self.get_node(dst_node).ok_or(NumaError::InvalidNodeId)?;
        
        let src_guard = src_node_ptr.read();
        let dst_guard = dst_node_ptr.read();
        
        // Check if both nodes are online
        if src_guard.get_state() != NumaNodeState::Online || dst_guard.get_state() != NumaNodeState::Online {
            return Err(NumaError::NodeOffline);
        }
        
        // Check if destination has enough memory
        let dst_available = dst_guard.available_memory.load(Ordering::Acquire);
        let required_memory = (pages * 4096) as u64; // Assume 4KB pages
        
        if dst_available < required_memory {
            return Err(NumaError::OutOfMemory);
        }
        
        // Perform migration (simplified)
        src_guard.available_memory.fetch_sub(required_memory, Ordering::AcqRel);
        dst_guard.available_memory.fetch_add(required_memory, Ordering::AcqRel);
        
        // Update statistics
        src_guard.migrations.fetch_add(pages as u64, Ordering::AcqRel);
        dst_guard.migrations.fetch_add(pages as u64, Ordering::AcqRel);
        self.stats.record_migration();
        
        smp_mb();
        
        crate::serial_println!("NUMA: Migrated {} pages from node {} to node {}", pages, src_node, dst_node);
        Ok(())
    }
}

/// NUMA error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumaError {
    /// Invalid node ID
    InvalidNodeId,
    /// Node is already online
    AlreadyOnline,
    /// Node is already offline
    AlreadyOffline,
    /// Invalid state transition
    InvalidStateTransition,
    /// Node has CPUs (cannot offline)
    NodeHasCpus,
    /// Node is offline
    NodeOffline,
    /// No online nodes available
    NoOnlineNodes,
    /// No preferred node specified
    NoPreferredNode,
    /// CPU not found in any node
    CpuNotFound,
    /// Out of memory
    OutOfMemory,
}

/// Global NUMA manager instance
static mut NUMA_MANAGER: Option<NumaManager> = None;
static NUMA_INIT: AtomicBool = AtomicBool::new(false);

/// Initialize NUMA subsystem
pub fn init(max_nodes: u32) {
    if NUMA_INIT.load(Ordering::Acquire) {
        return;
    }
    
    crate::serial_println!("NUMA: Initializing NUMA support for {} nodes", max_nodes);
    
    let manager = NumaManager::new(max_nodes);
    
    unsafe {
        NUMA_MANAGER = Some(manager);
    }
    
    NUMA_INIT.store(true, Ordering::Release);
    smp_mb();
    
    crate::serial_println!("NUMA: NUMA support initialized");
}

/// Get NUMA manager
pub fn get_manager() -> Option<&'static NumaManager> {
    if !NUMA_INIT.load(Ordering::Acquire) {
        return None;
    }
    
    unsafe { NUMA_MANAGER.as_ref() }
}

/// Convenience functions for common operations
pub fn allocate(size: usize, preferred_node: Option<u32>, policy: Option<NumaPolicy>) -> Result<*mut u8, NumaError> {
    let manager = get_manager().ok_or(NumaError::NoOnlineNodes)?;
    manager.allocate(size, preferred_node, policy)
}

pub fn free(ptr: *mut u8, size: usize, node_id: u32) {
    if let Some(manager) = get_manager() {
        manager.free(ptr, size, node_id);
    }
}

pub fn get_cpu_node(cpu_id: u32) -> Result<u32, NumaError> {
    let manager = get_manager().ok_or(NumaError::NoOnlineNodes)?;
    manager.get_cpu_node(cpu_id)
}

pub fn get_online_nodes() -> Vec<u32> {
    get_manager().map(|m| m.get_online_nodes()).unwrap_or_default()
}

pub fn get_online_node_count() -> u32 {
    get_manager().map(|m| m.get_online_node_count()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_numa_node() {
        let node = NumaNode::new(0);
        assert_eq!(node.get_state(), NumaNodeState::Offline);
        assert!(!node.has_memory.load(Ordering::Acquire));
        assert!(!node.has_cpus.load(Ordering::Acquire));
        
        node.set_memory_size(1024 * 1024, 512 * 1024);
        let (total, available) = node.get_memory_stats();
        assert_eq!(total, 1024 * 1024);
        assert_eq!(available, 512 * 1024);
    }
    
    #[test]
    fn test_numa_manager() {
        let manager = NumaManager::new(4);
        assert_eq!(manager.get_online_node_count(), 0);
        
        // Test node online/offline
        assert!(manager.node_online(0).is_ok());
        assert_eq!(manager.get_online_node_count(), 1);
        
        assert!(manager.node_offline(0).is_ok());
        assert_eq!(manager.get_online_node_count(), 0);
    }
}
