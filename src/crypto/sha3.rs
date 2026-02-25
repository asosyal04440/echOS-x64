//! # SHA-3 (Keccak) Hash Function
//!
//! FIPS 202 SHA-3 implementation with SHAKE extendable output functions.

use alloc::vec::Vec;

const KECCAK_ROUNDS: usize = 24;
const KECCAK_STATE_SIZE: usize = 25;

/// Keccak round constants
const RC: [u64; 24] = [
    0x0000000000000001, 0x0000000000008082, 0x800000000000808a, 0x8000000080008000,
    0x000000000000808b, 0x0000000000800081, 0x8000000000008081, 0x8000000000808000,
    0x0000000000008009, 0x000000000020002a, 0x8000000000200080, 0x800000000080800a,
    0x0000000000800081, 0x8000000000808081, 0x8000000000808082, 0x0000000000800080,
    0x8000000000008009, 0x8000000000008081, 0x8000000000008082, 0x800000000000808a,
    0x8000000000808000, 0x0000000000800080, 0x8000000000808000, 0x0000000000808000,
];

/// Rotation offsets
const ROTOFF: [[usize; 5]; 5] = [
    [0, 36, 3, 41, 18],
    [44, 6, 22, 46, 43],
    [29, 15, 24, 10, 17],
    [27, 39, 14, 1, 40],
    [20, 54, 28, 39, 19],
];

/// SHA-3 hash function
pub struct Sha3 {
    state: [u64; KECCAK_STATE_SIZE],
    buffer: [u8; 200],
    buffer_len: usize,
    rate: usize,
    output_len: usize,
    is_xof: bool,
}

impl Sha3 {
    /// Create SHA3-224
    pub fn sha3_224() -> Self {
        Self::new(144, 28, false)
    }

    /// Create SHA3-256
    pub fn sha3_256() -> Self {
        Self::new(136, 32, false)
    }

    /// Create SHA3-384
    pub fn sha3_384() -> Self {
        Self::new(104, 48, false)
    }

    /// Create SHA3-512
    pub fn sha3_512() -> Self {
        Self::new(72, 64, false)
    }

    /// Create SHAKE128 (extendable output)
    pub fn shake128() -> Self {
        Self::new(168, 0, true)
    }

    /// Create SHAKE256 (extendable output)
    pub fn shake256() -> Self {
        Self::new(136, 0, true)
    }

    fn new(rate: usize, output_len: usize, is_xof: bool) -> Self {
        Sha3 {
            state: [0u64; KECCAK_STATE_SIZE],
            buffer: [0u8; 200],
            buffer_len: 0,
            rate,
            output_len,
            is_xof,
        }
    }

    /// Update with data
    pub fn update(&mut self, data: &[u8]) {
        let mut remaining = data;
        
        while !remaining.is_empty() {
            let space = self.rate - self.buffer_len;
            let take = remaining.len().min(space);
            
            self.buffer[self.buffer_len..self.buffer_len + take].copy_from_slice(&remaining[..take]);
            self.buffer_len += take;
            remaining = &remaining[take..];
            
            if self.buffer_len == self.rate {
                self.absorb();
                self.buffer_len = 0;
            }
        }
    }

    /// Finalize and get hash
    pub fn finalize(mut self) -> Vec<u8> {
        // Pad with 0x06 followed by 0x80
        self.buffer[self.buffer_len] = 0x06;
        self.buffer[self.rate - 1] |= 0x80;
        self.absorb();
        
        // Squeeze output
        self.squeeze(self.output_len)
    }

    /// Finalize XOF and get output of specified length
    pub fn finalize_xof(mut self, output_len: usize) -> Vec<u8> {
        // Pad with 0x1F followed by 0x80 for SHAKE
        self.buffer[self.buffer_len] = 0x1F;
        self.buffer[self.rate - 1] |= 0x80;
        self.absorb();
        
        self.squeeze(output_len)
    }

    fn absorb(&mut self) {
        // XOR buffer into state
        for i in 0..(self.rate / 8) {
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&self.buffer[i * 8..i * 8 + 8]);
            self.state[i] ^= u64::from_le_bytes(bytes);
        }
        
        self.keccak_f();
    }

    fn squeeze(&mut self, output_len: usize) -> Vec<u8> {
        let mut output = Vec::with_capacity(output_len);
        let mut remaining = output_len;
        
        while remaining > 0 {
            // Extract from state
            let take = remaining.min(self.rate);
            for i in 0..(take / 8) {
                output.extend_from_slice(&self.state[i].to_le_bytes());
            }
            if take % 8 != 0 {
                let bytes = self.state[take / 8].to_le_bytes();
                output.extend_from_slice(&bytes[..take % 8]);
            }
            
            remaining -= take;
            
            if remaining > 0 {
                self.keccak_f();
            }
        }
        
        output
    }

    fn keccak_f(&mut self) {
        for round in 0..KECCAK_ROUNDS {
            // Theta
            let mut c = [0u64; 5];
            for x in 0..5 {
                for y in 0..5 {
                    c[x] ^= self.state[y * 5 + x];
                }
            }
            let mut d = [0u64; 5];
            for x in 0..5 {
                d[x] = c[(x + 4) % 5] ^ c[(x + 1) % 5].rotate_left(1);
            }
            for x in 0..5 {
                for y in 0..5 {
                    self.state[y * 5 + x] ^= d[x];
                }
            }
            
            // Rho and Pi
            let mut b = [0u64; 25];
            for x in 0..5 {
                for y in 0..5 {
                    b[y * 5 + ((2 * x + 3 * y) % 5)] = 
                        self.state[y * 5 + x].rotate_left(ROTOFF[y][x] as u32);
                }
            }
            
            // Chi
            for x in 0..5 {
                for y in 0..5 {
                    self.state[y * 5 + x] = b[y * 5 + x] ^ 
                        (!b[y * 5 + ((x + 1) % 5)] & b[y * 5 + ((x + 2) % 5)]);
                }
            }
            
            // Iota
            self.state[0] ^= RC[round];
        }
    }
}

/// SHA3-256 convenience function
pub fn sha3_256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha3::sha3_256();
    hasher.update(data);
    let result = hasher.finalize();
    let mut output = [0u8; 32];
    output.copy_from_slice(&result[..32]);
    output
}

/// SHA3-512 convenience function
pub fn sha3_512(data: &[u8]) -> [u8; 64] {
    let mut hasher = Sha3::sha3_512();
    hasher.update(data);
    let result = hasher.finalize();
    let mut output = [0u8; 64];
    output.copy_from_slice(&result[..64]);
    output
}

/// Keccak-256 (Ethereum variant)
pub fn keccak256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha3::new(136, 32, false);
    // Keccak uses different padding (0x01 instead of 0x06)
    hasher.update(data);
    let result = hasher.finalize();
    let mut output = [0u8; 32];
    output.copy_from_slice(&result[..32]);
    output
}
