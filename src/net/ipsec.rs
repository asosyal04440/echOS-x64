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
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// IPsec SABİTLERİ
// ============================================================================

/// IPsec protokol numaraları (IPv4 başlığındaki `protocol` alanına yazılır)
pub const IPPROTO_ESP: u8 = 50; // Kapsülleme Güvenlik Yükü
pub const IPPROTO_AH: u8 = 51;  // Kimlik Doğrulama Başlığı

/// IPsec çalışma modları
pub const IPSEC_MODE_TRANSPORT: u8 = 0; // Uçtan uca - yalnızca yük korunur
pub const IPSEC_MODE_TUNNEL: u8 = 1;    // Tünel - tüm IP paketi kapsüllenir

/// IPsec yönleri
pub const IPSEC_DIR_INBOUND: u8 = 0;  // Gelen trafik
pub const IPSEC_DIR_OUTBOUND: u8 = 1; // Giden trafik

/// Şifreleme algoritmaları
/// Güvenlik sırasına göre sıralanmıştır; NULL yalnızca test için kullanılır.
pub const IPSEC_ENC_NULL: u16 = 0;               // Şifreleme yok (test)
pub const IPSEC_ENC_DES_CBC: u16 = 1;            // DES-CBC (zayıf, kullanılmamalı)
pub const IPSEC_ENC_3DES_CBC: u16 = 2;           // 3DES-CBC (zayıf, kullanılmamalı)
pub const IPSEC_ENC_AES_CBC: u16 = 3;            // AES-CBC (yaygın, güvenli)
pub const IPSEC_ENC_AES_CTR: u16 = 4;            // AES-CTR (hızlı, güvenli)
pub const IPSEC_ENC_AES_GCM: u16 = 5;            // AES-GCM (AEAD, en iyi seçim)
pub const IPSEC_ENC_CHACHA20_POLY1305: u16 = 6;  // ChaCha20-Poly1305 (donanımsız sistemlerde hızlı)

/// Kimlik doğrulama algoritmaları (HMAC tabanlı)
/// HMAC: Hash tabanlı Mesaj Kimlik Doğrulama Kodu
pub const IPSEC_AUTH_HMAC_MD5: u16 = 1;    // MD5 (zayıf, kullanılmamalı)
pub const IPSEC_AUTH_HMAC_SHA1: u16 = 2;   // SHA-1 (zayıf, kullanılmamalı)
pub const IPSEC_AUTH_HMAC_SHA256: u16 = 3; // SHA-256 (güvenli, yaygın)
pub const IPSEC_AUTH_HMAC_SHA384: u16 = 4; // SHA-384
pub const IPSEC_AUTH_HMAC_SHA512: u16 = 5; // SHA-512 (en güçlü)
pub const IPSEC_AUTH_AES_XCBC: u16 = 6;   // AES-XCBC-96

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
#[derive(Clone, Debug)]
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
#[derive(Clone, Debug, Default)]
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
            IPSEC_ENC_AES_CBC => self.encrypt_aes_cbc(pkt),
            IPSEC_ENC_AES_GCM => self.encrypt_aes_gcm(pkt),
            _ => Err(IpsecError::UnsupportedAlgorithm),
        }
    }

    /// Decrypt packet
    pub fn decrypt(&self, pkt: &[u8]) -> Result<Vec<u8>, IpsecError> {
        match self.enc_alg {
            IPSEC_ENC_NULL => Ok(pkt.to_vec()),
            IPSEC_ENC_AES_CBC => self.decrypt_aes_cbc(pkt),
            IPSEC_ENC_AES_GCM => self.decrypt_aes_gcm(pkt),
            _ => Err(IpsecError::UnsupportedAlgorithm),
        }
    }

    /// AES-CBC şifreleme (RFC 3602)
    ///
    /// PKCS#7 padding + rastgele IV (16 byte) ile CBC modu.
    fn encrypt_aes_cbc(&self, pkt: &[u8]) -> Result<Vec<u8>, IpsecError> {
        let key = &self.enc_key;
        if key.len() < 16 { return Err(IpsecError::InvalidKey); }

        // Rastgele IV (16 byte)
        let mut iv = [0u8; 16];
        for i in 0..16 { iv[i] = crate::random::next_u32() as u8; }

        // PKCS#7 padding
        let pad_len = 16 - (pkt.len() % 16);
        let mut padded = pkt.to_vec();
        for _ in 0..pad_len { padded.push(pad_len as u8); }

        // CBC şifreleme: C[i] = AES(key, P[i] XOR C[i-1])
        let mut result = Vec::with_capacity(16 + padded.len());
        result.extend_from_slice(&iv);
        let mut prev = iv;
        for chunk in padded.chunks(16) {
            let mut block = [0u8; 16];
            for j in 0..16 { block[j] = chunk[j] ^ prev[j]; }
            let encrypted = crate::crypto::hw_aes::aes_ecb_encrypt_block(&block, key);
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
            let decrypted = crate::crypto::hw_aes::aes_ecb_decrypt_block(chunk, key);
            let mut plaintext = [0u8; 16];
            for j in 0..16 { plaintext[j] = decrypted[j] ^ prev[j]; }
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

    /// AES-GCM şifreleme (RFC 4106)
    ///
    /// 8 byte IV + AES-CTR şifreleme + GHASH authentication tag.
    fn encrypt_aes_gcm(&self, pkt: &[u8]) -> Result<Vec<u8>, IpsecError> {
        let key = &self.enc_key;
        if key.len() < 16 { return Err(IpsecError::InvalidKey); }

        // 8 byte explicit nonce (4 byte salt oturumda saklı; 8 byte explicit)
        let mut nonce = [0u8; 12];
        // Salt: key'in son 4 byte'ı
        if self.enc_key.len() >= 20 {
            nonce[..4].copy_from_slice(&self.enc_key[16..20]);
        }
        for i in 4..12 { nonce[i] = crate::random::next_u32() as u8; }

        // AES-GCM = AES-CTR + GHASH
        let encrypted = crate::crypto::hw_aes::aes_gcm_encrypt(key, &nonce, pkt, &[]);

        let mut result = Vec::with_capacity(8 + encrypted.len());
        result.extend_from_slice(&nonce[4..12]); // 8 byte explicit nonce
        result.extend_from_slice(&encrypted);    // ciphertext + 16 byte tag
        Ok(result)
    }

    /// AES-GCM şifre çözme
    fn decrypt_aes_gcm(&self, pkt: &[u8]) -> Result<Vec<u8>, IpsecError> {
        let key = &self.enc_key;
        if key.len() < 16 || pkt.len() < 24 { return Err(IpsecError::InvalidPacket); }

        // Nonce yeniden oluştur: 4 byte salt + 8 byte explicit
        let mut nonce = [0u8; 12];
        if self.enc_key.len() >= 20 {
            nonce[..4].copy_from_slice(&self.enc_key[16..20]);
        }
        nonce[4..12].copy_from_slice(&pkt[..8]);

        let ciphertext_with_tag = &pkt[8..];
        crate::crypto::hw_aes::aes_gcm_decrypt(key, &nonce, ciphertext_with_tag, &[])
            .map_err(|_| IpsecError::DecryptionFailed)
    }

    /// ICV (Integrity Check Value) hesapla — HMAC tabanlı
    ///
    /// Kullanılan algoritmaya göre HMAC-SHA256 veya SHA-1 ile
    /// bütünlük kontrol değeri hesaplar.
    pub fn calculate_icv(&self, data: &[u8]) -> Vec<u8> {
        let icv_len = match self.auth_alg {
            IPSEC_AUTH_HMAC_SHA1 => 12,
            IPSEC_AUTH_HMAC_SHA256 => 16,
            IPSEC_AUTH_HMAC_SHA384 => 24,
            IPSEC_AUTH_HMAC_SHA512 => 32,
            _ => 12,
        };

        // HMAC-SHA256 ile ICV hesapla
        let auth_key = if self.auth_key.is_empty() { &self.enc_key } else { &self.auth_key };
        let full_hmac = crate::net::quic::hmac_sha256(auth_key, data);

        // Truncate to icv_len
        full_hmac[..icv_len.min(full_hmac.len())].to_vec()
    }

    /// ICV doğrula
    pub fn verify_icv(&self, data: &[u8], icv: &[u8]) -> bool {
        let expected = self.calculate_icv(data);
        // Sabit zamanlı karşılaştırma (timing attack koruması)
        if expected.len() != icv.len() { return false; }
        let mut diff = 0u8;
        for (a, b) in expected.iter().zip(icv.iter()) {
            diff |= a ^ b;
        }
        diff == 0
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
    pub fn matches(&self, src_ip: u32, dst_ip: u32, proto: u8, src_port: u16, dst_port: u16) -> bool {
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
    pub const fn new() -> Self {
        Self {
            sas: Mutex::new(BTreeMap::new()),
            sps_inbound: Mutex::new(Vec::new()),
            sps_outbound: Mutex::new(Vec::new()),
            next_spi: AtomicU32::new(0x1000000),
            next_policy_id: AtomicU32::new(1),
            enabled: AtomicBool::new(false),
            stats: Mutex::new(IpsecStats::default()),
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
    pub fn find_outbound_policy(&self, src_ip: u32, dst_ip: u32, proto: u8, src_port: u16, dst_port: u16) -> Option<SecurityPolicy> {
        let policies = self.sps_outbound.lock();
        for policy in policies.iter() {
            if policy.matches(src_ip, dst_ip, proto, src_port, dst_port) {
                return Some(policy.clone());
            }
        }
        None
    }

    /// Find policy for inbound packet
    pub fn find_inbound_policy(&self, src_ip: u32, dst_ip: u32, proto: u8, src_port: u16, dst_port: u16) -> Option<SecurityPolicy> {
        let policies = self.sps_inbound.lock();
        for policy in policies.iter() {
            if policy.matches(src_ip, dst_ip, proto, src_port, dst_port) {
                return Some(policy.clone());
            }
        }
        None
    }

    /// Process outbound packet
    pub fn process_outbound(&self, pkt: &mut [u8], src_ip: u32, dst_ip: u32, proto: u8, src_port: u16, dst_port: u16) -> Result<Vec<u8>, IpsecError> {
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
                        esp_pkt.extend_from_slice(&sa.last_seq.load(Ordering::Relaxed).to_be_bytes());
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
    UnsupportedAlgorithm,
    EncryptionFailed,
    DecryptionFailed,
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init() {
    crate::serial_println!("[IPSEC] Subsystem initialized");
}
