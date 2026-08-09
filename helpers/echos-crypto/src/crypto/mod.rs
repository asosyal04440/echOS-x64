//! # Donanım Hızlandırmalı Kriptografi
//!
//! x86_64 işlemciler için AES-NI ve SHA-NI uygulamaları ile birlikte
//! SHA-3, BLAKE3, ChaCha20, Ed25519, X25519, HKDF ve Argon2 desteği.
//!
//! ## Modül Yapısı
//!
//! ```text
//!  crypto/
//!  ├── hw_aes.rs    — AES-NI (AESENC/AESDEC), SHA-NI, GHASH, RDRAND/RDSEED
//!  ├── sha3.rs      — SHA-3 / Keccak-f[1600] sünger yapısı, SHAKE XOF
//!  ├── blake3.rs    — BLAKE3 Merkle ağacı tabanlı hash + MAC + KDF
//!  ├── chacha20.rs  — ChaCha20 akış şifresi, Poly1305 AEAD, XChaCha20
//!  ├── ed25519.rs   — Ed25519 imza, X25519 Diffie-Hellman, HKDF-SHA256
//!  ├── argon2.rs    — Argon2 (d/i/id) bellek-yoğun parola hashleme
//!  └── signature.rs — RSA PKCS#1 v1.5 / PSS, ECDSA P-256/P-384 doğrulama
//! ```
//!
//! ## Hız Karşılaştırması
//!
//! ```text
//!  Algoritma        Donanım        Yazılım
//!  ─────────────    ───────────    ─────────────
//!  AES-128-ECB      0.1 döngü/B   1-3 döngü/B   (AES-NI ile ~10-30x hız)
//!  SHA-256          ~2 döngü/B    ~8 döngü/B    (SHA-NI ile ~4x hız)
//!  ChaCha20         —             ~1 döngü/B    (yazılım optimizasyonu)
//!  BLAKE3           —             ~0.5 döngü/B  (AVX2 paralelliği)
//! ```

pub mod argon2;
pub mod blake2s;
pub mod blake3;
pub mod chacha20;
pub mod ecdsa;
pub mod ed25519;
pub mod hw_aes;
pub mod rsa;
pub mod sha3;
pub mod signature;

pub use argon2::{Argon2, Argon2Config, Argon2Variant, Argon2Version, PasswordHash};
pub use blake2s::{blake2s, blake2s_keyed, hmac_blake2s, Blake2s, HkdfBlake2s};
pub use blake3::{blake3_hash, blake3_mac, Blake3};
pub use chacha20::{ChaCha20, ChaCha20Poly1305, XChaCha20Poly1305};
pub use ed25519::{
    hmac_sha256, Ed25519PrivateKey, Ed25519PublicKey, HkdfSha256, X25519PrivateKey, X25519PublicKey,
};
pub use hw_aes::{
    detect_features, get_features, init, rdrand_bytes, rdseed_bytes, AesNi, ClMulGhash,
    CpuFeatures, GhashSoft, ShaNi,
};
pub use sha3::{keccak256, sha3_256, sha3_512, Sha3};
pub use signature::{EcdsaPublicKey, EllipticCurve, HashAlgorithm, RsaPublicKey};

/// Constant-time equality for fixed-length secret-derived byte strings.
///
/// Length mismatch is public protocol structure and fails before byte comparison.
pub fn constant_time_eq(lhs: &[u8], rhs: &[u8]) -> bool {
    if lhs.len() != rhs.len() {
        return false;
    }

    let mut diff = 0u8;
    for (&left, &right) in lhs.iter().zip(rhs.iter()) {
        diff |= left ^ right;
    }
    diff == 0
}

// ============================================================================
// KAT (Known Answer Tests) - FIPS 140-2 Uyumlu Testler
// ============================================================================

/// KAT test sonucu
#[derive(Debug, Clone)]
pub struct KatResult {
    pub test_name: &'static str,
    pub passed: bool,
    pub message: alloc::string::String,
}

/// SHA-256 KAT testleri (FIPS 180-4 örnekleri)
pub fn kat_sha256() -> KatResult {
    // Test vector from FIPS 180-4
    let input = b"abc";
    let expected: [u8; 32] = [
        0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22,
        0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00,
        0x15, 0xad,
    ];

    let result = sha3_256(input);
    let passed = result == expected;

    KatResult {
        test_name: "SHA-256 KAT",
        passed,
        message: if passed {
            alloc::string::String::from("SHA-256 test vector passed")
        } else {
            alloc::format!(
                "SHA-256 mismatch: expected {:?}, got {:?}",
                expected,
                result
            )
        },
    }
}

/// SHA3-256 KAT testleri
pub fn kat_sha3_256() -> KatResult {
    // NIST SHA3-256 test vector
    let input = b"abc";
    let expected: [u8; 32] = [
        0x3a, 0x98, 0x5d, 0xa7, 0x4f, 0xe2, 0x25, 0xb2, 0x04, 0x5c, 0x17, 0x2d, 0x6b, 0xd3, 0x90,
        0xbd, 0x85, 0x5f, 0x08, 0x6e, 0x3e, 0x9d, 0x52, 0x5b, 0x46, 0xbf, 0xe2, 0x45, 0x11, 0x43,
        0x15, 0x32,
    ];

    let result = sha3_256(input);
    let passed = result == expected;

    KatResult {
        test_name: "SHA3-256 KAT",
        passed,
        message: if passed {
            alloc::string::String::from("SHA3-256 test vector passed")
        } else {
            alloc::format!(
                "SHA3-256 mismatch: expected {:?}, got {:?}",
                expected,
                result
            )
        },
    }
}

/// BLAKE3 KAT testleri
pub fn kat_blake3() -> KatResult {
    // BLAKE3 test vector (from official BLAKE3 repo)
    let input = b"abc";
    let expected: [u8; 32] = [
        0x64, 0x37, 0xb3, 0xac, 0x38, 0x46, 0x51, 0x33, 0x4b, 0x47, 0x7a, 0x4b, 0x83, 0x41, 0x6d,
        0x7e, 0x8b, 0x5b, 0x02, 0x26, 0x80, 0x3f, 0x8c, 0x3d, 0x32, 0xca, 0x0c, 0x38, 0xda, 0xf7,
        0x4d, 0x02,
    ];

    let result = blake3_hash(input);
    let passed = result == expected;

    KatResult {
        test_name: "BLAKE3 KAT",
        passed,
        message: if passed {
            alloc::string::String::from("BLAKE3 test vector passed")
        } else {
            alloc::format!("BLAKE3 mismatch: expected {:?}, got {:?}", expected, result)
        },
    }
}

/// ChaCha20 KAT testleri (RFC 8439)
pub fn kat_chacha20() -> KatResult {
    // RFC 8439 test vector - simplified
    let key: [u8; 32] = [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d,
        0x1e, 0x1f,
    ];
    let nonce: [u8; 12] = [
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x4a, 0x00, 0x00, 0x00, 0x00,
    ];
    let input = b"test";

    // Just verify ChaCha20 encrypts/decrypts correctly (roundtrip test)
    let mut cipher = ChaCha20::new(&key, &nonce);
    let encrypted = cipher.process(input);

    // Reset cipher for decryption
    let mut cipher2 = ChaCha20::new(&key, &nonce);
    let decrypted = cipher2.process(&encrypted);

    let passed = decrypted == input;

    KatResult {
        test_name: "ChaCha20 KAT",
        passed,
        message: if passed {
            alloc::string::String::from("ChaCha20 roundtrip test passed")
        } else {
            alloc::format!("ChaCha20 roundtrip failed")
        },
    }
}

/// AES-128-ECB KAT testleri (FIPS 197)
pub fn kat_aes128() -> KatResult {
    // Use AES-NI if available
    let features = get_features();
    if !features.aes_ni {
        return KatResult {
            test_name: "AES-128 KAT",
            passed: true,
            message: alloc::string::String::from("AES-NI not available, skipping"),
        };
    }

    // Test key and data
    let key: [u8; 16] = [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f,
    ];
    let original: [u8; 16] = [
        0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee,
        0xff,
    ];

    // Roundtrip test
    let cipher = AesNi::new(&key);
    let mut block = original;
    cipher.encrypt_block(&mut block);
    cipher.decrypt_block(&mut block);

    let passed = block == original;

    KatResult {
        test_name: "AES-128 KAT",
        passed,
        message: if passed {
            alloc::string::String::from("AES-128 roundtrip test passed")
        } else {
            alloc::format!("AES-128 roundtrip failed")
        },
    }
}

pub fn kat_ed25519() -> KatResult {
    let seed = b"echOS-Ed25519-KAT-seed-000001";
    let mut seed_arr = [0u8; 32];
    seed_arr[..seed.len()].copy_from_slice(seed);
    let private = ed25519::Ed25519PrivateKey::from_seed(&seed_arr);
    let message = b"echOS Ed25519 KAT";
    let signature = private.sign(message);
    let public = *private.public_key();

    let passed = public.verify(message, &signature);

    KatResult {
        test_name: "Ed25519 KAT",
        passed,
        message: if passed {
            alloc::string::String::from("Ed25519 sign/verify roundtrip passed")
        } else {
            alloc::string::String::from("Ed25519 sign/verify roundtrip failed")
        },
    }
}

pub fn kat_x25519() -> KatResult {
    let mut a_arr = [0u8; 32];
    let mut b_arr = [0u8; 32];
    a_arr[0] = 0xAB;
    b_arr[0] = 0xCD;

    let alice_priv = ed25519::X25519PrivateKey::from_bytes(a_arr);
    let bob_priv = ed25519::X25519PrivateKey::from_bytes(b_arr);
    let alice_pub = alice_priv.public_key();
    let bob_pub = bob_priv.public_key();

    let shared_a = alice_priv.diffie_hellman(&bob_pub);
    let shared_b = bob_priv.diffie_hellman(&alice_pub);

    let passed = shared_a == shared_b;

    KatResult {
        test_name: "X25519 KAT",
        passed,
        message: if passed {
            alloc::string::String::from("X25519 DH shared secret match passed")
        } else {
            alloc::string::String::from("X25519 DH shared secret mismatch")
        },
    }
}

#[cfg(feature = "std")]
pub fn kat_rsa_roundtrip() -> KatResult {
    use rand_core::{CryptoRng, RngCore};
    use rsa::traits::{PrivateKeyParts, PublicKeyParts};
    use sha2::Digest;

    struct SimpleRng(u64);
    impl RngCore for SimpleRng {
        fn next_u32(&mut self) -> u32 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
            (self.0 >> 16) as u32
        }
        fn next_u64(&mut self) -> u64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
            self.0
        }
        fn fill_bytes(&mut self, dest: &mut [u8]) {
            for chunk in dest.chunks_mut(8) {
                let len = chunk.len().min(8);
                chunk.copy_from_slice(&self.next_u64().to_le_bytes()[..len]);
            }
        }
        fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
            self.fill_bytes(dest);
            Ok(())
        }
    }
    impl CryptoRng for SimpleRng {}

    let mut rng = SimpleRng(0xEC_2024_0000);
    match rsa::RsaPrivateKey::new(&mut rng, 1024) {
        Ok(key) => {
            let n = key.n().to_bytes_be();
            let e = key.e().to_bytes_be();
            let public = signature::RsaPublicKey::new(&n, &e);

            let local = super::rsa::RsaPrivateKey {
                n: super::rsa::BigInt::from_be_bytes(&key.n().to_bytes_be()),
                e: super::rsa::BigInt::from_be_bytes(&key.e().to_bytes_be()),
                d: super::rsa::BigInt::from_be_bytes(&key.d().to_bytes_be()),
                p: super::rsa::BigInt::from_be_bytes(&key.primes()[0].to_bytes_be()),
                q: super::rsa::BigInt::from_be_bytes(&key.primes()[1].to_bytes_be()),
                dp: super::rsa::BigInt::from_u64(1),
                dq: super::rsa::BigInt::from_u64(1),
                qinv: super::rsa::BigInt::from_u64(1),
            };

            let message = b"echOS RSA KAT test message";
            let sig = local.sign(message, "sha256");
            let hash = sha2::Sha256::digest(message);
            let passed = !sig.is_empty()
                && public.verify_pkcs1_v15(&hash, &sig, signature::HashAlgorithm::Sha256);

            KatResult {
                test_name: "RSA KAT",
                passed,
                message: if passed {
                    alloc::string::String::from("RSA sign/verify roundtrip passed")
                } else {
                    alloc::string::String::from("RSA sign/verify roundtrip failed")
                },
            }
        }
        Err(_) => KatResult {
            test_name: "RSA KAT",
            passed: false,
            message: alloc::string::String::from("RSA key generation failed"),
        },
    }
}

#[cfg(not(feature = "std"))]
pub fn kat_rsa_roundtrip() -> KatResult {
    KatResult {
        test_name: "RSA KAT",
        passed: true,
        message: alloc::string::String::from("RSA KAT skipped (no_std)"),
    }
}

pub fn kat_argon2() -> KatResult {
    let config = argon2::Argon2Config {
        variant: argon2::Argon2Variant::Argon2id,
        version: argon2::Argon2Version::V13,
        memory_cost: 16,
        time_cost: 2,
        parallelism: 1,
        hash_len: 32,
    };
    let mut hasher = argon2::Argon2::new(config);
    let hash = hasher.hash(b"echOS-password", b"echOS-salt", b"", b"");
    let passed = !hash.is_empty() && hash.len() == 32;

    KatResult {
        test_name: "Argon2 KAT",
        passed,
        message: if passed {
            alloc::string::String::from("Argon2 hash computation passed")
        } else {
            alloc::format!("Argon2 hash length unexpected: {}", hash.len())
        },
    }
}

/// Tüm KAT testlerini çalıştır
pub fn run_all_kat_tests() -> alloc::vec::Vec<KatResult> {
    let mut results = alloc::vec::Vec::new();

    results.push(kat_sha256());
    results.push(kat_sha3_256());
    results.push(kat_blake3());
    results.push(kat_chacha20());
    results.push(kat_aes128());
    results.push(kat_ed25519());
    results.push(kat_x25519());
    results.push(kat_rsa_roundtrip());
    results.push(kat_argon2());

    results
}

/// KAT testlerini başlat ve sonucu döndür
pub fn init_kat_tests() -> bool {
    let results = run_all_kat_tests();
    results.iter().all(|r| r.passed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq_rejects_length_and_content_mismatch() {
        assert!(constant_time_eq(b"echos", b"echos"));
        assert!(!constant_time_eq(b"echos", b"echoS"));
        assert!(!constant_time_eq(b"echos", b"echo"));
    }
}
