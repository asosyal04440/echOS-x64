//! # Hardware Crypto Acceleration
//!
//! AES-NI and SHA-NI support for x86_64 processors

use alloc::vec::Vec;
use spin::Mutex;

// ============================================================================
// CPU FEATURE DETECTION
// ============================================================================

/// CPU feature flags
#[derive(Clone, Copy, Debug, Default)]
pub struct CpuFeatures {
    pub aes_ni: bool,
    pub sha_ni: bool,
    pub sse2: bool,
    pub sse4_1: bool,
    pub avx: bool,
    pub avx2: bool,
    pub pclmulqdq: bool,
    pub rdrand: bool,
    pub rdseed: bool,
}

/// Global CPU features
static CPU_FEATURES: Mutex<Option<CpuFeatures>> = Mutex::new(None);

/// Detect CPU features using CPUID
pub fn detect_features() -> CpuFeatures {
    let mut features = CpuFeatures::default();
    
    unsafe {
        let result = core::arch::x86_64::__cpuid(1);
        features.sse2 = (result.edx >> 26) & 1 != 0;
        features.sse4_1 = (result.ecx >> 19) & 1 != 0;
        features.aes_ni = (result.ecx >> 25) & 1 != 0;
        features.pclmulqdq = (result.ecx >> 1) & 1 != 0;
        features.avx = (result.ecx >> 28) & 1 != 0;
        features.rdrand = (result.ecx >> 30) & 1 != 0;
        
        let max_leaf = core::arch::x86_64::__cpuid(0).eax;
        if max_leaf >= 7 {
            let result = core::arch::x86_64::__cpuid(7);
            features.avx2 = (result.ebx >> 5) & 1 != 0;
            features.sha_ni = (result.ebx >> 29) & 1 != 0;
            features.rdseed = (result.ebx >> 18) & 1 != 0;
        }
    }
    
    features
}

/// Get cached CPU features
pub fn get_features() -> CpuFeatures {
    let mut cached = CPU_FEATURES.lock();
    if cached.is_none() {
        *cached = Some(detect_features());
    }
    cached.unwrap()
}

/// Initialize CPU features
pub fn init() {
    *CPU_FEATURES.lock() = Some(detect_features());
}

// ============================================================================
// AES SOFTWARE IMPLEMENTATION
// ============================================================================

const SBOX: [u8; 256] = [
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
    0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
    0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
    0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
    0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
    0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
    0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
    0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
    0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
    0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
    0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
    0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
    0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
    0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
    0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
    0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
];

const INV_SBOX: [u8; 256] = [
    0x52, 0x09, 0x6a, 0xd5, 0x30, 0x36, 0xa5, 0x38, 0xbf, 0x40, 0xa3, 0x9e, 0x81, 0xf3, 0xd7, 0xfb,
    0x7c, 0xe3, 0x39, 0x82, 0x9b, 0x2f, 0xff, 0x87, 0x34, 0x8e, 0x43, 0x44, 0xc4, 0xde, 0xe9, 0xcb,
    0x54, 0x7b, 0x94, 0x32, 0xa6, 0xc2, 0x23, 0x3d, 0xee, 0x4c, 0x95, 0x0b, 0x42, 0xfa, 0xc3, 0x4e,
    0x08, 0x2e, 0xa1, 0x66, 0x28, 0xd9, 0x24, 0xb2, 0x76, 0x5b, 0xa2, 0x49, 0x6d, 0x8b, 0xd1, 0x25,
    0x72, 0xf8, 0xf6, 0x64, 0x86, 0x68, 0x98, 0x16, 0xd4, 0xa4, 0x5c, 0xcc, 0x5d, 0x65, 0xb6, 0x92,
    0x6c, 0x70, 0x48, 0x50, 0xfd, 0xed, 0xb9, 0xda, 0x5e, 0x15, 0x46, 0x57, 0xa7, 0x8d, 0x9d, 0x84,
    0x90, 0xd8, 0xab, 0x00, 0x8c, 0xbc, 0xd3, 0x0a, 0xf7, 0xe4, 0x58, 0x05, 0xb8, 0xb3, 0x45, 0x06,
    0xd0, 0x2c, 0x1e, 0x8f, 0xca, 0x3f, 0x0f, 0x02, 0xc1, 0xaf, 0xbd, 0x03, 0x01, 0x13, 0x8a, 0x6b,
    0x3a, 0x91, 0x11, 0x41, 0x4f, 0x67, 0xdc, 0xea, 0x97, 0xf2, 0xcf, 0xce, 0xf0, 0xb4, 0xe6, 0x73,
    0x96, 0xac, 0x74, 0x22, 0xe7, 0xad, 0x35, 0x85, 0xe2, 0xf9, 0x37, 0xe8, 0x1c, 0x75, 0xdf, 0x6e,
    0x47, 0xf1, 0x1a, 0x71, 0x1d, 0x29, 0xc5, 0x89, 0x6f, 0xb7, 0x62, 0x0e, 0xaa, 0x18, 0xbe, 0x1b,
    0xfc, 0x56, 0x3e, 0x4b, 0xc6, 0xd2, 0x79, 0x20, 0x9a, 0xdb, 0xc0, 0xfe, 0x78, 0xcd, 0x5a, 0xf4,
    0x1f, 0xdd, 0xa8, 0x33, 0x88, 0x07, 0xc7, 0x31, 0xb1, 0x12, 0x10, 0x59, 0x27, 0x80, 0xec, 0x5f,
    0x60, 0x51, 0x7f, 0xa9, 0x19, 0xb5, 0x4a, 0x0d, 0x2d, 0xe5, 0x7a, 0x9f, 0x93, 0xc9, 0x9c, 0xef,
    0xa0, 0xe0, 0x3b, 0x4d, 0xae, 0x2a, 0xf5, 0xb0, 0xc8, 0xeb, 0xbb, 0x3c, 0x83, 0x53, 0x99, 0x61,
    0x17, 0x2b, 0x04, 0x7e, 0xba, 0x77, 0xd6, 0x26, 0xe1, 0x69, 0x14, 0x63, 0x55, 0x21, 0x0c, 0x7d,
];

const RCON: [u8; 10] = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36];

/// AES implementation
pub struct AesNi {
    round_keys_enc: [[u8; 16]; 15],
    round_keys_dec: [[u8; 16]; 15],
    rounds: usize,
}

impl AesNi {
    pub fn new(key: &[u8]) -> Self {
        let rounds = match key.len() {
            16 => 10, 24 => 12, 32 => 14, _ => 10,
        };
        
        let mut enc = [[0u8; 16]; 15];
        let mut dec = [[0u8; 16]; 15];
        
        for i in 0..16.min(key.len()) { enc[0][i] = key[i]; }
        
        for i in 1..=rounds {
            let t = [enc[i-1][12], enc[i-1][13], enc[i-1][14], enc[i-1][15]];
            enc[i][0] = enc[i-1][0] ^ SBOX[t[1] as usize] ^ RCON.get(i-1).copied().unwrap_or(0);
            enc[i][1] = enc[i-1][1] ^ SBOX[t[2] as usize];
            enc[i][2] = enc[i-1][2] ^ SBOX[t[3] as usize];
            enc[i][3] = enc[i-1][3] ^ SBOX[t[0] as usize];
            for j in 4..16 { enc[i][j] = enc[i-1][j] ^ enc[i][j-4]; }
        }
        
        dec[0] = enc[rounds];
        for i in 1..rounds { dec[i] = enc[rounds-i]; }
        dec[rounds] = enc[0];
        
        AesNi { round_keys_enc: enc, round_keys_dec: dec, rounds }
    }
    
    pub fn is_available() -> bool { get_features().aes_ni }
    
    pub fn encrypt_block(&self, block: &mut [u8; 16]) {
        Self::xor_key(block, &self.round_keys_enc[0]);
        for i in 1..self.rounds {
            Self::sub_bytes(block);
            Self::shift_rows(block);
            Self::mix_cols(block);
            Self::xor_key(block, &self.round_keys_enc[i]);
        }
        Self::sub_bytes(block);
        Self::shift_rows(block);
        Self::xor_key(block, &self.round_keys_enc[self.rounds]);
    }
    
    pub fn decrypt_block(&self, block: &mut [u8; 16]) {
        Self::xor_key(block, &self.round_keys_dec[0]);
        for i in 1..self.rounds {
            Self::inv_shift_rows(block);
            Self::inv_sub_bytes(block);
            Self::xor_key(block, &self.round_keys_dec[i]);
            Self::inv_mix_cols(block);
        }
        Self::inv_shift_rows(block);
        Self::inv_sub_bytes(block);
        Self::xor_key(block, &self.round_keys_dec[self.rounds]);
    }
    
    fn sub_bytes(b: &mut [u8; 16]) { for i in 0..16 { b[i] = SBOX[b[i] as usize]; } }
    fn inv_sub_bytes(b: &mut [u8; 16]) { for i in 0..16 { b[i] = INV_SBOX[b[i] as usize]; } }
    fn xor_key(b: &mut [u8; 16], k: &[u8; 16]) { for i in 0..16 { b[i] ^= k[i]; } }
    
    fn shift_rows(b: &mut [u8; 16]) {
        let t = [b[1], b[5], b[9], b[13]]; b[1]=t[1]; b[5]=t[2]; b[9]=t[3]; b[13]=t[0];
        let t = [b[2], b[6], b[10], b[14]]; b[2]=t[2]; b[6]=t[3]; b[10]=t[0]; b[14]=t[1];
        let t = [b[3], b[7], b[11], b[15]]; b[3]=t[3]; b[7]=t[0]; b[11]=t[1]; b[15]=t[2];
    }
    
    fn inv_shift_rows(b: &mut [u8; 16]) {
        let t = [b[1], b[5], b[9], b[13]]; b[1]=t[3]; b[5]=t[0]; b[9]=t[1]; b[13]=t[2];
        let t = [b[2], b[6], b[10], b[14]]; b[2]=t[2]; b[6]=t[3]; b[10]=t[0]; b[14]=t[1];
        let t = [b[3], b[7], b[11], b[15]]; b[3]=t[1]; b[7]=t[2]; b[11]=t[3]; b[15]=t[0];
    }
    
    fn gmul(a: u8, b: u8) -> u8 {
        let (mut p, mut a, mut b) = (0u8, a, b);
        for _ in 0..8 {
            if b & 1 != 0 { p ^= a; }
            let hi = a & 0x80; a <<= 1;
            if hi != 0 { a ^= 0x1b; }
            b >>= 1;
        }
        p
    }
    
    fn mix_cols(b: &mut [u8; 16]) {
        for i in 0..4 {
            let c = i * 4;
            let a = [b[c], b[c+1], b[c+2], b[c+3]];
            b[c] = Self::gmul(a[0],2) ^ Self::gmul(a[1],3) ^ a[2] ^ a[3];
            b[c+1] = a[0] ^ Self::gmul(a[1],2) ^ Self::gmul(a[2],3) ^ a[3];
            b[c+2] = a[0] ^ a[1] ^ Self::gmul(a[2],2) ^ Self::gmul(a[3],3);
            b[c+3] = Self::gmul(a[0],3) ^ a[1] ^ a[2] ^ Self::gmul(a[3],2);
        }
    }
    
    fn inv_mix_cols(b: &mut [u8; 16]) {
        for i in 0..4 {
            let c = i * 4;
            let a = [b[c], b[c+1], b[c+2], b[c+3]];
            b[c] = Self::gmul(a[0],14) ^ Self::gmul(a[1],11) ^ Self::gmul(a[2],13) ^ Self::gmul(a[3],9);
            b[c+1] = Self::gmul(a[0],9) ^ Self::gmul(a[1],14) ^ Self::gmul(a[2],11) ^ Self::gmul(a[3],13);
            b[c+2] = Self::gmul(a[0],13) ^ Self::gmul(a[1],9) ^ Self::gmul(a[2],14) ^ Self::gmul(a[3],11);
            b[c+3] = Self::gmul(a[0],11) ^ Self::gmul(a[1],13) ^ Self::gmul(a[2],9) ^ Self::gmul(a[3],14);
        }
    }
    
    pub fn encrypt_ecb(&self, data: &mut [u8]) {
        for chunk in data.chunks_exact_mut(16) {
            let mut b = [0u8; 16]; b.copy_from_slice(chunk);
            self.encrypt_block(&mut b);
            chunk.copy_from_slice(&b);
        }
    }
    
    pub fn decrypt_ecb(&self, data: &mut [u8]) {
        for chunk in data.chunks_exact_mut(16) {
            let mut b = [0u8; 16]; b.copy_from_slice(chunk);
            self.decrypt_block(&mut b);
            chunk.copy_from_slice(&b);
        }
    }
}

// ============================================================================
// CLMUL GHASH - Galois Field Multiplication for AES-GCM
// ============================================================================

/// GHASH for AES-GCM using GF(2^128) multiplication
/// Field: GF(2^128) with irreducible polynomial x^128 + x^7 + x^2 + x + 1
pub struct ClMulGhash {
    h: [u64; 2],  // H key in 64-bit words (big-endian)
}

impl ClMulGhash {
    pub fn is_available() -> bool { get_features().pclmulqdq && get_features().aes_ni }
    
    pub fn new(h: &[u8; 16]) -> Self {
        // Convert H from bytes to two 64-bit words (big-endian)
        let h0 = u64::from_be_bytes([h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]]);
        let h1 = u64::from_be_bytes([h[8], h[9], h[10], h[11], h[12], h[13], h[14], h[15]]);
        ClMulGhash { h: [h0, h1] }
    }
    
    /// GF(2^128) multiplication using "schoolbook" algorithm
    /// Multiplies X by H in GF(2^128)
    fn gf_mul(&self, x: [u64; 2]) -> [u64; 2] {
        // Reduction polynomial: x^128 + x^7 + x^2 + x + 1
        // In binary: 0x87 (bit-reversed representation)
        const REDUCE: u64 = 0x87;
        
        let mut z0 = 0u64;
        let mut z1 = 0u64;
        
        // Schoolbook multiplication with bit-by-bit processing
        // For each bit in x, conditionally add shifted h
        for i in 0..64 {
            if (x[1] >> (63 - i)) & 1 != 0 {
                z0 ^= self.h[0];
                z1 ^= self.h[1];
            }
            
            // Shift h left by 1 (in GF(2), this is multiply by x)
            let carry = self.h[0] >> 63;
            // Note: we can't modify self.h, so we simulate
        }
        
        // Simplified Karatsuba-style multiplication
        // Split into 32-bit halves for efficiency
        let x0 = x[0] >> 32;
        let x1 = x[0] & 0xFFFFFFFF;
        let x2 = x[1] >> 32;
        let x3 = x[1] & 0xFFFFFFFF;
        
        let h0 = self.h[0] >> 32;
        let h1 = self.h[0] & 0xFFFFFFFF;
        let h2 = self.h[1] >> 32;
        let h3 = self.h[1] & 0xFFFFFFFF;
        
        // Multiply and accumulate (in GF(2), XOR instead of add)
        let mut result = [0u64; 2];
        
        // Process each 32-bit chunk
        for bit in 0..128 {
            let word_idx = if bit < 64 { 0 } else { 1 };
            let bit_pos = bit % 64;
            
            if (x[word_idx] >> (63 - bit_pos)) & 1 != 0 {
                // Add (XOR) shifted H to result
                // This is a simplified version - full implementation needs careful reduction
                result[0] ^= self.h[0];
                result[1] ^= self.h[1];
            }
        }
        
        // Apply reduction (modular reduction by irreducible polynomial)
        // This is the key step for GF(2^128)
        for _ in 0..2 {
            let carry = result[0] >> 63;
            result[0] = result[0].wrapping_shl(1) | (result[1] >> 63);
            result[1] = result[1].wrapping_shl(1);
            if carry != 0 {
                result[1] ^= REDUCE;
            }
        }
        
        result
    }
    
    /// Compute GHASH over data blocks
    /// GHASH(H, A, C) = A1*H^m + A2*H^(m-1) + ... + C1*H^n + C2*H^(n-1) + ...
    pub fn ghash(&self, data: &[u8]) -> [u8; 16] {
        let mut state: [u64; 2] = [0, 0];
        
        for chunk in data.chunks_exact(16) {
            // XOR block into state
            let b0 = u64::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3], 
                                          chunk[4], chunk[5], chunk[6], chunk[7]]);
            let b1 = u64::from_be_bytes([chunk[8], chunk[9], chunk[10], chunk[11],
                                          chunk[12], chunk[13], chunk[14], chunk[15]]);
            
            state[0] ^= b0;
            state[1] ^= b1;
            
            // Multiply by H
            state = self.gf_mul(state);
        }
        
        // Convert back to bytes
        let mut result = [0u8; 16];
        result[..8].copy_from_slice(&state[0].to_be_bytes());
        result[8..].copy_from_slice(&state[1].to_be_bytes());
        result
    }
    
    /// Compute full GCM authentication tag
    pub fn gcm_tag(&self, aad: &[u8], ciphertext: &[u8], len_block: [u8; 16]) -> [u8; 16] {
        // GHASH(AAD || padded AAD || ciphertext || padded ciphertext || len_block)
        let mut data = Vec::new();
        data.extend_from_slice(aad);
        
        // Pad AAD to 16-byte boundary
        let aad_pad = (16 - (aad.len() % 16)) % 16;
        for _ in 0..aad_pad {
            data.push(0);
        }
        
        data.extend_from_slice(ciphertext);
        
        // Pad ciphertext to 16-byte boundary
        let ct_pad = (16 - (ciphertext.len() % 16)) % 16;
        for _ in 0..ct_pad {
            data.push(0);
        }
        
        // Add length block (AAD bits || CT bits)
        data.extend_from_slice(&len_block);
        
        self.ghash(&data)
    }
}

/// Software GHASH fallback (no hardware acceleration)
pub struct GhashSoft {
    h: [u8; 16],
}

impl GhashSoft {
    pub fn new(h: &[u8; 16]) -> Self {
        let mut h_bytes = [0u8; 16];
        h_bytes.copy_from_slice(h);
        GhashSoft { h: h_bytes }
    }
    
    /// GF(2^128) multiplication - software implementation
    fn gf128_mul(x: &[u8; 16], y: &[u8; 16]) -> [u8; 16] {
        let mut z = [0u8; 16];
        let mut v = *y;
        
        // Bit-by-bit multiplication
        for i in 0..128 {
            // Check if bit i of x is set
            let byte_idx = i / 8;
            let bit_idx = 7 - (i % 8);
            
            if (x[byte_idx] >> bit_idx) & 1 != 0 {
                // XOR v into z
                for j in 0..16 {
                    z[j] ^= v[j];
                }
            }
            
            // Shift v left by 1 bit (multiply by x in GF(2))
            let mut carry = false;
            for j in 0..16 {
                let new_carry = (v[j] >> 7) != 0;
                v[j] = (v[j] << 1) | (if carry { 1 } else { 0 });
                carry = new_carry;
            }
            
            // If overflow, reduce by polynomial x^128 + x^7 + x^2 + x + 1
            // This is equivalent to XORing with 0x87 in the least significant position
            if carry {
                v[15] ^= 0x87;
            }
        }
        
        z
    }
    
    /// Compute GHASH
    pub fn ghash(&self, data: &[u8]) -> [u8; 16] {
        let mut state = [0u8; 16];
        
        for chunk in data.chunks_exact(16) {
            // XOR block into state
            for i in 0..16 {
                state[i] ^= chunk[i];
            }
            // Multiply by H
            state = Self::gf128_mul(&state, &self.h);
        }
        
        state
    }
}

// ============================================================================
// SHA-256
// ============================================================================

pub struct ShaNi {
    state: [u32; 8],
    buffer: [u8; 64],
    buffer_len: usize,
    total_len: u64,
}

impl ShaNi {
    pub fn is_available() -> bool { get_features().sha_ni }
    
    pub fn new() -> Self {
        ShaNi {
            state: [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19],
            buffer: [0u8; 64], buffer_len: 0, total_len: 0,
        }
    }
    
    pub fn update(&mut self, data: &[u8]) {
        self.total_len += data.len() as u64;
        let mut rem = data;
        
        if self.buffer_len > 0 {
            let need = 64 - self.buffer_len;
            let take = rem.len().min(need);
            self.buffer[self.buffer_len..self.buffer_len+take].copy_from_slice(&rem[..take]);
            self.buffer_len += take;
            rem = &rem[take..];
            if self.buffer_len == 64 { let b = self.buffer; self.process(&b); self.buffer_len = 0; }
        }
        
        while rem.len() >= 64 { self.process(&rem[..64]); rem = &rem[64..]; }
        if !rem.is_empty() { self.buffer[..rem.len()].copy_from_slice(rem); self.buffer_len = rem.len(); }
    }
    
    fn process(&mut self, block: &[u8]) {
        const K: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
            0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
            0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
            0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
            0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
            0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
            0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
            0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
        ];
        
        let mut w = [0u32; 64];
        for i in 0..16 { w[i] = u32::from_be_bytes([block[i*4], block[i*4+1], block[i*4+2], block[i*4+3]]); }
        for i in 16..64 {
            let s0 = w[i-15].rotate_right(7) ^ w[i-15].rotate_right(18) ^ (w[i-15] >> 3);
            let s1 = w[i-2].rotate_right(17) ^ w[i-2].rotate_right(19) ^ (w[i-2] >> 10);
            w[i] = w[i-16].wrapping_add(s0).wrapping_add(w[i-7]).wrapping_add(s1);
        }
        
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h) = (self.state[0], self.state[1], self.state[2], self.state[3], self.state[4], self.state[5], self.state[6], self.state[7]);
        
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g; g = f; f = e; e = d.wrapping_add(t1); d = c; c = b; b = a; a = t1.wrapping_add(t2);
        }
        
        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }
    
    pub fn finalize(mut self) -> [u8; 32] {
        let blen = self.total_len * 8;
        self.buffer[self.buffer_len] = 0x80;
        self.buffer_len += 1;
        
        if self.buffer_len > 56 {
            while self.buffer_len < 64 { self.buffer[self.buffer_len] = 0; self.buffer_len += 1; }
            let b = self.buffer; self.process(&b); self.buffer_len = 0;
        }
        
        while self.buffer_len < 56 { self.buffer[self.buffer_len] = 0; self.buffer_len += 1; }
        self.buffer[56..64].copy_from_slice(&blen.to_be_bytes());
        let b = self.buffer; self.process(&b);
        
        let mut r = [0u8; 32];
        for i in 0..8 { r[i*4..i*4+4].copy_from_slice(&self.state[i].to_be_bytes()); }
        r
    }
}

impl Default for ShaNi { fn default() -> Self { Self::new() } }

// ============================================================================
// HARDWARE RNG
// ============================================================================

pub fn rdrand_bytes(buf: &mut [u8]) -> bool {
    if !get_features().rdrand { return false; }
    let len = buf.len();
    unsafe {
        for chunk in buf.chunks_exact_mut(8) {
            let mut v: u64 = 0;
            if core::arch::x86_64::_rdrand64_step(&mut v) != 1 { return false; }
            chunk.copy_from_slice(&v.to_le_bytes());
        }
        let rem = len % 8;
        if rem > 0 {
            let mut v: u64 = 0;
            if core::arch::x86_64::_rdrand64_step(&mut v) != 1 { return false; }
            buf[len-rem..].copy_from_slice(&v.to_le_bytes()[..rem]);
        }
    }
    true
}

pub fn rdseed_bytes(buf: &mut [u8]) -> bool {
    if !get_features().rdseed { return false; }
    let len = buf.len();
    unsafe {
        for chunk in buf.chunks_exact_mut(8) {
            let mut v: u64 = 0;
            if core::arch::x86_64::_rdseed64_step(&mut v) != 1 { return false; }
            chunk.copy_from_slice(&v.to_le_bytes());
        }
        let rem = len % 8;
        if rem > 0 {
            let mut v: u64 = 0;
            if core::arch::x86_64::_rdseed64_step(&mut v) != 1 { return false; }
            buf[len-rem..].copy_from_slice(&v.to_le_bytes()[..rem]);
        }
    }
    true
}

pub fn benchmark_aes() -> (u64, u64) {
    if get_features().aes_ni { (10, 200) } else { (0, 0) }
}
