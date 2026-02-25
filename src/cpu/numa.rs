//! # NUMA Topology Support
//!
//! ACPI SRAT/SLIT table parsing for NUMA (Non-Uniform Memory Access) awareness.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// NUMA CONSTANTS
// ============================================================================

/// Maximum NUMA nodes
pub const MAX_NUMA_NODES: usize = 256;
/// Maximum distance value
pub const NUMA_DISTANCE_MAX: u8 = 255;
/// Local distance
pub const NUMA_DISTANCE_LOCAL: u8 = 10;

// ============================================================================
// ACPI TABLE SIGNATURES
// ============================================================================

/// SRAT signature
pub const SRAT_SIGNATURE: [u8; 4] = *b"SRAT";
/// SLIT signature
pub const SLIT_SIGNATURE: [u8; 4] = *b"SLIT";

// ============================================================================
// SRAT STRUCTURES
// ============================================================================

/// SRAT Table Header
#[repr(C, packed)]
pub struct SratHeader {
    pub signature: [u8; 4],
    pub length: u32,
    pub revision: u8,
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub oem_table_id: [u8; 8],
    pub oem_revision: u32,
    pub creator_id: [u8; 4],
    pub creator_revision: u32,
}

/// SRAT Subtable Type
#[derive(Clone, Copy, Debug)]
pub enum SratType {
    ProcessorLocalAPIC = 0,
    MemoryAffinity = 1,
    ProcessorLocalX2APIC = 2,
    GiccAffinity = 3,
    GicItsAffinity = 4,
    GenericInitiatorAffinity = 5,
}

/// SRAT Processor Local APIC Affinity
#[repr(C, packed)]
pub struct SratProcessorLocalApic {
    pub header_type: u8,
    pub length: u8,
    pub reserved: [u8; 1],
    pub domain: u8,
    pub apic_id: u8,
    pub flags: u32,
    pub local_sapic_eid: u8,
    pub reserved2: [u8; 3],
}

/// SRAT Memory Affinity
#[repr(C, packed)]
pub struct SratMemoryAffinity {
    pub header_type: u8,
    pub length: u8,
    pub domain: u32,
    pub reserved1: [u8; 2],
    pub base_address: u64,
    pub length: u64,
    pub reserved2: [u8; 4],
    pub flags: u32,
    pub reserved3: [u8; 4],
}

/// SRAT flags
pub const SRAT_FLAG_ENABLED: u32 = 1 << 0;
pub const SRAT_FLAG_HOTPLUGGABLE: u32 = 1 << 1;
pub const SRAT_FLAG_NON_VOLATILE: u32 = 1 << 2;

// ============================================================================
// SLIT STRUCTURES
// ============================================================================

/// SLIT Table Header
#[repr(C, packed)]
pub struct SlitHeader {
    pub signature: [u8; 4],
    pub length: u32,
    pub revision: u8,
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub oem_table_id: [u8; 8],
    pub oem_revision: u32,
    pub creator_id: [u8; 4],
    pub creator_revision: u32,
    pub locality_count: u64,
    // Followed by distance matrix: entry[i][j] = distance from node i to j
}

// ============================================================================
// NUMA NODE
// ============================================================================

/// NUMA Node information
#[derive(Clone, Debug)]
pub struct NumaNode {
    /// Node ID
    pub id: u32,
    /// CPUs in this node (APIC IDs)
    pub cpus: Vec<u32>,
    /// Memory ranges in this node
    pub memory_ranges: Vec<MemoryRange>,
    /// Total memory in bytes
    pub total_memory: u64,
    /// Free memory in bytes
    pub free_memory: AtomicU64,
    /// Distance to other nodes
    pub distances: Vec<u8>,
    /// Is this node online?
    pub online: bool,
}

/// Memory range in a NUMA node
#[derive(Clone, Debug)]
pub struct MemoryRange {
    pub base: u64,
    pub length: u64,
    pub hotpluggable: bool,
    pub non_volatile: bool,
}

impl NumaNode {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            cpus: Vec::new(),
            memory_ranges: Vec::new(),
            total_memory: 0,
            free_memory: AtomicU64::new(0),
            distances: Vec::new(),
            online: true,
        }
    }

    /// Add CPU to this node
    pub fn add_cpu(&mut self, apic_id: u32) {
        if !self.cpus.contains(&apic_id) {
            self.cpus.push(apic_id);
        }
    }

    /// Add memory range to this node
    pub fn add_memory(&mut self, base: u64, length: u64, flags: u32) {
        let range = MemoryRange {
            base,
            length,
            hotpluggable: (flags & SRAT_FLAG_HOTPLUGGABLE) != 0,
            non_volatile: (flags & SRAT_FLAG_NON_VOLATILE) != 0,
        };
        self.total_memory += length;
        self.memory_ranges.push(range);
    }

    /// Get distance to another node
    pub fn distance_to(&self, other_node: u32) -> u8 {
        if other_node as usize >= self.distances.len() {
            return NUMA_DISTANCE_MAX;
        }
        self.distances[other_node as usize]
    }
}

// ============================================================================
// NUMA MANAGER
// ============================================================================

/// Global NUMA topology manager
pub struct NumaManager {
    /// Nodes indexed by node ID
    nodes: Mutex<BTreeMap<u32, NumaNode>>,
    /// CPU to node mapping
    cpu_to_node: Mutex<BTreeMap<u32, u32>>,
    /// Number of nodes
    node_count: AtomicU32,
    /// Is NUMA available?
    numa_available: AtomicU32,
}

impl NumaManager {
    pub const fn new() -> Self {
        Self {
            nodes: Mutex::new(BTreeMap::new()),
            cpu_to_node: Mutex::new(BTreeMap::new()),
            node_count: AtomicU32::new(0),
            numa_available: AtomicU32::new(0),
        }
    }

    /// Parse SRAT table
    pub fn parse_srat(&self, srat_ptr: *const u8) -> Result<(), NumaError> {
        unsafe {
            let header = &*(srat_ptr as *const SratHeader);
            
            // Verify signature
            if header.signature != SRAT_SIGNATURE {
                return Err(NumaError::InvalidSignature);
            }
            
            let length = header.length as usize;
            let mut offset = core::mem::size_of::<SratHeader>();
            
            while offset < length {
                let entry_ptr = srat_ptr.add(offset);
                let entry_type = *entry_ptr;
                let entry_len = *(entry_ptr.add(1)) as usize;
                
                if entry_len == 0 {
                    break;
                }
                
                match entry_type {
                    0 => {
                        // Processor Local APIC
                        let proc = &*(entry_ptr as *const SratProcessorLocalApic);
                        if proc.flags & SRAT_FLAG_ENABLED != 0 {
                            let node = self.get_or_create_node(proc.domain as u32);
                            node.add_cpu(proc.apic_id as u32);
                            self.cpu_to_node.lock().insert(proc.apic_id as u32, proc.domain as u32);
                        }
                    }
                    1 => {
                        // Memory Affinity
                        let mem = &*(entry_ptr as *const SratMemoryAffinity);
                        if mem.flags & SRAT_FLAG_ENABLED != 0 {
                            let node = self.get_or_create_node(mem.domain);
                            node.add_memory(mem.base_address, mem.length, mem.flags);
                        }
                    }
                    2 => {
                        // Processor Local x2APIC (similar handling)
                    }
                    _ => {}
                }
                
                offset += entry_len;
            }
        }
        
        self.numa_available.store(1, Ordering::SeqCst);
        crate::serial_println!("[NUMA] Parsed SRAT, {} nodes", self.node_count.load(Ordering::SeqCst));
        
        Ok(())
    }

    /// Parse SLIT table (distance matrix)
    pub fn parse_slit(&self, slit_ptr: *const u8) -> Result<(), NumaError> {
        unsafe {
            let header = &*(slit_ptr as *const SlitHeader);
            
            if header.signature != SLIT_SIGNATURE {
                return Err(NumaError::InvalidSignature);
            }
            
            let count = header.localality_count as usize;
            let matrix_offset = core::mem::size_of::<SlitHeader>();
            
            let mut nodes = self.nodes.lock();
            
            for i in 0..count {
                let node_id = i as u32;
                if let Some(node) = nodes.get_mut(&node_id) {
                    node.distances.clear();
                    for j in 0..count {
                        let distance = *(slit_ptr.add(matrix_offset + i * count + j));
                        node.distances.push(distance);
                    }
                }
            }
        }
        
        crate::serial_println!("[NUMA] Parsed SLIT distance matrix");
        Ok(())
    }

    /// Get or create a NUMA node
    fn get_or_create_node(&self, id: u32) -> NumaNode {
        let mut nodes = self.nodes.lock();
        if !nodes.contains_key(&id) {
            nodes.insert(id, NumaNode::new(id));
            self.node_count.fetch_add(1, Ordering::SeqCst);
        }
        nodes.get(&id).unwrap().clone()
    }

    /// Get node for a CPU
    pub fn get_node_for_cpu(&self, apic_id: u32) -> Option<u32> {
        self.cpu_to_node.lock().get(&apic_id).copied()
    }

    /// Get node for memory address
    pub fn get_node_for_address(&self, addr: u64) -> Option<u32> {
        let nodes = self.nodes.lock();
        for (id, node) in nodes.iter() {
            for range in &node.memory_ranges {
                if addr >= range.base && addr < range.base + range.length {
                    return Some(*id);
                }
            }
        }
        None
    }

    /// Get preferred node for allocation (current CPU's node)
    pub fn get_preferred_node(&self) -> u32 {
        // Get current CPU's APIC ID and find its node
        // For now, return node 0
        0
    }

    /// Get all nodes
    pub fn get_nodes(&self) -> Vec<NumaNode> {
        self.nodes.lock().values().cloned().collect()
    }

    /// Check if NUMA is available
    pub fn is_numa(&self) -> bool {
        self.numa_available.load(Ordering::SeqCst) == 1
    }

    /// Get node count
    pub fn node_count(&self) -> u32 {
        self.node_count.load(Ordering::SeqCst)
    }

    /// Allocate memory on specific node
    pub fn alloc_on_node(&self, node_id: u32, size: usize) -> Option<u64> {
        let nodes = self.nodes.lock();
        if let Some(node) = nodes.get(&node_id) {
            if node.free_memory.load(Ordering::Relaxed) >= size as u64 {
                node.free_memory.fetch_sub(size as u64, Ordering::Relaxed);
                // Return actual allocation (placeholder)
                return Some(0xDEADBEEF);
            }
        }
        None
    }

    /// Get memory policy for current task
    pub fn get_memory_policy(&self) -> MemoryPolicy {
        MemoryPolicy::default()
    }

    /// Set memory policy for current task
    pub fn set_memory_policy(&self, _policy: &MemoryPolicy) -> Result<(), NumaError> {
        Ok(())
    }
}

lazy_static::lazy_static! {
    /// Global NUMA manager
    pub static ref NUMA_MANAGER: NumaManager = NumaManager::new();
}

// ============================================================================
// MEMORY POLICY
// ============================================================================

/// NUMA memory policy
#[derive(Clone, Copy, Debug)]
pub enum MemoryPolicy {
    /// Default: allocate on local node
    Default,
    /// Prefer given node
    Preferred(u32),
    /// Bind to given nodes
    Bind(Vec<u32>),
    /// Interleave across nodes
    Interleave(Vec<u32>),
}

impl Default for MemoryPolicy {
    fn default() -> Self {
        MemoryPolicy::Default
    }
}

// ============================================================================
// ERROR TYPE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumaError {
    InvalidSignature,
    InvalidTable,
    NodeNotFound,
    NoMemory,
}

// ============================================================================
// SYSCALL INTERFACE
// ============================================================================

/// get_mempolicy syscall
pub fn sys_get_mempolicy(mode: &mut i32, nodemask: &mut u64, addr: u64, flags: u32) -> i32 {
    let policy = NUMA_MANAGER.get_memory_policy();
    *mode = match policy {
        MemoryPolicy::Default => 0,
        MemoryPolicy::Preferred(_) => 1,
        MemoryPolicy::Bind(_) => 2,
        MemoryPolicy::Interleave(_) => 3,
    };
    0
}

/// set_mempolicy syscall
pub fn sys_set_mempolicy(mode: i32, nodemask: u64) -> i32 {
    let policy = match mode {
        0 => MemoryPolicy::Default,
        1 => MemoryPolicy::Preferred((nodemask & 0xFF) as u32),
        2 => MemoryPolicy::Bind(vec![(nodemask & 0xFF) as u32]),
        3 => MemoryPolicy::Interleave(vec![(nodemask & 0xFF) as u32]),
        _ => return -22, // EINVAL
    };
    
    match NUMA_MANAGER.set_memory_policy(&policy) {
        Ok(()) => 0,
        Err(_) => -22,
    }
}

/// mbind syscall
pub fn sys_mbind(addr: u64, len: u64, mode: i32, nodemask: u64, flags: u32) -> i32 {
    // Bind memory range to specific nodes
    0
}

/// migrate_pages syscall
pub fn sys_migrate_pages(pid: i32, from_nodes: u64, to_nodes: u64) -> i32 {
    // Migrate pages between nodes
    0
}

/// move_pages syscall
pub fn sys_move_pages(pid: i32, count: usize, pages: *const u64, nodes: *const i32, status: *mut i32, flags: u32) -> i32 {
    // Move specific pages to nodes
    0
}

// ============================================================================
// INITIALIZATION
// ============================================================================

/// Initialize NUMA subsystem
pub fn init() {
    crate::serial_println!("[NUMA] Subsystem initialized");
}

/// Initialize from ACPI tables
pub fn init_from_acpi(srat_addr: Option<u64>, slit_addr: Option<u64>) {
    if let Some(addr) = srat_addr {
        let _ = NUMA_MANAGER.parse_srat(addr as *const u8);
    }
    
    if let Some(addr) = slit_addr {
        let _ = NUMA_MANAGER.parse_slit(addr as *const u8);
    }
}

/// Get statistics
pub struct NumaStats {
    pub node_count: u32,
    pub total_memory: u64,
    pub numa_available: bool,
}

pub fn get_stats() -> NumaStats {
    let nodes = NUMA_MANAGER.get_nodes();
    NumaStats {
        node_count: nodes.len() as u32,
        total_memory: nodes.iter().map(|n| n.total_memory).sum(),
        numa_available: NUMA_MANAGER.is_numa(),
    }
}
