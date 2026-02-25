//! # POSIX ACL (Access Control Lists)
//!
//! Fine-grained file permissions beyond owner/group/other.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// ACL CONSTANTS
// ============================================================================

/// ACL tag types
pub const ACL_USER_OBJ: u32 = 0x01;
pub const ACL_USER: u32 = 0x02;
pub const ACL_GROUP_OBJ: u32 = 0x04;
pub const ACL_GROUP: u32 = 0x08;
pub const ACL_MASK: u32 = 0x10;
pub const ACL_OTHER: u32 = 0x20;

/// ACL permissions
pub const ACL_READ: u32 = 0x04;
pub const ACL_WRITE: u32 = 0x02;
pub const ACL_EXECUTE: u32 = 0x01;

/// ACL types
pub const ACL_TYPE_ACCESS: u32 = 0x8000_0000;
pub const ACL_TYPE_DEFAULT: u32 = 0x4000_0000;

/// ACL commands
pub const ACL_GET_TYPE: u32 = 0x0001;
pub const ACL_SET_TYPE: u32 = 0x0002;
pub const ACL_GET_FILE: u32 = 0x0003;
pub const ACL_SET_FILE: u32 = 0x0004;
pub const ACL_DELETE_FILE: u32 = 0x0005;

// ============================================================================
// ACL ENTRY
// ============================================================================

/// ACL entry
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AclEntry {
    /// Tag type (USER_OBJ, USER, GROUP_OBJ, GROUP, MASK, OTHER)
    pub tag: u32,
    /// Permission bits
    pub perm: u32,
    /// Qualifier (UID for USER, GID for GROUP)
    pub qualifier: u32,
}

impl AclEntry {
    pub fn new(tag: u32, perm: u32, qualifier: u32) -> Self {
        Self { tag, perm, qualifier }
    }

    /// Create user_obj entry from mode
    pub fn user_obj_from_mode(mode: u32) -> Self {
        Self::new(ACL_USER_OBJ, (mode >> 6) & 0x7, 0)
    }

    /// Create group_obj entry from mode
    pub fn group_obj_from_mode(mode: u32) -> Self {
        Self::new(ACL_GROUP_OBJ, (mode >> 3) & 0x7, 0)
    }

    /// Create other entry from mode
    pub fn other_from_mode(mode: u32) -> Self {
        Self::new(ACL_OTHER, mode & 0x7, 0)
    }

    /// Check if entry grants permission
    pub fn grants(&self, perm: u32) -> bool {
        (self.perm & perm) == perm
    }
}

// ============================================================================
// ACL
// ============================================================================

/// Full ACL for a file
#[derive(Clone, Debug)]
pub struct Acl {
    /// Inode this ACL belongs to
    pub inode: u64,
    /// Access ACL entries
    pub access: Vec<AclEntry>,
    /// Default ACL entries (for directories)
    pub default: Vec<AclEntry>,
}

impl Acl {
    pub fn new(inode: u64) -> Self {
        Self {
            inode,
            access: Vec::new(),
            default: Vec::new(),
        }
    }

    /// Create minimal ACL from mode
    pub fn from_mode(inode: u64, mode: u32) -> Self {
        let mut acl = Self::new(inode);
        acl.access.push(AclEntry::user_obj_from_mode(mode));
        acl.access.push(AclEntry::group_obj_from_mode(mode));
        acl.access.push(AclEntry::other_from_mode(mode));
        acl
    }

    /// Add entry to access ACL
    pub fn add_access(&mut self, entry: AclEntry) {
        self.access.push(entry);
    }

    /// Add entry to default ACL
    pub fn add_default(&mut self, entry: AclEntry) {
        self.default.push(entry);
    }

    /// Remove entry from access ACL
    pub fn remove_access(&mut self, tag: u32, qualifier: u32) -> bool {
        let len = self.access.len();
        self.access.retain(|e| !(e.tag == tag && e.qualifier == qualifier));
        self.access.len() != len
    }

    /// Check permission for user
    pub fn check_permission(&self, uid: u32, gid: u32, mask: u32, perm: u32) -> bool {
        // Check user entries
        for entry in &self.access {
            match entry.tag {
                ACL_USER_OBJ => {
                    // Owner permission - checked elsewhere
                }
                ACL_USER => {
                    if entry.qualifier == uid {
                        return self.apply_mask(entry.perm, mask) & perm == perm;
                    }
                }
                ACL_GROUP_OBJ => {
                    // Group permission - checked elsewhere
                }
                ACL_GROUP => {
                    if entry.qualifier == gid {
                        if self.apply_mask(entry.perm, mask) & perm == perm {
                            return true;
                        }
                    }
                }
                ACL_MASK => {}
                ACL_OTHER => {
                    // Other permission - checked last
                }
                _ => {}
            }
        }
        false
    }

    /// Apply mask to permissions
    fn apply_mask(&self, perm: u32, mask: u32) -> u32 {
        // Find mask entry
        for entry in &self.access {
            if entry.tag == ACL_MASK {
                return perm & entry.perm & mask;
            }
        }
        perm & mask
    }

    /// Get mask entry
    pub fn get_mask(&self) -> Option<&AclEntry> {
        self.access.iter().find(|e| e.tag == ACL_MASK)
    }

    /// Check if ACL is minimal (only user_obj, group_obj, other)
    pub fn is_minimal(&self) -> bool {
        self.access.len() == 3 &&
        self.access.iter().all(|e| e.tag == ACL_USER_OBJ || 
                                   e.tag == ACL_GROUP_OBJ || 
                                   e.tag == ACL_OTHER)
    }

    /// Convert to mode bits
    pub fn to_mode(&self) -> u32 {
        let mut mode = 0u32;
        
        for entry in &self.access {
            match entry.tag {
                ACL_USER_OBJ => mode |= entry.perm << 6,
                ACL_GROUP_OBJ => mode |= entry.perm << 3,
                ACL_OTHER => mode |= entry.perm,
                _ => {}
            }
        }
        
        mode
    }

    /// Serialize to binary format
    pub fn to_binary(&self) -> Vec<u8> {
        let mut data = Vec::new();
        
        // Header
        data.extend_from_slice(&(self.access.len() as u32).to_le_bytes());
        data.extend_from_slice(&(self.default.len() as u32).to_le_bytes());
        
        // Access entries
        for entry in &self.access {
            data.extend_from_slice(&entry.tag.to_le_bytes());
            data.extend_from_slice(&entry.perm.to_le_bytes());
            data.extend_from_slice(&entry.qualifier.to_le_bytes());
        }
        
        // Default entries
        for entry in &self.default {
            data.extend_from_slice(&entry.tag.to_le_bytes());
            data.extend_from_slice(&entry.perm.to_le_bytes());
            data.extend_from_slice(&entry.qualifier.to_le_bytes());
        }
        
        data
    }

    /// Deserialize from binary
    pub fn from_binary(inode: u64, data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }
        
        let access_count = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        let default_count = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
        
        let mut acl = Self::new(inode);
        let mut offset = 8;
        
        for _ in 0..access_count {
            if offset + 12 > data.len() {
                return None;
            }
            let tag = u32::from_le_bytes([data[offset], data[offset+1], data[offset+2], data[offset+3]]);
            let perm = u32::from_le_bytes([data[offset+4], data[offset+5], data[offset+6], data[offset+7]]);
            let qual = u32::from_le_bytes([data[offset+8], data[offset+9], data[offset+10], data[offset+11]]);
            acl.access.push(AclEntry::new(tag, perm, qual));
            offset += 12;
        }
        
        for _ in 0..default_count {
            if offset + 12 > data.len() {
                return None;
            }
            let tag = u32::from_le_bytes([data[offset], data[offset+1], data[offset+2], data[offset+3]]);
            let perm = u32::from_le_bytes([data[offset+4], data[offset+5], data[offset+6], data[offset+7]]);
            let qual = u32::from_le_bytes([data[offset+8], data[offset+9], data[offset+10], data[offset+11]]);
            acl.default.push(AclEntry::new(tag, perm, qual));
            offset += 12;
        }
        
        Some(acl)
    }
}

// ============================================================================
// ACL MANAGER
// ============================================================================

pub struct AclManager {
    acls: Mutex<BTreeMap<u64, Acl>>,
    total_acls: AtomicU64,
}

impl AclManager {
    pub const fn new() -> Self {
        Self {
            acls: Mutex::new(BTreeMap::new()),
            total_acls: AtomicU64::new(0),
        }
    }

    /// Get ACL for inode
    pub fn get_acl(&self, inode: u64) -> Option<Acl> {
        self.acls.lock().get(&inode).cloned()
    }

    /// Set ACL for inode
    pub fn set_acl(&self, inode: u64, acl: Acl) {
        self.acls.lock().insert(inode, acl);
        self.total_acls.fetch_add(1, Ordering::Relaxed);
    }

    /// Remove ACL for inode
    pub fn remove_acl(&self, inode: u64) {
        self.acls.lock().remove(&inode);
    }

    /// Check permission
    pub fn check_permission(&self, inode: u64, uid: u32, gid: u32, mask: u32, perm: u32) -> bool {
        if let Some(acl) = self.get_acl(inode) {
            acl.check_permission(uid, gid, mask, perm)
        } else {
            // No ACL, use standard permissions
            true
        }
    }

    /// Get statistics
    pub fn get_stats(&self) -> AclStats {
        AclStats {
            total_acls: self.total_acls.load(Ordering::Relaxed),
        }
    }
}

lazy_static::lazy_static! {
    pub static ref ACL_MANAGER: AclManager = AclManager::new();
}

pub struct AclStats {
    pub total_acls: u64,
}

// ============================================================================
// SYSCALL INTERFACE
// ============================================================================

pub fn sys_acl_get_file(path: &str, acl_type: u32) -> i64 {
    let inode = hash_path(path);
    
    match ACL_MANAGER.get_acl(inode) {
        Some(acl) => {
            let data = if acl_type == ACL_TYPE_ACCESS {
                acl.to_binary()
            } else {
                // Return default ACL
                let mut data = Vec::new();
                data.extend_from_slice(&0u32.to_le_bytes());
                data.extend_from_slice(&(acl.default.len() as u32).to_le_bytes());
                for entry in &acl.default {
                    data.extend_from_slice(&entry.tag.to_le_bytes());
                    data.extend_from_slice(&entry.perm.to_le_bytes());
                    data.extend_from_slice(&entry.qualifier.to_le_bytes());
                }
                data
            };
            data.len() as i64
        }
        None => -61, // ENODATA
    }
}

pub fn sys_acl_set_file(path: &str, acl_type: u32, data: &[u8]) -> i32 {
    let inode = hash_path(path);
    
    match Acl::from_binary(inode, data) {
        Some(acl) => {
            ACL_MANAGER.set_acl(inode, acl);
            0
        }
        None => -22,
    }
}

pub fn sys_acl_delete_file(path: &str, acl_type: u32) -> i32 {
    let inode = hash_path(path);
    ACL_MANAGER.remove_acl(inode);
    0
}

fn hash_path(path: &str) -> u64 {
    let mut hash: u64 = 5381;
    for byte in path.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
    }
    hash
}

pub fn init() {
    crate::serial_println!("[ACL] Subsystem initialized");
}
