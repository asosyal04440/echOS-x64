//! # sendfile and splice
//!
//! Zero-copy data transfer between file descriptors.

use core::sync::atomic::{AtomicU64, Ordering};

// ============================================================================
// SENDFILE
// ============================================================================

/// sendfile syscall - transfer data between file descriptors
/// 
/// # Arguments
/// - `out_fd`: Output file descriptor
/// - `in_fd`: Input file descriptor  
/// - `offset`: Offset to read from (optional)
/// - `count`: Number of bytes to transfer
/// 
/// # Returns
/// Number of bytes transferred, or negative errno
pub fn sys_sendfile(out_fd: i32, in_fd: i32, offset: Option<&mut u64>, count: usize) -> i64 {
    // Validate file descriptors
    if out_fd < 0 || in_fd < 0 {
        return -9; // EBADF
    }
    
    if count == 0 {
        return 0;
    }
    
    // Get file types
    // in_fd must be a regular file with mmap support
    // out_fd must be a socket or pipe
    
    let mut bytes_transferred: u64 = 0;
    let mut read_offset = offset.map(|o| *o).unwrap_or(0);
    
    // Transfer in chunks
    let chunk_size = 65536; // 64KB chunks
    let mut remaining = count;
    
    while remaining > 0 {
        let to_transfer = core::cmp::min(remaining, chunk_size);
        
        // Read from input
        // In real implementation, would use page cache directly
        let read_bytes = read_from_fd(in_fd, read_offset, to_transfer as u32);
        
        if read_bytes <= 0 {
            break;
        }
        
        // Write to output
        let written = write_to_fd(out_fd, read_offset, read_bytes as u32);
        
        if written <= 0 {
            break;
        }
        
        bytes_transferred += written as u64;
        read_offset += written as u64;
        remaining -= written as usize;
        
        if written < read_bytes {
            // Short write
            break;
        }
    }
    
    // Update offset if provided
    if let Some(o) = offset {
        *o = read_offset;
    }
    
    SENDFILE_STATS.fetch_add(bytes_transferred, Ordering::Relaxed);
    
    bytes_transferred as i64
}

/// Read from file descriptor (placeholder)
fn read_from_fd(fd: i32, offset: u64, count: u32) -> i32 {
    // Would call into VFS
    count as i32
}

/// Write to file descriptor (placeholder)
fn write_to_fd(fd: i32, offset: u64, count: u32) -> i32 {
    // Would call into VFS
    count as i32
}

lazy_static::lazy_static! {
    static ref SENDFILE_STATS: AtomicU64 = AtomicU64::new(0);
}

// ============================================================================
// SPLICE
// ============================================================================

/// splice flags
pub const SPLICE_F_MOVE: u32 = 1;
pub const SPLICE_F_NONBLOCK: u32 = 2;
pub const SPLICE_F_MORE: u32 = 4;
pub const SPLICE_F_GIFT: u32 = 8;

/// Pipe buffer size
pub const PIPE_DEF_BUFSIZE: usize = 65536;

/// splice syscall - move data between pipe and file
/// 
/// # Arguments
/// - `fd_in`: Input file descriptor
/// - `off_in`: Input offset (optional)
/// - `fd_out`: Output file descriptor
/// - `off_out`: Output offset (optional)
/// - `len`: Number of bytes
/// - `flags`: Splice flags
pub fn sys_splice(
    fd_in: i32,
    off_in: Option<&mut u64>,
    fd_out: i32,
    off_out: Option<&mut u64>,
    len: usize,
    flags: u32,
) -> i64 {
    if fd_in < 0 || fd_out < 0 {
        return -9; // EBADF
    }
    
    // One of the fds must be a pipe
    // For now, assume both are valid
    
    let mut bytes_spliced: u64 = 0;
    let mut in_offset = off_in.map(|o| *o).unwrap_or(0);
    let mut out_offset = off_out.map(|o| *o).unwrap_or(0);
    
    // Perform splice
    // In real implementation, would use pipe buffers for zero-copy
    
    let chunk_size = 65536;
    let mut remaining = len;
    
    while remaining > 0 {
        let to_splice = core::cmp::min(remaining, chunk_size);
        
        // Read into pipe buffer
        let read = read_from_fd(fd_in, in_offset, to_splice as u32);
        if read <= 0 {
            break;
        }
        
        // Write from pipe buffer
        let written = write_to_fd(fd_out, out_offset, read as u32);
        if written <= 0 {
            break;
        }
        
        bytes_spliced += written as u64;
        in_offset += written as u64;
        out_offset += written as u64;
        remaining -= written as usize;
        
        if written < read {
            break;
        }
    }
    
    // Update offsets
    if let Some(o) = off_in {
        *o = in_offset;
    }
    if let Some(o) = off_out {
        *o = out_offset;
    }
    
    SPLICE_STATS.fetch_add(bytes_spliced, Ordering::Relaxed);
    
    bytes_spliced as i64
}

lazy_static::lazy_static! {
    static ref SPLICE_STATS: AtomicU64 = AtomicU64::new(0);
}

// ============================================================================
// TEE
// ============================================================================

/// tee syscall - duplicate pipe data
pub fn sys_tee(fd_in: i32, fd_out: i32, len: usize, flags: u32) -> i64 {
    if fd_in < 0 || fd_out < 0 {
        return -9;
    }
    
    // Both fds must be pipes
    // Duplicate data from one pipe to another
    
    len as i64
}

// ============================================================================
// VMSPLICE
// ============================================================================

/// vmsplice syscall - splice user memory to/from pipe
pub fn sys_vmsplice(fd: i32, iovs: &[IoVec], flags: u32) -> i64 {
    if fd < 0 {
        return -9;
    }
    
    let mut total = 0i64;
    for iov in iovs {
        total += iov.len as i64;
    }
    
    total
}

/// I/O vector
#[repr(C)]
pub struct IoVec {
    pub base: u64,
    pub len: u64,
}

// ============================================================================
// COPY_FILE_RANGE
// ============================================================================

/// copy_file_range syscall - copy data between files
pub fn sys_copy_file_range(
    fd_in: i32,
    off_in: Option<&mut u64>,
    fd_out: i32,
    off_out: Option<&mut u64>,
    len: usize,
    flags: u32,
) -> i64 {
    if fd_in < 0 || fd_out < 0 {
        return -9;
    }
    
    // Similar to sendfile but works with any file types
    // Can use reflink for efficient copying on supported filesystems
    
    sys_sendfile(fd_out, fd_in, off_in, len)
}

// ============================================================================
// STATISTICS
// ============================================================================

pub struct ZeroCopyStats {
    pub sendfile_bytes: u64,
    pub splice_bytes: u64,
}

pub fn get_stats() -> ZeroCopyStats {
    ZeroCopyStats {
        sendfile_bytes: SENDFILE_STATS.load(Ordering::Relaxed),
        splice_bytes: SPLICE_STATS.load(Ordering::Relaxed),
    }
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init() {
    crate::serial_println!("[ZEROCOPY] sendfile/splice initialized");
}
