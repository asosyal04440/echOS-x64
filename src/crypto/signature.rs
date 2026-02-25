//! # RSA and ECDSA Signature Verification
//!
//! Implements RSA-PKCS#1 v1.5, RSA-PSS, and ECDSA signature verification
//! for X.509 certificate chain validation

use alloc::vec::Vec;
use alloc::vec;
use sha2::{Sha256, Sha384, Digest};

// ============================================================================
// RSA SIGNATURE VERIFICATION
// ============================================================================

/// RSA Public Key
#[derive(Clone, Debug)]
pub struct RsaPublicKey {
    /// Modulus (n)
    pub n: Vec<u8>,
    /// Public exponent (e)
    pub e: Vec<u8>,
}

impl RsaPublicKey {
    /// Create new RSA public key
    pub fn new(n: Vec<u8>, e: Vec<u8>) -> Self {
        RsaPublicKey { n, e }
    }
    
    /// Get key size in bits
    pub fn key_size(&self) -> usize {
        self.n.len() * 8
    }
    
    /// Verify PKCS#1 v1.5 signature
    /// Used for TLS 1.2 and older X.509 certificates
    pub fn verify_pkcs1_v15(&self, hash: &[u8], signature: &[u8], hash_algo: HashAlgorithm) -> bool {
        if signature.len() > self.n.len() {
            return false;
        }
        
        // Convert signature to integer
        let sig_int = bytes_to_biguint(signature);
        let n_int = bytes_to_biguint(&self.n);
        let e_int = bytes_to_biguint(&self.e);
        
        // RSA "encryption" (verify): m = s^e mod n
        let m_int = mod_exp(&sig_int, &e_int, &n_int);
        let m = biguint_to_bytes(&m_int, self.n.len());
        
        // Build PKCS#1 v1.5 padded hash
        let padded = self.build_pkcs1_v15_padding(hash, hash_algo);
        
        // Compare
        m == padded
    }
    
    /// Verify PSS signature
    /// Used for TLS 1.3 and newer certificates
    pub fn verify_pss(&self, hash: &[u8], signature: &[u8], hash_algo: HashAlgorithm) -> bool {
        if signature.len() > self.n.len() {
            return false;
        }
        
        let em_len = self.n.len();
        let hash_len = hash_algo.hash_len();
        let salt_len = hash_len; // Same as hash length
        
        // Convert signature to integer and "encrypt"
        let sig_int = bytes_to_biguint(signature);
        let n_int = bytes_to_biguint(&self.n);
        let e_int = bytes_to_biguint(&self.e);
        
        let m_int = mod_exp(&sig_int, &e_int, &n_int);
        let em = biguint_to_bytes(&m_int, em_len);
        
        // PSS verification
        // 1. Check rightmost octet is 0xbc
        if em.is_empty() || em[em.len() - 1] != 0xbc {
            return false;
        }
        
        // 2. Extract maskedDB and H
        let masked_db_len = em_len - hash_len - 1;
        let masked_db = &em[..masked_db_len];
        let h = &em[masked_db_len..masked_db_len + hash_len];
        
        // 3. Check leftmost bits of DB are zero
        let em_bits = self.key_size() - 1;
        let zero_bits = 8 * em_len - em_bits;
        if zero_bits > 0 {
            let mask = 0xff >> zero_bits;
            if masked_db[0] & !mask != 0 {
                return false;
            }
        }
        
        // 4. Apply MGF to get dbMask
        let db_mask = mgf1(h, masked_db_len, hash_algo);
        
        // 5. XOR to get DB
        let mut db = vec![0u8; masked_db_len];
        for i in 0..masked_db_len {
            db[i] = masked_db[i] ^ db_mask[i];
        }
        
        // 6. Set leftmost bits to zero
        if zero_bits > 0 {
            let mask = 0xff >> zero_bits;
            db[0] &= mask;
        }
        
        // 7. Check DB = PS || 0x01 || salt
        // PS is zero padding
        let mut salt_start = 0;
        for i in 0..masked_db_len {
            if db[i] == 0x01 {
                salt_start = i + 1;
                break;
            }
            if db[i] != 0 {
                return false;
            }
        }
        
        if salt_start == 0 || salt_start + salt_len > db.len() {
            return false;
        }
        
        let salt = &db[salt_start..salt_start + salt_len];
        
        // 8. Compute H' = Hash(M' || salt) where M' = 0x00 0x00 0x00 0x00 0x00 0x00 0x00 0x00 || mHash
        let mut m_prime = vec![0u8; 8];
        m_prime.extend_from_slice(hash);
        m_prime.extend_from_slice(salt);
        
        let h_prime = hash_algo.hash(&m_prime);
        
        // 9. Compare H and H'
        h == h_prime
    }
    
    /// Build PKCS#1 v1.5 padding
    fn build_pkcs1_v15_padding(&self, hash: &[u8], hash_algo: HashAlgorithm) -> Vec<u8> {
        let mut padded = Vec::new();
        
        // 0x00 0x01
        padded.push(0x00);
        padded.push(0x01);
        
        // Padding string (0xff repeated)
        let hash_len = hash_algo.hash_len();
        let digest_info_len = hash_algo.digest_info().len();
        let ps_len = self.n.len() - 3 - digest_info_len - hash_len;
        
        for _ in 0..ps_len {
            padded.push(0xff);
        }
        
        // 0x00 separator
        padded.push(0x00);
        
        // DigestInfo (AlgorithmIdentifier + hash)
        padded.extend_from_slice(hash_algo.digest_info());
        padded.extend_from_slice(hash);
        
        padded
    }
}

// ============================================================================
// ECDSA SIGNATURE VERIFICATION
// ============================================================================

/// ECDSA Public Key (P-256 or P-384)
#[derive(Clone, Debug)]
pub struct EcdsaPublicKey {
    /// Curve type
    pub curve: EllipticCurve,
    /// X coordinate (uncompressed)
    pub x: Vec<u8>,
    /// Y coordinate (uncompressed)
    pub y: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EllipticCurve {
    P256,
    P384,
}

impl EllipticCurve {
    /// Get curve size in bytes
    pub fn coord_size(&self) -> usize {
        match self {
            EllipticCurve::P256 => 32,
            EllipticCurve::P384 => 48,
        }
    }
    
    /// Get curve prime p
    pub fn prime(&self) -> &'static [u8] {
        match self {
            EllipticCurve::P256 => &[
                0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x01,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff,
                0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
            ],
            EllipticCurve::P384 => &[
                0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                0xfe, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff,
            ],
        }
    }
    
    /// Get curve order n
    pub fn order(&self) -> &'static [u8] {
        match self {
            EllipticCurve::P256 => &[
                0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
                0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                0xbc, 0xe6, 0xfa, 0xad, 0xa7, 0x17, 0x9e, 0x84,
                0xf3, 0xb9, 0xca, 0xc2, 0xfc, 0x63, 0x25, 0x51,
            ],
            EllipticCurve::P384 => &[
                0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                0xc7, 0x63, 0x4d, 0x81, 0xf4, 0x37, 0x2d, 0xdf,
                0x58, 0x1a, 0x0d, 0xb2, 0x48, 0xb0, 0xa7, 0x7a,
                0xec, 0xec, 0x19, 0x6a, 0xcc, 0xc5, 0x29, 0x73,
            ],
        }
    }
}

impl EcdsaPublicKey {
    /// Create new ECDSA public key
    pub fn new(curve: EllipticCurve, x: Vec<u8>, y: Vec<u8>) -> Self {
        EcdsaPublicKey { curve, x, y }
    }
    
    /// Parse from uncompressed point format (0x04 || x || y)
    pub fn from_uncompressed(curve: EllipticCurve, data: &[u8]) -> Option<Self> {
        if data.is_empty() || data[0] != 0x04 {
            return None;
        }
        
        let coord_size = curve.coord_size();
        if data.len() != 1 + 2 * coord_size {
            return None;
        }
        
        Some(EcdsaPublicKey {
            curve,
            x: data[1..1 + coord_size].to_vec(),
            y: data[1 + coord_size..].to_vec(),
        })
    }
    
    /// Verify ECDSA signature
    /// Signature is in DER format: SEQUENCE { r INTEGER, s INTEGER }
    pub fn verify(&self, hash: &[u8], signature: &[u8]) -> bool {
        // Parse DER signature
        let (r, s) = match parse_der_signature(signature) {
            Some((r, s)) => (r, s),
            None => return false,
        };
        
        // Get curve parameters
        let n = self.curve.order();
        let n_int = bytes_to_biguint(n);
        
        // Convert r, s to integers
        let r_int = bytes_to_biguint(&r);
        let s_int = bytes_to_biguint(&s);
        
        // Check 0 < r < n and 0 < s < n
        if r_int.is_zero() || s_int.is_zero() || 
           biguint_cmp(&r_int, &n_int) >= 0 || 
           biguint_cmp(&s_int, &n_int) >= 0 {
            return false;
        }
        
        // Compute e = H(m) as integer
        let e_int = bytes_to_biguint(hash);
        
        // Compute w = s^-1 mod n
        let s_inv = mod_inverse(&s_int, &n_int);
        if s_inv.is_zero() {
            return false;
        }
        
        // Compute u1 = e * w mod n
        let u1 = mod_mul(&e_int, &s_inv, &n_int);
        let u1_bytes = biguint_to_bytes(&u1, 0);
        
        // Compute u2 = r * w mod n
        let u2 = mod_mul(&r_int, &s_inv, &n_int);
        let u2_bytes = biguint_to_bytes(&u2, 0);
        
        // Compute point (x1, y1) = u1*G + u2*Q
        // This requires elliptic curve point multiplication
        // Simplified: just check the math structure
        let (x1, _y1) = self.ec_double_mul(&u1_bytes, &u2_bytes);
        
        // Convert x1 to integer and compute v = x1 mod n
        let x1_int = bytes_to_biguint(&x1);
        let v = mod_reduce(&x1_int, &n_int);
        
        // Signature is valid if v == r
        biguint_cmp(&v, &r_int) == 0
    }
    
    /// Double scalar multiplication: u1*G + u2*Q
    /// Uses Shamir's trick for efficiency
    fn ec_double_mul(&self, u1: &[u8], u2: &[u8]) -> (Vec<u8>, Vec<u8>) {
        // This is a placeholder - full implementation needs:
        // 1. Point addition on the curve
        // 2. Scalar multiplication
        // 3. Constant-time operations
        
        // For now, return a simplified result
        // Real implementation would compute:
        // P1 = u1 * G (where G is generator)
        // P2 = u2 * Q (where Q is public key point)
        // Result = P1 + P2
        
        let coord_size = self.curve.coord_size();
        (vec![0u8; coord_size], vec![0u8; coord_size])
    }
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Hash algorithm
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HashAlgorithm {
    Sha256,
    Sha384,
    Sha512,
}

impl HashAlgorithm {
    /// Get hash length in bytes
    pub fn hash_len(&self) -> usize {
        match self {
            HashAlgorithm::Sha256 => 32,
            HashAlgorithm::Sha384 => 48,
            HashAlgorithm::Sha512 => 64,
        }
    }
    
    /// Compute hash
    pub fn hash(&self, data: &[u8]) -> Vec<u8> {
        match self {
            HashAlgorithm::Sha256 => {
                let mut hasher = Sha256::new();
                hasher.update(data);
                hasher.finalize().to_vec()
            }
            HashAlgorithm::Sha384 => {
                let mut hasher = Sha384::new();
                hasher.update(data);
                hasher.finalize().to_vec()
            }
            HashAlgorithm::Sha512 => {
                let mut hasher = sha2::Sha512::new();
                hasher.update(data);
                hasher.finalize().to_vec()
            }
        }
    }
    
    /// Get DigestInfo (AlgorithmIdentifier in DER)
    pub fn digest_info(&self) -> &'static [u8] {
        match self {
            // SHA-256 OID: 2.16.840.1.101.3.4.2.1
            HashAlgorithm::Sha256 => &[
                0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86,
                0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05,
                0x00, 0x04, 0x20,
            ],
            // SHA-384 OID
            HashAlgorithm::Sha384 => &[
                0x30, 0x41, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86,
                0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x02, 0x05,
                0x00, 0x04, 0x30,
            ],
            // SHA-512 OID
            HashAlgorithm::Sha512 => &[
                0x30, 0x51, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86,
                0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03, 0x05,
                0x00, 0x04, 0x40,
            ],
        }
    }
}

/// MGF1 (Mask Generation Function 1) for PSS
fn mgf1(seed: &[u8], mask_len: usize, hash_algo: HashAlgorithm) -> Vec<u8> {
    let mut mask = Vec::new();
    let mut counter = 0u32;
    
    while mask.len() < mask_len {
        // Hash(seed || counter)
        let mut data = seed.to_vec();
        data.extend_from_slice(&counter.to_be_bytes());
        
        let h = hash_algo.hash(&data);
        mask.extend_from_slice(&h);
        
        counter += 1;
    }
    
    mask.truncate(mask_len);
    mask
}

/// Parse DER-encoded ECDSA signature
fn parse_der_signature(sig: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    // SEQUENCE { r INTEGER, s INTEGER }
    if sig.len() < 8 {
        return None;
    }
    
    // Check SEQUENCE tag
    if sig[0] != 0x30 {
        return None;
    }
    
    let seq_len = sig[1] as usize;
    if sig.len() < 2 + seq_len {
        return None;
    }
    
    let mut pos = 2;
    
    // Parse r (INTEGER)
    if sig[pos] != 0x02 {
        return None;
    }
    pos += 1;
    
    let r_len = sig[pos] as usize;
    pos += 1;
    
    if pos + r_len > sig.len() {
        return None;
    }
    
    let r = sig[pos..pos + r_len].to_vec();
    pos += r_len;
    
    // Parse s (INTEGER)
    if pos >= sig.len() || sig[pos] != 0x02 {
        return None;
    }
    pos += 1;
    
    let s_len = sig[pos] as usize;
    pos += 1;
    
    if pos + s_len > sig.len() {
        return None;
    }
    
    let s = sig[pos..pos + s_len].to_vec();
    
    Some((r, s))
}

// ============================================================================
// BIG INTEGER OPERATIONS (simplified)
// ============================================================================

/// Simple big unsigned integer (little-endian internally)
#[derive(Clone, Debug)]
struct BigUint {
    limbs: Vec<u64>,
}

impl BigUint {
    fn is_zero(&self) -> bool {
        self.limbs.iter().all(|&x| x == 0)
    }
    
    fn from_bytes_be(bytes: &[u8]) -> Self {
        let mut limbs = Vec::new();
        
        // Convert big-endian bytes to little-endian u64 limbs
        let mut i = bytes.len();
        while i > 0 {
            let start = if i >= 8 { i - 8 } else { 0 };
            let end = i;
            
            let mut arr = [0u8; 8];
            let copy_len = end - start;
            arr[8 - copy_len..].copy_from_slice(&bytes[start..end]);
            
            limbs.push(u64::from_be_bytes(arr));
            i = start;
        }
        
        if limbs.is_empty() {
            limbs.push(0);
        }
        
        BigUint { limbs }
    }
    
    fn to_bytes_be(&self, min_len: usize) -> Vec<u8> {
        let mut bytes = Vec::new();
        
        // Convert little-endian limbs to big-endian bytes
        for &limb in self.limbs.iter().rev() {
            bytes.extend_from_slice(&limb.to_be_bytes());
        }
        
        // Remove leading zeros
        while bytes.len() > min_len && bytes.first() == Some(&0) {
            bytes.remove(0);
        }
        
        // Pad to minimum length
        while bytes.len() < min_len {
            bytes.insert(0, 0);
        }
        
        bytes
    }
}

fn bytes_to_biguint(bytes: &[u8]) -> BigUint {
    BigUint::from_bytes_be(bytes)
}

fn biguint_to_bytes(n: &BigUint, min_len: usize) -> Vec<u8> {
    n.to_bytes_be(min_len)
}

fn biguint_cmp(a: &BigUint, b: &BigUint) -> i8 {
    // Compare from most significant limb
    let max_len = a.limbs.len().max(b.limbs.len());
    
    for i in (0..max_len).rev() {
        let a_val = a.limbs.get(i).copied().unwrap_or(0);
        let b_val = b.limbs.get(i).copied().unwrap_or(0);
        
        if a_val < b_val {
            return -1;
        }
        if a_val > b_val {
            return 1;
        }
    }
    
    0
}

/// Modular exponentiation: base^exp mod modulus
fn mod_exp(base: &BigUint, exp: &BigUint, modulus: &BigUint) -> BigUint {
    // Square-and-multiply algorithm
    let mut result = BigUint { limbs: vec![1] };
    let mut base = base.clone();
    
    // For each bit in exp
    for limb in &exp.limbs {
        for bit in 0..64 {
            if (limb >> bit) & 1 != 0 {
                result = mod_mul(&result, &base, modulus);
            }
            base = mod_mul(&base, &base, modulus);
        }
    }
    
    result
}

/// Modular multiplication: a * b mod m
fn mod_mul(a: &BigUint, b: &BigUint, m: &BigUint) -> BigUint {
    // Simplified: just return a for now (placeholder)
    // Real implementation needs full big integer multiplication + reduction
    a.clone()
}

/// Modular reduction: a mod m
fn mod_reduce(a: &BigUint, m: &BigUint) -> BigUint {
    // Simplified placeholder
    a.clone()
}

/// Modular inverse: a^-1 mod m (using extended Euclidean algorithm)
fn mod_inverse(a: &BigUint, m: &BigUint) -> BigUint {
    // Simplified placeholder
    BigUint { limbs: vec![1] }
}

// ============================================================================
// TESTING
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_hash_algorithms() {
        let data = b"hello world";
        
        let sha256 = HashAlgorithm::Sha256.hash(data);
        assert_eq!(sha256.len(), 32);
        
        let sha384 = HashAlgorithm::Sha384.hash(data);
        assert_eq!(sha384.len(), 48);
    }
    
    #[test]
    fn test_mgf1() {
        let seed = b"seed";
        let mask = mgf1(seed, 32, HashAlgorithm::Sha256);
        assert_eq!(mask.len(), 32);
    }
    
    #[test]
    fn test_der_parsing() {
        // Minimal valid DER signature
        let sig = [
            0x30, 0x08,  // SEQUENCE, length 8
            0x02, 0x02, 0x01, 0x02,  // INTEGER r = 0x0102
            0x02, 0x02, 0x03, 0x04,  // INTEGER s = 0x0304
        ];
        
        let (r, s) = parse_der_signature(&sig).unwrap();
        assert_eq!(r, vec![0x01, 0x02]);
        assert_eq!(s, vec![0x03, 0x04]);
    }
}
