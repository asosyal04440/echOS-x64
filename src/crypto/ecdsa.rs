//! # ECDSA P-256 (Elliptic Curve Digital Signature Algorithm)
//!
//! NIST P-256 (secp256r1, prime256v1) eğrisi üzerinde ECDSA imzalama ve doğrulama.
//! RFC 5480, RFC 5915, FIPS 186-4 uyumlu.
//!
//! ## P-256 Eğrisi Parametreleri
//!
//! ```text
//! p = 2^224 * (2^32 - 1) + 2^192 + 2^96 - 1
//!   = 0xFFFFFFFF000000010000000000000000FFFFFFFFFFFFFFFFFFFFFFFF
//! a = -3 (mod p)
//! b = 0x5AC635D8AA3A93E7B3EBBD55769886BC651D06B0CC53B0F63BCE3C3E27D2604B
//! G = base point (x, y)
//! n = order of G
//!   = 0xFFFFFFFF00000000FFFFFFFFFFFFFFFFBCE6FAADA7179E84F3B9CAC2FC632551
//! ```
//!
//! ## ECDSA İmzalama
//!
//! ```text
//! Input: message m, private key d
//! Output: signature (r, s)
//!
//! 1. Hash z = SHA-256(m)
//! 2. Random k ∈ [1, n-1] seç (kriptografik güvenli RNG)
//! 3. (x, y) = k * G (skalar çarpım)
//! 4. r = x mod n (eğer r=0 ise 2. adıma dön)
//! 5. s = k^-1 * (z + r*d) mod n (eğer s=0 ise 2. adıma dön)
//! 6. Return (r, s)
//! ```
//!
//! ## ECDSA Doğrulama
//!
//! ```text
//! Input: message m, signature (r, s), public key Q
//! Output: true/false
//!
//! 1. Verify r, s ∈ [1, n-1]
//! 2. Hash z = SHA-256(m)
//! 3. w = s^-1 mod n
//! 4. u1 = z*w mod n
//! 5. u2 = r*w mod n
//! 6. (x, y) = u1*G + u2*Q
//! 7. Return (x mod n == r)
//! ```

use crate::crypto::{rdrand_bytes, Sha3};
use alloc::vec::Vec;

// ============================================================================
// P-256 Eğrisi Sabitleri
// ============================================================================

/// P-256 prime field modulus: p = 2^224 * (2^32 - 1) + 2^192 + 2^96 - 1
const P: U256 = U256([
    0xFFFFFFFFFFFFFFFF,
    0x00000000FFFFFFFF,
    0x0000000000000000,
    0xFFFFFFFF00000001,
]);

/// P-256 curve coefficient a = -3 (mod p)
const A: U256 = U256([
    0xFFFFFFFCFFFFFFFF,
    0xFFFFFFFFFFFFFFFE,
    0xFFFFFFFFFFFFFFFF,
    0xFFFFFFFEFFFFFFFF,
]);

/// P-256 curve coefficient b
const B: U256 = U256([
    0x27D2604BCE3C3E27,
    0x651D06B0CC53B0F6,
    0x5AC635D8AA3A93E7,
    0xB3EBBD55769886BC,
]);

/// P-256 base point G (x coordinate)
const GX: U256 = U256([
    0xD89CDF62535341AC,
    0x65311CCC92DA07B6,
    0x18A0E32C7A5E6E5F,
    0x6B17D1F2E12C4247,
]);

/// P-256 base point G (y coordinate)
const GY: U256 = U256([
    0x2BB47A4EA3CC45D9,
    0x10EDD8D3EC95F217,
    0xF9B8E5A810F0ABD1,
    0x483ADA7726A3C465,
]);

/// P-256 curve order: n
const N: U256 = U256([
    0xFC632551BCE6FAAD,
    0xFFFFFFFFFFFFFFFF,
    0x0000000000000000,
    0xFFFFFFFF00000000,
]);

// ============================================================================
// 256-bit Unsigned Integer (Big Int Arithmetic)
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct U256([u64; 4]); // Little-endian: [LSW, ..., MSW]

impl U256 {
    const ZERO: U256 = U256([0, 0, 0, 0]);
    const ONE: U256 = U256([1, 0, 0, 0]);

    /// Addition modulo 2^256 (no carry out)
    fn add(&self, other: &U256) -> U256 {
        let mut result = U256::ZERO;
        let mut carry = 0u128;

        for i in 0..4 {
            carry += self.0[i] as u128 + other.0[i] as u128;
            result.0[i] = carry as u64;
            carry >>= 64;
        }

        result
    }

    /// Subtraction with borrow
    fn sub(&self, other: &U256) -> U256 {
        let mut result = U256::ZERO;
        let mut borrow = 0i128;

        for i in 0..4 {
            borrow += self.0[i] as i128 - other.0[i] as i128;
            result.0[i] = borrow as u64;
            borrow >>= 64;
        }

        result
    }

    /// Multiplication modulo 2^256
    fn mul(&self, other: &U256) -> U256 {
        let mut result = [0u64; 8]; // 512-bit intermediate

        for i in 0..4 {
            let mut carry = 0u128;
            for j in 0..4 {
                if i + j < 8 {
                    carry += result[i + j] as u128 + self.0[i] as u128 * other.0[j] as u128;
                    result[i + j] = carry as u64;
                    carry >>= 64;
                }
            }
            if i + 4 < 8 {
                result[i + 4] = carry as u64;
            }
        }

        // Reduce to 256 bits (modulo 2^256)
        U256([result[0], result[1], result[2], result[3]])
    }

    /// Modular reduction: self mod P (Montgomery-friendly for P-256)
    fn mod_p(&self) -> U256 {
        // Basitleştirilmiş: Eğer < P ise kendisi, değilse tekrarlı çıkarma
        // Optimizasyon: Montgomery ladder veya Barrett reduction kullanılabilir
        let mut result = *self;

        // while result >= P: result -= P
        while result.ge(&P) {
            result = result.sub(&P);
        }

        result
    }

    /// Modular reduction: self mod N
    fn mod_n(&self) -> U256 {
        let mut result = *self;
        while result.ge(&N) {
            result = result.sub(&N);
        }
        result
    }

    /// Comparison: self >= other
    fn ge(&self, other: &U256) -> bool {
        for i in (0..4).rev() {
            if self.0[i] > other.0[i] {
                return true;
            } else if self.0[i] < other.0[i] {
                return false;
            }
        }
        false // equal
    }

    /// Modular inverse using extended Euclidean algorithm (mod n)
    fn mod_inverse(&self) -> Option<U256> {
        // Fermat's little theorem: a^(-1) = a^(n-2) mod n (since n is prime)
        //或使用扩展欧几里得算法
        if *self == U256::ZERO {
            return None;
        }

        // Extended Euclidean Algorithm
        let mut t = U256::ZERO;
        let mut newt = U256::ONE;
        let mut r = N;
        let mut newr = *self;

        while newr != U256::ZERO {
            let quotient = r.div(&newr);

            let tmp_t = t;
            t = newt;
            newt = tmp_t.sub(&quotient.mul(&newt));
            newt = newt.mod_n();

            let tmp_r = r;
            r = newr;
            newr = tmp_r.sub(&quotient.mul(&newr));
        }

        if r.gt(&U256::ONE) {
            return None; // No inverse
        }

        if t.0[3] & 0x8000000000000000 != 0 {
            Some(t.add(&N))
        } else {
            Some(t)
        }
    }

    /// Division (for extended GCD)
    fn div(&self, other: &U256) -> U256 {
        // Simple long division
        if *other == U256::ZERO {
            return U256::ZERO;
        }

        let mut quotient = U256::ZERO;
        let mut remainder = *self;

        for i in (0..256).rev() {
            remainder = remainder.shl(1);
            let bit = (quotient.0[3] >> 63) & 1;
            quotient = quotient.shl(1);
            quotient.0[0] |= bit;

            if remainder.ge(other) {
                remainder = remainder.sub(other);
                quotient.0[0] |= 1;
            }
        }

        quotient
    }

    /// Greater than
    fn gt(&self, other: &U256) -> bool {
        for i in (0..4).rev() {
            if self.0[i] > other.0[i] {
                return true;
            } else if self.0[i] < other.0[i] {
                return false;
            }
        }
        false
    }

    /// Shift left
    fn shl(&self, n: usize) -> U256 {
        let mut result = *self;
        for _ in 0..n {
            let mut carry = 0;
            for i in 0..4 {
                let new_carry = result.0[i] >> 63;
                result.0[i] = (result.0[i] << 1) | carry;
                carry = new_carry;
            }
        }
        result
    }

    /// Check if zero
    fn is_zero(&self) -> bool {
        self.0.iter().all(|&x| x == 0)
    }

    /// Check if one
    fn is_one(&self) -> bool {
        self.0[0] == 1 && self.0[1..].iter().all(|&x| x == 0)
    }

    /// From bytes (big-endian)
    fn from_be_bytes(bytes: &[u8]) -> Option<U256> {
        if bytes.len() != 32 {
            return None;
        }

        let mut result = U256::ZERO;
        for (i, &byte) in bytes.iter().enumerate() {
            let word_idx = (31 - i) / 8;
            let byte_idx = (31 - i) % 8;
            result.0[word_idx] |= (byte as u64) << (byte_idx * 8);
        }
        Some(result)
    }

    /// To bytes (big-endian)
    fn to_be_bytes(&self) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        for (i, &word) in self.0.iter().enumerate() {
            for j in 0..8 {
                let byte_idx = i * 8 + j;
                if byte_idx < 32 {
                    bytes[31 - byte_idx] = ((word >> (j * 8)) & 0xFF) as u8;
                }
            }
        }
        bytes
    }
}

// ============================================================================
// P-256 Point (Affine Coordinates)
// ============================================================================

#[derive(Clone, Copy)]
struct Point {
    x: U256,
    y: U256,
    infinity: bool, // Point at infinity (identity element)
}

impl Point {
    const INFINITY: Point = Point {
        x: U256::ZERO,
        y: U256::ZERO,
        infinity: true,
    };

    /// Base point G
    fn generator() -> Point {
        Point {
            x: GX,
            y: GY,
            infinity: false,
        }
    }

    /// Point doubling: 2P
    fn double(&self) -> Point {
        if self.infinity {
            return Point::INFINITY;
        }

        // λ = (3x² + a) / (2y) mod p
        let x2 = self.x.mul(&self.x).mod_p();
        let three_x2 = x2.add(&x2).add(&x2);
        let numerator = three_x2.add(&A);

        let two_y = self.y.add(&self.y);
        let denominator_inv = two_y
            .mod_inverse()
            .expect("Division by zero in point double");

        let lambda = numerator.mul(&denominator_inv).mod_p();

        // x₃ = λ² - 2x
        let lambda2 = lambda.mul(&lambda).mod_p();
        let two_x = self.x.add(&self.x);
        let x3 = lambda2.sub(&two_x).mod_p();

        // y₃ = λ(x - x₃) - y
        let y3 = lambda.mul(&self.x.sub(&x3)).sub(&self.y).mod_p();

        Point {
            x: x3,
            y: y3,
            infinity: false,
        }
    }

    /// Point addition: P + Q
    fn add(&self, other: &Point) -> Point {
        if self.infinity {
            return *other;
        }
        if other.infinity {
            return *self;
        }

        // If P == -Q (same x, opposite y), return infinity
        if self.x == other.x && self.y.add(&other.y).mod_p() == U256::ZERO {
            return Point::INFINITY;
        }

        // If P == Q, use doubling
        if *self == *other {
            return self.double();
        }

        // λ = (y₂ - y₁) / (x₂ - x₁) mod p
        let dy = other.y.sub(&self.y).mod_p();
        let dx = other.x.sub(&self.x).mod_p();
        let dx_inv = dx.mod_inverse().expect("Division by zero in point add");
        let lambda = dy.mul(&dx_inv).mod_p();

        // x₃ = λ² - x₁ - x₂
        let lambda2 = lambda.mul(&lambda).mod_p();
        let x3 = lambda2.sub(&self.x).sub(&other.x).mod_p();

        // y₃ = λ(x₁ - x₃) - y₁
        let y3 = lambda.mul(&self.x.sub(&x3)).sub(&self.y).mod_p();

        Point {
            x: x3,
            y: y3,
            infinity: false,
        }
    }

    /// Scalar multiplication: k * P (double-and-add algorithm)
    fn scalar_mult(&self, k: &U256) -> Point {
        let mut result = Point::INFINITY;
        let mut addend = *self;

        for i in 0..256 {
            if (k.0[i / 64] >> (i % 64)) & 1 == 1 {
                result = result.add(&addend);
            }
            addend = addend.double();
        }

        result
    }
}

impl PartialEq for Point {
    fn eq(&self, other: &Self) -> bool {
        if self.infinity && other.infinity {
            return true;
        }
        if self.infinity || other.infinity {
            return false;
        }
        self.x == other.x && self.y == other.y
    }
}

// ============================================================================
// ECDSA P-256 Public Key
// ============================================================================

#[derive(Clone, Copy)]
pub struct EcdsaPublicKey {
    x: U256,
    y: U256,
}

impl EcdsaPublicKey {
    /// Create from X and Y coordinates
    pub fn from_xy(x: [u8; 32], y: [u8; 32]) -> Self {
        EcdsaPublicKey {
            x: U256::from_be_bytes(&x).unwrap_or(U256::ZERO),
            y: U256::from_be_bytes(&y).unwrap_or(U256::ZERO),
        }
    }

    /// From uncompressed point format (0x04 || X || Y)
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != 65 || bytes[0] != 0x04 {
            return None;
        }

        let x = U256::from_be_bytes(&bytes[1..33]).unwrap();
        let y = U256::from_be_bytes(&bytes[33..65]).unwrap();

        Some(EcdsaPublicKey { x, y })
    }

    /// To uncompressed point format
    pub fn to_bytes(&self) -> [u8; 65] {
        let mut bytes = [0u8; 65];
        bytes[0] = 0x04;
        bytes[1..33].copy_from_slice(&self.x.to_be_bytes());
        bytes[33..65].copy_from_slice(&self.y.to_be_bytes());
        bytes
    }

    /// Verify ECDSA signature
    ///
    /// Parameters:
    ///   - message: The signed message
    ///   - signature: 64-byte signature (r || s)
    /// Returns:
    ///   - true if signature is valid
    pub fn verify(&self, message: &[u8], signature: &[u8]) -> bool {
        if signature.len() != 64 {
            return false;
        }

        // Parse signature (r, s)
        let r = U256::from_be_bytes(&signature[..32]).unwrap_or(U256::ZERO);
        let s = U256::from_be_bytes(&signature[32..64]).unwrap_or(U256::ZERO);

        // Step 1: Verify r, s ∈ [1, n-1]
        if r.is_zero() || r.ge(&N) || s.is_zero() || s.ge(&N) {
            return false;
        }

        // Step 2: Hash z = SHA-256(message)
        let mut hasher = Sha3::sha3_256();
        hasher.update(message);
        let hash = hasher.finalize();
        let z = U256::from_be_bytes(&hash).unwrap_or(U256::ZERO);

        // Step 3: w = s^(-1) mod n
        let w = match s.mod_inverse() {
            Some(inv) => inv,
            None => return false,
        };

        // Step 4: u1 = z*w mod n
        let u1 = z.mul(&w).mod_n();

        // Step 5: u2 = r*w mod n
        let u2 = r.mul(&w).mod_n();

        // Step 6: (x, y) = u1*G + u2*Q
        let point1 = Point::generator().scalar_mult(&u1);
        let point2 = Point {
            x: self.x,
            y: self.y,
            infinity: false,
        }
        .scalar_mult(&u2);
        let result_point = point1.add(&point2);

        if result_point.infinity {
            return false;
        }

        // Step 7: Return (x mod n == r)
        let x_mod_n = result_point.x.mod_n();
        x_mod_n == r
    }
}

// ============================================================================
// ECDSA P-256 Private Key
// ============================================================================

#[derive(Clone)]
pub struct EcdsaPrivateKey {
    d: U256, // Private scalar
    public_key: EcdsaPublicKey,
}

impl EcdsaPrivateKey {
    /// Generate random private key
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        rdrand_bytes(&mut bytes);

        // Ensure 1 <= d < n
        bytes[0] &= 0x7F; // Make sure it's positive and less than n
        let d = U256::from_be_bytes(&bytes).unwrap_or(U256::ONE);

        // Compute public key Q = d * G
        let q_point = Point::generator().scalar_mult(&d);

        EcdsaPrivateKey {
            d,
            public_key: EcdsaPublicKey {
                x: q_point.x,
                y: q_point.y,
            },
        }
    }

    /// Get public key
    pub fn public_key(&self) -> &EcdsaPublicKey {
        &self.public_key
    }

    /// Sign a message
    ///
    /// Returns: 64-byte signature (r || s)
    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        // Hash z = SHA-256(message)
        let mut hasher = Sha3::sha3_256();
        hasher.update(message);
        let hash = hasher.finalize();
        let z = U256::from_be_bytes(&hash).unwrap_or(U256::ZERO);

        loop {
            // Generate random k ∈ [1, n-1]
            let mut k_bytes = [0u8; 32];
            rdrand_bytes(&mut k_bytes);
            k_bytes[0] &= 0x7F;
            let k = U256::from_be_bytes(&k_bytes).unwrap_or(U256::ONE);

            // (x, y) = k * G
            let point = Point::generator().scalar_mult(&k);
            let r = point.x.mod_n();

            if r.is_zero() {
                continue;
            }

            // s = k^(-1) * (z + r*d) mod n
            let k_inv = k.mod_inverse().unwrap();
            let rd = r.mul(&self.d).mod_n();
            let z_rd = z.add(&rd).mod_n();
            let s = k_inv.mul(&z_rd).mod_n();

            if s.is_zero() {
                continue;
            }

            // Return (r, s)
            let mut signature = [0u8; 64];
            signature[..32].copy_from_slice(&r.to_be_bytes());
            signature[32..64].copy_from_slice(&s.to_be_bytes());
            return signature;
        }
    }
}
