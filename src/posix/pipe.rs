//! # Pipe and FIFO
//!
//! Anonymous pipes and named pipes (FIFOs).

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// PIPE CONSTANTS
// ============================================================================

/// Default pipe buffer size
pub const PIPE_BUF_SIZE: usize = 65536; // 64KB

/// Pipe flags
pub const O_RDONLY: u32 = 0;
pub const O_WRONLY: u32 = 1;
pub const O_RDWR: u32 = 2;
pub const O_NONBLOCK: u32 = 0x800;

/// Pipe buffer limits
pub const PIPE_MIN_BUF_SIZE: usize = 4096;
pub const PIPE_MAX_BUF_SIZE: usize = 1048576; // 1MB

// ============================================================================
// PIPE BUFFER
// ============================================================================

pub struct PipeBuffer {
    /// Data buffer
    buffer: Mutex<VecDeque<u8>>,
    /// Maximum size
    max_size: usize,
    /// Readers count
    readers: AtomicU32,
    /// Writers count
    writers: AtomicU32,
    /// Is non-blocking
    nonblocking: AtomicBool,
    /// Total bytes written
    bytes_written: AtomicU64,
    /// Total bytes read
    bytes_read: AtomicU64,
    /// Waiting readers
    waiting_readers: AtomicU32,
    /// Waiting writers
    waiting_writers: AtomicU32,
}

impl PipeBuffer {
    pub fn new(size: usize) -> Self {
        Self {
            buffer: Mutex::new(VecDeque::with_capacity(size)),
            max_size: size.max(PIPE_MIN_BUF_SIZE).min(PIPE_MAX_BUF_SIZE),
            readers: AtomicU32::new(0),
            writers: AtomicU32::new(0),
            nonblocking: AtomicBool::new(false),
            bytes_written: AtomicU64::new(0),
            bytes_read: AtomicU64::new(0),
            waiting_readers: AtomicU32::new(0),
            waiting_writers: AtomicU32::new(0),
        }
    }

    /// Add reader
    pub fn add_reader(&self) {
        self.readers.fetch_add(1, Ordering::SeqCst);
    }

    /// Remove reader
    pub fn remove_reader(&self) {
        self.readers.fetch_sub(1, Ordering::SeqCst);
    }

    /// Add writer
    pub fn add_writer(&self) {
        self.writers.fetch_add(1, Ordering::SeqCst);
    }

    /// Remove writer
    pub fn remove_writer(&self) {
        self.writers.fetch_sub(1, Ordering::SeqCst);
    }

    /// Get readers count
    pub fn get_readers(&self) -> u32 {
        self.readers.load(Ordering::SeqCst)
    }

    /// Get writers count
    pub fn get_writers(&self) -> u32 {
        self.writers.load(Ordering::SeqCst)
    }

    /// Read from pipe
    pub fn read(&self, buf: &mut [u8]) -> Result<usize, PipeError> {
        if self.readers.load(Ordering::SeqCst) == 0 {
            return Err(PipeError::NoReader);
        }

        let mut buffer = self.buffer.lock();

        if buffer.is_empty() {
            if self.writers.load(Ordering::SeqCst) == 0 {
                return Ok(0); // EOF
            }

            if self.nonblocking.load(Ordering::SeqCst) {
                return Err(PipeError::WouldBlock);
            }

            // Would block - wait for data
            return Err(PipeError::WouldBlock);
        }

        let to_read = buf.len().min(buffer.len());
        for i in 0..to_read {
            buf[i] = buffer.pop_front().unwrap();
        }

        self.bytes_read.fetch_add(to_read as u64, Ordering::SeqCst);

        Ok(to_read)
    }

    /// Write to pipe
    pub fn write(&self, buf: &[u8]) -> Result<usize, PipeError> {
        if self.writers.load(Ordering::SeqCst) == 0 {
            return Err(PipeError::NoWriter);
        }

        if self.readers.load(Ordering::SeqCst) == 0 {
            return Err(PipeError::BrokenPipe);
        }

        let mut buffer = self.buffer.lock();

        let available = self.max_size.saturating_sub(buffer.len());
        
        if available == 0 {
            if self.nonblocking.load(Ordering::SeqCst) {
                return Err(PipeError::WouldBlock);
            }
            // Would block - wait for space
            return Err(PipeError::WouldBlock);
        }

        let to_write = buf.len().min(available);
        for i in 0..to_write {
            buffer.push_back(buf[i]);
        }

        self.bytes_written.fetch_add(to_write as u64, Ordering::SeqCst);

        Ok(to_write)
    }

    /// Get available space
    pub fn space(&self) -> usize {
        let buffer = self.buffer.lock();
        self.max_size.saturating_sub(buffer.len())
    }

    /// Get buffered data size
    pub fn len(&self) -> usize {
        self.buffer.lock().len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.buffer.lock().is_empty()
    }

    /// Set non-blocking mode
    pub fn set_nonblocking(&self, nonblock: bool) {
        self.nonblocking.store(nonblock, Ordering::SeqCst);
    }

    /// Poll for events
    pub fn poll(&self, events: u32) -> u32 {
        let mut revents = 0u32;

        // POLLIN
        if events & 0x001 != 0 && !self.is_empty() {
            revents |= 0x001;
        }

        // POLLOUT
        if events & 0x004 != 0 && self.space() > 0 {
            revents |= 0x004;
        }

        // POLLHUP
        if self.writers.load(Ordering::SeqCst) == 0 {
            revents |= 0x010;
        }

        // POLLERR
        if self.readers.load(Ordering::SeqCst) == 0 {
            revents |= 0x008;
        }

        revents
    }
}

// ============================================================================
// PIPE
// ============================================================================

pub struct Pipe {
    /// Pipe buffer
    buffer: Arc<PipeBuffer>,
    /// Read fd
    pub read_fd: i32,
    /// Write fd
    pub write_fd: i32,
}

impl Pipe {
    pub fn new(size: usize) -> Self {
        let buffer = Arc::new(PipeBuffer::new(size));
        buffer.add_reader();
        buffer.add_writer();

        Self {
            buffer,
            read_fd: -1,
            write_fd: -1,
        }
    }

    /// Get buffer
    pub fn get_buffer(&self) -> Arc<PipeBuffer> {
        self.buffer.clone()
    }

    /// Read
    pub fn read(&self, buf: &mut [u8]) -> Result<usize, PipeError> {
        self.buffer.read(buf)
    }

    /// Write
    pub fn write(&self, buf: &[u8]) -> Result<usize, PipeError> {
        self.buffer.write(buf)
    }

    /// Close read end
    pub fn close_read(&self) {
        self.buffer.remove_reader();
    }

    /// Close write end
    pub fn close_write(&self) {
        self.buffer.remove_writer();
    }
}

// ============================================================================
// FIFO (NAMED PIPE)
// ============================================================================

pub struct Fifo {
    /// Path
    pub path: String,
    /// Mode
    pub mode: u32,
    /// Pipe buffer
    buffer: Arc<PipeBuffer>,
    /// Is open for reading
    open_read: AtomicBool,
    /// Is open for writing
    open_write: AtomicBool,
}

impl Fifo {
    pub fn new(path: &str, mode: u32) -> Self {
        let buffer = Arc::new(PipeBuffer::new(PIPE_BUF_SIZE));

        Self {
            path: String::from(path),
            mode,
            buffer,
            open_read: AtomicBool::new(false),
            open_write: AtomicBool::new(false),
        }
    }

    /// Open for reading
    pub fn open_read(&self) -> Result<(), PipeError> {
        self.buffer.add_reader();
        self.open_read.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Open for writing
    pub fn open_write(&self) -> Result<(), PipeError> {
        self.buffer.add_writer();
        self.open_write.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Close read end
    pub fn close_read(&self) {
        if self.open_read.swap(false, Ordering::SeqCst) {
            self.buffer.remove_reader();
        }
    }

    /// Close write end
    pub fn close_write(&self) {
        if self.open_write.swap(false, Ordering::SeqCst) {
            self.buffer.remove_writer();
        }
    }

    /// Read
    pub fn read(&self, buf: &mut [u8]) -> Result<usize, PipeError> {
        self.buffer.read(buf)
    }

    /// Write
    pub fn write(&self, buf: &[u8]) -> Result<usize, PipeError> {
        self.buffer.write(buf)
    }

    /// Get buffer
    pub fn get_buffer(&self) -> Arc<PipeBuffer> {
        self.buffer.clone()
    }
}

// ============================================================================
// PIPE MANAGER
// ============================================================================

pub struct PipeManager {
    /// Named pipes (FIFOs)
    fifos: Mutex<BTreeMap<String, Arc<Fifo>>>,
    /// Anonymous pipes
    pipes: Mutex<BTreeMap<u64, Arc<Pipe>>>,
    /// Next pipe ID
    next_pipe_id: AtomicU64,
    /// Statistics
    stats: Mutex<PipeStats>,
}

use alloc::collections::BTreeMap;

#[derive(Clone, Debug, Default)]
pub struct PipeStats {
    pub pipes_created: u64,
    pub fifos_created: u64,
    pub bytes_read: u64,
    pub bytes_written: u64,
}

impl PipeManager {
    pub const fn new() -> Self {
        Self {
            fifos: Mutex::new(BTreeMap::new()),
            pipes: Mutex::new(BTreeMap::new()),
            next_pipe_id: AtomicU64::new(1),
            stats: Mutex::new(PipeStats::default()),
        }
    }

    /// Create anonymous pipe
    pub fn create_pipe(&self, size: usize) -> Arc<Pipe> {
        let id = self.next_pipe_id.fetch_add(1, Ordering::SeqCst);
        let pipe = Arc::new(Pipe::new(size));

        self.pipes.lock().insert(id, pipe.clone());

        let mut stats = self.stats.lock();
        stats.pipes_created += 1;

        pipe
    }

    /// Create FIFO
    pub fn create_fifo(&self, path: &str, mode: u32) -> Result<Arc<Fifo>, PipeError> {
        let mut fifos = self.fifos.lock();

        if fifos.contains_key(path) {
            return Err(PipeError::AlreadyExists);
        }

        let fifo = Arc::new(Fifo::new(path, mode));
        fifos.insert(String::from(path), fifo.clone());

        let mut stats = self.stats.lock();
        stats.fifos_created += 1;

        Ok(fifo)
    }

    /// Get FIFO by path
    pub fn get_fifo(&self, path: &str) -> Option<Arc<Fifo>> {
        self.fifos.lock().get(path).cloned()
    }

    /// Remove FIFO
    pub fn remove_fifo(&self, path: &str) {
        self.fifos.lock().remove(path);
    }

    /// Get statistics
    pub fn get_stats(&self) -> PipeStats {
        self.stats.lock().clone()
    }
}

lazy_static::lazy_static! {
    pub static ref PIPE_MANAGER: PipeManager = PipeManager::new();
}

// ============================================================================
// ERROR TYPE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipeError {
    WouldBlock,
    BrokenPipe,
    NoReader,
    NoWriter,
    AlreadyExists,
    NotFound,
}

// ============================================================================
// SYSCALL INTERFACE
// ============================================================================

/// pipe(int pipefd[2])
pub fn sys_pipe(fds: &mut [i32; 2]) -> i32 {
    let pipe = PIPE_MANAGER.create_pipe(PIPE_BUF_SIZE);

    // Allocate file descriptors
    fds[0] = pipe.read_fd;
    fds[1] = pipe.write_fd;

    0
}

/// mkfifo(const char *pathname, mode_t mode)
pub fn sys_mkfifo(path: &str, mode: u32) -> i32 {
    match PIPE_MANAGER.create_fifo(path, mode) {
        Ok(_) => 0,
        Err(PipeError::AlreadyExists) => -17, // EEXIST
        Err(_) => -5,
    }
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init() {
    crate::serial_println!("[PIPE] Pipe/FIFO initialized");
}
