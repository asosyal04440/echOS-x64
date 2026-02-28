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

pub mod hw_aes;
pub mod signature;
pub mod sha3;
pub mod blake3;
pub mod chacha20;
pub mod ed25519;
pub mod argon2;

pub use hw_aes::{AesNi, ShaNi, ClMulGhash, GhashSoft, CpuFeatures, detect_features, get_features, init};
pub use hw_aes::{rdrand_bytes, rdseed_bytes};
pub use signature::{RsaPublicKey, EcdsaPublicKey, EllipticCurve, HashAlgorithm};
pub use sha3::{Sha3, sha3_256, sha3_512, keccak256};
pub use blake3::{Blake3, blake3_hash, blake3_mac};
pub use chacha20::{ChaCha20, ChaCha20Poly1305, XChaCha20Poly1305};
pub use ed25519::{Ed25519PublicKey, Ed25519PrivateKey, X25519PublicKey, X25519PrivateKey, HkdfSha256, hmac_sha256};
pub use argon2::{Argon2, Argon2Config, Argon2Variant, Argon2Version, PasswordHash};
