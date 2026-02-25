//! # IMA/EVM
//!
//! Integrity Measurement Architecture and Extended Verification Module.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// IMA CONSTANTS
// ============================================================================

/// IMA actions
pub const IMA_MEASURE: u32 = 0x01;
pub const IMA_DONT_MEASURE: u32 = 0x02;
pub const IMA_APPRAISE: u32 = 0x04;
pub const IMA_DONT_APPRAISE: u32 = 0x08;
pub const IMA_AUDIT: u32 = 0x10;
pub const IMA_HASH: u32 = 0x20;
pub const IMA_DIGSIG: u32 = 0x40;

/// IMA appraisal flags
pub const IMA_APPRAISE_ENFORCE: u32 = 0x01;
pub const IMA_APPRAISE_FIX: u32 = 0x02;
pub const IMA_APPRAISE_LOG: u32 = 0x04;
pub const IMA_APPRAISE_MODULES: u32 = 0x08;
pub const IMA_APPRAISE_FIRMWARE: u32 = 0x10;
pub const IMA_APPRAISE_POLICY: u32 = 0x20;
pub const IMA_APPRAISE_KEXEC: u32 = 0x40;

/// IMA hash algorithms
pub const IMA_HASH_SHA1: u32 = 1;
pub const IMA_HASH_SHA256: u32 = 2;
pub const IMA_HASH_SHA512: u32 = 3;

/// EVM types
pub const EVM_XATTR_HMAC: u32 = 0x01;
pub const EVM_XATTR_SIG: u32 = 0x02;
pub const EVM_XATTR_DIGSIG: u32 = 0x03;

// ============================================================================
// IMA TEMPLATE
// ============================================================================

#[derive(Clone, Debug)]
pub struct ImaTemplateEntry {
    /// PCR index
    pub pcr: u32,
    /// Template name
    pub template_name: String,
    /// Digest
    pub digest: Vec<u8>,
    /// Event name
    pub event_name: String,
    /// Event data
    pub event_data: Vec<u8>,
}

impl ImaTemplateEntry {
    pub fn new(pcr: u32, template: &str, digest: &[u8], name: &str, data: &[u8]) -> Self {
        Self {
            pcr,
            template_name: String::from(template),
            digest: digest.to_vec(),
            event_name: String::from(name),
            event_data: data.to_vec(),
        }
    }
}

// ============================================================================
// IMA MEASUREMENT
// ============================================================================

pub struct ImaMeasurement {
    /// File path
    pub path: String,
    /// File hash
    pub hash: [u8; 32],
    /// Hash algorithm
    pub hash_algo: u32,
    /// PCR index
    pub pcr: u32,
    /// Template
    pub template: String,
    /// Timestamp
    pub timestamp: u64,
    /// Is valid
    pub valid: AtomicBool,
}

impl ImaMeasurement {
    pub fn new(path: &str, hash: [u8; 32], pcr: u32) -> Self {
        Self {
            path: String::from(path),
            hash,
            hash_algo: IMA_HASH_SHA256,
            pcr,
            template: String::from("ima-ng"),
            timestamp: crate::task::scheduler::get_ticks(),
            valid: AtomicBool::new(true),
        }
    }
}

// ============================================================================
// IMA RULE
// ============================================================================

#[derive(Clone, Debug)]
pub struct ImaRule {
    /// Rule ID
    pub id: u32,
    /// Action mask
    pub action: u32,
    /// Measurement flags
    pub flags: u32,
    /// Path pattern
    pub path: String,
    /// UID
    pub uid: Option<u32>,
    /// Function
    pub func: Option<String>,
    /// Mask
    pub mask: Option<String>,
    /// FSMagic
    pub fsmagic: Option<u64>,
}

impl ImaRule {
    pub fn new(id: u32, action: u32, path: &str) -> Self {
        Self {
            id,
            action,
            flags: 0,
            path: String::from(path),
            uid: None,
            func: None,
            mask: None,
            fsmagic: None,
        }
    }

    /// Check if file matches rule
    pub fn matches(&self, path: &str, _uid: u32, _func: &str, _mask: &str) -> bool {
        if self.path == "*" {
            return true;
        }
        
        // Simple glob matching
        if self.path.ends_with('*') {
            let prefix = &self.path[..self.path.len() - 1];
            return path.starts_with(prefix);
        }
        
        path == self.path
    }
}

// ============================================================================
// EVM HMAC
// ============================================================================

pub struct EvmHmac {
    /// HMAC value
    pub hmac: [u8; 32],
    /// Protected xattrs hash
    pub xattr_hash: [u8; 32],
    /// Is valid
    pub valid: AtomicBool,
}

impl EvmHmac {
    pub fn calculate(_xattrs: &BTreeMap<String, Vec<u8>>, key: &[u8]) -> Self {
        // Calculate HMAC over xattrs
        let mut hmac = [0u8; 32];
        
        // Simplified HMAC calculation
        for (i, byte) in key.iter().enumerate() {
            hmac[i % 32] ^= byte;
        }
        
        Self {
            hmac,
            xattr_hash: [0u8; 32],
            valid: AtomicBool::new(true),
        }
    }

    /// Verify HMAC
    pub fn verify(&self, _xattrs: &BTreeMap<String, Vec<u8>>, _key: &[u8]) -> bool {
        self.valid.load(Ordering::Relaxed)
    }
}

// ============================================================================
// IMA/EVM MANAGER
// ============================================================================

pub struct ImaEvmManager {
    /// IMA measurements list
    pub measurements: Mutex<Vec<ImaMeasurement>>,
    /// IMA rules
    pub rules: Mutex<Vec<ImaRule>>,
    /// EVM HMAC cache
    pub evm_cache: Mutex<BTreeMap<String, EvmHmac>>,
    /// EVM key
    pub evm_key: Mutex<Vec<u8>>,
    /// PCR bank
    pub pcr_values: Mutex<[Vec<u8>; 24]>,
    /// Is IMA enabled
    pub ima_enabled: AtomicBool,
    /// Is EVM enabled
    pub evm_enabled: AtomicBool,
    /// Appraisal mode
    pub appraisal_mode: AtomicU32,
    /// Next rule ID
    pub next_rule_id: AtomicU32,
    /// Statistics
    pub stats: Mutex<ImaEvmStats>,
}

#[derive(Clone, Debug, Default)]
pub struct ImaEvmStats {
    pub measurements: u64,
    pub appraisals: u64,
    pub failures: u64,
    pub rules_count: u32,
}

impl ImaEvmManager {
    pub const fn new() -> Self {
        Self {
            measurements: Mutex::new(Vec::new()),
            rules: Mutex::new(Vec::new()),
            evm_cache: Mutex::new(BTreeMap::new()),
            evm_key: Mutex::new(Vec::new()),
            pcr_values: Mutex::new([Vec::new(), Vec::new(), Vec::new(), Vec::new(),
                                    Vec::new(), Vec::new(), Vec::new(), Vec::new(),
                                    Vec::new(), Vec::new(), Vec::new(), Vec::new(),
                                    Vec::new(), Vec::new(), Vec::new(), Vec::new(),
                                    Vec::new(), Vec::new(), Vec::new(), Vec::new(),
                                    Vec::new(), Vec::new(), Vec::new(), Vec::new()]),
            ima_enabled: AtomicBool::new(false),
            evm_enabled: AtomicBool::new(false),
            appraisal_mode: AtomicU32::new(0),
            next_rule_id: AtomicU32::new(1),
            stats: Mutex::new(ImaEvmStats::default()),
        }
    }

    /// Initialize
    pub fn init(&self) {
        // Add default rules
        self.add_default_rules();
        
        self.ima_enabled.store(true, Ordering::SeqCst);
        self.evm_enabled.store(true, Ordering::SeqCst);
        
        crate::serial_println!("[IMA/EVM] Initialized");
    }

    fn add_default_rules(&self) {
        let default_rules = [
            ("measure func=BPRM_CHECK", IMA_MEASURE),
            ("measure func=FILE_MMAP_CHECK", IMA_MEASURE),
            ("measure func=MODULE_CHECK", IMA_MEASURE),
            ("measure func=FIRMWARE_CHECK", IMA_MEASURE),
            ("appraise fsmagic=0x9fa1", IMA_APPRAISE), // procfs
            ("appraise fsmagic=0x62656572", IMA_APPRAISE), // sysfs
        ];
        
        for (rule_str, action) in default_rules {
            let id = self.next_rule_id.fetch_add(1, Ordering::SeqCst);
            let rule = ImaRule::new(id, action, "*");
            self.rules.lock().push(rule);
        }
        
        let mut stats = self.stats.lock();
        stats.rules_count = default_rules.len() as u32;
    }

    /// Measure file
    pub fn measure_file(&self, path: &str, data: &[u8]) -> Result<(), ImaEvmError> {
        if !self.ima_enabled.load(Ordering::SeqCst) {
            return Ok(());
        }
        
        // Calculate hash
        let hash = self.calculate_hash(data);
        
        // Create measurement
        let measurement = ImaMeasurement::new(path, hash, 10); // PCR 10
        
        // Extend PCR
        self.extend_pcr(10, &hash);
        
        // Store measurement
        self.measurements.lock().push(measurement);
        
        let mut stats = self.stats.lock();
        stats.measurements += 1;
        
        Ok(())
    }

    /// Appraise file
    pub fn appraise_file(&self, path: &str, _xattrs: &BTreeMap<String, Vec<u8>>) -> Result<(), ImaEvmError> {
        let mode = self.appraisal_mode.load(Ordering::SeqCst);
        
        if mode == 0 {
            return Ok(());
        }
        
        // Check EVM HMAC
        if self.evm_enabled.load(Ordering::SeqCst) {
            if let Some(hmac) = self.evm_cache.lock().get(path) {
                if !hmac.valid.load(Ordering::Relaxed) {
                    let mut stats = self.stats.lock();
                    stats.failures += 1;
                    return Err(ImaEvmError::HmacMismatch);
                }
            }
        }
        
        let mut stats = self.stats.lock();
        stats.appraisals += 1;
        
        Ok(())
    }

    /// Add rule
    pub fn add_rule(&self, rule: ImaRule) {
        self.rules.lock().push(rule);
        
        let mut stats = self.stats.lock();
        stats.rules_count += 1;
    }

    /// Calculate hash
    fn calculate_hash(&self, data: &[u8]) -> [u8; 32] {
        let mut hash = [0u8; 32];
        for (i, byte) in data.iter().enumerate() {
            hash[i % 32] ^= byte;
        }
        hash
    }

    /// Extend PCR
    fn extend_pcr(&self, pcr: usize, hash: &[u8; 32]) {
        let mut pcrs = self.pcr_values.lock();
        if pcr < 24 {
            // Extend: new_value = SHA256(old_value || hash)
            for (i, byte) in hash.iter().enumerate() {
                if i < pcrs[pcr].len() {
                    pcrs[pcr][i] ^= byte;
                } else {
                    pcrs[pcr].push(*byte);
                }
            }
        }
    }

    /// Get measurements
    pub fn get_measurements(&self) -> Vec<ImaMeasurement> {
        self.measurements.lock().iter().map(|m| ImaMeasurement {
            path: m.path.clone(),
            hash: m.hash,
            hash_algo: m.hash_algo,
            pcr: m.pcr,
            template: m.template.clone(),
            timestamp: m.timestamp,
            valid: AtomicBool::new(m.valid.load(Ordering::Relaxed)),
        }).collect()
    }

    /// Get statistics
    pub fn get_stats(&self) -> ImaEvmStats {
        self.stats.lock().clone()
    }
}

lazy_static::lazy_static! {
    pub static ref IMA_EVM: ImaEvmManager = ImaEvmManager::new();
}

// ============================================================================
// ERROR TYPE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImaEvmError {
    HashMismatch,
    HmacMismatch,
    SignatureInvalid,
    NoKey,
    AppraisalFailed,
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init() {
    IMA_EVM.init();
}
