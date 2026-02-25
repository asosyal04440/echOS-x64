//! # smoltcp Integration for echOS
//!
//! smoltcp TCP/IP stack için basit interface

use alloc::vec::Vec;
use alloc::string::String;
use spin::Mutex;

/// Network interface state
pub struct NetInterface {
    pub ip: Option<[u8; 4]>,
    pub gateway: Option<[u8; 4]>,
    pub dns: Option<[u8; 4]>,
    pub mac: Option<[u8; 6]>,
}

impl NetInterface {
    pub fn new() -> Self {
        Self {
            ip: None,
            gateway: None,
            dns: None,
            mac: None,
        }
    }
}

// Global network state
use lazy_static::lazy_static;

lazy_static! {
    static ref NET_INTERFACE: Mutex<NetInterface> = Mutex::new(NetInterface::new());
}

/// Get network interface
pub fn get_interface() -> &'static Mutex<NetInterface> {
    &NET_INTERFACE
}

/// Initialize smoltcp interface
pub fn init() -> bool {
    crate::serial_println!("[smoltcp] Interface initialized");
    
    // VirtIO-Net'den MAC adresi al
    if crate::drivers::virtio_net::is_initialized() {
        crate::serial_println!("[smoltcp] VirtIO-Net available");
    }
    
    true
}

/// Configure network with DHCP
pub fn dhcp_configure() -> bool {
    crate::serial_println!("[smoltcp] DHCP configuration started");
    
    // TODO: smoltcp DHCP client kullan
    // Şimdilik QEMU user-mode network için varsayılan IP
    let mut iface = NET_INTERFACE.lock();
    iface.ip = Some([10, 0, 2, 15]);      // QEMU default guest IP
    iface.gateway = Some([10, 0, 2, 2]);  // QEMU default gateway
    iface.dns = Some([10, 0, 2, 3]);      // QEMU default DNS
    
    crate::serial_println!("[smoltcp] DHCP configured: IP={:?}", iface.ip);
    true
}

/// Get IP address
pub fn get_ip() -> Option<[u8; 4]> {
    NET_INTERFACE.lock().ip
}

/// Get gateway
pub fn get_gateway() -> Option<[u8; 4]> {
    NET_INTERFACE.lock().gateway
}

/// Get DNS server
pub fn get_dns() -> Option<[u8; 4]> {
    NET_INTERFACE.lock().dns
}

/// DNS lookup using smoltcp
pub fn dns_lookup(hostname: &str) -> Option<[u8; 4]> {
    crate::serial_println!("[smoltcp] DNS lookup: {}", hostname);
    
    // TODO: smoltcp DNS socket kullan
    // Şimdilik bilinen hostlar için hardcoded
    match hostname {
        "localhost" => Some([127, 0, 0, 1]),
        "gateway" => get_gateway(),
        _ => {
            crate::serial_println!("[smoltcp] DNS: unknown host");
            None
        }
    }
}

/// Simple TCP connect
pub fn tcp_connect(_ip: [u8; 4], _port: u16) -> bool {
    // TODO: smoltcp TCP socket
    crate::serial_println!("[smoltcp] TCP connect not implemented");
    false
}

/// Simple HTTP GET
pub fn http_get(_url: &str) -> Result<Vec<u8>, String> {
    // TODO: smoltcp HTTP
    Err(String::from("HTTP not implemented"))
}
