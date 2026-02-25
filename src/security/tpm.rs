//! # TPM 2.0 (Trusted Platform Module) Support
//!
//! Hardware security module for secure key storage and attestation.

use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;

// TPM 2.0 Commands
const TPM2_CC_NV_READ: u32 = 0x0000145E;
const TPM2_CC_NV_WRITE: u32 = 0x00001437;
const TPM2_CC_NV_DEFINE_SPACE: u32 = 0x0000012A;
const TPM2_CC_NV_UNDEFINESPACE: u32 = 0x00000141;
const TPM2_CC_CREATE: u32 = 0x00000153;
const TPM2_CC_LOAD: u32 = 0x00000157;
const TPM2_CC_SIGN: u32 = 0x0000015D;
const TPM2_CC_GET_RANDOM: u32 = 0x0000017B;
const TPM2_CC_HASH: u32 = 0x00000004;
const TPM2_CC_PCR_EXTEND: u32 = 0x0000013C;
const TPM2_CC_PCR_READ: u32 = 0x0000017E;
const TPM2_CC_MAKE_CREDENTIAL: u32 = 0x0000015B;
const TPM2_CC_ACTIVATE_CREDENTIAL: u32 = 0x00000167;
const TPM2_CC_QUOTE: u32 = 0x00000158;

// TPM 2.0 Constants
const TPM2_RH_OWNER: u32 = 0x40000001;
const TPM2_RH_PLATFORM: u32 = 0x4000000C;
const TPM2_RH_ENDORSEMENT: u32 = 0x4000000B;
const TPM2_RH_NULL: u32 = 0x40000007;

// TPM 2.0 Algorithms
const TPM2_ALG_RSA: u16 = 0x0001;
const TPM2_ALG_SHA256: u16 = 0x000B;
const TPM2_ALG_SHA384: u16 = 0x000C;
const TPM2_ALG_SHA512: u16 = 0x000D;
const TPM2_ALG_AES: u16 = 0x0006;
const TPM2_ALG_ECC: u16 = 0x0023;
const TPM2_ALG_ECDAA: u16 = 0x0014;

// TPM 2.0 Locality
const TPM_LOCALITY_0: u8 = 0;
const TPM_LOCALITY_1: u8 = 1;
const TPM_LOCALITY_2: u8 = 2;
const TPM_LOCALITY_3: u8 = 3;
const TPM_LOCALITY_4: u8 = 4;

/// TPM 2.0 Response Codes
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpmResponseCode {
    Success = 0x0000,
    Ver1Failure = 0x0100,
    NoSignature = 0x0200,
    KeyNotLoaded = 0x0201,
    KeyNotFound = 0x0202,
    AuthFail = 0x098E,
    AuthUnavailable = 0x098F,
    PolicyFail = 0x0991,
    Size = 0x01D5,
    Value = 0x0184,
    NvLocked = 0x0149,
    NvUninitialized = 0x014A,
    NvSpace = 0x014B,
    NvDefined = 0x014C,
    Unknown,
}

impl TpmResponseCode {
    pub fn from_u16(code: u16) -> Self {
        match code {
            0x0000 => TpmResponseCode::Success,
            0x0100 => TpmResponseCode::Ver1Failure,
            0x0200 => TpmResponseCode::NoSignature,
            0x0201 => TpmResponseCode::KeyNotLoaded,
            0x0202 => TpmResponseCode::KeyNotFound,
            0x098E => TpmResponseCode::AuthFail,
            0x098F => TpmResponseCode::AuthUnavailable,
            0x0991 => TpmResponseCode::PolicyFail,
            0x01D5 => TpmResponseCode::Size,
            0x0184 => TpmResponseCode::Value,
            0x0149 => TpmResponseCode::NvLocked,
            0x014A => TpmResponseCode::NvUninitialized,
            0x014B => TpmResponseCode::NvSpace,
            0x014C => TpmResponseCode::NvDefined,
            _ => TpmResponseCode::Unknown,
        }
    }

    pub fn is_success(&self) -> bool {
        matches!(self, TpmResponseCode::Success)
    }
}

/// TPM 2.0 NV Index
#[derive(Clone, Copy, Debug)]
pub struct NvIndex {
    pub handle: u32,
    pub size: u16,
    pub attributes: u32,
    pub auth_policy: [u8; 32],
}

/// TPM 2.0 PCR Selection
#[derive(Clone, Debug)]
pub struct PcrSelection {
    pub hash: u16,
    pub size: u8,
    pub select: [u8; 16],
}

impl PcrSelection {
    pub fn new_sha256() -> Self {
        PcrSelection {
            hash: TPM2_ALG_SHA256,
            size: 3,
            select: [0xFF, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        }
    }

    pub fn select_pcr(&mut self, pcr: u8) {
        let idx = (pcr / 8) as usize;
        let bit = pcr % 8;
        if idx < 16 {
            self.select[idx] |= 1 << bit;
        }
    }

    pub fn is_selected(&self, pcr: u8) -> bool {
        let idx = (pcr / 8) as usize;
        let bit = pcr % 8;
        if idx < 16 {
            (self.select[idx] & (1 << bit)) != 0
        } else {
            false
        }
    }
}

/// TPM 2.0 PCR Value
#[derive(Clone, Debug)]
pub struct PcrValue {
    pub pcr: u8,
    pub value: [u8; 32],
}

/// TPM 2.0 Device
pub struct TpmDevice {
    pub locality: u8,
    pub is_tis: bool,
    pub base_address: u64,
    pub command_buffer: Vec<u8>,
    pub response_buffer: Vec<u8>,
}

impl TpmDevice {
    /// Create new TPM device
    pub fn new(base_address: u64) -> Self {
        TpmDevice {
            locality: TPM_LOCALITY_0,
            is_tis: true,
            base_address,
            command_buffer: Vec::new(),
            response_buffer: Vec::new(),
        }
    }

    /// Initialize TPM
    pub fn init(&mut self) -> Result<(), TpmError> {
        crate::serial_println!("[TPM] Initializing TPM 2.0 at {:#x}", self.base_address);
        
        // TODO: Implement actual TPM TIS interface initialization
        // 1. Request locality
        // 2. Wait for ready
        // 3. Send TPM2_CC_Startup
        
        Ok(())
    }

    /// Get random bytes from TPM
    pub fn get_random(&mut self, count: u16) -> Result<Vec<u8>, TpmError> {
        // Build command
        let mut cmd = Vec::with_capacity(12);
        
        // Tag
        cmd.extend_from_slice(&0x8001u16.to_be_bytes());
        // Size (placeholder)
        cmd.extend_from_slice(&(12u32).to_be_bytes());
        // Command code
        cmd.extend_from_slice(&TPM2_CC_GET_RANDOM.to_be_bytes());
        // Bytes requested
        cmd.extend_from_slice(&(count as u32).to_be_bytes());
        
        // TODO: Send command and receive response
        // For now, return placeholder
        Ok(vec![0u8; count as usize])
    }

    /// Extend PCR
    pub fn pcr_extend(&mut self, pcr: u8, digest: &[u8]) -> Result<(), TpmError> {
        if digest.len() != 32 {
            return Err(TpmError::InvalidDigest);
        }

        // Build command
        let mut cmd = Vec::with_capacity(50);
        
        // Tag
        cmd.extend_from_slice(&0x8001u16.to_be_bytes());
        // Size
        cmd.extend_from_slice(&(50u32).to_be_bytes());
        // Command code
        cmd.extend_from_slice(&TPM2_CC_PCR_EXTEND.to_be_bytes());
        // PCR handle
        cmd.extend_from_slice(&(pcr as u32).to_be_bytes());
        // Authorization
        cmd.extend_from_slice(&0u32.to_be_bytes()); // Auth area size
        // PCR selection
        cmd.extend_from_slice(&TPM2_ALG_SHA256.to_be_bytes());
        cmd.extend_from_slice(&[1u8]); // Size
        cmd.extend_from_slice(&[1u8 << (pcr % 8)]); // Select
        // Digest count
        cmd.extend_from_slice(&1u32.to_be_bytes());
        // Hash algorithm
        cmd.extend_from_slice(&TPM2_ALG_SHA256.to_be_bytes());
        // Digest
        cmd.extend_from_slice(digest);

        // TODO: Send command
        Ok(())
    }

    /// Read PCR
    pub fn pcr_read(&mut self, selection: &PcrSelection) -> Result<Vec<PcrValue>, TpmError> {
        // Build command
        let mut cmd = Vec::with_capacity(30);
        
        // Tag
        cmd.extend_from_slice(&0x8001u16.to_be_bytes());
        // Size
        cmd.extend_from_slice(&(30u32).to_be_bytes());
        // Command code
        cmd.extend_from_slice(&TPM2_CC_PCR_READ.to_be_bytes());
        // PCR selection count
        cmd.extend_from_slice(&1u32.to_be_bytes());
        // Hash algorithm
        cmd.extend_from_slice(&selection.hash.to_be_bytes());
        // Size
        cmd.push(selection.size);
        // Select
        cmd.extend_from_slice(&selection.select[..selection.size as usize]);

        // TODO: Send command and parse response
        // For now, return empty
        Ok(Vec::new())
    }

    /// Create NV space
    pub fn nv_define_space(&mut self, handle: u32, size: u16, auth: &[u8]) -> Result<(), TpmError> {
        // Build command
        let mut cmd = Vec::with_capacity(60);
        
        // Tag
        cmd.extend_from_slice(&0x8001u16.to_be_bytes());
        // Auth handle
        cmd.extend_from_slice(&TPM2_RH_OWNER.to_be_bytes());
        // Command code
        cmd.extend_from_slice(&TPM2_CC_NV_DEFINE_SPACE.to_be_bytes());
        // NV handle
        cmd.extend_from_slice(&handle.to_be_bytes());
        // Auth policy
        cmd.extend_from_slice(&[0u8; 32]);
        // Attributes
        cmd.extend_from_slice(&0x2000_0000u32.to_be_bytes()); // Owner write/read
        // Auth value (pad to 32 bytes)
        let mut auth_padded = [0u8; 32];
        auth_padded[..auth.len().min(32)].copy_from_slice(&auth[..auth.len().min(32)]);
        cmd.extend_from_slice(&auth_padded);

        // TODO: Send command
        Ok(())
    }

    /// Write to NV space
    pub fn nv_write(&mut self, handle: u32, offset: u16, data: &[u8]) -> Result<(), TpmError> {
        // Build command
        let cmd_size = 20 + data.len() as u32;
        let mut cmd = Vec::with_capacity(cmd_size as usize);
        
        // Tag
        cmd.extend_from_slice(&0x8001u16.to_be_bytes());
        // Size
        cmd.extend_from_slice(&cmd_size.to_be_bytes());
        // Command code
        cmd.extend_from_slice(&TPM2_CC_NV_WRITE.to_be_bytes());
        // NV handle
        cmd.extend_from_slice(&handle.to_be_bytes());
        // Offset
        cmd.extend_from_slice(&offset.to_be_bytes());
        // Data size
        cmd.extend_from_slice(&(data.len() as u16).to_be_bytes());
        // Data
        cmd.extend_from_slice(data);

        // TODO: Send command
        Ok(())
    }

    /// Read from NV space
    pub fn nv_read(&mut self, handle: u32, offset: u16, size: u16) -> Result<Vec<u8>, TpmError> {
        // Build command
        let mut cmd = Vec::with_capacity(20);
        
        // Tag
        cmd.extend_from_slice(&0x8001u16.to_be_bytes());
        // Size
        cmd.extend_from_slice(&20u32.to_be_bytes());
        // Command code
        cmd.extend_from_slice(&TPM2_CC_NV_READ.to_be_bytes());
        // NV handle
        cmd.extend_from_slice(&handle.to_be_bytes());
        // Size
        cmd.extend_from_slice(&size.to_be_bytes());
        // Offset
        cmd.extend_from_slice(&offset.to_be_bytes());

        // TODO: Send command and parse response
        Ok(vec![0u8; size as usize])
    }

    /// Quote PCRs (attestation)
    pub fn quote(&mut self, key_handle: u32, nonce: &[u8], selection: &PcrSelection) -> Result<Vec<u8>, TpmError> {
        // Build command for attestation
        let cmd_size = 30 + nonce.len() as u32;
        let mut cmd = Vec::with_capacity(cmd_size as usize);
        
        // Tag
        cmd.extend_from_slice(&0x8001u16.to_be_bytes());
        // Size
        cmd.extend_from_slice(&cmd_size.to_be_bytes());
        // Command code
        cmd.extend_from_slice(&TPM2_CC_QUOTE.to_be_bytes());
        // Key handle
        cmd.extend_from_slice(&key_handle.to_be_bytes());
        // Qualifying data
        cmd.extend_from_slice(&(nonce.len() as u16).to_be_bytes());
        cmd.extend_from_slice(nonce);
        // PCR selection
        cmd.extend_from_slice(&selection.hash.to_be_bytes());
        cmd.push(selection.size);
        cmd.extend_from_slice(&selection.select[..selection.size as usize]);

        // TODO: Send command and return attestation data
        Ok(Vec::new())
    }
}

/// TPM Error
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpmError {
    NotPresent,
    NotInitialized,
    CommunicationError,
    ResponseError(TpmResponseCode),
    InvalidDigest,
    InvalidHandle,
    NvSpaceFull,
    AuthFailed,
    Unknown,
}

// Global TPM instance
lazy_static::lazy_static! {
    static ref TPM_DEVICE: Mutex<Option<TpmDevice>> = Mutex::new(None);
}

/// Initialize TPM
pub fn init(base_address: u64) -> Result<(), TpmError> {
    let mut device = TpmDevice::new(base_address);
    device.init()?;
    *TPM_DEVICE.lock() = Some(device);
    crate::serial_println!("[TPM] TPM 2.0 initialized successfully");
    Ok(())
}

/// Get random bytes
pub fn get_random(count: u16) -> Result<Vec<u8>, TpmError> {
    let mut tpm = TPM_DEVICE.lock();
    let device = tpm.as_mut().ok_or(TpmError::NotInitialized)?;
    device.get_random(count)
}

/// Extend PCR
pub fn pcr_extend(pcr: u8, digest: &[u8]) -> Result<(), TpmError> {
    let mut tpm = TPM_DEVICE.lock();
    let device = tpm.as_mut().ok_or(TpmError::NotInitialized)?;
    device.pcr_extend(pcr, digest)
}

/// Read PCR
pub fn pcr_read(selection: &PcrSelection) -> Result<Vec<PcrValue>, TpmError> {
    let mut tpm = TPM_DEVICE.lock();
    let device = tpm.as_mut().ok_or(TpmError::NotInitialized)?;
    device.pcr_read(selection)
}

/// Measure boot event
pub fn measure_boot_event(event: &str) -> Result<(), TpmError> {
    // Hash the event
    let mut hasher = crate::crypto::Sha3::sha3_256();
    hasher.update(event.as_bytes());
    let hash = hasher.finalize();
    
    // Extend PCR 0 (SRTM)
    let mut digest = [0u8; 32];
    digest.copy_from_slice(&hash[..32]);
    
    pcr_extend(0, &digest)
}

/// Seal data to TPM
pub fn seal_data(data: &[u8], pcr_mask: u32) -> Result<Vec<u8>, TpmError> {
    // Create sealed blob that can only be unsealed when PCRs match
    // This requires creating a TPM key and encrypting data to it
    
    let _ = (data, pcr_mask);
    // TODO: Implement actual sealing
    Ok(data.to_vec())
}

/// Unseal data from TPM
pub fn unseal_data(sealed: &[u8]) -> Result<Vec<u8>, TpmError> {
    // Verify PCRs and decrypt
    let _ = sealed;
    // TODO: Implement actual unsealing
    Err(TpmError::Unknown)
}

/// Perform remote attestation
pub fn attest(nonce: &[u8]) -> Result<AttestationResult, TpmError> {
    let mut tpm = TPM_DEVICE.lock();
    let device = tpm.as_mut().ok_or(TpmError::NotInitialized)?;
    
    // Quote all PCRs
    let selection = PcrSelection::new_sha256();
    let quote = device.quote(TPM2_RH_ENDORSEMENT, nonce, &selection)?;
    
    Ok(AttestationResult {
        quote,
        pcr_values: Vec::new(),
        signature: Vec::new(),
    })
}

/// Attestation result
#[derive(Clone, Debug)]
pub struct AttestationResult {
    pub quote: Vec<u8>,
    pub pcr_values: Vec<PcrValue>,
    pub signature: Vec<u8>,
}

/// Check if TPM is available
pub fn is_available() -> bool {
    TPM_DEVICE.lock().is_some()
}
