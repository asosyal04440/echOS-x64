//! # Extended Attributes (xattr)
//!
//! POSIX extended attributes support for files.
//! Allows storing additional metadata as key-value pairs.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// XATTR CONSTANTS
// ============================================================================

/// Maximum xattr name length
pub const XATTR_NAME_MAX: usize = 255;
/// Maximum xattr value length
pub const XATTR_VALUE_MAX: usize = 65536;
/// Maximum xattr list size
pub const XATTR_LIST_MAX: usize = 65536;

/// xattr namespace prefixes
pub const XATTR_USER_PREFIX: &str = "user.";
pub const XATTR_TRUSTED_PREFIX: &str = "trusted.";
pub const XATTR_SECURITY_PREFIX: &str = "security.";
pub const XATTR_SYSTEM_PREFIX: &str = "system.";

/// xattr flags for setxattr
pub const XATTR_CREATE: i32 = 1;  // Create only (fail if exists)
pub const XATTR_REPLACE: i32 = 2; // Replace only (fail if not exists)

// ============================================================================
// XATTR STRUCTURE
// ============================================================================

/// An extended attribute
#[derive(Clone, Debug)]
pub struct Xattr {
    /// Attribute name (including namespace prefix)
    pub name: String,
    /// Attribute value
    pub value: Vec<u8>,
    /// Flags
    pub flags: u32,
}

impl Xattr {
    pub fn new(name: &str, value: &[u8]) -> Self {
        Self {
            name: String::from(name),
            value: Vec::from(value),
            flags: 0,
        }
    }

    /// Get namespace from name
    pub fn namespace(&self) -> &str {
        if self.name.starts_with(XATTR_USER_PREFIX) {
            "user"
        } else if self.name.starts_with(XATTR_TRUSTED_PREFIX) {
            "trusted"
        } else if self.name.starts_with(XATTR_SECURITY_PREFIX) {
            "security"
        } else if self.name.starts_with(XATTR_SYSTEM_PREFIX) {
            "system"
        } else {
            "unknown"
        }
    }

    /// Check if attribute name is valid
    pub fn is_valid_name(name: &str) -> bool {
        if name.is_empty() || name.len() > XATTR_NAME_MAX {
            return false;
        }
        
        // Must have a namespace prefix
        name.contains('.')
    }
}

// ============================================================================
// XATTR MANAGER
// ============================================================================

/// Extended attribute storage (per-inode)
#[derive(Clone, Debug)]
pub struct XattrStorage {
    /// Inode number
    pub inode: u64,
    /// Attributes (name -> value)
    attrs: BTreeMap<String, Vec<u8>>,
}

impl XattrStorage {
    pub fn new(inode: u64) -> Self {
        Self {
            inode,
            attrs: BTreeMap::new(),
        }
    }

    /// Get attribute value
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.attrs.get(name).map(|v| v.as_slice())
    }

    /// Set attribute value
    pub fn set(&mut self, name: &str, value: &[u8], flags: i32) -> Result<(), XattrError> {
        if !Xattr::is_valid_name(name) {
            return Err(XattrError::InvalidName);
        }
        
        if value.len() > XATTR_VALUE_MAX {
            return Err(XattrError::ValueTooLarge);
        }
        
        let exists = self.attrs.contains_key(name);
        
        // Check flags
        if flags & XATTR_CREATE != 0 && exists {
            return Err(XattrError::AlreadyExists);
        }
        if flags & XATTR_REPLACE != 0 && !exists {
            return Err(XattrError::NotFound);
        }
        
        self.attrs.insert(String::from(name), Vec::from(value));
        Ok(())
    }

    /// Remove attribute
    pub fn remove(&mut self, name: &str) -> Result<(), XattrError> {
        if self.attrs.remove(name).is_some() {
            Ok(())
        } else {
            Err(XattrError::NotFound)
        }
    }

    /// List all attribute names
    pub fn list(&self) -> Vec<u8> {
        let mut result = Vec::new();
        for name in self.attrs.keys() {
            result.extend_from_slice(name.as_bytes());
            result.push(0); // null terminator
        }
        result
    }

    /// Get attribute count
    pub fn count(&self) -> usize {
        self.attrs.len()
    }

    /// Get total size of all attributes
    pub fn total_size(&self) -> usize {
        self.attrs.iter()
            .map(|(k, v)| k.len() + 1 + v.len())
            .sum()
    }
}

/// Global xattr manager
pub struct XattrManager {
    /// Storage per inode
    storage: Mutex<BTreeMap<u64, XattrStorage>>,
    /// Total xattr count
    total_xattrs: AtomicU64,
    /// Total size
    total_size: AtomicU64,
}

impl XattrManager {
    pub const fn new() -> Self {
        Self {
            storage: Mutex::new(BTreeMap::new()),
            total_xattrs: AtomicU64::new(0),
            total_size: AtomicU64::new(0),
        }
    }

    /// Get storage for inode (create if not exists)
    fn get_or_create_storage(&self, inode: u64) -> XattrStorage {
        let mut storage = self.storage.lock();
        storage.entry(inode).or_insert_with(|| XattrStorage::new(inode)).clone()
    }

    /// Get attribute
    pub fn get(&self, inode: u64, name: &str) -> Option<Vec<u8>> {
        let storage = self.storage.lock();
        storage.get(&inode).and_then(|s| s.get(name).map(|v| v.to_vec()))
    }

    /// Set attribute
    pub fn set(&self, inode: u64, name: &str, value: &[u8], flags: i32) -> Result<(), XattrError> {
        let mut storage = self.storage.lock();
        let entry = storage.entry(inode).or_insert_with(|| XattrStorage::new(inode));
        
        let old_size = entry.get(name).map(|v| v.len()).unwrap_or(0);
        entry.set(name, value, flags)?;
        
        // Update stats
        self.total_xattrs.fetch_add(1, Ordering::Relaxed);
        self.total_size.fetch_add((value.len() - old_size) as u64, Ordering::Relaxed);
        
        crate::serial_println!(
            "[XATTR] Set '{}' on inode {:#x} ({} bytes)",
            name, inode, value.len()
        );
        
        Ok(())
    }

    /// Remove attribute
    pub fn remove(&self, inode: u64, name: &str) -> Result<(), XattrError> {
        let mut storage = self.storage.lock();
        
        if let Some(entry) = storage.get_mut(&inode) {
            let size = entry.get(name).map(|v| v.len()).unwrap_or(0);
            entry.remove(name)?;
            
            self.total_xattrs.fetch_sub(1, Ordering::Relaxed);
            self.total_size.fetch_sub(size as u64, Ordering::Relaxed);
            
            crate::serial_println!("[XATTR] Removed '{}' from inode {:#x}", name, inode);
            Ok(())
        } else {
            Err(XattrError::NotFound)
        }
    }

    /// List attributes
    pub fn list(&self, inode: u64) -> Vec<u8> {
        let storage = self.storage.lock();
        storage.get(&inode).map(|s| s.list()).unwrap_or_default()
    }

    /// Remove all attributes for inode
    pub fn remove_all(&self, inode: u64) {
        let mut storage = self.storage.lock();
        if let Some(entry) = storage.remove(&inode) {
            let count = entry.count() as u64;
            let size = entry.total_size() as u64;
            self.total_xattrs.fetch_sub(count, Ordering::Relaxed);
            self.total_size.fetch_sub(size, Ordering::Relaxed);
        }
    }
}

lazy_static::lazy_static! {
    /// Global xattr manager
    static ref XATTR_MANAGER: XattrManager = XattrManager::new();
}

// ============================================================================
// ERROR TYPE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XattrError {
    InvalidName,
    ValueTooLarge,
    NotFound,
    AlreadyExists,
    PermissionDenied,
    NotSupported,
}

// ============================================================================
// SYSCALL INTERFACE
// ============================================================================

/// setxattr syscall implementation
pub fn sys_setxattr(path: &str, name: &str, value: &[u8], flags: i32) -> i32 {
    if !Xattr::is_valid_name(name) {
        return -22; // EINVAL
    }
    
    if value.len() > XATTR_VALUE_MAX {
        return -7; // E2BIG
    }
    
    // Get inode from path (placeholder)
    let inode = hash_path(path);
    
    // Check namespace permissions
    if name.starts_with(XATTR_TRUSTED_PREFIX) || name.starts_with(XATTR_SECURITY_PREFIX) {
        // These require CAP_SYS_ADMIN
        // For now, allow all
    }
    
    match XATTR_MANAGER.set(inode, name, value, flags) {
        Ok(()) => 0,
        Err(XattrError::AlreadyExists) => -17, // EEXIST
        Err(XattrError::NotFound) => -2, // ENOENT
        Err(XattrError::InvalidName) => -22, // EINVAL
        Err(XattrError::ValueTooLarge) => -7, // E2BIG
        Err(_) => -5, // EIO
    }
}

/// lsetxattr syscall (same as setxattr but doesn't follow symlinks)
pub fn sys_lsetxattr(path: &str, name: &str, value: &[u8], flags: i32) -> i32 {
    // For now, same as setxattr
    sys_setxattr(path, name, value, flags)
}

/// fsetxattr syscall (by file descriptor)
pub fn sys_fsetxattr(fd: i32, name: &str, value: &[u8], flags: i32) -> i32 {
    if fd < 0 {
        return -9; // EBADF
    }
    
    // Get inode from fd (placeholder)
    let inode = fd as u64;
    
    match XATTR_MANAGER.set(inode, name, value, flags) {
        Ok(()) => 0,
        Err(_) => -5, // EIO
    }
}

/// getxattr syscall implementation
pub fn sys_getxattr(path: &str, name: &str, buf: &mut [u8]) -> i64 {
    if !Xattr::is_valid_name(name) {
        return -22; // EINVAL
    }
    
    let inode = hash_path(path);
    
    match XATTR_MANAGER.get(inode, name) {
        Some(value) => {
            if buf.is_empty() {
                return value.len() as i64;
            }
            
            if value.len() > buf.len() {
                return -34; // ERANGE
            }
            
            buf[..value.len()].copy_from_slice(&value);
            value.len() as i64
        }
        None => -61, // ENODATA
    }
}

/// lgetxattr syscall
pub fn sys_lgetxattr(path: &str, name: &str, buf: &mut [u8]) -> i64 {
    sys_getxattr(path, name, buf)
}

/// fgetxattr syscall
pub fn sys_fgetxattr(fd: i32, name: &str, buf: &mut [u8]) -> i64 {
    if fd < 0 {
        return -9; // EBADF
    }
    
    let inode = fd as u64;
    
    match XATTR_MANAGER.get(inode, name) {
        Some(value) => {
            if buf.is_empty() {
                return value.len() as i64;
            }
            
            if value.len() > buf.len() {
                return -34; // ERANGE
            }
            
            buf[..value.len()].copy_from_slice(&value);
            value.len() as i64
        }
        None => -61, // ENODATA
    }
}

/// listxattr syscall implementation
pub fn sys_listxattr(path: &str, buf: &mut [u8]) -> i64 {
    let inode = hash_path(path);
    
    let list = XATTR_MANAGER.list(inode);
    
    if buf.is_empty() {
        return list.len() as i64;
    }
    
    if list.len() > buf.len() {
        return -34; // ERANGE
    }
    
    buf[..list.len()].copy_from_slice(&list);
    list.len() as i64
}

/// llistxattr syscall
pub fn sys_llistxattr(path: &str, buf: &mut [u8]) -> i64 {
    sys_listxattr(path, buf)
}

/// flistxattr syscall
pub fn sys_flistxattr(fd: i32, buf: &mut [u8]) -> i64 {
    if fd < 0 {
        return -9; // EBADF
    }
    
    let inode = fd as u64;
    let list = XATTR_MANAGER.list(inode);
    
    if buf.is_empty() {
        return list.len() as i64;
    }
    
    if list.len() > buf.len() {
        return -34; // ERANGE
    }
    
    buf[..list.len()].copy_from_slice(&list);
    list.len() as i64
}

/// removexattr syscall implementation
pub fn sys_removexattr(path: &str, name: &str) -> i32 {
    if !Xattr::is_valid_name(name) {
        return -22; // EINVAL
    }
    
    let inode = hash_path(path);
    
    match XATTR_MANAGER.remove(inode, name) {
        Ok(()) => 0,
        Err(XattrError::NotFound) => -61, // ENODATA
        Err(_) => -5, // EIO
    }
}

/// lremovexattr syscall
pub fn sys_lremovexattr(path: &str, name: &str) -> i32 {
    sys_removexattr(path, name)
}

/// fremovexattr syscall
pub fn sys_fremovexattr(fd: i32, name: &str) -> i32 {
    if fd < 0 {
        return -9; // EBADF
    }
    
    let inode = fd as u64;
    
    match XATTR_MANAGER.remove(inode, name) {
        Ok(()) => 0,
        Err(XattrError::NotFound) => -61, // ENODATA
        Err(_) => -5, // EIO
    }
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Simple path hash for inode placeholder
fn hash_path(path: &str) -> u64 {
    let mut hash: u64 = 5381;
    for byte in path.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
    }
    hash
}

// ============================================================================
// SPECIAL XATTR HANDLING
// ============================================================================

/// ACL related xattrs
pub const XATTR_NAME_POSIX_ACL_ACCESS: &str = "system.posix_acl_access";
pub const XATTR_NAME_POSIX_ACL_DEFAULT: &str = "system.posix_acl_default";

/// SELinux xattr
pub const XATTR_NAME_SELINUX: &str = "security.selinux";

/// Capability xattrs
pub const XATTR_NAME_CAPS: &str = "security.capability";

/// Check if xattr is a special system xattr
pub fn is_system_xattr(name: &str) -> bool {
    name == XATTR_NAME_POSIX_ACL_ACCESS ||
    name == XATTR_NAME_POSIX_ACL_DEFAULT ||
    name == XATTR_NAME_CAPS
}

/// Check if xattr is security-related
pub fn is_security_xattr(name: &str) -> bool {
    name.starts_with(XATTR_SECURITY_PREFIX)
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// Initialize xattr subsystem
pub fn init() {
    crate::serial_println!("[XATTR] Subsystem initialized");
}

/// Get xattr statistics
pub struct XattrStats {
    pub inode_count: usize,
    pub total_xattrs: u64,
    pub total_size: u64,
}

/// Get statistics
pub fn get_stats() -> XattrStats {
    XattrStats {
        inode_count: XATTR_MANAGER.storage.lock().len(),
        total_xattrs: XATTR_MANAGER.total_xattrs.load(Ordering::Relaxed),
        total_size: XATTR_MANAGER.total_size.load(Ordering::Relaxed),
    }
}

/// Remove all xattrs for an inode (called when file is deleted)
pub fn remove_inode_xattrs(inode: u64) {
    XATTR_MANAGER.remove_all(inode);
}
