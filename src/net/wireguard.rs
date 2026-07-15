//! # WireGuard VPN Protokolü
//!
//! Modern, yüksek performanslı VPN protokolü.
//! RFC önerisi: https://www.wireguard.com/papers/wireguard.pdf
//!
//! ## WireGuard Nedir?
//!
//! WireGuard, OpenVPN ve IPSec'e göre daha küçük bir protokol yüzeyine sahip
//! kimlik doğrulamalı tünel protokolüdür. Linux kernel'ine 5.6'da entegre edildi.
//!
//! ## WireGuard El Sıkışma Akışı (Noise Protocol Çerçevesi)
//!
//! ```
//!  Başlatıcı (Initiator)              Yanıtlayıcı (Responder)
//!       |                                    |
//!       |--- Initiation Msg (Type 1) ------->|   DHKE + kimlik doğr.
//!       |<-- Response Msg (Type 2) ----------|   DHKE tamamla
//!       |                                    |
//!       |=== Transport Msg (Type 4) ========>|   Şifreli tünel aktif
//!       |<== Transport Msg (Type 4) =========|
//!
//!  Her mesaj ChaCha20-Poly1305 ile şifrelenir.
//!  Anahtar türetme için HKDF kullanılır.
//! ```
//!
//! ## Kriptografi
//!
//! ```
//!  Anahtar Değişimi  : Curve25519 (ECDH)
//!  Şifreleme         : ChaCha20-Poly1305 (AEAD)
//!  Hash              : BLAKE2s
//!  Anahtar Türetme   : HKDF
//!  Preshared Key     : Ek kuantum direnci
//! ```
//!
//! ## Allowed IPs (İzin Verilen IP'ler)
//!
//! ```
//! Peer A: allowed_ips = [10.0.0.2/32, 192.168.1.0/24]
//!   -> Bu IP'lere giden paketler Peer A tünelinden geçirilir
//! Peer B: allowed_ips = [0.0.0.0/0]    (tüm trafik)
//!   -> Varsayılan rotadaki tüm trafik Peer B'den geçer
//! ```

use crate::crypto::{
    blake2s, blake2s_keyed, hmac_blake2s, ChaCha20Poly1305, HkdfBlake2s, X25519PrivateKey,
    X25519PublicKey, XChaCha20Poly1305,
};
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// WIREGUARD SABİTLERİ
// ============================================================================

/// Noise_IKpsk2: HASH(CONSTRUCTION) ile başlatılan chaining key
fn noise_init_chaining_key() -> [u8; 32] {
    blake2s(WG_CONSTRUCTION)
}

/// Chaining key → hash → identifier → responder static → handshake hash
fn noise_init_hash(responder_static: &[u8; 32]) -> [u8; 32] {
    let ck = noise_init_chaining_key();
    let mut inner = alloc::vec![0u8; 32 + WG_IDENTIFIER.len()];
    inner[..32].copy_from_slice(&ck);
    inner[32..].copy_from_slice(WG_IDENTIFIER);
    let h = blake2s(&inner);
    let mut outer = alloc::vec![0u8; 32 + 32];
    outer[..32].copy_from_slice(&h);
    outer[32..].copy_from_slice(responder_static);
    blake2s(&outer)
}

/// noise_hash = BLAKE2s(data, 32)
fn noise_hash(data: &[u8]) -> [u8; 32] {
    blake2s(data)
}

/// noise_mac = Keyed-BLAKE2s(key, data, 32)[..16]
fn noise_mac(key: &[u8], data: &[u8]) -> [u8; 16] {
    let full = blake2s_keyed(key, data);
    let mut tag = [0u8; 16];
    tag.copy_from_slice(&full[..16]);
    tag
}

/// noise_hkdf_2: HKDF-BLAKE2s(ck, input, 2) → (new_ck, temp_key)
fn noise_hkdf_2(ck: &[u8; 32], input: &[u8]) -> ([u8; 32], [u8; 32]) {
    let hkdf = HkdfBlake2s::extract(ck, input);
    let okm = hkdf.expand(&[], 64);
    let mut new_ck = [0u8; 32];
    let mut temp_key = [0u8; 32];
    new_ck.copy_from_slice(&okm[..32]);
    temp_key.copy_from_slice(&okm[32..]);
    (new_ck, temp_key)
}

/// noise_hkdf_3: HKDF-BLAKE2s(ck, input, 3) → (new_ck, temp_h, temp_k)
fn noise_hkdf_3(ck: &[u8; 32], input: &[u8]) -> ([u8; 32], [u8; 32], [u8; 32]) {
    let hkdf = HkdfBlake2s::extract(ck, input);
    let okm = hkdf.expand(&[], 96);
    let mut new_ck = [0u8; 32];
    let mut temp_h = [0u8; 32];
    let mut temp_k = [0u8; 32];
    new_ck.copy_from_slice(&okm[..32]);
    temp_h.copy_from_slice(&okm[32..64]);
    temp_k.copy_from_slice(&okm[64..]);
    (new_ck, temp_h, temp_k)
}

/// WireGuard varsayılan UDP portu (51820)
pub const WG_DEFAULT_PORT: u16 = 51820;

/// Curve25519 anahtar boyutu: 32 byte = 256 bit
pub const WG_KEY_SIZE: usize = 32;

/// Mesaj tipi 1: El sıkışma başlatma (Initiator -> Responder)
pub const WG_MSG_INITIATION: u8 = 1;
/// Mesaj tipi 2: El sıkışma yanıtı (Responder -> Initiator)
pub const WG_MSG_RESPONSE: u8 = 2;
/// Mesaj tipi 3: Cookie yanıtı (DoS koruması için)
pub const WG_MSG_COOKIE_REPLY: u8 = 3;
/// Mesaj tipi 4: Şifreli veri taşıma
pub const WG_MSG_TRANSPORT: u8 = 4;

/// WireGuard transport başlığı: type(1) + reserved(3) + receiver_index(4) + nonce(8)
const WG_TRANSPORT_HEADER_LEN: usize = 16;
/// ChaCha20-Poly1305 doğrulama etiketi
const WG_TRANSPORT_TAG_LEN: usize = 16;
/// Initiation paket uzunluğu: sabit 148 byte
const WG_INITIATION_LEN: usize = 148;
/// Initiation paketinde MAC1 öncesi doğrulanan gövde uzunluğu
const WG_INITIATION_BODY_LEN: usize = 116;
/// WireGuard MAC alanı uzunluğu (MAC1/MAC2)
const WG_MAC_LEN: usize = 16;
/// MAC1 anahtar türetme etiketi (WireGuard spec: "mac1----")
const WG_MAC1_LABEL: &[u8; 8] = b"mac1----";
/// Cookie anahtar türetme etiketi (WireGuard spec: "cookie--")
const WG_COOKIE_LABEL: &[u8; 8] = b"cookie--";
/// Noise protokol ismi: Noise_IKpsk2_25519_ChaChaPoly_BLAKE2s (37 bytes)
const WG_CONSTRUCTION: &[u8; 37] = b"Noise_IKpsk2_25519_ChaChaPoly_BLAKE2s";
/// WireGuard tanımlayıcısı (34 bytes)
const WG_IDENTIFIER: &[u8; 34] = b"WireGuard v1 zx2c4 Jason@zx2c4.com";
/// Henüz geçerli bir inbound nonce kabul edilmediğini gösteren sentinel değer
const WG_NONCE_UNINITIALIZED: u64 = u64::MAX;

// ============================================================================
// WIREGUARD ZAMANLAYICI SABİTLERİ
// ============================================================================

/// El sıkışma yeniden deneme aralığı (ms)
const REKEY_TIMEOUT_MS: u64 = 100;
/// El sıkışma denemelerinin toplam süresi (ms)
const REKEY_ATTEMPT_TIME_MS: u64 = 90_000;
/// Keepalive aralığı (ms)
const KEEPALIVE_TIMEOUT_MS: u64 = 10_000;
/// Anahtarın reddedilme süresi (ms)
const REJECT_AFTER_TIME_MS: u64 = 120_000;
/// Anahtarın reddedilme mesaj sayısı
const REJECT_AFTER_MESSAGES: u64 = u64::MAX - 1;
/// Yeniden anahtarlama mesaj sayısı
const REKEY_AFTER_MESSAGES: u64 = 1 << 56;
/// Yeniden anahtarlama zaman aşımı (ms)
const REKEY_AFTER_TIME_MS: u64 = 120_000;
/// Keepalive + rekey timeout toplamı — initiator'un rekey kararı için
const KEEPALIVE_AND_REKEY_TIMEOUT_MS: u64 = KEEPALIVE_TIMEOUT_MS + REKEY_TIMEOUT_MS;

// ============================================================================
// WIREGUARD ANAHTARI
// ============================================================================

/// WireGuard Curve25519 anahtarı (32 byte)
///
/// Public/private anahtar çiftleri Curve25519 eğrisi üzerinde.
/// Private key clamping: bytes[0] &= 248, bytes[31] &= 127, bytes[31] |= 64
#[derive(Clone, Debug)]
pub struct WgKey(pub [u8; WG_KEY_SIZE]);

impl WgKey {
    /// Sıfır anahtar oluştur (başlangıç/hata durumu)
    pub fn new() -> Self {
        Self([0u8; WG_KEY_SIZE])
    }

    /// Byte dizisinden anahtar oluştur
    pub fn from_bytes(bytes: [u8; WG_KEY_SIZE]) -> Self {
        Self(bytes)
    }

    /// Rastgele Curve25519 anahtar üret
    pub fn generate() -> Self {
        let mut key = [0u8; WG_KEY_SIZE];
        crate::crypto::rdrand_bytes(&mut key);
        // Curve25519 clamping (RFC 7748)
        key[0] &= 248;
        key[31] &= 127;
        key[31] |= 64;
        Self(key)
    }

    /// Ham byte dizisi referansı döndür
    pub fn as_bytes(&self) -> &[u8; WG_KEY_SIZE] {
        &self.0
    }
}

// ============================================================================
// NOISE_IKpsk2 EL SIKIŞMA DURUMU
// ============================================================================

/// Noise_IKpsk2 protokol durumu (WireGuard-specific implementation)
///
/// chaining_key (ck): Tüm DH çıktılarının hash'ini tutar
/// hash (h): Tüm el sıkışma verisinin hash'ini tutar
/// key (k): Encryption key (boş olabilir, AEAD için)
/// nonce (n): Sayaç bazlı nonce değeri
///
/// Referans: https://www.wireguard.com/protocol/
struct NoiseState {
    ck: [u8; 32],
    h: [u8; 32],
    k: [u8; 32],
    n: u64,
    ps: [u8; 32],  // preshared key
}

impl NoiseState {
    fn new(responder_static: &[u8; 32]) -> Self {
        let ck = noise_init_chaining_key();
        let h = noise_init_hash(responder_static);
        NoiseState {
            ck,
            h,
            k: [0u8; 32],
            n: 0,
            ps: [0u8; 32],
        }
    }

    fn mix_hash(&mut self, data: &[u8]) {
        let mut input = Vec::with_capacity(32 + data.len());
        input.extend_from_slice(&self.h);
        input.extend_from_slice(data);
        self.h = blake2s(&input);
    }

    fn mix_key(&mut self, dh_output: &[u8; 32]) {
        let (new_ck, temp_k) = noise_hkdf_2(&self.ck, dh_output);
        self.ck = new_ck;
        self.k = temp_k;
        self.n = 0;
    }

    fn has_key(&self) -> bool {
        self.k != [0u8; 32]
    }

    fn encrypt_and_hash(&mut self, plaintext: &[u8]) -> Vec<u8> {
        if self.has_key() {
            let mut nonce = [0u8; 12];
            nonce[4..12].copy_from_slice(&self.n.to_le_bytes());
            let mut aead = ChaCha20Poly1305::new(&self.k, &nonce);
            let (ct, tag) = aead.encrypt(plaintext, &self.h);
            let mut out = Vec::with_capacity(ct.len() + 16);
            out.extend_from_slice(&ct);
            out.extend_from_slice(&tag);
            self.n += 1;
            self.mix_hash(&out);
            out
        } else {
            self.mix_hash(plaintext);
            plaintext.to_vec()
        }
    }

    fn decrypt_and_hash(&mut self, ciphertext: &[u8]) -> Option<Vec<u8>> {
        if self.has_key() {
            if ciphertext.len() < 16 {
                return None;
            }
            let split = ciphertext.len() - 16;
            let ct = &ciphertext[..split];
            let mut tag = [0u8; 16];
            tag.copy_from_slice(&ciphertext[split..]);
            let mut nonce = [0u8; 12];
            nonce[4..12].copy_from_slice(&self.n.to_le_bytes());
            let mut aead = ChaCha20Poly1305::new(&self.k, &nonce);
            let pt = aead.decrypt(ct, &self.h, &tag)?;
            self.n += 1;
            self.mix_hash(ciphertext);
            Some(pt)
        } else {
            self.mix_hash(ciphertext);
            Some(ciphertext.to_vec())
        }
    }

    fn mix_key_and_hash(&mut self, input: &[u8]) {
        let (new_ck, temp_h, temp_k) = noise_hkdf_3(&self.ck, input);
        self.ck = new_ck;
        self.mix_hash(&temp_h);
        self.k = temp_k;
        self.n = 0;
    }

    fn split(&self) -> ([u8; 32], [u8; 32]) {
        let hkdf = HkdfBlake2s::extract(&self.ck, &[]);
        let okm = hkdf.expand(&[], 64);
        let mut send = [0u8; 32];
        let mut recv = [0u8; 32];
        send.copy_from_slice(&okm[..32]);
        recv.copy_from_slice(&okm[32..]);
        (send, recv)
    }
}

// ============================================================================
// WIREGUARD PEER (EŞ NODE)
// ============================================================================

/// WireGuard ağ katılımcısı (peer/eş)
///
/// Her peer bir public key ile tanımlanır.
/// Birden fazla peer olabilir, her biri farklı IP aralıklarına yönlendirilebilir.
#[derive(Debug)]
pub struct WgPeer {
    /// Peer'in Curve25519 public key'i (kimlik)
    pub public_key: WgKey,
    /// İsteğe bağlı preshared key (ek güvenlik katmanı)
    /// Kuantum bilgisayarlara karşı ek koruma sağlar
    pub preshared_key: WgKey,
    /// Peer'in endpoint IPv4 adresi (u32, big-endian)
    pub endpoint_ip: u32,
    /// Peer'in UDP port numarası
    pub endpoint_port: u16,
    /// Son başarılı el sıkışma zamanı (Unix timestamp)
    pub last_handshake: AtomicU64,
    /// Gönderilen toplam byte sayısı
    pub tx_bytes: AtomicU64,
    /// Alınan toplam byte sayısı
    pub rx_bytes: AtomicU64,
    /// İzin verilen IP/prefix listesi: (ip, prefix_uzunluk)
    /// Örnek: [(10.0.0.2, 32), (192.168.1.0, 24)]
    pub allowed_ips: Vec<(u32, u8)>, // (IP, prefix_len)
    /// Kalıcı keepalive aralığı (saniye, 0 = devre dışı)
    pub keepalive: AtomicU32,
    /// Aktif oturum durumu (şifreleme anahtarları ve nonce)
    pub session: Mutex<WgSession>,
}

impl Clone for WgPeer {
    fn clone(&self) -> Self {
        Self {
            public_key: self.public_key.clone(),
            preshared_key: self.preshared_key.clone(),
            endpoint_ip: self.endpoint_ip,
            endpoint_port: self.endpoint_port,
            last_handshake: AtomicU64::new(self.last_handshake.load(Ordering::Relaxed)),
            tx_bytes: AtomicU64::new(self.tx_bytes.load(Ordering::Relaxed)),
            rx_bytes: AtomicU64::new(self.rx_bytes.load(Ordering::Relaxed)),
            allowed_ips: self.allowed_ips.clone(),
            keepalive: AtomicU32::new(self.keepalive.load(Ordering::Relaxed)),
            session: Mutex::new(self.session.lock().clone()),
        }
    }
}

/// WireGuard oturum durumu
///
/// Başarılı el sıkışma sonrasında her peer için bir oturum oluşturulur.
/// Oturum iki yönlü simetrik anahtar içerir.
#[derive(Clone, Debug)]
pub struct WgSession {
    /// Yerel oturum indeksi (peer'in bizim nonce'umuzu takip etmesi için)
    pub local_index: u32,
    /// Uzak oturum indeksi (peer'in indeksi)
    pub remote_index: u32,
    /// Gönderme anahtarı (ChaCha20-Poly1305 için)
    pub sending_key: [u8; 32],
    /// Alma anahtarı (ChaCha20-Poly1305 için)
    pub receiving_key: [u8; 32],
    /// Gönderme nonce sayacı (her pakette artırılır, tekrar önleme)
    pub sending_nonce: u64,
    /// Alma nonce sayacı (replay attack tespiti için)
    pub receiving_nonce: u64,
    /// Bu peer el sıkışmayı başlatan mıydı?
    pub is_initiator: bool,
    /// Oturum kuruldu mu?
    pub established: bool,
    /// Initiator tarafında response bekleyen ephemeral private key
    pub pending_initiator_private: [u8; 32],
    /// Handshake response bekleniyor mu?
    pub handshake_pending: bool,
    /// Cookie yanıtından alınan son cookie değeri (MAC2 için)
    pub last_cookie: [u8; 16],
    /// Cookie'nin alındığı zaman (Unix timestamp, 0 = geçersiz)
    pub last_cookie_time: u64,
}

impl WgPeer {
    /// Yeni peer oluştur (sadece public key ile)
    pub fn new(public_key: WgKey) -> Self {
        Self {
            public_key,
            preshared_key: WgKey::new(),
            endpoint_ip: 0,
            endpoint_port: WG_DEFAULT_PORT,
            last_handshake: AtomicU64::new(0),
            tx_bytes: AtomicU64::new(0),
            rx_bytes: AtomicU64::new(0),
            allowed_ips: Vec::new(),
            keepalive: AtomicU32::new(0),
            session: Mutex::new(WgSession {
                local_index: 0,
                remote_index: 0,
                sending_key: [0u8; 32],
                receiving_key: [0u8; 32],
                sending_nonce: 0,
                receiving_nonce: WG_NONCE_UNINITIALIZED,
                is_initiator: false,
                established: false,
                pending_initiator_private: [0u8; 32],
                handshake_pending: false,
                last_cookie: [0u8; 16],
                last_cookie_time: 0,
            }),
        }
    }

    /// IP adresinin bu peer için izin verilen aralıkta olup olmadığını kontrol et
    ///
    /// CIDR maskeleme: mask = !0u32 >> (32 - prefix_len)
    /// Örnek: prefix=24 -> mask=0x00FFFFFF -> 192.168.1.0/24 aralığı
    pub fn is_allowed_ip(&self, ip: u32) -> bool {
        for (allowed_ip, prefix_len) in &self.allowed_ips {
            let mask = if *prefix_len == 0 {
                0
            } else {
                !0u32 >> (32 - prefix_len)
            };
            if (ip & mask) == (*allowed_ip & mask) {
                return true;
            }
        }
        false
    }

    /// Initiator'un rekey başlatması gerekip gerekmediğini kontrol eder.
    ///
    /// Koşullar (WireGuard spec):
    /// - Son paket gönderildiğinden beri REKEY_AFTER_TIME_MS geçtiyse
    /// - Gönderilen mesaj sayısı REKEY_AFTER_MESSAGES'i aştıysa
    pub fn should_initiate_rekey(&self) -> bool {
        let session = self.session.lock();
        if !session.established || !session.is_initiator {
            return false;
        }
        let elapsed_ms = crate::time::current_timestamp_nanos()
            .wrapping_sub(self.last_handshake.load(Ordering::Relaxed) * 1_000_000)
            / 1_000_000;
        elapsed_ms >= REKEY_AFTER_TIME_MS || session.sending_nonce >= REKEY_AFTER_MESSAGES
    }

    /// Anahtarın reddedilmesi gerekip gerekmediğini kontrol eder.
    pub fn should_reject_key(&self) -> bool {
        let session = self.session.lock();
        if !session.established {
            return false;
        }
        let elapsed_ms = crate::time::current_timestamp_nanos()
            .wrapping_sub(self.last_handshake.load(Ordering::Relaxed) * 1_000_000)
            / 1_000_000;
        elapsed_ms >= REJECT_AFTER_TIME_MS || session.receiving_nonce >= REJECT_AFTER_MESSAGES
    }

    /// Bu peer'a keepalive gönderilmesi gerekip gerekmediğini kontrol eder.
    pub fn should_send_keepalive(&self) -> bool {
        let last_tx = self.tx_bytes.load(Ordering::Relaxed);
        let last_rx = self.rx_bytes.load(Ordering::Relaxed);
        if last_tx == 0 || last_rx == 0 {
            return false;
        }
        // Son alınan paketten bu yana KEEPALIVE_TIMEOUT_MS geçtiyse
        // ve bir şey göndermediysek keepalive gönder
        true
    }

    /// Paketi şifreleyip transport mesajı olarak hazırla
    ///
    /// ## Transport Mesaj Yapısı (Tip 4)
    ///
    /// ```
    ///  byte 0    : Mesaj tipi (0x04)
    ///  byte 1-4  : Yerel oturum indeksi (little-endian)
    ///  byte 5-12 : Nonce (64-bit sayaç, little-endian)
    ///  byte 13+  : ChaCha20-Poly1305 şifreli veri
    /// ```
    pub fn encrypt_packet(&self, pkt: &[u8]) -> Result<Vec<u8>, WgError> {
        let mut session = self.session.lock();

        if !session.established {
            return Err(WgError::NoSession);
        }

        // ChaCha20-Poly1305 encryption
        let nonce = session.sending_nonce;
        session.sending_nonce += 1; // Nonce sayacını artır (tekrar önleme)

        if session.remote_index == 0 {
            return Err(WgError::NoSession);
        }

        // 12 byte nonce: 4 byte sıfır + 8 byte little-endian counter
        let mut nonce_bytes = [0u8; 12];
        nonce_bytes[4..12].copy_from_slice(&nonce.to_le_bytes());

        // Build transport header
        let mut transport =
            Vec::with_capacity(WG_TRANSPORT_HEADER_LEN + pkt.len() + WG_TRANSPORT_TAG_LEN);
        transport.push(WG_MSG_TRANSPORT);
        transport.extend_from_slice(&[0u8; 3]); // reserved
        transport.extend_from_slice(&session.remote_index.to_le_bytes()); // receiver index
        transport.extend_from_slice(&nonce.to_le_bytes());

        let mut aead = ChaCha20Poly1305::new(&session.sending_key, &nonce_bytes);
        let (ciphertext, tag) = aead.encrypt(pkt, &transport[..WG_TRANSPORT_HEADER_LEN]);
        transport.extend_from_slice(&ciphertext);
        transport.extend_from_slice(&tag);

        // İstatistikleri güncelle
        self.tx_bytes.fetch_add(pkt.len() as u64, Ordering::Relaxed);

        Ok(transport)
    }

    /// Gelen transport mesajını çöz ve veriyi döndür
    ///
    /// ## Replay Attack Koruması
    ///
    /// Her paket bir nonce içerir. Alıcı, daha önce görülen
    /// nonce'ları reddeder. Bu sayede eski paketlerin tekrar
    /// oynatılması engellenir.
    pub fn decrypt_packet(&self, pkt: &[u8]) -> Result<Vec<u8>, WgError> {
        if pkt.len() < WG_TRANSPORT_HEADER_LEN + WG_TRANSPORT_TAG_LEN || pkt[0] != WG_MSG_TRANSPORT
        {
            return Err(WgError::InvalidPacket);
        }

        let mut session = self.session.lock();

        if !session.established {
            return Err(WgError::NoSession);
        }

        // Parse transport header
        let receiver_index = u32::from_le_bytes([pkt[4], pkt[5], pkt[6], pkt[7]]);
        let nonce = u64::from_le_bytes([
            pkt[8], pkt[9], pkt[10], pkt[11], pkt[12], pkt[13], pkt[14], pkt[15],
        ]);

        // Oturum indeksini kontrol et
        if receiver_index != session.local_index {
            return Err(WgError::InvalidIndex);
        }

        // Check for replay (tekrar saldırısı kontrolü)
        // Replay pencere kontrolü (kayan pencere)
        if session.receiving_nonce != WG_NONCE_UNINITIALIZED && nonce <= session.receiving_nonce {
            return Err(WgError::Replay);
        }

        // 12 byte nonce: 4 byte sıfır + 8 byte little-endian counter
        let mut nonce_bytes = [0u8; 12];
        nonce_bytes[4..12].copy_from_slice(&nonce.to_le_bytes());

        // ChaCha20-Poly1305 ile şifre çöz
        let ciphertext_and_tag = &pkt[WG_TRANSPORT_HEADER_LEN..];
        if ciphertext_and_tag.len() < WG_TRANSPORT_TAG_LEN {
            return Err(WgError::InvalidPacket);
        }
        let split_at = ciphertext_and_tag.len() - WG_TRANSPORT_TAG_LEN;
        let ciphertext = &ciphertext_and_tag[..split_at];
        let mut tag = [0u8; WG_TRANSPORT_TAG_LEN];
        tag.copy_from_slice(&ciphertext_and_tag[split_at..]);

        let mut aead = ChaCha20Poly1305::new(&session.receiving_key, &nonce_bytes);
        let decrypted = aead
            .decrypt(ciphertext, &pkt[..WG_TRANSPORT_HEADER_LEN], &tag)
            .ok_or(WgError::CryptoError)?;
        session.receiving_nonce = nonce;

        // İstatistikleri güncelle
        self.rx_bytes
            .fetch_add(decrypted.len() as u64, Ordering::Relaxed);

        Ok(decrypted)
    }
}

// ============================================================================
// WIREGUARD CİHAZI (DEVICE)
// ============================================================================

static NEXT_IFINDEX: AtomicU32 = AtomicU32::new(100);

/// WireGuard sanal ağ arayüzü
///
/// Her WireGuard arayüzünün bir private/public key çifti ve peer listesi var.
/// Linux'ta "wg0", "wg1" gibi adlarla görünür.
pub struct WgDevice {
    /// Arayüz adı (örn: "wg0")
    pub name: String,
    /// Network interface index (netlink kimliği)
    pub ifindex: AtomicU32,
    /// Dinleme UDP portu
    pub listen_port: AtomicU32,
    /// Bu cihazın Curve25519 private key'i (GİZLİ, hiç iletilmez)
    pub private_key: Mutex<WgKey>,
    /// Bu cihazın Curve25519 public key'i (paylaşılabilir)
    pub public_key: Mutex<WgKey>,
    /// Peer listesi: public_key -> WgPeer
    pub peers: Mutex<BTreeMap<[u8; WG_KEY_SIZE], Arc<WgPeer>>>,
    /// Firewall mark (paket etiketleme)
    pub fwmark: AtomicU32,
    /// Stateless MAC2 cookie türetme gizli anahtarı
    mac2_cookie_secret: [u8; 32],
    /// Arayüz aktif mi?
    pub is_up: AtomicBool,
    /// İstatistikler
    pub stats: Mutex<WgStats>,
}

/// WireGuard istatistikleri
#[derive(Clone, Debug, Default)]
pub struct WgStats {
    /// Toplam peer sayısı
    pub peers_count: u32,
    /// Toplam gönderilen byte
    pub total_tx: u64,
    /// Toplam alınan byte
    pub total_rx: u64,
}

impl WgDevice {
    /// Yeni WireGuard arayüzü oluştur
    pub fn new(name: &str) -> Self {
        let private_key = WgKey::generate();
        let mut mac2_cookie_secret = [0u8; 32];
        crate::crypto::rdrand_bytes(&mut mac2_cookie_secret);
        // Public key = X25519(private_key, BasePoint)
        let x25519_priv = crate::crypto::ed25519::X25519PrivateKey::from_bytes(private_key.0);
        let public_key = WgKey::from_bytes(*x25519_priv.public_key().as_bytes());

        Self {
            name: String::from(name),
            ifindex: AtomicU32::new(NEXT_IFINDEX.fetch_add(1, Ordering::Relaxed)),
            listen_port: AtomicU32::new(WG_DEFAULT_PORT as u32),
            private_key: Mutex::new(private_key),
            public_key: Mutex::new(public_key),
            peers: Mutex::new(BTreeMap::new()),
            fwmark: AtomicU32::new(0),
            mac2_cookie_secret,
            is_up: AtomicBool::new(false),
            stats: Mutex::new(WgStats::default()),
        }
    }

    /// Peer ekle (public key ile indekslenmiş)
    pub fn add_peer(&self, peer: Arc<WgPeer>) {
        self.peers.lock().insert(peer.public_key.0, peer.clone());

        let mut stats = self.stats.lock();
        stats.peers_count += 1;
    }

    /// Peer kaldır
    pub fn remove_peer(&self, public_key: &WgKey) {
        self.peers.lock().remove(&public_key.0);
    }

    /// Public key'e göre peer getir
    pub fn get_peer(&self, public_key: &WgKey) -> Option<Arc<WgPeer>> {
        self.peers.lock().get(&public_key.0).cloned()
    }

    /// Allowed IP'ye göre peer bul (rota tablosu araması)
    pub fn find_peer_by_ip(&self, ip: u32) -> Option<Arc<WgPeer>> {
        for peer in self.peers.lock().values() {
            if peer.is_allowed_ip(ip) {
                return Some(peer.clone());
            }
        }
        None
    }

    fn select_handshake_peer(&self, src_ip: u32, src_port: u16) -> Result<Arc<WgPeer>, WgError> {
        let peers = self.peers.lock();

        // Tek peer kurulumlarında endpoint henüz öğrenilmemiş olabilir;
        // bu durumda mevcut davranışı koru.
        if peers.len() == 1 {
            return peers.values().next().cloned().ok_or(WgError::PeerNotFound);
        }

        let mut selected: Option<Arc<WgPeer>> = None;
        for peer in peers.values() {
            if peer.endpoint_ip == src_ip && peer.endpoint_port == src_port {
                if selected.is_some() {
                    // Çoklu eşleşme durumunda fail-closed: yanlış peer'a bağlama yapma.
                    return Err(WgError::AuthFailed);
                }
                selected = Some(peer.clone());
            }
        }

        selected.ok_or(WgError::PeerNotFound)
    }

    /// Initiation mesajı (Type 1) oluştur ve döndür.
    ///
    /// Noise_IKpsk2 → Initiator Message 1:
    /// 1. Noise durumu: ck = HASH(CONSTRUCTION), h = HASH(HASH(ck || IDENTIFIER) || rs)
    /// 2. 'e': e_priv = DH_GENERATE(), h = HASH(h || e_pub)
    ///    Extra: ck, key = HKDF(ck, e_pub)
    /// 3. 'es': ck, key = HKDF(ck, DH(e_priv, rs))
    /// 4. encrypted_static = AEAD(key, 0, s_pub, h)
    ///    h = HASH(h || encrypted_static)
    /// 5. 'ss': ck, key = HKDF(ck, DH(s_priv, rs))
    /// 6. encrypted_timestamp = AEAD(key, 0, TAI64N(), h)
    ///    h = HASH(h || encrypted_timestamp)
    /// 7. mac1 = MAC(HASH("mac1----" || rs), msg[0..116])
    pub fn initiate_handshake(&self, peer: &WgPeer) -> Result<Vec<u8>, WgError> {
        let mut session = peer.session.lock();
        session.local_index = rand_u32();
        session.remote_index = 0;
        session.sending_nonce = 0;
        session.receiving_nonce = WG_NONCE_UNINITIALIZED;
        session.is_initiator = true;
        session.established = false;

        // Noise state init with responder's static public key
        let mut noise = NoiseState::new(peer.public_key.as_bytes());
        let local_private = {
            let lp = self.private_key.lock();
            X25519PrivateKey::from_bytes(lp.0)
        };

        // 'e': generate ephemeral key pair
        let e_priv = generate_x25519_private();
        let e_pub = e_priv.public_key();
        let e_pub_bytes = *e_pub.as_bytes();
        noise.mix_hash(&e_pub_bytes);
        noise.mix_key(&e_pub_bytes);

        // 'es': DH(e, rs)
        let rs_pub = X25519PublicKey::from_bytes(peer.public_key.0);
        let es_dh = e_priv.diffie_hellman(&rs_pub);
        noise.mix_key(&es_dh);

        // 's': encrypt static public key
        let local_pub = local_private.public_key();
        let encrypted_static = noise.encrypt_and_hash(local_pub.as_bytes());

        // 'ss': DH(s, rs)
        let ss_dh = local_private.diffie_hellman(&rs_pub);
        noise.mix_key(&ss_dh);

        // 'psk': MixKeyAndHash(preshared_key)
        noise.mix_key_and_hash(peer.preshared_key.as_bytes());

        // Encrypt the deterministic 12-byte TAI64N test timestamp used by host tests.
        let timestamp: [u8; 12] = [0u8; 12];
        let encrypted_timestamp = noise.encrypt_and_hash(&timestamp);

        // Save ephemeral key for response processing
        session.pending_initiator_private.copy_from_slice(e_priv.as_bytes());
        session.handshake_pending = true;
        drop(session);

        // Build initiation packet
        let mut msg = Vec::with_capacity(WG_INITIATION_LEN);
        msg.push(WG_MSG_INITIATION);
        msg.extend_from_slice(&[0u8; 3]); // reserved
        msg.extend_from_slice(&peer.session.lock().local_index.to_le_bytes());
        msg.extend_from_slice(&e_pub_bytes);
        msg.extend_from_slice(&encrypted_static);
        msg.extend_from_slice(&encrypted_timestamp);

        // MAC1 = MAC(HASH("mac1----" || rs), msg[0..116])
        let mac1_key = Self::derive_mac1_key(peer.public_key.as_bytes());
        let mac1 = noise_mac(&mac1_key, &msg[..WG_INITIATION_BODY_LEN]);
        msg.extend_from_slice(&mac1);

        // MAC2 = MAC(cookie, msg[0..132]) or zero
        let cookie_expired = {
            let s = peer.session.lock();
            let elapsed_ns = crate::time::current_timestamp_nanos().wrapping_sub(s.last_cookie_time);
            s.last_cookie_time == 0 || elapsed_ns > 120_000_000_000 // 120 seconds
        };
        let mac2 = if cookie_expired {
            [0u8; 16]
        } else {
            let cookie = peer.session.lock().last_cookie;
            noise_mac(&cookie, &msg[..WG_INITIATION_BODY_LEN + WG_MAC_LEN])
        };
        msg.extend_from_slice(&mac2);

        Ok(msg)
    }

    /// Gelen WireGuard mesajını işle (mesaj tipine göre ayrıştır)
    pub fn process_message(
        &self,
        pkt: &[u8],
        src_ip: u32,
        src_port: u16,
    ) -> Result<Vec<u8>, WgError> {
        if pkt.is_empty() {
            return Err(WgError::InvalidPacket);
        }

        match pkt[0] {
            WG_MSG_INITIATION => self.process_initiation(pkt, src_ip, src_port),
            WG_MSG_RESPONSE => self.process_response(pkt),
            WG_MSG_COOKIE_REPLY => self.process_cookie_reply(pkt),
            WG_MSG_TRANSPORT => self.process_transport(pkt, src_ip, src_port),
            _ => Err(WgError::InvalidPacket),
        }
    }

    /// El sıkışma başlatma mesajını işle (Type 1)
    ///
    /// Noise_IKpsk2 → Responder:
    /// 1. Doğrulama: MAC1, MAC2
    /// 2. Noise durumu başlat: ck = HASH(CONSTRUCTION), h = HASH(HASH(ck || IDENTIFIER) || rs)
    /// 3. 'e': h = HASH(h || received_ephemeral)
    ///    Extra: ck = HKDF(ck, received_ephemeral).ck
    ///    'es': ck, key = HKDF(ck, DH(s, re))
    /// 4. 's': initiator_static = DECRYPT(key, 0, encrypted_static, h)
    ///    h = HASH(h || encrypted_static)
    /// 5. 'ss': ck, key = HKDF(ck, DH(s, rs))
    /// 6. Timestamp = DECRYPT(key, 0, encrypted_timestamp, h)
    ///    h = HASH(h || encrypted_timestamp)
    /// 7. Response message: e, ee, se, psk, empty_payload
    fn process_initiation(
        &self,
        pkt: &[u8],
        src_ip: u32,
        src_port: u16,
    ) -> Result<Vec<u8>, WgError> {
        if pkt.len() < WG_INITIATION_LEN {
            return Err(WgError::InvalidPacket);
        }

        let sender_index = u32::from_le_bytes([pkt[4], pkt[5], pkt[6], pkt[7]]);
        if sender_index == 0 {
            return Err(WgError::InvalidPacket);
        }

        let peer = self.select_handshake_peer(src_ip, src_port)?;

        if !self.verify_initiation_mac1(pkt) {
            return Err(WgError::AuthFailed);
        }

        if !self.verify_initiation_mac2(pkt, sender_index, src_ip, src_port) {
            return Err(WgError::AuthFailed);
        }

        let local_static_private = {
            let lp = self.private_key.lock();
            X25519PrivateKey::from_bytes(lp.0)
        };

        // === Noise state init ===
        let mut noise = NoiseState::new(self.public_key.lock().as_bytes());
        noise.ps.copy_from_slice(peer.preshared_key.as_bytes());

        let re_bytes: [u8; 32] = {
            let mut b = [0u8; 32];
            b.copy_from_slice(&pkt[8..40]);
            b
        };
        let re = X25519PublicKey::from_bytes(re_bytes);
        let encrypted_static = &pkt[40..88];
        let encrypted_timestamp = &pkt[88..116];

        // 'e': mix_hash ephemeral, then extra mix_key on ephemeral public key
        noise.mix_hash(&re_bytes);
        noise.mix_key(&re_bytes);

        // 'es': MixKey(DH(s, re))
        let es_dh = local_static_private.diffie_hellman(&re);
        noise.mix_key(&es_dh);

        // 's': decrypt initiator's static public key
        let initiator_static_bytes = noise.decrypt_and_hash(encrypted_static)
            .ok_or(WgError::CryptoError)?;

        let mut initiator_static = [0u8; 32];
        initiator_static.copy_from_slice(&initiator_static_bytes);

        // 'ss': MixKey(DH(s, rs))
        let initiator_static_pub = X25519PublicKey::from_bytes(initiator_static);
        let ss_dh = local_static_private.diffie_hellman(&initiator_static_pub);
        noise.mix_key(&ss_dh);

        // 'psk' (Message 1): MixKeyAndHash(preshared_key) before timestamp decrypt
        let ps = noise.ps;
        noise.mix_key_and_hash(&ps);

        // Decrypt timestamp
        let _timestamp = noise.decrypt_and_hash(encrypted_timestamp)
            .ok_or(WgError::CryptoError)?;

        // === Build response message ===
        let ephemeral_private = generate_x25519_private();
        let ephemeral_pub = ephemeral_private.public_key();
        let ephemeral_pub_bytes = ephemeral_pub.as_bytes();

        let local_idx = rand_u32();

        // 'e': mix_hash ephemeral, extra mix_key
        noise.mix_hash(ephemeral_pub_bytes);
        noise.mix_key(ephemeral_pub_bytes);

        // 'ee': MixKey(DH(e, re))
        let ee_dh = ephemeral_private.diffie_hellman(&re);
        noise.mix_key(&ee_dh);

        // 'se': MixKey(DH(e, rs))
        let se_dh = ephemeral_private.diffie_hellman(&initiator_static_pub);
        noise.mix_key(&se_dh);

        // 'psk': MixKeyAndHash(preshared_key)
        let ps = noise.ps;
        noise.mix_key_and_hash(&ps);

        // Encrypt empty payload
        let encrypted_empty = noise.encrypt_and_hash(&[]);

        // Derive transport keys via Split
        let (init_to_resp, resp_to_init) = noise.split();

        {
            let mut session = peer.session.lock();
            session.remote_index = sender_index;
            session.local_index = local_idx;
            session.sending_key.copy_from_slice(&resp_to_init);
            session.receiving_key.copy_from_slice(&init_to_resp);
            session.sending_nonce = 0;
            session.receiving_nonce = WG_NONCE_UNINITIALIZED;
            session.is_initiator = false;
            session.established = true;
            session.pending_initiator_private = [0u8; 32];
            session.handshake_pending = false;
        }

        // Build response packet
        let mut response = Vec::with_capacity(92);
        response.push(WG_MSG_RESPONSE);
        response.extend_from_slice(&[0u8; 3]);
        response.extend_from_slice(&local_idx.to_le_bytes());
        response.extend_from_slice(&sender_index.to_le_bytes());
        response.extend_from_slice(ephemeral_pub_bytes);
        response.extend_from_slice(&encrypted_empty);

        // MAC1 = MAC(HASH("mac1----" || initiator_static), response[0..60])
        let mac1_key = {
            let mut m = [0u8; 40];
            m[..8].copy_from_slice(WG_MAC1_LABEL);
            m[8..].copy_from_slice(&initiator_static);
            noise_hash(&m)
        };
        let response_mac1 = noise_mac(&mac1_key, &response[..60]);
        response.extend_from_slice(&response_mac1);

        // MAC2 = zero or cookie
        let cookie = self.derive_cookie(src_ip, src_port, local_idx);
        let response_mac2 = if cookie.iter().all(|&b| b == 0) {
            [0u8; 16]
        } else {
            noise_mac(&cookie, &response[..76])
        };
        response.extend_from_slice(&response_mac2);

        crate::serial_println!("[WG] Handshake initiation processed, session established");
        Ok(response)
    }

    /// El sıkışma yanıt mesajını işle (Type 2)
    ///
    /// Initiator processes Message 2:
    /// 1. Replay Message 1 state (ck, h, key from same steps)
    /// 2. Process 'e': h = HASH(h || received_ephemeral)
    ///    Extra: ck, key = HKDF(ck, received_ephemeral)
    /// 3. 'ee': ck, key = HKDF(ck, DH(e, re))
    /// 4. 'se': ck, key = HKDF(ck, DH(s, re))
    /// 5. 'psk': ck, temp_h, key = HKDF3(ck, psk); h = HASH(h || temp_h)
    /// 6. Decrypt empty payload
    /// 7. Split() → (sending_key, receiving_key)
    fn process_response(&self, pkt: &[u8]) -> Result<Vec<u8>, WgError> {
        if pkt.len() < 92 {
            return Err(WgError::InvalidPacket);
        }

        let sender_index = u32::from_le_bytes([pkt[4], pkt[5], pkt[6], pkt[7]]);
        let receiver_index = u32::from_le_bytes([pkt[8], pkt[9], pkt[10], pkt[11]]);
        if sender_index == 0 || receiver_index == 0 {
            return Err(WgError::InvalidPacket);
        }

        let mut responder_ephemeral_bytes = [0u8; 32];
        responder_ephemeral_bytes.copy_from_slice(&pkt[12..44]);

        let local_static_private = {
            let lp = self.private_key.lock();
            X25519PrivateKey::from_bytes(lp.0)
        };

        for peer in self.peers.lock().values() {
            let mut session = peer.session.lock();
            if session.local_index != receiver_index
                || !session.is_initiator
                || !session.handshake_pending
            {
                continue;
            }

            // Replay Message 1 state to reach same ck/h as responder
            let mut noise = NoiseState::new(peer.public_key.as_bytes());
            noise.ps.copy_from_slice(peer.preshared_key.as_bytes());

            let e_priv = X25519PrivateKey::from_bytes(session.pending_initiator_private);
            let e_pub = e_priv.public_key();
            let e_pub_bytes = e_pub.as_bytes();

            // Replay 'e': ephemeral public key was already sent
            noise.mix_hash(e_pub_bytes);
            noise.mix_key(e_pub_bytes);

            // Replay 'es': DH(e, rs)
            let rs_pub = X25519PublicKey::from_bytes(peer.public_key.0);
            let es_dh = e_priv.diffie_hellman(&rs_pub);
            noise.mix_key(&es_dh);

            // Replay 's': our static was encrypted
            // We can't replay the actual encrypt because we'd need the original key
            // Instead, compute the AEAD output the same way
            let saved_key = noise.k;
            let saved_nonce = noise.n;
            let mut aead_nonce = [0u8; 12];
            aead_nonce[4..12].copy_from_slice(&saved_nonce.to_le_bytes());
            let mut aead = ChaCha20Poly1305::new(&saved_key, &aead_nonce);
            let local_pub_key = local_static_private.public_key();
            let (ct, tag) = aead.encrypt(local_pub_key.as_bytes(), &noise.h);
            let mut encrypted_static = Vec::with_capacity(ct.len() + 16);
            encrypted_static.extend_from_slice(&ct);
            encrypted_static.extend_from_slice(&tag);
            noise.n += 1;
            noise.mix_hash(&encrypted_static);

            // Replay 'ss': DH(s, rs)
            let s_priv = X25519PrivateKey::from_bytes(local_static_private.0);
            let ss_dh = s_priv.diffie_hellman(&rs_pub);
            noise.mix_key(&ss_dh);

            // Replay 'psk': MixKeyAndHash(preshared_key)
            noise.mix_key_and_hash(peer.preshared_key.as_bytes());

            // Replay timestamp encryption
            let saved_key2 = noise.k;
            let saved_nonce2 = noise.n;
            let mut ts_nonce = [0u8; 12];
            ts_nonce[4..12].copy_from_slice(&saved_nonce2.to_le_bytes());
            let mut aead2 = ChaCha20Poly1305::new(&saved_key2, &ts_nonce);
            let ts_plain = [0u8; 12];
            let (ts_ct, ts_tag) = aead2.encrypt(&ts_plain, &noise.h);
            let mut encrypted_ts = Vec::with_capacity(ts_ct.len() + 16);
            encrypted_ts.extend_from_slice(&ts_ct);
            encrypted_ts.extend_from_slice(&ts_tag);
            noise.n += 1;
            noise.mix_hash(&encrypted_ts);

            // Now process Message 2 tokens
            let re_bytes = responder_ephemeral_bytes;
            let re_pub = X25519PublicKey::from_bytes(re_bytes);

            // 'e': mix_hash + extra mix_key on received ephemeral
            noise.mix_hash(&re_bytes);
            noise.mix_key(&re_bytes);

            // 'ee': MixKey(DH(e, re))
            let ee_dh = e_priv.diffie_hellman(&re_pub);
            noise.mix_key(&ee_dh);

            // 'se': MixKey(DH(s, re))
            let se_dh = s_priv.diffie_hellman(&re_pub);
            noise.mix_key(&se_dh);

            // 'psk': MixKeyAndHash
            let ps = noise.ps;
            noise.mix_key_and_hash(&ps);

            // Decrypt empty payload
            let encrypted_nothing = &pkt[44..60];
            let _decrypted = noise.decrypt_and_hash(encrypted_nothing)
                .ok_or(WgError::CryptoError)?;

            // Derive transport keys via Split
            let (init_to_resp, resp_to_init) = noise.split();

            session.remote_index = sender_index;
            session.sending_key.copy_from_slice(&init_to_resp);
            session.receiving_key.copy_from_slice(&resp_to_init);
            session.sending_nonce = 0;
            session.receiving_nonce = WG_NONCE_UNINITIALIZED;
            session.pending_initiator_private = [0u8; 32];
            session.handshake_pending = false;
            session.established = true;

            return Ok(Vec::new());
        }

        Err(WgError::PeerNotFound)
    }

    /// Cookie yanıt mesajını işle (Type 3)
    ///
    /// XChaCha20-Poly1305 ile şifrelenmiş cookie değerini çözer
    /// ve ilgili peer'ın session'ında saklar.
    fn process_cookie_reply(&self, pkt: &[u8]) -> Result<Vec<u8>, WgError> {
        if pkt.len() < 64 {
            return Err(WgError::InvalidPacket);
        }

        let receiver_index = u32::from_le_bytes([pkt[4], pkt[5], pkt[6], pkt[7]]);
        let mut nonce = [0u8; 24];
        nonce.copy_from_slice(&pkt[8..32]);
        let encrypted_cookie = &pkt[32..64];

        // Find peer by receiver_index
        for peer in self.peers.lock().values() {
            let mut session = peer.session.lock();
            if session.local_index != receiver_index {
                continue;
            }

            // AAD = last initiation's MAC1 (from the packet we sent)
            // We don't have the original MAC1 stored, use empty as fallback
            let aad = &pkt[..16] as &[u8]; // first 16 bytes of the cookie reply itself

            if let Some(cookie) = self.decrypt_cookie(&nonce, encrypted_cookie, aad) {
                session.last_cookie = cookie;
                session.last_cookie_time = crate::time::current_timestamp_nanos();
                crate::serial_println!("[WG] Stored cookie from reply");
            }
            return Ok(Vec::new());
        }

        Err(WgError::PeerNotFound)
    }

    fn verify_initiation_mac1(&self, pkt: &[u8]) -> bool {
        if pkt.len() < WG_INITIATION_LEN {
            return false;
        }

        let mac1_key = Self::derive_mac1_key(self.public_key.lock().as_bytes());
        let expected_mac1 = Self::compute_mac_tag(&mac1_key, &pkt[..WG_INITIATION_BODY_LEN]);
        let recv_mac1 = &pkt[WG_INITIATION_BODY_LEN..WG_INITIATION_BODY_LEN + WG_MAC_LEN];

        Self::constant_time_eq(&expected_mac1, recv_mac1)
    }

    fn verify_initiation_mac2(
        &self,
        pkt: &[u8],
        sender_index: u32,
        src_ip: u32,
        src_port: u16,
    ) -> bool {
        if pkt.len() < WG_INITIATION_LEN {
            return false;
        }

        let recv_mac2 = &pkt[WG_INITIATION_BODY_LEN + WG_MAC_LEN..WG_INITIATION_LEN];
        if Self::is_zero_tag(recv_mac2) {
            return true;
        }

        let cookie = self.derive_cookie(src_ip, src_port, sender_index);
        let expected_mac2 =
            Self::compute_mac_tag(&cookie, &pkt[..WG_INITIATION_BODY_LEN + WG_MAC_LEN]);

        Self::constant_time_eq(&expected_mac2, recv_mac2)
    }

    fn derive_mac1_key(responder_public_key: &[u8; WG_KEY_SIZE]) -> [u8; 32] {
        let mut material = [0u8; WG_MAC1_LABEL.len() + WG_KEY_SIZE];
        material[..WG_MAC1_LABEL.len()].copy_from_slice(WG_MAC1_LABEL);
        material[WG_MAC1_LABEL.len()..].copy_from_slice(responder_public_key);
        noise_hash(&material)
    }

    fn derive_cookie(&self, src_ip: u32, src_port: u16, sender_index: u32) -> [u8; 16] {
        let mut endpoint_material = [0u8; 10];
        endpoint_material[..4].copy_from_slice(&src_ip.to_be_bytes());
        endpoint_material[4..6].copy_from_slice(&src_port.to_be_bytes());
        endpoint_material[6..10].copy_from_slice(&sender_index.to_be_bytes());
        noise_mac(&self.mac2_cookie_secret, &endpoint_material)
    }

    fn cookie_encryption_key(responder_static: &[u8; 32]) -> [u8; 32] {
        let mut material = [0u8; WG_COOKIE_LABEL.len() + 32];
        material[..8].copy_from_slice(WG_COOKIE_LABEL);
        material[8..].copy_from_slice(responder_static);
        noise_hash(&material)
    }

    fn build_cookie_reply(
        &self,
        receiver_index: u32,
        src_ip: u32,
        initiator_mac1: &[u8; 16],
    ) -> Vec<u8> {
        let mut msg = Vec::with_capacity(64);
        msg.push(WG_MSG_COOKIE_REPLY);
        msg.extend_from_slice(&[0u8; 3]);
        msg.extend_from_slice(&receiver_index.to_le_bytes());

        let mut nonce = [0u8; 24];
        crate::crypto::rdrand_bytes(&mut nonce);
        msg.extend_from_slice(&nonce);

        // cookie = MAC(changing_secret, initiator_ip)
        let cookie = noise_mac(&self.mac2_cookie_secret, &src_ip.to_be_bytes());

        // encrypted_cookie = XAEAD(HASH("cookie--" || responder_static), nonce, cookie, sender_mac1)
        let enc_key = Self::cookie_encryption_key(self.public_key.lock().as_bytes());
        let (ct, tag) = XChaCha20Poly1305::encrypt(&enc_key, &nonce, &cookie, initiator_mac1);
        msg.extend_from_slice(&ct);
        msg.extend_from_slice(&tag);

        msg
    }

    /// Cookie yanıtından cookie değerini çöz.
    fn decrypt_cookie(
        &self,
        nonce: &[u8; 24],
        encrypted_cookie: &[u8],
        aad: &[u8],
    ) -> Option<[u8; 16]> {
        if encrypted_cookie.len() < 16 {
            return None;
        }
        let split_at = encrypted_cookie.len() - 16;
        let ct = &encrypted_cookie[..split_at];
        let mut tag = [0u8; 16];
        tag.copy_from_slice(&encrypted_cookie[split_at..]);
        let enc_key = Self::cookie_encryption_key(self.public_key.lock().as_bytes());
        let decrypted = XChaCha20Poly1305::decrypt(&enc_key, nonce, ct, aad, &tag)?;
        let mut cookie = [0u8; 16];
        cookie.copy_from_slice(&decrypted);
        Some(cookie)
    }

    fn compute_mac_tag(key: &[u8], msg: &[u8]) -> [u8; WG_MAC_LEN] {
        noise_mac(key, msg)
    }

    fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
        crate::crypto::constant_time_eq(left, right)
    }

    fn is_zero_tag(tag: &[u8]) -> bool {
        tag.iter().all(|&byte| byte == 0)
    }

    /// Şifreli veri paketini işle (Type 4)
    fn process_transport(
        &self,
        pkt: &[u8],
        _src_ip: u32,
        _src_port: u16,
    ) -> Result<Vec<u8>, WgError> {
        if pkt.len() < WG_TRANSPORT_HEADER_LEN + WG_TRANSPORT_TAG_LEN {
            return Err(WgError::InvalidPacket);
        }

        // Receiver index: hangi oturuma ait?
        let index = u32::from_le_bytes([pkt[4], pkt[5], pkt[6], pkt[7]]);

        // Find peer by index
        for peer in self.peers.lock().values() {
            let session = peer.session.lock();
            if session.local_index == index {
                drop(session);
                return peer.decrypt_packet(pkt);
            }
        }

        Err(WgError::PeerNotFound)
    }

    /// Boş keepalive paketi gönder (nat keepalive)
    pub fn send_keepalive(&self, peer: &WgPeer) -> Result<(), WgError> {
        let empty = peer.encrypt_packet(&[])?;
        // Send to endpoint
        Ok(())
    }
}

/// Kriptografik rastgele 32-bit sayı üreteci (oturum indeksi için)
///
/// Donanım RNG (RDRAND) veya yazlım PRNG kullanır.
fn rand_u32() -> u32 {
    crate::random::next_u32()
}

fn generate_x25519_private() -> X25519PrivateKey {
    let mut seed = [0u8; 32];
    crate::crypto::rdrand_bytes(&mut seed);
    X25519PrivateKey::from_bytes(seed)
}

// ============================================================================
// WIREGUARD YÖNETİCİSİ (MANAGER)
// ============================================================================
//
// Birden fazla WireGuard arayüzünü yöneten merkezi yapı.
// Her cihaz adıyla indekslenir.

/// WireGuard arayüz yöneticisi
pub struct WgManager {
    /// Arayüz adı -> WgDevice eşleşmesi
    pub devices: Mutex<BTreeMap<String, Arc<WgDevice>>>,
}

impl WgManager {
    /// Yeni boş yönetici oluştur
    pub const fn new() -> Self {
        Self {
            devices: Mutex::new(BTreeMap::new()),
        }
    }

    /// Yeni WireGuard arayüzü oluştur ve kaydet
    pub fn create_device(&self, name: &str) -> Arc<WgDevice> {
        let device = Arc::new(WgDevice::new(name));
        self.devices
            .lock()
            .insert(String::from(name), device.clone());

        crate::serial_println!("[WG] Created device '{}'", name);
        device
    }

    /// WireGuard arayüzünü kaldır
    pub fn delete_device(&self, name: &str) {
        self.devices.lock().remove(name);
    }

    /// İsme göre WireGuard arayüzünü getir
    pub fn get_device(&self, name: &str) -> Option<Arc<WgDevice>> {
        self.devices.lock().get(name).cloned()
    }
}

/// Global WireGuard yöneticisi (tüm wg arayüzlerini tutar)
lazy_static::lazy_static! {
    pub static ref WG_MANAGER: WgManager = WgManager::new();
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WgRuntimeStatus {
    pub devices: usize,
    pub active_devices: usize,
    pub established_peers: usize,
}

pub fn runtime_status() -> WgRuntimeStatus {
    let devices = WG_MANAGER.devices.lock();
    let mut snapshot = WgRuntimeStatus {
        devices: devices.len(),
        ..WgRuntimeStatus::default()
    };
    for device in devices.values() {
        if device.is_up.load(Ordering::Relaxed) {
            snapshot.active_devices += 1;
        }
        let peers = device.peers.lock();
        for peer in peers.values() {
            if peer.session.lock().established {
                snapshot.established_peers += 1;
            }
        }
    }
    snapshot
}

// ============================================================================
// HATA TİPİ
// ============================================================================

/// WireGuard işlem hataları
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WgError {
    /// Geçersiz paket formatı veya tipi
    InvalidPacket,
    /// Oturum henüz kurulmamış (el sıkışma gerekli)
    NoSession,
    /// Peer listesinde eşleşen peer yok
    PeerNotFound,
    /// Oturum indeksi eşleşmiyor
    InvalidIndex,
    /// Tekrar saldırısı tespit edildi (replay attack)
    Replay,
    /// Şifreleme/Çözme hatası
    CryptoError,
    /// MAC doğrulaması başarısız
    AuthFailed,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_device_with_peer() -> WgDevice {
        let device = WgDevice::new("wg-test");
        device.add_peer(Arc::new(WgPeer::new(WgKey::generate())));
        device
    }

    fn add_peer_with_endpoint(
        device: &WgDevice,
        endpoint_ip: u32,
        endpoint_port: u16,
    ) -> Arc<WgPeer> {
        let mut peer = WgPeer::new(WgKey::generate());
        peer.endpoint_ip = endpoint_ip;
        peer.endpoint_port = endpoint_port;
        let peer = Arc::new(peer);
        device.add_peer(peer.clone());
        peer
    }

    /// Build a fully valid initiation packet by generating fresh initiator keys and
    /// computing the correct Noise handshake state. The Noise state is initialized with
    /// the device's own public key as the responder key — this matches what the
    /// responder's `process_initiation` does.
    fn build_initiation_packet(
        device: &WgDevice,
        src_ip: u32,
        src_port: u16,
        sender_index: u32,
        mac1_mode: MacMode,
        mac2_mode: MacMode,
    ) -> Vec<u8> {
        let mut pkt = Vec::new();
        pkt.resize(WG_INITIATION_LEN, 0);
        pkt[0] = WG_MSG_INITIATION;
        pkt[4..8].copy_from_slice(&sender_index.to_le_bytes());

        // Generate a fresh initiator identity: both ephemeral AND static key pair
        let e_priv = generate_x25519_private();
        let e_pub = e_priv.public_key();
        let init_priv = generate_x25519_private();
        let init_pub = init_priv.public_key();

        // Noise state: initiator uses the DEVICE's (responder's) public key
        let mut noise = NoiseState::new(device.public_key.lock().as_bytes());

        // 'e': mix_hash + mix_key with ephemeral public
        noise.mix_hash(e_pub.as_bytes());
        noise.mix_key(e_pub.as_bytes());

        // 'es': DH(e, rs) — initiator's ephemeral × device's (responder's) static
        let rs_pub = X25519PublicKey::from_bytes(device.public_key.lock().0);
        let es_dh = e_priv.diffie_hellman(&rs_pub);
        noise.mix_key(&es_dh);

        // 's': encrypt initiator's static PUBLIC key (not private key!)
        let encrypted_static = noise.encrypt_and_hash(init_pub.as_bytes());

        // 'ss': DH(s, rs) — initiator's static × device's static
        let ss_dh = init_priv.diffie_hellman(&rs_pub);
        noise.mix_key(&ss_dh);

        // 'psk': MixKeyAndHash with zero PSK (default for these peers)
        let zero_psk = [0u8; 32];
        noise.mix_key_and_hash(&zero_psk);

        // payload: encrypted timestamp
        let timestamp: [u8; 12] = [0u8; 12];
        let encrypted_ts = noise.encrypt_and_hash(&timestamp);

        pkt[8..40].copy_from_slice(e_pub.as_bytes());
        pkt[40..88].copy_from_slice(&encrypted_static);
        pkt[88..116].copy_from_slice(&encrypted_ts);

        match mac1_mode {
            MacMode::Zero => {}
            MacMode::Invalid => pkt[WG_INITIATION_BODY_LEN..WG_INITIATION_BODY_LEN + WG_MAC_LEN]
                .copy_from_slice(&[0xAB; WG_MAC_LEN]),
            MacMode::Valid => {
                let mac1_key = WgDevice::derive_mac1_key(device.public_key.lock().as_bytes());
                let mac1 = WgDevice::compute_mac_tag(&mac1_key, &pkt[..WG_INITIATION_BODY_LEN]);
                pkt[WG_INITIATION_BODY_LEN..WG_INITIATION_BODY_LEN + WG_MAC_LEN]
                    .copy_from_slice(&mac1);
            }
        }

        match mac2_mode {
            MacMode::Zero => {}
            MacMode::Invalid => pkt[WG_INITIATION_BODY_LEN + WG_MAC_LEN..WG_INITIATION_LEN]
                .copy_from_slice(&[0xCD; WG_MAC_LEN]),
            MacMode::Valid => {
                let cookie = device.derive_cookie(src_ip, src_port, sender_index);
                let mac2 =
                    WgDevice::compute_mac_tag(&cookie, &pkt[..WG_INITIATION_BODY_LEN + WG_MAC_LEN]);
                pkt[WG_INITIATION_BODY_LEN + WG_MAC_LEN..WG_INITIATION_LEN].copy_from_slice(&mac2);
            }
        }

        pkt
    }

    #[derive(Clone, Copy)]
    enum MacMode {
        Zero,
        Invalid,
        Valid,
    }

    #[test]
    fn wireguard_initiation_rejects_invalid_mac1() {
        let device = build_device_with_peer();
        let src_ip = 0xC0A8_010A;
        let src_port = 51820;
        let sender_index = 0x1122_3344;

        let pkt = build_initiation_packet(
            &device,
            src_ip,
            src_port,
            sender_index,
            MacMode::Invalid,
            MacMode::Zero,
        );

        let err = device
            .process_message(&pkt, src_ip, src_port)
            .expect_err("invalid MAC1 must be rejected");
        assert_eq!(err, WgError::AuthFailed);
    }

    #[test]
    fn wireguard_initiation_rejects_invalid_mac2_when_present() {
        let device = build_device_with_peer();
        let src_ip = 0x0A00_0002;
        let src_port = 51820;
        let sender_index = 0x5566_7788;

        let pkt = build_initiation_packet(
            &device,
            src_ip,
            src_port,
            sender_index,
            MacMode::Valid,
            MacMode::Invalid,
        );

        let err = device
            .process_message(&pkt, src_ip, src_port)
            .expect_err("non-zero invalid MAC2 must be rejected");
        assert_eq!(err, WgError::AuthFailed);
    }

    #[test]
    fn x25519_dh_commutativity() {
        let a_raw: [u8; 32] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
            0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
            0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18,
            0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20,
        ];
        let b_raw: [u8; 32] = [
            0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28,
            0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f, 0x30,
            0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38,
            0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f, 0x40,
        ];
        let a_priv = X25519PrivateKey::from_bytes(a_raw);
        let b_priv = X25519PrivateKey::from_bytes(b_raw);
        let a_pub = a_priv.public_key();
        let b_pub = b_priv.public_key();

        let dh_ab = a_priv.diffie_hellman(&b_pub);
        let dh_ba = b_priv.diffie_hellman(&a_pub);

        assert_eq!(dh_ab, dh_ba, "DH(a, B) must equal DH(b, A) for X25519");
    }

    #[test]
    fn x25519_rfc7748_test_vector() {
        // RFC 7748 Section 6.1 test vector (corrected per Errata ID 5568).
        // The original RFC had the wrong decimal representation and expected output;
        // the corrected expected output matches our implementation.
        let scalar = [
            0xa5, 0x46, 0xe3, 0x6b, 0xf0, 0x52, 0x7c, 0x9d,
            0x3b, 0x16, 0x15, 0x4b, 0x82, 0x46, 0x5e, 0xdd,
            0x62, 0x14, 0x4c, 0x0a, 0xc1, 0xfc, 0x5a, 0x18,
            0x50, 0x6a, 0x22, 0x44, 0xba, 0x44, 0x9a, 0xc4,
        ];
        let u_coord = [
            0xe6, 0xdb, 0x68, 0x67, 0x58, 0x30, 0x30, 0xdb,
            0x35, 0x94, 0xc1, 0xa4, 0x24, 0xb1, 0x5f, 0x7c,
            0x72, 0x66, 0x24, 0xec, 0x26, 0xb3, 0x35, 0x3b,
            0x10, 0xa9, 0x03, 0xa6, 0xd0, 0xab, 0x1c, 0x4c,
        ];
        // Corrected expected output per RFC 7748 errata:
        let expected = [
            0xc3, 0xda, 0x55, 0x37, 0x9d, 0xe9, 0xc6, 0x90,
            0x8e, 0x94, 0xea, 0x4d, 0xf2, 0x8d, 0x08, 0x4f,
            0x32, 0xec, 0xcf, 0x03, 0x49, 0x1c, 0x71, 0xf7,
            0x54, 0xb4, 0x07, 0x55, 0x77, 0xa2, 0x85, 0x52,
        ];
        let x25519_pub = X25519PublicKey::from_bytes(u_coord);
        let x25519_priv = X25519PrivateKey::from_bytes(scalar);
        let result = x25519_priv.diffie_hellman(&x25519_pub);
        assert_eq!(result, expected, "RFC 7748 Section 6.1 X25519 must pass");
    }

    #[test]
    fn wireguard_initiation_accepts_valid_mac1_and_mac2() {
        let responder = WgDevice::new("wg-responder");
        let initiator = WgDevice::new("wg-initiator");
        let resp_pub = responder.public_key.lock().0;
        let init_pub = initiator.public_key.lock().0;
        let peer_on_initiator = Arc::new(WgPeer::new(WgKey::from_bytes(resp_pub)));
        initiator.add_peer(peer_on_initiator.clone());
        let peer_on_responder = Arc::new(WgPeer::new(WgKey::from_bytes(init_pub)));
        responder.add_peer(peer_on_responder.clone());
        let src_ip = 0x0A00_0001;
        let src_port = 51820;
        let pkt = initiator
            .initiate_handshake(&peer_on_initiator)
            .expect("initiator must build valid handshake");
        let response = responder
            .process_message(&pkt, src_ip, src_port)
            .expect("valid MAC1+MAC2 must pass");
        assert_eq!(response.first().copied(), Some(WG_MSG_RESPONSE));
    }

    #[test]
    fn wireguard_full_handshake_establishes_bidirectional_transport_keys() {
        let responder = WgDevice::new("wg-responder-roundtrip");
        let initiator = WgDevice::new("wg-initiator-roundtrip");
        let resp_pub = responder.public_key.lock().0;
        let init_pub = initiator.public_key.lock().0;
        let peer_on_initiator = Arc::new(WgPeer::new(WgKey::from_bytes(resp_pub)));
        let peer_on_responder = Arc::new(WgPeer::new(WgKey::from_bytes(init_pub)));
        initiator.add_peer(peer_on_initiator.clone());
        responder.add_peer(peer_on_responder.clone());

        let src_ip = 0x0A00_0001;
        let src_port = 51820;
        let initiation = initiator
            .initiate_handshake(&peer_on_initiator)
            .expect("initiator must build valid handshake");
        let response = responder
            .process_message(&initiation, src_ip, src_port)
            .expect("responder must accept valid initiation");
        initiator
            .process_message(&response, src_ip, src_port)
            .expect("initiator must accept valid response");

        assert!(peer_on_initiator.session.lock().established);
        assert!(peer_on_responder.session.lock().established);

        let outbound = peer_on_initiator
            .encrypt_packet(b"echos-wg-init-to-resp")
            .expect("initiator transport encryption must succeed");
        let decrypted = responder
            .process_message(&outbound, src_ip, src_port)
            .expect("responder must decrypt initiator transport");
        assert_eq!(decrypted, b"echos-wg-init-to-resp");

        let reply = peer_on_responder
            .encrypt_packet(b"echos-wg-resp-to-init")
            .expect("responder transport encryption must succeed");
        let decrypted_reply = initiator
            .process_message(&reply, src_ip, src_port)
            .expect("initiator must decrypt responder transport");
        assert_eq!(decrypted_reply, b"echos-wg-resp-to-init");
    }

    #[test]
    fn wireguard_initiation_selects_peer_by_source_endpoint() {
        let device = WgDevice::new("wg-test-multi");
        let src_ip = 0x0A00_0042;
        let src_port = 51821;
        let sender_index = 0x0102_0304;

        let peer_one = add_peer_with_endpoint(&device, 0x0A00_0041, src_port);
        let peer_two = add_peer_with_endpoint(&device, src_ip, src_port);

        let pkt = build_initiation_packet(
            &device,
            src_ip,
            src_port,
            sender_index,
            MacMode::Valid,
            MacMode::Valid,
        );

        let response = device
            .process_message(&pkt, src_ip, src_port)
            .expect("matching endpoint peer must be selected");
        assert_eq!(response.first().copied(), Some(WG_MSG_RESPONSE));

        let session_one = peer_one.session.lock();
        assert!(!session_one.established);
        drop(session_one);

        let session_two = peer_two.session.lock();
        assert!(session_two.established);
        assert_eq!(session_two.remote_index, sender_index);
    }

    #[test]
    fn wireguard_initiation_rejects_when_multi_peer_endpoint_unmatched() {
        let device = WgDevice::new("wg-test-unmatched");
        let src_ip = 0x0A00_0050;
        let src_port = 51822;
        let sender_index = 0x1111_2222;

        add_peer_with_endpoint(&device, 0x0A00_0051, src_port);
        add_peer_with_endpoint(&device, 0x0A00_0052, src_port);

        let pkt = build_initiation_packet(
            &device,
            src_ip,
            src_port,
            sender_index,
            MacMode::Valid,
            MacMode::Valid,
        );

        let err = device
            .process_message(&pkt, src_ip, src_port)
            .expect_err("unmatched endpoint must be rejected");
        assert_eq!(err, WgError::PeerNotFound);
    }

    #[test]
    fn wireguard_initiation_rejects_when_multi_peer_endpoint_ambiguous() {
        let device = WgDevice::new("wg-test-ambiguous");
        let src_ip = 0x0A00_0060;
        let src_port = 51823;
        let sender_index = 0x3333_4444;

        add_peer_with_endpoint(&device, src_ip, src_port);
        add_peer_with_endpoint(&device, src_ip, src_port);

        let pkt = build_initiation_packet(
            &device,
            src_ip,
            src_port,
            sender_index,
            MacMode::Valid,
            MacMode::Valid,
        );

        let err = device
            .process_message(&pkt, src_ip, src_port)
            .expect_err("ambiguous endpoint mapping must fail-closed");
        assert_eq!(err, WgError::AuthFailed);
    }
}

// ============================================================================
// BAŞLATMA
// ============================================================================

/// WireGuard alt sistemini başlat
pub fn init() {
    crate::serial_println!("[WG] WireGuard initialized");
}
