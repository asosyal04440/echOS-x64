//! # Netlink Socket
//!
//! Netlink protocol for kernel-userspace communication.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// NETLINK CONSTANTS
// ============================================================================

/// Netlink protocols
pub const NETLINK_ROUTE: u32 = 0;       // Routing/neigh
pub const NETLINK_UNUSED: u32 = 1;      // Unused
pub const NETLINK_USERSOCK: u32 = 2;    // User mode socket
pub const NETLINK_FIREWALL: u32 = 3;    // Firewall
pub const NETLINK_SOCK_DIAG: u32 = 4;   // Socket monitoring
pub const NETLINK_NFLOG: u32 = 5;       // Netfilter logging
pub const NETLINK_XFRM: u32 = 6;        // IPsec
pub const NETLINK_SELINUX: u32 = 7;     // SELinux
pub const NETLINK_ISCSI: u32 = 8;       // iSCSI
pub const NETLINK_AUDIT: u32 = 9;       // Audit
pub const NETLINK_FIB_LOOKUP: u32 = 10; // FIB lookup
pub const NETLINK_CONNECTOR: u32 = 11;  // Connector
pub const NETLINK_NETFILTER: u32 = 12;  // Netfilter
pub const NETLINK_IP6_FW: u32 = 13;     // IPv6 firewall
pub const NETLINK_DNRTMSG: u32 = 14;    // DECnet routing
pub const NETLINK_KOBJECT_UEVENT: u32 = 15; // Kernel events
pub const NETLINK_GENERIC: u32 = 16;   // Generic netlink
pub const NETLINK_SCSITRANSPORT: u32 = 18; // SCSI transport
pub const NETLINK_ECRYPTFS: u32 = 19;  // eCryptfs
pub const NETLINK_RDMA: u32 = 20;       // RDMA
pub const NETLINK_CRYPTO: u32 = 21;     // Crypto

/// Netlink message flags
pub const NLM_F_REQUEST: u16 = 1;
pub const NLM_F_MULTI: u16 = 2;
pub const NLM_F_ACK: u16 = 4;
pub const NLM_F_ECHO: u16 = 8;
pub const NLM_F_DUMP_INTR: u16 = 16;
pub const NLM_F_DUMP_FILTERED: u16 = 32;

/// GET flags
pub const NLM_F_ROOT: u16 = 0x100;
pub const NLM_F_MATCH: u16 = 0x200;
pub const NLM_F_ATOMIC: u16 = 0x400;
pub const NLM_F_DUMP: u16 = NLM_F_ROOT | NLM_F_MATCH;

/// NEW flags
pub const NLM_F_REPLACE: u16 = 0x100;
pub const NLM_F_EXCL: u16 = 0x200;
pub const NLM_F_CREATE: u16 = 0x400;
pub const NLM_F_APPEND: u16 = 0x800;

/// Netlink error codes
pub const NLE_SUCCESS: i32 = 0;
pub const NLE_ERROR: i32 = -1;
pub const NLE_NOACCESS: i32 = -13;

// ============================================================================
// NETLINK MESSAGE HEADER
// ============================================================================

#[repr(C)]
pub struct NlMsgHdr {
    pub nlmsg_len: u32,
    pub nlmsg_type: u16,
    pub nlmsg_flags: u16,
    pub nlmsg_seq: u32,
    pub nlmsg_pid: u32,
}

impl NlMsgHdr {
    pub fn new(len: u32, msg_type: u16, flags: u16, seq: u32, pid: u32) -> Self {
        Self {
            nlmsg_len: len,
            nlmsg_type: msg_type,
            nlmsg_flags: flags,
            nlmsg_seq: seq,
            nlmsg_pid: pid,
        }
    }

    pub fn size() -> usize {
        core::mem::size_of::<Self>()
    }
}

// ============================================================================
// NETLINK ATTRIBUTES
// ============================================================================

#[repr(C)]
pub struct NlAttr {
    pub nla_len: u16,
    pub nla_type: u16,
}

impl NlAttr {
    pub fn new(attr_type: u16, data: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        let len = (4 + data.len()) as u16;
        buf.extend_from_slice(&len.to_le_bytes());
        buf.extend_from_slice(&attr_type.to_le_bytes());
        buf.extend_from_slice(data);
        // Pad to 4 bytes
        while buf.len() % 4 != 0 {
            buf.push(0);
        }
        buf
    }
}

// ============================================================================
// NETLINK MESSAGE TYPES
// ============================================================================

/// RTM message types
pub const RTM_NEWLINK: u16 = 16;
pub const RTM_DELLINK: u16 = 17;
pub const RTM_GETLINK: u16 = 18;
pub const RTM_SETLINK: u16 = 19;
pub const RTM_NEWADDR: u16 = 20;
pub const RTM_DELADDR: u16 = 21;
pub const RTM_GETADDR: u16 = 22;
pub const RTM_NEWROUTE: u16 = 24;
pub const RTM_DELROUTE: u16 = 25;
pub const RTM_GETROUTE: u16 = 26;
pub const RTM_NEWNEIGH: u16 = 28;
pub const RTM_DELNEIGH: u16 = 29;
pub const RTM_GETNEIGH: u16 = 30;
pub const RTM_NEWRULE: u16 = 32;
pub const RTM_DELRULE: u16 = 33;
pub const RTM_GETRULE: u16 = 34;

/// Generic netlink commands
pub const GENL_ID_GENERATE: u16 = 0;
pub const GENL_ID_CTRL: u16 = 0x10;

// ============================================================================
// NETLINK SOCKET
// ============================================================================

pub struct NetlinkSocket {
    /// Socket ID
    pub id: u64,
    /// Protocol
    pub protocol: u32,
    /// Port ID (PID)
    pub port_id: AtomicU32,
    /// Destination port ID
    pub dst_port_id: AtomicU32,
    /// Destination group
    pub dst_group: AtomicU32,
    /// Receive buffer
    pub rx_buf: Mutex<Vec<NetlinkMessage>>,
    /// Send buffer
    pub tx_buf: Mutex<Vec<NetlinkMessage>>,
    /// Subscribed groups
    pub groups: Mutex<BTreeMap<u32, bool>>,
    /// Non-blocking
    pub nonblocking: AtomicBool,
    /// Sequence number
    pub seq: AtomicU32,
}

#[derive(Clone, Debug)]
pub struct NetlinkMessage {
    pub header: NlMsgHdr,
    pub payload: Vec<u8>,
}

impl NetlinkSocket {
    pub fn new(id: u64, protocol: u32, port_id: u32) -> Self {
        Self {
            id,
            protocol,
            port_id: AtomicU32::new(port_id),
            dst_port_id: AtomicU32::new(0),
            dst_group: AtomicU32::new(0),
            rx_buf: Mutex::new(Vec::new()),
            tx_buf: Mutex::new(Vec::new()),
            groups: Mutex::new(BTreeMap::new()),
            nonblocking: AtomicBool::new(false),
            seq: AtomicU32::new(1),
        }
    }

    /// Send message
    pub fn send(&self, msg: NetlinkMessage) -> Result<(), NetlinkError> {
        let pid = self.dst_port_id.load(Ordering::Relaxed);
        let group = self.dst_group.load(Ordering::Relaxed);
        
        // Route to destination
        if pid == 0 && group == 0 {
            // Kernel message
            self.handle_kernel_message(&msg)?;
        } else {
            // Userspace message
            if let Some(sock) = NETLINK_SOCKS.lock().get(&pid) {
                sock.rx_buf.lock().push(msg);
            }
        }
        
        Ok(())
    }

    /// Receive message
    pub fn recv(&self) -> Option<NetlinkMessage> {
        self.rx_buf.lock().pop()
    }

    /// Handle kernel-bound message
    fn handle_kernel_message(&self, msg: &NetlinkMessage) -> Result<(), NetlinkError> {
        match self.protocol {
            NETLINK_ROUTE => self.handle_route(msg),
            NETLINK_NETFILTER => self.handle_netfilter(msg),
            NETLINK_XFRM => self.handle_xfrm(msg),
            NETLINK_AUDIT => self.handle_audit(msg),
            NETLINK_KOBJECT_UEVENT => self.handle_uevent(msg),
            NETLINK_GENERIC => self.handle_generic(msg),
            _ => Ok(()),
        }
    }

    fn handle_route(&self, msg: &NetlinkMessage) -> Result<(), NetlinkError> {
        match msg.header.nlmsg_type {
            RTM_GETLINK | RTM_GETADDR | RTM_GETROUTE => {
                // Dump network configuration
                let reply = self.build_route_reply(msg);
                self.rx_buf.lock().push(reply);
            }
            RTM_NEWLINK | RTM_NEWADDR | RTM_NEWROUTE => {
                // Configure network
            }
            _ => {}
        }
        Ok(())
    }

    fn build_route_reply(&self, _msg: &NetlinkMessage) -> NetlinkMessage {
        NetlinkMessage {
            header: NlMsgHdr::new(0, RTM_NEWLINK, NLM_F_MULTI, 0, 0),
            payload: Vec::new(),
        }
    }

    fn handle_netfilter(&self, _msg: &NetlinkMessage) -> Result<(), NetlinkError> {
        Ok(())
    }

    fn handle_xfrm(&self, _msg: &NetlinkMessage) -> Result<(), NetlinkError> {
        Ok(())
    }

    fn handle_audit(&self, _msg: &NetlinkMessage) -> Result<(), NetlinkError> {
        Ok(())
    }

    fn handle_uevent(&self, _msg: &NetlinkMessage) -> Result<(), NetlinkError> {
        Ok(())
    }

    fn handle_generic(&self, _msg: &NetlinkMessage) -> Result<(), NetlinkError> {
        Ok(())
    }

    /// Join multicast group
    pub fn join_group(&self, group: u32) {
        self.groups.lock().insert(group, true);
    }

    /// Leave multicast group
    pub fn leave_group(&self, group: u32) {
        self.groups.lock().remove(&group);
    }

    /// Set destination
    pub fn set_destination(&self, pid: u32, group: u32) {
        self.dst_port_id.store(pid, Ordering::SeqCst);
        self.dst_group.store(group, Ordering::SeqCst);
    }
}

// ============================================================================
// NETLINK MANAGER
// ============================================================================

pub struct NetlinkManager {
    sockets: Mutex<BTreeMap<u32, Arc<NetlinkSocket>>>,
    next_port_id: AtomicU32,
    next_socket_id: AtomicU64,
    /// Multicast groups
    groups: Mutex<BTreeMap<u32, Vec<u32>>>, // group -> [port_ids]
}

impl NetlinkManager {
    pub const fn new() -> Self {
        Self {
            sockets: Mutex::new(BTreeMap::new()),
            next_port_id: AtomicU32::new(1),
            next_socket_id: AtomicU64::new(1),
            groups: Mutex::new(BTreeMap::new()),
        }
    }

    /// Create socket
    pub fn create_socket(&self, protocol: u32) -> Arc<NetlinkSocket> {
        let id = self.next_socket_id.fetch_add(1, Ordering::SeqCst);
        let port_id = self.next_port_id.fetch_add(1, Ordering::SeqCst);
        
        let sock = Arc::new(NetlinkSocket::new(id, protocol, port_id));
        self.sockets.lock().insert(port_id, sock.clone());
        
        sock
    }

    /// Close socket
    pub fn close_socket(&self, port_id: u32) {
        self.sockets.lock().remove(&port_id);
        
        // Remove from all groups
        for members in self.groups.lock().values_mut() {
            members.retain(|&p| p != port_id);
        }
    }

    /// Get socket
    pub fn get_socket(&self, port_id: u32) -> Option<Arc<NetlinkSocket>> {
        self.sockets.lock().get(&port_id).cloned()
    }

    /// Broadcast to group
    pub fn broadcast(&self, group: u32, msg: NetlinkMessage) {
        if let Some(members) = self.groups.lock().get(&group) {
            for port_id in members {
                if let Some(sock) = self.sockets.lock().get(port_id) {
                    sock.rx_buf.lock().push(msg.clone());
                }
            }
        }
    }

    /// Join group
    pub fn join_group(&self, port_id: u32, group: u32) {
        self.groups.lock()
            .entry(group)
            .or_insert_with(Vec::new)
            .push(port_id);
    }

    /// Leave group
    pub fn leave_group(&self, port_id: u32, group: u32) {
        if let Some(members) = self.groups.lock().get_mut(&group) {
            members.retain(|&p| p != port_id);
        }
    }
}

lazy_static::lazy_static! {
    pub static ref NETLINK_MANAGER: NetlinkManager = NetlinkManager::new();
    pub static ref NETLINK_SOCKS: Mutex<BTreeMap<u32, Arc<NetlinkSocket>>> = 
        Mutex::new(BTreeMap::new());
}

// ============================================================================
// ERROR TYPE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetlinkError {
    InvalidMessage,
    InvalidProtocol,
    PermissionDenied,
    BufferFull,
    NotFound,
}

// ============================================================================
// SYSCALL INTERFACE
// ============================================================================

pub fn sys_socket_netlink(protocol: u32) -> i32 {
    let sock = NETLINK_MANAGER.create_socket(protocol);
    sock.port_id.load(Ordering::Relaxed) as i32
}

pub fn sys_sendmsg_netlink(port_id: u32, buf: &[u8], flags: u32) -> i32 {
    if buf.len() < NlMsgHdr::size() {
        return -22;
    }
    
    let header = unsafe {
        *(buf.as_ptr() as *const NlMsgHdr)
    };
    
    let msg = NetlinkMessage {
        header,
        payload: buf[NlMsgHdr::size()..].to_vec(),
    };
    
    if let Some(sock) = NETLINK_MANAGER.get_socket(port_id) {
        match sock.send(msg) {
            Ok(()) => buf.len() as i32,
            Err(_) => -5,
        }
    } else {
        -9
    }
}

pub fn sys_recvmsg_netlink(port_id: u32, buf: &mut [u8], flags: u32) -> i32 {
    if let Some(sock) = NETLINK_MANAGER.get_socket(port_id) {
        if let Some(msg) = sock.recv() {
            let total_len = NlMsgHdr::size() + msg.payload.len();
            if buf.len() < total_len {
                return -7;
            }
            
            // Write header
            unsafe {
                let ptr = buf.as_mut_ptr() as *mut NlMsgHdr;
                (*ptr) = msg.header;
            }
            
            // Write payload
            buf[NlMsgHdr::size()..total_len].copy_from_slice(&msg.payload);
            
            return total_len as i32;
        }
        return -11; // EAGAIN
    }
    -9
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init() {
    crate::serial_println!("[NETLINK] Subsystem initialized");
}
