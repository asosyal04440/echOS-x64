//! # memfd_create and userfaultfd
//!
//! Anonymous file creation and user-space page fault handling.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, AtomicBool, Ordering};
use spin::Mutex;

// ============================================================================
// MEMFD CONSTANTS
// ============================================================================

/// memfd_create flags
pub const MFD_CLOEXEC: u32 = 0x0001;
pub const MFD_ALLOW_SEALING: u32 = 0x0002;
pub const MFD_HUGETLB: u32 = 0x0004;
pub const MFD_NOEXEC_SEAL: u32 = 0x0008;

/// File seals
pub const F_SEAL_SEAL: u32 = 0x0001;
pub const F_SEAL_SHRINK: u32 = 0x0002;
pub const F_SEAL_GROW: u32 = 0x0004;
pub const F_SEAL_WRITE: u32 = 0x0008;
pub const F_SEAL_FUTURE_WRITE: u32 = 0x0010;
pub const F_SEAL_EXEC: u32 = 0x0020;

// ============================================================================
// MEMFD STRUCTURE
// ============================================================================

/// memfd file
pub struct Memfd {
    /// File descriptor
    pub fd: i32,
    /// Name
    pub name: String,
    /// Size
    pub size: AtomicU64,
    /// Seals applied
    pub seals: AtomicU32,
    /// Flags
    pub flags: u32,
    /// Data pages
    pub pages: Mutex<BTreeMap<u64, Vec<u8>>>,
    /// Reference count
    pub ref_count: AtomicU32,
    /// Is hugetlb
    pub hugetlb: bool,
}

impl Memfd {
    pub fn new(fd: i32, name: &str, flags: u32) -> Self {
        Self {
            fd,
            name: String::from(name),
            size: AtomicU64::new(0),
            seals: AtomicU32::new(0),
            flags,
            pages: Mutex::new(BTreeMap::new()),
            ref_count: AtomicU32::new(1),
            hugetlb: (flags & MFD_HUGETLB) != 0,
        }
    }

    /// Read from memfd
    pub fn read(&self, offset: u64, buf: &mut [u8]) -> usize {
        let pages = self.pages.lock();
        let page_size = if self.hugetlb { 2 * 1024 * 1024 } else { 4096 };
        
        let mut read = 0;
        let mut pos = offset;
        
        while read < buf.len() {
            let page_idx = pos / page_size as u64;
            let page_offset = (pos % page_size as u64) as usize;
            
            if let Some(page) = pages.get(&page_idx) {
                let to_read = core::cmp::min(
                    page.len().saturating_sub(page_offset),
                    buf.len() - read
                );
                buf[read..read + to_read].copy_from_slice(&page[page_offset..page_offset + to_read]);
                read += to_read;
                pos += to_read as u64;
            } else {
                // Unallocated page returns zeros
                let to_read = core::cmp::min(
                    page_size - page_offset,
                    buf.len() - read
                );
                for i in 0..to_read {
                    buf[read + i] = 0;
                }
                read += to_read;
                pos += to_read as u64;
            }
        }
        
        read
    }

    /// Write to memfd
    pub fn write(&self, offset: u64, buf: &[u8]) -> Result<usize, MemfdError> {
        // Check seals
        let seals = self.seals.load(Ordering::SeqCst);
        if seals & F_SEAL_WRITE != 0 || seals & F_SEAL_FUTURE_WRITE != 0 {
            return Err(MemfdError::Sealed);
        }
        
        let page_size = if self.hugetlb { 2 * 1024 * 1024 } else { 4096 };
        let mut pages = self.pages.lock();
        
        let mut written = 0;
        let mut pos = offset;
        
        while written < buf.len() {
            let page_idx = pos / page_size as u64;
            let page_offset = (pos % page_size as u64) as usize;
            
            let page = pages.entry(page_idx).or_insert_with(|| {
                vec![0u8; page_size]
            });
            
            let to_write = core::cmp::min(
                page.len().saturating_sub(page_offset),
                buf.len() - written
            );
            page[page_offset..page_offset + to_write]
                .copy_from_slice(&buf[written..written + to_write]);
            
            written += to_write;
            pos += to_write as u64;
        }
        
        // Update size
        let current_size = self.size.load(Ordering::SeqCst);
        if pos > current_size {
            self.size.store(pos, Ordering::SeqCst);
        }
        
        Ok(written)
    }

    /// Set seals
    pub fn set_seals(&self, new_seals: u32) -> Result<(), MemfdError> {
        let current = self.seals.load(Ordering::SeqCst);
        
        // Can't add seals if already sealed
        if current & F_SEAL_SEAL != 0 {
            return Err(MemfdError::Sealed);
        }
        
        // Can't remove seals
        if new_seals & !current != new_seals {
            return Err(MemfdError::InvalidSeal);
        }
        
        self.seals.store(new_seals, Ordering::SeqCst);
        Ok(())
    }

    /// Get seals
    pub fn get_seals(&self) -> u32 {
        self.seals.load(Ordering::SeqCst)
    }

    /// Truncate
    pub fn truncate(&self, new_size: u64) -> Result<(), MemfdError> {
        let seals = self.seals.load(Ordering::SeqCst);
        if seals & F_SEAL_SHRINK != 0 && new_size < self.size.load(Ordering::SeqCst) {
            return Err(MemfdError::Sealed);
        }
        if seals & F_SEAL_GROW != 0 && new_size > self.size.load(Ordering::SeqCst) {
            return Err(MemfdError::Sealed);
        }
        
        self.size.store(new_size, Ordering::SeqCst);
        Ok(())
    }
}

// ============================================================================
// MEMFD MANAGER
// ============================================================================

pub struct MemfdManager {
    memfds: Mutex<BTreeMap<i32, Arc<Memfd>>>,
    next_fd: AtomicI32,
}

impl MemfdManager {
    pub const fn new() -> Self {
        Self {
            memfds: Mutex::new(BTreeMap::new()),
            next_fd: AtomicI32::new(1000),
        }
    }

    pub fn create(&self, name: &str, flags: u32) -> Result<i32, MemfdError> {
        let fd = self.next_fd.fetch_add(1, Ordering::SeqCst);
        let memfd = Arc::new(Memfd::new(fd, name, flags));
        self.memfds.lock().insert(fd, memfd);
        
        crate::serial_println!("[MEMFD] Created memfd '{}' (fd={})", name, fd);
        Ok(fd)
    }

    pub fn get(&self, fd: i32) -> Option<Arc<Memfd>> {
        self.memfds.lock().get(&fd).cloned()
    }

    pub fn close(&self, fd: i32) -> bool {
        self.memfds.lock().remove(&fd).is_some()
    }
}

lazy_static::lazy_static! {
    pub static ref MEMFD_MANAGER: MemfdManager = MemfdManager::new();
}

// ============================================================================
// USERFAULTFD
// ============================================================================

/// userfaultfd flags
pub const O_NONBLOCK: u32 = 0x800;
pub const UFFD_USER_MODE_ONLY: u64 = 1;

/// userfaultfd operations
pub const UFFD_API: u64 = 0xAA;
pub const UFFD_REGISTER: u64 = 0xC0;
pub const UFFD_UNREGISTER: u64 = 0xC1;
pub const UFFD_WAKEUP: u64 = 0xC2;
pub const UFFD_COPY: u64 = 0xC3;
pub const UFFD_ZEROPAGE: u64 = 0xC4;
pub const UFFD_WRITEPROTECT: u64 = 0xC5;

/// userfaultfd event types
pub const UFFD_EVENT_PAGEFAULT: u32 = 12;
pub const UFFD_EVENT_FORK: u32 = 13;
pub const UFFD_EVENT_REMAP: u32 = 14;
pub const UFFD_EVENT_REMOVE: u32 = 15;
pub const UFFD_EVENT_UNMAP: u32 = 16;

/// Page fault flags
pub const UFFD_PAGE_FAULT_FLAG_WRITE: u64 = 1 << 0;
pub const UFFD_PAGE_FAULT_FLAG_WP: u64 = 1 << 1;

/// userfaultfd structure
pub struct UserfaultFd {
    pub fd: i32,
    pub flags: u32,
    pub registered_ranges: Mutex<Vec<(u64, u64)>>,
    pub pending_faults: Mutex<Vec<UserfaultEvent>>,
    pub features: AtomicU64,
    pub api_version: AtomicU32,
}

#[derive(Clone, Debug)]
pub struct UserfaultEvent {
    pub event_type: u32,
    pub address: u64,
    pub flags: u64,
    pub tid: u64,
}

impl UserfaultFd {
    pub fn new(fd: i32, flags: u32) -> Self {
        Self {
            fd,
            flags,
            registered_ranges: Mutex::new(Vec::new()),
            pending_faults: Mutex::new(Vec::new()),
            features: AtomicU64::new(0),
            api_version: AtomicU32::new(0),
        }
    }

    /// Register address range
    pub fn register(&self, start: u64, len: u64) -> Result<(), UserfaultError> {
        self.registered_ranges.lock().push((start, start + len));
        Ok(())
    }

    /// Unregister address range
    pub fn unregister(&self, start: u64, len: u64) -> Result<(), UserfaultError> {
        self.registered_ranges.lock().retain(|(s, e)| {
            !(*s >= start && *e <= start + len)
        });
        Ok(())
    }

    /// Generate page fault event
    pub fn generate_fault(&self, addr: u64, flags: u64) {
        let event = UserfaultEvent {
            event_type: UFFD_EVENT_PAGEFAULT,
            address: addr,
            flags,
            tid: crate::task::scheduler::current_task_id() as u64,
        };
        self.pending_faults.lock().push(event);
    }

    /// Read next event
    pub fn read_event(&self) -> Option<UserfaultEvent> {
        self.pending_faults.lock().pop()
    }

    /// Wake up waiting threads
    pub fn wakeup(&self, start: u64, len: u64) {
        // Wake threads waiting on faults in this range
    }

    /// Copy page into process
    pub fn copy(&self, dst: u64, src: u64, len: u64, wp: bool) -> Result<(), UserfaultError> {
        // Copy page from source to destination
        Ok(())
    }

    /// Zero page
    pub fn zeropage(&self, start: u64, len: u64) -> Result<(), UserfaultError> {
        // Map zero page
        Ok(())
    }
}

pub struct UserfaultManager {
    fds: Mutex<BTreeMap<i32, Arc<UserfaultFd>>>,
    next_fd: AtomicI32,
}

impl UserfaultManager {
    pub const fn new() -> Self {
        Self {
            fds: Mutex::new(BTreeMap::new()),
            next_fd: AtomicI32::new(2000),
        }
    }

    pub fn create(&self, flags: u32) -> Result<i32, UserfaultError> {
        let fd = self.next_fd.fetch_add(1, Ordering::SeqCst);
        let uffd = Arc::new(UserfaultFd::new(fd, flags));
        self.fds.lock().insert(fd, uffd);
        Ok(fd)
    }

    pub fn get(&self, fd: i32) -> Option<Arc<UserfaultFd>> {
        self.fds.lock().get(&fd).cloned()
    }

    pub fn close(&self, fd: i32) {
        self.fds.lock().remove(&fd);
    }
}

lazy_static::lazy_static! {
    pub static ref USERFAULT_MANAGER: UserfaultManager = UserfaultManager::new();
}

// ============================================================================
// SYSCALL INTERFACE
// ============================================================================

pub fn sys_memfd_create(name: &str, flags: u32) -> i32 {
    match MEMFD_MANAGER.create(name, flags) {
        Ok(fd) => fd,
        Err(_) => -22,
    }
}

pub fn sys_memfd_get_seals(fd: i32) -> i32 {
    match MEMFD_MANAGER.get(fd) {
        Some(memfd) => memfd.get_seals() as i32,
        None => -9,
    }
}

pub fn sys_memfd_add_seals(fd: i32, seals: u32) -> i32 {
    match MEMFD_MANAGER.get(fd) {
        Some(memfd) => match memfd.set_seals(seals) {
            Ok(()) => 0,
            Err(_) => -22,
        },
        None => -9,
    }
}

pub fn sys_userfaultfd(flags: u32) -> i32 {
    match USERFAULT_MANAGER.create(flags) {
        Ok(fd) => fd,
        Err(_) => -22,
    }
}

// ============================================================================
// ERROR TYPES
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemfdError {
    Sealed,
    InvalidSeal,
    TooBig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserfaultError {
    NotRegistered,
    InvalidRange,
    CopyFailed,
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init() {
    crate::serial_println!("[MEMFD] Subsystem initialized");
}
