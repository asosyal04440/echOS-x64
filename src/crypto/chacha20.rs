//! # ChaCha20 Stream Cipher
//!
//! ChaCha20-Poly1305 AEAD implementation.

use alloc::vec::Vec;

const CHACHA20_ROUNDS: usize = 20;

/// ChaCha20 quarter round constants ("expa", "nd 3", "2-by", "te k")
const CONSTANTS: [u32; 4] = [0x61707865, 0x3320646e, 0x79622d32, 0x6b206574];

/// ChaCha20 cipher
pub struct ChaCha20 {
    state: [u32; 16],
    counter: u64,
    buffer: [u8; 64],
    buffer_pos: usize,
}

impl ChaCha20 {
    /// Create new ChaCha20 cipher with key and nonce
    pub fn new(key: &[u8; 32], nonce: &[u8; 12]) -> Self {
        let mut state = [0u32; 16];
        
        // Constants
        state[0] = CONSTANTS[0];
        state[1] = CONSTANTS[1];
        state[2] = CONSTANTS[2];
        state[3] = CONSTANTS[3];
        
        // Key
        for i in 0..8 {
            state[4 + i] = u32::from_le_bytes([
                key[i * 4],
                key[i * 4 + 1],
                key[i * 4 + 2],
                key[i * 4 + 3],
            ]);
        }
        
        // Counter (starts at 0)
        state[12] = 0;
        
        // Nonce
        state[13] = u32::from_le_bytes([nonce[0], nonce[1], nonce[2], nonce[3]]);
        state[14] = u32::from_le_bytes([nonce[4], nonce[5], nonce[6], nonce[7]]);
        state[15] = u32::from_le_bytes([nonce[8], nonce[9], nonce[10], nonce[11]]);
        
        ChaCha20 {
            state,
            counter: 0,
            buffer: [0u8; 64],
            buffer_pos: 64,
        }
    }

    /// Create with 8-byte nonce (legacy IETF version)
    pub fn new_ietf(key: &[u8; 32], nonce: &[u8; 8], counter: u32) -> Self {
        let mut state = [0u32; 16];
        
        state[0] = CONSTANTS[0];
        state[1] = CONSTANTS[1];
        state[2] = CONSTANTS[2];
        state[3] = CONSTANTS[3];
        
        for i in 0..8 {
            state[4 + i] = u32::from_le_bytes([
                key[i * 4],
                key[i * 4 + 1],
                key[i * 4 + 2],
                key[i * 4 + 3],
            ]);
        }
        
        state[12] = counter;
        state[13] = 0;
        state[14] = u32::from_le_bytes([nonce[0], nonce[1], nonce[2], nonce[3]]);
        state[15] = u32::from_le_bytes([nonce[4], nonce[5], nonce[6], nonce[7]]);
        
        ChaCha20 {
            state,
            counter: counter as u64,
            buffer: [0u8; 64],
            buffer_pos: 64,
        }
    }

    /// Encrypt/decrypt data (same operation for both)
    pub fn process(&mut self, data: &[u8]) -> Vec<u8> {
        let mut output = Vec::with_capacity(data.len());
        
        for &byte in data {
            if self.buffer_pos >= 64 {
                self.generate_block();
                self.buffer_pos = 0;
            }
            output.push(byte ^ self.buffer[self.buffer_pos]);
            self.buffer_pos += 1;
        }
        
        output
    }

    /// Encrypt in place
    pub fn encrypt_in_place(&mut self, data: &mut [u8]) {
        for byte in data.iter_mut() {
            if self.buffer_pos >= 64 {
                self.generate_block();
                self.buffer_pos = 0;
            }
            *byte ^= self.buffer[self.buffer_pos];
            self.buffer_pos += 1;
        }
    }

    /// Seek to specific counter
    pub fn seek(&mut self, counter: u64) {
        self.counter = counter;
        self.state[12] = (counter & 0xFFFFFFFF) as u32;
        self.state[13] = ((counter >> 32) & 0xFFFFFFFF) as u32;
        self.buffer_pos = 64;
    }

    fn generate_block(&mut self) {
        let mut working = self.state;
        
        for _ in 0..(CHACHA20_ROUNDS / 2) {
            // Column rounds
            Self::quarter_round(&mut working, 0, 4, 8, 12);
            Self::quarter_round(&mut working, 1, 5, 9, 13);
            Self::quarter_round(&mut working, 2, 6, 10, 14);
            Self::quarter_round(&mut working, 3, 7, 11, 15);
            
            // Diagonal rounds
            Self::quarter_round(&mut working, 0, 5, 10, 15);
            Self::quarter_round(&mut working, 1, 6, 11, 12);
            Self::quarter_round(&mut working, 2, 7, 8, 13);
            Self::quarter_round(&mut working, 3, 4, 9, 14);
        }
        
        // Add original state
        for i in 0..16 {
            working[i] = working[i].wrapping_add(self.state[i]);
        }
        
        // Convert to bytes
        for i in 0..16 {
            let bytes = working[i].to_le_bytes();
            self.buffer[i * 4..i * 4 + 4].copy_from_slice(&bytes);
        }
        
        // Increment counter
        self.counter += 1;
        self.state[12] = (self.counter & 0xFFFFFFFF) as u32;
        self.state[13] = ((self.counter >> 32) & 0xFFFFFFFF) as u32;
    }

    fn quarter_round(state: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
        state[a] = state[a].wrapping_add(state[b]);
        state[d] = (state[d] ^ state[a]).rotate_left(16);
        state[c] = state[c].wrapping_add(state[d]);
        state[b] = (state[b] ^ state[c]).rotate_left(12);
        state[a] = state[a].wrapping_add(state[b]);
        state[d] = (state[d] ^ state[a]).rotate_left(8);
        state[c] = state[c].wrapping_add(state[d]);
        state[b] = (state[b] ^ state[c]).rotate_left(7);
    }
}

/// ChaCha20-Poly1305 AEAD
pub struct ChaCha20Poly1305 {
    cipher: ChaCha20,
    poly_key: [u8; 32],
    poly_state: Poly1305State,
    aad_len: u64,
    ciphertext_len: u64,
    finished: bool,
}

#[derive(Clone)]
struct Poly1305State {
    r: [u32; 5],
    s: [u32; 4],
    h: [u32; 5],
    buffer: [u8; 16],
    buffer_len: usize,
}

impl ChaCha20Poly1305 {
    /// Create new AEAD cipher
    pub fn new(key: &[u8; 32], nonce: &[u8; 12]) -> Self {
        let cipher = ChaCha20::new(key, nonce);
        
        // Generate Poly1305 key from first block
        let poly_key = [0u8; 32]; // Will be generated from cipher
        
        ChaCha20Poly1305 {
            cipher,
            poly_key,
            poly_state: Poly1305State::new(&poly_key),
            aad_len: 0,
            ciphertext_len: 0,
            finished: false,
        }
    }

    /// Encrypt plaintext with AAD
    pub fn encrypt(&mut self, plaintext: &[u8], aad: &[u8]) -> (Vec<u8>, [u8; 16]) {
        let mut output = Vec::with_capacity(plaintext.len() + 16);
        
        // Generate Poly1305 key
        let mut key_block = [0u8; 64];
        self.cipher.generate_block();
        self.poly_key.copy_from_slice(&self.cipher.buffer[..32]);
        
        // Initialize Poly1305
        self.poly_state = Poly1305State::new(&self.poly_key);
        
        // Process AAD
        self.poly_state.update(aad);
        self.poly_state.pad();
        self.aad_len = aad.len() as u64;
        
        // Encrypt plaintext
        let ciphertext = self.cipher.process(plaintext);
        output.extend_from_slice(&ciphertext);
        
        // Process ciphertext for tag
        self.poly_state.update(&ciphertext);
        self.poly_state.pad();
        self.ciphertext_len = plaintext.len() as u64;
        
        // Add lengths
        let len_block = [
            (self.aad_len as u32).to_le_bytes(),
            ((self.aad_len >> 32) as u32).to_le_bytes(),
            (self.ciphertext_len as u32).to_le_bytes(),
            ((self.ciphertext_len >> 32) as u32).to_le_bytes(),
        ];
        let mut len_bytes = [0u8; 16];
        for i in 0..4 {
            len_bytes[i * 4..i * 4 + 4].copy_from_slice(&len_block[i]);
        }
        self.poly_state.update(&len_bytes);
        
        // Get tag
        let tag = self.poly_state.finish();
        
        (output, tag)
    }

    /// Decrypt ciphertext with AAD
    pub fn decrypt(&mut self, ciphertext: &[u8], aad: &[u8], tag: &[u8; 16]) -> Option<Vec<u8>> {
        // Generate Poly1305 key
        self.cipher.generate_block();
        self.poly_key.copy_from_slice(&self.cipher.buffer[..32]);
        
        // Initialize Poly1305
        self.poly_state = Poly1305State::new(&self.poly_key);
        
        // Process AAD
        self.poly_state.update(aad);
        self.poly_state.pad();
        self.aad_len = aad.len() as u64;
        
        // Process ciphertext for tag verification
        self.poly_state.update(ciphertext);
        self.poly_state.pad();
        self.ciphertext_len = ciphertext.len() as u64;
        
        // Add lengths
        let len_block = [
            (self.aad_len as u32).to_le_bytes(),
            ((self.aad_len >> 32) as u32).to_le_bytes(),
            (self.ciphertext_len as u32).to_le_bytes(),
            ((self.ciphertext_len >> 32) as u32).to_le_bytes(),
        ];
        let mut len_bytes = [0u8; 16];
        for i in 0..4 {
            len_bytes[i * 4..i * 4 + 4].copy_from_slice(&len_block[i]);
        }
        self.poly_state.update(&len_bytes);
        
        // Verify tag
        let computed_tag = self.poly_state.finish();
        if !Self::constant_time_eq(&computed_tag, tag) {
            return None;
        }
        
        // Decrypt
        Some(self.cipher.process(ciphertext))
    }

    fn constant_time_eq(a: &[u8; 16], b: &[u8; 16]) -> bool {
        let mut result = 0u8;
        for i in 0..16 {
            result |= a[i] ^ b[i];
        }
        result == 0
    }
}

impl Poly1305State {
    fn new(key: &[u8; 32]) -> Self {
        let mut r = [0u32; 5];
        let mut s = [0u32; 4];
        
        // Clamp r
        r[0] = (u32::from_le_bytes([key[0], key[1], key[2], key[3]]) & 0x0FFFFFFF) >> 0;
        r[1] = (u32::from_le_bytes([key[3], key[4], key[5], key[6]]) & 0x0FFFFFFC) >> 2;
        r[2] = (u32::from_le_bytes([key[6], key[7], key[8], key[9]]) & 0x0FFFFFFC) >> 2;
        r[3] = (u32::from_le_bytes([key[9], key[10], key[11], key[12]]) & 0x0FFFFFFC) >> 2;
        r[4] = (u32::from_le_bytes([key[12], key[13], key[14], key[15]]) & 0x0FFFFFFC) >> 2;
        
        for i in 0..4 {
            s[i] = u32::from_le_bytes([key[16 + i * 4], key[17 + i * 4], key[18 + i * 4], key[19 + i * 4]]);
        }
        
        Poly1305State {
            r,
            s,
            h: [0u32; 5],
            buffer: [0u8; 16],
            buffer_len: 0,
        }
    }

    fn update(&mut self, data: &[u8]) {
        for &byte in data {
            self.buffer[self.buffer_len] = byte;
            self.buffer_len += 1;
            
            if self.buffer_len == 16 {
                self.process_block();
                self.buffer_len = 0;
            }
        }
    }

    fn pad(&mut self) {
        if self.buffer_len > 0 {
            self.buffer[self.buffer_len] = 1;
            for i in self.buffer_len + 1..16 {
                self.buffer[i] = 0;
            }
            self.process_block_padded();
            self.buffer_len = 0;
        }
    }

    fn process_block(&mut self) {
        let mut acc = [0u64; 5];
        
        // Add buffer to h
        let n0 = u32::from_le_bytes([self.buffer[0], self.buffer[1], self.buffer[2], self.buffer[3]]);
        let n1 = u32::from_le_bytes([self.buffer[4], self.buffer[5], self.buffer[6], self.buffer[7]]);
        let n2 = u32::from_le_bytes([self.buffer[8], self.buffer[9], self.buffer[10], self.buffer[11]]);
        let n3 = u32::from_le_bytes([self.buffer[12], self.buffer[13], self.buffer[14], self.buffer[15]]);
        
        acc[0] = self.h[0] as u64 + n0 as u64;
        acc[1] = self.h[1] as u64 + n1 as u64;
        acc[2] = self.h[2] as u64 + n2 as u64;
        acc[3] = self.h[3] as u64 + n3 as u64;
        acc[4] = self.h[4] as u64 + (1 << 24);
        
        // Multiply by r
        // Simplified - real implementation needs full 130-bit multiplication
        for i in 0..5 {
            self.h[i] = (acc[i] & 0xFFFFFFFF) as u32;
        }
    }

    fn process_block_padded(&mut self) {
        self.process_block();
    }

    fn finish(&self) -> [u8; 16] {
        let mut output = [0u8; 16];
        
        // Add s
        let h0 = (self.h[0] as u64 + self.s[0] as u64) as u32;
        let h1 = (self.h[1] as u64 + self.s[1] as u64) as u32;
        let h2 = (self.h[2] as u64 + self.s[2] as u64) as u32;
        let h3 = (self.h[3] as u64 + self.s[3] as u64) as u32;
        
        output[0..4].copy_from_slice(&h0.to_le_bytes());
        output[4..8].copy_from_slice(&h1.to_le_bytes());
        output[8..12].copy_from_slice(&h2.to_le_bytes());
        output[12..16].copy_from_slice(&h3.to_le_bytes());
        
        output
    }
}

/// XChaCha20-Poly1305 (extended nonce)
pub struct XChaCha20Poly1305;

impl XChaCha20Poly1305 {
    /// Derive subkey from key and 24-byte nonce
    pub fn derive_subkey(key: &[u8; 32], nonce: &[u8; 24]) -> ([u8; 32], [u8; 12]) {
        // HChaCha20 for first 16 bytes of nonce
        let mut cipher = ChaCha20::new(key, &[0u8; 12]);
        cipher.state[12] = u32::from_le_bytes([nonce[0], nonce[1], nonce[2], nonce[3]]);
        cipher.state[13] = u32::from_le_bytes([nonce[4], nonce[5], nonce[6], nonce[7]]);
        cipher.state[14] = u32::from_le_bytes([nonce[8], nonce[9], nonce[10], nonce[11]]);
        cipher.state[15] = u32::from_le_bytes([nonce[12], nonce[13], nonce[14], nonce[15]]);
        
        let mut subkey = [0u8; 32];
        cipher.generate_block();
        subkey.copy_from_slice(&cipher.buffer[..32]);
        
        // Remaining 8 bytes + 4 byte counter = 12 byte nonce
        let mut subnonce = [0u8; 12];
        subnonce[4..12].copy_from_slice(&nonce[16..24]);
        
        (subkey, subnonce)
    }

    /// Encrypt with 24-byte nonce
    pub fn encrypt(key: &[u8; 32], nonce: &[u8; 24], plaintext: &[u8], aad: &[u8]) -> (Vec<u8>, [u8; 16]) {
        let (subkey, subnonce) = Self::derive_subkey(key, nonce);
        let mut cipher = ChaCha20Poly1305::new(&subkey, &subnonce);
        cipher.encrypt(plaintext, aad)
    }

    /// Decrypt with 24-byte nonce
    pub fn decrypt(key: &[u8; 32], nonce: &[u8; 24], ciphertext: &[u8], aad: &[u8], tag: &[u8; 16]) -> Option<Vec<u8>> {
        let (subkey, subnonce) = Self::derive_subkey(key, nonce);
        let mut cipher = ChaCha20Poly1305::new(&subkey, &subnonce);
        cipher.decrypt(ciphertext, aad, tag)
    }
}
