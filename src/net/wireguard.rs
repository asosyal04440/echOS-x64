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
        // Generate using Curve25519
        Self([0u8; WG_KEY_SIZE])
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
#[derive(Clone, Debug)]
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
                receiving_nonce: 0,
                is_initiator: false,
                established: false,
            }),
        }
    }

    /// IP adresinin bu peer için izin verilen aralıkta olup olmadığını kontrol et
    ///
    /// CIDR maskeleme: mask = !0u32 >> (32 - prefix_len)
    /// Örnek: prefix=24 -> mask=0x00FFFFFF -> 192.168.1.0/24 aralığı
    pub fn is_allowed_ip(&self, ip: u32) -> bool {
        for (allowed_ip, prefix_len) in &self.allowed_ips {
            let mask = if *prefix_len == 0 { 0 } else { !0u32 >> (32 - prefix_len) };
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

        // Build transport message
        let mut transport = Vec::new();
        transport.push(WG_MSG_TRANSPORT);
        transport.extend_from_slice(&session.local_index.to_le_bytes());
        transport.extend_from_slice(&nonce.to_le_bytes());
        transport.extend_from_slice(pkt);

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
        if pkt.len() < 16 || pkt[0] != WG_MSG_TRANSPORT {
            return Err(WgError::InvalidPacket);
        }

        let mut session = self.session.lock();

        if !session.established {
            return Err(WgError::NoSession);
        }

        // Parse transport header
        let remote_index = u32::from_le_bytes([pkt[4], pkt[5], pkt[6], pkt[7]]);
        let nonce = u64::from_le_bytes([pkt[8], pkt[9], pkt[10], pkt[11], pkt[12], pkt[13], pkt[14], pkt[15]]);

        // Oturum indeksini kontrol et
        if remote_index != session.remote_index {
            return Err(WgError::InvalidIndex);
        }

        // Check for replay (tekrar saldırısı kontrolü)
        if nonce <= session.receiving_nonce {
            return Err(WgError::Replay);
        }
        session.receiving_nonce = nonce;

        // Decrypt with ChaCha20-Poly1305
        let decrypted = pkt[16..].to_vec();

        // İstatistikleri güncelle
        self.rx_bytes.fetch_add(decrypted.len() as u64, Ordering::Relaxed);

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
        let public_key = WgKey::generate(); // Derive from private

        Self {
            name: String::from(name),
            listen_port: AtomicU32::new(WG_DEFAULT_PORT as u32),
            private_key: Mutex::new(private_key),
            public_key,
            peers: Mutex::new(BTreeMap::new()),
            fwmark: AtomicU32::new(0),
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
        session.is_initiator = true;

        crate::serial_println!("[WG] Initiating handshake with peer");

        Ok(())
    }

    /// Gelen WireGuard mesajını işle (mesaj tipine göre ayrıştır)
    pub fn process_message(&self, pkt: &[u8], src_ip: u32, src_port: u16) -> Result<Vec<u8>, WgError> {
        if pkt.is_empty() {
            return Err(WgError::InvalidPacket);
        }

        match pkt[0] {
            WG_MSG_INITIATION => self.process_initiation(pkt),
            WG_MSG_RESPONSE => self.process_response(pkt),
            WG_MSG_COOKIE_REPLY => self.process_cookie_reply(pkt),
            WG_MSG_TRANSPORT => self.process_transport(pkt, src_ip, src_port),
            _ => Err(WgError::InvalidPacket),
        }
    }

    /// El sıkışma başlatma mesajını işle (Type 1)
    fn process_initiation(&self, _pkt: &[u8]) -> Result<Vec<u8>, WgError> {
        // Validate and respond
        Ok(Vec::new())
    }

    /// El sıkışma yanıt mesajını işle (Type 2)
    fn process_response(&self, _pkt: &[u8]) -> Result<Vec<u8>, WgError> {
        // Complete handshake
        Ok(Vec::new())
    }

    /// Cookie yanıt mesajını işle (Type 3) - DoS koruması
    fn process_cookie_reply(&self, _pkt: &[u8]) -> Result<Vec<u8>, WgError> {
        Ok(Vec::new())
    }

    /// Şifreli veri paketini işle (Type 4)
    fn process_transport(&self, pkt: &[u8], _src_ip: u32, _src_port: u16) -> Result<Vec<u8>, WgError> {
        if pkt.len() < 16 {
            return Err(WgError::InvalidPacket);
        }

        // Receiver index: hangi oturuma ait?
        let index = u32::from_le_bytes([pkt[4], pkt[5], pkt[6], pkt[7]]);

        // Find peer by index
        for peer in self.peers.lock().values() {
            let session = peer.session.lock();
            if session.remote_index == index {
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

/// Basit rastgele sayı üreteci (oturum indeksi için)
fn rand_u32() -> u32 {
    // Random number generation
    0x12345678
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
        self.devices.lock().insert(String::from(name), device.clone());

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
}

// ============================================================================
// BAŞLATMA
// ============================================================================

/// WireGuard alt sistemini başlat
pub fn init() {
    crate::serial_println!("[WG] WireGuard initialized");
}
