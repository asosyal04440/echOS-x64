//! # POSIX Socket API
//!
//! Linux-compatible socket interface

use super::{Ipv4Addr, Port, NetError};
use super::tcp;
use super::udp;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

// Re-export SocketAddr for other modules
pub use super::SocketAddr;

/// Socket domain
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressFamily {
    UNSPEC = 0,
    IPV4 = 2,    // AF_INET
    IPV6 = 10,   // AF_INET6
}

/// Socket type
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketType {
    STREAM = 1,  // SOCK_STREAM (TCP)
    DGRAM = 2,   // SOCK_DGRAM (UDP)
    RAW = 3,     // SOCK_RAW
}

/// Socket protocol
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    DEFAULT = 0,
    IP = 4,
    TCP = 6,
    UDP = 17,
    ICMP = 1,
}

/// Socket options
#[derive(Clone, Copy, Debug)]
pub enum SocketOption {
    ReuseAddr,
    ReusePort,
    KeepAlive,
    NoDelay,
    RcvBuf(usize),
    SndBuf(usize),
    RcvTimeout(u64),
    SndTimeout(u64),
}

/// Socket structure
pub struct Socket {
    pub id: u32,
    pub domain: AddressFamily,
    pub sock_type: SocketType,
    pub protocol: Protocol,
    pub bound: bool,
    pub listening: bool,
    pub nonblocking: bool,
}

impl Socket {
    pub fn new(domain: AddressFamily, sock_type: SocketType, protocol: Protocol) -> Result<Self, NetError> {
        let id = match sock_type {
            SocketType::STREAM => tcp::create_socket(),
            SocketType::DGRAM => udp::create_socket(),
            SocketType::RAW => return Err(NetError::NotSupported),
        };
        
        Ok(Socket {
            id,
            domain,
            sock_type,
            protocol,
            bound: false,
            listening: false,
            nonblocking: false,
        })
    }
}

// ============================================================================
// POSIX SOCKET API
// ============================================================================

/// socket() - Create socket
pub fn socket(domain: AddressFamily, sock_type: SocketType, protocol: Protocol) -> Result<u32, NetError> {
    let sock = Socket::new(domain, sock_type, protocol)?;
    Ok(sock.id)
}

/// bind() - Bind socket to address
pub fn bind(socket_id: u32, addr: SocketAddr) -> Result<(), NetError> {
    // Determine socket type from ID (hacky but works)
    if tcp::bind(socket_id, addr).is_ok() {
        return Ok(());
    }
    
    udp::bind(socket_id, addr)
}

/// listen() - Listen for connections
pub fn listen(socket_id: u32, backlog: usize) -> Result<(), NetError> {
    tcp::listen(socket_id, backlog)
}

/// accept() - Accept connection
pub fn accept(socket_id: u32) -> Result<(u32, SocketAddr), NetError> {
    tcp::accept(socket_id)
}

/// connect() - Connect to remote
pub fn connect(socket_id: u32, addr: SocketAddr) -> Result<(), NetError> {
    tcp::connect(socket_id, addr)
}

/// send() - Send data on connected socket
pub fn send(socket_id: u32, data: &[u8], flags: u32) -> Result<usize, NetError> {
    let _ = flags; // TODO: implement flags
    tcp::send(socket_id, data)
}

/// recv() - Receive data from connected socket
pub fn recv(socket_id: u32, buf: &mut [u8], flags: u32) -> Result<usize, NetError> {
    let _ = flags;
    tcp::recv(socket_id, buf)
}

/// sendto() - Send datagram to address
pub fn sendto(socket_id: u32, data: &[u8], dest: SocketAddr, flags: u32) -> Result<usize, NetError> {
    let _ = flags;
    udp::send_to(socket_id, data, dest)
}

/// recvfrom() - Receive datagram with source address
pub fn recvfrom(socket_id: u32, buf: &mut [u8], flags: u32) -> Result<(usize, SocketAddr), NetError> {
    let _ = flags;
    udp::recv_from(socket_id, buf)
}

/// close() - Close socket
pub fn close(socket_id: u32) -> Result<(), NetError> {
    tcp::close(socket_id).ok();
    udp::close(socket_id);
    Ok(())
}

/// setsockopt() - Set socket option
pub fn setsockopt(socket_id: u32, option: SocketOption) -> Result<(), NetError> {
    let _ = (socket_id, option);
    // TODO: implement socket options
    Ok(())
}

/// getsockopt() - Get socket option
pub fn getsockopt(socket_id: u32, option: SocketOption) -> Result<usize, NetError> {
    let _ = (socket_id, option);
    // TODO: implement socket options
    Ok(0)
}

/// shutdown() - Shutdown socket
pub fn shutdown(socket_id: u32, how: i32) -> Result<(), NetError> {
    let _ = how; // 0=recv, 1=send, 2=both
    tcp::close(socket_id)
}

/// getsockname() - Get socket address
pub fn getsockname(socket_id: u32) -> Result<SocketAddr, NetError> {
    let _ = socket_id;
    // TODO: implement
    Ok(SocketAddr::default())
}

/// getpeername() - Get peer address
pub fn getpeername(socket_id: u32) -> Result<SocketAddr, NetError> {
    let _ = socket_id;
    // TODO: implement
    Ok(SocketAddr::default())
}

// ============================================================================
// EVENT-DRIVEN I/O (select/poll/epoll)
// ============================================================================

/// Poll events
pub const POLLIN: u16 = 0x001;     // Readable
pub const POLLPRI: u16 = 0x002;    // Priority data
pub const POLLOUT: u16 = 0x004;    // Writable
pub const POLLERR: u16 = 0x008;    // Error
pub const POLLHUP: u16 = 0x010;    // Hung up
pub const POLLNVAL: u16 = 0x020;   // Invalid request

/// Poll file descriptor
#[derive(Clone, Copy, Debug)]
pub struct PollFd {
    pub fd: i32,
    pub events: u16,    // Input: events to watch
    pub revents: u16,   // Output: events that occurred
}

impl PollFd {
    pub fn new(fd: i32, events: u16) -> Self {
        PollFd { fd, events, revents: 0 }
    }
}

/// poll() - Wait for events on file descriptors
pub fn poll(fds: &mut [PollFd], timeout_ms: i32) -> Result<i32, NetError> {
    let mut ready_count = 0i32;
    let start_time = crate::interrupts::get_ticks();
    
    loop {
        for fd in fds.iter_mut() {
            fd.revents = 0;
            
            let socket_id = fd.fd as u32;
            
            // Check for readability
            if fd.events & POLLIN != 0 {
                if can_read(socket_id) {
                    fd.revents |= POLLIN;
                }
            }
            
            // Check for writability
            if fd.events & POLLOUT != 0 {
                if can_write(socket_id) {
                    fd.revents |= POLLOUT;
                }
            }
            
            // Check for errors/hangup
            if is_hungup(socket_id) {
                fd.revents |= POLLHUP;
            }
            
            if has_error(socket_id) {
                fd.revents |= POLLERR;
            }
            
            if fd.revents != 0 {
                ready_count += 1;
            }
        }
        
        if ready_count > 0 {
            return Ok(ready_count);
        }
        
        // Check timeout
        if timeout_ms >= 0 {
            let elapsed = crate::interrupts::get_ticks() - start_time;
            if elapsed >= timeout_ms as u64 {
                return Ok(0); // Timeout
            }
        }
        
        // Yield CPU
            crate::task::scheduler::schedule();
    }
}

/// select() - Monitor file descriptors for readiness
pub fn select(
    nfds: i32,
    readfds: &mut [u8],
    writefds: &mut [u8],
    exceptfds: &mut [u8],
    timeout_ms: Option<i32>,
) -> Result<i32, NetError> {
    let mut ready_count = 0i32;
    let start_time = crate::interrupts::get_ticks();
    
    loop {
        // Check readfds
        for fd in 0..nfds {
            let byte_idx = (fd / 8) as usize;
            let bit_idx = (fd % 8) as usize;
            
            if byte_idx < readfds.len() {
                if readfds[byte_idx] & (1 << bit_idx) != 0 {
                    if can_read(fd as u32) {
                        // Already set
                    } else {
                        readfds[byte_idx] &= !(1 << bit_idx);
                    }
                }
            }
            
            if byte_idx < writefds.len() {
                if writefds[byte_idx] & (1 << bit_idx) != 0 {
                    if can_write(fd as u32) {
                        // Already set
                    } else {
                        writefds[byte_idx] &= !(1 << bit_idx);
                    }
                }
            }
            
            if byte_idx < exceptfds.len() {
                if exceptfds[byte_idx] & (1 << bit_idx) != 0 {
                    if has_error(fd as u32) {
                        // Already set
                    } else {
                        exceptfds[byte_idx] &= !(1 << bit_idx);
                    }
                }
            }
        }
        
        // Count ready FDs
        ready_count = 0;
        for fd in 0..nfds {
            let byte_idx = (fd / 8) as usize;
            let bit_idx = (fd % 8) as usize;
            
            if byte_idx < readfds.len() && readfds[byte_idx] & (1 << bit_idx) != 0 {
                ready_count += 1;
            }
            if byte_idx < writefds.len() && writefds[byte_idx] & (1 << bit_idx) != 0 {
                ready_count += 1;
            }
            if byte_idx < exceptfds.len() && exceptfds[byte_idx] & (1 << bit_idx) != 0 {
                ready_count += 1;
            }
        }
        
        if ready_count > 0 {
            return Ok(ready_count);
        }
        
        // Check timeout
        if let Some(timeout) = timeout_ms {
            if timeout >= 0 {
                let elapsed = crate::interrupts::get_ticks() - start_time;
                if elapsed >= timeout as u64 {
                    return Ok(0);
                }
            }
        }
        
        crate::task::scheduler::schedule();
    }
}

// ============================================================================
// EPOLL (Event Poll)
// ============================================================================

/// Epoll operations
pub const EPOLL_CTL_ADD: i32 = 1;
pub const EPOLL_CTL_DEL: i32 = 2;
pub const EPOLL_CTL_MOD: i32 = 3;

/// Epoll events
pub const EPOLLIN: u32 = 0x001;
pub const EPOLLOUT: u32 = 0x004;
pub const EPOLLERR: u32 = 0x008;
pub const EPOLLHUP: u32 = 0x010;
pub const EPOLLET: u32 = 0x80000000;  // Edge-triggered

/// Epoll event structure
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct EpollEvent {
    pub events: u32,
    pub data: u64,  // User data
}

/// Epoll instance
pub struct EpollInstance {
    pub id: u32,
    pub events: alloc::collections::BTreeMap<i32, EpollEvent>,
}

static EPOLL_INSTANCES: spin::Mutex<alloc::collections::BTreeMap<u32, EpollInstance>> = 
    spin::Mutex::new(alloc::collections::BTreeMap::new());
static EPOLL_NEXT_ID: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(1);

/// epoll_create() - Create epoll instance
pub fn epoll_create(size: i32) -> Result<i32, NetError> {
    let _ = size; // Ignored since Linux 2.6.8
    let id = EPOLL_NEXT_ID.fetch_add(1, Ordering::SeqCst);
    
    let instance = EpollInstance {
        id,
        events: alloc::collections::BTreeMap::new(),
    };
    
    EPOLL_INSTANCES.lock().insert(id, instance);
    Ok(id as i32)
}

/// epoll_ctl() - Control epoll instance
pub fn epoll_ctl(epfd: i32, op: i32, fd: i32, event: &EpollEvent) -> Result<(), NetError> {
    let mut instances = EPOLL_INSTANCES.lock();
    let instance = instances.get_mut(&(epfd as u32))
        .ok_or(NetError::InvalidFd)?;
    
    match op {
        EPOLL_CTL_ADD => {
            instance.events.insert(fd, *event);
        }
        EPOLL_CTL_DEL => {
            instance.events.remove(&fd);
        }
        EPOLL_CTL_MOD => {
            instance.events.insert(fd, *event);
        }
        _ => return Err(NetError::InvalidParam),
    }
    
    Ok(())
}

/// epoll_wait() - Wait for events
pub fn epoll_wait(epfd: i32, events: &mut [EpollEvent], max_events: i32, timeout_ms: i32) -> Result<i32, NetError> {
    if events.len() < max_events as usize {
        return Err(NetError::BufferFull);
    }
    
    let start_time = crate::interrupts::get_ticks();
    let mut ready_count = 0i32;
    
    loop {
        let instances = EPOLL_INSTANCES.lock();
        let instance = instances.get(&(epfd as u32))
            .ok_or(NetError::InvalidFd)?;
        
        for (&fd, &event) in &instance.events {
            if ready_count >= max_events {
                break;
            }
            
            let mut revents = 0u32;
            
            if event.events & EPOLLIN != 0 && can_read(fd as u32) {
                revents |= EPOLLIN;
            }
            if event.events & EPOLLOUT != 0 && can_write(fd as u32) {
                revents |= EPOLLOUT;
            }
            if has_error(fd as u32) {
                revents |= EPOLLERR;
            }
            if is_hungup(fd as u32) {
                revents |= EPOLLHUP;
            }
            
            if revents != 0 {
                events[ready_count as usize] = EpollEvent {
                    events: revents,
                    data: event.data,
                };
                ready_count += 1;
            }
        }
        
        drop(instances);
        
        if ready_count > 0 {
            return Ok(ready_count);
        }
        
        // Check timeout
        if timeout_ms >= 0 {
            let elapsed = crate::interrupts::get_ticks() - start_time;
            if elapsed >= timeout_ms as u64 {
                return Ok(0);
            }
        }
        
        crate::task::scheduler::schedule();
    }
}

/// epoll_close() - Close epoll instance
pub fn epoll_close(epfd: i32) -> Result<(), NetError> {
    EPOLL_INSTANCES.lock().remove(&(epfd as u32));
    Ok(())
}
// HELPER FUNCTIONS FOR EVENT CHECKING
// ============================================================================

/// Check if socket can be read
pub fn can_read(socket_id: u32) -> bool {
    // Try TCP first, then UDP
    if let Some(conn) = tcp::get_connection(socket_id) {
        return !conn.rx_buffer.is_empty() || 
               conn.state == tcp::TcpState::CloseWait ||
               conn.state == tcp::TcpState::Closed;
    }
    
    if let Some(sock) = udp::get_socket(socket_id) {
        return !sock.rx_buffer.is_empty();
    }
    
    false
}

/// Check if socket can be written
pub fn can_write(socket_id: u32) -> bool {
    if let Some(conn) = tcp::get_connection(socket_id) {
        return conn.state == tcp::TcpState::Established;
    }
    
    // UDP is always writable
    if udp::get_socket(socket_id).is_some() {
        return true;
    }
    
    false
}

/// Check if socket has error
fn has_error(socket_id: u32) -> bool {
    if let Some(conn) = tcp::get_connection(socket_id) {
        return conn.state == tcp::TcpState::Closed;
    }
    false
}

/// Check if socket is hung up
fn is_hungup(socket_id: u32) -> bool {
    if let Some(conn) = tcp::get_connection(socket_id) {
        return conn.state == tcp::TcpState::CloseWait ||
               conn.state == tcp::TcpState::TimeWait;
    }
    false
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Parse IP address from string (e.g., "192.168.1.1")
pub fn parse_ipv4(s: &str) -> Option<Ipv4Addr> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    
    let mut bytes = [0u8; 4];
    for (i, part) in parts.iter().enumerate() {
        bytes[i] = part.parse().ok()?;
    }
    
    Some(Ipv4Addr::from_bytes(bytes))
}

/// Format IP address to string
pub fn format_ipv4(ip: Ipv4Addr) -> alloc::string::String {
    alloc::format!("{}.{}.{}.{}", ip.0[0], ip.0[1], ip.0[2], ip.0[3])
}

/// Parse port from string
pub fn parse_port(s: &str) -> Option<Port> {
    let port: u16 = s.parse().ok()?;
    Some(Port(port))
}
