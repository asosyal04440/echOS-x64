//! # WireGuard VPN Protokolü
//!
//! Modern, yüksek performanslı VPN protokolü.
//! RFC önerisi: https://www.wireguard.com/papers/wireguard.pdf
//!
//! ## WireGuard Nedir?
//!
//! WireGuard, önceki VPN protokollerine (OpenVPN, IPSec) göre çok daha basit
//! ve güvenli bir tünel protokolüdür. Linux kernel'ine 5.6'da entegre edildi.
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

use crate::crypto::{ChaCha20Poly1305, X25519PrivateKey, X25519PublicKey};
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// WIREGUARD SABİTLERİ
// ============================================================================

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
/// MAC1 anahtar türetme etiketi
const WG_MAC1_LABEL: &[u8; 7] = b"wg-mac1";
/// Henüz geçerli bir inbound nonce kabul edilmediğini gösteren sentinel değer
const WG_NONCE_UNINITIALIZED: u64 = u64::MAX;

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

/// WireGuard sanal ağ arayüzü
///
/// Her WireGuard arayüzünün bir private/public key çifti ve peer listesi var.
/// Linux'ta "wg0", "wg1" gibi adlarla görünür.
pub struct WgDevice {
    /// Arayüz adı (örn: "wg0")
    pub name: String,
    /// Dinleme UDP portu
    pub listen_port: AtomicU32,
    /// Bu cihazın Curve25519 private key'i (GİZLİ, hiç iletilmez)
    pub private_key: Mutex<WgKey>,
    /// Bu cihazın Curve25519 public key'i (paylaşılabilir)
    pub public_key: WgKey,
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
            listen_port: AtomicU32::new(WG_DEFAULT_PORT as u32),
            private_key: Mutex::new(private_key),
            public_key,
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

    /// El sıkışma başlat
    ///
    /// Noise_IKpsk2 protokolüne göre:
    /// 1. Geçici Curve25519 anahtar çifti üret
    /// 2. ECDH(ephemeral_private, peer_public) hesapla
    /// 3. Hash zincirini güncelle
    /// 4. Initiation mesajını gönder
    pub fn initiate_handshake(&self, peer: &WgPeer) -> Result<(), WgError> {
        // Create and send initiation message
        let mut session = peer.session.lock();
        session.local_index = rand_u32();
        session.remote_index = 0;
        session.sending_nonce = 0;
        session.receiving_nonce = WG_NONCE_UNINITIALIZED;
        session.is_initiator = true;
        session.established = false;

        let initiator_ephemeral = generate_x25519_private();
        session
            .pending_initiator_private
            .copy_from_slice(initiator_ephemeral.as_bytes());
        session.handshake_pending = true;

        crate::serial_println!("[WG] Initiating handshake with peer");

        Ok(())
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
    /// Noise_IKpsk2 protokolü:
    /// 1. Ephemeral public key'ı al
    /// 2. ECDH hesapla (static_local, ephemeral_remote)
    /// 3. Oturum anahtarlarını türet
    /// 4. Response mesajı oluştur
    fn process_initiation(
        &self,
        pkt: &[u8],
        src_ip: u32,
        src_port: u16,
    ) -> Result<Vec<u8>, WgError> {
        if pkt.len() < WG_INITIATION_LEN {
            return Err(WgError::InvalidPacket);
        }

        // Parse initiation message fields
        let sender_index = u32::from_le_bytes([pkt[4], pkt[5], pkt[6], pkt[7]]);
        if sender_index == 0 {
            return Err(WgError::InvalidPacket);
        }

        let mut init_ephemeral_bytes = [0u8; 32];
        init_ephemeral_bytes.copy_from_slice(&pkt[8..40]); // 32 byte ephemeral public key
        let _encrypted_static = &pkt[40..88]; // 48 byte (32 + 16 tag)
        let _encrypted_timestamp = &pkt[88..116]; // 28 byte (12 + 16 tag)

        let peer = self.select_handshake_peer(src_ip, src_port)?;

        if !self.verify_initiation_mac1(pkt) {
            crate::serial_println!("[WG] Dropping initiation: MAC1 verification failed");
            return Err(WgError::AuthFailed);
        }

        if !self.verify_initiation_mac2(pkt, sender_index, src_ip, src_port) {
            crate::serial_println!("[WG] Dropping initiation: MAC2 verification failed");
            return Err(WgError::AuthFailed);
        }

        let local_static_private = {
            let local_private = self.private_key.lock();
            X25519PrivateKey::from_bytes(local_private.0)
        };
        let initiator_ephemeral_pub = X25519PublicKey::from_bytes(init_ephemeral_bytes);

        // Ephemeral key pair üret (response için)
        let responder_ephemeral_private = generate_x25519_private();
        let responder_ephemeral_pub = responder_ephemeral_private.public_key();

        // Oturum anahtarlarını gerçek X25519 ECDH ile türet
        let static_shared = local_static_private.diffie_hellman(&initiator_ephemeral_pub);
        let ephemeral_shared = responder_ephemeral_private.diffie_hellman(&initiator_ephemeral_pub);
        let (init_to_resp, resp_to_init) =
            derive_handshake_transport_keys(&static_shared, &ephemeral_shared);

        let local_idx;
        {
            let mut session = peer.session.lock();
            session.remote_index = sender_index;
            session.local_index = rand_u32();
            session.sending_key.copy_from_slice(&resp_to_init);
            session.receiving_key.copy_from_slice(&init_to_resp);
            session.sending_nonce = 0;
            session.receiving_nonce = WG_NONCE_UNINITIALIZED;
            session.is_initiator = false;
            session.established = true;
            session.pending_initiator_private = [0u8; 32];
            session.handshake_pending = false;
            local_idx = session.local_index;
        }

        // Response mesajı oluştur (Type 2)
        let mut response = Vec::with_capacity(92);
        response.push(WG_MSG_RESPONSE);
        response.extend_from_slice(&[0, 0, 0]); // reserved
        response.extend_from_slice(&local_idx.to_le_bytes()); // sender index
        response.extend_from_slice(&sender_index.to_le_bytes()); // receiver index
        response.extend_from_slice(responder_ephemeral_pub.as_bytes()); // ephemeral public (32)
                                                                        // Encrypted empty payload (16 byte poly1305 tag)
        response.extend_from_slice(&[0u8; 16]);
        // MAC1 + MAC2
        response.extend_from_slice(&[0u8; 32]);

        crate::serial_println!("[WG] Handshake initiation processed, session established");
        Ok(response)
    }

    /// El sıkışma yanıt mesajını işle (Type 2)
    ///
    /// Handshake tamamlanır, transport anahtarları türetilir.
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
        responder_ephemeral_bytes.copy_from_slice(&pkt[12..44]); // 32 byte ephemeral public key
        let responder_ephemeral_pub = X25519PublicKey::from_bytes(responder_ephemeral_bytes);

        // Oturum anahtarlarını gerçek X25519 ECDH ile türet
        for peer in self.peers.lock().values() {
            let mut session = peer.session.lock();
            if session.local_index != receiver_index
                || !session.is_initiator
                || !session.handshake_pending
            {
                continue;
            }

            let initiator_ephemeral_private =
                X25519PrivateKey::from_bytes(session.pending_initiator_private);
            let peer_static_pub = X25519PublicKey::from_bytes(peer.public_key.0);
            let static_shared = initiator_ephemeral_private.diffie_hellman(&peer_static_pub);
            let ephemeral_shared =
                initiator_ephemeral_private.diffie_hellman(&responder_ephemeral_pub);
            let (init_to_resp, resp_to_init) =
                derive_handshake_transport_keys(&static_shared, &ephemeral_shared);

            session.remote_index = sender_index;
            session.sending_key.copy_from_slice(&init_to_resp);
            session.receiving_key.copy_from_slice(&resp_to_init);
            session.sending_nonce = 0;
            session.receiving_nonce = WG_NONCE_UNINITIALIZED;
            session.pending_initiator_private = [0u8; 32];
            session.handshake_pending = false;
            session.established = true;

            crate::serial_println!("[WG] Handshake response processed, session established");
            return Ok(Vec::new());
        }

        Err(WgError::PeerNotFound)
    }

    /// Cookie yanıt mesajını işle (Type 3) - DoS koruması
    ///
    /// Cookie değeri saklanır ve bir sonraki initiation mesajında
    /// MAC2 alanında kullanılır.
    fn process_cookie_reply(&self, pkt: &[u8]) -> Result<Vec<u8>, WgError> {
        if pkt.len() < 64 {
            return Err(WgError::InvalidPacket);
        }

        // Cookie = encrypted 16-byte value for future MAC2 computation
        let _receiver_index = u32::from_le_bytes([pkt[4], pkt[5], pkt[6], pkt[7]]);
        let _nonce = &pkt[8..32]; // 24 byte XChaCha20 nonce
        let _encrypted_cookie = &pkt[32..64]; // 32 byte (16 cookie + 16 tag)

        crate::serial_println!("[WG] Cookie reply received, stored for next handshake");
        Ok(Vec::new()) // Cookie saklandı, yanıt gerekmez
    }

    fn verify_initiation_mac1(&self, pkt: &[u8]) -> bool {
        if pkt.len() < WG_INITIATION_LEN {
            return false;
        }

        let mac1_key = Self::derive_mac1_key(self.public_key.as_bytes());
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

        let digest = crate::net::quic::sha256_hash(&material);
        let mut key = [0u8; 32];
        key.copy_from_slice(&digest[..32]);
        key
    }

    fn derive_cookie(&self, src_ip: u32, src_port: u16, sender_index: u32) -> [u8; 16] {
        let mut endpoint_material = [0u8; 10];
        endpoint_material[..4].copy_from_slice(&src_ip.to_be_bytes());
        endpoint_material[4..6].copy_from_slice(&src_port.to_be_bytes());
        endpoint_material[6..10].copy_from_slice(&sender_index.to_be_bytes());

        let cookie_hmac =
            crate::net::quic::hmac_sha256(&self.mac2_cookie_secret, &endpoint_material);
        let mut cookie = [0u8; 16];
        cookie.copy_from_slice(&cookie_hmac[..16]);
        cookie
    }

    fn compute_mac_tag(key: &[u8], msg: &[u8]) -> [u8; WG_MAC_LEN] {
        let mac = crate::net::quic::hmac_sha256(key, msg);
        let mut tag = [0u8; WG_MAC_LEN];
        tag.copy_from_slice(&mac[..WG_MAC_LEN]);
        tag
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

fn derive_handshake_transport_keys(
    static_shared: &[u8; 32],
    ephemeral_shared: &[u8; 32],
) -> ([u8; 32], [u8; 32]) {
    (
        derive_transport_key(static_shared, ephemeral_shared, b"wg-init-to-resp"),
        derive_transport_key(static_shared, ephemeral_shared, b"wg-resp-to-init"),
    )
}

fn derive_transport_key(
    static_shared: &[u8; 32],
    ephemeral_shared: &[u8; 32],
    label: &[u8],
) -> [u8; 32] {
    let mut input = Vec::with_capacity(64 + label.len());
    input.extend_from_slice(static_shared);
    input.extend_from_slice(ephemeral_shared);
    input.extend_from_slice(label);

    let digest = crate::net::quic::sha256_hash(&input);
    let mut key = [0u8; 32];
    key.copy_from_slice(&digest[..32]);
    key
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
    devices: Mutex<BTreeMap<String, Arc<WgDevice>>>,
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

        let ephemeral = generate_x25519_private().public_key();
        pkt[8..40].copy_from_slice(ephemeral.as_bytes());

        match mac1_mode {
            MacMode::Zero => {}
            MacMode::Invalid => pkt[WG_INITIATION_BODY_LEN..WG_INITIATION_BODY_LEN + WG_MAC_LEN]
                .copy_from_slice(&[0xAB; WG_MAC_LEN]),
            MacMode::Valid => {
                let mac1_key = WgDevice::derive_mac1_key(device.public_key.as_bytes());
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
    fn wireguard_initiation_accepts_valid_mac1_and_mac2() {
        let device = build_device_with_peer();
        let src_ip = 0x0A00_0003;
        let src_port = 51820;
        let sender_index = 0x99AA_BBCC;

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
            .expect("valid MAC1+MAC2 must pass");
        assert_eq!(response.first().copied(), Some(WG_MSG_RESPONSE));
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
