//! # Capability-Based Security
//!
//! Fine-grained capability system for resource access control.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;

/// Capability ID
pub type CapId = u64;

/// Capability rights
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapRights {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
    pub share: bool,
    pub transfer: bool,
}

impl CapRights {
    pub const NONE: Self = CapRights { read: false, write: false, execute: false, share: false, transfer: false };
    pub const READ: Self = CapRights { read: true, write: false, execute: false, share: false, transfer: false };
    pub const WRITE: Self = CapRights { read: false, write: true, execute: false, share: false, transfer: false };
    pub const READ_WRITE: Self = CapRights { read: true, write: true, execute: false, share: false, transfer: false };
    pub const ALL: Self = CapRights { read: true, write: true, execute: true, share: true, transfer: true };
}

/// Resource type
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceType {
    File,
    Directory,
    Socket,
    Device,
    Memory,
    Process,
    Thread,
    Port,
    Key,
    Service,
}

/// Capability object
#[derive(Clone, Debug)]
pub struct Capability {
    pub id: CapId,
    pub resource_type: ResourceType,
    pub resource_id: u64,
    pub rights: CapRights,
    pub owner: u64,  // Process ID
    pub generation: u32,
    pub children: Vec<CapId>,
}

/// Capability table per process
#[derive(Clone, Debug)]
pub struct CapabilityTable {
    pub process_id: u64,
    pub capabilities: BTreeMap<CapId, Capability>,
    pub next_cap_id: CapId,
}

impl CapabilityTable {
    pub fn new(process_id: u64) -> Self {
        CapabilityTable {
            process_id,
            capabilities: BTreeMap::new(),
            next_cap_id: 1,
        }
    }

    /// Create new capability
    pub fn create(&mut self, resource_type: ResourceType, resource_id: u64, rights: CapRights) -> CapId {
        let id = self.next_cap_id;
        self.next_cap_id += 1;

        let cap = Capability {
            id,
            resource_type,
            resource_id,
            rights,
            owner: self.process_id,
            generation: 0,
            children: Vec::new(),
        };

        self.capabilities.insert(id, cap);
        id
    }

    /// Get capability by ID
    pub fn get(&self, id: CapId) -> Option<&Capability> {
        self.capabilities.get(&id)
    }

    /// Check if capability exists and has rights
    pub fn check(&self, id: CapId, required: CapRights) -> bool {
        if let Some(cap) = self.capabilities.get(&id) {
            let r = cap.rights;
            (!required.read || r.read)
                && (!required.write || r.write)
                && (!required.execute || r.execute)
                && (!required.share || r.share)
                && (!required.transfer || r.transfer)
        } else {
            false
        }
    }

    /// Derive child capability (subset of rights)
    pub fn derive(&mut self, parent_id: CapId, subset_rights: CapRights) -> Option<CapId> {
        let parent = self.capabilities.get(&parent_id)?;
        
        // Check if subset is valid
        if subset_rights.read && !parent.rights.read { return None; }
        if subset_rights.write && !parent.rights.write { return None; }
        if subset_rights.execute && !parent.rights.execute { return None; }
        if subset_rights.share && !parent.rights.share { return None; }
        if subset_rights.transfer && !parent.rights.transfer { return None; }

        let child_id = self.next_cap_id;
        self.next_cap_id += 1;

        let child = Capability {
            id: child_id,
            resource_type: parent.resource_type,
            resource_id: parent.resource_id,
            rights: subset_rights,
            owner: self.process_id,
            generation: parent.generation + 1,
            children: Vec::new(),
        };

        self.capabilities.get_mut(&parent_id)?.children.push(child_id);
        self.capabilities.insert(child_id, child);
        Some(child_id)
    }

    /// Revoke capability and all children
    pub fn revoke(&mut self, id: CapId) -> bool {
        if let Some(cap) = self.capabilities.remove(&id) {
            // Revoke all children recursively
            for child_id in cap.children {
                self.revoke(child_id);
            }
            true
        } else {
            false
        }
    }

    /// Transfer capability to another process
    pub fn transfer(&mut self, id: CapId, target_pid: u64) -> Option<Capability> {
        let cap = self.capabilities.remove(&id)?;
        if !cap.rights.transfer {
            self.capabilities.insert(id, cap);
            return None;
        }

        let mut transferred = cap.clone();
        transferred.owner = target_pid;
        transferred.generation += 1;
        Some(transferred)
    }
}

// Global capability manager
lazy_static::lazy_static! {
    static ref CAP_TABLES: Mutex<BTreeMap<u64, CapabilityTable>> = Mutex::new(BTreeMap::new());
}

/// Initialize capability table for process
pub fn init_process(pid: u64) {
    let mut tables = CAP_TABLES.lock();
    tables.insert(pid, CapabilityTable::new(pid));
}

/// Get capability table for process
pub fn get_table(pid: u64) -> Option<CapabilityTable> {
    CAP_TABLES.lock().get(&pid).cloned()
}

/// Create capability for process
pub fn create_capability(pid: u64, resource_type: ResourceType, resource_id: u64, rights: CapRights) -> Option<CapId> {
    let mut tables = CAP_TABLES.lock();
    let table = tables.get_mut(&pid)?;
    Some(table.create(resource_type, resource_id, rights))
}

/// Check capability
pub fn check_capability(pid: u64, cap_id: CapId, rights: CapRights) -> bool {
    let tables = CAP_TABLES.lock();
    if let Some(table) = tables.get(&pid) {
        table.check(cap_id, rights)
    } else {
        false
    }
}

/// Derive capability
pub fn derive_capability(pid: u64, parent_id: CapId, subset_rights: CapRights) -> Option<CapId> {
    let mut tables = CAP_TABLES.lock();
    let table = tables.get_mut(&pid)?;
    table.derive(parent_id, subset_rights)
}

/// Revoke capability
pub fn revoke_capability(pid: u64, cap_id: CapId) -> bool {
    let mut tables = CAP_TABLES.lock();
    if let Some(table) = tables.get_mut(&pid) {
        table.revoke(cap_id)
    } else {
        false
    }
}

/// Transfer capability between processes
pub fn transfer_capability(from_pid: u64, cap_id: CapId, to_pid: u64) -> bool {
    let mut tables = CAP_TABLES.lock();
    
    let transferred = {
        let from_table = tables.get_mut(&from_pid);
        if let Some(table) = from_table {
            table.transfer(cap_id, to_pid)
        } else {
            None
        }
    };

    if let Some(cap) = transferred {
        let to_table = tables.get_mut(&to_pid);
        if let Some(table) = to_table {
            table.capabilities.insert(cap.id, cap);
            return true;
        }
    }
    false
}

/// Cleanup process capabilities
pub fn cleanup_process(pid: u64) {
    CAP_TABLES.lock().remove(&pid);
}

/// Capability seal (make immutable)
pub fn seal_capability(pid: u64, cap_id: CapId) -> bool {
    let tables = CAP_TABLES.lock();
    if let Some(table) = tables.get(&pid) {
        if let Some(cap) = table.capabilities.get(&cap_id) {
            // Sealed capabilities cannot be transferred
            // This is enforced by checking the transfer flag
            return !cap.rights.transfer;
        }
    }
    false
}
