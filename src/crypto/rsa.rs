//! # RSA (Rivest-Shamir-Adleman) Digital Signature
//!
//! RSA-PKCS#1 v1.5 signature scheme (RFC 8017).
//! SHA-256 ve SHA-512 hash fonksiyonları ile kullanım.
//!
//! ## RSA Anahtar Üretimi
//!
//! ```text
//! 1. İki büyük asal sayı üret: p, q (her biri 1024-bit için toplam 2048-bit)
//! 2. Modulus: n = p * q
//! 3. Euler's totient: φ(n) = (p-1)*(q-1)
//! 4. Public exponent: e = 65537 (standart)
//! 5. Private exponent: d = e^(-1) mod φ(n)
//! ```
//!
//! ## RSA İmzalama (PKCS#1 v1.5)
//!
//! ```text
//! Input: message m, private key (d, n)
//! Output: signature s
//!
//! 1. Hash h = SHA-256(m)
//! 2. DigestInfo oluştur (ASN.1 DER encoded)
//! 3. Padding: 0x00 || 0x01 || 0xFF...FF || 0x00 || DigestInfo
//! 4. Integer conversion: m_int = bytes_to_int(padded)
//! 5. Signature: s = m_int^d mod n
//! ```
//!
//! ## RSA Doğrulama
//!
//! ```text
//! Input: message m, signature s, public key (e, n)
//! Output: true/false
//!
//! 1. Integer conversion: m_int = s^e mod n
//! 2. Convert to bytes
//! 3. Verify padding: 0x00 || 0x01 || 0xFF...FF || 0x00 || DigestInfo
//! 4. Extract hash from DigestInfo
//! 5. Hash message: h' = SHA-256(m)
//! 6. Return (h == h')
//! ```

use crate::crypto::rdrand_bytes;
use alloc::vec;
use alloc::vec::Vec;
use rsa::rand_core::{CryptoRng, Error as RandError, RngCore};
use rsa::{
    BigUint as ExternalBigUint, Pkcs1v15Sign, RsaPrivateKey as ExternalRsaPrivateKey,
    RsaPublicKey as ExternalRsaPublicKey,
};
use sha1::{Digest as Sha1Digest, Sha1};
use sha2::{Sha256, Sha512};

const RSA_SHA1_DIGESTINFO_PREFIX: &[u8] = &[
    0x30, 0x21, 0x30, 0x09, 0x06, 0x05, 0x2b, 0x0e, 0x03, 0x02, 0x1a, 0x05, 0x00, 0x04, 0x14,
];
const RSA_SHA256_DIGESTINFO_PREFIX: &[u8] = &[
    0x30, 0x31, 0x30, 0x0D, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05,
    0x00, 0x04, 0x20,
];
const RSA_SHA512_DIGESTINFO_PREFIX: &[u8] = &[
    0x30, 0x51, 0x30, 0x0D, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03, 0x05,
    0x00, 0x04, 0x40,
];

struct RdrandCryptoRng;

impl RngCore for RdrandCryptoRng {
    fn next_u32(&mut self) -> u32 {
        let mut bytes = [0u8; 4];
        rdrand_bytes(&mut bytes);
        u32::from_le_bytes(bytes)
    }

    fn next_u64(&mut self) -> u64 {
        let mut bytes = [0u8; 8];
        rdrand_bytes(&mut bytes);
        u64::from_le_bytes(bytes)
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        rdrand_bytes(dest);
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), RandError> {
        rdrand_bytes(dest);
        Ok(())
    }
}

impl CryptoRng for RdrandCryptoRng {}

fn rsa_pkcs1v15_hash_and_padding(
    message: &[u8],
    hash_type: &str,
) -> Option<(Vec<u8>, Pkcs1v15Sign)> {
    match hash_type {
        "sha1" => {
            let mut hasher = Sha1::new();
            hasher.update(message);
            let hash = hasher.finalize().to_vec();
            Some((
                hash,
                Pkcs1v15Sign {
                    hash_len: Some(20),
                    prefix: RSA_SHA1_DIGESTINFO_PREFIX.to_vec().into_boxed_slice(),
                },
            ))
        }
        "sha256" => {
            let mut hasher = Sha256::new();
            hasher.update(message);
            let hash = hasher.finalize().to_vec();
            Some((
                hash,
                Pkcs1v15Sign {
                    hash_len: Some(32),
                    prefix: RSA_SHA256_DIGESTINFO_PREFIX.to_vec().into_boxed_slice(),
                },
            ))
        }
        "sha512" => {
            let mut hasher = Sha512::new();
            hasher.update(message);
            let hash = hasher.finalize().to_vec();
            Some((
                hash,
                Pkcs1v15Sign {
                    hash_len: Some(64),
                    prefix: RSA_SHA512_DIGESTINFO_PREFIX.to_vec().into_boxed_slice(),
                },
            ))
        }
        _ => None,
    }
}

// ============================================================================
// Big Integer Arithmetic (2048-bit support)
// ============================================================================

#[derive(Clone, Debug, PartialEq, Eq)]
struct BigInt {
    limbs: Vec<u64>, // Little-endian u64 limbs
}

impl BigInt {
    /// Create from u64
    fn from_u64(value: u64) -> Self {
        BigInt {
            limbs: Vec::from([value]),
        }
    }

    /// Create from bytes (big-endian)
    fn from_be_bytes(bytes: &[u8]) -> Self {
        if bytes.is_empty() {
            return BigInt::from_u64(0);
        }

        let num_limbs = (bytes.len() + 7) / 8;
        let mut limbs = Vec::with_capacity(num_limbs);
        limbs.resize(num_limbs, 0);

        for (i, &byte) in bytes.iter().enumerate() {
            let limb_idx = (bytes.len() - 1 - i) / 8;
            let byte_idx = (bytes.len() - 1 - i) % 8;
            limbs[limb_idx] |= (byte as u64) << (byte_idx * 8);
        }

        // Remove leading zeros
        while limbs.len() > 1 && limbs.last() == Some(&0) {
            limbs.pop();
        }

        BigInt { limbs }
    }

    /// Convert to bytes (big-endian)
    fn to_be_bytes(&self) -> Vec<u8> {
        if self.limbs.is_empty() {
            return vec![0];
        }

        let mut bytes = Vec::with_capacity(self.limbs.len() * 8);

        for limb in self.limbs.iter().rev() {
            for i in 0..8 {
                bytes.push(((limb >> (56 - i * 8)) & 0xFF) as u8);
            }
        }

        let first_non_zero = bytes
            .iter()
            .position(|&byte| byte != 0)
            .unwrap_or(bytes.len().saturating_sub(1));

        bytes[first_non_zero..].to_vec()
    }

    /// Addition
    fn add(&self, other: &BigInt) -> BigInt {
        let max_len = self.limbs.len().max(other.limbs.len());
        let mut result = Vec::with_capacity(max_len + 1);
        let mut carry = 0u128;

        for i in 0..max_len {
            let a = if i < self.limbs.len() {
                self.limbs[i] as u128
            } else {
                0
            };
            let b = if i < other.limbs.len() {
                other.limbs[i] as u128
            } else {
                0
            };
            carry += a + b;
            result.push(carry as u64);
            carry >>= 64;
        }

        if carry != 0 {
            result.push(carry as u64);
        }

        BigInt { limbs: result }
    }

    /// Subtraction (assumes self >= other)
    fn sub(&self, other: &BigInt) -> BigInt {
        let mut result = Vec::with_capacity(self.limbs.len());
        let mut borrow = 0i128;

        for i in 0..self.limbs.len() {
            let a = self.limbs[i] as i128;
            let b = if i < other.limbs.len() {
                other.limbs[i] as i128
            } else {
                0
            };
            borrow += a - b;
            result.push(borrow as u64);
            borrow >>= 64;
        }

        // Remove leading zeros
        while result.len() > 1 && result.last() == Some(&0) {
            result.pop();
        }

        BigInt { limbs: result }
    }

    /// Multiplication (schoolbook algorithm)
    fn mul(&self, other: &BigInt) -> BigInt {
        let result_len = self.limbs.len() + other.limbs.len();
        let mut result = Vec::with_capacity(result_len);
        result.resize(result_len, 0);

        for i in 0..self.limbs.len() {
            let mut carry = 0u128;
            for j in 0..other.limbs.len() {
                let idx = i + j;
                carry += result[idx] as u128 + self.limbs[i] as u128 * other.limbs[j] as u128;
                result[idx] = carry as u64;
                carry >>= 64;
            }
            if carry != 0 {
                result[i + other.limbs.len()] = carry as u64;
            }
        }

        // Remove leading zeros
        while result.len() > 1 && result.last() == Some(&0) {
            result.pop();
        }

        BigInt { limbs: result }
    }

    /// Comparison: self >= other
    fn ge(&self, other: &BigInt) -> bool {
        // Compare lengths first
        if self.limbs.len() > other.limbs.len() {
            return true;
        }
        if self.limbs.len() < other.limbs.len() {
            return false;
        }

        // Same length, compare from most significant limb
        for i in (0..self.limbs.len()).rev() {
            if self.limbs[i] > other.limbs[i] {
                return true;
            }
            if self.limbs[i] < other.limbs[i] {
                return false;
            }
        }

        false // equal
    }

    /// Comparison: self > other
    fn gt(&self, other: &BigInt) -> bool {
        if self.limbs.len() > other.limbs.len() {
            return true;
        }
        if self.limbs.len() < other.limbs.len() {
            return false;
        }

        for i in (0..self.limbs.len()).rev() {
            if self.limbs[i] > other.limbs[i] {
                return true;
            }
            if self.limbs[i] < other.limbs[i] {
                return false;
            }
        }

        false
    }

    /// Check if zero
    fn is_zero(&self) -> bool {
        self.limbs.len() == 1 && self.limbs[0] == 0
    }

    /// Check if one
    fn is_one(&self) -> bool {
        self.limbs.len() == 1 && self.limbs[0] == 1
    }

    /// Modular exponentiation: base^exp mod modulus
    /// Using square-and-multiply algorithm
    ///
    /// Legacy private-op lane only; production sign path uses RustCrypto.
    #[cfg(feature = "rsa_legacy_private_ops")]
    fn mod_pow(&self, exp: &BigInt, modulus: &BigInt) -> BigInt {
        let mut result = BigInt::from_u64(1);
        let mut base = self.clone();
        let mut exponent = exp.clone();

        // Process each bit of exponent from LSB to MSB
        while !exponent.is_zero() {
            // If current bit is 1, multiply result by base
            if exponent.limbs[0] & 1 == 1 {
                result = result.mul(&base);
                result = result.mod_reduce(modulus);
            }

            // Shift exponent right by 1 bit
            exponent.shr(1);
            if exponent.is_zero() {
                break;
            }

            // Square base only if more exponent bits remain.
            base = base.mul(&base);
            base = base.mod_reduce(modulus);
        }

        result
    }

    /// Modular reduction: self mod modulus
    fn mod_reduce(&self, modulus: &BigInt) -> BigInt {
        if modulus.is_zero() {
            return BigInt::from_u64(0);
        }

        let value = ExternalBigUint::from_bytes_be(&self.to_be_bytes());
        let modulus_value = ExternalBigUint::from_bytes_be(&modulus.to_be_bytes());
        let remainder = value % modulus_value;

        BigInt::from_be_bytes(&remainder.to_bytes_be())
    }

    /// Right shift by n bits
    fn shr(&mut self, n: usize) -> &mut Self {
        let word_shift = n / 64;
        let bit_shift = n % 64;

        // Shift words
        if word_shift > 0 && word_shift < self.limbs.len() {
            for i in 0..(self.limbs.len() - word_shift) {
                self.limbs[i] = self.limbs[i + word_shift];
            }
            for i in (self.limbs.len() - word_shift)..self.limbs.len() {
                self.limbs[i] = 0;
            }
        } else if word_shift >= self.limbs.len() {
            self.limbs = vec![0];
            return self;
        }

        // Shift bits within words
        if bit_shift > 0 {
            let len = self.limbs.len();
            let mut values = Vec::with_capacity(len);
            for i in 0..len {
                values.push(self.limbs[i]);
            }

            for i in 0..(len - 1) {
                self.limbs[i] = (values[i] >> bit_shift) | (values[i + 1] << (64 - bit_shift));
            }
            self.limbs[len - 1] >>= bit_shift;
        }

        // Remove leading zeros
        while self.limbs.len() > 1 && self.limbs.last() == Some(&0) {
            self.limbs.pop();
        }

        self
    }

    /// Extended Euclidean Algorithm for modular inverse
    fn mod_inverse(&self, modulus: &BigInt) -> Option<BigInt> {
        if self.is_zero() || modulus.is_zero() {
            return None;
        }

        let mut t = BigInt::from_u64(0);
        let mut newt = BigInt::from_u64(1);
        let mut r = modulus.clone();
        let mut newr = self.mod_reduce(modulus);

        if newr.is_zero() {
            return None;
        }

        while !newr.is_zero() {
            let quotient = r.div(&newr)?;

            let q_times_newt = quotient.mul(&newt).mod_reduce(modulus);
            let next_t = if t.ge(&q_times_newt) {
                t.sub(&q_times_newt)
            } else {
                t.add(modulus).sub(&q_times_newt)
            }
            .mod_reduce(modulus);

            let q_times_newr = quotient.mul(&newr);
            if q_times_newr.gt(&r) {
                return None;
            }
            let next_r = r.sub(&q_times_newr);

            t = newt;
            newt = next_t;
            r = newr;
            newr = next_r;
        }

        if !r.is_one() {
            return None; // No inverse exists
        }

        Some(t.mod_reduce(modulus))
    }

    /// Division
    fn div(&self, divisor: &BigInt) -> Option<BigInt> {
        if divisor.is_zero() {
            return None;
        }

        let dividend = ExternalBigUint::from_bytes_be(&self.to_be_bytes());
        let divisor_value = ExternalBigUint::from_bytes_be(&divisor.to_be_bytes());
        let quotient = dividend / divisor_value;

        Some(BigInt::from_be_bytes(&quotient.to_bytes_be()))
    }

    /// Left shift
    fn shl(&self, n: usize) -> BigInt {
        let mut result = self.clone();
        let word_shift = n / 64;
        let bit_shift = n % 64;

        // Add zero limbs for word shift
        for _ in 0..word_shift {
            result.limbs.insert(0, 0);
        }

        // Shift bits
        if bit_shift > 0 {
            for i in (1..result.limbs.len()).rev() {
                result.limbs[i] =
                    (result.limbs[i] << bit_shift) | (result.limbs[i - 1] >> (64 - bit_shift));
            }
            result.limbs[0] <<= bit_shift;
        }

        result
    }
}

// ============================================================================
// RSA Public Key
// ============================================================================

#[derive(Clone)]
pub struct RsaPublicKey {
    n: BigInt, // Modulus
    e: BigInt, // Public exponent
}

impl RsaPublicKey {
    /// Create from modulus and public exponent
    pub fn new(n: &[u8], e: &[u8]) -> Self {
        RsaPublicKey {
            n: BigInt::from_be_bytes(n),
            e: BigInt::from_be_bytes(e),
        }
    }

    /// Return modulus bytes in big-endian form.
    pub fn modulus_bytes(&self) -> Vec<u8> {
        self.n.to_be_bytes()
    }

    /// Return exponent bytes in big-endian form.
    pub fn exponent_bytes(&self) -> Vec<u8> {
        self.e.to_be_bytes()
    }

    fn to_external_key(&self) -> Option<ExternalRsaPublicKey> {
        let n = ExternalBigUint::from_bytes_be(&self.n.to_be_bytes());
        let e = ExternalBigUint::from_bytes_be(&self.e.to_be_bytes());
        ExternalRsaPublicKey::new(n, e).ok()
    }

    /// Verify RSA-PKCS#1 v1.5 signature
    ///
    /// Parameters:
    ///   - message: The signed message
    ///   - signature: Raw signature bytes
    ///   - hash_type: "sha1", "sha256" or "sha512"
    /// Returns:
    ///   - true if signature is valid
    pub fn verify(&self, message: &[u8], signature: &[u8], hash_type: &str) -> bool {
        let Some(pub_key) = self.to_external_key() else {
            return false;
        };
        let Some((hash, padding)) = rsa_pkcs1v15_hash_and_padding(message, hash_type) else {
            return false;
        };
        pub_key.verify(padding, &hash, signature).is_ok()
    }

    /// Build DigestInfo structure (ASN.1 DER encoding)
    fn build_digest_info(&self, hash: &[u8], hash_type: &str) -> Vec<u8> {
        // Simplified DigestInfo construction
        // In production, full ASN.1 DER encoding is needed
        let mut result = Vec::new();

        match hash_type {
            "sha1" => {
                // SHA-1 OID: 1.3.14.3.2.26
                result.extend_from_slice(&[
                    0x30, 0x21, // SEQUENCE, length 33
                    0x30, 0x09, // SEQUENCE, length 9
                    0x06, 0x05, // OID, length 5
                    0x2b, 0x0e, 0x03, 0x02, 0x1a, // SHA-1 OID
                    0x05, 0x00, // NULL
                    0x04, 0x14, // OCTET STRING, length 20
                ]);
                result.extend_from_slice(hash);
            }
            "sha256" => {
                // SHA-256 OID: 1.2.840.113549.2.7
                result.extend_from_slice(&[
                    0x30, 0x31, // SEQUENCE, length 49
                    0x30, 0x0D, // SEQUENCE, length 13
                    0x06, 0x09, // OID, length 9
                    0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, // SHA-256 OID
                    0x05, 0x00, // NULL
                    0x04, 0x20, // OCTET STRING, length 32
                ]);
                result.extend_from_slice(hash);
            }
            "sha512" => {
                // SHA-512 OID: 1.2.840.113549.2.9
                result.extend_from_slice(&[
                    0x30, 0x51, // SEQUENCE, length 81
                    0x30, 0x0D, // SEQUENCE, length 13
                    0x06, 0x09, // OID, length 9
                    0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03, // SHA-512 OID
                    0x05, 0x00, // NULL
                    0x04, 0x40, // OCTET STRING, length 64
                ]);
                result.extend_from_slice(hash);
            }
            _ => {}
        }

        result
    }
}

// ============================================================================
// RSA Private Key
// ============================================================================

#[derive(Clone)]
pub struct RsaPrivateKey {
    n: BigInt,    // Modulus
    e: BigInt,    // Public exponent
    d: BigInt,    // Private exponent
    p: BigInt,    // First prime factor
    q: BigInt,    // Second prime factor
    dp: BigInt,   // d mod (p-1)
    dq: BigInt,   // d mod (q-1)
    qinv: BigInt, // q^(-1) mod p
}

impl RsaPrivateKey {
    /// Generate RSA key pair
    ///
    /// Parameters:
    ///   - key_size: Key size in bits (2048 recommended)
    #[cfg(feature = "rsa_legacy_private_ops")]
    pub fn generate(key_size: usize) -> Self {
        // Generate two large primes using Miller-Rabin primality test
        let p = Self::generate_prime(key_size / 2);
        let q = Self::generate_prime(key_size / 2);

        let n = p.mul(&q);
        let phi_n = p
            .sub(&BigInt::from_u64(1))
            .mul(&q.sub(&BigInt::from_u64(1)));

        let e = BigInt::from_u64(65537);
        let d = e
            .mod_inverse(&phi_n)
            .expect("Failed to compute modular inverse");

        // CRT components
        let dp = d.mod_reduce(&p.sub(&BigInt::from_u64(1)));
        let dq = d.mod_reduce(&q.sub(&BigInt::from_u64(1)));
        let qinv = q.mod_inverse(&p).expect("Failed to compute qinv");

        RsaPrivateKey {
            n,
            e,
            d,
            p,
            q,
            dp,
            dq,
            qinv,
        }
    }

    /// Generate a probable prime using Miller-Rabin test
    #[cfg(feature = "rsa_legacy_private_ops")]
    fn generate_prime(bits: usize) -> BigInt {
        loop {
            // Generate random odd number with specified bit length
            let mut bytes = vec![0u8; (bits + 7) / 8];
            crate::crypto::rdrand_bytes(&mut bytes);

            let len = bytes.len();

            // Set MSB to 1 (ensure correct bit length)
            if len > 0 {
                bytes[0] |= 0x80;
            }

            // Set LSB to 1 (make it odd)
            if len > 0 {
                bytes[len - 1] |= 0x01;
            }

            let candidate = BigInt::from_be_bytes(&bytes);

            // Perform Miller-Rabin primality test
            if Self::miller_rabin_test(&candidate, 20) {
                return candidate;
            }
        }
    }

    /// Miller-Rabin probabilistic primality test
    ///
    /// Returns true if n is probably prime, false if composite
    /// k: number of rounds (higher = more accurate)
    #[cfg(feature = "rsa_legacy_private_ops")]
    fn miller_rabin_test(n: &BigInt, k: usize) -> bool {
        // Handle small numbers
        if n.limbs.len() == 1 {
            match n.limbs[0] {
                2 | 3 => return true,
                0 | 1 => return false,
                _ => {}
            }
        }

        // Even numbers > 2 are composite
        if n.limbs[0] & 1 == 0 {
            return false;
        }

        // Write n-1 as d * 2^r
        let n_minus_1 = n.sub(&BigInt::from_u64(1));
        let mut r = 0;
        let mut d = n_minus_1.clone();

        // Count trailing zeros
        while d.limbs[0] & 1 == 0 {
            d.shr(1);
            r += 1;
        }

        // Witness loop
        for _ in 0..k {
            // Pick random witness a in [2, n-2]
            let a = Self::random_range(&BigInt::from_u64(2), &n_minus_1);

            let mut x = a.mod_pow(&d, n);

            if x.is_one() || x == n_minus_1 {
                continue;
            }

            let mut composite = true;
            for _ in 0..(r - 1) {
                x = x.mul(&x).mod_reduce(n);
                if x.is_one() {
                    return false; // Composite
                }
                if x == n_minus_1 {
                    composite = false;
                    break;
                }
            }

            if composite {
                return false; // Composite
            }
        }

        true // Probably prime
    }

    /// Generate random number in range [min, max)
    #[cfg(feature = "rsa_legacy_private_ops")]
    fn random_range(min: &BigInt, max: &BigInt) -> BigInt {
        let range = max.sub(min);
        let mut bytes = vec![0u8; range.limbs.len() * 8];
        crate::crypto::rdrand_bytes(&mut bytes);

        let rand_num = BigInt::from_be_bytes(&bytes);
        rand_num.mod_reduce(&range).add(min)
    }

    /// Get public key
    pub fn public_key(&self) -> RsaPublicKey {
        RsaPublicKey {
            n: self.n.clone(),
            e: self.e.clone(),
        }
    }

    fn to_external_key(&self) -> Option<ExternalRsaPrivateKey> {
        let n = ExternalBigUint::from_bytes_be(&self.n.to_be_bytes());
        let e = ExternalBigUint::from_bytes_be(&self.e.to_be_bytes());
        let d = ExternalBigUint::from_bytes_be(&self.d.to_be_bytes());
        let p = ExternalBigUint::from_bytes_be(&self.p.to_be_bytes());
        let q = ExternalBigUint::from_bytes_be(&self.q.to_be_bytes());
        ExternalRsaPrivateKey::from_components(n, e, d, vec![p, q]).ok()
    }

    /// Sign a message using RSA-PKCS#1 v1.5
    ///
    /// Parameters:
    ///   - message: The message to sign
    ///   - hash_type: "sha1", "sha256" or "sha512"
    /// Returns:
    ///   - Signature bytes
    pub fn sign(&self, message: &[u8], hash_type: &str) -> Vec<u8> {
        let Some((hash, padding)) = rsa_pkcs1v15_hash_and_padding(message, hash_type) else {
            return Vec::new();
        };
        let Some(private_key) = self.to_external_key() else {
            return Vec::new();
        };
        let mut rng = RdrandCryptoRng;
        private_key
            .sign_with_rng(&mut rng, padding, &hash)
            .unwrap_or_else(|_| Vec::new())
    }

    /// RSA private operation using Chinese Remainder Theorem.
    ///
    /// Legacy private-op lane only; production sign path uses RustCrypto.
    #[cfg(feature = "rsa_legacy_private_ops")]
    fn rsa_crt_private_op(&self, m: &BigInt) -> BigInt {
        // m1 = m^dp mod p
        let m1 = m.mod_pow(&self.dp, &self.p);

        // m2 = m^dq mod q
        let m2 = m.mod_pow(&self.dq, &self.q);

        // h = qinv * (m1 - m2) mod p
        let diff = if m1.ge(&m2) {
            m1.sub(&m2)
        } else {
            let sum = m2.sub(&m1);
            self.p.sub(&sum)
        };

        let h = self.qinv.mul(&diff);
        let h_mod_p = h.mod_reduce(&self.p);

        // s = m2 + h * q
        m2.add(&h_mod_p.mul(&self.q))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::traits::{PrivateKeyParts, PublicKeyParts};

    struct TestCryptoRng {
        state: u64,
    }

    impl TestCryptoRng {
        fn new(seed: u64) -> Self {
            Self { state: seed }
        }

        fn next_word(&mut self) -> u64 {
            let mut x = self.state;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.state = x;
            x
        }
    }

    impl RngCore for TestCryptoRng {
        fn next_u32(&mut self) -> u32 {
            self.next_word() as u32
        }

        fn next_u64(&mut self) -> u64 {
            self.next_word()
        }

        fn fill_bytes(&mut self, dest: &mut [u8]) {
            let mut offset = 0;
            while offset < dest.len() {
                let word = self.next_word().to_le_bytes();
                let chunk = core::cmp::min(8, dest.len() - offset);
                dest[offset..offset + chunk].copy_from_slice(&word[..chunk]);
                offset += chunk;
            }
        }

        fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), RandError> {
            self.fill_bytes(dest);
            Ok(())
        }
    }

    impl CryptoRng for TestCryptoRng {}

    fn left_pad_to_len(mut bytes: Vec<u8>, target_len: usize) -> Vec<u8> {
        if bytes.len() >= target_len {
            return bytes;
        }

        let mut padded = vec![0u8; target_len - bytes.len()];
        padded.append(&mut bytes);
        padded
    }

    #[test]
    fn sha256_mapping_uses_sha2_digest_not_sha3() {
        let message = b"echos-rsa-sha2-mapping";
        let (hash, padding) =
            rsa_pkcs1v15_hash_and_padding(message, "sha256").expect("sha256 mapping available");

        let mut sha256 = Sha256::new();
        sha256.update(message);
        let sha2_hash = sha256.finalize();

        assert_eq!(hash, sha2_hash.to_vec());
        assert_ne!(hash.as_slice(), &crate::crypto::sha3_256(message));
        assert_eq!(padding.prefix.as_ref(), RSA_SHA256_DIGESTINFO_PREFIX);
    }

    #[test]
    fn verify_rejects_pkcs1v15_digestinfo_with_trailing_garbage() {
        let mut rng = TestCryptoRng::new(0x9e37_79b9_7f4a_7c15);
        let private =
            ExternalRsaPrivateKey::new(&mut rng, 1024).expect("deterministic RSA key generation");

        let message = b"pkcs1v15 trailing garbage must be rejected";
        let mut sha256 = Sha256::new();
        sha256.update(message);
        let digest = sha256.finalize();

        let mut digest_info = Vec::from(RSA_SHA256_DIGESTINFO_PREFIX);
        digest_info.extend_from_slice(&digest);

        let modulus_len = private.n().to_bytes_be().len();
        let garbage_len = 8;
        let ff_len = modulus_len
            .checked_sub(digest_info.len() + garbage_len + 3)
            .expect("modulus length supports PKCS#1 v1.5 block");
        assert!(ff_len >= 8);

        let mut encoded_message = Vec::with_capacity(modulus_len);
        encoded_message.push(0x00);
        encoded_message.push(0x01);
        encoded_message.extend(core::iter::repeat_n(0xff, ff_len));
        encoded_message.push(0x00);
        encoded_message.extend_from_slice(&digest_info);
        encoded_message.extend(core::iter::repeat_n(0x42, garbage_len));

        let encoded_int = ExternalBigUint::from_bytes_be(&encoded_message);
        let forged_signature = encoded_int.modpow(private.d(), private.n());
        let signature = left_pad_to_len(forged_signature.to_bytes_be(), modulus_len);

        let public = RsaPublicKey::new(&private.n().to_bytes_be(), &private.e().to_bytes_be());
        assert!(!public.verify(message, &signature, "sha256"));
    }

    #[test]
    fn bigint_division_by_zero_returns_none() {
        let dividend = BigInt::from_u64(1234);
        let divisor = BigInt::from_u64(0);

        assert!(dividend.div(&divisor).is_none());
    }

    #[test]
    fn mod_inverse_handles_negative_intermediate_coefficients() {
        let value = BigInt::from_u64(17);
        let modulus = BigInt::from_u64(3120);

        let inverse = value
            .mod_inverse(&modulus)
            .expect("17 must be invertible modulo 3120");
        assert_eq!(inverse, BigInt::from_u64(2753));

        let one = value.mul(&inverse).mod_reduce(&modulus);
        assert_eq!(one, BigInt::from_u64(1));
    }

    #[test]
    fn mod_inverse_returns_none_for_non_coprime_values() {
        let value = BigInt::from_u64(12);
        let modulus = BigInt::from_u64(18);

        assert!(value.mod_inverse(&modulus).is_none());
    }

    #[test]
    fn bigint_mod_reduce_matches_biguint_remainder() {
        let value = BigInt::from_be_bytes(&[
            0x8f, 0x72, 0x14, 0x9d, 0xe1, 0x6a, 0x3c, 0xb8, 0x55, 0x20, 0x93, 0xfe, 0x44, 0x7a,
            0x61, 0xd0, 0xab, 0x31, 0x19, 0x4e, 0x76, 0xcf, 0x80, 0x12, 0x33, 0x98, 0xee, 0x4a,
            0x7c, 0xd2, 0x01, 0x65,
        ]);
        let modulus = BigInt::from_be_bytes(&[
            0x01, 0xff, 0x10, 0x29, 0x63, 0x7b, 0x4a, 0xd8, 0x9c, 0xee, 0x74, 0x21, 0x5b, 0x6f,
            0x93, 0x0d,
        ]);

        let reduced = value.mod_reduce(&modulus);
        let expected = ExternalBigUint::from_bytes_be(&value.to_be_bytes())
            % ExternalBigUint::from_bytes_be(&modulus.to_be_bytes());
        let expected_bytes = {
            let bytes = expected.to_bytes_be();
            if bytes.is_empty() {
                vec![0]
            } else {
                bytes
            }
        };

        assert_eq!(reduced.to_be_bytes(), expected_bytes);
    }

    #[test]
    fn sign_and_verify_roundtrip_uses_external_rsa_lane() {
        let mut keygen_rng = TestCryptoRng::new(0x1234_5678_9abc_def0);
        let private = ExternalRsaPrivateKey::new(&mut keygen_rng, 1024)
            .expect("deterministic RSA key generation");

        let local = RsaPrivateKey {
            n: BigInt::from_be_bytes(&private.n().to_bytes_be()),
            e: BigInt::from_be_bytes(&private.e().to_bytes_be()),
            d: BigInt::from_be_bytes(&private.d().to_bytes_be()),
            p: BigInt::from_be_bytes(&private.primes()[0].to_bytes_be()),
            q: BigInt::from_be_bytes(&private.primes()[1].to_bytes_be()),
            dp: BigInt::from_u64(1),
            dq: BigInt::from_u64(1),
            qinv: BigInt::from_u64(1),
        };

        let message = b"echos-rsa-external-lane-roundtrip";
        let signature = local.sign(message, "sha256");
        assert!(!signature.is_empty());

        let public = RsaPublicKey::new(&private.n().to_bytes_be(), &private.e().to_bytes_be());
        assert!(public.verify(message, &signature, "sha256"));
    }

    #[test]
    fn sign_unknown_hash_returns_empty_without_panic() {
        let key = RsaPrivateKey {
            n: BigInt::from_u64(1),
            e: BigInt::from_u64(1),
            d: BigInt::from_u64(1),
            p: BigInt::from_u64(1),
            q: BigInt::from_u64(1),
            dp: BigInt::from_u64(1),
            dq: BigInt::from_u64(1),
            qinv: BigInt::from_u64(1),
        };

        let result =
            std::panic::catch_unwind(|| key.sign(b"echos-rsa-unsupported-hash", "sha3-512"));
        assert!(result.is_ok(), "unsupported hash must not panic");
        assert!(result.unwrap().is_empty());
    }
}
