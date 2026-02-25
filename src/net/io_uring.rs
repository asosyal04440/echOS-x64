//! # echOS io_uring Implementation
//!
//! High-performance async I/O using submission/completion queues
//! Linux-compatible io_uring interface

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::boxed::Box;
use core::mem::size_of;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// IO_URING CONSTANTS
// ============================================================================

/// io_uring opcode
pub const IORING_OP_NOP: u8 = 0;
pub const IORING_OP_READV: u8 = 1;
pub const IORING_OP_WRITEV: u8 = 2;
pub const IORING_OP_FSYNC: u8 = 3;
pub const IORING_OP_READ_FIXED: u8 = 4;
pub const IORING_OP_WRITE_FIXED: u8 = 5;
pub const IORING_OP_POLL_ADD: u8 = 6;
pub const IORING_OP_POLL_REMOVE: u8 = 7;
pub const IORING_OP_SENDMSG: u8 = 8;
pub const IORING_OP_RECVMSG: u8 = 9;
pub const IORING_OP_TIMEOUT: u8 = 11;
pub const IORING_OP_TIMEOUT_REMOVE: u8 = 12;
pub const IORING_OP_ACCEPT: u8 = 13;
pub const IORING_OP_ASYNC_CANCEL: u8 = 14;
pub const IORING_OP_LINK_TIMEOUT: u8 = 15;
pub const IORING_OP_CONNECT: u8 = 16;
pub const IORING_OP_SEND: u8 = 17;
pub const IORING_OP_RECV: u8 = 18;
pub const IORING_OP_OPENAT: u8 = 19;
pub const IORING_OP_CLOSE: u8 = 20;
pub const IORING_OP_STATX: u8 = 21;
pub const IORING_OP_SOCKET: u8 = 26;
pub const IORING_OP_PROVIDE_BUFFERS: u8 = 31;
pub const IORING_OP_REMOVE_BUFFERS: u8 = 32;

/// io_uring sqe flags
pub const IOSQE_FIXED_FILE: u8 = 1 << 0;
pub const IOSQE_ASYNC: u8 = 1 << 1;
pub const IOSQE_IO_LINK: u8 = 1 << 2;
pub const IOSQE_IO_HARDLINK: u8 = 1 << 3;
pub const IOSQE_ASYNC_NORMAL: u8 = 1 << 4;
pub const IOSQE_BUFFER_SELECT: u8 = 1 << 5;

/// io_uring features
pub const IORING_FEAT_SINGLE_MMAP: u32 = 1 << 0;
pub const IORING_FEAT_NODROP: u32 = 1 << 1;
pub const IORING_FEAT_SUBMIT_STABLE: u32 = 1 << 2;
pub const IORING_FEAT_RW_CUR_POS: u32 = 1 << 3;
pub const IORING_FEAT_CUR_PERSONALITY: u32 = 1 << 4;
pub const IORING_FEAT_FAST_POLL: u32 = 1 << 5;
pub const IORING_FEAT_POLL_32BITS: u32 = 1 << 6;

/// io_uring params flags
pub const IORING_SETUP_IOPOLL: u32 = 1 << 0;
pub const IORING_SETUP_SQPOLL: u32 = 1 << 1;
pub const IORING_SETUP_SQ_AFF: u32 = 1 << 2;
pub const IORING_SETUP_CQSIZE: u32 = 1 << 3;
pub const IORING_SETUP_CLAMP: u32 = 1 << 4;
pub const IORING_SETUP_ATTACH_WQ: u32 = 1 << 5;
pub const IORING_SETUP_R_DISABLED: u32 = 1 << 6;

// ============================================================================
// IO_URING DATA STRUCTURES
// ============================================================================

/// Submission Queue Entry (SQE)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct IoUringSqe {
    /// Opcode (IORING_OP_*)
    pub opcode: u8,
    /// Flags (IOSQE_*)
    pub flags: u8,
    /// I/O priority
    pub ioprio: u16,
    /// File descriptor
    pub fd: i32,
    /// Offset (for read/write)
    pub off: u64,
    /// Address (buffer or iovec)
    pub addr: u64,
    /// Length (buffer size or iovec count)
    pub len: u32,
    /// Operation-specific data (rw flags, etc.)
    pub rw_flags: u32,
    /// User data (passed to completion)
    pub user_data: u64,
    /// Buffer selection
    pub buf_group: u16,
    /// Personality
    pub personality: u16,
    /// Splice file descriptor
    pub splice_fd_in: i32,
    /// Padding
    pub pad: u32,
}

/// Completion Queue Entry (CQE)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct IoUringCqe {
    /// User data (from SQE)
    pub user_data: u64,
    /// Result (return value or -errno)
    pub res: i32,
    /// Flags
    pub flags: u32,
}

/// io_uring params structure
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct IoUringParams {
    /// Number of SQ entries
    pub sq_entries: u32,
    /// Number of CQ entries
    pub cq_entries: u32,
    /// Flags
    pub flags: u32,
    /// SQ thread CPU affinity
    pub sq_thread_cpu: u32,
    /// SQ thread idle timeout (ms)
    pub sq_thread_idle: u32,
    /// Features
    pub features: u32,
    /// Reserved
    pub reserved: [u32; 4],
    /// SQ ring offset (mmap)
    pub sq_off: IoUringSqOffsets,
    /// CQ ring offset (mmap)
    pub cq_off: IoUringCqOffsets,
}

/// SQ ring offsets
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct IoUringSqOffsets {
    pub head: u32,
    pub tail: u32,
    pub ring_mask: u32,
    pub ring_entries: u32,
    pub flags: u32,
    pub dropped: u32,
    pub array: u32,
    pub resv1: u32,
    pub resv2: u64,
}

/// CQ ring offsets
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct IoUringCqOffsets {
    pub head: u32,
    pub tail: u32,
    pub ring_mask: u32,
    pub ring_entries: u32,
    pub overflow: u32,
    pub cqes: u32,
    pub flags: u32,
    pub resv1: u32,
    pub resv2: u64,
}

// ============================================================================
// IO_URING RING BUFFER
// ============================================================================

/// Ring buffer for SQ/CQ
pub struct IoUringRing<T> {
    /// Ring buffer memory
    buffer: *mut T,
    /// Physical address
    paddr: usize,
    /// Number of entries
    entries: u32,
    /// Ring mask (entries - 1)
    mask: u32,
    /// Head index
    head: u32,
    /// Tail index
    tail: u32,
}

impl<T> Clone for IoUringRing<T> {
    fn clone(&self) -> Self {
        IoUringRing {
            buffer: self.buffer,
            paddr: self.paddr,
            entries: self.entries,
            mask: self.mask,
            head: self.head,
            tail: self.tail,
        }
    }
}

impl<T> IoUringRing<T> {
    /// Create a new ring buffer
    pub fn new(entries: u32) -> Option<Self> {
        let size = entries as usize * size_of::<T>();
        let pages = (size + 4095) / 4096;
        
        let (paddr, vaddr) = crate::memory::dma_alloc(pages)?;
        
        // Zero the buffer
        unsafe {
            core::ptr::write_bytes(vaddr.as_ptr(), 0, size);
        }
        
        Some(IoUringRing {
            buffer: vaddr.as_ptr() as *mut T,
            paddr,
            entries,
            mask: entries - 1,
            head: 0,
            tail: 0,
        })
    }
    
    /// Get entry at index
    pub unsafe fn get(&self, index: u32) -> &T {
        &*(self.buffer.add((index & self.mask) as usize))
    }
    
    /// Get mutable entry at index
    pub unsafe fn get_mut(&mut self, index: u32) -> &mut T {
        &mut *(self.buffer.add((index & self.mask) as usize))
    }
    
    /// Get head
    pub fn head(&self) -> u32 {
        self.head
    }
    
    /// Get tail
    pub fn tail(&self) -> u32 {
        self.tail
    }
    
    /// Advance head
    pub fn advance_head(&mut self) {
        self.head = self.head.wrapping_add(1);
    }
    
    /// Advance tail
    pub fn advance_tail(&mut self) {
        self.tail = self.tail.wrapping_add(1);
    }
    
    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.head == self.tail
    }
    
    /// Check if full
    pub fn is_full(&self) -> bool {
        self.tail.wrapping_sub(self.head) == self.entries
    }
    
    /// Get count
    pub fn count(&self) -> u32 {
        self.tail.wrapping_sub(self.head)
    }
    
    /// Get entries
    pub fn entries(&self) -> u32 {
        self.entries
    }
    
    /// Get mask
    pub fn mask(&self) -> u32 {
        self.mask
    }
}

impl<T> Drop for IoUringRing<T> {
    fn drop(&mut self) {
        if self.paddr != 0 {
            let size = self.entries as usize * size_of::<T>();
            let pages = (size + 4095) / 4096;
            crate::memory::dma_dealloc(self.paddr, pages);
        }
    }
}

unsafe impl<T: Send> Send for IoUringRing<T> {}
unsafe impl<T: Sync> Sync for IoUringRing<T> {}

// ============================================================================
// IO_URING INSTANCE
// ============================================================================

/// io_uring instance
#[derive(Clone)]
pub struct IoUring {
    /// Instance ID
    pub id: u32,
    /// SQ ring
    pub sq_ring: IoUringRing<IoUringSqe>,
    /// CQ ring
    pub cq_ring: IoUringRing<IoUringCqe>,
    /// SQ array (index to SQE) - stored as Vec for thread safety
    pub sq_array: Vec<u32>,
    /// Parameters
    pub params: IoUringParams,
    /// Pending operations
    pub pending: BTreeMap<u64, IoUringSqe>,
    /// Next user data
    pub next_user_data: u64,
    /// SQ poll thread active
    pub sq_poll_active: bool,
}

impl IoUring {
    /// Create a new io_uring instance
    pub fn new(entries: u32, params: Option<IoUringParams>) -> Option<Self> {
        let sq_entries = entries.next_power_of_two();
        let cq_entries = sq_entries * 2; // CQ is usually 2x SQ size
        
        let sq_ring = IoUringRing::new(sq_entries)?;
        let cq_ring = IoUringRing::new(cq_entries)?;
        
        // Allocate SQ array as Vec
        let sq_array = alloc::vec![0u32; sq_entries as usize];
        
        let mut io_uring_params = params.unwrap_or_default();
        io_uring_params.sq_entries = sq_entries;
        io_uring_params.cq_entries = cq_entries;
        io_uring_params.features = IORING_FEAT_SINGLE_MMAP 
            | IORING_FEAT_NODROP 
            | IORING_FEAT_FAST_POLL;
        
        Some(IoUring {
            id: 0,
            sq_ring,
            cq_ring,
            sq_array,
            params: io_uring_params,
            pending: BTreeMap::new(),
            next_user_data: 1,
            sq_poll_active: false,
        })
    }
    
    /// Get next user data
    pub fn next_user_data(&mut self) -> u64 {
        let ud = self.next_user_data;
        self.next_user_data += 1;
        ud
    }
    
    /// Submit SQE to SQ
    pub fn submit_sqe(&mut self, sqe: IoUringSqe) -> Result<(), IoUringError> {
        if self.sq_ring.is_full() {
            return Err(IoUringError::QueueFull);
        }
        
        // Get next SQ slot
        let tail = self.sq_ring.tail();
        let idx = (tail & self.sq_ring.mask()) as usize;
        
        // Write SQE
        unsafe {
            *self.sq_ring.get_mut(idx as u32) = sqe;
        }
        
        // Update SQ array
        self.sq_array[idx] = idx as u32;
        
        // Advance tail
        self.sq_ring.advance_tail();
        
        // Track pending
        self.pending.insert(sqe.user_data, sqe);
        
        Ok(())
    }
    
    /// Get CQE from CQ
    pub fn get_cqe(&mut self) -> Option<IoUringCqe> {
        if self.cq_ring.is_empty() {
            return None;
        }
        
        let head = self.cq_ring.head();
        let idx = head & self.cq_ring.mask();
        
        let cqe = unsafe { *self.cq_ring.get(idx) };
        
        // Remove from pending
        self.pending.remove(&cqe.user_data);
        
        // Advance head
        self.cq_ring.advance_head();
        
        Some(cqe)
    }
    
    /// Peek CQE without removing
    pub fn peek_cqe(&self) -> Option<IoUringCqe> {
        if self.cq_ring.is_empty() {
            return None;
        }
        
        let head = self.cq_ring.head();
        let idx = head & self.cq_ring.mask();
        
        Some(unsafe { *self.cq_ring.get(idx) })
    }
    
    /// Wait for CQE
    pub fn wait_cqe(&mut self, timeout_ms: u64) -> Option<IoUringCqe> {
        let start = crate::interrupts::get_ticks();
        
        loop {
            if let Some(cqe) = self.get_cqe() {
                return Some(cqe);
            }
            
            // Check timeout
            if timeout_ms > 0 {
                let elapsed = crate::interrupts::get_ticks() - start;
                if elapsed >= timeout_ms {
                    return None;
                }
            }
            
            // Yield CPU
            crate::task::scheduler::schedule();
        }
    }
    
    /// Process pending SQEs
    pub fn process_pending(&mut self) -> u32 {
        let mut processed = 0u32;
        
        // Process all pending SQEs
        let pending: Vec<(u64, IoUringSqe)> = self.pending.iter()
            .map(|(k, v)| (*k, *v))
            .collect();
        
        for (user_data, sqe) in pending {
            let result = self.execute_op(&sqe);
            
            // Create CQE
            let cqe = IoUringCqe {
                user_data,
                res: result,
                flags: 0,
            };
            
            // Add to CQ
            if !self.cq_ring.is_full() {
                let tail = self.cq_ring.tail();
                let idx = tail & self.cq_ring.mask();
                unsafe {
                    *self.cq_ring.get_mut(idx) = cqe;
                }
                self.cq_ring.advance_tail();
                processed += 1;
            }
        }
        
        processed
    }
    
    /// Execute operation
    fn execute_op(&self, sqe: &IoUringSqe) -> i32 {
        match sqe.opcode {
            IORING_OP_NOP => 0,
            
            IORING_OP_READ => {
                // Read from file/socket
                let fd = sqe.fd as u32;
                let buf = sqe.addr as *mut u8;
                let len = sqe.len as usize;
                
                // Try socket read
                if let Ok(n) = crate::net::socket::recv(fd, unsafe { 
                    core::slice::from_raw_parts_mut(buf, len) 
                }, 0) {
                    n as i32
                } else {
                    -5 // -EIO
                }
            }
            
            IORING_OP_WRITE => {
                // Write to file/socket
                let fd = sqe.fd as u32;
                let buf = sqe.addr as *const u8;
                let len = sqe.len as usize;
                
                // Try socket write
                if let Ok(n) = crate::net::socket::send(fd, unsafe { 
                    core::slice::from_raw_parts(buf, len) 
                }, 0) {
                    n as i32
                } else {
                    -5 // -EIO
                }
            }
            
            IORING_OP_SEND => {
                let fd = sqe.fd as u32;
                let buf = sqe.addr as *const u8;
                let len = sqe.len as usize;
                
                if let Ok(n) = crate::net::socket::send(fd, unsafe { 
                    core::slice::from_raw_parts(buf, len) 
                }, 0) {
                    n as i32
                } else {
                    -5
                }
            }
            
            IORING_OP_RECV => {
                let fd = sqe.fd as u32;
                let buf = sqe.addr as *mut u8;
                let len = sqe.len as usize;
                
                if let Ok(n) = crate::net::socket::recv(fd, unsafe { 
                    core::slice::from_raw_parts_mut(buf, len) 
                }, 0) {
                    n as i32
                } else {
                    -5
                }
            }
            
            IORING_OP_ACCEPT => {
                let fd = sqe.fd as u32;
                match crate::net::socket::accept(fd) {
                    Ok((new_fd, _addr)) => new_fd as i32,
                    Err(_) => -11, // -EAGAIN
                }
            }
            
            IORING_OP_CONNECT => {
                let fd = sqe.fd as u32;
                // Parse sockaddr from sqe.addr
                // Simplified: assume IPv4
                let addr_bytes = unsafe { 
                    core::slice::from_raw_parts(sqe.addr as *const u8, 16) 
                };
                let ip = crate::net::Ipv4Addr::from_bytes([
                    addr_bytes[4], addr_bytes[5], addr_bytes[6], addr_bytes[7]
                ]);
                let port = u16::from_be_bytes([addr_bytes[2], addr_bytes[3]]);
                let addr = crate::net::SocketAddr::new(ip, crate::net::Port(port));
                
                match crate::net::socket::connect(fd, addr) {
                    Ok(()) => 0,
                    Err(_) => -111, // -ECONNREFUSED
                }
            }
            
            IORING_OP_SOCKET => {
                // Create socket
                let domain = sqe.fd;
                let sock_type = (sqe.len >> 16) as i32;
                let protocol = (sqe.len & 0xFFFF) as i32;
                
                let af = match domain {
                    2 => crate::net::socket::AddressFamily::IPV4,
                    10 => crate::net::socket::AddressFamily::IPV6,
                    _ => return -22, // -EINVAL
                };
                
                let st = match sock_type {
                    1 => crate::net::socket::SocketType::STREAM,
                    2 => crate::net::socket::SocketType::DGRAM,
                    _ => return -22,
                };
                
                let proto = match protocol {
                    0 => crate::net::socket::Protocol::DEFAULT,
                    6 => crate::net::socket::Protocol::TCP,
                    17 => crate::net::socket::Protocol::UDP,
                    _ => return -22,
                };
                
                match crate::net::socket::socket(af, st, proto) {
                    Ok(fd) => fd as i32,
                    Err(_) => -24, // -EMFILE
                }
            }
            
            IORING_OP_CLOSE => {
                let fd = sqe.fd as u32;
                match crate::net::socket::close(fd) {
                    Ok(()) => 0,
                    Err(_) => -9, // -EBADF
                }
            }
            
            IORING_OP_POLL_ADD => {
                let fd = sqe.fd as u32;
                let events = sqe.len as u16;
                
                // Check if events are ready
                let ready = if events & 1 != 0 {
                    crate::net::socket::can_read(fd)
                } else {
                    false
                };
                
                let ready = ready || if events & 4 != 0 {
                    crate::net::socket::can_write(fd)
                } else {
                    false
                };
                
                if ready {
                    events as i32
                } else {
                    -11 // -EAGAIN
                }
            }
            
            IORING_OP_TIMEOUT => {
                // Timeout operation
                let ts = unsafe { &*(sqe.addr as *const IoUringTimeout) };
                let timeout_ns = ts.ts_nsec;
                let timeout_ms = timeout_ns / 1_000_000;
                
                // Wait for timeout
                let start = crate::interrupts::get_ticks();
                loop {
                    let elapsed = crate::interrupts::get_ticks() - start;
                    if elapsed >= timeout_ms as u64 {
                        break;
                    }
                    crate::task::scheduler::schedule();
                }
                
                -62 // -ETIME
            }
            
            _ => -22, // -EINVAL
        }
    }
}

/// io_uring timeout structure
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct IoUringTimeout {
    pub ts_sec: u64,
    pub ts_nsec: u64,
    pub flags: u32,
    pub count: u32,
}

/// io_uring error
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IoUringError {
    QueueFull,
    QueueEmpty,
    InvalidParam,
    NoMemory,
    NotReady,
}

// ============================================================================
// IO_URING MANAGER
// ============================================================================

static IO_URING_INSTANCES: Mutex<BTreeMap<u32, Box<IoUring>>> = Mutex::new(BTreeMap::new());
static NEXT_IO_URING_ID: AtomicU32 = AtomicU32::new(1);

/// Create io_uring instance
pub fn io_uring_setup(entries: u32, params: Option<IoUringParams>) -> Result<u32, IoUringError> {
    let mut instances = IO_URING_INSTANCES.lock();
    
    let id = NEXT_IO_URING_ID.fetch_add(1, Ordering::Relaxed);
    let mut io_uring = IoUring::new(entries, params).ok_or(IoUringError::NoMemory)?;
    io_uring.id = id;
    
    instances.insert(id, Box::new(io_uring));
    
    Ok(id)
}

/// Enter io_uring (submit/wait)
pub fn io_uring_enter(fd: u32, to_submit: u32, min_complete: u32, flags: u32) -> Result<u32, IoUringError> {
    let mut instances = IO_URING_INSTANCES.lock();
    let io_uring = instances.get_mut(&fd).ok_or(IoUringError::InvalidParam)?;
    
    let mut submitted = 0u32;
    let mut completed = 0u32;
    
    // Submit SQEs
    if to_submit > 0 {
        submitted = io_uring.process_pending();
    }
    
    // Wait for completions
    if min_complete > 0 {
        for _ in 0..min_complete {
            if io_uring.get_cqe().is_some() {
                completed += 1;
            } else if flags & 1 != 0 {
                // Non-blocking
                break;
            } else {
                // Blocking wait
                let start = crate::interrupts::get_ticks();
                loop {
                    if io_uring.get_cqe().is_some() {
                        completed += 1;
                        break;
                    }
                    
                    // Timeout check (30 seconds)
                    if crate::interrupts::get_ticks() - start > 30000 {
                        break;
                    }
                    
                    crate::task::scheduler::schedule();
                }
            }
        }
    }
    
    Ok(submitted.max(completed))
}

/// Register buffers/files
pub fn io_uring_register(fd: u32, opcode: u32, arg: u64, nr_args: u32) -> Result<i32, IoUringError> {
    let _instances = IO_URING_INSTANCES.lock();
    
    match opcode {
        0 => {
            // Register files
            // TODO: Implement file registration
            Ok(0)
        }
        1 => {
            // Register buffers
            // TODO: Implement buffer registration
            Ok(0)
        }
        _ => Err(IoUringError::InvalidParam)
    }
}

/// Close io_uring instance
pub fn io_uring_close(fd: u32) -> Result<(), IoUringError> {
    let mut instances = IO_URING_INSTANCES.lock();
    instances.remove(&fd).map(|_| ()).ok_or(IoUringError::InvalidParam)
}

/// Get io_uring instance
pub fn get_io_uring(fd: u32) -> Option<IoUring> {
    let instances = IO_URING_INSTANCES.lock();
    instances.get(&fd).map(|i| (**i).clone())
}

/// Get SQE for submission
pub fn get_sqe(fd: u32) -> Option<IoUringSqe> {
    let instances = IO_URING_INSTANCES.lock();
    let io_uring = instances.get(&fd)?;
    
    if io_uring.sq_ring.is_full() {
        return None;
    }
    
    Some(IoUringSqe::default())
}

/// Submit SQE
pub fn submit_sqe(fd: u32, sqe: IoUringSqe) -> Result<(), IoUringError> {
    let mut instances = IO_URING_INSTANCES.lock();
    let io_uring = instances.get_mut(&fd).ok_or(IoUringError::InvalidParam)?;
    io_uring.submit_sqe(sqe)
}

/// Get CQE
pub fn get_cqe(fd: u32) -> Option<IoUringCqe> {
    let mut instances = IO_URING_INSTANCES.lock();
    let io_uring = instances.get_mut(&fd)?;
    io_uring.get_cqe()
}

/// Wait for CQE
pub fn wait_cqe(fd: u32, timeout_ms: u64) -> Option<IoUringCqe> {
    let mut instances = IO_URING_INSTANCES.lock();
    let io_uring = instances.get_mut(&fd)?;
    io_uring.wait_cqe(timeout_ms)
}

/// Initialize io_uring subsystem
pub fn init() {
    crate::serial_println!("[IO_URING] Initialized");
}
