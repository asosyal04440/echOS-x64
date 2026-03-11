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

use crate::crypto::{rdrand_bytes, Sha3};
use alloc::vec;
use alloc::vec::Vec;

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
        let mut bytes = Vec::with_capacity(self.limbs.len() * 8);

        for limb in self.limbs.iter().rev() {
            for i in 0..8 {
                bytes.push(((limb >> (56 - i * 8)) & 0xFF) as u8);
            }
        }

        // Remove leading zeros
        while bytes.len() > 1 && bytes[0] == 0 {
            bytes.remove(0);
        }

        bytes
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

            // Square base
            base = base.mul(&base);
            base = base.mod_reduce(modulus);

            // Shift exponent right by 1 bit
            exponent.shr(1);
        }

        result
    }

    /// Modular reduction: self mod modulus
    fn mod_reduce(&self, modulus: &BigInt) -> BigInt {
        if self.limbs.len() < modulus.limbs.len() {
            return self.clone();
        }

        let mut remainder = self.clone();

        // Align modulus with most significant part of remainder
        let shift = (remainder.limbs.len() - modulus.limbs.len()) * 64;

        for i in (0..=shift).rev() {
            let mut shifted_mod = modulus.clone();
            // Shift left by i bits (simplified, should be by words)
            for _ in 0..i / 64 {
                shifted_mod.limbs.insert(0, 0);
            }

            // Subtract while possible
            while remainder.ge(&shifted_mod) {
                remainder = remainder.sub(&shifted_mod);
            }
        }

        remainder
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
        if self.is_zero() {
            return None;
        }

        let mut t = BigInt::from_u64(0);
        let mut newt = BigInt::from_u64(1);
        let mut r = modulus.clone();
        let mut newr = self.clone();

        while !newr.is_zero() {
            let quotient = r.div(&newr);

            let tmp_t = t.clone();
            let tmp_newt = newt.clone();
            t = newt;
            let q_times_newt = quotient.mul(&tmp_newt);
            newt = if tmp_t.ge(&q_times_newt) {
                tmp_t.sub(&q_times_newt)
            } else {
                // Handle negative result (simplified)
                BigInt::from_u64(0)
            };

            let tmp_r = r.clone();
            let tmp_newr = newr.clone();
            r = newr;
            let q_times_newr = quotient.mul(&tmp_newr);
            newr = if tmp_r.ge(&q_times_newr) {
                tmp_r.sub(&q_times_newr)
            } else {
                BigInt::from_u64(0)
            };
        }

        if r.limbs.len() > 1 || r.limbs[0] > 1 {
            return None; // No inverse exists
        }

        if t.limbs.len() > 1 || (t.limbs.len() == 1 && t.limbs[0] & 0x8000000000000000 != 0) {
            Some(t.add(modulus))
        } else {
            Some(t)
        }
    }

    /// Division
    fn div(&self, divisor: &BigInt) -> BigInt {
        if divisor.is_zero() {
            return BigInt::from_u64(0);
        }

        let mut quotient = BigInt::from_u64(0);
        let mut remainder = self.clone();

        // Normalize divisor
        let mut normalized_divisor = divisor.clone();
        let mut shift_count = 0;

        while normalized_divisor.limbs.len() < remainder.limbs.len()
            || (normalized_divisor.limbs.len() == remainder.limbs.len()
                && !remainder.ge(&normalized_divisor))
        {
            normalized_divisor = normalized_divisor.shl(1);
            shift_count += 1;
        }

        // Long division
        for i in (0..=shift_count).rev() {
            let mut shifted_divisor = divisor.clone();
            for _ in 0..i {
                shifted_divisor = shifted_divisor.shl(1);
            }

            if remainder.ge(&shifted_divisor) {
                remainder = remainder.sub(&shifted_divisor);
                quotient = quotient.add(&BigInt::from_u64(1 << i));
            }
        }

        quotient
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

    /// Verify RSA-PKCS#1 v1.5 signature
    ///
    /// Parameters:
    ///   - message: The signed message
    ///   - signature: Raw signature bytes
    ///   - hash_type: "sha256" or "sha512"
    /// Returns:
    ///   - true if signature is valid
    pub fn verify(&self, message: &[u8], signature: &[u8], hash_type: &str) -> bool {
        // Convert signature to integer
        let sig_int = BigInt::from_be_bytes(signature);

        // Verify signature is in valid range [0, n-1]
        if sig_int.ge(&self.n) {
            return false;
        }

        // RSA operation: m = s^e mod n
        let m_int = sig_int.mod_pow(&self.e, &self.n);

        // Convert to bytes
        let mut m_bytes = m_int.to_be_bytes();

        // Pad to modulus size
        let key_size = (self.n.limbs.len() * 8);
        while m_bytes.len() < key_size {
            m_bytes.insert(0, 0);
        }

        // Verify PKCS#1 v1.5 padding
        if m_bytes.len() < 11 || m_bytes[0] != 0x00 || m_bytes[1] != 0x01 {
            return false;
        }

        // Find 0x00 separator after padding bytes
        let mut separator_idx = 2;
        while separator_idx < m_bytes.len() && m_bytes[separator_idx] == 0xFF {
            separator_idx += 1;
        }

        if separator_idx >= m_bytes.len() || m_bytes[separator_idx] != 0x00 {
            return false;
        }

        // Extract DigestInfo
        let digest_info = &m_bytes[separator_idx + 1..];

        // Hash the message
        let hash = match hash_type {
            "sha256" => {
                let mut hasher = Sha3::sha3_256();
                hasher.update(message);
                hasher.finalize()
            }
            "sha512" => {
                let mut hasher = Sha3::sha3_512();
                hasher.update(message);
                hasher.finalize()
            }
            _ => return false,
        };

        // Build expected DigestInfo (simplified ASN.1 DER)
        let expected_digest_info = self.build_digest_info(&hash, hash_type);

        // Compare
        digest_info == expected_digest_info
    }

    /// Build DigestInfo structure (ASN.1 DER encoding)
    fn build_digest_info(&self, hash: &[u8], hash_type: &str) -> Vec<u8> {
        // Simplified DigestInfo construction
        // In production, full ASN.1 DER encoding is needed
        let mut result = Vec::new();

        match hash_type {
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

    /// Sign a message using RSA-PKCS#1 v1.5
    ///
    /// Parameters:
    ///   - message: The message to sign
    ///   - hash_type: "sha256" or "sha512"
    /// Returns:
    ///   - Signature bytes
    pub fn sign(&self, message: &[u8], hash_type: &str) -> Vec<u8> {
        // Hash the message
        let hash = match hash_type {
            "sha256" => {
                let mut hasher = Sha3::sha3_256();
                hasher.update(message);
                hasher.finalize()
            }
            "sha512" => {
                let mut hasher = Sha3::sha3_512();
                hasher.update(message);
                hasher.finalize()
            }
            _ => panic!("Unsupported hash type"),
        };

        // Build DigestInfo
        let digest_info = self.public_key().build_digest_info(&hash, hash_type);

        // Build padded message
        let key_size = self.n.limbs.len() * 8;
        let mut padded = Vec::with_capacity(key_size);
        padded.push(0x00);
        padded.push(0x01);

        // Padding bytes (0xFF)
        let padding_len = key_size - digest_info.len() - 3;
        for _ in 0..padding_len {
            padded.push(0xFF);
        }

        padded.push(0x00);
        padded.extend_from_slice(&digest_info);

        // Convert to integer
        let m_int = BigInt::from_be_bytes(&padded);

        // RSA operation: s = m^d mod n
        // Using CRT for efficiency
        let s = self.rsa_crt_private_op(&m_int);

        // Convert to bytes
        s.to_be_bytes()
    }

    /// RSA private operation using Chinese Remainder Theorem
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
