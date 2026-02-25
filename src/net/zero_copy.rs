//! # Zero-Copy Networking
//!
//! High-performance zero-copy network I/O with io_uring-style interface
//! Supports scatter-gather I/O, memory-mapped buffers, and async operations

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;
use core::sync::atomic::{AtomicU64, AtomicU32, AtomicBool, Ordering};
use core::mem;

use super::{NetError, MacAddr, Ipv4Addr, SocketAddr};
use super::ethernet::{EthernetFrame, EthernetHeader, EtherType};

// ============================================================================
// ZERO-COPY CONSTANTS
// ============================================================================

/// Maximum buffer pool size (16MB)
const BUFFER_POOL_SIZE: usize = 16 * 1024 * 1024;

/// Buffer chunk size (page-aligned, 4KB)
const BUFFER_CHUNK_SIZE: usize = 4096;

/// Maximum number of buffer chunks
const MAX_CHUNKS: usize = BUFFER_POOL_SIZE / BUFFER_CHUNK_SIZE;

/// Maximum ring entries
const RING_SIZE: usize = 4096;

/// Maximum scatter-gather segments
const MAX_IOV: usize = 8;

// ============================================================================
// BUFFER DESCRIPTOR
// ============================================================================

/// Buffer descriptor for zero-copy I/O
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BufferDescriptor {
    /// Buffer ID
    pub buf_id: u32,
    /// Physical address of buffer
    pub phys_addr: u64,
    /// Virtual address (for kernel access)
    pub virt_addr: u64,
    /// Buffer length
    pub len: u32,
    /// Reference count
    pub ref_count: u32,
    /// Flags
    pub flags: u32,
}

impl BufferDescriptor {
    pub const FLAG_TX: u32 = 1 << 0;
    pub const FLAG_RX: u32 = 1 << 1;
    pub const FLAG_IN_USE: u32 = 1 << 2;
    pub const FLAG_MAPPED: u32 = 1 << 3;
    
    pub fn new(buf_id: u32, phys_addr: u64, virt_addr: u64, len: u32) -> Self {
        BufferDescriptor {
            buf_id,
            phys_addr,
            virt_addr,
            len,
            ref_count: 0,
            flags: 0,
        }
    }
}

// ============================================================================
// SCATTER-GATHER VECTOR
// ============================================================================

/// I/O vector for scatter-gather operations
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct IoVec {
    /// Buffer ID
    pub buf_id: u32,
    /// Offset within buffer
    pub offset: u32,
    /// Length of this segment
    pub len: u32,
}

impl IoVec {
    pub fn new(buf_id: u32, offset: u32, len: u32) -> Self {
        IoVec { buf_id, offset, len }
    }
}

// ============================================================================
// SUBMISSION QUEUE ENTRY
// ============================================================================

/// Operation type
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpCode {
    /// Receive packet
    Recv = 0,
    /// Send packet
    Send = 1,
    /// Accept connection
    Accept = 2,
    /// Connect to remote
    Connect = 3,
    /// Close socket
    Close = 4,
    /// Allocate buffer
    AllocBuf = 5,
    /// Free buffer
    FreeBuf = 6,
    /// Map buffer to userspace
    MapBuf = 7,
    /// Unmap buffer
    UnmapBuf = 8,
}

/// Submission queue entry (SQE)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Sqe {
    /// Operation code
    pub opcode: OpCode,
    /// Flags
    pub flags: u8,
    /// Socket ID (for socket operations)
    pub socket_id: u32,
    /// User data (passed to completion)
    pub user_data: u64,
    /// Buffer ID (for buffer operations)
    pub buf_id: u32,
    /// Number of I/O vectors
    pub iov_count: u8,
    /// Reserved
    pub reserved: [u8; 3],
    /// Scatter-gather vectors
    pub iov: [IoVec; MAX_IOV],
    /// Remote address (for connect/accept)
    pub addr: SocketAddr,
}

impl Sqe {
    pub fn new(opcode: OpCode, socket_id: u32, user_data: u64) -> Self {
        Sqe {
            opcode,
            flags: 0,
            socket_id,
            user_data,
            buf_id: 0,
            iov_count: 0,
            reserved: [0; 3],
            iov: [IoVec::new(0, 0, 0); MAX_IOV],
            addr: SocketAddr::default(),
        }
    }
    
    pub fn with_buffer(opcode: OpCode, socket_id: u32, buf_id: u32, user_data: u64) -> Self {
        let mut sqe = Self::new(opcode, socket_id, user_data);
        sqe.buf_id = buf_id;
        sqe
    }
    
    pub fn with_iov(opcode: OpCode, socket_id: u32, iov: &[IoVec], user_data: u64) -> Self {
        let mut sqe = Self::new(opcode, socket_id, user_data);
        sqe.iov_count = iov.len().min(MAX_IOV) as u8;
        sqe.iov[..iov.len()].copy_from_slice(iov);
        sqe
    }
}

/// SQE flags
pub const SQE_FLAG_IOV: u8 = 1 << 0;
pub const SQE_FLAG_FIXED_BUF: u8 = 1 << 1;
pub const SQE_FLAG_NONBLOCK: u8 = 1 << 2;

// ============================================================================
// COMPLETION QUEUE ENTRY
// ============================================================================

/// Completion queue entry (CQE)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Cqe {
    /// User data from SQE
    pub user_data: u64,
    /// Result (positive = bytes transferred, negative = error)
    pub result: i32,
    /// Flags
    pub flags: u32,
    /// Buffer ID (for recv operations)
    pub buf_id: u32,
    /// Reserved
    pub reserved: [u32; 2],
}

impl Cqe {
    pub fn new(user_data: u64, result: i32, buf_id: u32) -> Self {
        Cqe {
            user_data,
            result,
            flags: 0,
            buf_id,
            reserved: [0; 2],
        }
    }
    
    pub fn success(user_data: u64, bytes: u32) -> Self {
        Self::new(user_data, bytes as i32, 0)
    }
    
    pub fn error(user_data: u64, err: NetError) -> Self {
        Self::new(user_data, -(err as i32), 0)
    }
}

// ============================================================================
// RING BUFFER
// ============================================================================

/// Ring buffer for submission/completion queues
pub struct RingBuffer<T: Copy + Clone> {
    /// Ring entries
    entries: Vec<T>,
    /// Head index (where consumer reads)
    head: AtomicU32,
    /// Tail index (where producer writes)
    tail: AtomicU32,
    /// Ring size
    size: u32,
    /// Ring mask (size - 1, for fast modulo)
    mask: u32,
}

impl<T: Copy + Clone> RingBuffer<T> {
    pub fn new(size: usize) -> Self {
        let size = size.next_power_of_two();
        RingBuffer {
            entries: vec![unsafe { mem::zeroed() }; size],
            head: AtomicU32::new(0),
            tail: AtomicU32::new(0),
            size: size as u32,
            mask: (size - 1) as u32,
        }
    }
    
    /// Check if ring is empty
    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::Acquire) == self.tail.load(Ordering::Acquire)
    }
    
    /// Check if ring is full
    pub fn is_full(&self) -> bool {
        let tail = self.tail.load(Ordering::Acquire);
        let head = self.head.load(Ordering::Acquire);
        tail.wrapping_sub(head) >= self.size
    }
    
    /// Get number of entries
    pub fn len(&self) -> usize {
        let tail = self.tail.load(Ordering::Acquire);
        let head = self.head.load(Ordering::Acquire);
        (tail.wrapping_sub(head) as usize) & (self.size as usize - 1)
    }
    
    /// Push entry to ring
    pub fn push(&mut self, entry: T) -> bool {
        if self.is_full() {
            return false;
        }
        
        let tail = self.tail.load(Ordering::Acquire);
        let idx = tail & self.mask;
        self.entries[idx as usize] = entry;
        
        // Memory barrier
        core::sync::atomic::fence(Ordering::Release);
        
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        true
    }
    
    /// Pop entry from ring
    pub fn pop(&mut self) -> Option<T> {
        if self.is_empty() {
            return None;
        }
        
        let head = self.head.load(Ordering::Acquire);
        let idx = head & self.mask;
        let entry = self.entries[idx as usize];
        
        // Memory barrier
        core::sync::atomic::fence(Ordering::Release);
        
        self.head.store(head.wrapping_add(1), Ordering::Release);
        Some(entry)
    }
    
    /// Peek at head entry
    pub fn peek(&self) -> Option<T> {
        if self.is_empty() {
            return None;
        }
        
        let head = self.head.load(Ordering::Acquire);
        let idx = head & self.mask;
        Some(self.entries[idx as usize])
    }
}

// ============================================================================
// BUFFER POOL
// ============================================================================

/// Buffer pool for zero-copy operations
pub struct BufferPool {
    /// Buffer descriptors
    descriptors: Vec<BufferDescriptor>,
    /// Free buffer IDs
    free_list: VecDeque<u32>,
    /// Pool base physical address
    base_phys: u64,
    /// Pool base virtual address
    base_virt: u64,
    /// Total chunks
    total_chunks: usize,
    /// Available chunks
    available: AtomicU32,
}

impl BufferPool {
    /// Create new buffer pool
    pub fn new() -> Option<Self> {
        let total_chunks = MAX_CHUNKS;
        
        // Allocate contiguous physical memory for DMA
        let pages = (BUFFER_POOL_SIZE + 4095) / 4096;
        let (phys, virt) = crate::memory::dma_alloc(pages)?;
        
        // Zero the pool
        unsafe {
            core::ptr::write_bytes(virt.as_ptr(), 0, BUFFER_POOL_SIZE);
        }
        
        // Create descriptors
        let mut descriptors = Vec::with_capacity(total_chunks);
        let mut free_list = VecDeque::with_capacity(total_chunks);
        
        for i in 0..total_chunks {
            let chunk_phys = phys as u64 + (i * BUFFER_CHUNK_SIZE) as u64;
            let chunk_virt = virt.as_ptr() as u64 + (i * BUFFER_CHUNK_SIZE) as u64;
            
            descriptors.push(BufferDescriptor::new(
                i as u32,
                chunk_phys,
                chunk_virt,
                BUFFER_CHUNK_SIZE as u32,
            ));
            
            free_list.push_back(i as u32);
        }
        
        crate::serial_println!("[ZC-NET] Buffer pool initialized: {} chunks ({} MB)", 
            total_chunks, BUFFER_POOL_SIZE / (1024 * 1024));
        
        Some(BufferPool {
            descriptors,
            free_list,
            base_phys: phys as u64,
            base_virt: virt.as_ptr() as u64,
            total_chunks,
            available: AtomicU32::new(total_chunks as u32),
        })
    }
    
    /// Allocate a buffer
    pub fn alloc(&mut self) -> Option<u32> {
        let buf_id = self.free_list.pop_front()?;
        self.descriptors[buf_id as usize].ref_count = 1;
        self.descriptors[buf_id as usize].flags |= BufferDescriptor::FLAG_IN_USE;
        self.available.fetch_sub(1, Ordering::Relaxed);
        Some(buf_id)
    }
    
    /// Allocate multiple contiguous buffers
    pub fn alloc_contiguous(&mut self, count: usize) -> Option<u32> {
        if count > self.free_list.len() {
            return None;
        }
        
        // Try to find contiguous range
        let mut start_id = None;
        let mut consecutive = 0;
        
        for &id in &self.free_list {
            if let Some(start) = start_id {
                if id == start + consecutive as u32 {
                    consecutive += 1;
                    if consecutive >= count {
                        break;
                    }
                } else {
                    start_id = Some(id);
                    consecutive = 1;
                }
            } else {
                start_id = Some(id);
                consecutive = 1;
            }
        }
        
        if consecutive < count {
            return None;
        }
        
        // Remove from free list
        let start = start_id.unwrap();
        for i in 0..count {
            self.free_list.retain(|&id| id != start + i as u32);
            self.descriptors[(start + i as u32) as usize].ref_count = 1;
            self.descriptors[(start + i as u32) as usize].flags |= BufferDescriptor::FLAG_IN_USE;
        }
        
        self.available.fetch_sub(count as u32, Ordering::Relaxed);
        Some(start)
    }
    
    /// Free a buffer
    pub fn free(&mut self, buf_id: u32) {
        if buf_id as usize >= self.descriptors.len() {
            return;
        }
        
        let desc = &mut self.descriptors[buf_id as usize];
        if desc.ref_count > 0 {
            desc.ref_count -= 1;
            
            if desc.ref_count == 0 {
                desc.flags &= !BufferDescriptor::FLAG_IN_USE;
                self.free_list.push_back(buf_id);
                self.available.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    
    /// Increment reference count
    pub fn get(&mut self, buf_id: u32) {
        if (buf_id as usize) < self.descriptors.len() {
            self.descriptors[buf_id as usize].ref_count += 1;
        }
    }
    
    /// Get buffer descriptor
    pub fn get_descriptor(&self, buf_id: u32) -> Option<&BufferDescriptor> {
        self.descriptors.get(buf_id as usize)
    }
    
    /// Get buffer virtual address
    pub fn get_virt_addr(&self, buf_id: u32) -> Option<u64> {
        self.descriptors.get(buf_id as usize).map(|d| d.virt_addr)
    }
    
    /// Get buffer physical address
    pub fn get_phys_addr(&self, buf_id: u32) -> Option<u64> {
        self.descriptors.get(buf_id as usize).map(|d| d.phys_addr)
    }
    
    /// Get available buffer count
    pub fn available(&self) -> u32 {
        self.available.load(Ordering::Relaxed)
    }
    
    /// Write data to buffer
    pub fn write(&mut self, buf_id: u32, offset: usize, data: &[u8]) -> Result<(), NetError> {
        let desc = self.descriptors.get_mut(buf_id as usize)
            .ok_or(NetError::InvalidParam)?;
        
        if offset + data.len() > desc.len as usize {
            return Err(NetError::BufferFull);
        }
        
        unsafe {
            let dst = (desc.virt_addr + offset as u64) as *mut u8;
            core::ptr::copy_nonoverlapping(data.as_ptr(), dst, data.len());
        }
        
        Ok(())
    }
    
    /// Read data from buffer
    pub fn read(&self, buf_id: u32, offset: usize, len: usize) -> Option<Vec<u8>> {
        let desc = self.descriptors.get(buf_id as usize)?;
        
        if offset + len > desc.len as usize {
            return None;
        }
        
        let mut data = vec![0u8; len];
        unsafe {
            let src = (desc.virt_addr + offset as u64) as *const u8;
            core::ptr::copy_nonoverlapping(src, data.as_mut_ptr(), len);
        }
        
        Some(data)
    }
}

// ============================================================================
// IO_URING INTERFACE
// ============================================================================

/// Zero-copy I/O ring interface
pub struct IoUring {
    /// Submission queue
    sq: RingBuffer<Sqe>,
    /// Completion queue
    cq: RingBuffer<Cqe>,
    /// Buffer pool
    buffers: BufferPool,
    /// Pending operations count
    pending: AtomicU32,
    /// Ring ID
    ring_id: u32,
    /// Active flag
    active: AtomicBool,
}

impl IoUring {
    /// Create new I/O ring
    pub fn new(ring_id: u32) -> Option<Self> {
        let buffers = BufferPool::new()?;
        
        Some(IoUring {
            sq: RingBuffer::new(RING_SIZE),
            cq: RingBuffer::new(RING_SIZE),
            buffers,
            pending: AtomicU32::new(0),
            ring_id,
            active: AtomicBool::new(true),
        })
    }
    
    /// Submit operation
    pub fn submit(&mut self, sqe: Sqe) -> Result<(), NetError> {
        if !self.active.load(Ordering::Relaxed) {
            return Err(NetError::NotSupported);
        }
        
        if !self.sq.push(sqe) {
            return Err(NetError::BufferFull);
        }
        
        self.pending.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    
    /// Get completion
    pub fn complete(&mut self) -> Option<Cqe> {
        let cqe = self.cq.pop()?;
        self.pending.fetch_sub(1, Ordering::Relaxed);
        Some(cqe)
    }
    
    /// Peek at completion
    pub fn peek_completion(&self) -> Option<Cqe> {
        self.cq.peek()
    }
    
    /// Process pending submissions
    pub fn process(&mut self) -> usize {
        let mut processed = 0;
        
        while let Some(sqe) = self.sq.pop() {
            let result = self.process_sqe(&sqe);
            let cqe = match result {
                Ok(bytes) => Cqe::success(sqe.user_data, bytes),
                Err(err) => Cqe::error(sqe.user_data, err),
            };
            
            self.cq.push(cqe);
            processed += 1;
        }
        
        processed
    }
    
    /// Process single SQE
    fn process_sqe(&mut self, sqe: &Sqe) -> Result<u32, NetError> {
        match sqe.opcode {
            OpCode::AllocBuf => {
                let buf_id = self.buffers.alloc().ok_or(NetError::BufferFull)?;
                Ok(buf_id)
            }
            OpCode::FreeBuf => {
                self.buffers.free(sqe.buf_id);
                Ok(0)
            }
            OpCode::Send => {
                self.process_send(sqe)
            }
            OpCode::Recv => {
                self.process_recv(sqe)
            }
            OpCode::MapBuf => {
                // Map buffer to userspace
                // Would set up page table mappings
                Ok(sqe.buf_id)
            }
            OpCode::UnmapBuf => {
                // Unmap buffer from userspace
                Ok(0)
            }
            _ => Err(NetError::NotSupported),
        }
    }
    
    /// Process send operation
    fn process_send(&mut self, sqe: &Sqe) -> Result<u32, NetError> {
        // Gather data from I/O vectors
        let mut total_len = 0;
        let mut packet_data = Vec::new();
        
        for i in 0..sqe.iov_count as usize {
            let iov = &sqe.iov[i];
            if let Some(data) = self.buffers.read(iov.buf_id, iov.offset as usize, iov.len as usize) {
                packet_data.extend_from_slice(&data);
                total_len += data.len();
            }
        }
        
        if packet_data.is_empty() {
            return Err(NetError::BufferEmpty);
        }
        
        // Send through network interface
        super::send_packet(&packet_data)?;
        
        // Free buffers if not fixed
        if sqe.flags & SQE_FLAG_FIXED_BUF == 0 {
            for i in 0..sqe.iov_count as usize {
                self.buffers.free(sqe.iov[i].buf_id);
            }
        }
        
        Ok(total_len as u32)
    }
    
    /// Process receive operation
    fn process_recv(&mut self, sqe: &Sqe) -> Result<u32, NetError> {
        // Try to receive packet
        let iface = super::default_interface().ok_or(NetError::NoInterface)?;
        let packet = iface.lock().recv().ok_or(NetError::WouldBlock)?;
        
        // Allocate buffer for received data
        let buf_id = self.buffers.alloc().ok_or(NetError::BufferFull)?;
        let len = packet.len().min(BUFFER_CHUNK_SIZE);
        
        self.buffers.write(buf_id, 0, &packet[..len])?;
        
        Ok(len as u32)
    }
    
    /// Allocate buffer
    pub fn alloc_buffer(&mut self) -> Option<u32> {
        self.buffers.alloc()
    }
    
    /// Free buffer
    pub fn free_buffer(&mut self, buf_id: u32) {
        self.buffers.free(buf_id);
    }
    
    /// Get buffer pool statistics
    pub fn buffer_stats(&self) -> (u32, u32) {
        (self.buffers.total_chunks as u32, self.buffers.available())
    }
    
    /// Get pending operations count
    pub fn pending_count(&self) -> u32 {
        self.pending.load(Ordering::Relaxed)
    }
    
    /// Get ring ID
    pub fn id(&self) -> u32 {
        self.ring_id
    }
}

// ============================================================================
// GLOBAL IO_URING INSTANCE
// ============================================================================

lazy_static::lazy_static! {
    static ref IO_RINGS: Mutex<Vec<Arc<Mutex<IoUring>>>> = Mutex::new(Vec::new());
    static ref NEXT_RING_ID: AtomicU32 = AtomicU32::new(1);
}

/// Create new I/O ring
pub fn create_ring() -> Option<u32> {
    let ring_id = NEXT_RING_ID.fetch_add(1, Ordering::Relaxed);
    let ring = IoUring::new(ring_id)?;
    
    IO_RINGS.lock().push(Arc::new(Mutex::new(ring)));
    
    crate::serial_println!("[ZC-NET] Created I/O ring {}", ring_id);
    Some(ring_id)
}

/// Get I/O ring by ID
pub fn get_ring(ring_id: u32) -> Option<Arc<Mutex<IoUring>>> {
    IO_RINGS.lock().iter()
        .find(|r| r.lock().id() == ring_id)
        .cloned()
}

/// Process all rings
pub fn process_all_rings() {
    let rings = IO_RINGS.lock();
    for ring in rings.iter() {
        let processed = ring.lock().process();
        if processed > 0 {
            crate::serial_println!("[ZC-NET] Ring {} processed {} ops", 
                ring.lock().id(), processed);
        }
    }
}

// ============================================================================
// USERSPACE INTERFACE
// ============================================================================

/// I/O ring setup structure (for userspace)
#[repr(C)]
pub struct IoUringSetup {
    /// Ring ID
    pub ring_id: u32,
    /// Submission queue entries (memory address)
    pub sq_entries: u64,
    /// Completion queue entries (memory address)
    pub cq_entries: u64,
    /// Buffer pool base address
    pub buffer_base: u64,
    /// Number of buffers
    pub buffer_count: u32,
    /// Ring size
    pub ring_size: u32,
}

/// Setup I/O ring for userspace
pub fn setup_userspace_ring() -> Option<IoUringSetup> {
    let ring_id = create_ring()?;
    let ring = get_ring(ring_id)?;
    let ring_guard = ring.lock();
    
    Some(IoUringSetup {
        ring_id,
        sq_entries: ring_guard.sq.entries.as_ptr() as u64,
        cq_entries: ring_guard.cq.entries.as_ptr() as u64,
        buffer_base: ring_guard.buffers.base_virt,
        buffer_count: ring_guard.buffers.total_chunks as u32,
        ring_size: RING_SIZE as u32,
    })
}

// ============================================================================
// INITIALIZATION
// ============================================================================

/// Initialize zero-copy networking
pub fn init() {
    crate::serial_println!("[ZC-NET] Initializing zero-copy networking...");
    
    // Create default ring
    if create_ring().is_some() {
        crate::serial_println!("[ZC-NET] Default I/O ring created");
    }
    
    crate::serial_println!("[ZC-NET] Zero-copy networking initialized");
}
