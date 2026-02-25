//! # Hardware-Accelerated Cryptography
//!
//! Provides AES-NI and SHA-NI implementations for x86_64 processors
//! plus SHA-3, BLAKE3, ChaCha20, Ed25519, X25519, HKDF, Argon2

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
