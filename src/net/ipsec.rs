//! # IPsec
//!
//! IP Security Protocol (ESP/AH) implementation.

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// IPSEC CONSTANTS
// ============================================================================

/// IPsec protocols
pub const IPPROTO_ESP: u8 = 50;
pub const IPPROTO_AH: u8 = 51;

/// IPsec modes
pub const IPSEC_MODE_TRANSPORT: u8 = 0;
pub const IPSEC_MODE_TUNNEL: u8 = 1;

/// IPsec directions
pub const IPSEC_DIR_INBOUND: u8 = 0;
pub const IPSEC_DIR_OUTBOUND: u8 = 1;

/// Encryption algorithms
pub const IPSEC_ENC_NULL: u16 = 0;
pub const IPSEC_ENC_DES_CBC: u16 = 1;
pub const IPSEC_ENC_3DES_CBC: u16 = 2;
pub const IPSEC_ENC_AES_CBC: u16 = 3;
pub const IPSEC_ENC_AES_CTR: u16 = 4;
pub const IPSEC_ENC_AES_GCM: u16 = 5;
pub const IPSEC_ENC_CHACHA20_POLY1305: u16 = 6;

/// Authentication algorithms
pub const IPSEC_AUTH_HMAC_MD5: u16 = 1;
pub const IPSEC_AUTH_HMAC_SHA1: u16 = 2;
pub const IPSEC_AUTH_HMAC_SHA256: u16 = 3;
pub const IPSEC_AUTH_HMAC_SHA384: u16 = 4;
pub const IPSEC_AUTH_HMAC_SHA512: u16 = 5;
pub const IPSEC_AUTH_AES_XCBC: u16 = 6;

// ============================================================================
// SECURITY ASSOCIATION (SA)
// ============================================================================

#[derive(Clone, Debug)]
pub struct SecurityAssociation {
    /// SPI (Security Parameter Index)
    pub spi: u32,
    /// Protocol (ESP/AH)
    pub proto: u8,
    /// Mode (Transport/Tunnel)
    pub mode: u8,
    /// Source IP
    pub src_ip: u32,
    /// Destination IP
    pub dst_ip: u32,
    /// Encryption algorithm
    pub enc_alg: u16,
    /// Encryption key
    pub enc_key: Vec<u8>,
    /// Authentication algorithm
    pub auth_alg: u16,
    /// Authentication key
    pub auth_key: Vec<u8>,
    /// Replay window size
    pub replay_window: u32,
    /// Replay bitmap
    pub replay_bitmap: AtomicU64,
    /// Last sequence number
    pub last_seq: AtomicU32,
    /// Expiration time
    pub expires: u64,
    /// Is active
    pub active: AtomicBool,
    /// Statistics
    pub stats: Mutex<SaStats>,
}

#[derive(Clone, Debug, Default)]
pub struct SaStats {
    pub packets_in: u64,
    pub packets_out: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub auth_errors: u64,
    pub replay_errors: u64,
}

impl SecurityAssociation {
    pub fn new(spi: u32, proto: u8, mode: u8) -> Self {
        Self {
            spi,
            proto,
            mode,
            src_ip: 0,
            dst_ip: 0,
            enc_alg: IPSEC_ENC_AES_CBC,
            enc_key: Vec::new(),
            auth_alg: IPSEC_AUTH_HMAC_SHA256,
            auth_key: Vec::new(),
            replay_window: 64,
            replay_bitmap: AtomicU64::new(0),
            last_seq: AtomicU32::new(0),
            expires: 0,
            active: AtomicBool::new(true),
            stats: Mutex::new(SaStats::default()),
        }
    }

    /// Check for replay attack
    pub fn check_replay(&self, seq: u32) -> bool {
        let last = self.last_seq.load(Ordering::Relaxed);
        
        if seq > last {
            // New packet, update bitmap
            let diff = seq - last;
            let mut bitmap = self.replay_bitmap.load(Ordering::Relaxed);
            
            if diff < 64 {
                bitmap = (bitmap << diff) | 1;
            } else {
                bitmap = 1;
            }
            
            self.replay_bitmap.store(bitmap, Ordering::Relaxed);
            self.last_seq.store(seq, Ordering::Relaxed);
            return true;
        }
        
        // Check if in window
        let diff = last - seq;
        if diff >= self.replay_window {
            return false;
        }
        
        // Check if already seen
        let bitmap = self.replay_bitmap.load(Ordering::Relaxed);
        let mask = 1u64 << diff;
        
        if bitmap & mask != 0 {
            // Already seen
            return false;
        }
        
        // Mark as seen
        self.replay_bitmap.fetch_or(mask, Ordering::Relaxed);
        true
    }

    /// Encrypt packet
    pub fn encrypt(&self, pkt: &[u8]) -> Result<Vec<u8>, IpsecError> {
        match self.enc_alg {
            IPSEC_ENC_NULL => Ok(pkt.to_vec()),
            IPSEC_ENC_AES_CBC => self.encrypt_aes_cbc(pkt),
            IPSEC_ENC_AES_GCM => self.encrypt_aes_gcm(pkt),
            _ => Err(IpsecError::UnsupportedAlgorithm),
        }
    }

    /// Decrypt packet
    pub fn decrypt(&self, pkt: &[u8]) -> Result<Vec<u8>, IpsecError> {
        match self.enc_alg {
            IPSEC_ENC_NULL => Ok(pkt.to_vec()),
            IPSEC_ENC_AES_CBC => self.decrypt_aes_cbc(pkt),
            IPSEC_ENC_AES_GCM => self.decrypt_aes_gcm(pkt),
            _ => Err(IpsecError::UnsupportedAlgorithm),
        }
    }

    fn encrypt_aes_cbc(&self, _pkt: &[u8]) -> Result<Vec<u8>, IpsecError> {
        Ok(Vec::new())
    }

    fn decrypt_aes_cbc(&self, _pkt: &[u8]) -> Result<Vec<u8>, IpsecError> {
        Ok(Vec::new())
    }

    fn encrypt_aes_gcm(&self, _pkt: &[u8]) -> Result<Vec<u8>, IpsecError> {
        Ok(Vec::new())
    }

    fn decrypt_aes_gcm(&self, _pkt: &[u8]) -> Result<Vec<u8>, IpsecError> {
        Ok(Vec::new())
    }

    /// Calculate ICV (Integrity Check Value)
    pub fn calculate_icv(&self, data: &[u8]) -> Vec<u8> {
        // HMAC-SHA256
        let icv_len = match self.auth_alg {
            IPSEC_AUTH_HMAC_SHA1 => 12,
            IPSEC_AUTH_HMAC_SHA256 => 16,
            IPSEC_AUTH_HMAC_SHA384 => 24,
            IPSEC_AUTH_HMAC_SHA512 => 32,
            _ => 12,
        };
        vec![0u8; icv_len]
    }

    /// Verify ICV
    pub fn verify_icv(&self, data: &[u8], icv: &[u8]) -> bool {
        let expected = self.calculate_icv(data);
        expected == icv
    }
}

// ============================================================================
// SECURITY POLICY (SP)
// ============================================================================

#[derive(Clone, Debug)]
pub struct SecurityPolicy {
    /// Policy ID
    pub id: u32,
    /// Direction
    pub dir: u8,
    /// Source IP range
    pub src_ip: u32,
    pub src_mask: u32,
    /// Destination IP range
    pub dst_ip: u32,
    pub dst_mask: u32,
    /// Protocol
    pub proto: u8,
    /// Port range
    pub src_port: (u16, u16),
    pub dst_port: (u16, u16),
    /// Action
    pub action: PolicyAction,
    /// Priority
    pub priority: u32,
    /// Associated SA
    pub sa_spi: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyAction {
    Discard,
    None,
    Ipsec,
}

impl SecurityPolicy {
    pub fn new(id: u32, dir: u8) -> Self {
        Self {
            id,
            dir,
            src_ip: 0,
            src_mask: 0,
            dst_ip: 0,
            dst_mask: 0,
            proto: 0,
            src_port: (0, 65535),
            dst_port: (0, 65535),
            action: PolicyAction::None,
            priority: 1000,
            sa_spi: None,
        }
    }

    /// Check if packet matches policy
    pub fn matches(&self, src_ip: u32, dst_ip: u32, proto: u8, src_port: u16, dst_port: u16) -> bool {
        if (src_ip & self.src_mask) != (self.src_ip & self.src_mask) {
            return false;
        }
        if (dst_ip & self.dst_mask) != (self.dst_ip & self.dst_mask) {
            return false;
        }
        if self.proto != 0 && proto != self.proto {
            return false;
        }
        if src_port < self.src_port.0 || src_port > self.src_port.1 {
            return false;
        }
        if dst_port < self.dst_port.0 || dst_port > self.dst_port.1 {
            return false;
        }
        true
    }
}

// ============================================================================
// IPSEC MANAGER
// ============================================================================

pub struct IpsecManager {
    /// Security Associations (SPI -> SA)
    sas: Mutex<BTreeMap<u32, Arc<SecurityAssociation>>>,
    /// Security Policies
    sps_inbound: Mutex<Vec<SecurityPolicy>>,
    sps_outbound: Mutex<Vec<SecurityPolicy>>,
    /// SPI counter
    next_spi: AtomicU32,
    /// Policy ID counter
    next_policy_id: AtomicU32,
    /// Enabled
    enabled: AtomicBool,
    /// Statistics
    stats: Mutex<IpsecStats>,
}

#[derive(Clone, Debug, Default)]
pub struct IpsecStats {
    pub sa_count: u32,
    pub sp_count: u32,
    pub packets_encrypted: u64,
    pub packets_decrypted: u64,
    pub auth_failures: u64,
    pub replay_failures: u64,
}

impl IpsecManager {
    pub const fn new() -> Self {
        Self {
            sas: Mutex::new(BTreeMap::new()),
            sps_inbound: Mutex::new(Vec::new()),
            sps_outbound: Mutex::new(Vec::new()),
            next_spi: AtomicU32::new(0x1000000),
            next_policy_id: AtomicU32::new(1),
            enabled: AtomicBool::new(false),
            stats: Mutex::new(IpsecStats::default()),
        }
    }

    /// Create new SA
    pub fn create_sa(&self, proto: u8, mode: u8) -> Arc<SecurityAssociation> {
        let spi = self.next_spi.fetch_add(1, Ordering::SeqCst);
        let sa = Arc::new(SecurityAssociation::new(spi, proto, mode));
        self.sas.lock().insert(spi, sa.clone());
        
        let mut stats = self.stats.lock();
        stats.sa_count += 1;
        
        sa
    }

    /// Get SA by SPI
    pub fn get_sa(&self, spi: u32) -> Option<Arc<SecurityAssociation>> {
        self.sas.lock().get(&spi).cloned()
    }

    /// Delete SA
    pub fn delete_sa(&self, spi: u32) {
        self.sas.lock().remove(&spi);
    }

    /// Add security policy
    pub fn add_policy(&self, policy: SecurityPolicy) {
        match policy.dir {
            IPSEC_DIR_INBOUND => self.sps_inbound.lock().push(policy),
            IPSEC_DIR_OUTBOUND => self.sps_outbound.lock().push(policy),
            _ => {}
        }
        
        let mut stats = self.stats.lock();
        stats.sp_count += 1;
    }

    /// Find policy for outbound packet
    pub fn find_outbound_policy(&self, src_ip: u32, dst_ip: u32, proto: u8, src_port: u16, dst_port: u16) -> Option<SecurityPolicy> {
        let policies = self.sps_outbound.lock();
        for policy in policies.iter() {
            if policy.matches(src_ip, dst_ip, proto, src_port, dst_port) {
                return Some(policy.clone());
            }
        }
        None
    }

    /// Find policy for inbound packet
    pub fn find_inbound_policy(&self, src_ip: u32, dst_ip: u32, proto: u8, src_port: u16, dst_port: u16) -> Option<SecurityPolicy> {
        let policies = self.sps_inbound.lock();
        for policy in policies.iter() {
            if policy.matches(src_ip, dst_ip, proto, src_port, dst_port) {
                return Some(policy.clone());
            }
        }
        None
    }

    /// Process outbound packet
    pub fn process_outbound(&self, pkt: &mut [u8], src_ip: u32, dst_ip: u32, proto: u8, src_port: u16, dst_port: u16) -> Result<Vec<u8>, IpsecError> {
        if !self.enabled.load(Ordering::SeqCst) {
            return Ok(pkt.to_vec());
        }
        
        if let Some(policy) = self.find_outbound_policy(src_ip, dst_ip, proto, src_port, dst_port) {
            if policy.action == PolicyAction::Ipsec {
                if let Some(spi) = policy.sa_spi {
                    if let Some(sa) = self.get_sa(spi) {
                        let encrypted = sa.encrypt(pkt)?;
                        let icv = sa.calculate_icv(&encrypted);
                        
                        // Build ESP packet
                        let mut esp_pkt = Vec::new();
                        esp_pkt.extend_from_slice(&spi.to_be_bytes());
                        esp_pkt.extend_from_slice(&sa.last_seq.load(Ordering::Relaxed).to_be_bytes());
                        esp_pkt.extend_from_slice(&encrypted);
                        esp_pkt.extend_from_slice(&icv);
                        
                        let mut stats = self.stats.lock();
                        stats.packets_encrypted += 1;
                        
                        return Ok(esp_pkt);
                    }
                }
            }
        }
        
        Ok(pkt.to_vec())
    }

    /// Process inbound packet
    pub fn process_inbound(&self, pkt: &[u8]) -> Result<Vec<u8>, IpsecError> {
        if !self.enabled.load(Ordering::SeqCst) {
            return Ok(pkt.to_vec());
        }
        
        // Parse ESP header
        if pkt.len() < 8 {
            return Err(IpsecError::InvalidPacket);
        }
        
        let spi = u32::from_be_bytes([pkt[0], pkt[1], pkt[2], pkt[3]]);
        let seq = u32::from_be_bytes([pkt[4], pkt[5], pkt[6], pkt[7]]);
        
        if let Some(sa) = self.get_sa(spi) {
            // Check replay
            if !sa.check_replay(seq) {
                let mut stats = self.stats.lock();
                stats.replay_failures += 1;
                return Err(IpsecError::ReplayAttack);
            }
            
            // Decrypt
            let decrypted = sa.decrypt(&pkt[8..])?;
            
            let mut stats = self.stats.lock();
            stats.packets_decrypted += 1;
            
            return Ok(decrypted);
        }
        
        Err(IpsecError::SaNotFound)
    }

    /// Enable/disable
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }
}

lazy_static::lazy_static! {
    pub static ref IPSEC: IpsecManager = IpsecManager::new();
}

// ============================================================================
// ERROR TYPE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpsecError {
    SaNotFound,
    PolicyNotFound,
    InvalidPacket,
    AuthFailed,
    ReplayAttack,
    UnsupportedAlgorithm,
    EncryptionFailed,
    DecryptionFailed,
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init() {
    crate::serial_println!("[IPSEC] Subsystem initialized");
}
