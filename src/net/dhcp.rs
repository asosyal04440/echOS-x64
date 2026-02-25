//! # DHCP Client
//!
//! Dynamic Host Configuration Protocol for automatic IP configuration

use super::{Ipv4Addr, MacAddr, Port, NetError, SocketAddr};
use super::udp;
use super::socket::{socket, bind, sendto, recvfrom, close};
use alloc::vec::Vec;
use alloc::vec;
use alloc::boxed::Box;
use spin::Mutex;
use core::sync::atomic::{AtomicBool, Ordering};

/// DHCP client port
const DHCP_CLIENT_PORT: u16 = 68;
/// DHCP server port
const DHCP_SERVER_PORT: u16 = 67;

/// DHCP message types
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DhcpMessageType {
    Discover = 1,
    Offer = 2,
    Request = 3,
    Decline = 4,
    Ack = 5,
    Nak = 6,
    Release = 7,
    Inform = 8,
    Unknown = 0,
}

impl DhcpMessageType {
    pub fn from_u8(val: u8) -> Self {
        match val {
            1 => DhcpMessageType::Discover,
            2 => DhcpMessageType::Offer,
            3 => DhcpMessageType::Request,
            4 => DhcpMessageType::Decline,
            5 => DhcpMessageType::Ack,
            6 => DhcpMessageType::Nak,
            7 => DhcpMessageType::Release,
            8 => DhcpMessageType::Inform,
            _ => DhcpMessageType::Unknown,
        }
    }
}

/// DHCP header
#[derive(Clone, Debug)]
pub struct DhcpMessage {
    pub op: u8,           // 1=request, 2=reply
    pub htype: u8,        // Hardware type (1=Ethernet)
    pub hlen: u8,         // Hardware address length (6)
    pub hops: u8,
    pub xid: u32,         // Transaction ID
    pub secs: u16,
    pub flags: u16,
    pub ciaddr: Ipv4Addr, // Client IP
    pub yiaddr: Ipv4Addr, // Your IP
    pub siaddr: Ipv4Addr, // Server IP
    pub giaddr: Ipv4Addr, // Gateway IP
    pub chaddr: [u8; 16], // Client hardware address
    pub sname: [u8; 64],  // Server name
    pub file: [u8; 128],  // Boot file
    pub options: Vec<u8>,
}

impl DhcpMessage {
    pub const MIN_SIZE: usize = 236;
    pub const MAGIC_COOKIE: [u8; 4] = [0x63, 0x82, 0x53, 0x63];
    
    pub fn new_discover(mac: MacAddr, xid: u32) -> Self {
        let mut chaddr = [0u8; 16];
        chaddr[..6].copy_from_slice(mac.as_bytes());
        
        let mut options = Vec::new();
        // Magic cookie
        options.extend_from_slice(&Self::MAGIC_COOKIE);
        // DHCP Message Type option
        options.push(53); // Option: Message Type
        options.push(1);  // Length
        options.push(DhcpMessageType::Discover as u8);
        // Client Identifier
        options.push(61); // Option: Client Identifier
        options.push(7);  // Length
        options.push(1);  // Hardware type: Ethernet
        options.extend_from_slice(mac.as_bytes());
        // Parameter Request List
        options.push(55); // Option: Parameter Request List
        options.push(4);  // Length
        options.push(1);  // Subnet Mask
        options.push(3);  // Router
        options.push(6);  // DNS Server
        options.push(15); // Domain Name
        // End option
        options.push(255);
        
        DhcpMessage {
            op: 1,
            htype: 1,
            hlen: 6,
            hops: 0,
            xid,
            secs: 0,
            flags: 0x8000, // Broadcast flag
            ciaddr: Ipv4Addr::UNSPECIFIED,
            yiaddr: Ipv4Addr::UNSPECIFIED,
            siaddr: Ipv4Addr::UNSPECIFIED,
            giaddr: Ipv4Addr::UNSPECIFIED,
            chaddr,
            sname: [0u8; 64],
            file: [0u8; 128],
            options,
        }
    }
    
    pub fn new_request(mac: MacAddr, xid: u32, requested_ip: Ipv4Addr, server_ip: Ipv4Addr) -> Self {
        let mut chaddr = [0u8; 16];
        chaddr[..6].copy_from_slice(mac.as_bytes());
        
        let mut options = Vec::new();
        options.extend_from_slice(&Self::MAGIC_COOKIE);
        // DHCP Message Type
        options.push(53);
        options.push(1);
        options.push(DhcpMessageType::Request as u8);
        // Requested IP
        options.push(50);
        options.push(4);
        options.extend_from_slice(requested_ip.as_bytes());
        // Server Identifier
        options.push(54);
        options.push(4);
        options.extend_from_slice(server_ip.as_bytes());
        // End
        options.push(255);
        
        DhcpMessage {
            op: 1,
            htype: 1,
            hlen: 6,
            hops: 0,
            xid,
            secs: 0,
            flags: 0x8000,
            ciaddr: Ipv4Addr::UNSPECIFIED,
            yiaddr: Ipv4Addr::UNSPECIFIED,
            siaddr: Ipv4Addr::UNSPECIFIED,
            giaddr: Ipv4Addr::UNSPECIFIED,
            chaddr,
            sname: [0u8; 64],
            file: [0u8; 128],
            options,
        }
    }
    
    pub fn parse(data: &[u8]) -> Result<Self, NetError> {
        if data.len() < Self::MIN_SIZE {
            return Err(NetError::InvalidPacket);
        }
        
        let op = data[0];
        let htype = data[1];
        let hlen = data[2];
        let hops = data[3];
        let xid = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        let secs = u16::from_be_bytes([data[8], data[9]]);
        let flags = u16::from_be_bytes([data[10], data[11]]);
        let ciaddr = Ipv4Addr::from_bytes([data[12], data[13], data[14], data[15]]);
        let yiaddr = Ipv4Addr::from_bytes([data[16], data[17], data[18], data[19]]);
        let siaddr = Ipv4Addr::from_bytes([data[20], data[21], data[22], data[23]]);
        let giaddr = Ipv4Addr::from_bytes([data[24], data[25], data[26], data[27]]);
        
        let mut chaddr = [0u8; 16];
        chaddr.copy_from_slice(&data[28..44]);
        
        let mut sname = [0u8; 64];
        sname.copy_from_slice(&data[44..108]);
        
        let mut file = [0u8; 128];
        file.copy_from_slice(&data[108..236]);
        
        let options = data[236..].to_vec();
        
        Ok(DhcpMessage {
            op, htype, hlen, hops, xid, secs, flags,
            ciaddr, yiaddr, siaddr, giaddr,
            chaddr, sname, file, options,
        })
    }
    
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::MIN_SIZE + self.options.len());
        
        buf.push(self.op);
        buf.push(self.htype);
        buf.push(self.hlen);
        buf.push(self.hops);
        buf.extend_from_slice(&self.xid.to_be_bytes());
        buf.extend_from_slice(&self.secs.to_be_bytes());
        buf.extend_from_slice(&self.flags.to_be_bytes());
        buf.extend_from_slice(self.ciaddr.as_bytes());
        buf.extend_from_slice(self.yiaddr.as_bytes());
        buf.extend_from_slice(self.siaddr.as_bytes());
        buf.extend_from_slice(self.giaddr.as_bytes());
        buf.extend_from_slice(&self.chaddr);
        buf.extend_from_slice(&self.sname);
        buf.extend_from_slice(&self.file);
        buf.extend(&self.options);
        
        buf
    }
    
    pub fn get_message_type(&self) -> DhcpMessageType {
        for i in 0..self.options.len() {
            if self.options[i] == 53 && i + 2 < self.options.len() {
                return DhcpMessageType::from_u8(self.options[i + 2]);
            }
        }
        DhcpMessageType::Unknown
    }
    
    pub fn get_option(&self, code: u8) -> Option<&[u8]> {
        let mut i = 4; // Skip magic cookie
        while i < self.options.len() {
            let opt_code = self.options[i];
            if opt_code == 255 {
                break;
            }
            if opt_code == 0 {
                i += 1;
                continue;
            }
            if i + 1 >= self.options.len() {
                break;
            }
            let opt_len = self.options[i + 1] as usize;
            if opt_code == code {
                return Some(&self.options[i + 2..i + 2 + opt_len]);
            }
            i += 2 + opt_len;
        }
        None
    }
}

// ============================================================================
// DHCP CLIENT
// ============================================================================

static DHCP_SOCKET: Mutex<Option<u32>> = Mutex::new(None);
static DHCP_CONFIGURED: AtomicBool = AtomicBool::new(false);

/// DHCP lease state
#[derive(Clone, Debug)]
pub struct DhcpLease {
    pub ip: Ipv4Addr,
    pub subnet_mask: Ipv4Addr,
    pub gateway: Ipv4Addr,
    pub dns_servers: Vec<Ipv4Addr>,
    pub server_ip: Ipv4Addr,
    pub lease_time: u32,        // seconds
    pub renewal_time: u32,      // T1 (typically 0.5 * lease_time)
    pub rebinding_time: u32,    // T2 (typically 0.875 * lease_time)
    pub obtained_at: u64,       // timestamp when lease was obtained
    pub xid: u32,               // transaction ID for renewal
}

impl DhcpLease {
    pub fn new() -> Self {
        Self {
            ip: Ipv4Addr::UNSPECIFIED,
            subnet_mask: Ipv4Addr::from_bytes([255, 255, 255, 0]),
            gateway: Ipv4Addr::UNSPECIFIED,
            dns_servers: Vec::new(),
            server_ip: Ipv4Addr::UNSPECIFIED,
            lease_time: 0,
            renewal_time: 0,
            rebinding_time: 0,
            obtained_at: 0,
            xid: 0,
        }
    }
    
    /// Check if lease is valid
    pub fn is_valid(&self) -> bool {
        !self.ip.is_unspecified() && self.lease_time > 0
    }
    
    /// Check if lease needs renewal
    pub fn needs_renewal(&self, current_time: u64) -> bool {
        if !self.is_valid() {
            return false;
        }
        let elapsed = current_time.saturating_sub(self.obtained_at);
        elapsed >= self.renewal_time as u64
    }
    
    /// Check if lease needs rebinding
    pub fn needs_rebinding(&self, current_time: u64) -> bool {
        if !self.is_valid() {
            return false;
        }
        let elapsed = current_time.saturating_sub(self.obtained_at);
        elapsed >= self.rebinding_time as u64
    }
    
    /// Check if lease is expired
    pub fn is_expired(&self, current_time: u64) -> bool {
        if !self.is_valid() {
            return true;
        }
        let elapsed = current_time.saturating_sub(self.obtained_at);
        elapsed >= self.lease_time as u64
    }
    
    /// Get remaining lease time
    pub fn remaining_time(&self, current_time: u64) -> u32 {
        if !self.is_valid() {
            return 0;
        }
        let elapsed = current_time.saturating_sub(self.obtained_at);
        self.lease_time.saturating_sub(elapsed as u32)
    }
}

impl Default for DhcpLease {
    fn default() -> Self {
        Self::new()
    }
}

/// Global DHCP lease state
static DHCP_LEASE: Mutex<Option<DhcpLease>> = Mutex::new(None);

/// Initialize DHCP client
pub fn init() {
    crate::serial_println!("[DHCP] Client initialized");
}

/// Get current DHCP lease
pub fn get_lease() -> Option<DhcpLease> {
    DHCP_LEASE.lock().clone()
}

/// Create DHCP release message
pub fn new_release(mac: MacAddr, xid: u32, client_ip: Ipv4Addr, server_ip: Ipv4Addr) -> DhcpMessage {
    let mut chaddr = [0u8; 16];
    chaddr[..6].copy_from_slice(mac.as_bytes());
    
    let mut options = Vec::new();
    options.extend_from_slice(&DhcpMessage::MAGIC_COOKIE);
    // DHCP Message Type
    options.push(53);
    options.push(1);
    options.push(DhcpMessageType::Release as u8);
    // Server Identifier
    options.push(54);
    options.push(4);
    options.extend_from_slice(server_ip.as_bytes());
    // End
    options.push(255);
    
    DhcpMessage {
        op: 1,
        htype: 1,
        hlen: 6,
        hops: 0,
        xid,
        secs: 0,
        flags: 0,
        ciaddr: client_ip,
        yiaddr: Ipv4Addr::UNSPECIFIED,
        siaddr: server_ip,
        giaddr: Ipv4Addr::UNSPECIFIED,
        chaddr,
        sname: [0u8; 64],
        file: [0u8; 128],
        options,
    }
}

/// Create DHCP renew request
pub fn new_renew(mac: MacAddr, xid: u32, client_ip: Ipv4Addr, server_ip: Ipv4Addr) -> DhcpMessage {
    let mut chaddr = [0u8; 16];
    chaddr[..6].copy_from_slice(mac.as_bytes());
    
    let mut options = Vec::new();
    options.extend_from_slice(&DhcpMessage::MAGIC_COOKIE);
    // DHCP Message Type
    options.push(53);
    options.push(1);
    options.push(DhcpMessageType::Request as u8);
    // Client Identifier
    options.push(61);
    options.push(7);
    options.push(1);
    options.extend_from_slice(mac.as_bytes());
    // Server Identifier (for unicast renewal)
    options.push(54);
    options.push(4);
    options.extend_from_slice(server_ip.as_bytes());
    // End
    options.push(255);
    
    DhcpMessage {
        op: 1,
        htype: 1,
        hlen: 6,
        hops: 0,
        xid,
        secs: 0,
        flags: 0, // No broadcast for renewal
        ciaddr: client_ip,
        yiaddr: Ipv4Addr::UNSPECIFIED,
        siaddr: Ipv4Addr::UNSPECIFIED,
        giaddr: Ipv4Addr::UNSPECIFIED,
        chaddr,
        sname: [0u8; 64],
        file: [0u8; 128],
        options,
    }
}

/// Start DHCP discovery
pub fn discover() -> Result<(), NetError> {
    let mac = super::default_interface()
        .ok_or(NetError::NoInterface)?
        .lock()
        .mac();
    
    // Create UDP socket
    let sock_id = socket(
        super::socket::AddressFamily::IPV4,
        super::socket::SocketType::DGRAM,
        super::socket::Protocol::UDP,
    )?;
    
    // Bind to DHCP client port
    bind(sock_id, SocketAddr::new(Ipv4Addr::UNSPECIFIED, Port(DHCP_CLIENT_PORT)))?;
    
    *DHCP_SOCKET.lock() = Some(sock_id);
    
    // Create discover message
    let xid = crate::random::rand_u64() as u32;
    let discover = DhcpMessage::new_discover(mac, xid);
    let data = discover.serialize();
    
    // Send to broadcast
    let dst = SocketAddr::new(Ipv4Addr::BROADCAST, Port(DHCP_SERVER_PORT));
    sendto(sock_id, &data, dst, 0)?;
    
    crate::serial_println!("[DHCP] Discover sent (xid={:#x})", xid);
    
    Ok(())
}

/// Process DHCP response
pub fn process_response() -> Result<super::NetworkConfig, NetError> {
    let sock_id = DHCP_SOCKET.lock().ok_or(NetError::ProtocolError)?;
    
    let mut buf = vec![0u8; 1500];
    let (len, _src) = recvfrom(sock_id, &mut buf, 0)?;
    
    let msg = DhcpMessage::parse(&buf[..len])?;
    
    match msg.get_message_type() {
        DhcpMessageType::Offer => {
            crate::serial_println!("[DHCP] Offer received: {}",
                super::socket::format_ipv4(msg.yiaddr));
            
            // Send request
            let mac = super::default_interface()
                .ok_or(NetError::NoInterface)?
                .lock()
                .mac();
            
            let request = DhcpMessage::new_request(mac, msg.xid, msg.yiaddr, msg.siaddr);
            let data = request.serialize();
            
            let dst = SocketAddr::new(Ipv4Addr::BROADCAST, Port(DHCP_SERVER_PORT));
            sendto(sock_id, &data, dst, 0)?;
            
            crate::serial_println!("[DHCP] Request sent for {}", 
                super::socket::format_ipv4(msg.yiaddr));
            
            Err(NetError::WouldBlock)
        }
        DhcpMessageType::Ack => {
            crate::serial_println!("[DHCP] ACK received!");
            
            let mut config = super::NetworkConfig::new();
            config.ip_addr = *msg.yiaddr.as_bytes();
            
            // Get subnet mask
            if let Some(mask) = msg.get_option(1) {
                config.netmask = [mask[0], mask[1], mask[2], mask[3]];
            }
            
            // Get gateway
            if let Some(gw) = msg.get_option(3) {
                config.gateway = [gw[0], gw[1], gw[2], gw[3]];
            }
            
            // Get DNS servers
            if let Some(dns) = msg.get_option(6) {
                for i in (0..dns.len()).step_by(4) {
                    if i + 4 <= dns.len() {
                        config.dns_servers.push([dns[i], dns[i+1], dns[i+2], dns[i+3]]);
                    }
                }
            }
            
            DHCP_CONFIGURED.store(true, Ordering::SeqCst);
            
            // Close socket
            close(sock_id)?;
            *DHCP_SOCKET.lock() = None;
            
            Ok(config)
        }
        DhcpMessageType::Nak => {
            crate::serial_println!("[DHCP] NAK received");
            Err(NetError::ProtocolError)
        }
        _ => Err(NetError::WouldBlock),
    }
}

/// Check if DHCP is configured
pub fn is_configured() -> bool {
    DHCP_CONFIGURED.load(Ordering::SeqCst)
}
