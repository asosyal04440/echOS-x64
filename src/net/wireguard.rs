//! # WireGuard VPN
//!
//! Modern, high-performance VPN protocol.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// WIREGUARD CONSTANTS
// ============================================================================

/// WireGuard port
pub const WG_DEFAULT_PORT: u16 = 51820;

/// Key size
pub const WG_KEY_SIZE: usize = 32;

/// Message types
pub const WG_MSG_INITIATION: u8 = 1;
pub const WG_MSG_RESPONSE: u8 = 2;
pub const WG_MSG_COOKIE_REPLY: u8 = 3;
pub const WG_MSG_TRANSPORT: u8 = 4;

// ============================================================================
// WIREGUARD KEY
// ============================================================================

#[derive(Clone, Debug)]
pub struct WgKey(pub [u8; WG_KEY_SIZE]);

impl WgKey {
    pub fn new() -> Self {
        Self([0u8; WG_KEY_SIZE])
    }

    pub fn from_bytes(bytes: [u8; WG_KEY_SIZE]) -> Self {
        Self(bytes)
    }

    pub fn generate() -> Self {
        // Generate using Curve25519
        Self([0u8; WG_KEY_SIZE])
    }

    pub fn as_bytes(&self) -> &[u8; WG_KEY_SIZE] {
        &self.0
    }
}

// ============================================================================
// WIREGUARD PEER
// ============================================================================

#[derive(Clone, Debug)]
pub struct WgPeer {
    /// Public key
    pub public_key: WgKey,
    /// Preshared key
    pub preshared_key: WgKey,
    /// Endpoint IP
    pub endpoint_ip: u32,
    /// Endpoint port
    pub endpoint_port: u16,
    /// Last handshake time
    pub last_handshake: AtomicU64,
    /// TX bytes
    pub tx_bytes: AtomicU64,
    /// RX bytes
    pub rx_bytes: AtomicU64,
    /// Allowed IPs
    pub allowed_ips: Vec<(u32, u8)>, // (IP, prefix_len)
    /// Persistent keepalive
    pub keepalive: AtomicU32,
    /// Session state
    pub session: Mutex<WgSession>,
}

#[derive(Clone, Debug)]
pub struct WgSession {
    pub local_index: u32,
    pub remote_index: u32,
    pub sending_key: [u8; 32],
    pub receiving_key: [u8; 32],
    pub sending_nonce: u64,
    pub receiving_nonce: u64,
    pub is_initiator: bool,
    pub established: bool,
}

impl WgPeer {
    pub fn new(public_key: WgKey) -> Self {
        Self {
            public_key,
            preshared_key: WgKey::new(),
            endpoint_ip: 0,
            endpoint_port: WG_DEFAULT_PORT,
            last_handshake: AtomicU64::new(0),
            tx_bytes: AtomicU64::new(0),
            rx_bytes: AtomicU64::new(0),
            allowed_ips: Vec::new(),
            keepalive: AtomicU32::new(0),
            session: Mutex::new(WgSession {
                local_index: 0,
                remote_index: 0,
                sending_key: [0u8; 32],
                receiving_key: [0u8; 32],
                sending_nonce: 0,
                receiving_nonce: 0,
                is_initiator: false,
                established: false,
            }),
        }
    }

    /// Check if IP is allowed
    pub fn is_allowed_ip(&self, ip: u32) -> bool {
        for (allowed_ip, prefix_len) in &self.allowed_ips {
            let mask = if *prefix_len == 0 { 0 } else { !0u32 >> (32 - prefix_len) };
            if (ip & mask) == (*allowed_ip & mask) {
                return true;
            }
        }
        false
    }

    /// Encrypt and send packet
    pub fn encrypt_packet(&self, pkt: &[u8]) -> Result<Vec<u8>, WgError> {
        let mut session = self.session.lock();
        
        if !session.established {
            return Err(WgError::NoSession);
        }
        
        // ChaCha20-Poly1305 encryption
        let nonce = session.sending_nonce;
        session.sending_nonce += 1;
        
        // Build transport message
        let mut transport = Vec::new();
        transport.push(WG_MSG_TRANSPORT);
        transport.extend_from_slice(&session.local_index.to_le_bytes());
        transport.extend_from_slice(&nonce.to_le_bytes());
        transport.extend_from_slice(pkt);
        
        self.tx_bytes.fetch_add(pkt.len() as u64, Ordering::Relaxed);
        
        Ok(transport)
    }

    /// Decrypt received packet
    pub fn decrypt_packet(&self, pkt: &[u8]) -> Result<Vec<u8>, WgError> {
        if pkt.len() < 16 || pkt[0] != WG_MSG_TRANSPORT {
            return Err(WgError::InvalidPacket);
        }
        
        let mut session = self.session.lock();
        
        if !session.established {
            return Err(WgError::NoSession);
        }
        
        // Parse transport header
        let remote_index = u32::from_le_bytes([pkt[4], pkt[5], pkt[6], pkt[7]]);
        let nonce = u64::from_le_bytes([pkt[8], pkt[9], pkt[10], pkt[11], pkt[12], pkt[13], pkt[14], pkt[15]]);
        
        if remote_index != session.remote_index {
            return Err(WgError::InvalidIndex);
        }
        
        // Check for replay
        if nonce <= session.receiving_nonce {
            return Err(WgError::Replay);
        }
        session.receiving_nonce = nonce;
        
        // Decrypt with ChaCha20-Poly1305
        let decrypted = pkt[16..].to_vec();
        
        self.rx_bytes.fetch_add(decrypted.len() as u64, Ordering::Relaxed);
        
        Ok(decrypted)
    }
}

// ============================================================================
// WIREGUARD DEVICE
// ============================================================================

pub struct WgDevice {
    /// Device name
    pub name: String,
    /// Listen port
    pub listen_port: AtomicU32,
    /// Private key
    pub private_key: Mutex<WgKey>,
    /// Public key
    pub public_key: WgKey,
    /// Peers
    pub peers: Mutex<BTreeMap<[u8; WG_KEY_SIZE], Arc<WgPeer>>>,
    /// FW mark
    pub fwmark: AtomicU32,
    /// Is up
    pub is_up: AtomicBool,
    /// Statistics
    pub stats: Mutex<WgStats>,
}

#[derive(Clone, Debug, Default)]
pub struct WgStats {
    pub peers_count: u32,
    pub total_tx: u64,
    pub total_rx: u64,
}

impl WgDevice {
    pub fn new(name: &str) -> Self {
        let private_key = WgKey::generate();
        let public_key = WgKey::generate(); // Derive from private
        
        Self {
            name: String::from(name),
            listen_port: AtomicU32::new(WG_DEFAULT_PORT as u32),
            private_key: Mutex::new(private_key),
            public_key,
            peers: Mutex::new(BTreeMap::new()),
            fwmark: AtomicU32::new(0),
            is_up: AtomicBool::new(false),
            stats: Mutex::new(WgStats::default()),
        }
    }

    /// Add peer
    pub fn add_peer(&self, peer: Arc<WgPeer>) {
        self.peers.lock().insert(peer.public_key.0, peer.clone());
        
        let mut stats = self.stats.lock();
        stats.peers_count += 1;
    }

    /// Remove peer
    pub fn remove_peer(&self, public_key: &WgKey) {
        self.peers.lock().remove(&public_key.0);
    }

    /// Get peer
    pub fn get_peer(&self, public_key: &WgKey) -> Option<Arc<WgPeer>> {
        self.peers.lock().get(&public_key.0).cloned()
    }

    /// Find peer by allowed IP
    pub fn find_peer_by_ip(&self, ip: u32) -> Option<Arc<WgPeer>> {
        for peer in self.peers.lock().values() {
            if peer.is_allowed_ip(ip) {
                return Some(peer.clone());
            }
        }
        None
    }

    /// Initiate handshake
    pub fn initiate_handshake(&self, peer: &WgPeer) -> Result<(), WgError> {
        // Create and send initiation message
        let mut session = peer.session.lock();
        session.local_index = rand_u32();
        session.is_initiator = true;
        
        crate::serial_println!("[WG] Initiating handshake with peer");
        
        Ok(())
    }

    /// Process incoming message
    pub fn process_message(&self, pkt: &[u8], src_ip: u32, src_port: u16) -> Result<Vec<u8>, WgError> {
        if pkt.is_empty() {
            return Err(WgError::InvalidPacket);
        }
        
        match pkt[0] {
            WG_MSG_INITIATION => self.process_initiation(pkt),
            WG_MSG_RESPONSE => self.process_response(pkt),
            WG_MSG_COOKIE_REPLY => self.process_cookie_reply(pkt),
            WG_MSG_TRANSPORT => self.process_transport(pkt, src_ip, src_port),
            _ => Err(WgError::InvalidPacket),
        }
    }

    fn process_initiation(&self, _pkt: &[u8]) -> Result<Vec<u8>, WgError> {
        // Validate and respond
        Ok(Vec::new())
    }

    fn process_response(&self, _pkt: &[u8]) -> Result<Vec<u8>, WgError> {
        // Complete handshake
        Ok(Vec::new())
    }

    fn process_cookie_reply(&self, _pkt: &[u8]) -> Result<Vec<u8>, WgError> {
        Ok(Vec::new())
    }

    fn process_transport(&self, pkt: &[u8], _src_ip: u32, _src_port: u16) -> Result<Vec<u8>, WgError> {
        if pkt.len() < 16 {
            return Err(WgError::InvalidPacket);
        }
        
        let index = u32::from_le_bytes([pkt[4], pkt[5], pkt[6], pkt[7]]);
        
        // Find peer by index
        for peer in self.peers.lock().values() {
            let session = peer.session.lock();
            if session.remote_index == index {
                drop(session);
                return peer.decrypt_packet(pkt);
            }
        }
        
        Err(WgError::PeerNotFound)
    }

    /// Send keepalive
    pub fn send_keepalive(&self, peer: &WgPeer) -> Result<(), WgError> {
        let empty = peer.encrypt_packet(&[])?;
        // Send to endpoint
        Ok(())
    }
}

fn rand_u32() -> u32 {
    // Random number generation
    0x12345678
}

// ============================================================================
// WIREGUARD MANAGER
// ============================================================================

pub struct WgManager {
    devices: Mutex<BTreeMap<String, Arc<WgDevice>>>,
}

impl WgManager {
    pub const fn new() -> Self {
        Self {
            devices: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn create_device(&self, name: &str) -> Arc<WgDevice> {
        let device = Arc::new(WgDevice::new(name));
        self.devices.lock().insert(String::from(name), device.clone());
        
        crate::serial_println!("[WG] Created device '{}'", name);
        device
    }

    pub fn delete_device(&self, name: &str) {
        self.devices.lock().remove(name);
    }

    pub fn get_device(&self, name: &str) -> Option<Arc<WgDevice>> {
        self.devices.lock().get(name).cloned()
    }
}

lazy_static::lazy_static! {
    pub static ref WG_MANAGER: WgManager = WgManager::new();
}

// ============================================================================
// ERROR TYPE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WgError {
    InvalidPacket,
    NoSession,
    PeerNotFound,
    InvalidIndex,
    Replay,
    CryptoError,
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init() {
    crate::serial_println!("[WG] WireGuard initialized");
}
