//! # IPsec - IP Güvenlik Protokolü
//!
//! IP Güvenlik Protokolü (ESP/AH) gerçekleştirimi.
//!
//! ## IPsec Nedir?
//!
//! IPsec, IP katmanında kimlik doğrulama ve şifreleme sağlayan protokol takımıdır.
//! İki ana protokolden oluşur:
//! - **ESP** (Encapsulating Security Payload): Hem şifreleme hem kimlik doğrulama sunar.
//! - **AH** (Authentication Header): Yalnızca bütünlük/kimlik doğrulama sağlar, şifrelemez.
//!
//! ## Çalışma Modları
//!
//! ```
//! Transport Modu (uçtan uca - host to host):
//! ┌──────────┬──────────┬────────────────────────────┐
//! │ IP Başlık│ ESP/AH   │ TCP/UDP + Veri (şifreli)   │
//! └──────────┴──────────┴────────────────────────────┘
//!
//! Tünel Modu (VPN - gateway to gateway):
//! ┌──────────┬─────────┬──────────────────────────────┐
//! │ Dış IP   │ ESP/AH  │ İç IP Başlık + TCP/UDP + Veri│
//! │(tünel)   │         │       (tamamen şifreli)       │
//! └──────────┴─────────┴──────────────────────────────┘
//! ```
//!
//! ## ESP Paket Yapısı
//!
//! ```
//! ┌──────────────────────────────────────────────────┐
//! │         SPI (32 bit) - Güvenlik Parametresi      │
//! ├──────────────────────────────────────────────────┤
//! │      Sıra Numarası (32 bit) - Tekrar saldırısı   │
//! ├─────────────────╔═════════════════════╗──────────┤
//! │  IV (Başlangıç  ║  Şifrelenmiş Yük    ║ Dolgu    │
//! │  Vektörü)       ║  (TCP/UDP + Veri)   ║          │
//! ├─────────────────╚═════════════════════╝──────────┤
//! │           ICV (Bütünlük Doğrulama Değeri)         │
//! └──────────────────────────────────────────────────┘
//! ```
//!
//! ## Güvenlik İlişkilendirmesi (SA) ve Politika (SP)
//!
//! ```
//! SP (ne zaman IPsec uygula?)
//!    │
//!    ▼
//! SA (nasıl uygula? hangi anahtar ve algoritma?)
//!    │
//!    ├── Şifreleme: AES-CBC / AES-GCM / ChaCha20-Poly1305
//!    └── Kimlik doğrulama: HMAC-SHA256 / HMAC-SHA512
//! ```

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use sha1::{Digest as Sha1Digest, Sha1};
use sha2::{Sha384, Sha512};
use spin::Mutex;

use crate::crypto::{AesNi, ChaCha20Poly1305, ClMulGhash, GhashSoft};

type IpsecCipherFn = fn(&SecurityAssociation, &[u8]) -> Result<Vec<u8>, IpsecError>;
type IpsecAuthFn = fn(&SecurityAssociation, &[u8]) -> Result<Vec<u8>, IpsecError>;

#[derive(Clone, Copy)]
struct IpsecCipherRegistryEntry {
    encrypt: IpsecCipherFn,
    decrypt: IpsecCipherFn,
}

#[derive(Clone, Copy)]
struct IpsecCipherFamilyEntry {
    family_id: u16,
    mask: u16,
    encrypt: IpsecCipherFn,
    decrypt: IpsecCipherFn,
}

#[derive(Clone, Copy)]
struct IpsecAuthRegistryEntry {
    icv_len: usize,
    calculate: IpsecAuthFn,
}

#[derive(Clone, Copy)]
struct IpsecAuthFamilyEntry {
    family_id: u16,
    mask: u16,
    icv_len: usize,
    calculate: IpsecAuthFn,
}

lazy_static::lazy_static! {
    static ref IPSEC_CIPHER_REGISTRY: Mutex<BTreeMap<u16, IpsecCipherRegistryEntry>> =
        Mutex::new(BTreeMap::new());
    static ref IPSEC_AUTH_REGISTRY: Mutex<BTreeMap<u16, IpsecAuthRegistryEntry>> =
        Mutex::new(BTreeMap::new());
    static ref IPSEC_CIPHER_FAMILY_REGISTRY: Mutex<Vec<IpsecCipherFamilyEntry>> =
        Mutex::new(Vec::new());
    static ref IPSEC_AUTH_FAMILY_REGISTRY: Mutex<Vec<IpsecAuthFamilyEntry>> =
        Mutex::new(Vec::new());
}

// ============================================================================
// IPsec SABİTLERİ
// ============================================================================

/// IPsec protokol numaraları (IPv4 başlığındaki `protocol` alanına yazılır)
pub const IPPROTO_ESP: u8 = 50; // Kapsülleme Güvenlik Yükü
pub const IPPROTO_AH: u8 = 51; // Kimlik Doğrulama Başlığı

/// IPsec çalışma modları
pub const IPSEC_MODE_TRANSPORT: u8 = 0; // Uçtan uca - yalnızca yük korunur
pub const IPSEC_MODE_TUNNEL: u8 = 1; // Tünel - tüm IP paketi kapsüllenir

/// IPsec yönleri
pub const IPSEC_DIR_INBOUND: u8 = 0; // Gelen trafik
pub const IPSEC_DIR_OUTBOUND: u8 = 1; // Giden trafik

/// Şifreleme algoritmaları
/// Güvenlik sırasına göre sıralanmıştır; NULL yalnızca test için kullanılır.
pub const IPSEC_ENC_NULL: u16 = 0; // Şifreleme yok (test)
pub const IPSEC_ENC_DES_CBC: u16 = 1; // DES-CBC (zayıf, kullanılmamalı)
pub const IPSEC_ENC_3DES_CBC: u16 = 2; // 3DES-CBC (zayıf, kullanılmamalı)
pub const IPSEC_ENC_AES_CBC: u16 = 3; // AES-CBC (yaygın, güvenli)
pub const IPSEC_ENC_AES_CTR: u16 = 4; // AES-CTR (hızlı, güvenli)
pub const IPSEC_ENC_AES_GCM: u16 = 5; // AES-GCM (AEAD, en iyi seçim)
pub const IPSEC_ENC_CHACHA20_POLY1305: u16 = 6; // ChaCha20-Poly1305 (donanımsız sistemlerde hızlı)

/// Kimlik doğrulama algoritmaları (HMAC tabanlı)
/// HMAC: Hash tabanlı Mesaj Kimlik Doğrulama Kodu
pub const IPSEC_AUTH_HMAC_MD5: u16 = 1; // MD5 (zayıf, kullanılmamalı)
pub const IPSEC_AUTH_HMAC_SHA1: u16 = 2; // SHA-1 (zayıf, kullanılmamalı)
pub const IPSEC_AUTH_HMAC_SHA256: u16 = 3; // SHA-256 (güvenli, yaygın)
pub const IPSEC_AUTH_HMAC_SHA384: u16 = 4; // SHA-384
pub const IPSEC_AUTH_HMAC_SHA512: u16 = 5; // SHA-512 (en güçlü)
pub const IPSEC_AUTH_AES_XCBC: u16 = 6; // AES-XCBC-96

const DES_BLOCK_SIZE: usize = 8;

const DES_IP: [u8; 64] = [
    58, 50, 42, 34, 26, 18, 10, 2, 60, 52, 44, 36, 28, 20, 12, 4, 62, 54, 46, 38, 30, 22, 14, 6,
    64, 56, 48, 40, 32, 24, 16, 8, 57, 49, 41, 33, 25, 17, 9, 1, 59, 51, 43, 35, 27, 19, 11, 3, 61,
    53, 45, 37, 29, 21, 13, 5, 63, 55, 47, 39, 31, 23, 15, 7,
];

const DES_FP: [u8; 64] = [
    40, 8, 48, 16, 56, 24, 64, 32, 39, 7, 47, 15, 55, 23, 63, 31, 38, 6, 46, 14, 54, 22, 62, 30,
    37, 5, 45, 13, 53, 21, 61, 29, 36, 4, 44, 12, 52, 20, 60, 28, 35, 3, 43, 11, 51, 19, 59, 27,
    34, 2, 42, 10, 50, 18, 58, 26, 33, 1, 41, 9, 49, 17, 57, 25,
];

const DES_E: [u8; 48] = [
    32, 1, 2, 3, 4, 5, 4, 5, 6, 7, 8, 9, 8, 9, 10, 11, 12, 13, 12, 13, 14, 15, 16, 17, 16, 17, 18,
    19, 20, 21, 20, 21, 22, 23, 24, 25, 24, 25, 26, 27, 28, 29, 28, 29, 30, 31, 32, 1,
];

const DES_P: [u8; 32] = [
    16, 7, 20, 21, 29, 12, 28, 17, 1, 15, 23, 26, 5, 18, 31, 10, 2, 8, 24, 14, 32, 27, 3, 9, 19,
    13, 30, 6, 22, 11, 4, 25,
];

const DES_PC1: [u8; 56] = [
    57, 49, 41, 33, 25, 17, 9, 1, 58, 50, 42, 34, 26, 18, 10, 2, 59, 51, 43, 35, 27, 19, 11, 3, 60,
    52, 44, 36, 63, 55, 47, 39, 31, 23, 15, 7, 62, 54, 46, 38, 30, 22, 14, 6, 61, 53, 45, 37, 29,
    21, 13, 5, 28, 20, 12, 4,
];

const DES_PC2: [u8; 48] = [
    14, 17, 11, 24, 1, 5, 3, 28, 15, 6, 21, 10, 23, 19, 12, 4, 26, 8, 16, 7, 27, 20, 13, 2, 41, 52,
    31, 37, 47, 55, 30, 40, 51, 45, 33, 48, 44, 49, 39, 56, 34, 53, 46, 42, 50, 36, 29, 32,
];

const DES_SHIFTS: [u8; 16] = [1, 1, 2, 2, 2, 2, 2, 2, 1, 2, 2, 2, 2, 2, 2, 1];

const DES_SBOXES: [[[u8; 16]; 4]; 8] = [
    [
        [14, 4, 13, 1, 2, 15, 11, 8, 3, 10, 6, 12, 5, 9, 0, 7],
        [0, 15, 7, 4, 14, 2, 13, 1, 10, 6, 12, 11, 9, 5, 3, 8],
        [4, 1, 14, 8, 13, 6, 2, 11, 15, 12, 9, 7, 3, 10, 5, 0],
        [15, 12, 8, 2, 4, 9, 1, 7, 5, 11, 3, 14, 10, 0, 6, 13],
    ],
    [
        [15, 1, 8, 14, 6, 11, 3, 4, 9, 7, 2, 13, 12, 0, 5, 10],
        [3, 13, 4, 7, 15, 2, 8, 14, 12, 0, 1, 10, 6, 9, 11, 5],
        [0, 14, 7, 11, 10, 4, 13, 1, 5, 8, 12, 6, 9, 3, 2, 15],
        [13, 8, 10, 1, 3, 15, 4, 2, 11, 6, 7, 12, 0, 5, 14, 9],
    ],
    [
        [10, 0, 9, 14, 6, 3, 15, 5, 1, 13, 12, 7, 11, 4, 2, 8],
        [13, 7, 0, 9, 3, 4, 6, 10, 2, 8, 5, 14, 12, 11, 15, 1],
        [13, 6, 4, 9, 8, 15, 3, 0, 11, 1, 2, 12, 5, 10, 14, 7],
        [1, 10, 13, 0, 6, 9, 8, 7, 4, 15, 14, 3, 11, 5, 2, 12],
    ],
    [
        [7, 13, 14, 3, 0, 6, 9, 10, 1, 2, 8, 5, 11, 12, 4, 15],
        [13, 8, 11, 5, 6, 15, 0, 3, 4, 7, 2, 12, 1, 10, 14, 9],
        [10, 6, 9, 0, 12, 11, 7, 13, 15, 1, 3, 14, 5, 2, 8, 4],
        [3, 15, 0, 6, 10, 1, 13, 8, 9, 4, 5, 11, 12, 7, 2, 14],
    ],
    [
        [2, 12, 4, 1, 7, 10, 11, 6, 8, 5, 3, 15, 13, 0, 14, 9],
        [14, 11, 2, 12, 4, 7, 13, 1, 5, 0, 15, 10, 3, 9, 8, 6],
        [4, 2, 1, 11, 10, 13, 7, 8, 15, 9, 12, 5, 6, 3, 0, 14],
        [11, 8, 12, 7, 1, 14, 2, 13, 6, 15, 0, 9, 10, 4, 5, 3],
    ],
    [
        [12, 1, 10, 15, 9, 2, 6, 8, 0, 13, 3, 4, 14, 7, 5, 11],
        [10, 15, 4, 2, 7, 12, 9, 5, 6, 1, 13, 14, 0, 11, 3, 8],
        [9, 14, 15, 5, 2, 8, 12, 3, 7, 0, 4, 10, 1, 13, 11, 6],
        [4, 3, 2, 12, 9, 5, 15, 10, 11, 14, 1, 7, 6, 0, 8, 13],
    ],
    [
        [4, 11, 2, 14, 15, 0, 8, 13, 3, 12, 9, 7, 5, 10, 6, 1],
        [13, 0, 11, 7, 4, 9, 1, 10, 14, 3, 5, 12, 2, 15, 8, 6],
        [1, 4, 11, 13, 12, 3, 7, 14, 10, 15, 6, 8, 0, 5, 9, 2],
        [6, 11, 13, 8, 1, 4, 10, 7, 9, 5, 0, 15, 14, 2, 3, 12],
    ],
    [
        [13, 2, 8, 4, 6, 15, 11, 1, 10, 9, 3, 14, 5, 0, 12, 7],
        [1, 15, 13, 8, 10, 3, 7, 4, 12, 5, 6, 11, 0, 14, 9, 2],
        [7, 11, 4, 1, 9, 12, 14, 2, 0, 6, 10, 13, 15, 3, 5, 8],
        [2, 1, 14, 7, 4, 10, 8, 13, 15, 12, 9, 0, 3, 5, 6, 11],
    ],
];

// ============================================================================
// GÜVENLİK İLİŞKİLENDİRMESİ (SA - Security Association)
// ============================================================================

/// Güvenlik İlişkilendirmesi (SA)
///
/// SA, iki uç arasındaki tek yönlü güvenli kanal tanımıdır.
/// Her güvenli bağlantı için 2 SA gerekir: biri giden, biri gelen.
///
/// SA benzersiz olarak (SPI, hedef IP, protokol) üçlüsüyle tanımlanır.
/// SPI değeri alıcı tarafından seçilir ve pakete yazılır.
///
/// ```
/// Host A                 Host B
///   │   SA(SPI=100, A→B) │
///   │──── ESP paket ─────►│ SPI=100 ile şifrele
///   │                     │
///   │   SA(SPI=200, B→A) │
///   │◄─── ESP paket ──────│ SPI=200 ile şifrele
/// ```
#[derive(Debug)]
pub struct SecurityAssociation {
    /// SPI (Güvenlik Parametre İndeksi) - alıcı tarafından belirlenir
    pub spi: u32,
    /// Protokol (ESP/AH)
    pub proto: u8,
    /// Mod (Transport/Tunnel)
    pub mode: u8,
    /// Kaynak IP
    pub src_ip: u32,
    /// Hedef IP
    pub dst_ip: u32,
    /// Şifreleme algoritması
    pub enc_alg: u16,
    /// Şifreleme anahtarı
    pub enc_key: Vec<u8>,
    /// Kimlik doğrulama algoritması
    pub auth_alg: u16,
    /// Kimlik doğrulama anahtarı
    pub auth_key: Vec<u8>,
    /// Tekrar penceresi boyutu (replay window)
    pub replay_window: u32,
    /// Tekrar bitmap'i (hangi sıra numaraları görüldü)
    pub replay_bitmap: AtomicU64,
    /// Son görülen sıra numarası
    pub last_seq: AtomicU32,
    /// Geçerlilik süresi (Unix zaman damgası)
    pub expires: u64,
    /// SA etkin mi?
    pub active: AtomicBool,
    /// İstatistikler
    pub stats: Mutex<SaStats>,
}

/// SA istatistikleri
///
/// Her SA'nın işlediği paket ve bayt sayısını takip eder.
/// Kimlik doğrulama ve tekrar saldırısı hatalarını da sayar.
#[derive(Debug, Default)]
pub struct SaStats {
    pub packets_in: u64,
    pub packets_out: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub auth_errors: u64,   // Kimlik doğrulama başarısız sayısı
    pub replay_errors: u64, // Tekrar saldırısı tespit sayısı
}

impl SecurityAssociation {
    pub fn new(spi: u32, proto: u8, mode: u8) -> Self {
        Self {
            spi,
            proto,
            mode,
            src_ip: 0,
            dst_ip: 0,
            enc_alg: IPSEC_ENC_AES_CBC,
            enc_key: Vec::new(),
            auth_alg: IPSEC_AUTH_HMAC_SHA256,
            auth_key: Vec::new(),
            replay_window: 64,
            replay_bitmap: AtomicU64::new(0),
            last_seq: AtomicU32::new(0),
            expires: 0,
            active: AtomicBool::new(true),
            stats: Mutex::new(SaStats::default()),
        }
    }

    /// Tekrar saldırısını (replay attack) kontrol eder.
    ///
    /// Tekrar saldırısı: saldırgan daha önce yakaladığı geçerli bir paketyi
    /// yeniden göndererek sistemi yanıltmaya çalışır.
    ///
    /// Sliding window (kayan pencere) yöntemi:
    /// ```
    /// last_seq = 100, window = 64
    ///
    ///   36 ... 100
    ///   └──────────┘ geçerli pencere
    ///
    /// seq=101 → yeni, kabul et, window ilerle
    /// seq=99  → pencere içinde, bitmap'e bak
    /// seq=35  → pencereden önce, BEL
    /// seq=50  → bitmap'de var mı? Varsa tekrar saldırısı!
    /// ```
    pub fn check_replay(&self, seq: u32) -> bool {
        let last = self.last_seq.load(Ordering::Relaxed);

        if seq > last {
            // Yeni paket: pencereyi ilerlet ve bitmap güncelle
            let diff = seq - last;
            let mut bitmap = self.replay_bitmap.load(Ordering::Relaxed);

            if diff < 64 {
                bitmap = (bitmap << diff) | 1;
            } else {
                bitmap = 1;
            }

            self.replay_bitmap.store(bitmap, Ordering::Relaxed);
            self.last_seq.store(seq, Ordering::Relaxed);
            return true;
        }

        // Check if in window
        // Pencere dışında eski paket: reddet
        let diff = last - seq;
        if diff >= self.replay_window {
            return false;
        }

        // Check if already seen
        // Bitmap'te kontrol et: bu pozisyon 1 ise daha önce görülmüş → tekrar saldırısı
        let bitmap = self.replay_bitmap.load(Ordering::Relaxed);
        let mask = 1u64 << diff;

        if bitmap & mask != 0 {
            // Already seen
            return false;
        }

        // Mark as seen
        self.replay_bitmap.fetch_or(mask, Ordering::Relaxed);
        true
    }

    /// Encrypt packet
    pub fn encrypt(&self, pkt: &[u8]) -> Result<Vec<u8>, IpsecError> {
        match self.enc_alg {
            IPSEC_ENC_NULL => Ok(pkt.to_vec()),
            IPSEC_ENC_DES_CBC => self.encrypt_des_cbc(pkt),
            IPSEC_ENC_3DES_CBC => self.encrypt_3des_cbc(pkt),
            IPSEC_ENC_AES_CBC => self.encrypt_aes_cbc(pkt),
            IPSEC_ENC_AES_CTR => self.encrypt_aes_ctr(pkt),
            IPSEC_ENC_AES_GCM => self.encrypt_aes_gcm(pkt),
            IPSEC_ENC_CHACHA20_POLY1305 => self.encrypt_chacha20_poly1305(pkt),
            alg_id => {
                let entry = lookup_cipher_registry(alg_id).unwrap_or_else(default_cipher_entry);
                (entry.encrypt)(self, pkt)
            }
        }
    }

    /// Decrypt packet
    pub fn decrypt(&self, pkt: &[u8]) -> Result<Vec<u8>, IpsecError> {
        match self.enc_alg {
            IPSEC_ENC_NULL => Ok(pkt.to_vec()),
            IPSEC_ENC_DES_CBC => self.decrypt_des_cbc(pkt),
            IPSEC_ENC_3DES_CBC => self.decrypt_3des_cbc(pkt),
            IPSEC_ENC_AES_CBC => self.decrypt_aes_cbc(pkt),
            IPSEC_ENC_AES_CTR => self.decrypt_aes_ctr(pkt),
            IPSEC_ENC_AES_GCM => self.decrypt_aes_gcm(pkt),
            IPSEC_ENC_CHACHA20_POLY1305 => self.decrypt_chacha20_poly1305(pkt),
            alg_id => {
                let entry = lookup_cipher_registry(alg_id).unwrap_or_else(default_cipher_entry);
                (entry.decrypt)(self, pkt)
            }
        }
    }

    /// AES-CBC şifreleme (RFC 3602)
    ///
    /// PKCS#7 padding + rastgele IV (16 byte) ile CBC modu.
    fn encrypt_aes_cbc(&self, pkt: &[u8]) -> Result<Vec<u8>, IpsecError> {
        let key = &self.enc_key;
        if key.len() < 16 {
            return Err(IpsecError::InvalidKey);
        }

        // Rastgele IV (16 byte)
        let mut iv = [0u8; 16];
        for i in 0..16 {
            iv[i] = crate::random::next_u32() as u8;
        }

        // PKCS#7 padding
        let pad_len = 16 - (pkt.len() % 16);
        let mut padded = pkt.to_vec();
        for _ in 0..pad_len {
            padded.push(pad_len as u8);
        }

        // CBC şifreleme: C[i] = AES(key, P[i] XOR C[i-1])
        let mut result = Vec::with_capacity(16 + padded.len());
        result.extend_from_slice(&iv);
        let mut prev = iv;
        for chunk in padded.chunks(16) {
            let mut block = [0u8; 16];
            for j in 0..16 {
                block[j] = chunk[j] ^ prev[j];
            }
            let mut encrypted = block;
            let cipher = AesNi::new(key);
            cipher.encrypt_block(&mut encrypted);
            result.extend_from_slice(&encrypted);
            prev = encrypted;
        }

        Ok(result)
    }

    /// AES-CBC şifre çözme
    fn decrypt_aes_cbc(&self, pkt: &[u8]) -> Result<Vec<u8>, IpsecError> {
        let key = &self.enc_key;
        if key.len() < 16 || pkt.len() < 32 || pkt.len() % 16 != 0 {
            return Err(IpsecError::InvalidPacket);
        }

        let iv = &pkt[..16];
        let ciphertext = &pkt[16..];

        // CBC şifre çözme: P[i] = AES_DEC(key, C[i]) XOR C[i-1]
        let mut result = Vec::with_capacity(ciphertext.len());
        let mut prev = [0u8; 16];
        prev.copy_from_slice(iv);
        for chunk in ciphertext.chunks(16) {
            let mut decrypted = [0u8; 16];
            decrypted.copy_from_slice(chunk);
            let cipher = AesNi::new(key);
            cipher.decrypt_block(&mut decrypted);
            let mut plaintext = [0u8; 16];
            for j in 0..16 {
                plaintext[j] = decrypted[j] ^ prev[j];
            }
            result.extend_from_slice(&plaintext);
            prev.copy_from_slice(chunk);
        }

        // PKCS#7 padding kaldır
        if let Some(&pad_len) = result.last() {
            let pad = pad_len as usize;
            if pad > 0 && pad <= 16 && result.len() >= pad {
                result.truncate(result.len() - pad);
            }
        }

        Ok(result)
    }

    fn encrypt_des_cbc(&self, pkt: &[u8]) -> Result<Vec<u8>, IpsecError> {
        let key = self
            .enc_key
            .get(..DES_BLOCK_SIZE)
            .ok_or(IpsecError::InvalidKey)?;

        let mut iv = [0u8; DES_BLOCK_SIZE];
        for byte in &mut iv {
            *byte = crate::random::next_u32() as u8;
        }

        let mut padded = pkt.to_vec();
        let pad_len = DES_BLOCK_SIZE - (pkt.len() % DES_BLOCK_SIZE);
        padded.extend(core::iter::repeat_n(pad_len as u8, pad_len));

        let subkeys = Self::des_generate_subkeys(key.try_into().unwrap());
        let mut result = Vec::with_capacity(DES_BLOCK_SIZE + padded.len());
        result.extend_from_slice(&iv);

        let mut prev = iv;
        for chunk in padded.chunks(DES_BLOCK_SIZE) {
            let mut block = [0u8; DES_BLOCK_SIZE];
            for i in 0..DES_BLOCK_SIZE {
                block[i] = chunk[i] ^ prev[i];
            }
            let encrypted = Self::des_encrypt_block(&block, &subkeys);
            result.extend_from_slice(&encrypted);
            prev = encrypted;
        }

        Ok(result)
    }

    fn decrypt_des_cbc(&self, pkt: &[u8]) -> Result<Vec<u8>, IpsecError> {
        let key = self
            .enc_key
            .get(..DES_BLOCK_SIZE)
            .ok_or(IpsecError::InvalidKey)?;
        if pkt.len() < DES_BLOCK_SIZE * 2 || pkt.len() % DES_BLOCK_SIZE != 0 {
            return Err(IpsecError::InvalidPacket);
        }

        let subkeys = Self::des_generate_subkeys(key.try_into().unwrap());
        let mut prev = [0u8; DES_BLOCK_SIZE];
        prev.copy_from_slice(&pkt[..DES_BLOCK_SIZE]);

        let mut plaintext = Vec::with_capacity(pkt.len() - DES_BLOCK_SIZE);
        for chunk in pkt[DES_BLOCK_SIZE..].chunks(DES_BLOCK_SIZE) {
            let mut block = [0u8; DES_BLOCK_SIZE];
            block.copy_from_slice(chunk);
            let decrypted = Self::des_decrypt_block(&block, &subkeys);
            for i in 0..DES_BLOCK_SIZE {
                plaintext.push(decrypted[i] ^ prev[i]);
            }
            prev = block;
        }

        Self::strip_pkcs7(&mut plaintext, DES_BLOCK_SIZE)?;
        Ok(plaintext)
    }

    fn encrypt_3des_cbc(&self, pkt: &[u8]) -> Result<Vec<u8>, IpsecError> {
        let (k1, k2, k3) = Self::extract_3des_keys(&self.enc_key)?;
        let mut iv = [0u8; DES_BLOCK_SIZE];
        for byte in &mut iv {
            *byte = crate::random::next_u32() as u8;
        }

        let mut padded = pkt.to_vec();
        let pad_len = DES_BLOCK_SIZE - (pkt.len() % DES_BLOCK_SIZE);
        padded.extend(core::iter::repeat_n(pad_len as u8, pad_len));

        let sk1 = Self::des_generate_subkeys(&k1);
        let sk2 = Self::des_generate_subkeys(&k2);
        let sk3 = Self::des_generate_subkeys(&k3);

        let mut result = Vec::with_capacity(DES_BLOCK_SIZE + padded.len());
        result.extend_from_slice(&iv);
        let mut prev = iv;
        for chunk in padded.chunks(DES_BLOCK_SIZE) {
            let mut block = [0u8; DES_BLOCK_SIZE];
            for i in 0..DES_BLOCK_SIZE {
                block[i] = chunk[i] ^ prev[i];
            }
            let encrypted = Self::des3_encrypt_block(&block, &sk1, &sk2, &sk3);
            result.extend_from_slice(&encrypted);
            prev = encrypted;
        }

        Ok(result)
    }

    fn decrypt_3des_cbc(&self, pkt: &[u8]) -> Result<Vec<u8>, IpsecError> {
        let (k1, k2, k3) = Self::extract_3des_keys(&self.enc_key)?;
        if pkt.len() < DES_BLOCK_SIZE * 2 || pkt.len() % DES_BLOCK_SIZE != 0 {
            return Err(IpsecError::InvalidPacket);
        }

        let sk1 = Self::des_generate_subkeys(&k1);
        let sk2 = Self::des_generate_subkeys(&k2);
        let sk3 = Self::des_generate_subkeys(&k3);

        let mut prev = [0u8; DES_BLOCK_SIZE];
        prev.copy_from_slice(&pkt[..DES_BLOCK_SIZE]);
        let mut plaintext = Vec::with_capacity(pkt.len() - DES_BLOCK_SIZE);
        for chunk in pkt[DES_BLOCK_SIZE..].chunks(DES_BLOCK_SIZE) {
            let mut block = [0u8; DES_BLOCK_SIZE];
            block.copy_from_slice(chunk);
            let decrypted = Self::des3_decrypt_block(&block, &sk1, &sk2, &sk3);
            for i in 0..DES_BLOCK_SIZE {
                plaintext.push(decrypted[i] ^ prev[i]);
            }
            prev = block;
        }

        Self::strip_pkcs7(&mut plaintext, DES_BLOCK_SIZE)?;
        Ok(plaintext)
    }

    /// AES-CTR şifreleme.
    ///
    /// Paket başına 16 byte nonce/counter bloğu üretir; sayaç big-endian artar.
    fn encrypt_aes_ctr(&self, pkt: &[u8]) -> Result<Vec<u8>, IpsecError> {
        let key = &self.enc_key;
        if key.len() < 16 {
            return Err(IpsecError::InvalidKey);
        }

        let mut counter_block = [0u8; 16];
        for byte in &mut counter_block {
            *byte = crate::random::next_u32() as u8;
        }

        let mut result = Vec::with_capacity(16 + pkt.len());
        result.extend_from_slice(&counter_block);

        for chunk in pkt.chunks(16) {
            let mut keystream = counter_block;
            let cipher = AesNi::new(key);
            cipher.encrypt_block(&mut keystream);
            for (idx, &byte) in chunk.iter().enumerate() {
                result.push(byte ^ keystream[idx]);
            }
            Self::increment_counter_be(&mut counter_block);
        }

        Ok(result)
    }

    fn decrypt_aes_ctr(&self, pkt: &[u8]) -> Result<Vec<u8>, IpsecError> {
        let key = &self.enc_key;
        if key.len() < 16 || pkt.len() < 16 {
            return Err(IpsecError::InvalidPacket);
        }

        let mut counter_block = [0u8; 16];
        counter_block.copy_from_slice(&pkt[..16]);

        let mut plaintext = Vec::with_capacity(pkt.len() - 16);
        for chunk in pkt[16..].chunks(16) {
            let mut keystream = counter_block;
            let cipher = AesNi::new(key);
            cipher.encrypt_block(&mut keystream);
            for (idx, &byte) in chunk.iter().enumerate() {
                plaintext.push(byte ^ keystream[idx]);
            }
            Self::increment_counter_be(&mut counter_block);
        }

        Ok(plaintext)
    }

    /// AES-GCM şifreleme (RFC 4106)
    ///
    /// 8 byte IV + AES-CTR şifreleme + GHASH authentication tag.
    fn encrypt_aes_gcm(&self, pkt: &[u8]) -> Result<Vec<u8>, IpsecError> {
        let key = &self.enc_key;
        if key.len() < 16 {
            return Err(IpsecError::InvalidKey);
        }

        // 8 byte explicit nonce (4 byte salt oturumda saklı; 8 byte explicit)
        let mut nonce = [0u8; 12];
        // Salt: key'in son 4 byte'ı
        if self.enc_key.len() >= 20 {
            nonce[..4].copy_from_slice(&self.enc_key[16..20]);
        }
        for i in 4..12 {
            nonce[i] = crate::random::next_u32() as u8;
        }

        let cipher = AesNi::new(key);
        let (ciphertext, tag) = self.aes_gcm_seal(&cipher, &nonce, pkt, &[])?;

        let mut result = Vec::with_capacity(8 + ciphertext.len() + tag.len());
        result.extend_from_slice(&nonce[4..12]); // 8 byte explicit nonce
        result.extend_from_slice(&ciphertext);
        result.extend_from_slice(&tag);
        Ok(result)
    }

    /// AES-GCM şifre çözme
    fn decrypt_aes_gcm(&self, pkt: &[u8]) -> Result<Vec<u8>, IpsecError> {
        let key = &self.enc_key;
        if key.len() < 16 || pkt.len() < 24 {
            return Err(IpsecError::InvalidPacket);
        }

        // Nonce yeniden oluştur: 4 byte salt + 8 byte explicit
        let mut nonce = [0u8; 12];
        if self.enc_key.len() >= 20 {
            nonce[..4].copy_from_slice(&self.enc_key[16..20]);
        }
        nonce[4..12].copy_from_slice(&pkt[..8]);

        let ciphertext_with_tag = &pkt[8..];
        if ciphertext_with_tag.len() < 16 {
            return Err(IpsecError::InvalidPacket);
        }

        let cipher = AesNi::new(key);
        self.aes_gcm_open(&cipher, &nonce, ciphertext_with_tag, &[])
    }

    fn encrypt_chacha20_poly1305(&self, pkt: &[u8]) -> Result<Vec<u8>, IpsecError> {
        let key: [u8; 32] = self
            .enc_key
            .get(..32)
            .ok_or(IpsecError::InvalidKey)?
            .try_into()
            .map_err(|_| IpsecError::InvalidKey)?;

        let mut nonce = [0u8; 12];
        for byte in &mut nonce {
            *byte = crate::random::next_u32() as u8;
        }

        let mut cipher = ChaCha20Poly1305::new(&key, &nonce);
        let (ciphertext, tag) = cipher.encrypt(pkt, &[]);

        let mut result = Vec::with_capacity(12 + ciphertext.len() + tag.len());
        result.extend_from_slice(&nonce);
        result.extend_from_slice(&ciphertext);
        result.extend_from_slice(&tag);
        Ok(result)
    }

    fn decrypt_chacha20_poly1305(&self, pkt: &[u8]) -> Result<Vec<u8>, IpsecError> {
        let key: [u8; 32] = self
            .enc_key
            .get(..32)
            .ok_or(IpsecError::InvalidKey)?
            .try_into()
            .map_err(|_| IpsecError::InvalidKey)?;

        if pkt.len() < 28 {
            return Err(IpsecError::InvalidPacket);
        }

        let mut nonce = [0u8; 12];
        nonce.copy_from_slice(&pkt[..12]);
        let ciphertext = &pkt[12..pkt.len() - 16];
        let mut tag = [0u8; 16];
        tag.copy_from_slice(&pkt[pkt.len() - 16..]);

        let mut cipher = ChaCha20Poly1305::new(&key, &nonce);
        cipher
            .decrypt(ciphertext, &[], &tag)
            .ok_or(IpsecError::DecryptionFailed)
    }

    fn increment_counter_be(counter: &mut [u8; 16]) {
        for byte in counter.iter_mut().rev() {
            let (next, carry) = byte.overflowing_add(1);
            *byte = next;
            if !carry {
                break;
            }
        }
    }

    fn increment_counter32(counter: &mut [u8; 16]) {
        for idx in (12..16).rev() {
            let (next, carry) = counter[idx].overflowing_add(1);
            counter[idx] = next;
            if !carry {
                break;
            }
        }
    }

    fn ghash_tag(h: &[u8; 16], aad: &[u8], ciphertext: &[u8]) -> [u8; 16] {
        let mut len_block = [0u8; 16];
        len_block[..8].copy_from_slice(&((aad.len() as u64) * 8).to_be_bytes());
        len_block[8..].copy_from_slice(&((ciphertext.len() as u64) * 8).to_be_bytes());

        if ClMulGhash::is_available() {
            ClMulGhash::new(h).gcm_tag(aad, ciphertext, len_block)
        } else {
            let mut data = Vec::new();
            data.extend_from_slice(aad);

            let aad_pad = (16 - (aad.len() % 16)) % 16;
            data.extend(core::iter::repeat_n(0, aad_pad));

            data.extend_from_slice(ciphertext);

            let ct_pad = (16 - (ciphertext.len() % 16)) % 16;
            data.extend(core::iter::repeat_n(0, ct_pad));

            data.extend_from_slice(&len_block);
            GhashSoft::new(h).ghash(&data)
        }
    }

    fn aes_gcm_seal(
        &self,
        cipher: &AesNi,
        nonce: &[u8; 12],
        plaintext: &[u8],
        aad: &[u8],
    ) -> Result<(Vec<u8>, [u8; 16]), IpsecError> {
        let mut hash_subkey = [0u8; 16];
        cipher.encrypt_block(&mut hash_subkey);

        let mut j0 = [0u8; 16];
        j0[..12].copy_from_slice(nonce);
        j0[15] = 1;

        let mut counter = j0;
        Self::increment_counter32(&mut counter);

        let mut ciphertext = Vec::with_capacity(plaintext.len());
        for chunk in plaintext.chunks(16) {
            let mut keystream = counter;
            cipher.encrypt_block(&mut keystream);
            for (idx, &byte) in chunk.iter().enumerate() {
                ciphertext.push(byte ^ keystream[idx]);
            }
            Self::increment_counter32(&mut counter);
        }

        let ghash = Self::ghash_tag(&hash_subkey, aad, &ciphertext);
        let mut tag_mask = j0;
        cipher.encrypt_block(&mut tag_mask);

        let mut tag = [0u8; 16];
        for idx in 0..16 {
            tag[idx] = ghash[idx] ^ tag_mask[idx];
        }

        Ok((ciphertext, tag))
    }

    fn aes_gcm_open(
        &self,
        cipher: &AesNi,
        nonce: &[u8; 12],
        ciphertext_with_tag: &[u8],
        aad: &[u8],
    ) -> Result<Vec<u8>, IpsecError> {
        if ciphertext_with_tag.len() < 16 {
            return Err(IpsecError::InvalidPacket);
        }

        let ciphertext_len = ciphertext_with_tag.len() - 16;
        let ciphertext = &ciphertext_with_tag[..ciphertext_len];
        let recv_tag = &ciphertext_with_tag[ciphertext_len..];

        let mut hash_subkey = [0u8; 16];
        cipher.encrypt_block(&mut hash_subkey);

        let mut j0 = [0u8; 16];
        j0[..12].copy_from_slice(nonce);
        j0[15] = 1;

        let ghash = Self::ghash_tag(&hash_subkey, aad, ciphertext);
        let mut tag_mask = j0;
        cipher.encrypt_block(&mut tag_mask);

        let mut expected_tag = [0u8; 16];
        for idx in 0..16 {
            expected_tag[idx] = ghash[idx] ^ tag_mask[idx];
        }

        let mut tag_diff = 0u8;
        for idx in 0..16 {
            tag_diff |= expected_tag[idx] ^ recv_tag[idx];
        }
        if tag_diff != 0 {
            return Err(IpsecError::DecryptionFailed);
        }

        let mut counter = j0;
        Self::increment_counter32(&mut counter);

        let mut plaintext = Vec::with_capacity(ciphertext.len());
        for chunk in ciphertext.chunks(16) {
            let mut keystream = counter;
            cipher.encrypt_block(&mut keystream);
            for (idx, &byte) in chunk.iter().enumerate() {
                plaintext.push(byte ^ keystream[idx]);
            }
            Self::increment_counter32(&mut counter);
        }

        Ok(plaintext)
    }

    /// ICV (Integrity Check Value) hesapla.
    ///
    /// Kullanılan algoritmaya göre HMAC veya AES-XCBC-96 ile
    /// bütünlük kontrol değeri hesaplar.
    pub fn calculate_icv(&self, data: &[u8]) -> Vec<u8> {
        let auth_key = if self.auth_key.is_empty() {
            &self.enc_key
        } else {
            &self.auth_key
        };

        let icv_len = match self.auth_alg {
            IPSEC_AUTH_HMAC_SHA1 => 12,
            IPSEC_AUTH_HMAC_SHA256 => 16,
            IPSEC_AUTH_HMAC_SHA384 => 24,
            IPSEC_AUTH_HMAC_SHA512 => 32,
            IPSEC_AUTH_AES_XCBC => 12,
            _ => lookup_auth_registry(self.auth_alg)
                .map(|entry| entry.icv_len)
                .unwrap_or_else(|| default_auth_icv_len(auth_key)),
        };

        if self.auth_alg == IPSEC_AUTH_AES_XCBC {
            return Self::aes_xcbc_mac_96(auth_key, data)
                .map(|tag| tag.to_vec())
                .unwrap_or_default();
        }
        let full_hmac = match self.auth_alg {
            IPSEC_AUTH_HMAC_MD5 => Self::hmac_with_pad(auth_key, data, 64, Self::md5_digest),
            IPSEC_AUTH_HMAC_SHA1 => Self::hmac_with_pad(auth_key, data, 64, |msg| {
                let mut hasher = Sha1::new();
                hasher.update(msg);
                hasher.finalize().to_vec()
            }),
            IPSEC_AUTH_HMAC_SHA256 => crate::net::quic::hmac_sha256(auth_key, data),
            IPSEC_AUTH_HMAC_SHA384 => Self::hmac_with_pad(auth_key, data, 128, |msg| {
                let mut hasher = Sha384::new();
                hasher.update(msg);
                hasher.finalize().to_vec()
            }),
            IPSEC_AUTH_HMAC_SHA512 => Self::hmac_with_pad(auth_key, data, 128, |msg| {
                let mut hasher = Sha512::new();
                hasher.update(msg);
                hasher.finalize().to_vec()
            }),
            _ => lookup_auth_registry(self.auth_alg)
                .and_then(|entry| (entry.calculate)(self, data).ok())
                .unwrap_or_else(|| calculate_default_auth_icv(auth_key, data)),
        };

        // Truncate to icv_len
        full_hmac[..icv_len.min(full_hmac.len())].to_vec()
    }

    /// ICV doğrula
    pub fn verify_icv(&self, data: &[u8], icv: &[u8]) -> bool {
        let expected = self.calculate_icv(data);
        // Sabit zamanlı karşılaştırma (timing attack koruması)
        if expected.len() != icv.len() {
            return false;
        }
        let mut diff = 0u8;
        for (a, b) in expected.iter().zip(icv.iter()) {
            diff |= a ^ b;
        }
        diff == 0
    }

    fn strip_pkcs7(buf: &mut Vec<u8>, block_size: usize) -> Result<(), IpsecError> {
        let Some(&pad_len) = buf.last() else {
            return Err(IpsecError::InvalidPacket);
        };
        let pad = pad_len as usize;
        if pad == 0 || pad > block_size || buf.len() < pad {
            return Err(IpsecError::InvalidPacket);
        }
        if !buf[buf.len() - pad..]
            .iter()
            .all(|&byte| byte as usize == pad)
        {
            return Err(IpsecError::InvalidPacket);
        }
        buf.truncate(buf.len() - pad);
        Ok(())
    }

    fn hmac_with_pad<F>(key: &[u8], message: &[u8], block_size: usize, hash: F) -> Vec<u8>
    where
        F: Fn(&[u8]) -> Vec<u8>,
    {
        let mut working_key = if key.len() > block_size {
            hash(key)
        } else {
            key.to_vec()
        };
        working_key.resize(block_size, 0);

        let mut inner = vec![0x36u8; block_size];
        let mut outer = vec![0x5cu8; block_size];
        for (idx, byte) in working_key.iter().enumerate() {
            inner[idx] ^= byte;
            outer[idx] ^= byte;
        }

        inner.extend_from_slice(message);
        let inner_hash = hash(&inner);
        outer.extend_from_slice(&inner_hash);
        hash(&outer)
    }

    fn aes_xcbc_mac_96(key: &[u8], message: &[u8]) -> Result<[u8; 12], IpsecError> {
        let key128 = key.get(..16).ok_or(IpsecError::InvalidKey)?;
        let master = AesNi::new(key128);

        let mut k1 = [0x01u8; 16];
        let mut k2 = [0x02u8; 16];
        let mut k3 = [0x03u8; 16];
        master.encrypt_block(&mut k1);
        master.encrypt_block(&mut k2);
        master.encrypt_block(&mut k3);

        let cipher = AesNi::new(&k1);
        let mut state = [0u8; 16];

        let full_blocks = message.len() / 16;
        let last_is_complete = !message.is_empty() && message.len() % 16 == 0;
        let prefix_blocks = if last_is_complete {
            full_blocks.saturating_sub(1)
        } else {
            full_blocks
        };

        for block in message.chunks(16).take(prefix_blocks) {
            let mut xored = [0u8; 16];
            xored.copy_from_slice(block);
            Self::xor_block(&mut xored, &state);
            cipher.encrypt_block(&mut xored);
            state = xored;
        }

        let mut last = [0u8; 16];
        if last_is_complete {
            let start = (full_blocks - 1) * 16;
            last.copy_from_slice(&message[start..start + 16]);
            Self::xor_block(&mut last, &k2);
        } else {
            let remainder = message.len() % 16;
            if remainder != 0 {
                last[..remainder].copy_from_slice(&message[message.len() - remainder..]);
            }
            last[remainder] = 0x80;
            Self::xor_block(&mut last, &k3);
        }

        Self::xor_block(&mut last, &state);
        cipher.encrypt_block(&mut last);

        let mut mac = [0u8; 12];
        mac.copy_from_slice(&last[..12]);
        Ok(mac)
    }

    fn xor_block(block: &mut [u8; 16], other: &[u8; 16]) {
        for idx in 0..16 {
            block[idx] ^= other[idx];
        }
    }

    fn md5_digest(message: &[u8]) -> Vec<u8> {
        const S: [u32; 64] = [
            7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20,
            5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23,
            6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
        ];
        const K: [u32; 64] = [
            0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
            0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
            0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
            0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
            0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
            0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
            0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
            0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
            0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
            0xeb86d391,
        ];

        let mut state = [0x67452301u32, 0xefcdab89, 0x98badcfe, 0x10325476];
        let bit_len = (message.len() as u64) * 8;
        let mut padded = message.to_vec();
        padded.push(0x80);
        while padded.len() % 64 != 56 {
            padded.push(0);
        }
        padded.extend_from_slice(&bit_len.to_le_bytes());

        for chunk in padded.chunks_exact(64) {
            let mut m = [0u32; 16];
            for (idx, word) in m.iter_mut().enumerate() {
                let start = idx * 4;
                *word = u32::from_le_bytes(chunk[start..start + 4].try_into().unwrap());
            }

            let (mut a, mut b, mut c, mut d) = (state[0], state[1], state[2], state[3]);
            for i in 0..64 {
                let (f, g) = match i {
                    0..=15 => ((b & c) | ((!b) & d), i),
                    16..=31 => ((d & b) | ((!d) & c), (5 * i + 1) % 16),
                    32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                    _ => (c ^ (b | !d), (7 * i) % 16),
                };
                let tmp = d;
                d = c;
                c = b;
                b = b.wrapping_add(
                    a.wrapping_add(f)
                        .wrapping_add(K[i])
                        .wrapping_add(m[g])
                        .rotate_left(S[i]),
                );
                a = tmp;
            }

            state[0] = state[0].wrapping_add(a);
            state[1] = state[1].wrapping_add(b);
            state[2] = state[2].wrapping_add(c);
            state[3] = state[3].wrapping_add(d);
        }

        let mut digest = Vec::with_capacity(16);
        for word in state {
            digest.extend_from_slice(&word.to_le_bytes());
        }
        digest
    }

    fn extract_3des_keys(key: &[u8]) -> Result<([u8; 8], [u8; 8], [u8; 8]), IpsecError> {
        let k1: [u8; 8] = key
            .get(..8)
            .ok_or(IpsecError::InvalidKey)?
            .try_into()
            .unwrap();
        let k2: [u8; 8] = key
            .get(8..16)
            .ok_or(IpsecError::InvalidKey)?
            .try_into()
            .unwrap();
        let k3: [u8; 8] = if key.len() >= 24 {
            key[16..24].try_into().unwrap()
        } else {
            k1
        };
        Ok((k1, k2, k3))
    }

    fn des3_encrypt_block(
        block: &[u8; 8],
        sk1: &[u64; 16],
        sk2: &[u64; 16],
        sk3: &[u64; 16],
    ) -> [u8; 8] {
        let step1 = Self::des_encrypt_block(block, sk1);
        let step2 = Self::des_decrypt_block(&step1, sk2);
        Self::des_encrypt_block(&step2, sk3)
    }

    fn des3_decrypt_block(
        block: &[u8; 8],
        sk1: &[u64; 16],
        sk2: &[u64; 16],
        sk3: &[u64; 16],
    ) -> [u8; 8] {
        let step1 = Self::des_decrypt_block(block, sk3);
        let step2 = Self::des_encrypt_block(&step1, sk2);
        Self::des_decrypt_block(&step2, sk1)
    }

    fn des_encrypt_block(block: &[u8; 8], subkeys: &[u64; 16]) -> [u8; 8] {
        Self::des_process_block(block, subkeys, false)
    }

    fn des_decrypt_block(block: &[u8; 8], subkeys: &[u64; 16]) -> [u8; 8] {
        Self::des_process_block(block, subkeys, true)
    }

    fn des_process_block(block: &[u8; 8], subkeys: &[u64; 16], reverse: bool) -> [u8; 8] {
        let permuted = Self::des_permute(u64::from_be_bytes(*block), 64, &DES_IP);
        let mut left = (permuted >> 32) as u32;
        let mut right = permuted as u32;

        for round in 0..16 {
            let key_index = if reverse { 15 - round } else { round };
            let next = left ^ Self::des_feistel(right, subkeys[key_index]);
            left = right;
            right = next;
        }

        let preoutput = ((right as u64) << 32) | left as u64;
        Self::des_permute(preoutput, 64, &DES_FP).to_be_bytes()
    }

    fn des_generate_subkeys(key: &[u8; 8]) -> [u64; 16] {
        let permuted = Self::des_permute(u64::from_be_bytes(*key), 64, &DES_PC1);
        let mut c = ((permuted >> 28) & 0x0fff_ffff) as u32;
        let mut d = (permuted & 0x0fff_ffff) as u32;
        let mut subkeys = [0u64; 16];

        for (round, shift) in DES_SHIFTS.iter().enumerate() {
            c = Self::des_rotate28(c, *shift);
            d = Self::des_rotate28(d, *shift);
            let joined = ((c as u64) << 28) | d as u64;
            subkeys[round] = Self::des_permute(joined, 56, &DES_PC2);
        }

        subkeys
    }

    fn des_rotate28(value: u32, shift: u8) -> u32 {
        ((value << shift) | (value >> (28 - shift))) & 0x0fff_ffff
    }

    fn des_feistel(right: u32, subkey: u64) -> u32 {
        let expanded = Self::des_permute(right as u64, 32, &DES_E) ^ subkey;
        let mut sbox_out = 0u32;
        for (idx, sbox) in DES_SBOXES.iter().enumerate() {
            let shift = 42 - idx * 6;
            let chunk = ((expanded >> shift) & 0x3f) as u8;
            let row = ((chunk & 0x20) >> 4) | (chunk & 0x01);
            let col = (chunk >> 1) & 0x0f;
            sbox_out = (sbox_out << 4) | sbox[row as usize][col as usize] as u32;
        }
        Self::des_permute(sbox_out as u64, 32, &DES_P) as u32
    }

    fn des_permute(input: u64, input_bits: usize, table: &[u8]) -> u64 {
        let mut output = 0u64;
        for &bit_pos in table {
            let shift = input_bits - bit_pos as usize;
            output = (output << 1) | ((input >> shift) & 1);
        }
        output
    }
}

fn default_cipher_entry() -> IpsecCipherRegistryEntry {
    IpsecCipherRegistryEntry {
        encrypt: default_encrypt_cipher,
        decrypt: default_decrypt_cipher,
    }
}

fn default_encrypt_cipher(sa: &SecurityAssociation, pkt: &[u8]) -> Result<Vec<u8>, IpsecError> {
    if sa.enc_key.len() >= 32 {
        sa.encrypt_chacha20_poly1305(pkt)
    } else if sa.enc_key.len() >= 16 {
        sa.encrypt_aes_ctr(pkt)
    } else if sa.enc_key.len() >= 8 {
        sa.encrypt_des_cbc(pkt)
    } else {
        Err(IpsecError::InvalidKey)
    }
}

fn default_decrypt_cipher(sa: &SecurityAssociation, pkt: &[u8]) -> Result<Vec<u8>, IpsecError> {
    if sa.enc_key.len() >= 32 {
        sa.decrypt_chacha20_poly1305(pkt)
    } else if sa.enc_key.len() >= 16 {
        sa.decrypt_aes_ctr(pkt)
    } else if sa.enc_key.len() >= 8 {
        sa.decrypt_des_cbc(pkt)
    } else {
        Err(IpsecError::InvalidKey)
    }
}

fn default_auth_icv_len(auth_key: &[u8]) -> usize {
    if auth_key.len() >= 64 {
        32
    } else if auth_key.len() >= 48 {
        24
    } else if auth_key.len() >= 16 {
        16
    } else {
        12
    }
}

fn calculate_default_auth_icv(auth_key: &[u8], data: &[u8]) -> Vec<u8> {
    if auth_key.len() >= 64 {
        SecurityAssociation::hmac_with_pad(auth_key, data, 128, |msg| {
            let mut hasher = Sha512::new();
            hasher.update(msg);
            hasher.finalize().to_vec()
        })
    } else if auth_key.len() >= 48 {
        SecurityAssociation::hmac_with_pad(auth_key, data, 128, |msg| {
            let mut hasher = Sha384::new();
            hasher.update(msg);
            hasher.finalize().to_vec()
        })
    } else if auth_key.len() >= 16 {
        crate::net::quic::hmac_sha256(auth_key, data)
    } else {
        SecurityAssociation::hmac_with_pad(auth_key, data, 64, |msg| {
            let mut hasher = Sha1::new();
            hasher.update(msg);
            hasher.finalize().to_vec()
        })
    }
}

// ============================================================================
// SECURITY POLICY (SP)
// ============================================================================

#[derive(Clone, Debug)]
pub struct SecurityPolicy {
    /// Policy ID
    pub id: u32,
    /// Direction
    pub dir: u8,
    /// Source IP range
    pub src_ip: u32,
    pub src_mask: u32,
    /// Destination IP range
    pub dst_ip: u32,
    pub dst_mask: u32,
    /// Protocol
    pub proto: u8,
    /// Port range
    pub src_port: (u16, u16),
    pub dst_port: (u16, u16),
    /// Action
    pub action: PolicyAction,
    /// Priority
    pub priority: u32,
    /// Associated SA
    pub sa_spi: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyAction {
    Discard,
    None,
    Ipsec,
}

impl SecurityPolicy {
    pub fn new(id: u32, dir: u8) -> Self {
        Self {
            id,
            dir,
            src_ip: 0,
            src_mask: 0,
            dst_ip: 0,
            dst_mask: 0,
            proto: 0,
            src_port: (0, 65535),
            dst_port: (0, 65535),
            action: PolicyAction::None,
            priority: 1000,
            sa_spi: None,
        }
    }

    /// Check if packet matches policy
    pub fn matches(
        &self,
        src_ip: u32,
        dst_ip: u32,
        proto: u8,
        src_port: u16,
        dst_port: u16,
    ) -> bool {
        if (src_ip & self.src_mask) != (self.src_ip & self.src_mask) {
            return false;
        }
        if (dst_ip & self.dst_mask) != (self.dst_ip & self.dst_mask) {
            return false;
        }
        if self.proto != 0 && proto != self.proto {
            return false;
        }
        if src_port < self.src_port.0 || src_port > self.src_port.1 {
            return false;
        }
        if dst_port < self.dst_port.0 || dst_port > self.dst_port.1 {
            return false;
        }
        true
    }
}

// ============================================================================
// IPSEC MANAGER
// ============================================================================

pub struct IpsecManager {
    /// Security Associations (SPI -> SA)
    sas: Mutex<BTreeMap<u32, Arc<SecurityAssociation>>>,
    /// Security Policies
    sps_inbound: Mutex<Vec<SecurityPolicy>>,
    sps_outbound: Mutex<Vec<SecurityPolicy>>,
    /// SPI counter
    next_spi: AtomicU32,
    /// Policy ID counter
    next_policy_id: AtomicU32,
    /// Enabled
    enabled: AtomicBool,
    /// Statistics
    stats: Mutex<IpsecStats>,
}

#[derive(Clone, Debug, Default)]
pub struct IpsecStats {
    pub sa_count: u32,
    pub sp_count: u32,
    pub packets_encrypted: u64,
    pub packets_decrypted: u64,
    pub auth_failures: u64,
    pub replay_failures: u64,
}

impl IpsecManager {
    pub fn new() -> Self {
        Self {
            sas: Mutex::new(BTreeMap::new()),
            sps_inbound: Mutex::new(Vec::new()),
            sps_outbound: Mutex::new(Vec::new()),
            next_spi: AtomicU32::new(0x1000000),
            next_policy_id: AtomicU32::new(1),
            enabled: AtomicBool::new(false),
            stats: Mutex::new(IpsecStats {
                sa_count: 0,
                sp_count: 0,
                packets_encrypted: 0,
                packets_decrypted: 0,
                auth_failures: 0,
                replay_failures: 0,
            }),
        }
    }

    /// Create new SA
    pub fn create_sa(&self, proto: u8, mode: u8) -> Arc<SecurityAssociation> {
        let spi = self.next_spi.fetch_add(1, Ordering::SeqCst);
        let sa = Arc::new(SecurityAssociation::new(spi, proto, mode));
        self.sas.lock().insert(spi, sa.clone());

        let mut stats = self.stats.lock();
        stats.sa_count += 1;

        sa
    }

    /// Get SA by SPI
    pub fn get_sa(&self, spi: u32) -> Option<Arc<SecurityAssociation>> {
        self.sas.lock().get(&spi).cloned()
    }

    /// Delete SA
    pub fn delete_sa(&self, spi: u32) {
        self.sas.lock().remove(&spi);
    }

    /// Add security policy
    pub fn add_policy(&self, policy: SecurityPolicy) {
        match policy.dir {
            IPSEC_DIR_INBOUND => self.sps_inbound.lock().push(policy),
            IPSEC_DIR_OUTBOUND => self.sps_outbound.lock().push(policy),
            _ => {}
        }

        let mut stats = self.stats.lock();
        stats.sp_count += 1;
    }

    /// Find policy for outbound packet
    pub fn find_outbound_policy(
        &self,
        src_ip: u32,
        dst_ip: u32,
        proto: u8,
        src_port: u16,
        dst_port: u16,
    ) -> Option<SecurityPolicy> {
        let policies = self.sps_outbound.lock();
        for policy in policies.iter() {
            if policy.matches(src_ip, dst_ip, proto, src_port, dst_port) {
                return Some(policy.clone());
            }
        }
        None
    }

    /// Find policy for inbound packet
    pub fn find_inbound_policy(
        &self,
        src_ip: u32,
        dst_ip: u32,
        proto: u8,
        src_port: u16,
        dst_port: u16,
    ) -> Option<SecurityPolicy> {
        let policies = self.sps_inbound.lock();
        for policy in policies.iter() {
            if policy.matches(src_ip, dst_ip, proto, src_port, dst_port) {
                return Some(policy.clone());
            }
        }
        None
    }

    /// Process outbound packet
    pub fn process_outbound(
        &self,
        pkt: &mut [u8],
        src_ip: u32,
        dst_ip: u32,
        proto: u8,
        src_port: u16,
        dst_port: u16,
    ) -> Result<Vec<u8>, IpsecError> {
        if !self.enabled.load(Ordering::SeqCst) {
            return Ok(pkt.to_vec());
        }

        if let Some(policy) = self.find_outbound_policy(src_ip, dst_ip, proto, src_port, dst_port) {
            if policy.action == PolicyAction::Ipsec {
                if let Some(spi) = policy.sa_spi {
                    if let Some(sa) = self.get_sa(spi) {
                        let encrypted = sa.encrypt(pkt)?;
                        let icv = sa.calculate_icv(&encrypted);

                        // Build ESP packet
                        let mut esp_pkt = Vec::new();
                        esp_pkt.extend_from_slice(&spi.to_be_bytes());
                        esp_pkt
                            .extend_from_slice(&sa.last_seq.load(Ordering::Relaxed).to_be_bytes());
                        esp_pkt.extend_from_slice(&encrypted);
                        esp_pkt.extend_from_slice(&icv);

                        let mut stats = self.stats.lock();
                        stats.packets_encrypted += 1;

                        return Ok(esp_pkt);
                    }
                }
            }
        }

        Ok(pkt.to_vec())
    }

    /// Process inbound packet
    pub fn process_inbound(&self, pkt: &[u8]) -> Result<Vec<u8>, IpsecError> {
        if !self.enabled.load(Ordering::SeqCst) {
            return Ok(pkt.to_vec());
        }

        // Parse ESP header
        if pkt.len() < 8 {
            return Err(IpsecError::InvalidPacket);
        }

        let spi = u32::from_be_bytes([pkt[0], pkt[1], pkt[2], pkt[3]]);
        let seq = u32::from_be_bytes([pkt[4], pkt[5], pkt[6], pkt[7]]);

        if let Some(sa) = self.get_sa(spi) {
            // Check replay
            if !sa.check_replay(seq) {
                let mut stats = self.stats.lock();
                stats.replay_failures += 1;
                return Err(IpsecError::ReplayAttack);
            }

            // Decrypt
            let decrypted = sa.decrypt(&pkt[8..])?;

            let mut stats = self.stats.lock();
            stats.packets_decrypted += 1;

            return Ok(decrypted);
        }

        Err(IpsecError::SaNotFound)
    }

    /// Enable/disable
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }
}

lazy_static::lazy_static! {
    pub static ref IPSEC: IpsecManager = IpsecManager::new();
}

// ============================================================================
// ERROR TYPE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpsecError {
    SaNotFound,
    PolicyNotFound,
    InvalidPacket,
    InvalidKey,
    AuthFailed,
    ReplayAttack,
    EncryptionFailed,
    DecryptionFailed,
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init() {
    crate::serial_println!("[IPSEC] Subsystem initialized");
}

pub fn register_cipher_algorithm(alg_id: u16, encrypt: IpsecCipherFn, decrypt: IpsecCipherFn) {
    IPSEC_CIPHER_REGISTRY
        .lock()
        .insert(alg_id, IpsecCipherRegistryEntry { encrypt, decrypt });
}

pub fn register_cipher_family(
    family_id: u16,
    mask: u16,
    encrypt: IpsecCipherFn,
    decrypt: IpsecCipherFn,
) {
    IPSEC_CIPHER_FAMILY_REGISTRY
        .lock()
        .push(IpsecCipherFamilyEntry {
            family_id,
            mask,
            encrypt,
            decrypt,
        });
}

pub fn register_auth_algorithm(alg_id: u16, icv_len: usize, calculate: IpsecAuthFn) {
    IPSEC_AUTH_REGISTRY
        .lock()
        .insert(alg_id, IpsecAuthRegistryEntry { icv_len, calculate });
}

pub fn register_auth_family(family_id: u16, mask: u16, icv_len: usize, calculate: IpsecAuthFn) {
    IPSEC_AUTH_FAMILY_REGISTRY
        .lock()
        .push(IpsecAuthFamilyEntry {
            family_id,
            mask,
            icv_len,
            calculate,
        });
}

fn lookup_cipher_registry(alg_id: u16) -> Option<IpsecCipherRegistryEntry> {
    if let Some(entry) = IPSEC_CIPHER_REGISTRY.lock().get(&alg_id).copied() {
        return Some(entry);
    }

    let families = IPSEC_CIPHER_FAMILY_REGISTRY.lock();
    let mut best_match: Option<(u32, IpsecCipherRegistryEntry)> = None;
    for entry in families.iter().copied() {
        if (alg_id & entry.mask) != (entry.family_id & entry.mask) {
            continue;
        }
        let specificity = entry.mask.count_ones();
        let dispatch = IpsecCipherRegistryEntry {
            encrypt: entry.encrypt,
            decrypt: entry.decrypt,
        };
        if best_match
            .as_ref()
            .map(|(best_specificity, _)| specificity > *best_specificity)
            .unwrap_or(true)
        {
            best_match = Some((specificity, dispatch));
        }
    }
    best_match.map(|(_, dispatch)| dispatch)
}

fn lookup_auth_registry(alg_id: u16) -> Option<IpsecAuthRegistryEntry> {
    if let Some(entry) = IPSEC_AUTH_REGISTRY.lock().get(&alg_id).copied() {
        return Some(entry);
    }

    let families = IPSEC_AUTH_FAMILY_REGISTRY.lock();
    let mut best_match: Option<(u32, IpsecAuthRegistryEntry)> = None;
    for entry in families.iter().copied() {
        if (alg_id & entry.mask) != (entry.family_id & entry.mask) {
            continue;
        }
        let specificity = entry.mask.count_ones();
        let dispatch = IpsecAuthRegistryEntry {
            icv_len: entry.icv_len,
            calculate: entry.calculate,
        };
        if best_match
            .as_ref()
            .map(|(best_specificity, _)| specificity > *best_specificity)
            .unwrap_or(true)
        {
            best_match = Some((specificity, dispatch));
        }
    }
    best_match.map(|(_, dispatch)| dispatch)
}

#[cfg(test)]
mod tests {
    use super::*;

    const IPSEC_ENC_XOR_STREAM: u16 = 0x9001;
    const IPSEC_ENC_XOR_STREAM_FAMILY: u16 = 0x9100;
    const IPSEC_AUTH_XOR_MAC: u16 = 0x9002;
    const IPSEC_AUTH_XOR_MAC_FAMILY: u16 = 0x9200;

    fn xor_stream_cipher(sa: &SecurityAssociation, pkt: &[u8]) -> Result<Vec<u8>, IpsecError> {
        if sa.enc_key.is_empty() {
            return Err(IpsecError::InvalidKey);
        }
        let mut out = Vec::with_capacity(pkt.len());
        for (idx, byte) in pkt.iter().copied().enumerate() {
            out.push(byte ^ sa.enc_key[idx % sa.enc_key.len()]);
        }
        Ok(out)
    }

    fn xor_mac(sa: &SecurityAssociation, data: &[u8]) -> Result<Vec<u8>, IpsecError> {
        let auth_key = if sa.auth_key.is_empty() {
            &sa.enc_key
        } else {
            &sa.auth_key
        };
        if auth_key.is_empty() {
            return Err(IpsecError::InvalidKey);
        }
        let mut acc = 0u8;
        for (idx, byte) in data.iter().copied().enumerate() {
            acc ^= byte ^ auth_key[idx % auth_key.len()];
        }
        Ok(vec![acc; 12])
    }

    fn build_sa(enc_alg: u16, key_len: usize) -> SecurityAssociation {
        let mut sa = SecurityAssociation::new(0x1000, IPPROTO_ESP, IPSEC_MODE_TRANSPORT);
        sa.enc_alg = enc_alg;
        sa.enc_key = (0..key_len).map(|idx| idx as u8).collect();
        sa
    }

    #[test]
    fn ipsec_aes_ctr_roundtrip_is_stateful() {
        let sa = build_sa(IPSEC_ENC_AES_CTR, 16);
        let plaintext = b"echos-ipsec-aes-ctr-roundtrip";
        let encrypted = sa.encrypt(plaintext).unwrap();
        assert_ne!(&encrypted[16..], plaintext);
        let decrypted = sa.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn ipsec_chacha20_poly1305_roundtrip_is_stateful() {
        let sa = build_sa(IPSEC_ENC_CHACHA20_POLY1305, 32);
        let plaintext = b"echos-ipsec-chacha20-poly1305-roundtrip";
        let encrypted = sa.encrypt(plaintext).unwrap();
        assert!(encrypted.len() > plaintext.len());
        let decrypted = sa.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn ipsec_des_cbc_roundtrip_is_stateful() {
        let sa = build_sa(IPSEC_ENC_DES_CBC, 8);
        let plaintext = b"echos-ipsec-des-cbc-roundtrip";
        let encrypted = sa.encrypt(plaintext).unwrap();
        assert!(encrypted.len() > plaintext.len());
        let decrypted = sa.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn ipsec_3des_cbc_roundtrip_is_stateful() {
        let sa = build_sa(IPSEC_ENC_3DES_CBC, 24);
        let plaintext = b"echos-ipsec-3des-cbc-roundtrip";
        let encrypted = sa.encrypt(plaintext).unwrap();
        assert!(encrypted.len() > plaintext.len());
        let decrypted = sa.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn ipsec_icv_tracks_auth_algorithm_family() {
        let mut sa = build_sa(IPSEC_ENC_AES_CTR, 16);
        sa.auth_key = (0..64).map(|idx| idx as u8).collect();
        let payload = b"echos-ipsec-icv-family";

        sa.auth_alg = IPSEC_AUTH_HMAC_SHA1;
        let sha1 = sa.calculate_icv(payload);
        sa.auth_alg = IPSEC_AUTH_HMAC_SHA256;
        let sha256 = sa.calculate_icv(payload);
        sa.auth_alg = IPSEC_AUTH_HMAC_SHA384;
        let sha384 = sa.calculate_icv(payload);
        sa.auth_alg = IPSEC_AUTH_HMAC_SHA512;
        let sha512 = sa.calculate_icv(payload);
        sa.auth_key = (0..16).map(|idx| idx as u8).collect();
        sa.auth_alg = IPSEC_AUTH_AES_XCBC;
        let xcbc = sa.calculate_icv(payload);

        assert_eq!(sha1.len(), 12);
        assert_eq!(sha256.len(), 16);
        assert_eq!(sha384.len(), 24);
        assert_eq!(sha512.len(), 32);
        assert_eq!(xcbc.len(), 12);
        assert_ne!(sha1, sha256);
        assert_ne!(sha256, sha384);
        assert_ne!(sha384, sha512);
        assert_ne!(xcbc, sha1);
        assert_ne!(xcbc, sha256[..12]);
        assert!(sa.verify_icv(payload, &xcbc));
        sa.auth_key = (0..64).map(|idx| idx as u8).collect();
        sa.auth_alg = IPSEC_AUTH_HMAC_SHA512;
        assert!(sa.verify_icv(payload, &sha512));
    }

    #[test]
    fn ipsec_aes_xcbc_96_matches_rfc3566_vectors() {
        let key: Vec<u8> = (0x00u8..=0x0f).collect();
        let cases: &[(&[u8], [u8; 12])] = &[
            (
                &[],
                [
                    0x75, 0xf0, 0x25, 0x1d, 0x52, 0x8a, 0xc0, 0x1c, 0x45, 0x73, 0xdf, 0xd5,
                ],
            ),
            (
                &[0x00, 0x01, 0x02],
                [
                    0x5b, 0x37, 0x65, 0x80, 0xae, 0x2f, 0x19, 0xaf, 0xe7, 0x21, 0x9c, 0xee,
                ],
            ),
            (
                &[
                    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c,
                    0x0d, 0x0e, 0x0f,
                ],
                [
                    0xd2, 0xa2, 0x46, 0xfa, 0x34, 0x9b, 0x68, 0xa7, 0x99, 0x98, 0xa4, 0x39,
                ],
            ),
            (
                &[
                    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c,
                    0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13,
                ],
                [
                    0x47, 0xf5, 0x1b, 0x45, 0x64, 0x96, 0x62, 0x15, 0xb8, 0x98, 0x5c, 0x63,
                ],
            ),
            (
                &[
                    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c,
                    0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19,
                    0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
                ],
                [
                    0xf5, 0x4f, 0x0e, 0xc8, 0xd2, 0xb9, 0xf3, 0xd3, 0x68, 0x07, 0x73, 0x4b,
                ],
            ),
        ];

        for (message, expected) in cases {
            let mac = SecurityAssociation::aes_xcbc_mac_96(&key, message).unwrap();
            assert_eq!(&mac, expected);
        }
    }

    #[test]
    fn ipsec_registry_cipher_supports_extended_algorithm_ids() {
        register_cipher_algorithm(IPSEC_ENC_XOR_STREAM, xor_stream_cipher, xor_stream_cipher);
        let sa = build_sa(IPSEC_ENC_XOR_STREAM, 8);
        let plaintext = b"echos-ipsec-registry-cipher";
        let encrypted = sa.encrypt(plaintext).unwrap();
        assert_ne!(encrypted, plaintext);
        let decrypted = sa.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn ipsec_cipher_family_registry_supports_masked_algorithm_ranges() {
        register_cipher_family(
            IPSEC_ENC_XOR_STREAM_FAMILY,
            0xFF00,
            xor_stream_cipher,
            xor_stream_cipher,
        );
        let sa = build_sa(IPSEC_ENC_XOR_STREAM_FAMILY | 0x0037, 8);
        let plaintext = b"echos-ipsec-family-cipher";
        let encrypted = sa.encrypt(plaintext).unwrap();
        assert_ne!(encrypted, plaintext);
        let decrypted = sa.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn ipsec_registry_auth_supports_extended_algorithm_ids() {
        register_auth_algorithm(IPSEC_AUTH_XOR_MAC, 12, xor_mac);
        let mut sa = build_sa(IPSEC_ENC_AES_CTR, 16);
        sa.auth_alg = IPSEC_AUTH_XOR_MAC;
        sa.auth_key = b"echos-auth-key".to_vec();
        let payload = b"echos-ipsec-registry-auth";
        let icv = sa.calculate_icv(payload);
        assert_eq!(icv.len(), 12);
        assert!(sa.verify_icv(payload, &icv));
        let mut tampered = icv.clone();
        tampered[0] ^= 0x55;
        assert!(!sa.verify_icv(payload, &tampered));
    }

    #[test]
    fn ipsec_auth_family_registry_supports_masked_algorithm_ranges() {
        register_auth_family(IPSEC_AUTH_XOR_MAC_FAMILY, 0xFF00, 12, xor_mac);
        let mut sa = build_sa(IPSEC_ENC_AES_CTR, 16);
        sa.auth_alg = IPSEC_AUTH_XOR_MAC_FAMILY | 0x0009;
        sa.auth_key = b"echos-auth-family".to_vec();
        let payload = b"echos-ipsec-family-auth";
        let icv = sa.calculate_icv(payload);
        assert_eq!(icv.len(), 12);
        assert!(sa.verify_icv(payload, &icv));
    }

    #[test]
    fn ipsec_cipher_family_registry_supports_global_wildcard_family() {
        register_cipher_family(0, 0x0000, xor_stream_cipher, xor_stream_cipher);
        let sa = build_sa(0xA731, 8);
        let plaintext = b"echos-ipsec-global-family-cipher";
        let encrypted = sa.encrypt(plaintext).unwrap();
        assert_ne!(encrypted, plaintext);
        let decrypted = sa.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn ipsec_auth_family_registry_supports_global_wildcard_family() {
        register_auth_family(0, 0x0000, 12, xor_mac);
        let mut sa = build_sa(IPSEC_ENC_AES_CTR, 16);
        sa.auth_alg = 0xB409;
        sa.auth_key = b"echos-auth-global-family".to_vec();
        let payload = b"echos-ipsec-global-family-auth";
        let icv = sa.calculate_icv(payload);
        assert_eq!(icv.len(), 12);
        assert!(sa.verify_icv(payload, &icv));
    }

    #[test]
    fn ipsec_unknown_cipher_algorithm_uses_default_family_dispatch() {
        let sa = build_sa(0xD777, 16);
        let plaintext = b"echos-ipsec-default-cipher-dispatch";
        let encrypted = sa.encrypt(plaintext).unwrap();
        assert_ne!(encrypted, plaintext);
        let decrypted = sa.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn ipsec_unknown_auth_algorithm_uses_default_family_dispatch() {
        let mut sa = build_sa(IPSEC_ENC_AES_CTR, 16);
        sa.auth_alg = 0xC812;
        sa.auth_key = (0..48).map(|idx| idx as u8).collect();
        let payload = b"echos-ipsec-default-auth-dispatch";
        let icv = sa.calculate_icv(payload);
        assert_eq!(icv.len(), 24);
        assert!(sa.verify_icv(payload, &icv));
    }
}
