//! # BLAKE3 Hash Function
//!
//! Fast and secure hash function based on BLAKE2.

use alloc::vec::Vec;

const BLAKE3_BLOCK_SIZE: usize = 64;
const BLAKE3_KEY_LEN: usize = 32;
const BLAKE3_OUT_LEN: usize = 32;

/// BLAKE3 IV
const IV: [u32; 8] = [
    0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A,
    0x510E527F, 0x9B05688C, 0x1F83D9AB, 0x5BE0CD19,
];

/// BLAKE3 permutation
const MSG_PERM: [usize; 16] = [2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8];

/// BLAKE3 round constants
const ROUNDS: usize = 7;

/// BLAKE3 chunk state
#[derive(Clone, Copy)]
struct ChunkState {
    cv: [u32; 8],
    counter: u64,
    block: [u32; 16],
    block_len: usize,
    blocks_compressed: usize,
    flags: u32,
}

/// BLAKE3 hash function
pub struct Blake3 {
    key: [u8; 32],
    chunk_state: ChunkState,
    cv_stack: [[u32; 8]; 54],
    cv_stack_len: usize,
    flags: u32,
}

// Flags
const CHUNK_START: u32 = 1 << 0;
const CHUNK_END: u32 = 1 << 1;
const PARENT: u32 = 1 << 2;
const ROOT: u32 = 1 << 3;
const KEYED_HASH: u32 = 1 << 4;
const DERIVE_KEY_CONTEXT: u32 = 1 << 5;
const DERIVE_KEY_MATERIAL: u32 = 1 << 6;

impl Blake3 {
    /// Create default BLAKE3 hasher
    pub fn new() -> Self {
        Self::new_keyed(&[0u8; 32])
    }

    /// Create keyed BLAKE3 hasher
    pub fn new_keyed(key: &[u8; 32]) -> Self {
        let cv = Self::words_from_bytes(key);
        
        Blake3 {
            key: *key,
            chunk_state: ChunkState {
                cv,
                counter: 0,
                block: [0u32; 16],
                block_len: 0,
                blocks_compressed: 0,
                flags: CHUNK_START,
            },
            cv_stack: [[0u32; 8]; 54],
            cv_stack_len: 0,
            flags: 0,
        }
    }

    /// Create BLAKE3 for key derivation
    pub fn new_derive_key(context: &[u8]) -> Self {
        let mut hasher = Blake3::new();
        hasher.flags = DERIVE_KEY_CONTEXT;
        hasher.update(context);
        
        let mut key = [0u8; 32];
        hasher.finalize_into(&mut key);
        
        let mut result = Blake3::new_keyed(&key);
        result.flags = DERIVE_KEY_MATERIAL;
        result
    }

    /// Update with data
    pub fn update(&mut self, data: &[u8]) {
        let mut remaining = data;
        
        while !remaining.is_empty() {
            let take = BLAKE3_BLOCK_SIZE - self.chunk_state.block_len;
            let take = take.min(remaining.len());
            
            // Copy to block
            let block_bytes = Self::bytes_from_words(&self.chunk_state.block);
            for i in 0..take {
                self.chunk_state.block[self.chunk_state.block_len + i] = remaining[i] as u32;
            }
            self.chunk_state.block_len += take;
            remaining = &remaining[take..];
            
            if self.chunk_state.block_len == BLAKE3_BLOCK_SIZE {
                self.compress_chunk();
                self.chunk_state.block_len = 0;
                self.chunk_state.flags &= !CHUNK_START;
            }
        }
    }

    /// Finalize and get hash
    pub fn finalize(&self) -> [u8; 32] {
        let mut output = [0u8; 32];
        self.finalize_into(&mut output);
        output
    }

    /// Finalize into slice
    pub fn finalize_into(&self, output: &mut [u8]) {
        let mut cv = self.chunk_state.cv;
        
        // Pad remaining block
        let mut block = self.chunk_state.block;
        let block_len = self.chunk_state.block_len;
        
        // Final compress
        let flags = self.chunk_state.flags | CHUNK_END | ROOT;
        cv = Self::compress(&cv, &block, self.chunk_state.counter, flags);
        
        // Output
        for i in 0..8 {
            let bytes = cv[i].to_le_bytes();
            let offset = i * 4;
            if offset + 4 <= output.len() {
                output[offset..offset + 4].copy_from_slice(&bytes);
            }
        }
    }

    /// Finalize with extended output
    pub fn finalize_xof(&self, output_len: usize) -> Vec<u8> {
        let mut output = Vec::with_capacity(output_len);
        let mut cv = self.chunk_state.cv;
        let mut counter = 0u64;
        
        while output.len() < output_len {
            let flags = self.chunk_state.flags | CHUNK_END | ROOT;
            let block = Self::compress_block(&cv, counter, flags);
            
            for word in block.iter() {
                if output.len() + 4 <= output_len {
                    output.extend_from_slice(&word.to_le_bytes());
                } else {
                    let remaining = output_len - output.len();
                    let bytes = word.to_le_bytes();
                    output.extend_from_slice(&bytes[..remaining]);
                }
            }
            counter += 1;
        }
        
        output
    }

    fn compress_chunk(&mut self) {
        let cv = Self::compress(
            &self.chunk_state.cv,
            &self.chunk_state.block,
            self.chunk_state.counter,
            self.chunk_state.flags,
        );
        
        self.chunk_state.cv = cv;
        self.chunk_state.counter += 1;
        self.chunk_state.blocks_compressed += 1;
    }

    fn compress(cv: &[u32; 8], block: &[u32; 16], counter: u64, flags: u32) -> [u32; 8] {
        let mut state = [
            cv[0], cv[1], cv[2], cv[3], cv[4], cv[5], cv[6], cv[7],
            IV[0], IV[1], IV[2], IV[3],
            (counter as u32), ((counter >> 32) as u32), 0, flags,
        ];
        
        // Rounds
        for _ in 0..ROUNDS {
            // Column mixing
            Self::g(&mut state, 0, 4, 8, 12, block[0], block[1]);
            Self::g(&mut state, 2, 6, 10, 14, block[2], block[3]);
            Self::g(&mut state, 3, 7, 11, 15, block[4], block[5]);
            Self::g(&mut state, 1, 5, 9, 13, block[6], block[7]);
            
            // Diagonal mixing
            Self::g(&mut state, 0, 5, 10, 15, block[8], block[9]);
            Self::g(&mut state, 1, 6, 11, 12, block[10], block[11]);
            Self::g(&mut state, 2, 7, 8, 13, block[12], block[13]);
            Self::g(&mut state, 3, 4, 9, 14, block[14], block[15]);
        }
        
        // Finalize
        [
            state[0] ^ state[8],
            state[1] ^ state[9],
            state[2] ^ state[10],
            state[3] ^ state[11],
            state[4] ^ state[12],
            state[5] ^ state[13],
            state[6] ^ state[14],
            state[7] ^ state[15],
        ]
    }

    fn compress_block(cv: &[u32; 8], counter: u64, flags: u32) -> [u32; 16] {
        let mut state = [
            cv[0], cv[1], cv[2], cv[3], cv[4], cv[5], cv[6], cv[7],
            IV[0], IV[1], IV[2], IV[3],
            (counter as u32), ((counter >> 32) as u32), 0, flags,
        ];
        
        for _ in 0..ROUNDS {
            Self::g(&mut state, 0, 4, 8, 12, 0, 0);
            Self::g(&mut state, 2, 6, 10, 14, 0, 0);
            Self::g(&mut state, 3, 7, 11, 15, 0, 0);
            Self::g(&mut state, 1, 5, 9, 13, 0, 0);
            
            Self::g(&mut state, 0, 5, 10, 15, 0, 0);
            Self::g(&mut state, 1, 6, 11, 12, 0, 0);
            Self::g(&mut state, 2, 7, 8, 13, 0, 0);
            Self::g(&mut state, 3, 4, 9, 14, 0, 0);
        }
        
        let mut output = [0u32; 16];
        for i in 0..8 {
            output[i] = state[i] ^ state[i + 8];
            output[i + 8] = state[i + 8] ^ cv[i];
        }
        output
    }

    fn g(state: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize, mx: u32, my: u32) {
        state[a] = state[a].wrapping_add(state[b]).wrapping_add(mx);
        state[d] = (state[d] ^ state[a]).rotate_left(16);
        state[c] = state[c].wrapping_add(state[d]);
        state[b] = (state[b] ^ state[c]).rotate_left(12);
        state[a] = state[a].wrapping_add(state[b]).wrapping_add(my);
        state[d] = (state[d] ^ state[a]).rotate_left(8);
        state[c] = state[c].wrapping_add(state[d]);
        state[b] = (state[b] ^ state[c]).rotate_left(7);
    }

    fn words_from_bytes(bytes: &[u8; 32]) -> [u32; 8] {
        let mut words = [0u32; 8];
        for i in 0..8 {
            words[i] = u32::from_le_bytes([bytes[i * 4], bytes[i * 4 + 1], bytes[i * 4 + 2], bytes[i * 4 + 3]]);
        }
        words
    }

    fn bytes_from_words(words: &[u32; 16]) -> [u8; 64] {
        let mut bytes = [0u8; 64];
        for i in 0..16 {
            let word_bytes = words[i].to_le_bytes();
            bytes[i * 4..i * 4 + 4].copy_from_slice(&word_bytes);
        }
        bytes
    }
}

impl Default for Blake3 {
    fn default() -> Self {
        Self::new()
    }
}

/// BLAKE3 hash convenience function
pub fn blake3_hash(data: &[u8]) -> [u8; 32] {
    let mut hasher = Blake3::new();
    hasher.update(data);
    hasher.finalize()
}

/// BLAKE3 keyed MAC
pub fn blake3_mac(key: &[u8; 32], data: &[u8]) -> [u8; 32] {
    let mut hasher = Blake3::new_keyed(key);
    hasher.update(data);
    hasher.finalize()
}
