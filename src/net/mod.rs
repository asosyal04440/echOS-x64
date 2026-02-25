//! # echOS Networking Stack
//!
//! Tier 1 OS seviyesinde TCP/IP implementasyonu
//! POSIX socket API uyumlu

pub mod socket;
pub mod tcp;
pub mod udp;
pub mod ip;
pub mod ipv6;
pub mod ethernet;
pub mod arp;
pub mod dhcp;
pub mod dns;
pub mod dnssec;
pub mod doh;
pub mod dot;
pub mod netdev;
pub mod http;
pub mod http2;
pub mod websocket;
pub mod smoltcp_driver;
pub mod tls;
pub mod io_uring;
pub mod x509;
pub mod quic;
pub mod zero_copy;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::format;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::{AtomicU32, AtomicBool, Ordering};
use spin::Mutex;

// ============================================================================
// NETWORK CONFIGURATION
// ============================================================================

/// Maximum socket count
const MAX_SOCKETS: usize = 4096;

/// Network buffer size
const NET_BUF_SIZE: usize = 1514; // MTU + Ethernet header

/// Global network configuration
static NET_CONFIG: Mutex<NetworkConfig> = Mutex::new(NetworkConfig {
    ip_addr: [0, 0, 0, 0],
    netmask: [255, 255, 255, 0],
    gateway: [0, 0, 0, 0],
    dns_servers: Vec::new(),
    hostname: String::new(),
});

/// Network configuration
#[derive(Clone, Debug)]
pub struct NetworkConfig {
    pub ip_addr: [u8; 4],
    pub netmask: [u8; 4],
    pub gateway: [u8; 4],
    pub dns_servers: Vec<[u8; 4]>,
    pub hostname: String,
}

impl NetworkConfig {
    pub fn new() -> Self {
        Self {
            ip_addr: [0, 0, 0, 0],
            netmask: [255, 255, 255, 0],
            gateway: [0, 0, 0, 0],
            dns_servers: Vec::new(),
            hostname: String::from("echos"),
        }
    }
    
    pub fn is_configured(&self) -> bool {
        self.ip_addr != [0, 0, 0, 0]
    }
}

// ============================================================================
// MAC ADDRESS
// ============================================================================

/// MAC Address
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MacAddr(pub [u8; 6]);

impl MacAddr {
    pub const BROADCAST: MacAddr = MacAddr([0xFF; 6]);
    pub const ZERO: MacAddr = MacAddr([0x00; 6]);
    
    pub fn new(bytes: [u8; 6]) -> Self {
        MacAddr(bytes)
    }
    
    pub fn from_bytes(a: u8, b: u8, c: u8, d: u8, e: u8, f: u8) -> Self {
        MacAddr([a, b, c, d, e, f])
    }
    
    pub fn is_broadcast(&self) -> bool {
        self.0 == [0xFF; 6]
    }
    
    pub fn is_multicast(&self) -> bool {
        self.0[0] & 0x01 != 0
    }
    
    pub fn as_bytes(&self) -> &[u8; 6] {
        &self.0
    }
}

impl Default for MacAddr {
    fn default() -> Self {
        MacAddr::ZERO
    }
}

// ============================================================================
// IP ADDRESS
// ============================================================================

/// IPv4 Address
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ipv4Addr(pub [u8; 4]);

impl Ipv4Addr {
    pub const UNSPECIFIED: Ipv4Addr = Ipv4Addr([0, 0, 0, 0]);
    pub const BROADCAST: Ipv4Addr = Ipv4Addr([255, 255, 255, 255]);
    pub const LOCALHOST: Ipv4Addr = Ipv4Addr([127, 0, 0, 1]);
    
    pub fn new(a: u8, b: u8, c: u8, d: u8) -> Self {
        Ipv4Addr([a, b, c, d])
    }
    
    pub fn from_bytes(bytes: [u8; 4]) -> Self {
        Ipv4Addr(bytes)
    }
    
    pub fn is_unspecified(&self) -> bool {
        self.0 == [0, 0, 0, 0]
    }
    
    pub fn is_loopback(&self) -> bool {
        self.0[0] == 127
    }
    
    pub fn is_private(&self) -> bool {
        // 10.0.0.0/8
        self.0[0] == 10 ||
        // 172.16.0.0/12
        (self.0[0] == 172 && self.0[1] >= 16 && self.0[1] <= 31) ||
        // 192.168.0.0/16
        (self.0[0] == 192 && self.0[1] == 168)
    }
    
    pub fn is_multicast(&self) -> bool {
        self.0[0] >= 224 && self.0[0] <= 239
    }
    
    pub fn is_broadcast(&self) -> bool {
        self.0 == [255, 255, 255, 255]
    }
    
    pub fn as_bytes(&self) -> &[u8; 4] {
        &self.0
    }
    
    pub fn to_u32(&self) -> u32 {
        u32::from_be_bytes(self.0)
    }
    
    pub fn from_u32(val: u32) -> Self {
        Ipv4Addr(val.to_be_bytes())
    }
}

impl Default for Ipv4Addr {
    fn default() -> Self {
        Ipv4Addr::UNSPECIFIED
    }
}

impl core::fmt::Display for Ipv4Addr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}.{}.{}.{}", self.0[0], self.0[1], self.0[2], self.0[3])
    }
}

// ============================================================================
// PORT
// ============================================================================

/// Network port
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Port(pub u16);

impl Port {
    pub const HTTP: Port = Port(80);
    pub const HTTPS: Port = Port(443);
    pub const SSH: Port = Port(22);
    pub const DNS: Port = Port(53);
    pub const DHCP_CLIENT: Port = Port(68);
    pub const DHCP_SERVER: Port = Port(67);
    
    pub fn new(port: u16) -> Self {
        Port(port)
    }
    
    pub fn is_system(&self) -> bool {
        self.0 < 1024
    }
    
    pub fn is_dynamic(&self) -> bool {
        self.0 >= 49152
    }
    
    pub fn as_u16(&self) -> u16 {
        self.0
    }
}

// ============================================================================
// SOCKET ADDRESS
// ============================================================================

/// Socket address (IP + Port)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SocketAddr {
    pub ip: Ipv4Addr,
    pub port: Port,
}

impl SocketAddr {
    pub fn new(ip: Ipv4Addr, port: Port) -> Self {
        SocketAddr { ip, port }
    }
    
    pub fn unspecified(port: Port) -> Self {
        SocketAddr {
            ip: Ipv4Addr::UNSPECIFIED,
            port,
        }
    }
}

impl Default for SocketAddr {
    fn default() -> Self {
        SocketAddr::unspecified(Port(0))
    }
}

// ============================================================================
// NETWORK INTERFACE
// ============================================================================

/// Network interface statistics
#[derive(Clone, Debug, Default)]
pub struct NetStats {
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_errors: u64,
    pub tx_errors: u64,
    pub rx_dropped: u64,
    pub tx_dropped: u64,
}

/// Network interface
pub trait NetInterface: Send + Sync {
    /// Get interface name
    fn name(&self) -> &str;
    
    /// Get MAC address
    fn mac(&self) -> MacAddr;
    
    /// Get IP address
    fn ip(&self) -> Ipv4Addr;
    
    /// Set IP address
    fn set_ip(&mut self, ip: Ipv4Addr);
    
    /// Get netmask
    fn netmask(&self) -> Ipv4Addr;
    
    /// Set netmask
    fn set_netmask(&mut self, mask: Ipv4Addr);
    
    /// Get gateway
    fn gateway(&self) -> Option<Ipv4Addr>;
    
    /// Set gateway
    fn set_gateway(&mut self, gw: Ipv4Addr);
    
    /// Check if interface is up
    fn is_up(&self) -> bool;
    
    /// Bring interface up/down
    fn set_up(&mut self, up: bool);
    
    /// Send raw packet
    fn send(&mut self, data: &[u8]) -> Result<(), NetError>;
    
    /// Receive raw packet (non-blocking)
    fn recv(&mut self) -> Option<Vec<u8>>;
    
    /// Get statistics
    fn stats(&self) -> NetStats;
    
    /// Get MTU
    fn mtu(&self) -> u16 {
        1500
    }
}

/// Network error
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetError {
    NoInterface,
    NotUp,
    BufferFull,
    BufferEmpty,
    InvalidPacket,
    InvalidFd,
    InvalidParam,
    ChecksumError,
    Timeout,
    ConnectionRefused,
    ConnectionReset,
    ConnectionClosed,
    WouldBlock,
    AddrInUse,
    AddrNotAvailable,
    NetworkUnreachable,
    HostUnreachable,
    ProtocolError,
    NotSupported,
    Unknown,
}

// ============================================================================
// NETWORK MANAGER
// ============================================================================

/// Global network manager
static NET_INTERFACES: Mutex<Vec<Arc<Mutex<dyn NetInterface>>>> = Mutex::new(Vec::new());
static NET_INITIALIZED: AtomicBool = AtomicBool::new(false);
static NEXT_SOCKET_ID: AtomicU32 = AtomicU32::new(1);

/// Initialize networking subsystem
pub fn init() {
    if NET_INITIALIZED.swap(true, Ordering::SeqCst) {
        return;
    }
    
    crate::serial_println!("[NET] Initializing networking stack...");
    
    // Initialize network device drivers
    netdev::init();
    
    // Initialize protocols
    arp::init();
    tcp::init();
    udp::init();
    
    crate::serial_println!("[NET] Networking stack initialized");
}

/// Register network interface
pub fn register_interface(iface: Arc<Mutex<dyn NetInterface>>) {
    let mut interfaces = NET_INTERFACES.lock();
    interfaces.push(iface);
    crate::serial_println!("[NET] Interface registered: {}", interfaces.last().unwrap().lock().name());
}

/// Get interface by name
pub fn get_interface(name: &str) -> Option<Arc<Mutex<dyn NetInterface>>> {
    let interfaces = NET_INTERFACES.lock();
    for iface in interfaces.iter() {
        if iface.lock().name() == name {
            return Some(iface.clone());
        }
    }
    None
}

/// Get default interface
pub fn default_interface() -> Option<Arc<Mutex<dyn NetInterface>>> {
    let interfaces = NET_INTERFACES.lock();
    if !interfaces.is_empty() {
        Some(interfaces[0].clone())
    } else {
        None
    }
}

/// Allocate socket ID
pub fn allocate_socket_id() -> u32 {
    NEXT_SOCKET_ID.fetch_add(1, Ordering::Relaxed)
}

/// Check if network is configured
pub fn is_configured() -> bool {
    NET_CONFIG.lock().is_configured()
}

/// Get network configuration
pub fn get_config() -> NetworkConfig {
    NET_CONFIG.lock().clone()
}

/// Set network configuration
pub fn set_config(config: NetworkConfig) {
    let mut cfg = NET_CONFIG.lock();
    *cfg = config;
}

/// Get local IP address
pub fn local_ip() -> Ipv4Addr {
    Ipv4Addr::from_bytes(NET_CONFIG.lock().ip_addr)
}

/// Process incoming packet
pub fn process_packet(data: &[u8]) -> Result<(), NetError> {
    // Parse Ethernet frame
    let frame = ethernet::EthernetFrame::parse(data)?;
    
    // Dispatch by EtherType
    match frame.ether_type() {
        ethernet::EtherType::ARP => {
            arp::process_packet(&frame.payload)?;
        }
        ethernet::EtherType::IPV4 => {
            ip::process_packet(&frame.payload)?;
        }
        _ => {
            // Unknown protocol, drop
        }
    }
    
    Ok(())
}

/// Send packet through default interface
pub fn send_packet(data: &[u8]) -> Result<(), NetError> {
    let iface = default_interface().ok_or(NetError::NoInterface)?;
    iface.lock().send(data)?;
    Ok(())
}

// ============================================================================
// NETWORK UTILITIES (ss, nc, traceroute)
// ============================================================================

/// Socket statistics entry
#[derive(Clone, Debug)]
pub struct SocketStats {
    pub id: u32,
    pub proto: String,
    pub local: String,
    pub remote: String,
    pub state: String,
    pub rx_bytes: usize,
    pub tx_bytes: usize,
}

/// Get socket statistics (ss command)
pub fn get_socket_stats() -> Vec<SocketStats> {
    let mut stats = Vec::new();
    
    // TCP connections
    let tcp_conns = tcp::get_all_connections();
    for conn in tcp_conns {
        let state_str = match conn.state {
            tcp::TcpState::Closed => "CLOSED",
            tcp::TcpState::Listen => "LISTEN",
            tcp::TcpState::SynSent => "SYN-SENT",
            tcp::TcpState::SynReceived => "SYN-RECV",
            tcp::TcpState::Established => "ESTAB",
            tcp::TcpState::FinWait1 => "FIN-WAIT1",
            tcp::TcpState::FinWait2 => "FIN-WAIT2",
            tcp::TcpState::CloseWait => "CLOSE-WAIT",
            tcp::TcpState::Closing => "CLOSING",
            tcp::TcpState::LastAck => "LAST-ACK",
            tcp::TcpState::TimeWait => "TIME-WAIT",
        };
        
        stats.push(SocketStats {
            id: conn.id,
            proto: String::from("tcp"),
            local: format!("{}:{}", 
                socket::format_ipv4(conn.local.ip),
                conn.local.port.0),
            remote: format!("{}:{}", 
                socket::format_ipv4(conn.remote.ip),
                conn.remote.port.0),
            state: String::from(state_str),
            rx_bytes: conn.rx_buffer.len(),
            tx_bytes: conn.tx_buffer.len(),
        });
    }
    
    // UDP sockets
    let udp_socks = udp::get_all_sockets();
    for sock in udp_socks {
        stats.push(SocketStats {
            id: sock.id,
            proto: String::from("udp"),
            local: format!("{}:{}", 
                socket::format_ipv4(sock.local.ip),
                sock.local.port.0),
            remote: String::from("*:*"),
            state: String::from(" "),
            rx_bytes: sock.rx_buffer.iter().map(|(_, v)| v.len()).sum(),
            tx_bytes: 0,
        });
    }
    
    stats
}

/// Netcat - connect to host
pub fn nc_connect(host: &str, port: u16) -> Result<u32, NetError> {
    let dns_server = get_config().dns_servers.first()
        .map(|ip| Ipv4Addr::from_bytes(*ip))
        .ok_or(NetError::NetworkUnreachable)?;
    
    let ip = dns::resolve(host, dns_server)?;
    let addr = SocketAddr::new(ip, Port(port));
    
    let sock = socket::socket(
        socket::AddressFamily::IPV4,
        socket::SocketType::STREAM,
        socket::Protocol::TCP,
    )?;
    
    socket::connect(sock, addr)?;
    Ok(sock)
}

/// Netcat - send data
pub fn nc_send(sock: u32, data: &[u8]) -> Result<usize, NetError> {
    socket::send(sock, data, 0)
}

/// Netcat - receive data
pub fn nc_recv(sock: u32, buf: &mut [u8]) -> Result<usize, NetError> {
    socket::recv(sock, buf, 0)
}

/// Netcat - listen mode
pub fn nc_listen(port: u16) -> Result<u32, NetError> {
    let sock = socket::socket(
        socket::AddressFamily::IPV4,
        socket::SocketType::STREAM,
        socket::Protocol::TCP,
    )?;
    
    socket::bind(sock, SocketAddr::new(Ipv4Addr::UNSPECIFIED, Port(port)))?;
    socket::listen(sock, 1)?;
    Ok(sock)
}

/// Netcat - accept connection
pub fn nc_accept(sock: u32) -> Result<(u32, SocketAddr), NetError> {
    socket::accept(sock)
}

/// Traceroute hop info
#[derive(Clone, Debug)]
pub struct TracerouteHop {
    pub hop: u8,
    pub ip: Ipv4Addr,
    pub rtt_ms: u32,
    pub reached: bool,
}

/// Traceroute to destination
pub fn traceroute(dest: Ipv4Addr, max_hops: u8) -> Result<Vec<TracerouteHop>, NetError> {
    let mut hops = Vec::new();
    
    // Get gateway for routing
    let config = get_config();
    let gateway = Ipv4Addr::from_bytes(config.gateway);
    
    // Simple traceroute simulation
    // In real implementation, would send ICMP/UDP with increasing TTL
    for ttl in 1..=max_hops {
        // Check if we've reached destination
        if ttl == 1 {
            // First hop is usually gateway
            if !gateway.is_unspecified() {
                hops.push(TracerouteHop {
                    hop: ttl,
                    ip: gateway,
                    rtt_ms: 1,
                    reached: false,
                });
            }
        } else if ttl == max_hops || ttl >= 16 {
            // Assume we reached destination
            hops.push(TracerouteHop {
                hop: ttl,
                ip: dest,
                rtt_ms: ttl as u32 * 10,
                reached: true,
            });
            break;
        } else {
            // Intermediate hop (placeholder)
            // Real implementation would parse ICMP Time Exceeded responses
            let hop_ip = Ipv4Addr::from_bytes([
                gateway.0[0],
                gateway.0[1],
                gateway.0[2],
                ttl,
            ]);
            hops.push(TracerouteHop {
                hop: ttl,
                ip: hop_ip,
                rtt_ms: ttl as u32 * 5,
                reached: false,
            });
        }
    }
    
    Ok(hops)
}

/// Ping destination
pub fn ping(dest: Ipv4Addr, count: u8) -> Result<Vec<(u32, bool)>, NetError> {
    let mut results = Vec::new();
    
    // Simple ping simulation
    // Real implementation would use ICMP Echo Request/Reply
    for i in 0..count {
        // Simulate RTT (would be measured from actual ICMP)
        let rtt = 5 + (i as u32 * 2);
        let success = i < count - 1; // Simulate some packet loss
        
        results.push((rtt, success));
    }
    
    Ok(results)
}

/// Get ARP table
pub fn get_arp_table() -> Vec<(Ipv4Addr, MacAddr)> {
    arp::get_table()
}

/// Get network interfaces
pub fn get_interfaces() -> Vec<InterfaceInfo> {
    let mut interfaces = Vec::new();
    
    if let Some(iface) = default_interface() {
        let netdev = iface.lock();
        interfaces.push(InterfaceInfo {
            name: String::from("eth0"),
            mac: netdev.mac(),
            ip: local_ip(),
            netmask: Ipv4Addr::from_bytes(get_config().netmask),
            gateway: Ipv4Addr::from_bytes(get_config().gateway),
            mtu: 1500,
            up: true,
        });
    }
    
    interfaces
}

/// Interface information
#[derive(Clone, Debug)]
pub struct InterfaceInfo {
    pub name: String,
    pub mac: MacAddr,
    pub ip: Ipv4Addr,
    pub netmask: Ipv4Addr,
    pub gateway: Ipv4Addr,
    pub mtu: usize,
    pub up: bool,
}
