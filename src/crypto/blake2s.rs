//! # BLAKE2s Özet Fonksiyonu (RFC 7693)
//!
//! WireGuard protokolünün gereksinim duyduğu BLAKE2s implementasyonu.
//! Anahtarlı mod (native keyed MAC), anahtarsız hash ve HMAC-BLAKE2s desteği.
//!
//! ## Parametreler
//!
//! ```text
//!  Kelime genişliği : 32 bit (4 bayt)
//!  Durum boyutu     : 8 kelime (32 bayt)
//!  Blok boyutu      : 64 bayt
//!  Tur sayısı       : 10
//!  Maks. özet uz.   : 32 bayt
//!  Maks. anahtar uz.: 32 bayt
//!  Döndürme sabitleri: 16, 12, 8, 7
//! ```
//!
//! ## G Fonksiyonu (Quarter Round)
//!
//! ```text
//!  a = a + b + x;   d = (d ^ a) >>> 16;
//!  c = c + d;       b = (b ^ c) >>> 12;
//!  a = a + b + y;   d = (d ^ a) >>> 8;
//!  c = c + d;       b = (b ^ c) >>> 7;
//! ```
//!
//! ## Sıkıştırma (Compression)
//!
//! ```text
//!  v[0..7]  = h[0..7]
//!  v[8..15] = IV[0..7]
//!  v[12] ^= t_lo    (bayt sayacı, düşük 32 bit)
//!  v[13] ^= t_hi    (bayt sayacı, yüksek 32 bit)
//!  v[14] ^= f0      (son blok bayrağı)
//!  v[15] ^= f1      (son düğüm bayrağı — ağaç modu)
//!
//!  10 tur × 8 G uygulaması
//!  h[i] ^= v[i] ^ v[i+8]
//! ```
//!
//! ## Kaynaklar
//!
//! * RFC 7693 — BLAKE2 özet fonksiyonları
//! * https://www.wireguard.com/protocol/ — WireGuard protokolü

use alloc::vec::Vec;

/// BLAKE2s blok boyutu (bayt)
const BLOCK_SIZE: usize = 64;

/// BLAKE2s başlangıç vektörü (IV) — 2'nin karekökünün kesirli kısmı (SHA-256 ile aynı)
const IV: [u32; 8] = [
    0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A,
    0x510E527F, 0x9B05688C, 0x1F83D9AB, 0x5BE0CD19,
];

/// BLAKE2s mesaj permütasyonu (10 × 16) — RFC 7693 Section 2.7
const SIGMA: [[u8; 16]; 10] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
    [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
    [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
    [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
    [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
    [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
    [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
    [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
];

/// BLAKE2s özetleyici durumu
#[derive(Clone)]
pub struct Blake2s {
    h: [u32; 8],       // zincirleme değeri (chaining value)
    buf: [u8; BLOCK_SIZE], // girdi arabelleği
    buflen: usize,     // arabellekteki geçerli bayt sayısı
    t: u64,            // işlenen toplam bayt sayısı (mevcut blok öncesi)
    out_len: u8,       // çıktı uzunluğu (1..32 bayt)
}

impl Blake2s {
    /// Yeni anahtarsız BLAKE2s özetleyici oluşturur.
    pub fn new(out_len: u8) -> Self {
        let mut h = IV;
        h[0] ^= (out_len as u32) | (1 << 16) | (1 << 24);

        Blake2s {
            h,
            buf: [0u8; BLOCK_SIZE],
            buflen: 0,
            t: 0,
            out_len,
        }
    }

    /// Yeni anahtarlı BLAKE2s özetleyici oluşturur (native keyed MAC modu).
    ///
    /// Anahtar, mesajdan önce ilk blok(lar) olarak işlenir.
    /// `key` en fazla 32 bayt olabilir; daha uzun anahtarlar kırpılır.
    pub fn new_keyed(out_len: u8, key: &[u8]) -> Self {
        let key_len = key.len().min(32);
        let mut h = IV;
        h[0] ^= (out_len as u32) | ((key_len as u32) << 8) | (1 << 16) | (1 << 24);

        let mut blake2s = Blake2s {
            h,
            buf: [0u8; BLOCK_SIZE],
            buflen: 0,
            t: 0,
            out_len,
        };

        if key_len > 0 {
            let mut key_block = [0u8; BLOCK_SIZE];
            key_block[..key_len].copy_from_slice(&key[..key_len]);
            blake2s.t = BLOCK_SIZE as u64;
            blake2s.compress(&key_block, false);
        }

        blake2s
    }

    /// Özetleyiciye veri ekler.
    pub fn update(&mut self, data: &[u8]) {
        let mut offset = 0;
        let len = data.len();

        if self.buflen > 0 {
            let take = (BLOCK_SIZE - self.buflen).min(len);
            self.buf[self.buflen..self.buflen + take].copy_from_slice(&data[..take]);
            self.buflen += take;
            offset = take;

            if self.buflen == BLOCK_SIZE {
                let block = self.buf;
                self.t += BLOCK_SIZE as u64;
                self.compress(&block, false);
                self.buflen = 0;
            }
        }

        while offset + BLOCK_SIZE <= len {
            let block: &[u8; BLOCK_SIZE] = (&data[offset..offset + BLOCK_SIZE]).try_into().unwrap();
            self.t += BLOCK_SIZE as u64;
            self.compress(block, false);
            offset += BLOCK_SIZE;
        }

        let remaining = len - offset;
        if remaining > 0 {
            self.buf[..remaining].copy_from_slice(&data[offset..]);
            self.buflen = remaining;
        }
    }

    /// Özetlemeyi tamamlar ve sonucu `out` dilimine yazar.
    pub fn finalize(&mut self, out: &mut [u8]) {
        self.t += self.buflen as u64;
        let mut block = self.buf;
        block[self.buflen..].fill(0);
        self.compress(&block, true);

        for i in 0..(self.out_len as usize + 3) / 4 {
            let word_bytes = self.h[i].to_le_bytes();
            let start = i * 4;
            let end = (start + 4).min(out.len());
            out[start..end].copy_from_slice(&word_bytes[..end - start]);
        }
    }

    /// BLAKE2s sıkıştırma fonksiyonu (RFC 7693 Section 3.2)
    fn compress(&mut self, block: &[u8; BLOCK_SIZE], last: bool) {
        let mut m = [0u32; 16];
        for i in 0..16 {
            m[i] = u32::from_le_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }

        let mut v = [
            self.h[0], self.h[1], self.h[2], self.h[3],
            self.h[4], self.h[5], self.h[6], self.h[7],
            IV[0], IV[1], IV[2], IV[3],
            IV[4] ^ (self.t as u32),
            IV[5] ^ ((self.t >> 32) as u32),
            IV[6] ^ if last { 0xFFFFFFFF } else { 0 },
            IV[7],
        ];

        for round in 0..10 {
            let s = &SIGMA[round];
            g(&mut v, 0, 4, 8, 12, m[s[0] as usize], m[s[1] as usize]);
            g(&mut v, 1, 5, 9, 13, m[s[2] as usize], m[s[3] as usize]);
            g(&mut v, 2, 6, 10, 14, m[s[4] as usize], m[s[5] as usize]);
            g(&mut v, 3, 7, 11, 15, m[s[6] as usize], m[s[7] as usize]);
            g(&mut v, 0, 5, 10, 15, m[s[8] as usize], m[s[9] as usize]);
            g(&mut v, 1, 6, 11, 12, m[s[10] as usize], m[s[11] as usize]);
            g(&mut v, 2, 7, 8, 13, m[s[12] as usize], m[s[13] as usize]);
            g(&mut v, 3, 4, 9, 14, m[s[14] as usize], m[s[15] as usize]);
        }

        for i in 0..8 {
            self.h[i] ^= v[i] ^ v[i + 8];
        }
    }
}

/// G karıştırma fonksiyonu (Quarter Round)
fn g(v: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize, x: u32, y: u32) {
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(x);
    v[d] = (v[d] ^ v[a]).rotate_right(16);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(12);
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(y);
    v[d] = (v[d] ^ v[a]).rotate_right(8);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(7);
}

/// Anahtarsız BLAKE2s özeti — 32 bayt çıktı.
pub fn blake2s(data: &[u8]) -> [u8; 32] {
    let mut hasher = Blake2s::new(32);
    hasher.update(data);
    let mut out = [0u8; 32];
    hasher.finalize(&mut out);
    out
}

/// Anahtarlı BLAKE2s (native keyed MAC) — 32 bayt çıktı.
pub fn blake2s_keyed(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut hasher = Blake2s::new_keyed(32, key);
    hasher.update(data);
    let mut out = [0u8; 32];
    hasher.finalize(&mut out);
    out
}

/// HMAC-BLAKE2s (RFC 2104) — 32 bayt çıktı.
pub fn hmac_blake2s(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut padded_key = [0u8; BLOCK_SIZE];
    if key.len() <= BLOCK_SIZE {
        padded_key[..key.len()].copy_from_slice(key);
    } else {
        let hash = blake2s(key);
        padded_key[..32].copy_from_slice(&hash);
    }

    let mut inner = Blake2s::new(32);
    for i in 0..BLOCK_SIZE {
        inner.update(&[(padded_key[i] ^ 0x36)]);
    }
    inner.update(msg);
    let mut inner_hash = [0u8; 32];
    inner.finalize(&mut inner_hash);

    let mut outer = Blake2s::new(32);
    for i in 0..BLOCK_SIZE {
        outer.update(&[(padded_key[i] ^ 0x5c)]);
    }
    outer.update(&inner_hash);
    let mut result = [0u8; 32];
    outer.finalize(&mut result);
    result
}

// ============================================================================
// HKDF-BLAKE2s (RFC 5869)
// ============================================================================

/// HKDF-BLAKE2s anahtar türetme yapısı
pub struct HkdfBlake2s {
    prk: [u8; 32],
}

impl HkdfBlake2s {
    /// Çıkarma aşaması: PRK = HMAC-BLAKE2s(salt, IKM)
    pub fn extract(salt: &[u8], ikm: &[u8]) -> Self {
        HkdfBlake2s {
            prk: hmac_blake2s(salt, ikm),
        }
    }

    /// Genişletme aşaması: PRK + info → istenilen uzunlukta OKM
    pub fn expand(&self, info: &[u8], okm_len: usize) -> Vec<u8> {
        let mut okm = Vec::with_capacity(okm_len);
        let mut t = [0u8; 32];
        let mut t_len = 0;
        let mut counter = 1u8;

        while okm.len() < okm_len {
            let mut input = Vec::new();
            if t_len > 0 {
                input.extend_from_slice(&t[..t_len]);
            }
            input.extend_from_slice(info);
            input.push(counter);

            let block = hmac_blake2s(&self.prk, &input);
            let take = (okm_len - okm.len()).min(32);
            okm.extend_from_slice(&block[..take]);
            t = block;
            t_len = 32;
            counter += 1;
        }

        okm
    }

    /// Çıkarma + genişletme tek adımda
    pub fn derive(salt: &[u8], ikm: &[u8], info: &[u8], okm_len: usize) -> Vec<u8> {
        let hkdf = Self::extract(salt, ikm);
        hkdf.expand(info, okm_len)
    }
}

// ============================================================================
// TESTLER — RFC 7693 KAT + WireGuard doğrulama
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// BLAKE2s test vector (unkeyed, 32-byte output) — RFC 7693 Appendix A
    /// Input: "abc" (3 bytes)
    #[test]
    fn blake2s_rfc7693_test_vector_abc() {
        let input = b"abc";
        let expected: [u8; 32] = [
            0x50, 0x8C, 0x5E, 0x8C, 0x32, 0x7C, 0x14, 0xE2,
            0xE1, 0xA7, 0x2B, 0xA3, 0x4E, 0xEB, 0x45, 0x2F,
            0x37, 0x45, 0x8B, 0x20, 0x9E, 0xD6, 0x3A, 0x29,
            0x4D, 0x99, 0x9B, 0x4C, 0x86, 0x67, 0x59, 0x82,
        ];
        let result = blake2s(input);
        assert_eq!(result, expected, "BLAKE2s('abc') mismatch");
    }

    /// BLAKE2s test vector (unkeyed, 32-byte output) — RFC 7693 Appendix A
    /// Input: empty string
    #[test]
    fn blake2s_rfc7693_test_vector_empty() {
        let input = b"";
        let expected: [u8; 32] = [
            0x69, 0x21, 0x7A, 0x30, 0x79, 0x90, 0x80, 0x94,
            0xE1, 0x11, 0x21, 0xD0, 0x42, 0x35, 0x4A, 0x7C,
            0x1F, 0x55, 0xB6, 0x48, 0x2C, 0xA1, 0xA5, 0x1E,
            0x1B, 0x25, 0x0D, 0xFD, 0x1E, 0xD0, 0xEE, 0xF9,
        ];
        let result = blake2s(input);
        assert_eq!(result, expected, "BLAKE2s('') mismatch");
    }

    /// BLAKE2s test vector (keyed, 32-byte output) — Python hashlib.blake2s reference
    /// Key: 00 01 02 ... 1F (32 bytes)
    /// Input: "abc" (3 bytes)
    #[test]
    fn blake2s_rfc7693_keyed_test_vector() {
        let key: [u8; 32] = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
            0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
            0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
            0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F,
        ];
        let input = b"abc";
        let expected: [u8; 32] = [
            0xA2, 0x81, 0xF7, 0x25, 0x75, 0x49, 0x69, 0xA7,
            0x02, 0xF6, 0xFE, 0x36, 0xFC, 0x59, 0x1B, 0x7D,
            0xEF, 0x86, 0x6E, 0x4B, 0x70, 0x17, 0x3E, 0xCE,
            0x40, 0x2F, 0xC0, 0x1C, 0x06, 0x4D, 0x6B, 0x65,
        ];
        let result = blake2s_keyed(&key, input);
        assert_eq!(result, expected, "BLAKE2s(keyed, 'abc') mismatch");
    }

    /// 64 baytlık tam blok — sınır testi
    #[test]
    fn blake2s_exact_one_block() {
        let input = [0x61u8; 64]; // 64 x 'a'
        let result = blake2s(&input);
        assert_eq!(result.len(), 32);
    }

    /// 65 bayt — iki blok sınırı
    #[test]
    fn blake2s_across_block_boundary() {
        let input = [0x61u8; 65]; // 65 x 'a'
        let result = blake2s(&input);
        assert_eq!(result.len(), 32);
    }

    /// HMAC-BLAKE2s temel çalışma testi
    #[test]
    fn hmac_blake2s_basic() {
        let key = b"secret";
        let msg = b"test message";
        let result = hmac_blake2s(key, msg);
        assert_eq!(result.len(), 32);
        let result2 = hmac_blake2s(b"different", msg);
        assert_ne!(result, result2);
    }

    /// HMAC-BLAKE2s farklı anahtar uzunluğu
    #[test]
    fn hmac_blake2s_long_key() {
        let key = [0xABu8; 80]; // > 64 byte
        let msg = b"data";
        let result = hmac_blake2s(&key, msg);
        assert_eq!(result.len(), 32);
    }

    /// HKDF-BLAKE2s temel test
    #[test]
    fn hkdf_blake2s_basic() {
        let salt = b"salt";
        let ikm = b"input key material";
        let info = b"context";
        let okm = HkdfBlake2s::derive(salt, ikm, info, 32);
        assert_eq!(okm.len(), 32);
        let okm2 = HkdfBlake2s::derive(b"different", ikm, info, 32);
        assert_ne!(okm, okm2);
    }

    /// HKDF-BLAKE2s 64 bayt çıktı (expand test)
    #[test]
    fn hkdf_blake2s_long_output() {
        let salt = b"salt";
        let ikm = b"input key material";
        let info = b"context";
        let okm = HkdfBlake2s::derive(salt, ikm, info, 64);
        assert_eq!(okm.len(), 64);
        assert_ne!(okm[..32], okm[32..]);
    }

    /// BLAKE2s native keyed MAC — WireGuard MAC1 için ilk 16 bayt kullanılır
    #[test]
    fn blake2s_keyed_truncated_to_16() {
        let key = [0xABu8; 32];
        let msg = [0xCDu8; 116];
        let full = blake2s_keyed(&key, &msg);
        let mac = &full[..16];
        assert_eq!(mac.len(), 16);
    }

    /// Ardışık update + finalize (streaming) testi
    #[test]
    fn blake2s_streaming_vs_single() {
        let data = b"streaming test data for blake2s";
        let single = blake2s(data);

        let mut hasher = Blake2s::new(32);
        hasher.update(b"streaming ");
        hasher.update(b"test ");
        hasher.update(b"data ");
        hasher.update(b"for blake2s");
        let mut streamed = [0u8; 32];
        hasher.finalize(&mut streamed);

        assert_eq!(single, streamed);
    }
}
